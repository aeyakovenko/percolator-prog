//! INV-041 - Deterministic allocation and caller-order independence.
//!
//! Normative obligation: Caller ordering cannot change allocation, loss attribution, or economic outcome.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): force-close chunking,
//! multi-observation crank ordering, and all `4!` Recovery exit orders for two positions per side.
//! These tests exercise the deployed public wrapper with real SBF/LiteSVM accounts and assert
//! economic state, token custody, side counters, liveness, and compute outcomes.
//!
//! Guarantee boundary: the Recovery matrix exhausts four-party landing orders with unequal
//! one-/two-lot weights and a real 100-to-150 mark move, and independently reconstructs its side
//! state after every instruction. Liquidation, insurance, lien, payout, claim, and
//! close-preemption ordering remain separate open partitions.

use super::*;

const FOUR_PARTY_RECOVERY_ASSET: usize = 1;

#[test]
fn v16_attack_force_close_dust_chunking_is_value_path_independent() {
    fn run(
        chunks: &[u128],
        with_backing: bool,
    ) -> (
        (u128, i128, i128, u128, i128, i128, u128, u128, u128),
        (u128, u128, u128, u128),
    ) {
        const OPEN_PRICE: u64 = 101;
        const CLOSE_PRICE: u64 = 137;
        const SIZE_Q: u128 = POS_SCALE + 17;
        const SHUT_SLOT: u64 = 3;
        const CLOSE_SLOT: u64 = 5;

        let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
        env.configure_permissionless_resolve_with_cu(100, 1);
        env.configure_auth_mark_for_asset_as_admin(0, 1, OPEN_PRICE);
        if with_backing {
            env.top_up_backing_bucket(1, 1_000, 10);
        }

        let long_owner = Keypair::new();
        let short_owner = Keypair::new();
        let long = env.create_portfolio(&long_owner);
        let short = env.create_portfolio(&short_owner);
        env.deposit(&long_owner, long, 1_000_000);
        env.deposit(&short_owner, short, 1_000_000);
        env.trade_asset_with_cu(
            0,
            &long_owner,
            long,
            &short_owner,
            short,
            SIZE_Q as i128,
            OPEN_PRICE,
            0,
        );

        env.svm.warp_to_slot(2);
        env.push_auth_mark_for_asset_as_admin(0, 2, CLOSE_PRICE);
        env.crank_steps_after_market_catchup(
            long,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(0),
            },
            1,
        );
        env.crank(
            short,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(0),
            },
        );
        assert_ne!(
            env.portfolio_state(long).pnl.get(),
            0,
            "setup must realize nonzero mark-to-market value"
        );

        env.svm.warp_to_slot(SHUT_SLOT);
        env.update_asset_lifecycle_as_admin_with_cu(
            processor::ASSET_ACTION_SHUTDOWN,
            0,
            SHUT_SLOT,
            0,
        );
        env.svm.warp_to_slot(CLOSE_SLOT);
        let cranker = Keypair::new();
        for &chunk in chunks {
            if !has_active_leg_for_asset(&env.portfolio_state(long), 0) {
                break;
            }
            env.force_close_abandoned_asset_with_cu(&cranker, long, short, 0, CLOSE_SLOT, chunk);
        }
        if has_active_leg_for_asset(&env.portfolio_state(long), 0) {
            env.force_close_abandoned_asset_with_cu(
                &cranker,
                long,
                short,
                0,
                CLOSE_SLOT,
                u128::MAX,
            );
        }

        let long_state = env.portfolio_state(long);
        let short_state = env.portfolio_state(short);
        let group = env.market_state().1;
        assert!(!has_active_leg_for_asset(&long_state, 0));
        assert!(!has_active_leg_for_asset(&short_state, 0));
        let source = group.source_credit[1];
        if with_backing {
            assert!(
                source.positive_claim_bound_num != 0,
                "backed setup must create a real source-credit claim"
            );
        }
        (
            (
                long_state.capital.get(),
                long_state.pnl.get(),
                long_state.fee_credits.get(),
                short_state.capital.get(),
                short_state.pnl.get(),
                short_state.fee_credits.get(),
                group.insurance,
                group.assets[0].oi_eff_long_q,
                group.assets[0].oi_eff_short_q,
            ),
            (
                source.positive_claim_bound_num,
                source.fresh_reserved_backing_num,
                source.provider_receivable_num,
                group.source_backing_buckets[1].fresh_unliened_backing_num,
            ),
        )
    }

    let one_shot = run(&[u128::MAX], false);
    let dust_chunked = run(
        &[
            1,
            POS_SCALE / 7,
            3,
            POS_SCALE / 5,
            11,
            POS_SCALE / 3,
            u128::MAX,
        ],
        false,
    );
    assert_eq!(
        dust_chunked, one_shot,
        "permissionless close_q chunking must not change either user's value or market accounting"
    );

    let backed_one_shot = run(&[u128::MAX], true);
    let backed_dust_chunked = run(
        &[
            1,
            POS_SCALE / 7,
            3,
            POS_SCALE / 5,
            11,
            POS_SCALE / 3,
            u128::MAX,
        ],
        true,
    );
    assert_eq!(
        backed_dust_chunked, backed_one_shot,
        "chunking must not alter source-credit or provider-backing value allocation"
    );
}

#[test]
fn v16_attack_multi_observation_crank_order_cannot_change_economics() {
    const MARK: u64 = 1_000_000;
    const OPEN_SLOT: u64 = 1;
    const CRANK_SLOT: u64 = 2;

    let run = |order: [u16; 2]| {
        let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
        env.configure_auth_mark_for_asset_as_admin(0, OPEN_SLOT, MARK);
        env.configure_auth_mark_for_asset_as_admin(1, OPEN_SLOT, MARK);

        let owner_a = Keypair::new();
        let owner_b = Keypair::new();
        let account_a = env.create_portfolio(&owner_a);
        let account_b = env.create_portfolio(&owner_b);
        env.deposit(&owner_a, account_a, 100_000_000);
        env.deposit(&owner_b, account_b, 100_000_000);
        env.trade_asset_with_cu(
            0,
            &owner_a,
            account_a,
            &owner_b,
            account_b,
            POS_SCALE as i128,
            MARK,
            0,
        );
        env.svm.expire_blockhash();
        env.trade_asset_with_cu(
            1,
            &owner_a,
            account_a,
            &owner_b,
            account_b,
            POS_SCALE as i128,
            MARK,
            0,
        );

        set_test_clock(&mut env, CRANK_SLOT, 101);
        env.push_auth_mark_for_asset_as_admin(0, CRANK_SLOT, MARK + 10_000);
        env.push_auth_mark_for_asset_as_admin(1, CRANK_SLOT, MARK - 20_000);
        let mut observations = Vec::new();
        for asset_index in order {
            observations.extend(crank_observations(asset_index));
        }
        env.crank(
            account_a,
            ProgInstruction::PermissionlessCrank {
                now_slot: CRANK_SLOT,
                observations,
            },
        );

        let mut group = env.market_state().1;
        group.market_group_id = [0; 32];
        let mut a = env.portfolio_state(account_a);
        a.provenance_header = Default::default();
        a.owner = [0; 32];
        let mut b = env.portfolio_state(account_b);
        b.provenance_header = Default::default();
        b.owner = [0; 32];
        (group, a, b, env.token_amount(env.vault))
    };

    let forward = run([0, 1]);
    let reverse = run([1, 0]);
    assert_eq!(
        forward, reverse,
        "caller-chosen observation order must not change market or user economics"
    );
}

fn assert_recovery_order_census(env: &V16CuEnv, portfolios: &[Pubkey; 4]) -> usize {
    let asset = env.market_state().1.assets[FOUR_PARTY_RECOVERY_ASSET];
    let mut oi_long = 0u128;
    let mut oi_short = 0u128;
    let mut stored_long = 0u64;
    let mut stored_short = 0u64;
    let mut pending_long = 0u64;
    let mut pending_short = 0u64;
    let mut weight_long = 0u128;
    let mut weight_short = 0u128;

    for portfolio in portfolios {
        let account = env.portfolio_state(*portfolio);
        if !has_active_leg_for_asset(&account, FOUR_PARTY_RECOVERY_ASSET) {
            continue;
        }
        let leg = active_leg_for_asset(&account, FOUR_PARTY_RECOVERY_ASSET);
        match leg.side {
            SideV16::Long => {
                stored_long += 1;
                weight_long = weight_long.checked_add(leg.loss_weight).unwrap();
                if leg.basis_pos_q == 0 {
                    pending_long += 1;
                } else {
                    oi_long = oi_long.checked_add(leg.basis_pos_q.unsigned_abs()).unwrap();
                }
            }
            SideV16::Short => {
                stored_short += 1;
                weight_short = weight_short.checked_add(leg.loss_weight).unwrap();
                if leg.basis_pos_q == 0 {
                    pending_short += 1;
                } else {
                    oi_short = oi_short
                        .checked_add(leg.basis_pos_q.unsigned_abs())
                        .unwrap();
                }
            }
        }
    }

    assert_eq!(asset.oi_eff_long_q, oi_long);
    assert_eq!(asset.oi_eff_short_q, oi_short);
    assert_eq!(asset.stored_pos_count_long, stored_long);
    assert_eq!(asset.stored_pos_count_short, stored_short);
    assert_eq!(asset.pending_obligation_count_long, pending_long);
    assert_eq!(asset.pending_obligation_count_short, pending_short);
    assert_eq!(asset.loss_weight_sum_long, weight_long);
    assert_eq!(asset.loss_weight_sum_short, weight_short);
    assert!(pending_long <= stored_long);
    assert!(pending_short <= stored_short);
    (pending_long + pending_short) as usize
}

#[derive(Debug, PartialEq, Eq)]
struct FourPartyRecoveryOutcome {
    payouts: [u64; 4],
    terminal_vault: u64,
    terminal_c_tot: u128,
}

fn run_four_party_recovery_order(order: [usize; 4]) -> FourPartyRecoveryOutcome {
    const OPEN_PRICE: u64 = 100;
    const RECOVERY_PRICE: u64 = 150;
    const DEPOSIT: u128 = 1_000;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_permissionless_resolve_with_cu(100, 1);
    env.configure_auth_mark_for_asset_as_admin(FOUR_PARTY_RECOVERY_ASSET as u16, 0, OPEN_PRICE);
    let owners: [Keypair; 4] = std::array::from_fn(|_| Keypair::new());
    let portfolios: [Pubkey; 4] = std::array::from_fn(|index| {
        let portfolio = env.create_portfolio(&owners[index]);
        env.deposit(&owners[index], portfolio, DEPOSIT);
        portfolio
    });

    for (pair_index, (long_index, short_index)) in
        [(0usize, 1usize), (2, 3)].into_iter().enumerate()
    {
        let size_q = (pair_index as u128 + 1).checked_mul(POS_SCALE).unwrap();
        env.trade_asset_with_cu(
            FOUR_PARTY_RECOVERY_ASSET as u16,
            &owners[long_index],
            portfolios[long_index],
            &owners[short_index],
            portfolios[short_index],
            size_q as i128,
            OPEN_PRICE,
            0,
        );
    }
    assert_eq!(assert_recovery_order_census(&env, &portfolios), 0);

    env.svm.warp_to_slot(1);
    env.push_auth_mark_for_asset_as_admin(FOUR_PARTY_RECOVERY_ASSET as u16, 1, RECOVERY_PRICE);
    env.update_asset_lifecycle_as_admin_with_cu(
        processor::ASSET_ACTION_SHUTDOWN,
        FOUR_PARTY_RECOVERY_ASSET as u16,
        1,
        0,
    );
    assert_eq!(
        env.market_state().1.assets[FOUR_PARTY_RECOVERY_ASSET].lifecycle,
        AssetLifecycleV16::Recovery
    );
    assert_eq!(
        env.market_state().1.assets[FOUR_PARTY_RECOVERY_ASSET].effective_price,
        RECOVERY_PRICE
    );

    let label = format!("four-party Recovery order {order:?}");
    for index in order {
        let before = env.portfolio_state(portfolios[index]);
        assert!(has_active_leg_for_asset(&before, FOUR_PARTY_RECOVERY_ASSET));
        assert_ne!(
            active_leg_for_asset(&before, FOUR_PARTY_RECOVERY_ASSET).basis_pos_q,
            0
        );
        let cu = env.forfeit_recovery_leg_with_cu(
            &owners[index],
            portfolios[index],
            FOUR_PARTY_RECOVERY_ASSET as u16,
            u128::MAX,
        );
        assert_cu_within(&label, cu, CRANK_CU_LIMIT);
        let after = env.portfolio_state(portfolios[index]);
        assert!(
            !has_active_leg_for_asset(&after, FOUR_PARTY_RECOVERY_ASSET)
                || active_leg_for_asset(&after, FOUR_PARTY_RECOVERY_ASSET).basis_pos_q == 0,
            "{label}: an accepted owner forfeit retained real exposure"
        );
        assert_recovery_order_census(&env, &portfolios);
    }

    let retained = assert_recovery_order_census(&env, &portfolios);
    assert!(
        (2..=3).contains(&retained),
        "{label}: the four-party frontier must retain two or three obligations, got {retained}"
    );
    let after_forfeits = env.market_state().1.assets[FOUR_PARTY_RECOVERY_ASSET];
    assert_eq!(after_forfeits.oi_eff_long_q, 0);
    assert_eq!(after_forfeits.oi_eff_short_q, 0);

    for index in order.into_iter().rev() {
        if !has_active_leg_for_asset(
            &env.portfolio_state(portfolios[index]),
            FOUR_PARTY_RECOVERY_ASSET,
        ) {
            continue;
        }
        let market_before = env.svm.get_account(&env.market).unwrap();
        let account_before = env.svm.get_account(&portfolios[index]).unwrap();
        let cu = env.crank(
            portfolios[index],
            ProgInstruction::PermissionlessCrank {
                now_slot: 1,
                observations: crank_observations(FOUR_PARTY_RECOVERY_ASSET as u16),
            },
        );
        assert_cu_within(&label, cu, CRANK_CU_LIMIT);
        assert!(
            env.svm.get_account(&env.market).unwrap() != market_before
                || env.svm.get_account(&portfolios[index]).unwrap() != account_before,
            "{label}: an accepted cleanup crank was a no-op"
        );
        assert!(
            !has_active_leg_for_asset(
                &env.portfolio_state(portfolios[index]),
                FOUR_PARTY_RECOVERY_ASSET,
            ),
            "{label}: a released zero-basis obligation did not clear in one bounded crank"
        );
        assert_recovery_order_census(&env, &portfolios);
    }

    assert_eq!(assert_recovery_order_census(&env, &portfolios), 0);
    let terminal_asset = env.market_state().1.assets[FOUR_PARTY_RECOVERY_ASSET];
    assert_eq!(terminal_asset.oi_eff_long_q, 0);
    assert_eq!(terminal_asset.oi_eff_short_q, 0);
    assert_eq!(terminal_asset.stored_pos_count_long, 0);
    assert_eq!(terminal_asset.stored_pos_count_short, 0);
    assert_eq!(terminal_asset.pending_obligation_count_long, 0);
    assert_eq!(terminal_asset.pending_obligation_count_short, 0);
    assert_eq!(terminal_asset.loss_weight_sum_long, 0);
    assert_eq!(terminal_asset.loss_weight_sum_short, 0);

    let mut payouts = [0u64; 4];
    let expected_payouts = [DEPOSIT, DEPOSIT - 50, DEPOSIT, DEPOSIT - 100];
    for index in 0..4 {
        let account = env.portfolio_state(portfolios[index]);
        assert_eq!(account.capital.get(), expected_payouts[index]);
        assert_eq!(account.pnl.get(), 0);
        let destination = env.withdraw(&owners[index], portfolios[index], expected_payouts[index]);
        payouts[index] = env.token_amount(destination);
        env.close_portfolio_with_cu(&owners[index], portfolios[index]);
    }
    let terminal = env.market_state().1;
    assert_eq!(
        payouts.iter().copied().sum::<u64>(),
        expected_payouts.iter().copied().sum::<u128>() as u64
    );
    assert_eq!(terminal.vault as u64, env.token_amount(env.vault));
    FourPartyRecoveryOutcome {
        payouts,
        terminal_vault: env.token_amount(env.vault),
        terminal_c_tot: terminal.c_tot,
    }
}

#[test]
fn v16_program_four_party_recovery_exit_orders_are_economically_identical() {
    let mut baseline = None;
    let mut permutations = 0usize;
    for a in 0..4 {
        for b in 0..4 {
            for c in 0..4 {
                for d in 0..4 {
                    let order = [a, b, c, d];
                    if a == b || a == c || a == d || b == c || b == d || c == d {
                        continue;
                    }
                    let outcome = run_four_party_recovery_order(order);
                    assert_eq!(outcome.payouts, [1_000, 950, 1_000, 900]);
                    assert_eq!(outcome.terminal_vault, 150);
                    assert_eq!(outcome.terminal_c_tot, 0);
                    if let Some(expected) = &baseline {
                        assert_eq!(
                            &outcome, expected,
                            "Recovery landing order {order:?} changed terminal economics"
                        );
                    } else {
                        baseline = Some(outcome);
                    }
                    permutations += 1;
                }
            }
        }
    }
    assert_eq!(permutations, 24);
}
