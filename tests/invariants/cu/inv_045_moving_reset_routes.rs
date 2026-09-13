//! Scope C / row 425: repeated nonzero carry resets after K and anchor movement.
//! Public AuthMark histories; integral aligned lots, zero funding/fees, unit ADL.

use super::*;

fn route_reduce(
    world: &mut World,
    book: &mut Book,
    matcher: (Pubkey, Pubkey, Pubkey),
    batch: bool,
    cpi: bool,
    reverse: bool,
) {
    let before = profiles(world);
    if cpi {
        world.env.set_matcher_config_with_trade_fee_cap(
            matcher.0,
            &world.owners[1],
            world.portfolios[1],
            matcher.1,
            matcher.2,
            1,
            0,
        );
        book.check(world);
        assert_eq!(profiles(world), before);
    }
    let absent = [2, 3].map(|actor| world.env.svm.get_account(&world.portfolios[actor]));
    let order = if reverse { [1, 0] } else { [0, 1] };
    let chunks = if batch {
        vec![order.to_vec()]
    } else {
        order.map(|asset| vec![asset]).to_vec()
    };
    for assets in chunks {
        let env = &world.env;
        let [a, b, _, _] = world.portfolios;
        let legs: Vec<_> = assets
            .iter()
            .map(|&asset| BatchTradeLeg {
                asset_index: asset as u16,
                market_id: env.asset_market_id(asset as u16),
                size_q: -book.economics.lots[0][asset].signum() * POS_SCALE as i128,
                exec_price: book.economics.price[asset],
                fee_bps: 0,
            })
            .collect();
        let first = &legs[0];
        let data = match (batch, cpi) {
            (true, true) => env.batch_trade_cpi_ix(
                a,
                b,
                legs.iter()
                    .map(|leg| BatchTradeCpiLeg {
                        asset_index: leg.asset_index,
                        market_id: leg.market_id,
                        size_q: leg.size_q,
                        fee_bps: 0,
                        limit_price: leg.exec_price,
                    })
                    .collect(),
            ),
            (true, false) => env.batch_trade_no_cpi_ix(a, b, legs.clone()),
            (false, true) => {
                env.trade_cpi_ix(a, b, first.asset_index, first.size_q, 0, first.exec_price)
            }
            (false, false) => {
                env.trade_no_cpi_ix(a, b, first.asset_index, first.size_q, first.exec_price, 0)
            }
        };
        let mut accounts = vec![AccountMeta::new_readonly(world.owners[0].pubkey(), true)];
        if !cpi {
            accounts.push(AccountMeta::new_readonly(world.owners[1].pubkey(), true));
        }
        accounts.extend([
            AccountMeta::new(env.market, false),
            AccountMeta::new(a, false),
            AccountMeta::new(b, false),
        ]);
        if cpi {
            accounts.extend([
                AccountMeta::new_readonly(matcher.0, false),
                AccountMeta::new(matcher.1, false),
                AccountMeta::new_readonly(matcher.2, false),
            ]);
        }
        let ix = Instruction {
            program_id: env.program_id,
            accounts,
            data: data.encode(),
        };
        world.trace.push(format!(
            "slot={}: moving reset reduction {assets:?}, batch={batch}, cpi={cpi}",
            book.slot
        ));
        execute(world, book, ix, if cpi { &[0] } else { &[0, 1] }, true);
        for leg in legs {
            let asset = leg.asset_index as usize;
            book.economics.lots[0][asset] += leg.size_q / POS_SCALE as i128;
            book.economics.lots[1][asset] -= leg.size_q / POS_SCALE as i128;
        }
        book.check(world);
        assert_eq!(profiles(world), before);
        assert_eq!(
            [2, 3].map(|actor| world.env.svm.get_account(&world.portfolios[actor])),
            absent
        );
    }
}

#[test]
fn v16_program_repeated_moving_target_resets_preserve_carry_across_matcher_handoffs() {
    let mut peak = 0;
    let mut checks = 0;
    let mut rollbacks = 0;
    let mut resets = 0;
    for direction in [-1, 1] {
        let mut reference = None;
        for batch in [false, true] {
            for handoff in [false, true] {
                for fine in [false, true] {
                    let history = History {
                        direction,
                        batch,
                        split: false,
                        placement: 0,
                        reverse: fine,
                    };
                    let mut world = World::with_funding_and_accrual_limit(history, 0, 4);
                    for actor in [0, 2] {
                        world.env.svm.expire_blockhash();
                        world.env.trade_asset_with_cu(
                            1,
                            &world.owners[actor],
                            world.portfolios[actor],
                            &world.owners[actor + 1],
                            world.portfolios[actor + 1],
                            -2 * OPEN_LOTS[actor][1] * POS_SCALE as i128,
                            ANCHORS[1],
                            0,
                        );
                    }
                    send_raw_tx(
                        &mut world.env.svm,
                        &world.env.payer,
                        spl_token::instruction::set_authority(
                            &spl_token::ID,
                            &world.env.mint,
                            None,
                            spl_token::instruction::AuthorityType::MintTokens,
                            &world.env.admin.pubkey(),
                            &[],
                        )
                        .unwrap(),
                        &[&world.env.admin],
                    )
                    .unwrap();
                    let matcher = auth_matcher_for_lp_via_system_create(
                        &mut world.env,
                        &world.owners[1],
                        world.portfolios[1],
                    );
                    let mut book = Book::new(direction);
                    let passive = world.env.svm.get_account(&world.portfolios[3]);
                    book.check(&world);
                    // Reach a nearby target first, establishing noninitial cap anchors.
                    for asset in 0..2 {
                        let sign = direction * if asset == 0 { 1 } else { -1 };
                        publish(
                            &mut world,
                            &mut book,
                            asset,
                            (ANCHORS[asset] as i128 + sign) as u64,
                            false,
                        );
                    }
                    for endpoint in [5, 10, 15, 20] {
                        let previous = book.economics.price;
                        while book.slot < endpoint {
                            let next = (book.slot + if fine { 1 } else { 4 }).min(endpoint);
                            crank(&mut world, &mut book, 2, next, fine);
                        }
                        assert!(book
                            .economics
                            .price
                            .iter()
                            .zip(previous)
                            .all(|(price, old)| *price != old));
                        if endpoint == 5 {
                            assert_eq!(book.economics.price, book.target);
                            assert_eq!(book.economics.carry, [0; 2]);
                            assert_ne!(book.anchor, ANCHORS);
                        } else {
                            assert!(book.economics.carry.iter().all(|carry| *carry != 0));
                            resets += 2;
                        }
                        let cpi = handoff && matches!(endpoint, 10 | 20);
                        if fine {
                            route_reduce(&mut world, &mut book, matcher, batch, cpi, fine);
                        }
                        for asset in if fine { [1, 0] } else { [0, 1] } {
                            let sign = direction * if asset == 0 { 1 } else { -1 };
                            let target = (book.economics.price[asset] as i128
                                + sign * (20 + endpoint as i128))
                                as u64;
                            publish(&mut world, &mut book, asset, target, true);
                        }
                        assert_eq!(book.economics.carry, [0; 2]);
                        if !fine {
                            route_reduce(&mut world, &mut book, matcher, batch, cpi, fine);
                        }
                    }
                    while book.slot < 25 {
                        let next = (book.slot + if fine { 1 } else { 4 }).min(25);
                        crank(&mut world, &mut book, 2, next, fine);
                    }
                    assert!(book.economics.carry.iter().all(|carry| *carry != 0));
                    assert!(book.latent_checks > 0);
                    assert_eq!(world.env.svm.get_account(&world.portfolios[3]), passive);
                    let paid = pay_resolved_with_residue(
                        &mut world,
                        &book.economics,
                        fine,
                        100,
                        0,
                        |_, _, _| {},
                    );
                    let outcome = (book.economics.clone(), paid);
                    if let Some(reference) = &reference {
                        assert_eq!(&outcome, reference);
                    } else {
                        reference = Some(outcome);
                    }
                    peak = peak.max(world.max_cu);
                    checks += book.checks;
                    rollbacks += book.rollbacks;
                }
            }
        }
    }
    assert_eq!(resets, 96);
    assert_eq!(rollbacks, 224);
    assert_cu_within("moving carry reset routes", peak, 600_000);
    eprintln!("Scope C carry: 16 worlds, {resets} nonzero resets, {checks} owner checks, {rollbacks} exact rollbacks, 64 SPL payouts, peak {peak} CU");
}
