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

fn close_resolved_until_terminal(
    env: &mut V16CuEnv,
    owner: &Keypair,
    portfolio: Pubkey,
    label: &str,
) -> u64 {
    let mut paid = 0u64;
    for _ in 0..32 {
        if resolved_portfolio_is_terminal(env, portfolio) {
            return paid;
        }
        let (destination, cu) = env.close_resolved_with_cu(owner, portfolio);
        assert_cu_within(label, cu, CUSTODY_CU_LIMIT);
        paid = paid
            .checked_add(env.token_amount(destination))
            .expect("resolved payout total overflow");
        if resolved_portfolio_is_terminal(env, portfolio) {
            return paid;
        }
    }
    panic!("{label} did not reach a terminal portfolio in 32 bounded calls");
}

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
            payouts[i] = close_resolved_until_terminal(
                &mut env,
                &owners[i],
                portfolios[i],
                "force-close pair-order terminal settlement",
            );
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

        let (winner_a_paid, winner_b_paid, loser_paid) = if winner_attempts_first {
            let paid = drain_resolved_cohort(
                &mut env,
                &[
                    (&winner_a_owner, winner_a),
                    (&winner_b_owner, winner_b),
                    (&loser_owner, loser),
                ],
                "winner-first three-party settlement",
            );
            (paid[0], paid[1], paid[2])
        } else {
            let paid = drain_resolved_cohort(
                &mut env,
                &[
                    (&loser_owner, loser),
                    (&winner_b_owner, winner_b),
                    (&winner_a_owner, winner_a),
                ],
                "loser-first three-party settlement",
            );
            (paid[2], paid[1], paid[0])
        };

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
            payouts[i] = close_resolved_until_terminal(
                &mut env,
                &owners[i],
                portfolios[i],
                "unequal force-close terminal settlement",
            );
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

// security.md sweep — resolved wind-down LoF / over-claim (#22/#30/#48): a market can be resolved
// with OPEN positions (handle_resolve_market does not require flat). After resolution a long and a
// short must each recover their FAIR value via CloseResolved — neither stuck (LoF) nor able to
// over-claim. Total tokens paid out must never exceed total deposited.
#[test]
fn v16_regression_resolved_open_positions_recover_fairly_order_robust() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 1_000, 1_000, 500);
    env.configure_auth_mark_with_cu(0, 100);
    let lo_owner = Keypair::new();
    let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new();
    let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, 1_000_000);
    env.deposit(&sh_owner, sh, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &lo_owner,
        lo,
        &sh_owner,
        sh,
        (10_000 * POS_SCALE) as i128,
        100,
        0,
    ); // notional 1M
       // move price so the long wins, settle both legs across two slots, THEN resolve with positions still open.
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110);
    for slot in [10u64, 11] {
        env.svm.warp_to_slot(slot);
        for p in [sh, lo] {
            env.svm.expire_blockhash();
            let _ = env.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(p, false),
                ],
                &[],
            );
        }
    }
    env.resolve(); // resolve WITH open positions still on the book

    let bal = |env: &V16CuEnv, k: &Pubkey| -> u128 {
        let d = env.svm.get_account(k).unwrap().data;
        u64::from_le_bytes(d[64..72].try_into().unwrap()) as u128
    };
    // Winner (long) closes FIRST, before the loser has funded the vault. This may perform bounded
    // terminal cleanup, but it must defer payout and preserve the winner's capital and PnL exactly.
    let (dest_lo1, _) = env.close_resolved_with_cu(&lo_owner, lo);
    assert_eq!(
        bal(&env, &dest_lo1),
        0,
        "premature winner close pays nothing (vault not yet funded)"
    );
    let mid = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    assert_eq!(
        mid.capital.get(),
        1_000_000,
        "premature winner close preserves capital while payout is deferred"
    );
    assert_eq!(
        mid.pnl.get(),
        100_000,
        "premature winner close preserves parked pnl"
    );

    // Loser closes (recovers its post-loss capital, funding the vault for the winner).
    let (dest_sh, _) = env.close_resolved_with_cu(&sh_owner, sh);
    let out_sh = bal(&env, &dest_sh);
    // Winner RETRIES and now recovers full fair value.
    let (dest_lo2, _) = env.close_resolved_with_cu(&lo_owner, lo);
    let out_lo = bal(&env, &dest_lo2);

    // No LoF, exact value conservation, fair winner/loser split.
    assert_eq!(
        out_lo + out_sh,
        2_000_000,
        "every account recovers; total payout == total deposited (no LoF, no printing)"
    );
    assert_eq!(
        out_lo, 1_100_000,
        "winner recovers capital + realized profit"
    );
    assert_eq!(out_sh, 900_000, "loser recovers capital - realized loss");
    let a = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    let b = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
    assert_eq!(a.capital.get(), 0, "long fully wound down");
    assert_eq!(b.capital.get(), 0, "short fully wound down");
    // The winner's positive pnl is source-backed (the directional trade created source credit),
    // so at resolved close it is REALIZED into capital and paid as capital — no junior payout
    // receipt is parked. The account winds down completely (pnl 0, capital 0, no receipt).
    assert!(
        !resolved_receipt(&a).present,
        "winner fully paid via capital realization, no dangling receipt"
    );
    assert_eq!(a.pnl.get(), 0, "winner pnl fully realized");
    let (_, g) = env.market_state();
    assert_eq!(g.vault, 0, "vault fully drained, no funds stranded");
}

// security.md sweep — haircut payout rounding across multiple winners (#33/#37): when several
// resolved winners share ONE insufficient backing pool, each is paid floor(face * rate). The sum of
// floored payouts must NEVER exceed the backing (a rounding-up bug would let winners collectively
// extract more than the pool holds). Probe with deliberately non-divisible faces.
#[test]
fn v16_regression_resolved_multiwinner_haircut_no_overpay_no_strand() {
    const BACKING: u128 = 100;
    // three winners with non-divisible positive-pnl faces against a shared 100 backing.
    let faces: [u128; 3] = [250, 251, 253];
    let mut env = V16CuEnv::new();
    env.top_up_backing_bucket(1, BACKING, 10_000);
    let mut owners = Vec::new();
    let mut ports = Vec::new();
    for &face in faces.iter() {
        let o = Keypair::new();
        let p = env.create_portfolio(&o);
        env.deposit(&o, p, 1_000);
        env.add_source_positive_pnl(p, 1, face);
        owners.push(o);
        ports.push(p);
    }
    env.resolve();
    let actors: Vec<_> = owners.iter().zip(ports.iter().copied()).collect();
    let payouts = drain_resolved_cohort(
        &mut env,
        &actors,
        "three-winner nondivisible haircut settlement",
    );
    let total_out: u128 = payouts.iter().sum();
    let total_pnl_paid: u128 = payouts.iter().map(|paid| paid.saturating_sub(1_000)).sum();
    // CRUX 1: summed haircut pnl never exceeds the shared backing (no rounding-up over-pay).
    assert!(
        total_pnl_paid <= BACKING,
        "summed haircut pnl {} must not exceed backing {}",
        total_pnl_paid,
        BACKING
    );
    // CRUX 2 (no strand): every winner's receipt is closable and the portfolio dematerializes.
    for (o, p) in owners.iter().zip(ports.iter()) {
        let a = state::read_portfolio(&env.svm.get_account(p).unwrap().data).unwrap();
        assert_eq!(a.capital.get(), 0, "winner capital fully paid");
        assert!(
            !resolved_receipt(&a).present || resolved_receipt(&a).finalized,
            "receipt closable after retry"
        );
        env.close_portfolio_with_cu(o, *p); // panics if dematerialization is blocked
    }
    let (_, g) = env.market_state();
    assert_eq!(
        g.materialized_portfolio_count, 0,
        "all winners dematerialized — no permanent strand"
    );
    assert_eq!(g.c_tot, 0, "all capital wound down");
    assert!(
        g.vault <= 1,
        "at most conservative-rounding dust remains in vault (got {})",
        g.vault
    );
    assert!(
        total_out >= 3_000,
        "all senior capital recovered (no LoF on capital)"
    );
}

// security.md sweep — resolve mid-flight before settlement (#30 sequence/race): push a price move,
// then resolve WITHOUT any settlement crank. The resolved wind-down must still settle at the true
// post-move price — the winner recovers their gain, the loser bears their loss, value conserved.
// Attacker success = stale pre-move settlement (winner LoF, or loser escapes its loss).
#[test]
fn v16_regression_resolve_before_settlement_uses_official_price() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let lo_owner = Keypair::new();
    let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new();
    let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, 1_000_000);
    env.deposit(&sh_owner, sh, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &lo_owner,
        lo,
        &sh_owner,
        sh,
        (10_000 * POS_SCALE) as i128,
        100,
        0,
    ); // notional 1M
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110); // pending mark; NOT yet accrued into effective_price (anti-retroactivity)
                                         // NO crank: resolve immediately. The pushed mark is unaccrued, so the official effective_price is
                                         // still 100 and the position is officially flat.
    let (_, g_pre) = env.market_state();
    assert_eq!(
        g_pre.assets[0].effective_price, 100,
        "unaccrued mark push does NOT move the official price"
    );
    env.resolve();

    fn bal(env: &V16CuEnv, k: &Pubkey) -> u128 {
        let d = env.svm.get_account(k).unwrap().data;
        u64::from_le_bytes(d[64..72].try_into().unwrap()) as u128
    }
    // loser-first, then winner (order-robust wind-down established in batch 23). Retry winner if deferred.
    let _ = env.close_resolved(&sh_owner, sh);
    let d1 = env.close_resolved(&lo_owner, lo);
    let mut won = bal(&env, &d1);
    if won == 0 {
        let d2 = env.close_resolved(&lo_owner, lo);
        won = bal(&env, &d2);
    }
    let lost = {
        let b = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
        assert_eq!(b.capital.get(), 0, "loser wound down");
        2_000_000u128.saturating_sub(won)
    };
    // CORRECT behavior: resolve settles at the OFFICIAL accrued price (100). The unaccrued mark push
    // is NOT retroactively applied, so no value is created or destroyed — each party recovers exactly
    // its deposit. (Contrast batch 23: crank-to-accrue BEFORE resolve, and the winner gets 1.1M.)
    assert_eq!(
        won, 1_000_000,
        "no value invented from an unaccrued mark — deposit returned"
    );
    assert_eq!(
        won + lost,
        2_000_000,
        "exact conservation across resolve-before-settlement"
    );
    let (_, g) = env.market_state();
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    let a = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    assert_eq!(a.capital.get(), 0, "long fully wound down");
    assert!(
        resolved_receipt(&a).finalized || !resolved_receipt(&a).present,
        "receipt closable"
    );
}
