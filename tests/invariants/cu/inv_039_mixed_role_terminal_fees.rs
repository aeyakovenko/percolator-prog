//! INV-039/048/081, rows 419/435: maintenance fees and source conversion must
//! preserve a pending creditor's separate cross-asset debt through resolution.
//! The existing terminal-fee world keeps creditor/debtor roles separate, while
//! mixed-role resolution worlds disable maintenance. Here settling the original
//! debtor first funds source support for the mixed owner's debt; the reverse order
//! nets that debt against unbacked claim face before backing arrives. Both routes
//! owe the same owner entitlements after fees, with distinct backing consumption.
//! Regression: the mixed-first route currently freezes a zero-residual payout
//! snapshot, clears the peer's unpaid 36,000 receipt, and leaves that value in
//! the vault. The debtor-first controls pay the peer fully. Keep this assertion
//! strict: treating that unpaid receipt as dilution would bless lost attribution.
//! These finite solvent histories leave bankruptcy and generic INV-086 open.

use super::*;

const RATE: u128 = 7;
const RESOLVED: u64 = 10;
const DEBT: u128 = 36_000;
const CAPITAL: [u128; 5] = [400_000, 250_000, 300_000, 250_000, 777];

fn setup(reverse: bool) -> AttributionWorld {
    let mut world = AttributionWorld::new_with_deposits(
        reverse,
        V16CuMarketParams {
            max_portfolio_assets: 3,
            initial_price: 1_000_000,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_abs_funding_e9_per_slot: 0,
            maintenance_fee_per_slot: RATE,
            ..V16CuMarketParams::default()
        },
        CAPITAL,
    );
    let sign = if reverse { -1 } else { 1 };
    for (asset, holder, debtor, lots) in [(1, 0, 1, 1), (2, 2, 0, 2)] {
        world.env.trade_asset_with_cu(
            asset,
            &world.actors[holder].owner,
            world.actors[holder].portfolio,
            &world.actors[debtor].owner,
            world.actors[debtor].portfolio,
            lots * POS_SCALE as i128 * sign,
            1_000_000,
            0,
        );
    }
    for (pair, movement) in [GAIN, DEBT / 2].into_iter().enumerate() {
        for step in 1..=5 {
            let slot = pair as u64 * 5 + step;
            world.env.svm.warp_to_slot(slot);
            world.env.push_auth_mark_for_asset_as_admin(
                (pair + 1) as u16,
                slot,
                (1_000_000 + movement as i128 * step as i128 / 5 * sign) as u64,
            );
            world.env.crank(
                world.actors[4].portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: if pair == 0 {
                        crank_observations_for_assets(&[1, 2])
                    } else {
                        crank_observations(2)
                    },
                },
            );
        }
        if pair == 0 {
            // Materialize the creditor claim before forfeiting exposure; otherwise
            // ForfeitRecoveryLeg also waives the still-unsettled positive K.
            world.env.crank(
                world.actors[0].portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: 5,
                    observations: crank_observations_for_assets(&[1, 2]),
                },
            );
            world.env.update_asset_lifecycle_as_admin_with_cu(
                processor::ASSET_ACTION_SHUTDOWN,
                1,
                5,
                0,
            );
            world.env.forfeit_recovery_leg_with_cu(
                &world.actors[0].owner,
                world.actors[0].portfolio,
                1,
                u128::MAX,
            );
        } else {
            world.env.crank(
                world.actors[2].portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: RESOLVED,
                    observations: crank_observations(2),
                },
            );
        }
    }
    world
}

struct FeeBook {
    slots: [u64; 5],
    budgets: [u128; 2],
    original_settled: bool,
    mixed_settled: bool,
    support: u128,
}

impl FeeBook {
    fn check(&mut self, world: &AttributionWorld, reverse: bool) {
        let env = &world.env;
        let group = env.market_state().1;
        let accounts: Vec<_> = world
            .actors
            .iter()
            .map(|a| env.portfolio_state(a.portfolio))
            .collect();
        let side = usize::from(reverse);
        let leg = |actor: usize, asset: u32| {
            accounts[actor]
                .legs
                .iter()
                .map(|l| l.try_to_runtime().unwrap())
                .find(|l| l.active && l.asset_index == asset)
        };
        let original = leg(1, 1);
        let mixed = leg(0, 2);
        let original_settled =
            original.is_none_or(|l| l.k_snap == -(GAIN as i128) * ADL_ONE as i128);
        let mixed_settled = mixed.is_none_or(|l| l.k_snap == -(DEBT as i128 / 2) * ADL_ONE as i128);
        assert!(!self.original_settled || original_settled);
        assert!(!self.mixed_settled || mixed_settled);
        if mixed_settled && !self.mixed_settled {
            self.support = if self.original_settled { DEBT } else { 0 };
        }
        self.original_settled = original_settled;
        self.mixed_settled = mixed_settled;

        let mut oi = [[0u128; 2]; 3];
        let mut weights = oi;
        let mut counts = [[0u64; 2]; 3];
        let mut pending = counts;
        let mut paid_total = 0;
        for (i, account) in accounts.iter().enumerate() {
            assert_eq!(account.owner, world.actors[i].owner.pubkey().to_bytes());
            assert_eq!(close_progress(account), CloseProgressLedgerV16::default());
            for l in account
                .legs
                .iter()
                .map(|l| l.try_to_runtime().unwrap())
                .filter(|l| l.active)
            {
                let asset = l.asset_index as usize;
                let leg_side = usize::from(l.side == SideV16::Short);
                let expected_side = match (i, asset) {
                    (0, 1) | (2, 2) => side,
                    (1, 1) | (0, 2) => 1 - side,
                    _ => panic!("foreign debt/weight owner {i} asset {asset}"),
                };
                assert_eq!(leg_side, expected_side);
                assert_eq!(
                    l.loss_weight,
                    if asset == 1 { POS_SCALE } else { 2 * POS_SCALE }
                );
                assert_eq!((l.b_snap, l.b_rem), (0, 0));
                if (i, asset) == (0, 1) {
                    assert_eq!(
                        l.basis_pos_q, 0,
                        "creditor retains only pending loss weight"
                    );
                }
                oi[asset][leg_side] += l.basis_pos_q.unsigned_abs();
                weights[asset][leg_side] += l.loss_weight;
                counts[asset][leg_side] += 1;
                pending[asset][leg_side] += u64::from(l.basis_pos_q == 0);
            }
            for source in account.source_domains.iter().filter(|s| s.is_occupied()) {
                assert!(i == 0 || i == 2);
                assert_eq!(
                    source.domain.get() as usize,
                    if i == 0 { 3 - side } else { 5 - side }
                );
            }
            let slot = account.last_fee_slot.get();
            assert!(slot == self.slots[i] || slot == RESOLVED);
            let fee = RATE * u128::from(slot - self.slots[i]);
            self.budgets[0] += fee / 2;
            self.budgets[1] += fee - fee / 2;
            self.slots[i] = slot;
            let paid = env.token_amount(world.actors[i].token) as u128;
            paid_total += paid;
            let receipt = resolved_receipt(account);
            let due = if receipt.present {
                assert_eq!(i, 2);
                assert!(original_settled && mixed_settled);
                assert_eq!(receipt.terminal_positive_claim_face, DEBT);
                assert_eq!(receipt.prior_bound_contribution_num, DEBT * BOUND_SCALE);
                DEBT.checked_sub(receipt.paid_effective).unwrap()
            } else {
                0
            };
            let pnl = match i {
                0 => GAIN as i128 - if mixed_settled { DEBT as i128 } else { 0 },
                1 => {
                    if original_settled {
                        -(GAIN as i128)
                    } else {
                        0
                    }
                }
                2 => DEBT as i128,
                _ => 0,
            };
            let expected = CAPITAL[i] as i128 + pnl - (RATE * u128::from(slot)) as i128;
            assert_eq!(
                account.capital.get() as i128 + account.pnl.get() + due as i128 + paid as i128,
                expected,
                "owner {i}: fees cannot replace or transfer debt; original_settled={original_settled}, mixed_settled={mixed_settled}, support={}, vault={}, insurance={}, receipt={receipt:?}, ledger={:?}",
                self.support, group.vault, group.insurance, group.resolved_payout_ledger
            );
            if i == 0 && (!original_settled || !mixed_settled) {
                assert_eq!(paid, 0, "pending debt cannot authorize a creditor payout");
                assert!(!receipt.present);
            }
        }
        for asset in 0..3 {
            let a = group.assets[asset];
            assert_eq!([a.oi_eff_long_q, a.oi_eff_short_q], oi[asset]);
            assert_eq!(
                [a.loss_weight_sum_long, a.loss_weight_sum_short],
                weights[asset]
            );
            assert_eq!(
                [a.stored_pos_count_long, a.stored_pos_count_short],
                counts[asset]
            );
            assert_eq!(
                [
                    a.pending_obligation_count_long,
                    a.pending_obligation_count_short
                ],
                pending[asset]
            );
            assert_eq!([a.b_long_num, a.b_short_num], [0; 2]);
        }
        let source = group.source_credit[3 - side];
        let converted = source.positive_claim_bound_num == 0;
        assert!(!converted || (original_settled && mixed_settled));
        assert_eq!(
            source.positive_claim_bound_num,
            if converted {
                0
            } else {
                (GAIN - if mixed_settled { DEBT } else { 0 }) * BOUND_SCALE
            }
        );
        assert_eq!(
            source.fresh_reserved_backing_num,
            if original_settled {
                source.positive_claim_bound_num
            } else {
                0
            },
            "settled resolved fresh backing must equal remaining source-claim capacity"
        );
        assert_eq!(group.insurance, self.budgets.iter().sum());
        assert_eq!(group.insurance_domain_budget[..2], self.budgets);
        assert!(group.insurance_domain_budget[2..].iter().all(|x| *x == 0));
        assert!(group.insurance_domain_spent.iter().all(|x| *x == 0));
        assert_eq!(
            env.token_amount(env.vault) as u128 + paid_total,
            CAPITAL.iter().sum()
        );
        assert_market_stock_census(
            "mixed-role maintenance",
            &group,
            &env.svm.get_account(&env.market).unwrap().data,
            &accounts,
            env.token_amount(env.vault) as u128,
        )
        .unwrap();
        assert_reservation_encumbrance_census("mixed-role maintenance", &group, &accounts).unwrap();
    }
}

#[test]
fn v16_program_mixed_role_terminal_fees_preserve_pending_debt_and_source_conversion() {
    assert_certified_engine_pin("INV-039 mixed-role maintenance");
    let mut worlds = 0;
    let mut receipts = 0;
    let mut peak = 0;
    // Run all funded controls before the unbacked-netting regression.
    for order in [[1, 0, 2, 4, 3], [0, 1, 2, 3, 4]] {
        for reverse in [false, true] {
            for delay in [0, 40] {
                let mut world = setup(reverse);
                let mut book = FeeBook {
                    slots: [5, 0, 10, 0, 0],
                    budgets: [52, 53],
                    original_settled: false,
                    mixed_settled: false,
                    support: 0,
                };
                book.check(&world, reverse);
                assert!(!book.original_settled && !book.mixed_settled);
                let holder = world.env.portfolio_state(world.actors[0].portfolio);
                let pending = active_leg_for_asset(&holder, 1);
                assert_eq!((pending.basis_pos_q, pending.loss_weight), (0, POS_SCALE));
                assert_eq!(active_leg_for_asset(&holder, 2).k_snap, 0);
                let before = world.frame();
                world.env.resolve();
                for (key, account) in before {
                    if key != world.env.market {
                        assert_eq!(world.env.svm.get_account(&key), account);
                    }
                }
                assert_eq!(world.env.market_state().1.resolved_slot, RESOLVED);
                world.env.svm.warp_to_slot(RESOLVED + 5 + delay);
                book.check(&world, reverse);

                let settle = payout(&world, 0, false);
                let denied = deletion(&world, 0, false);
                peak = peak.max(land(&mut world, &[settle, denied], &[], &[], Some(1)));
                book.check(&world, reverse);
                assert!(!book.original_settled && !book.mixed_settled);
                assert_eq!(book.slots[0], 5, "fee anchor also rolls back");

                for round in 0..16 {
                    let round_before = world.frame();
                    for actor in order {
                        if resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio)
                        {
                            continue;
                        }
                        let before = world.frame();
                        match world.payout(actor, false) {
                            Ok(cu) => {
                                assert_cu_within(
                                    "mixed-role maintenance close",
                                    cu,
                                    CUSTODY_CU_LIMIT,
                                );
                                peak = peak.max(cu);
                                assert_ne!(world.frame(), before);
                            }
                            Err(error) => {
                                assert!(is_engine_non_progress_error(&error), "{error}");
                                assert_eq!(world.frame(), before);
                            }
                        }
                        for (key, account) in before {
                            if ![
                                world.env.market,
                                world.env.vault,
                                world.actors[actor].portfolio,
                                world.actors[actor].token,
                            ]
                            .contains(&key)
                            {
                                assert_eq!(world.env.svm.get_account(&key), account);
                            }
                        }
                        book.check(&world, reverse);
                    }
                    if world
                        .actors
                        .iter()
                        .all(|a| resolved_portfolio_is_terminal(&world.env, a.portfolio))
                    {
                        break;
                    }
                    assert_ne!(
                        world.frame(),
                        round_before,
                        "bounded progress round {round}"
                    );
                }
                assert!(book.original_settled && book.mixed_settled);
                assert_eq!(book.support, if order[0] == 1 { DEBT } else { 0 });
                assert_eq!(book.slots, [RESOLVED; 5]);
                assert_eq!(book.budgets, [174, 176]);
                let expected = [563_930, 49_930, 335_930, 249_930, 707];
                for (i, amount) in expected.into_iter().enumerate() {
                    assert!(resolved_portfolio_is_terminal(
                        &world.env,
                        world.actors[i].portfolio
                    ));
                    assert_eq!(world.env.token_amount(world.actors[i].token), amount);
                    let receipt =
                        resolved_receipt(&world.env.portfolio_state(world.actors[i].portfolio));
                    assert_eq!(receipt.present, i == 2);
                    if receipt.present {
                        let before = world.frame();
                        let cu = world.payout(i, true).expect("fully paid receipt retry");
                        assert_cu_within("mixed-role fee receipt retry", cu, CUSTODY_CU_LIMIT);
                        peak = peak.max(cu);
                        assert_eq!(
                            world.frame(),
                            before,
                            "retry collects neither fee nor debt twice"
                        );
                        receipts += 1;
                    }
                }
                book.check(&world, reverse);
                let group = world.env.market_state().1;
                assert_eq!(
                    (group.vault, group.c_tot, group.pnl_pos_tot, group.insurance),
                    (350, 0, 0, 350)
                );
                worlds += 1;
                println!("mixed-role maintenance passed: reverse={reverse}, order={order:?}, delay={delay}");
            }
        }
    }
    assert_eq!((worlds, receipts), (8, 8));
    println!("INV-039 mixed-role maintenance: {worlds} worlds, fee/debt rollback prefixes, {receipts} receipt retries; peak CU={peak}");
}
