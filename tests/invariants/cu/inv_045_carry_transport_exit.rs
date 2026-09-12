//! INV-045 / row 425: pending carry across authenticated matcher/bilateral handoffs.
//! Two-asset single/batch reductions retain input-derived owner entitlement through
//! later resolved payouts. Integral lots and zero fees/funding isolate route effects.

use super::*;

pub(super) fn profiles(world: &World) -> [state::AssetOracleProfileV16; 2] {
    let market = world.env.svm.get_account(&world.env.market).unwrap();
    [0, 1].map(|asset| state::read_asset_oracle_profile(&market.data, asset).unwrap())
}

fn cpi_reduce(
    world: &mut World,
    history: History,
    slot: u64,
    lots: i128,
    matcher: (Pubkey, Pubkey, Pubkey),
) {
    let (program, context, delegate) = matcher;
    world.trace.push(format!(
        "Clock.slot={}; frontier={slot}: owner 1 renews matcher delegation",
        world.env.svm.get_sysvar::<Clock>().slot
    ));
    let before = profiles(world);
    // Bilateral fills revoke the LP capability. Renewal is owner-signed and
    // uses the same System-created, publicly initialized matcher context.
    world.env.set_matcher_config_with_trade_fee_cap(
        program,
        &world.owners[1],
        world.portfolios[1],
        context,
        delegate,
        1,
        0,
    );
    world.check(history, slot);
    assert_eq!(profiles(world), before);

    let order = if history.reverse { [1, 0] } else { [0, 1] };
    let legs: Vec<_> = order
        .into_iter()
        .map(|asset| BatchTradeCpiLeg {
            asset_index: asset,
            market_id: world.env.asset_market_id(asset),
            size_q: -lots * POS_SCALE as i128,
            fee_bps: 0,
            limit_price: world.expected.price[asset as usize],
        })
        .collect();
    let chunks = if history.batch {
        vec![legs]
    } else {
        legs.into_iter().map(|leg| vec![leg]).collect()
    };
    for legs in chunks {
        world.trace.push(format!(
            "frontier={slot}: {}(actors=0/1, legs={legs:?})",
            if history.batch {
                "BatchTradeCpi"
            } else {
                "TradeCpi"
            }
        ));
        let passive = [2, 3].map(|actor| world.env.svm.get_account(&world.portfolios[actor]));
        world.env.svm.expire_blockhash();
        let result = if history.batch {
            world.env.send(
                world.env.batch_trade_cpi_ix(
                    world.portfolios[0],
                    world.portfolios[1],
                    legs.clone(),
                ),
                vec![
                    AccountMeta::new(world.owners[0].pubkey(), true),
                    AccountMeta::new(world.env.market, false),
                    AccountMeta::new(world.portfolios[0], false),
                    AccountMeta::new(world.portfolios[1], false),
                    AccountMeta::new_readonly(program, false),
                    AccountMeta::new(context, false),
                    AccountMeta::new_readonly(delegate, false),
                ],
                &[&world.owners[0]],
            )
        } else {
            let leg = &legs[0];
            world.env.try_trade_cpi_with_cu_on_asset(
                &world.owners[0],
                world.portfolios[0],
                &world.owners[1],
                world.portfolios[1],
                program,
                context,
                delegate,
                leg.asset_index,
                leg.size_q,
                0,
            )
        };
        world.max_cu = world
            .max_cu
            .max(result.unwrap_or_else(|error| panic!("{error}: {:?}", world.trace)));
        for leg in legs {
            world.expected.lots[0][leg.asset_index as usize] -= lots;
            world.expected.lots[1][leg.asset_index as usize] += lots;
        }
        world.check(history, slot);
        assert_eq!(profiles(world), before, "{:?}", world.trace);
        assert_eq!(
            [2, 3].map(|actor| world.env.svm.get_account(&world.portfolios[actor])),
            passive,
            "matcher reductions frame absent owners: {:?}",
            world.trace
        );
    }
}

fn pay_resolved(world: &mut World, endpoint: &Economics, reverse: bool) -> [u64; 4] {
    pay_resolved_with_residue(world, endpoint, reverse, 100, 0, |_, _, _| {})
}

pub(super) fn pay_resolved_with_residue(
    world: &mut World,
    endpoint: &Economics,
    reverse: bool,
    payout_slot: u64,
    settlement_rounding_residue: u128,
    mut before_close: impl FnMut(&mut World, usize, &Instruction),
) -> [u64; 4] {
    world.trace.push(format!(
        "ResolveMarket at slot 5; CloseResolved at slot {payout_slot}"
    ));
    let env = &mut world.env;
    let cu = env
        .send(
            ProgInstruction::ResolveMarket {
                asset_generation_frontier: env.market_state().1.next_market_id,
                authority_epoch: env.control_sequences(0).authority_epoch,
            },
            vec![
                AccountMeta::new(env.admin.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&env.admin.insecure_clone()],
        )
        .unwrap();
    world.max_cu = world.max_cu.max(cu);
    let check = |world: &World| {
        let group = world.env.market_state().1;
        assert_eq!(group.mode, MarketModeV16::Resolved);
        assert_eq!(group.resolved_slot, 5);
        assert_eq!(
            [0, 1].map(|asset| group.assets[asset].effective_price),
            endpoint.price
        );
        assert_eq!(
            profiles(world).map(|p| u64::from(p.price_move_remainder_bps_num)),
            endpoint.carry
        );
        let paid = world.tokens.map(|key| world.env.token_amount(key));
        assert_eq!(group.vault, world.env.token_amount(world.env.vault) as u128);
        assert_eq!(
            group.vault + paid.map(u128::from).iter().sum::<u128>(),
            endpoint.vault
        );
        assert_eq!(
            Mint::unpack(&world.env.svm.get_account(&world.env.mint).unwrap().data)
                .unwrap()
                .supply as u128,
            endpoint.vault
        );
        assert_eq!(group.insurance, 0);
        assert_eq!(
            group.c_tot,
            world
                .portfolios
                .iter()
                .map(|key| world.env.portfolio_state(*key).capital.get())
                .sum::<u128>()
        );
        for actor in 0..4 {
            assert!(
                i128::from(paid[actor]) <= endpoint.entitlement[actor],
                "{:?}",
                world.trace
            );
        }
        paid
    };
    check(world);
    world.env.svm.warp_to_slot(payout_slot);
    let keys: Vec<_> = [world.env.market, world.env.mint, world.env.vault]
        .into_iter()
        .chain(world.portfolios)
        .chain(world.tokens)
        .collect();
    let frame = |world: &World| {
        keys.iter()
            .map(|key| world.env.svm.get_account(key))
            .collect::<Vec<_>>()
    };
    for round in 0..16 {
        if world
            .portfolios
            .iter()
            .all(|key| resolved_portfolio_is_terminal(&world.env, *key))
        {
            break;
        }
        for actor in if reverse { [3, 2, 1, 0] } else { [0, 1, 2, 3] } {
            if resolved_portfolio_is_terminal(&world.env, world.portfolios[actor]) {
                continue;
            }
            world
                .trace
                .push(format!("CloseResolved(actor={actor}, round={round})"));
            let before = frame(world);
            world.env.svm.expire_blockhash();
            let ix = Instruction {
                program_id: world.env.program_id,
                data: ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                }
                .encode(),
                accounts: vec![
                    AccountMeta::new_readonly(world.owners[actor].pubkey(), true),
                    AccountMeta::new(world.env.market, false),
                    AccountMeta::new(world.portfolios[actor], false),
                    AccountMeta::new(world.tokens[actor], false),
                    AccountMeta::new(world.env.vault, false),
                    AccountMeta::new_readonly(world.env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
            };
            before_close(world, actor, &ix);
            let result = send_raw_tx(
                &mut world.env.svm,
                &world.env.payer,
                ix,
                &[&world.owners[actor]],
            );
            match result {
                Ok(cu) => {
                    world.max_cu = world.max_cu.max(cu);
                    assert_ne!(frame(world), before, "accepted close must progress");
                }
                Err(error) => {
                    assert!(
                        is_engine_non_progress_error(&error),
                        "{error}: {:?}",
                        world.trace
                    );
                    assert_eq!(frame(world), before, "non-progress close rollback");
                }
            }
            check(world);
        }
    }
    assert!(world
        .portfolios
        .iter()
        .all(|key| resolved_portfolio_is_terminal(&world.env, *key)));
    let paid = check(world);
    assert_eq!(
        paid.map(i128::from),
        endpoint.entitlement,
        "{:?}",
        world.trace
    );
    let group = world.env.market_state().1;
    assert_eq!(
        (group.vault, group.c_tot, group.pnl_pos_tot),
        (settlement_rounding_residue, 0, 0)
    );
    for asset in 0..2 {
        assert_eq!(
            (
                group.assets[asset].oi_eff_long_q,
                group.assets[asset].oi_eff_short_q
            ),
            (0, 0)
        );
    }
    paid
}

#[test]
fn v16_program_row425_matcher_handoffs_preserve_precrank_carry_and_exact_owner_exit() {
    let mut worlds = 0;
    let mut max_cu = 0;
    for direction in [-1, 1] {
        for early_parity in 0..2 {
            let mut baseline = None;
            for batch in [false, true] {
                // The bilateral control and both phases of an alternating CPI/bilateral
                // word share exactly the quantities held at each committed price atom.
                for cpi_phase in [None, Some(0), Some(1)] {
                    let history = History {
                        direction,
                        batch,
                        split: true,
                        placement: early_parity,
                        reverse: cpi_phase == Some(1),
                    };
                    let mut world = World::new(history);
                    world.trace.push(format!(
                        "early_parity={early_parity}, cpi_phase={cpi_phase:?}"
                    ));
                    world.check(history, 0);
                    let matcher = cpi_phase.map(|_| {
                        auth_matcher_for_lp_via_system_create(
                            &mut world.env,
                            &world.owners[1],
                            world.portfolios[1],
                        )
                    });
                    world.check(history, 0);
                    let mut reduction = 0;
                    let mut pending_cpi = 0;
                    for slot in 1..=5 {
                        world.env.svm.warp_to_slot(slot);
                        let early = usize::from(slot as usize % 2 == early_parity);
                        for phase in 0..2 {
                            if phase == 1 {
                                world.crank(2, history, slot);
                            }
                            let lots = if phase == 0 { early } else { 2 - early } as i128;
                            if lots == 0 {
                                continue;
                            }
                            let frontier = if phase == 0 { slot - 1 } else { slot };
                            let before = profiles(&world);
                            if cpi_phase == Some(reduction % 2) {
                                pending_cpi += usize::from(
                                    phase == 0
                                        && before
                                            .iter()
                                            .all(|p| p.price_move_remainder_bps_num != 0),
                                );
                                cpi_reduce(&mut world, history, frontier, lots, matcher.unwrap());
                            } else {
                                world.reduce(history, frontier, lots);
                            }
                            assert_eq!(profiles(&world), before, "{:?}", world.trace);
                            reduction += 1;
                        }
                        for actor in if history.reverse {
                            [1, 0, 3]
                        } else {
                            [0, 1, 3]
                        } {
                            world.crank(actor, history, slot);
                        }
                    }
                    if cpi_phase.is_some() {
                        assert!(
                            pending_cpi > 0,
                            "CPI must execute before accrual with both carries nonzero"
                        );
                    }
                    assert!(world.latent_checks > 0);
                    let endpoint = world.check(history, 5);
                    let active_pnl = direction * (-6 + if early_parity == 0 { 1 } else { -1 });
                    assert_eq!(endpoint.carry, [2_000, 5_000]);
                    assert_eq!(
                        endpoint.entitlement,
                        [
                            PRINCIPAL[0] as i128 + active_pnl,
                            PRINCIPAL[1] as i128 - active_pnl,
                            PRINCIPAL[2] as i128 - 4 * direction,
                            PRINCIPAL[3] as i128 + 4 * direction,
                        ]
                    );
                    let paid = pay_resolved(&mut world, &endpoint, history.reverse);
                    let outcome = (endpoint, paid);
                    if let Some(expected) = &baseline {
                        assert_eq!(&outcome, expected, "{:?}", world.trace);
                    } else {
                        baseline = Some(outcome);
                    }
                    max_cu = max_cu.max(world.max_cu);
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!(worlds, 24);
    assert_cu_within(
        "row425 matcher handoff and exact owner exit",
        max_cu,
        1_400_000,
    );
    eprintln!("row425 matcher handoff: {worlds} histories, max transition CU={max_cu}");
}
