//! INV-061 - Deterministic, bounded liquidation.
//!
//! Normative obligation: Liquidation is deterministic, risk reducing, OI coherent, and bounded at maximum shape.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): fixed-pin ADL-transfer
//! rejection and owner-exit matrices, reset-carry liquidation matrices, public liquidation health
//! checks, bounded partial closes, fee caps, no-repeat charging after restored health, reward split
//! bounds, and no vault minting. These tests exercise the deployed public wrapper with real
//! SBF/LiteSVM account construction and assert economic state, token, rollback, liveness, or
//! compute outcomes appropriate to the invariant.
//!
//! The reset-carry matrix creates a denominator-crossing social-loss carry through public
//! bankruptcies and owner reduction, then requires the sole public crank to liquidate the next
//! unhealthy account below the CU ceiling and normalize the carry. The remaining funded owners
//! then submit stale raw-basis work budgets; each public reduction is independently checked to
//! consume only effective OI, after which bounded cleanup, provider-receivable refill, both side
//! finalizers, and retirement preserve SPL custody. The shared INV-077 four-world maximum-shape
//! matrix additionally composes fourteen equally adverse legs with all twenty-eight historical
//! source domains under both persisted leg orders and both observation orders, then requires
//! complete senior and terminal-claim exit. INV-059 additionally crosses two authenticated
//! liquidation episodes through all four opening transports, exact same-state/malformed rollback,
//! and a post-episode owner reduction. The stateful two-asset ADL matrix creates two later
//! authenticated deficits on a multi-leg account: the second either retains the first selected
//! residual or canonically removes it and selects the other asset, while the third repeats that
//! second selection at a fresh bounded slot. `v16_program_liquidation_composition_is_source_complete`
//! closes the current account-local liquidation surface by binding the exact engine pin, selector,
//! sizing, fee, OI, residual, Recovery, and dispatch proofs to the sole public crank plus the
//! maximum-shape CU witnesses. Caller-sized close partitions are source-excluded. A new engine pin,
//! liquidation ingress, selector branch, supported shape, or witness reopens the invariant.

use super::*;

#[derive(Debug)]
struct PostAdlTransferOutcome {
    opposing_loss: u128,
    claimant_gain: u128,
    final_raw_q: u128,
    final_oi_q: [u128; 2],
    backing_unliened_num: u128,
}

fn run_post_adl_transfer_world(probe_before_mark: bool) -> PostAdlTransferOutcome {
    const OPEN_PRICE: u64 = 1_000_000;
    const CLOSE_PRICE: u64 = 500_000;
    const OPEN_Q: u128 = POS_SCALE / 10;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(1, OPEN_PRICE);
    env.top_up_backing_bucket(0, 200_000, 10_000);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let successor_owner = Keypair::new();
    let relay_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    let successor = env.create_portfolio(&successor_owner);
    let relay = env.create_portfolio(&relay_owner);
    for (owner, portfolio) in [
        (&long_owner, long),
        (&short_owner, short),
        (&successor_owner, successor),
        (&relay_owner, relay),
    ] {
        env.deposit(owner, portfolio, 1_000_000);
    }
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        OPEN_Q as i128,
        OPEN_PRICE,
        0,
    );
    env.rebalance_reduce_with_cu(&long_owner, long, 0, OPEN_Q / 2);
    let adl = env.market_state().1;
    assert_eq!(adl.assets[0].oi_eff_long_q, OPEN_Q / 2);
    assert_eq!(adl.assets[0].oi_eff_short_q, OPEN_Q / 2);
    assert!(adl.assets[0].a_short < percolator::ADL_ONE);
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(short), 0)
            .basis_pos_q
            .unsigned_abs(),
        OPEN_Q
    );

    if probe_before_mark {
        let market_before = env.svm.get_account(&env.market).unwrap();
        let short_before = env.svm.get_account(&short).unwrap();
        let successor_before = env.svm.get_account(&successor).unwrap();
        let vault_before = env.svm.get_account(&env.vault).unwrap();
        env.svm.expire_blockhash();
        let error = env
            .try_trade_asset_with_cu(
                0,
                &short_owner,
                short,
                &successor_owner,
                successor,
                (OPEN_Q / 2) as i128,
                OPEN_PRICE,
                0,
            )
            .expect_err("post-ADL transfer must not reissue raw short basis before settlement");
        assert!(
            error.contains("Custom(21)") || error.contains("custom program error: 0x15"),
            "post-ADL transfer reached the wrong gate: {error}"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&short).unwrap(), short_before);
        assert_eq!(env.svm.get_account(&successor).unwrap(), successor_before);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    }

    let account_value = |account: &percolator::PortfolioAccountV16Account| {
        account.capital.get() as i128 + account.pnl.get()
    };
    let long_value_before = account_value(&env.portfolio_state(long));
    let short_value_before = account_value(&env.portfolio_state(short));
    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, CLOSE_PRICE);
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
    let long_value_after = account_value(&env.portfolio_state(long));
    let short_value_after = account_value(&env.portfolio_state(short));
    let opposing_loss = (long_value_before - long_value_after) as u128;
    let claimant_gain = (short_value_after - short_value_before) as u128;
    assert!(opposing_loss > 0);
    assert_eq!(claimant_gain, opposing_loss);

    let market_before_transfer = env.svm.get_account(&env.market).unwrap();
    let short_before_transfer = env.svm.get_account(&short).unwrap();
    let relay_before_transfer = env.svm.get_account(&relay).unwrap();
    let vault_before_transfer = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let error = env
        .try_trade_asset_with_cu(
            0,
            &short_owner,
            short,
            &relay_owner,
            relay,
            (OPEN_Q / 2) as i128,
            CLOSE_PRICE,
            0,
        )
        .expect_err("post-mark transfer must not detach a post-ADL profitable claim");
    assert!(
        error.contains("Custom(21)") || error.contains("custom program error: 0x15"),
        "post-mark transfer reached the wrong gate: {error}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_transfer
    );
    assert_eq!(env.svm.get_account(&short).unwrap(), short_before_transfer);
    assert_eq!(env.svm.get_account(&relay).unwrap(), relay_before_transfer);
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before_transfer
    );

    let before_exit = env.market_state().1;
    let leg_before_exit = active_leg_for_asset(&env.portfolio_state(short), 0);
    let raw_before_exit = leg_before_exit.basis_pos_q.unsigned_abs();
    let effective_before_exit =
        reference_current_epoch_effective_abs(&before_exit, leg_before_exit);
    let exit_q = OPEN_Q / 4;
    let expected_effective_after = effective_before_exit - exit_q;
    let expected_raw_after = reference_raw_basis_for_current_effective(
        &before_exit,
        leg_before_exit,
        expected_effective_after,
    );
    let exit_cu = env.rebalance_reduce_with_cu(&short_owner, short, 0, exit_q);
    assert_cu_within("post-ADL short owner reduction", exit_cu, CUSTODY_CU_LIMIT);
    let after_exit = env.market_state().1;
    let final_raw_q = active_leg_for_asset(&env.portfolio_state(short), 0)
        .basis_pos_q
        .unsigned_abs();
    assert_eq!(final_raw_q, expected_raw_after);
    assert!(
        raw_before_exit - final_raw_q >= exit_q,
        "an effective-unit reduction must remove at least that much stale raw basis"
    );
    assert_eq!(
        after_exit.assets[0].oi_eff_long_q,
        before_exit.assets[0].oi_eff_long_q - exit_q
    );
    assert_eq!(
        after_exit.assets[0].oi_eff_short_q,
        before_exit.assets[0].oi_eff_short_q - exit_q
    );
    let backing_after = after_exit.source_backing_buckets[0];
    PostAdlTransferOutcome {
        opposing_loss,
        claimant_gain,
        final_raw_q,
        final_oi_q: [
            after_exit.assets[0].oi_eff_long_q,
            after_exit.assets[0].oi_eff_short_q,
        ],
        backing_unliened_num: backing_after.fresh_unliened_backing_num,
    }
}

#[test]
fn v16_program_post_adl_transfer_extraction_is_rejected_before_backing_drain() {
    let control = run_post_adl_transfer_world(false);
    let probed = run_post_adl_transfer_world(true);
    assert_eq!(probed.opposing_loss, control.opposing_loss);
    assert_eq!(probed.claimant_gain, control.claimant_gain);
    assert_eq!(probed.final_raw_q, control.final_raw_q);
    assert_eq!(probed.final_oi_q, control.final_oi_q);
    assert_eq!(
        probed.backing_unliened_num, control.backing_unliened_num,
        "a rejected transfer probe cannot consume additional provider backing"
    );
}

#[test]
fn v16_program_post_adl_transfer_rejects_phantom_value_and_preserves_owner_progress() {
    const PRICE: u64 = 100;
    const OPEN_Q: i128 = 13_000 * POS_SCALE as i128;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        h_max: 6_480_000,
        initial_price: PRICE,
        maintenance_margin_bps: 500,
        initial_margin_bps: 500,
        min_nonzero_mm_req: 59_900,
        min_nonzero_im_req: 60_000,
        max_price_move_bps_per_slot: 24,
        max_accrual_dt_slots: 20,
        max_abs_funding_e9_per_slot: 0,
        min_funding_lifetime_slots: 10_000_000,
        liquidation_fee_bps: 0,
        maintenance_fee_per_slot: 2_700,
        ..V16CuMarketParams::default()
    });
    env.top_up_backing_bucket(1, 100_000, 10_000);
    let survivor_owner = Keypair::new();
    let liquidated_owner = Keypair::new();
    let successor_owner = Keypair::new();
    let keeper_owner = Keypair::new();
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, PRICE);
    env.svm.warp_to_slot(8);
    let survivor = env.create_portfolio(&survivor_owner);
    let liquidated = env.create_portfolio(&liquidated_owner);
    let keeper = env.create_portfolio(&keeper_owner);
    env.deposit(&survivor_owner, survivor, 100_000);
    env.deposit(&liquidated_owner, liquidated, 118_900 + 7 * 2_700);

    env.trade_with_cu(
        &survivor_owner,
        survivor,
        &liquidated_owner,
        liquidated,
        OPEN_Q,
        PRICE,
        0,
    );
    env.svm.warp_to_slot(27);
    env.crank(
        keeper,
        ProgInstruction::PermissionlessCrank {
            now_slot: 27,
            observations: crank_observations(0),
        },
    );
    env.svm.warp_to_slot(35);
    env.sync_maintenance_fee_with_cu(liquidated, None, 35);
    env.crank_steps(
        liquidated,
        ProgInstruction::PermissionlessCrank {
            now_slot: 35,
            observations: crank_observations(0),
        },
        2,
    );

    let adl = env.market_state().1;
    let effective_q = adl.assets[0].oi_eff_long_q;
    let raw_q = active_leg_for_asset(&env.portfolio_state(survivor), 0)
        .basis_pos_q
        .unsigned_abs();
    assert_eq!(raw_q, OPEN_Q.unsigned_abs());
    assert!(effective_q < raw_q);
    assert!(adl.assets[0].a_long < percolator::ADL_ONE);

    // Create the fresh recipient at the transfer slot so maintenance-debt ordering cannot obscure
    // this matrix's independent post-ADL basis-transfer predicate.
    let successor = env.create_portfolio(&successor_owner);
    env.deposit(&successor_owner, successor, 100_000);
    env.svm.expire_blockhash();
    let market_before = env.svm.get_account(&env.market).unwrap();
    let survivor_before = env.svm.get_account(&survivor).unwrap();
    let successor_before = env.svm.get_account(&successor).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let error = env
        .try_trade_asset_with_cu(
            0,
            &survivor_owner,
            survivor,
            &successor_owner,
            successor,
            -((raw_q / 2) as i128),
            PRICE,
            0,
        )
        .expect_err("fixed engine must reject post-ADL raw-basis transfer");
    assert!(
        error.contains("Custom(21)") || error.contains("custom program error: 0x15"),
        "post-ADL transfer reached the wrong gate: {error}"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&survivor).unwrap(), survivor_before);
    assert_eq!(env.svm.get_account(&successor).unwrap(), successor_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    let before_exit = env.market_state().1;
    let leg_before_exit = active_leg_for_asset(&env.portfolio_state(survivor), 0);
    let exit_q = effective_q.min(raw_q / 2).max(1);
    let account_effective_before =
        reference_current_epoch_effective_abs(&before_exit, leg_before_exit);
    let expected_raw_after = reference_raw_basis_for_current_effective(
        &before_exit,
        leg_before_exit,
        account_effective_before - exit_q,
    );
    let exit_cu = env.rebalance_reduce_with_cu(&survivor_owner, survivor, 0, exit_q);
    assert_cu_within(
        "post-ADL survivor owner reduction",
        exit_cu,
        CUSTODY_CU_LIMIT,
    );
    let after_exit = env.market_state().1;
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(survivor), 0)
            .basis_pos_q
            .unsigned_abs(),
        expected_raw_after
    );
    assert_eq!(
        after_exit.assets[0].oi_eff_long_q,
        before_exit.assets[0].oi_eff_long_q - exit_q
    );
    assert_eq!(
        after_exit.assets[0].oi_eff_short_q,
        before_exit.assets[0].oi_eff_short_q - exit_q
    );
}

#[test]
fn v16_program_reset_carry_liquidation_matrix_preserves_progress() {
    const ASSET: u16 = 1;

    let mut params = V16CuMarketParams::default();
    params.max_portfolio_assets = 2;
    params.initial_price = 1;
    params.max_price_move_bps_per_slot = 10_000;
    let mut env = V16CuEnv::new_with_init_params(params);
    env.configure_auth_mark_for_asset_as_admin(ASSET, 0, 1);

    let l1o = Keypair::new();
    let l2o = Keypair::new();
    let l3o = Keypair::new();
    let l4o = Keypair::new();
    let l5o = Keypair::new();
    let s1o = Keypair::new();
    let s2o = Keypair::new();
    let s3o = Keypair::new();
    let neutral_owner = Keypair::new();
    let l1 = env.create_portfolio(&l1o);
    let l2 = env.create_portfolio(&l2o);
    let l3 = env.create_portfolio(&l3o);
    let l4 = env.create_portfolio(&l4o);
    let l5 = env.create_portfolio(&l5o);
    let s1 = env.create_portfolio(&s1o);
    let s2 = env.create_portfolio(&s2o);
    let s3 = env.create_portfolio(&s3o);
    let neutral = env.create_portfolio(&neutral_owner);

    for (owner, portfolio, deposit) in [
        (&l1o, l1, 1_000),
        (&l2o, l2, 1_000),
        (&l3o, l3, 1_000),
        (&l4o, l4, 1_000),
        (&l5o, l5, 1_000),
        (&s1o, s1, 2),
        (&s2o, s2, 5),
        (&s3o, s3, 1_000),
    ] {
        env.deposit(owner, portfolio, deposit);
    }
    for (long_owner, long, short_owner, short, quantity) in [
        (&l1o, l1, &s1o, s1, 1_897_305),
        (&l2o, l2, &s1o, s1, 102_695),
        (&l2o, l2, &s2o, s2, 666_301),
        (&l3o, l3, &s2o, s2, 65_831),
        (&l4o, l4, &s2o, s2, 430_043),
        (&l4o, l4, &s3o, s3, 1_130_061),
        (&l5o, l5, &s3o, s3, 767_244),
    ] {
        env.trade_asset_with_cu(ASSET, long_owner, long, short_owner, short, quantity, 1, 0);
    }

    for (slot, mark) in [(1u64, 2u64), (2, 3), (3, 4), (4, 5), (5, 6)] {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_for_asset_as_admin(ASSET, slot, mark);
        env.crank(
            s3,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(ASSET),
            },
        );
    }
    for _ in 0..4 {
        if !has_active_leg_for_asset(&env.portfolio_state(s1), ASSET as usize) {
            break;
        }
        env.crank(
            s1,
            ProgInstruction::PermissionlessCrank {
                now_slot: 5,
                observations: crank_observations(ASSET),
            },
        );
    }
    let first = env.market_state().1;
    assert!(!has_active_leg_for_asset(
        &env.portfolio_state(s1),
        ASSET as usize
    ));
    assert_eq!(first.mode, MarketModeV16::Live);
    assert_eq!(
        first.assets[ASSET as usize].social_loss_remainder_long_num,
        322_760
    );
    assert_ne!(first.assets[ASSET as usize].b_long_num, 0);

    for _ in 0..8 {
        let leg = active_leg_for_asset(&env.portfolio_state(l1), ASSET as usize);
        if !leg.b_stale && leg.b_snap == env.market_state().1.assets[ASSET as usize].b_long_num {
            break;
        }
        env.crank(
            l1,
            ProgInstruction::PermissionlessCrank {
                now_slot: 5,
                observations: crank_observations(ASSET),
            },
        );
    }
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(l1), ASSET as usize).b_rem,
        percolator::SOCIAL_LOSS_DEN - 121_035
    );
    let before_owner_reduction = env.market_state().1;
    let l1_leg_before = active_leg_for_asset(&env.portfolio_state(l1), ASSET as usize);
    let l1_effective_before =
        reference_current_epoch_effective_abs(&before_owner_reduction, l1_leg_before);
    assert!(
        l1_effective_before < l1_leg_before.basis_pos_q.unsigned_abs(),
        "fixture must exercise non-unit ADL conversion"
    );
    env.rebalance_reduce_with_cu(&l1o, l1, ASSET, 1_897_305);
    assert!(!has_active_leg_for_asset(
        &env.portfolio_state(l1),
        ASSET as usize
    ));
    let carry_state = env.market_state().1;
    assert_eq!(
        carry_state.assets[ASSET as usize].oi_eff_long_q,
        before_owner_reduction.assets[ASSET as usize].oi_eff_long_q - l1_effective_before
    );
    assert_eq!(
        carry_state.assets[ASSET as usize].oi_eff_short_q,
        before_owner_reduction.assets[ASSET as usize].oi_eff_short_q - l1_effective_before
    );
    assert_eq!(
        carry_state.assets[ASSET as usize].social_loss_dust_long_num,
        percolator::SOCIAL_LOSS_DEN - 121_035
    );

    env.svm.warp_to_slot(6);
    env.push_auth_mark_for_asset_as_admin(ASSET, 6, 7);
    env.crank(
        s2,
        ProgInstruction::PermissionlessCrank {
            now_slot: 6,
            observations: crank_observations(ASSET),
        },
    );
    assert!(has_active_leg_for_asset(
        &env.portfolio_state(s2),
        ASSET as usize
    ));
    assert!(health_cert(&env.portfolio_state(s2)).certified_liq_deficit != 0);

    let mut successful_steps = 0usize;
    for slot in 6..=8u64 {
        if !has_active_leg_for_asset(&env.portfolio_state(s2), ASSET as usize) {
            break;
        }
        env.svm.warp_to_slot(slot);
        env.crank_steps_after_market_catchup(
            neutral,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(ASSET),
            },
            1,
        );
        let fixed_market = env.svm.get_account(&env.market).unwrap();
        let fixed_loser = env.svm.get_account(&s2).unwrap();
        let fixed_vault = env.svm.get_account(&env.vault).unwrap();
        env.svm.expire_blockhash();
        let before = env.market_state().1;
        let before_basis = active_leg_for_asset(&env.portfolio_state(s2), ASSET as usize)
            .basis_pos_q
            .unsigned_abs();
        let liquidation_cu = env
            .send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(ASSET),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(s2, false),
                ],
                &[],
            )
            .expect("fractional reset carry must not abort permissionless liquidation");
        assert_cu_within(
            "fractional reset-carry liquidation",
            liquidation_cu,
            CRANK_CU_LIMIT,
        );
        successful_steps += 1;
        let after = env.market_state().1;
        let after_basis = if has_active_leg_for_asset(&env.portfolio_state(s2), ASSET as usize) {
            active_leg_for_asset(&env.portfolio_state(s2), ASSET as usize)
                .basis_pos_q
                .unsigned_abs()
        } else {
            0
        };
        assert!(
            after.assets[ASSET as usize].oi_eff_long_q
                < before.assets[ASSET as usize].oi_eff_long_q
                || after.assets[ASSET as usize].oi_eff_short_q
                    < before.assets[ASSET as usize].oi_eff_short_q
                || after_basis < before_basis,
            "every accepted target crank must strictly reduce liquidation risk"
        );
        assert_ne!(env.svm.get_account(&env.market).unwrap(), fixed_market);
        assert_ne!(env.svm.get_account(&s2).unwrap(), fixed_loser);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), fixed_vault);
    }
    assert!(successful_steps > 0);
    assert_eq!(env.market_state().1.mode, MarketModeV16::Live);
    assert!(!has_active_leg_for_asset(
        &env.portfolio_state(s2),
        ASSET as usize
    ));
    let liquidation_progressed = env.market_state().1;
    assert_eq!(
        liquidation_progressed.assets[ASSET as usize].oi_eff_long_q,
        liquidation_progressed.assets[ASSET as usize].oi_eff_short_q
    );
    assert!(liquidation_progressed.assets[ASSET as usize].oi_eff_long_q > 0);

    // The corrected engine no longer subtracts a raw ADL basis amount from pooled effective OI.
    // Explicitly close the other live owners through owner-signed reductions, using an independent
    // full-width oracle for each account's current effective quantity. Supplying retained raw basis
    // is only a best-effort work budget; the engine clamps it to economically live matched OI.
    let vault_before_matched_exits = env.svm.get_account(&env.vault).unwrap();
    for (name, long_owner, long) in [
        ("l2", &l2o, l2),
        ("l3", &l3o, l3),
        ("l4", &l4o, l4),
        ("l5", &l5o, l5),
    ] {
        let before = env.market_state().1;
        let long_leg = active_leg_for_asset(&env.portfolio_state(long), ASSET as usize);
        let raw_before_exit = long_leg.basis_pos_q.unsigned_abs();
        let exit_q = reference_current_epoch_effective_abs(&before, long_leg)
            .min(before.assets[ASSET as usize].oi_eff_long_q)
            .min(before.assets[ASSET as usize].oi_eff_short_q);
        assert!(exit_q > 0);
        let exit_cu = env.rebalance_reduce_with_cu(
            long_owner,
            long,
            ASSET,
            long_leg.basis_pos_q.unsigned_abs(),
        );
        assert_cu_within(
            &format!("fractional carry {name} effective owner exit"),
            exit_cu,
            CUSTODY_CU_LIMIT,
        );
        let after = env.market_state().1;
        assert_eq!(
            after.assets[ASSET as usize].oi_eff_long_q,
            before.assets[ASSET as usize].oi_eff_long_q - exit_q
        );
        assert_eq!(
            after.assets[ASSET as usize].oi_eff_short_q,
            before.assets[ASSET as usize].oi_eff_short_q - exit_q
        );
        let raw_after_exit = if has_active_leg_for_asset(&env.portfolio_state(long), ASSET as usize)
        {
            active_leg_for_asset(&env.portfolio_state(long), ASSET as usize)
                .basis_pos_q
                .unsigned_abs()
        } else {
            0
        };
        assert!(raw_after_exit < raw_before_exit);
        assert_eq!(
            env.svm.get_account(&env.vault).unwrap(),
            vault_before_matched_exits
        );
    }

    let progressed = env.market_state().1;
    assert_eq!(progressed.assets[ASSET as usize].oi_eff_long_q, 0);
    assert_eq!(progressed.assets[ASSET as usize].oi_eff_short_q, 0);
    assert_eq!(
        progressed.assets[ASSET as usize].social_loss_remainder_long_num,
        0
    );
    assert!(
        progressed.assets[ASSET as usize].social_loss_dust_long_num < percolator::SOCIAL_LOSS_DEN
    );
    assert_ne!(
        progressed.assets[ASSET as usize].explicit_unallocated_loss_long,
        0
    );
    assert_eq!(progressed.vault as u64, env.token_amount(env.vault));

    for portfolio in [l1, l2, l3, l4, l5, s1, s2, s3] {
        for _ in 0..8 {
            if !has_active_leg_for_asset(&env.portfolio_state(portfolio), ASSET as usize) {
                break;
            }
            env.crank(
                portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: 8,
                    observations: crank_observations(ASSET),
                },
            );
        }
        assert!(
            !has_active_leg_for_asset(&env.portfolio_state(portfolio), ASSET as usize),
            "every prior-epoch residue must detach in bounded public cranks"
        );
    }
    let reset = env.market_state().1;
    if reset.assets[ASSET as usize].mode_long == SideModeV16::ResetPending {
        let cu = env.finalize_reset_side_with_cu(ASSET, 0);
        assert_cu_within(
            "fractional carry long reset finalization",
            cu,
            CUSTODY_CU_LIMIT,
        );
    }
    if env.market_state().1.assets[ASSET as usize].mode_short == SideModeV16::ResetPending {
        let cu = env.finalize_reset_side_with_cu(ASSET, 1);
        assert_cu_within(
            "fractional carry short reset finalization",
            cu,
            CUSTODY_CU_LIMIT,
        );
    }
    let terminal_ready = env.market_state().1;
    assert_eq!(terminal_ready.mode, MarketModeV16::Live);
    assert_eq!(
        terminal_ready.assets[ASSET as usize].lifecycle,
        AssetLifecycleV16::Active
    );
    assert_eq!(
        terminal_ready.assets[ASSET as usize].mode_long,
        SideModeV16::Normal
    );
    assert_eq!(
        terminal_ready.assets[ASSET as usize].mode_short,
        SideModeV16::Normal
    );
    assert_eq!(terminal_ready.assets[ASSET as usize].k_epoch_start_long, 0);
    assert_eq!(terminal_ready.assets[ASSET as usize].k_epoch_start_short, 0);
    assert_eq!(
        terminal_ready.assets[ASSET as usize].f_epoch_start_long_num,
        0
    );
    assert_eq!(
        terminal_ready.assets[ASSET as usize].f_epoch_start_short_num,
        0
    );
    assert_eq!(
        terminal_ready.assets[ASSET as usize].b_epoch_start_long_num,
        0
    );
    assert_eq!(
        terminal_ready.assets[ASSET as usize].b_epoch_start_short_num,
        0
    );

    for (owner, portfolio) in [
        (&l1o, l1),
        (&l2o, l2),
        (&l3o, l3),
        (&l4o, l4),
        (&l5o, l5),
        (&s1o, s1),
        (&s2o, s2),
        (&s3o, s3),
    ] {
        let pnl = env.portfolio_state(portfolio).pnl.get();
        if pnl > 0 {
            let refresh_cu = env.crank_steps(
                portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: 8,
                    observations: crank_observations(ASSET),
                },
                2,
            );
            assert_cu_within(
                "fractional carry terminal claim refresh",
                refresh_cu,
                CRANK_CU_LIMIT,
            );
            let cu = env.convert_released_pnl_with_cu(owner, portfolio, pnl as u128);
            assert_cu_within(
                "fractional carry terminal PnL conversion",
                cu,
                CUSTODY_CU_LIMIT,
            );
            assert_eq!(env.portfolio_state(portfolio).pnl.get(), 0);
        }
    }
    assert_eq!(env.market_state().1.pnl_pos_tot, 0);

    let source_domain = ASSET * 2 + 1;
    let provider_obligated = env.market_state().1;
    let source = provider_obligated.source_credit[source_domain as usize];
    assert_eq!(source.positive_claim_bound_num, 0);
    assert_eq!(source.exact_positive_claim_num, 0);
    assert!(source.provider_receivable_num > 0);
    assert_eq!(source.provider_receivable_num, source.spent_backing_num);
    assert_eq!(source.provider_receivable_num % BOUND_SCALE, 0);
    let refill_atoms = source.provider_receivable_num / BOUND_SCALE;
    let refill_expiry =
        provider_obligated.source_backing_buckets[source_domain as usize].expiry_slot;
    env.top_up_backing_bucket(source_domain, refill_atoms, refill_expiry);

    let provider_ready = env.market_state().1;
    let refilled_source = provider_ready.source_credit[source_domain as usize];
    assert_eq!(refilled_source.provider_receivable_num, 0);
    assert_eq!(refilled_source.spent_backing_num, source.spent_backing_num);
    assert_eq!(
        refilled_source.fresh_reserved_backing_num,
        source.fresh_reserved_backing_num + source.provider_receivable_num
    );
    assert_eq!(refilled_source.fresh_reserved_backing_num % BOUND_SCALE, 0);
    let provider_principal = refilled_source.fresh_reserved_backing_num / BOUND_SCALE;
    let provider_token = env.token_account(env.admin.pubkey(), 0);
    assert!(
        provider_ready.bankruptcy_hlock_active,
        "the regression must retain the global bankruptcy history bit"
    );
    assert_eq!(
        provider_ready.negative_pnl_account_count, 0,
        "all active negative accounts must settle before backing withdrawal"
    );
    assert_eq!(
        provider_ready.pending_domain_loss_barriers[source_domain as usize], 0,
        "the selected source domain must have no live loss barrier"
    );
    let provider_withdraw_cu = env.withdraw_backing_bucket_to_admin_token_with_cu(
        provider_token,
        source_domain,
        provider_principal,
    );
    assert_cu_within(
        "fractional carry terminal provider settlement",
        provider_withdraw_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(env.token_amount(provider_token), provider_principal as u64);
    let provider_settled = env.market_state().1;
    assert_eq!(
        provider_settled.source_credit[source_domain as usize].provider_receivable_num,
        0
    );
    assert_eq!(
        provider_settled.source_backing_buckets[source_domain as usize],
        percolator::BackingBucketV16 {
            market_id: provider_settled.assets[ASSET as usize].market_id,
            ..percolator::BackingBucketV16::EMPTY
        }
    );
    assert_eq!(
        provider_settled.source_credit[source_domain as usize].spent_backing_num,
        source.spent_backing_num,
        "principal withdrawal preserves the cumulative spent-backing audit"
    );

    let custody_before_retirement = env.svm.get_account(&env.vault).unwrap();
    let drain_cu = env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_DRAIN_ONLY,
        ASSET,
        0,
        0,
    );
    assert_cu_within("fractional carry DrainOnly", drain_cu, CUSTODY_CU_LIMIT);
    assert_eq!(
        env.market_state().1.assets[ASSET as usize].lifecycle,
        AssetLifecycleV16::DrainOnly
    );
    let retire_cu = env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_RETIRE,
        ASSET,
        8,
        0,
    );
    assert_cu_within("fractional carry retirement", retire_cu, CUSTODY_CU_LIMIT);
    let retired = env.market_state().1;
    assert_eq!(
        retired.assets[ASSET as usize].lifecycle,
        AssetLifecycleV16::Retired
    );
    assert_eq!(
        retired.assets[ASSET as usize].social_loss_remainder_long_num,
        0
    );
    assert_eq!(
        retired.assets[ASSET as usize].social_loss_remainder_short_num,
        0
    );
    assert_eq!(retired.assets[ASSET as usize].social_loss_dust_long_num, 0);
    assert_eq!(retired.assets[ASSET as usize].social_loss_dust_short_num, 0);
    assert_eq!(
        retired.assets[ASSET as usize].explicit_unallocated_loss_long,
        0
    );
    assert_eq!(
        retired.assets[ASSET as usize].explicit_unallocated_loss_short,
        0
    );
    assert_eq!(
        retired.source_credit[source_domain as usize],
        percolator::SourceCreditStateV16::EMPTY,
        "retirement consumes only the historical spent-backing audit"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        custody_before_retirement
    );
}

#[test]
fn v16_attack_liquidation_reward_share_without_tail_still_progresses() {
    const LIQ_SLOT: u64 = 30;

    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.update_liquidation_fee_policy_with_cu(5_000);
    env.configure_auth_mark_with_cu(0, 1_000_000);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let keeper = Keypair::new();
    env.ensure_signer_account(keeper.pubkey());
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 100_000_000);
    env.deposit(&short_owner, short, 100_000);
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        1_000_000,
        0,
    );

    for slot in 1..=LIQ_SLOT {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_with_cu(slot, 2_000_000);
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(short, false),
            ],
            &[],
        );
    }
    assert!(
        health_cert(&env.portfolio_state(short)).certified_liq_deficit != 0,
        "setup must make the target liquidatable before the no-tail reward-share crank"
    );

    let (_, before) = env.market_state();
    env.svm.expire_blockhash();
    let accepted = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: LIQ_SLOT,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(keeper.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short, false),
        ],
        &[&keeper],
    );
    assert!(
        accepted.is_ok(),
        "reward-enabled liquidation must remain live when the keeper omits the optional reward tail: {accepted:?}"
    );

    let (_, after) = env.market_state();
    assert!(
        after.insurance > before.insurance,
        "without a reward tail, the liquidation fee is retained by insurance"
    );
    assert_eq!(
        after.vault, before.vault,
        "liquidation reward sharing is an internal fee split, not a vault mint"
    );
    assert_eq!(
        after.vault as u64,
        env.token_amount(env.vault),
        "vault accounting remains tied to SPL custody"
    );
    assert!(
        after.vault >= after.c_tot + after.insurance,
        "senior conservation"
    );
}

// keeper supplies no quantity, so repeated minimum-fee chunk selection is not representable.
#[test]
fn v16_program_partial_liquidation_bounded_and_conserves() {
    let mut env = V16CuEnv::new();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 250);
    env.configure_ewma_mark_with_cu(0, 100, 1, 0);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        100,
        0,
    );
    for (slot, mark) in [(1u64, 300u64), (2, 800)] {
        env.svm.warp_to_slot(slot);
        env.push_ewma_mark_with_cu(slot, mark);
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(short_account, false),
            ],
            &[],
        );
    }
    let (_, g_pre) = env.market_state();
    let oi_pre = g_pre.assets[0].oi_eff_short_q;
    let _ = env.send_crank_if_actionable(
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short_account, false),
        ],
        &[],
    );
    let (_, g_post) = env.market_state();
    let closed = oi_pre.saturating_sub(g_post.assets[0].oi_eff_short_q);
    assert!(closed <= oi_pre, "engine cannot close more than live OI");
    assert!(
        g_post.assets[0].oi_eff_short_q <= oi_pre,
        "OI never increased"
    );
    // conservation: vault unchanged (internal), accounting==real, senior conservation.
    assert_eq!(
        g_post.vault, g_pre.vault,
        "vault unchanged by partial liquidation"
    );
    assert_eq!(
        g_post.vault as u64,
        env.token_amount(env.vault),
        "accounting vault == real vault"
    );
    assert!(
        g_post.vault >= g_post.c_tot + g_post.insurance,
        "senior conservation"
    );
}

// force-close, no fee extraction, position intact.
#[test]
fn v16_program_healthy_account_not_liquidatable() {
    // maintenance 50% < initial 100% -> a freshly-opened account is well above maintenance.
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 5_000, 10_000, 1_000);
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    let basis0 = env.portfolio_state(pa).legs[0].basis_pos_q.get();
    assert!(basis0 != 0, "la opened a position");
    let (_, g0) = env.market_state();

    // attacker tries to liquidate the healthy la (no adverse price move).
    env.svm.warp_to_slot(1);
    let _ = env.send_crank_if_actionable(
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(pa, false),
        ],
        &[],
    );

    // healthy account: position intact, no fee extracted, conservation.
    assert_eq!(
        env.portfolio_state(pa).legs[0].basis_pos_q.get(),
        basis0,
        "healthy account's position NOT force-closed by liquidation"
    );
    assert_eq!(
        env.portfolio_state(pa).capital.get(),
        1_000_000,
        "healthy account capital not docked a liquidation fee"
    );
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert_eq!(
        g1.assets[0].oi_eff_long_q, g1.assets[0].oi_eff_short_q,
        "OI still balanced (position intact)"
    );
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}

// extraction. Attempting to liquidate the losing leg must not unfairly drain the healthy account.
#[test]
fn v16_program_cross_margin_solvent_account_not_unfairly_liquidated() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    let cfg = |env: &mut V16CuEnv, ix: ProgInstruction| {
        send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ix,
            vec![
                AccountMeta::new(env.admin.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&env.admin],
        )
        .expect("mark cfg");
    };
    cfg(
        &mut env,
        ProgInstruction::ConfigureAuthMark {
            market_id: 0,
            observation_sequence: 1,
            asset_index: 1,
            now_slot: 0,
            initial_mark_e6: 100,
            authority_epoch: 0,
        },
    );
    let victim_owner = Keypair::new();
    let victim = env.create_portfolio(&victim_owner);
    let cp_owner = Keypair::new();
    let cp = env.create_portfolio(&cp_owner);
    env.deposit(&victim_owner, victim, 1_000_000);
    env.deposit(&cp_owner, cp, 1_000_000);
    // victim LONG asset0 and SHORT asset1 (cross-margined opposite exposures).
    env.trade_asset_with_cu(
        0,
        &victim_owner,
        victim,
        &cp_owner,
        cp,
        POS_SCALE as i128,
        100,
        0,
    );
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        1,
        &victim_owner,
        victim,
        &cp_owner,
        cp,
        -(POS_SCALE as i128),
        100,
        0,
    );
    // both marks up 10%: victim GAINS on asset0 (long), LOSES on asset1 (short) -> net ~flat.
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110);
    cfg(
        &mut env,
        ProgInstruction::PushAuthMark {
            market_id: 0,
            observation_sequence: 2,
            asset_index: 1,
            now_slot: 10,
            mark_e6: 110,
            authority_epoch: 0,
        },
    );
    for slot in [10u64, 11] {
        env.svm.warp_to_slot(slot);
        for ai in [0u16, 1] {
            for p in [victim, cp] {
                let _ = env.send_crank_if_actionable(
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: crank_observations_for_assets(&[ai, 1 - ai]),
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
    }
    let v_before = state::read_portfolio(&env.svm.get_account(&victim).unwrap().data).unwrap();
    let equity_before = v_before.capital.get() as i128 + v_before.pnl.get();
    let (_, g_before) = env.market_state();
    // attacker tries to liquidate the victim's LOSING leg (asset1).
    let _ = env.send_crank_if_actionable(
        ProgInstruction::PermissionlessCrank {
            now_slot: 11,
            observations: crank_observations(1),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(victim, false),
        ],
        &[],
    );
    let v_after = state::read_portfolio(&env.svm.get_account(&victim).unwrap().data).unwrap();
    let equity_after = v_after.capital.get() as i128 + v_after.pnl.get();
    let (_, g_after) = env.market_state();
    // the solvent victim's total equity is not reduced by the liquidation attempt (no unfair drain).
    assert!(
        equity_after >= equity_before,
        "solvent cross-margined victim equity not drained by liquidation attempt ({} -> {})",
        equity_before,
        equity_after
    );
    assert_eq!(
        g_after.vault, g_before.vault,
        "no tokens moved by liquidation attempt"
    );
    assert!(
        g_after.vault >= g_after.c_tot + g_after.insurance,
        "senior conservation"
    );
}

// nonzero liquidation_fee_bps (via production_risk_params, which satisfies the engine solvency envelope).
#[test]
fn v16_program_liquidation_cranker_reward_bounded_by_fee() {
    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.update_liquidation_fee_policy_with_cu(5_000); // cranker share = 50%
    env.configure_auth_mark_with_cu(0, 1_000_000);
    let lo = Keypair::new();
    let l = env.create_portfolio(&lo);
    let so = Keypair::new();
    let s = env.create_portfolio(&so);
    let co = Keypair::new();
    let c = env.create_portfolio(&co); // cranker (could be a self-liquidator)
    env.deposit(&lo, l, 100_000_000);
    env.deposit(&so, s, 100_000); // ~2x the 5% IM -> insolvent on a modest move
    env.deposit(&co, c, 1_000);
    env.trade_asset_with_cu(0, &lo, l, &so, s, POS_SCALE as i128, 1_000_000, 0);
    for slot in 1..=30u64 {
        env.svm.warp_to_slot(slot);
        let _ = env.push_auth_mark_with_cu(slot, 2_000_000);
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(s, false),
            ],
            &[],
        );
    }
    let c0 = env.portfolio_state(c).capital.get();
    let (_, g0) = env.market_state();

    // liquidate the insolvent short, crediting the cranker portfolio.
    env.svm.expire_blockhash();
    let r = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::PermissionlessCrank {
            now_slot: 30,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(co.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(s, false),
            AccountMeta::new(c, false),
        ],
        &[&co],
    );
    assert!(r.is_ok(), "liquidation with a fee should proceed: {:?}", r);
    let (_, g1) = env.market_state();
    let cranker_reward = env.portfolio_state(c).capital.get() as i128 - c0 as i128;
    let ins_delta = g1.insurance as i128 - g0.insurance as i128;
    let total_fee = cranker_reward + ins_delta;

    // non-vacuity: a real liquidation fee was charged and the cranker got a real reward.
    assert!(
        cranker_reward > 0,
        "cranker received a reward (non-vacuous), got {}",
        cranker_reward
    );
    assert!(total_fee > 0, "a liquidation fee was charged");
    // BOUND: the reward never exceeds the fee. The configured share rounds down and the
    // indivisible remainder stays in insurance, so odd fees still partition exactly.
    assert!(
        cranker_reward <= total_fee,
        "cranker reward must not exceed the fee (no profit beyond the fee)"
    );
    assert_eq!(
        cranker_reward,
        percolator_prog::policy_v16::fee_share_floor(total_fee as u128, 5_000)
            .expect("canonical liquidation reward arithmetic") as i128,
        "deployed liquidation reward uses the canonical host floor"
    );
    assert_eq!(
        ins_delta,
        total_fee - cranker_reward,
        "insurance receives the exact indivisible remainder"
    );
    assert!(
        cranker_reward < total_fee,
        "cranker gets < the full fee -> self-liquidation nets negative"
    );
    // NO MINT: the fee is internal (liquidated account -> cranker + insurance), vault unchanged.
    assert_eq!(g1.vault, g0.vault, "liquidation fee mints no vault tokens");
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(
        g1.vault >= g1.c_tot + g1.insurance,
        "senior conservation through fee'd liquidation"
    );
}

// configured cap. Protection: the total fee (cranker + insurance) never exceeds liquidation_fee_cap.
#[test]
fn v16_program_liquidation_fee_capped() {
    const CAP: u128 = 100;
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        liquidation_fee_cap: CAP, // tiny cap; 5bps * ~1e6 notional would be ~535 uncapped
        ..production_risk_params()
    });
    env.update_liquidation_fee_policy_with_cu(5_000);
    env.configure_auth_mark_with_cu(0, 1_000_000);
    let lo = Keypair::new();
    let l = env.create_portfolio(&lo);
    let so = Keypair::new();
    let s = env.create_portfolio(&so);
    let co = Keypair::new();
    let c = env.create_portfolio(&co);
    env.deposit(&lo, l, 100_000_000);
    env.deposit(&so, s, 100_000);
    env.deposit(&co, c, 1_000);
    env.trade_asset_with_cu(0, &lo, l, &so, s, POS_SCALE as i128, 1_000_000, 0);
    for slot in 1..=30u64 {
        env.svm.warp_to_slot(slot);
        let _ = env.push_auth_mark_with_cu(slot, 2_000_000);
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(s, false),
            ],
            &[],
        );
    }
    let c0 = env.portfolio_state(c).capital.get();
    let (_, g0) = env.market_state();

    env.svm.expire_blockhash();
    let r = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::PermissionlessCrank {
            now_slot: 30,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(co.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(s, false),
            AccountMeta::new(c, false),
        ],
        &[&co],
    );
    assert!(r.is_ok(), "liquidation should proceed: {:?}", r);
    let (_, g1) = env.market_state();
    let cranker_reward = (env.portfolio_state(c).capital.get() as i128 - c0 as i128) as u128;
    let ins_delta = (g1.insurance as i128 - g0.insurance as i128) as u128;
    let total_fee = cranker_reward + ins_delta;

    // CAP: the total liquidation fee is bounded by liquidation_fee_cap — NOT the uncapped bps*notional.
    assert!(total_fee > 0, "a fee was charged (non-vacuous)");
    assert!(
        total_fee <= CAP,
        "total liquidation fee {} must be capped at liquidation_fee_cap {}",
        total_fee,
        CAP
    );
    // the cap actually bit: 5bps of ~1e6 notional (~535) would have exceeded CAP=100 absent the cap.
    assert_eq!(
        total_fee, CAP,
        "the fee is the cap exactly (uncapped fee would be ~535 >> 100)"
    );
    assert_eq!(g1.vault, g0.vault, "fee is internal, no vault mint");
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}

// the first selected liquidation charges a fee; the second no-longer-liquidatable crank charges zero.
#[test]
fn v16_program_repeated_partial_liquidation_stops_charging_after_health_restored() {
    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.update_liquidation_fee_policy_with_cu(0); // all fee to insurance (clean single accumulator)
    env.configure_auth_mark_with_cu(0, 1_000_000);
    let lo = Keypair::new();
    let l = env.create_portfolio(&lo);
    let so = Keypair::new();
    let s = env.create_portfolio(&so);
    let neutral_owner = Keypair::new();
    let neutral = env.create_portfolio(&neutral_owner);
    env.deposit(&lo, l, 100_000_000);
    env.deposit(&so, s, 200_000); // enough to open 2*POS_SCALE at 5% IM, then go insolvent
    env.trade_asset_with_cu(0, &lo, l, &so, s, (2 * POS_SCALE) as i128, 1_000_000, 0);
    for slot in 1..=30u64 {
        env.svm.warp_to_slot(slot);
        let _ = env.push_auth_mark_with_cu(slot, 2_000_000);
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(neutral, false),
            ],
            &[],
        );
    }
    // Mark accrual above used an unrelated account, so the first measured victim action is real.
    let insurance_before = env.market_state().1.insurance;
    for _ in 0..4 {
        env.crank_if_actionable(
            s,
            ProgInstruction::PermissionlessCrank {
                now_slot: 30,
                observations: crank_observations(0),
            },
        );
        if env.market_state().1.insurance > insurance_before {
            break;
        }
    }
    let fee1 = env.market_state().1.insurance - insurance_before;
    let retry = env.crank_if_actionable(
        s,
        ProgInstruction::PermissionlessCrank {
            now_slot: 30,
            observations: crank_observations(0),
        },
    );

    // STOP: a real first liquidation charged a fee, but replaying at the healthy fixed point is an
    // instruction error with exact rollback, not a second fee or a successful no-op.
    assert!(
        fee1 > 0,
        "first partial charges a fee (non-vacuous), fee1={}",
        fee1
    );
    assert!(
        retry.is_none(),
        "second no-longer-liquidatable close hint must reject after fee1={fee1}"
    );
    let g = env.market_state().1;
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
}

// transfer only). The existing bounded test only exercises a 50% share (exact cranker/insurance split).
#[test]
fn v16_program_liquidation_full_cranker_share_takes_whole_fee_no_mint() {
    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.update_liquidation_fee_policy_with_cu(10_000); // 100% cranker share — the boundary
    env.configure_auth_mark_with_cu(0, 1_000_000);
    let lo = Keypair::new();
    let l = env.create_portfolio(&lo);
    let so = Keypair::new();
    let s = env.create_portfolio(&so);
    let co = Keypair::new();
    let c = env.create_portfolio(&co); // cranker
    env.deposit(&lo, l, 100_000_000);
    env.deposit(&so, s, 100_000); // thin short -> insolvent on a price doubling
    env.deposit(&co, c, 1_000);
    env.trade_asset_with_cu(0, &lo, l, &so, s, POS_SCALE as i128, 1_000_000, 0);
    for slot in 1..=30u64 {
        env.svm.warp_to_slot(slot);
        let _ = env.push_auth_mark_with_cu(slot, 2_000_000);
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(s, false),
            ],
            &[],
        );
    }
    let c0 = env.portfolio_state(c).capital.get();
    let (_, g0) = env.market_state();

    // Liquidate the insolvent short, crediting the cranker portfolio.
    env.svm.expire_blockhash();
    let r = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::PermissionlessCrank {
            now_slot: 30,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(co.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(s, false),
            AccountMeta::new(c, false),
        ],
        &[&co],
    );
    assert!(r.is_ok(), "liquidation with a fee should proceed: {r:?}");

    let (_, g1) = env.market_state();
    let cranker_reward = env.portfolio_state(c).capital.get() as i128 - c0 as i128;
    let ins_delta = g1.insurance as i128 - g0.insurance as i128;
    let total_fee = cranker_reward + ins_delta;

    assert!(
        cranker_reward > 0,
        "cranker received a real reward (non-vacuous): {cranker_reward}"
    );
    assert!(total_fee > 0, "a liquidation fee was charged");
    // 100% share: the cranker takes the ENTIRE fee; insurance gets nothing.
    assert_eq!(
        ins_delta, 0,
        "100%% share: no liquidation fee retained to insurance"
    );
    assert_eq!(
        cranker_reward, total_fee,
        "100%% share: cranker reward == the whole fee"
    );
    // The reward never exceeds the fee (no profit/mint beyond what was charged).
    assert!(
        cranker_reward <= total_fee,
        "cranker reward bounded by the fee"
    );
    // The fee is internal (liquidated account -> cranker), minting no vault tokens.
    assert_eq!(g1.vault, g0.vault, "liquidation fee mints no vault tokens");
    assert!(
        g1.vault >= g1.c_tot + g1.insurance,
        "senior conservation through 100%-share liquidation"
    );
}

#[test]
fn v16_bpf_permissionless_liquidation_is_bounded() {
    let mut env = V16CuEnv::new();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 250);
    env.configure_ewma_mark_with_cu(0, 100, 1, 0);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(1);
    env.push_ewma_mark_with_cu(1, 300);
    let liquidation_cu = env.crank_steps(
        short_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
        2,
    );
    println!("v16 liquidation crank CU: {liquidation_cu}");
    assert!(
        liquidation_cu <= CRANK_CU_LIMIT,
        "liquidation CU {} exceeded limit {}",
        liquidation_cu,
        CRANK_CU_LIMIT
    );

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let short_data = env.svm.get_account(&short_account).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    let short = state::read_portfolio(&short_data).unwrap();
    assert_eq!(group.slot_last, 1);
    assert_eq!(group.assets[0].effective_price, 200);
    let remaining_q = if has_active_leg_for_asset(&short, 0) {
        active_leg_for_asset(&short, 0).basis_pos_q.unsigned_abs()
    } else {
        0
    };
    assert!(remaining_q < POS_SCALE, "liquidation strictly reduces risk");
    assert_eq!(
        health_cert(&short).certified_liq_deficit,
        0,
        "engine-selected partial restores maintenance health"
    );
}

// Engine-selected liquidation of a bankrupt account can never over-close into phantom OI or create
// value. The public instruction carries no liquidation quantity.
#[test]
fn v16_engine_selected_liquidation_cannot_overclose_or_create_value() {
    let mut env = V16CuEnv::new();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 250); // tiny -> insolvent on up-move
    env.configure_ewma_mark_with_cu(0, 100, 1, 0);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        100,
        0,
    );
    for (slot, mark) in [(1u64, 300u64), (2, 800)] {
        env.svm.warp_to_slot(slot);
        env.push_ewma_mark_with_cu(slot, mark);
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(short_account, false),
            ],
            &[],
        );
    }
    let (_, g_pre) = env.market_state();
    let oi_pre = g_pre.assets[0].oi_eff_long_q;
    let _ = env.send_crank_if_actionable(
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short_account, false),
        ],
        &[],
    );
    let (_, g) = env.market_state();
    assert!(g.assets[0].oi_eff_short_q <= oi_pre, "short OI never grows");
    assert!(g.assets[0].oi_eff_long_q <= oi_pre, "long OI never grows");
    assert_eq!(g.vault, 1_000_250, "liquidation creates no vault value");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    // the short is fully closed (position gone), not over-closed into a phantom opposite position.
    let sh = state::read_portfolio(&env.svm.get_account(&short_account).unwrap().data).unwrap();
    assert!(
        percolator::active_bitmap_is_empty(active_bitmap(&sh)),
        "short position fully closed, no phantom flip"
    );
}

// A deeply insolvent single-leg account cannot be partially liquidated while leaving uncovered open
// risk. With no keeper size input, the engine selects a full close, books the residual, and preserves
// custody and balanced OI.
#[test]
fn v16_engine_selected_deep_insolvency_close_is_full_and_conserving() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    let la = Keypair::new();
    let a = env.create_portfolio(&la); // long winner
    let lb = Keypair::new();
    let b = env.create_portfolio(&lb); // short, will be DEEPLY bankrupt
    env.deposit(&la, a, 1_000);
    env.deposit(&lb, b, 1_000);
    env.trade_asset_with_cu(0, &la, a, &lb, b, (7 * POS_SCALE) as i128, 100, 0);
    // 5x move: short loss ≈ 2800 >> its 1000 capital -> deeply bankrupt (negative equity).
    env.svm.warp_to_slot(6);
    env.push_auth_mark_with_cu(6, 500);
    for p in [b, a] {
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: 6,
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
    let g_pre = env.market_state().1;
    env.crank_steps_after_market_catchup(
        b,
        ProgInstruction::PermissionlessCrank {
            now_slot: 6,
            observations: crank_observations(0),
        },
        2,
    );

    let loser = env.portfolio_state(b);
    let g_post = env.market_state().1;
    assert!(
        percolator::active_bitmap_is_empty(active_bitmap(&loser)),
        "deeply insolvent single-leg account is fully closed"
    );
    assert_eq!(g_post.assets[0].oi_eff_long_q, 0);
    assert_eq!(g_post.assets[0].oi_eff_short_q, 0);
    assert_eq!(g_post.vault, g_pre.vault, "full close mints no vault value");
    assert_eq!(
        g_post.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(
        g_post.vault >= g_post.c_tot + g_post.insurance,
        "senior conservation preserved through residual booking"
    );
}

// The engine-selected liquidation fee is derived from actual closed risk, not keeper input, and remains
// bounded while value conservation holds.
#[test]
fn v16_engine_selected_liquidation_fee_is_bounded_by_closed_position() {
    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.update_liquidation_fee_policy_with_cu(0); // all to insurance
    env.configure_auth_mark_with_cu(0, 1_000_000);
    let lo = Keypair::new();
    let l = env.create_portfolio(&lo);
    let so = Keypair::new();
    let s = env.create_portfolio(&so);
    env.deposit(&lo, l, 100_000_000);
    env.deposit(&so, s, 100_000);
    env.trade_asset_with_cu(0, &lo, l, &so, s, POS_SCALE as i128, 1_000_000, 0); // POS_SCALE position only
    for slot in 1..=30u64 {
        env.svm.warp_to_slot(slot);
        let _ = env.push_auth_mark_with_cu(slot, 2_000_000);
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(s, false),
            ],
            &[],
        );
    }
    let (_, g0) = env.market_state();

    env.crank(
        s,
        ProgInstruction::PermissionlessCrank {
            now_slot: 30,
            observations: crank_observations(0),
        },
    );
    let (_, g1) = env.market_state();
    let fee = g1.insurance - g0.insurance;

    assert!(fee > 0, "a fee was charged (non-vacuous)");
    assert!(
        fee < 10_000,
        "fee is bounded by the actual closed position: {fee}"
    );
    let sl = env.portfolio_state(s);
    assert!(
        sl.legs[0].basis_pos_q.get().unsigned_abs() <= POS_SCALE,
        "position closed at most to its actual size (no phantom over-close)"
    );
    assert_eq!(g1.vault, g0.vault, "fee internal, no vault mint");
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}

#[derive(Clone, Copy)]
struct Inv061LiquidationClass {
    class: &'static str,
    engine_proofs: &'static [&'static str],
    public_witnesses: &'static [(&'static str, &'static str)],
}

fn inv061_source_defines_test(source: &str, function: &str) -> bool {
    let marker = format!("fn {function}");
    source.lines().any(|line| {
        line.trim()
            .strip_prefix(&marker)
            .is_some_and(|tail| tail.trim_start().starts_with('('))
    })
}

#[test]
fn v16_program_liquidation_composition_is_source_complete() {
    const ENGINE_PIN: &str = "495a5590c97055bd71c6f94d849ff0298f243145";
    const CLASSES: &[Inv061LiquidationClass] = &[
        Inv061LiquidationClass {
            class: "sole public ingress and deterministic dispatch",
            engine_proofs: &[
                "contract_check_first_actionable_slot",
                "contract_check_select_auto_crank_plan",
                "contract_check_select_progress_witness",
            ],
            public_witnesses: &[(
                "tests/invariants/cu/inv_059_fee_fragmentation_bound.rs",
                "v16_program_liquidation_fee_surface_is_single_route_and_engine_selected",
            )],
        },
        Inv061LiquidationClass {
            class: "minimum health-restoring close selection",
            engine_proofs: &[
                "proof_v16_liquidation_projection_identifies_minimum_no_fee_close",
                "proof_v16_liquidation_projection_includes_fee_equity_debit",
                "proof_v16_liquidation_selector_is_healthy_locally_minimal_or_full_close",
            ],
            public_witnesses: &[(
                "tests/invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs",
                "v16_program_scaled_liquidation_matches_independent_selector_model",
            )],
        },
        Inv061LiquidationClass {
            class: "risk reduction and effective OI coherence",
            engine_proofs: &[
                "proof_v16_trade_reductions_are_funded_only_by_preexisting_side_oi",
                "proof_v16_liquidation_cannot_leave_uncovered_loss_with_other_open_risk",
                "contract_check_kernel_resize_leg_same_side",
                "contract_check_kernel_clear_leg",
            ],
            public_witnesses: &[
                (
                    "tests/invariants/stateful/inv_061_deterministic_bounded_liquidation.rs",
                    "v16_program_multi_asset_adl_liquidation_is_order_local_and_exit_complete",
                ),
                (
                    "tests/invariants/cu/inv_048_matched_trade_and_open_interest_coherence.rs",
                    "v16_program_position_mutation_composition_is_source_complete",
                ),
            ],
        },
        Inv061LiquidationClass {
            class: "fee and reward partition",
            engine_proofs: &[
                "proof_v16_liquidation_fee_rejects_subminimum_partial_chunks",
                "proof_v16_liquidation_fee_allows_subminimum_full_close",
                "proof_v16_liquidation_partial_fee_acceptance_implies_no_min_floor_extraction",
            ],
            public_witnesses: &[
                (
                    "tests/invariants/cu/inv_059_fee_fragmentation_bound.rs",
                    "v16_program_new_liquidation_fee_episode_requires_new_authenticated_deficit",
                ),
                (
                    "tests/invariants/cu/inv_061_deterministic_bounded_liquidation.rs",
                    "v16_program_liquidation_cranker_reward_bounded_by_fee",
                ),
            ],
        },
        Inv061LiquidationClass {
            class: "durable residual or declared Recovery fallback",
            engine_proofs: &[
                "proof_v16_liquidation_preflight_accepts_only_fully_durable_residual",
                "proof_v16_liquidation_preflight_routes_insufficient_residual_capacity_to_recovery",
                "proof_v16_liquidation_error_commits_only_fully_declared_recovery",
            ],
            public_witnesses: &[
                (
                    "tests/invariants/cu/inv_061_deterministic_bounded_liquidation.rs",
                    "v16_program_reset_carry_liquidation_matrix_preserves_progress",
                ),
                (
                    "tests/invariants/stateful/inv_071_crank_progress.rs",
                    "v16_program_unattributed_multi_asset_loss_reaches_liquidation_and_terminal_payout",
                ),
            ],
        },
        Inv061LiquidationClass {
            class: "terminal order and funded exit",
            engine_proofs: &["proof_v16_prior_reset_cleanup_cannot_starve_live_liquidation"],
            public_witnesses: &[(
                "tests/invariants/stateful/inv_061_deterministic_bounded_liquidation.rs",
                "v16_program_resolved_adl_close_order_matrix_preserves_funded_exits",
            )],
        },
        Inv061LiquidationClass {
            class: "maximum account, source, and oracle shape",
            engine_proofs: &["proof_v16_recovery_legs_cannot_starve_dispatchable_auto_crank_work"],
            public_witnesses: &[
                (
                    "tests/invariants/cu/inv_077_bounded_work_and_maximum_shape_compute.rs",
                    "v16_attack_public_14_leg_28_source_equal_risk_liquidation_stays_bounded",
                ),
                (
                    "tests/invariants/cu/inv_077_bounded_work_and_maximum_shape_compute.rs",
                    "v16_attack_public_14_leg_28_source_42_feed_refresh_stays_bounded",
                ),
                (
                    "tests/invariants/cu/inv_077_bounded_work_and_maximum_shape_compute.rs",
                    "v16_program_max_source_liquidation_asset_matrix_has_bounded_public_exits",
                ),
            ],
        },
    ];

    let cargo = include_str!("../../../Cargo.toml");
    let lock = include_str!("../../../Cargo.lock");
    assert_eq!(
        cargo.matches(&format!("rev = \"{ENGINE_PIN}\"")).count(),
        2,
        "INV-061 proof composition must be reviewed on every engine pin change",
    );
    assert!(
        lock.contains(&format!("rev={ENGINE_PIN}#{ENGINE_PIN}")),
        "Cargo.lock must resolve the liquidation-certified engine revision",
    );

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut classes = std::collections::BTreeSet::new();
    let mut proofs = std::collections::BTreeSet::new();
    let mut source_cache = std::collections::BTreeMap::<&str, String>::new();
    for row in CLASSES {
        assert!(classes.insert(row.class), "duplicate liquidation class");
        assert!(!row.engine_proofs.is_empty());
        assert!(!row.public_witnesses.is_empty());
        for proof in row.engine_proofs {
            assert!(proofs.insert(*proof), "duplicate engine proof {proof}");
            assert!(
                proof.starts_with("contract_check_") || proof.starts_with("proof_v16_"),
                "unclassified liquidation proof {proof}",
            );
        }
        for (path, witness) in row.public_witnesses {
            let source = source_cache.entry(path).or_insert_with(|| {
                std::fs::read_to_string(root.join(path))
                    .unwrap_or_else(|error| panic!("read {path}: {error}"))
            });
            assert!(
                inv061_source_defines_test(source, witness),
                "liquidation class '{}' lacks executable witness {path}#{witness}",
                row.class,
            );
        }
    }
    assert_eq!(classes.len(), 7, "liquidation class roster drift");
    assert_eq!(proofs.len(), 18, "liquidation proof roster drift");

    let production = include_str!("../../../src/v16_program.rs");
    let production = production
        .split("    #[cfg(test)]\n    mod tests")
        .next()
        .expect("production prefix exists");
    assert_eq!(
        production.matches("AutoCrankPlanV16::Liquidate").count(),
        3,
        "a new liquidation dispatch requires selector, frame, and CU evidence",
    );
    assert_eq!(
        production.matches("LiquidationRequestV16").count(),
        0,
        "the wrapper must not construct a caller-sized liquidation request",
    );
    for forbidden_variant in ["Liquidate {", "LiquidatePosition", "LiquidateAccount"] {
        assert!(
            !production.contains(&format!("Self::{forbidden_variant}")),
            "a direct liquidation instruction reopens INV-061",
        );
    }

    let caller_roster = include_str!("../inv_023_caller_input_roster.tsv");
    assert!(caller_roster.contains("PermissionlessCrank\tobservations\tDISCOVERY_HINT\t"));
    assert!(caller_roster
        .contains("CrankObservationHint\tasset_index,oracle_accounts\tDISCOVERY_HINT\t"));
    assert!(!caller_roster.contains("PermissionlessCrank.close"));

    let transition_census =
        include_str!("inv_088_global_summaries_are_not_account_local_proofs.rs");
    assert!(transition_census.contains(
        "fn v16_program_every_wrapper_engine_transition_callsite_has_summary_disposition_and_witness"
    ));
}
