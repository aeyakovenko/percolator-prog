//! INV-066 - Resolved-payout fairness and order independence.
//!
//! Normative obligation: Resolved entitlement is snapshot-bound and claimant-order independent except explicit residue.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_attack_resolved_close_order_preserves_scarce_source_backing`, `v16_bpf_force_close_pair_order_preserves_terminal_user_payouts`, `v16_attack_resolved_two_public_winners_are_close_order_independent`, `v16_bpf_force_close_pair_order_preserves_unequal_partial_payouts`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_attack_resolved_close_order_preserves_scarce_source_backing() {
    fn run(reverse: bool) -> ([u128; 4], u128, u128, u128, u128, u128) {
        const OPEN_PRICE: u64 = 100;
        const FROZEN_PRICE: u64 = 300;
        const BACKING: u128 = 50;

        let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
        env.configure_permissionless_resolve_with_cu(100, 1);
        env.svm.warp_to_slot(1);
        env.configure_auth_mark_for_asset_as_admin(0, 1, OPEN_PRICE);
        env.top_up_backing_bucket(1, BACKING, 100);

        let owners = [
            Keypair::new(),
            Keypair::new(),
            Keypair::new(),
            Keypair::new(),
        ];
        let portfolios = owners.each_ref().map(|owner| env.create_portfolio(owner));
        for i in 0..4 {
            env.deposit(&owners[i], portfolios[i], 150);
        }
        env.trade_asset_with_cu(
            0,
            &owners[0],
            portfolios[0],
            &owners[2],
            portfolios[2],
            POS_SCALE as i128,
            OPEN_PRICE,
            0,
        );
        env.trade_asset_with_cu(
            0,
            &owners[1],
            portfolios[1],
            &owners[3],
            portfolios[3],
            POS_SCALE as i128,
            OPEN_PRICE,
            0,
        );

        // Accrue the market through a flat account so neither winner is settled
        // before the permissionless force-close ordering under test.
        let accrual_owner = Keypair::new();
        let accrual = env.create_portfolio(&accrual_owner);
        for slot in 2..=3 {
            env.svm.warp_to_slot(slot);
            env.push_auth_mark_for_asset_as_admin(0, slot, FROZEN_PRICE);
            env.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(accrual, false),
                ],
                &[],
            )
            .expect("flat-account observation must accrue the market");
        }
        assert_eq!(env.market_state().1.assets[0].effective_price, FROZEN_PRICE);
        env.close_portfolio_with_cu(&accrual_owner, accrual);

        let winner_order = if reverse { [1usize, 0] } else { [0usize, 1] };
        for i in winner_order {
            env.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: 3,
                    observations: vec![],
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[i], false),
                ],
                &[],
            )
            .expect("winner refresh must register its source-backed claim");
        }
        let source_before_resolve = env.market_state().1.source_credit[1];
        assert!(
            source_before_resolve.positive_claim_bound_num > BACKING * BOUND_SCALE,
            "the two winners must compete for undercollateralized backing"
        );
        assert!(
            source_before_resolve.credit_rate_num < percolator::CREDIT_RATE_SCALE,
            "the source-credit rate must reflect scarce backing"
        );

        env.resolve();
        env.svm.warp_to_slot(4);
        let terminal = |env: &V16CuEnv, portfolio: Pubkey| {
            let state = env.portfolio_state(portfolio);
            let receipt = resolved_receipt(&state);
            state.capital.get() == 0
                && state.pnl.get() == 0
                && percolator::active_bitmap_is_empty(active_bitmap(&state))
                && (!receipt.present || receipt.finalized)
        };
        let mut payouts = [0u128; 4];
        let close_order = if reverse {
            [2usize, 3, 1, 0]
        } else {
            [2usize, 3, 0, 1]
        };
        for _ in 0..32 {
            for i in close_order {
                if terminal(&env, portfolios[i]) {
                    continue;
                }
                let destination = env.close_resolved(&owners[i], portfolios[i]);
                payouts[i] += env.token_amount(destination) as u128;
            }
            if portfolios
                .iter()
                .all(|portfolio| terminal(&env, *portfolio))
            {
                break;
            }
        }
        assert!(
            portfolios
                .iter()
                .all(|portfolio| terminal(&env, *portfolio)),
            "honest round-robin resolved settlement must terminate"
        );
        let terminal = env.market_state().1;
        assert!(
            terminal.source_backing_buckets[1].consumed_liened_backing_num != 0
                || terminal.source_credit[1].provider_receivable_num != 0,
            "the differential must consume scarce source backing"
        );
        (
            payouts,
            terminal.vault,
            terminal.source_credit[1].positive_claim_bound_num,
            terminal.source_credit[1].provider_receivable_num,
            terminal.source_backing_buckets[1].consumed_liened_backing_num,
            terminal.source_backing_buckets[1].fresh_unliened_backing_num,
        )
    }

    let forward = run(false);
    let reverse = run(true);
    assert_eq!(
        reverse, forward,
        "permissionless resolved-close order allocated scarce source backing between users"
    );
    assert_eq!(forward.0, [300, 300, 0, 0]);
    assert_eq!(forward.1, 50, "provider backing remains canonical custody");
    assert_eq!(
        forward.5,
        50 * BOUND_SCALE,
        "neither claimant priority may consume the provider's fresh principal"
    );
}

#[test]
fn v16_bpf_force_close_pair_order_preserves_terminal_user_payouts() {
    fn run(cross_pair: bool) -> ([(u128, i128); 4], [u64; 4], u128, u128, u128, u128, u64) {
        let mut env = V16CuEnv::new();
        let cranker = Keypair::new();
        env.configure_permissionless_resolve_with_cu(100, 1);
        env.svm.warp_to_slot(1);
        env.configure_auth_mark_with_cu(1, 100);

        let owners = [
            Keypair::new(),
            Keypair::new(),
            Keypair::new(),
            Keypair::new(),
        ];
        let portfolios = [
            env.create_portfolio(&owners[0]),
            env.create_portfolio(&owners[1]),
            env.create_portfolio(&owners[2]),
            env.create_portfolio(&owners[3]),
        ];
        for i in 0..4 {
            env.deposit(&owners[i], portfolios[i], 100_000);
        }

        // The first pair enters at 100. The authenticated mark then advances to
        // 200 before the second pair enters, leaving one old winner and one old
        // loser when the asset freezes at 200.
        env.trade_asset_with_cu(
            0,
            &owners[0],
            portfolios[0],
            &owners[1],
            portfolios[1],
            POS_SCALE as i128,
            100,
            0,
        );
        env.svm.warp_to_slot(2);
        env.push_auth_mark_with_cu(2, 200);
        env.crank(
            portfolios[0],
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(0),
            },
        );
        assert_eq!(env.market_state().1.assets[0].effective_price, 200);
        env.trade_asset_with_cu(
            0,
            &owners[2],
            portfolios[2],
            &owners[3],
            portfolios[3],
            POS_SCALE as i128,
            200,
            0,
        );

        env.svm.warp_to_slot(3);
        env.update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_SHUTDOWN, 0, 3, 0);
        env.svm.warp_to_slot(4);

        if cross_pair {
            env.force_close_abandoned_asset_with_cu(
                &cranker,
                portfolios[0],
                portfolios[3],
                0,
                4,
                POS_SCALE,
            );
            env.force_close_abandoned_asset_with_cu(
                &cranker,
                portfolios[2],
                portfolios[1],
                0,
                4,
                POS_SCALE,
            );
        } else {
            env.force_close_abandoned_asset_with_cu(
                &cranker,
                portfolios[0],
                portfolios[1],
                0,
                4,
                POS_SCALE,
            );
            env.force_close_abandoned_asset_with_cu(
                &cranker,
                portfolios[2],
                portfolios[3],
                0,
                4,
                POS_SCALE,
            );
        }

        let accounts = portfolios.map(|portfolio| {
            let state = env.portfolio_state(portfolio);
            assert!(percolator::active_bitmap_is_empty(
                state.active_bitmap.map(|word| word.get())
            ));
            (state.capital.get(), state.pnl.get())
        });
        assert_eq!(
            accounts,
            [(100_000, 100), (99_900, 0), (100_000, 0), (100_000, 0),],
            "the probe must carry real realized PnL before comparing pair order"
        );
        // A flat winner's certificate can be invalidated by a later force-close
        // and cannot refresh against a Recovery asset. The configured bounded
        // permissionless market resolution must still provide an owner-independent
        // terminal exit.
        env.resolve_stale_permissionless_with_cu(103);
        env.svm.warp_to_slot(104);
        let mut payouts = [0u64; 4];
        for i in 0..4 {
            let destination = env.close_resolved(&owners[i], portfolios[i]);
            payouts[i] = env.token_amount(destination);
        }
        assert_eq!(payouts, [100_100, 99_900, 100_000, 100_000]);
        let (_, group) = env.market_state();
        assert_eq!(group.assets[0].oi_eff_long_q, 0);
        assert_eq!(group.assets[0].oi_eff_short_q, 0);
        (
            accounts,
            payouts,
            group.insurance,
            group.vault,
            group.insurance_domain_spent[0],
            group.insurance_domain_spent[1],
            env.token_amount(env.vault),
        )
    }

    let direct = run(false);
    let crossed = run(true);
    assert_eq!(
        crossed, direct,
        "a permissionless cranker must not allocate value by choosing force-close pairs"
    );
}

#[test]
fn v16_attack_resolved_two_public_winners_are_close_order_independent() {
    fn run(winner_attempts_first: bool) -> (u128, u128, u128) {
        let mut env = V16CuEnv::new_with_init_params(production_risk_params());
        env.configure_auth_mark_with_cu(0, 1_000_000);

        let winner_a_owner = Keypair::new();
        let winner_a = env.create_portfolio(&winner_a_owner);
        let winner_b_owner = Keypair::new();
        let winner_b = env.create_portfolio(&winner_b_owner);
        let loser_owner = Keypair::new();
        let loser = env.create_portfolio(&loser_owner);
        let accrual_owner = Keypair::new();
        let accrual = env.create_portfolio(&accrual_owner);
        for (owner, portfolio) in [
            (&winner_a_owner, winner_a),
            (&winner_b_owner, winner_b),
            (&loser_owner, loser),
        ] {
            env.deposit(owner, portfolio, 1_000_000);
        }
        env.trade_asset_with_cu(
            0,
            &winner_a_owner,
            winner_a,
            &loser_owner,
            loser,
            (POS_SCALE / 2) as i128,
            1_000_000,
            0,
        );
        env.trade_asset_with_cu(
            0,
            &winner_b_owner,
            winner_b,
            &loser_owner,
            loser,
            (POS_SCALE / 2) as i128,
            1_000_000,
            0,
        );

        // Reach +10% through the production 24-bps/slot circuit breaker without giving either
        // winner a different funding-settlement cadence.
        for slot in 1..=50u64 {
            env.svm.warp_to_slot(slot);
            env.push_auth_mark_with_cu(slot, 1_100_000);
            env.crank(
                accrual,
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
            );
        }
        assert_eq!(env.market_state().1.assets[0].effective_price, 1_100_000);
        env.close_portfolio_with_cu(&accrual_owner, accrual);
        for _ in 0..2 {
            for portfolio in [winner_a, winner_b, loser] {
                env.svm.expire_blockhash();
                env.send(
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 50,
                        observations: crank_observations(0),
                    },
                    vec![
                        AccountMeta::new(env.payer.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolio, false),
                    ],
                    &[],
                )
                .expect("public pre-resolution settlement crank");
            }
        }
        for (owner, winner) in [(&winner_a_owner, winner_a), (&winner_b_owner, winner_b)] {
            env.trade_asset_with_cu(
                0,
                owner,
                winner,
                &loser_owner,
                loser,
                -((POS_SCALE / 2) as i128),
                1_100_000,
                0,
            );
        }
        for portfolio in [winner_a, winner_b, loser] {
            assert!(percolator::active_bitmap_is_empty(active_bitmap(
                &env.portfolio_state(portfolio)
            )));
        }
        env.resolve();

        let close_once = |env: &mut V16CuEnv, owner: &Keypair, portfolio: Pubkey| -> u128 {
            let (dest, cu) = env.close_resolved_with_cu(owner, portfolio);
            assert_cu_within("three-party CloseResolved", cu, CUSTODY_CU_LIMIT);
            env.token_amount(dest) as u128
        };
        let drive = |env: &mut V16CuEnv, owner: &Keypair, portfolio: Pubkey| -> u128 {
            let mut paid = 0u128;
            for _ in 0..8 {
                paid += close_once(env, owner, portfolio);
                let state = env.portfolio_state(portfolio);
                if state.capital.get() == 0
                    && state.pnl.get() == 0
                    && percolator::active_bitmap_is_empty(active_bitmap(&state))
                    && (!resolved_receipt(&state).present || resolved_receipt(&state).finalized)
                {
                    return paid;
                }
            }
            panic!("resolved close did not terminate within eight bounded calls");
        };

        let mut winner_a_paid = 0;
        let mut winner_b_paid = 0;
        if winner_attempts_first {
            winner_a_paid += close_once(&mut env, &winner_a_owner, winner_a);
        }
        let loser_paid = drive(&mut env, &loser_owner, loser);
        if winner_attempts_first {
            winner_b_paid += drive(&mut env, &winner_b_owner, winner_b);
            winner_a_paid += drive(&mut env, &winner_a_owner, winner_a);
        } else {
            winner_b_paid += close_once(&mut env, &winner_b_owner, winner_b);
            winner_a_paid += drive(&mut env, &winner_a_owner, winner_a);
            winner_b_paid += drive(&mut env, &winner_b_owner, winner_b);
        }

        for (owner, portfolio) in [
            (&winner_a_owner, winner_a),
            (&winner_b_owner, winner_b),
            (&loser_owner, loser),
        ] {
            env.close_portfolio_with_cu(owner, portfolio);
        }
        let (_, group) = env.market_state();
        assert_eq!(group.materialized_portfolio_count, 0);
        assert_eq!(group.c_tot, 0);
        assert_eq!(group.vault, 0);
        (winner_a_paid, winner_b_paid, loser_paid)
    }

    let loser_first = run(false);
    let premature_winner_first = run(true);
    assert_eq!(premature_winner_first, loser_first);
    assert_eq!(loser_first.0, loser_first.1);
    assert_eq!(loser_first.0 + loser_first.1 + loser_first.2, 3_000_000);
    assert!(loser_first.0 > 1_000_000 && loser_first.2 < 1_000_000);
}

#[test]
fn v16_bpf_force_close_pair_order_preserves_unequal_partial_payouts() {
    fn run(cross_pair: bool) -> ([(u128, i128); 4], [u64; 4], u128, u128, u128, u128, u64) {
        let mut env = V16CuEnv::new();
        let cranker = Keypair::new();
        env.configure_permissionless_resolve_with_cu(100, 1);
        env.svm.warp_to_slot(1);
        env.configure_auth_mark_with_cu(1, 100);

        let owners = [
            Keypair::new(),
            Keypair::new(),
            Keypair::new(),
            Keypair::new(),
        ];
        let portfolios = [
            env.create_portfolio(&owners[0]),
            env.create_portfolio(&owners[1]),
            env.create_portfolio(&owners[2]),
            env.create_portfolio(&owners[3]),
        ];
        for (i, amount) in [1_000, 1_000, 2_000, 2_000].into_iter().enumerate() {
            env.deposit(&owners[i], portfolios[i], amount);
        }

        env.trade_asset_with_cu(
            0,
            &owners[0],
            portfolios[0],
            &owners[1],
            portfolios[1],
            POS_SCALE as i128,
            100,
            0,
        );
        env.trade_asset_with_cu(
            0,
            &owners[2],
            portfolios[2],
            &owners[3],
            portfolios[3],
            (2 * POS_SCALE) as i128,
            100,
            0,
        );
        for (slot, mark) in [(2, 200), (3, 300)] {
            env.svm.warp_to_slot(slot);
            env.push_auth_mark_with_cu(slot, mark);
            for portfolio in portfolios {
                env.crank(
                    portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: crank_observations(0),
                    },
                );
            }
        }

        env.svm.warp_to_slot(4);
        env.update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_SHUTDOWN, 0, 4, 0);
        env.svm.warp_to_slot(5);
        if cross_pair {
            for (long, short) in [
                (portfolios[0], portfolios[3]),
                (portfolios[2], portfolios[1]),
                (portfolios[2], portfolios[3]),
            ] {
                env.force_close_abandoned_asset_with_cu(&cranker, long, short, 0, 5, POS_SCALE);
            }
        } else {
            env.force_close_abandoned_asset_with_cu(
                &cranker,
                portfolios[0],
                portfolios[1],
                0,
                5,
                POS_SCALE,
            );
            env.force_close_abandoned_asset_with_cu(
                &cranker,
                portfolios[2],
                portfolios[3],
                0,
                5,
                2 * POS_SCALE,
            );
        }

        let accounts = portfolios.map(|portfolio| {
            let state = env.portfolio_state(portfolio);
            assert!(percolator::active_bitmap_is_empty(
                state.active_bitmap.map(|word| word.get())
            ));
            (state.capital.get(), state.pnl.get())
        });
        assert_eq!(
            accounts,
            [(1_000, 200), (800, 0), (2_000, 400), (1_600, 0)],
            "unequal force-closes must carry nonzero realized PnL before the order comparison"
        );

        env.svm.warp_to_slot(104);
        env.resolve_stale_permissionless_with_cu(104);
        env.svm.warp_to_slot(105);
        let mut payouts = [0u64; 4];
        for i in 0..4 {
            let destination = env.close_resolved(&owners[i], portfolios[i]);
            payouts[i] = env.token_amount(destination);
        }
        assert_eq!(payouts, [1_200, 800, 2_400, 1_600]);
        let (_, group) = env.market_state();
        (
            accounts,
            payouts,
            group.vault,
            group.insurance,
            group.assets[0].oi_eff_long_q,
            group.assets[0].oi_eff_short_q,
            env.token_amount(env.vault),
        )
    }

    let direct = run(false);
    let crossed = run(true);
    assert_eq!(
        crossed, direct,
        "a cranker allocated value by choosing an unequal partial force-close schedule"
    );
}
