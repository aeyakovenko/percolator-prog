//! INV-057 - Risk-reduction availability.
//!
//! Normative obligation: Every reachable exposed state retains a bounded owner-callable risk-reducing or terminal route.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_attack_tradecpi_open_and_exit_remain_live_with_backing_fee_policy`, `v16_attack_recovery_cpi_routes_allow_user_exit_before_force_close`, `v16_attack_drain_only_existing_risk_can_exit_through_cpi`, `v16_attack_stale_batch_nocpi_recovers_after_public_precrank`, `v16_attack_stale_batch_cpi_recovers_after_public_precrank`, `v16_attack_stale_tradecpi_recovers_after_public_precrank_with_max_tail`, `v16_attack_stale_tradenocpi_recovers_after_public_precrank`, `v16_attack_non_base_local_stale_owner_reduce_remains_live`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_attack_tradecpi_open_and_exit_remain_live_with_backing_fee_policy() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.update_backing_fee_policy_with_cu(0, 77, 5_000);
    assert_eq!(
        env.market_state().0.backing_trade_fee_policy_count,
        1,
        "test setup must activate the backing-fee batch gate"
    );

    let taker = Keypair::new();
    let lp = Keypair::new();
    let ta = env.create_portfolio(&taker);
    let la = env.create_portfolio(&lp);
    env.deposit(&taker, ta, 1_000_000);
    env.deposit(&lp, la, 1_000_000);

    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let (ctx, delegate, _) = env.init_auth_matcher_context(matcher_program, &lp, la);

    let sz = (5 * POS_SCALE) as i128;
    env.svm.expire_blockhash();
    let open_cu = env
        .try_trade_cpi_with_cu_on_asset(
            &taker,
            ta,
            &lp,
            la,
            matcher_program,
            ctx,
            delegate,
            0,
            sz,
            0,
        )
        .expect("single TradeCpi open must remain live with active backing-fee policy");
    assert_cu_within(
        "TradeCpi backing-fee-policy open",
        open_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(ta), 0).basis_pos_q,
        sz
    );
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(la), 0).basis_pos_q,
        -sz
    );

    env.svm.expire_blockhash();
    let close_cu = env
        .try_trade_cpi_with_cu_on_asset(
            &taker,
            ta,
            &lp,
            la,
            matcher_program,
            ctx,
            delegate,
            0,
            -sz,
            0,
        )
        .expect("single TradeCpi exit must remain live with active backing-fee policy");
    assert_cu_within(
        "TradeCpi backing-fee-policy exit",
        close_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );
    assert!(
        !has_active_leg_for_asset(&env.portfolio_state(ta), 0),
        "taker can fully exit through single TradeCpi while the backing-fee policy is active"
    );
    assert!(
        !has_active_leg_for_asset(&env.portfolio_state(la), 0),
        "LP leg closes through the same public CPI route"
    );
    assert_eq!(
        env.market_state().0.backing_trade_fee_policy_count,
        1,
        "the single TradeCpi escape route must not silently clear the batch gate"
    );
}

#[test]
fn v16_attack_recovery_cpi_routes_allow_user_exit_before_force_close() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 5_000, 10_000, 1_000);
    env.configure_permissionless_resolve_with_cu(100, 50);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);

    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);

    let single_taker = Keypair::new();
    let single_lp = Keypair::new();
    let single_taker_account = env.create_portfolio(&single_taker);
    let single_lp_account = env.create_portfolio(&single_lp);
    env.deposit(&single_taker, single_taker_account, 1_000_000);
    env.deposit(&single_lp, single_lp_account, 1_000_000);
    env.trade_asset_with_cu(
        1,
        &single_taker,
        single_taker_account,
        &single_lp,
        single_lp_account,
        POS_SCALE as i128,
        100,
        0,
    );
    let (single_ctx, single_delegate, _) =
        env.init_auth_matcher_context(matcher_program, &single_lp, single_lp_account);

    let batch_taker = Keypair::new();
    let batch_lp = Keypair::new();
    let batch_taker_account = env.create_portfolio(&batch_taker);
    let batch_lp_account = env.create_portfolio(&batch_lp);
    env.deposit(&batch_taker, batch_taker_account, 1_000_000);
    env.deposit(&batch_lp, batch_lp_account, 1_000_000);
    env.trade_asset_with_cu(
        1,
        &batch_taker,
        batch_taker_account,
        &batch_lp,
        batch_lp_account,
        POS_SCALE as i128,
        100,
        0,
    );
    let (batch_ctx, batch_delegate, _) =
        env.init_auth_matcher_context(matcher_program, &batch_lp, batch_lp_account);

    env.svm.warp_to_slot(10);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
        1,
        10,
        0,
    );
    let (_, recovery_group) = env.market_state();
    assert_eq!(
        recovery_group.assets[1].lifecycle,
        AssetLifecycleV16::Recovery
    );
    assert_eq!(recovery_group.assets[1].oi_eff_long_q, 2 * POS_SCALE);
    assert_eq!(recovery_group.assets[1].oi_eff_short_q, 2 * POS_SCALE);

    env.svm.expire_blockhash();
    let single_cu = env.trade_cpi_with_cu_on_asset(
        &single_taker,
        single_taker_account,
        &single_lp,
        single_lp_account,
        matcher_program,
        single_ctx,
        single_delegate,
        1,
        -(POS_SCALE as i128),
        0,
    );
    assert_cu_within("Recovery single TradeCpi exit", single_cu, TRADE_CU_LIMIT);
    assert!(
        !has_active_leg_for_asset(&env.portfolio_state(single_taker_account), 1),
        "single TradeCpi exit leaves the taker flat"
    );
    assert!(
        !has_active_leg_for_asset(&env.portfolio_state(single_lp_account), 1),
        "single TradeCpi exit leaves the LP flat"
    );

    env.svm.expire_blockhash();
    let batch_cu = env
        .send(
            ProgInstruction::BatchTradeCpi {
                legs: vec![BatchTradeCpiLeg {
                    asset_index: 1,
                    size_q: -(POS_SCALE as i128),
                    fee_bps: 0,
                    limit_price: 0,
                }],
            },
            vec![
                AccountMeta::new(batch_taker.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(batch_taker_account, false),
                AccountMeta::new(batch_lp_account, false),
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new(batch_ctx, false),
                AccountMeta::new_readonly(batch_delegate, false),
            ],
            &[&batch_taker],
        )
        .expect("BatchTradeCpi must let Recovery users exit before force-close");
    assert_cu_within("Recovery BatchTradeCpi exit", batch_cu, TRADE_CU_LIMIT);
    assert!(
        !has_active_leg_for_asset(&env.portfolio_state(batch_taker_account), 1),
        "BatchTradeCpi exit leaves the taker flat"
    );
    assert!(
        !has_active_leg_for_asset(&env.portfolio_state(batch_lp_account), 1),
        "BatchTradeCpi exit leaves the LP flat"
    );

    let (_, group_after) = env.market_state();
    assert_eq!(group_after.assets[1].oi_eff_long_q, 0);
    assert_eq!(group_after.assets[1].oi_eff_short_q, 0);
    assert_eq!(group_after.assets[1].lifecycle, AssetLifecycleV16::Recovery);
    assert!(group_after.vault >= group_after.c_tot + group_after.insurance);
}

#[test]
fn v16_attack_drain_only_existing_risk_can_exit_through_cpi() {
    assert_drain_only_existing_risk_can_exit_through_cpi(DrainOnlyCpiExitRoute::Single);
    assert_drain_only_existing_risk_can_exit_through_cpi(DrainOnlyCpiExitRoute::Batch);
}

#[test]
fn v16_attack_stale_batch_nocpi_recovers_after_public_precrank() {
    const LEGS: usize = 8;
    let mut env = V16CuEnv::new_with_market_params_and_price_move(LEGS as u16, 1_000, 1_000, 500);
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 100_000);
    env.deposit(&short_owner, short_account, 100_000);
    env.seed_n_leg_position_for_benchmark(long_account, short_account, LEGS);
    env.svm.warp_to_slot(16);

    let reduce_legs: Vec<BatchTradeLeg> = (0..LEGS as u16)
        .map(|asset_index| BatchTradeLeg {
            asset_index,
            size_q: -(POS_SCALE as i128),
            exec_price: 95,
            fee_bps: 0,
        })
        .collect();
    env.svm.expire_blockhash();
    let stale_batch = env.send(
        ProgInstruction::BatchTradeNoCpi {
            legs: reduce_legs.clone(),
        },
        vec![
            AccountMeta::new(long_owner.pubkey(), true),
            AccountMeta::new(short_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(long_account, false),
            AccountMeta::new(short_account, false),
        ],
        &[&long_owner, &short_owner],
    );
    let stale_err = stale_batch.expect_err("8-leg stale batch must require public pre-crank");
    assert!(
        stale_err.contains("Custom(19)") || stale_err.contains("custom program error: 0x13"),
        "stale batch should reject as EngineStale before pre-crank, got: {stale_err}"
    );
    assert!(
        !stale_err.contains("exceeded CUs"),
        "stale batch should reject before CU exhaustion: {stale_err}"
    );

    let mut max_crank_cu = 0;
    for portfolio in [long_account, short_account] {
        for asset_index in 0..LEGS as u16 {
            for _ in 0..2 {
                env.svm.expire_blockhash();
                let crank_cu = env.crank_steps_after_market_catchup(
                    portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 16,
                        observations: crank_observations(asset_index),
                    },
                    1,
                );
                max_crank_cu = max_crank_cu.max(crank_cu);
                assert!(
                    crank_cu < 1_400_000,
                    "BatchTradeNoCpi stale pre-crank CU {crank_cu} must fit the tx limit"
                );
            }
        }
    }
    for portfolio in [long_account, short_account] {
        env.svm.expire_blockhash();
        let crank_cu = env.crank_steps_after_market_catchup(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 16,
                observations: crank_observations(0),
            },
            1,
        );
        max_crank_cu = max_crank_cu.max(crank_cu);
        assert!(
            crank_cu < 1_400_000,
            "BatchTradeNoCpi stale catch-up pre-crank CU {crank_cu} must fit the tx limit"
        );
    }
    let (_, refreshed_group) = env.market_state();
    for (label, portfolio) in [
        ("long", env.portfolio_state(long_account)),
        ("short", env.portfolio_state(short_account)),
    ] {
        assert_eq!(portfolio.stale_state, 0, "{label} stale flag cleared");
        assert_eq!(portfolio.b_stale_state, 0, "{label} B-stale flag cleared");
        let cert = health_cert(&portfolio);
        assert!(cert.valid, "{label} health cert valid after pre-crank");
        assert_eq!(
            cert.cert_oracle_epoch, refreshed_group.oracle_epoch,
            "{label} oracle cert current after pre-crank"
        );
        assert_eq!(
            cert.active_bitmap_at_cert,
            active_bitmap(&portfolio),
            "{label} cert active set current after pre-crank"
        );
    }

    env.svm.expire_blockhash();
    let batch_cu = env
        .send(
            ProgInstruction::BatchTradeNoCpi { legs: reduce_legs },
            vec![
                AccountMeta::new(long_owner.pubkey(), true),
                AccountMeta::new(short_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(long_account, false),
                AccountMeta::new(short_account, false),
            ],
            &[&long_owner, &short_owner],
        )
        .expect("public pre-crank must restore the stale 8-leg batch exit path");
    println!(
        "v16 stale 8-leg BatchTradeNoCpi after pre-crank CU: batch={batch_cu}, max_crank={max_crank_cu}"
    );
    assert!(
        batch_cu < 1_400_000,
        "post-pre-crank 8-leg BatchTradeNoCpi CU {batch_cu} must fit the tx limit"
    );

    let long = env.portfolio_state(long_account);
    let short = env.portfolio_state(short_account);
    for asset_index in 0..LEGS {
        assert_eq!(
            active_leg_for_asset(&long, asset_index).basis_pos_q,
            (9 * POS_SCALE) as i128
        );
        assert_eq!(
            active_leg_for_asset(&short, asset_index).basis_pos_q,
            -((9 * POS_SCALE) as i128)
        );
    }
}

#[test]
fn v16_attack_stale_batch_cpi_recovers_after_public_precrank() {
    const LEGS: usize = 8;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(LEGS as u16, 1_000, 1_000, 500);
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 100_000);
    env.deposit(&short_owner, short_account, 100_000);
    env.seed_n_leg_position_for_benchmark(long_account, short_account, LEGS);
    env.svm.warp_to_slot(16);

    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let (ctx, delegate, _) =
        env.init_auth_matcher_context(matcher_program, &short_owner, short_account);
    let reduce_legs: Vec<BatchTradeCpiLeg> = (0..LEGS as u16)
        .map(|asset_index| BatchTradeCpiLeg {
            asset_index,
            size_q: -(POS_SCALE as i128),
            fee_bps: 0,
            limit_price: 0,
        })
        .collect();
    let batch_accounts = |env: &V16CuEnv| {
        vec![
            AccountMeta::new(long_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(long_account, false),
            AccountMeta::new(short_account, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ]
    };

    env.svm.expire_blockhash();
    let stale_batch = env.send(
        ProgInstruction::BatchTradeCpi {
            legs: reduce_legs.clone(),
        },
        batch_accounts(&env),
        &[&long_owner],
    );
    let stale_err = stale_batch.expect_err("8-leg stale BatchTradeCpi must require pre-crank");
    assert!(
        stale_err.contains("Custom(19)") || stale_err.contains("custom program error: 0x13"),
        "stale BatchTradeCpi should reject as EngineStale before pre-crank, got: {stale_err}"
    );
    assert!(
        !stale_err.contains("exceeded CUs"),
        "stale BatchTradeCpi should reject before CU exhaustion: {stale_err}"
    );

    let mut max_crank_cu = 0;
    for portfolio in [long_account, short_account] {
        for asset_index in 0..LEGS as u16 {
            for _ in 0..2 {
                env.svm.expire_blockhash();
                let crank_cu = env.crank_steps_after_market_catchup(
                    portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 16,
                        observations: crank_observations(asset_index),
                    },
                    1,
                );
                max_crank_cu = max_crank_cu.max(crank_cu);
                assert!(
                    crank_cu < 1_400_000,
                    "BatchTradeCpi stale pre-crank CU {crank_cu} must fit the tx limit"
                );
            }
        }
    }
    for portfolio in [long_account, short_account] {
        env.svm.expire_blockhash();
        let crank_cu = env.crank_steps_after_market_catchup(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 16,
                observations: crank_observations(0),
            },
            1,
        );
        max_crank_cu = max_crank_cu.max(crank_cu);
        assert!(
            crank_cu < 1_400_000,
            "BatchTradeCpi stale catch-up pre-crank CU {crank_cu} must fit the tx limit"
        );
    }
    let (_, refreshed_group) = env.market_state();
    for (label, portfolio) in [
        ("long", env.portfolio_state(long_account)),
        ("short", env.portfolio_state(short_account)),
    ] {
        assert_eq!(portfolio.stale_state, 0, "{label} stale flag cleared");
        assert_eq!(portfolio.b_stale_state, 0, "{label} B-stale flag cleared");
        let cert = health_cert(&portfolio);
        assert!(cert.valid, "{label} health cert valid after pre-crank");
        assert_eq!(
            cert.cert_oracle_epoch, refreshed_group.oracle_epoch,
            "{label} oracle cert current after pre-crank"
        );
        assert_eq!(
            cert.active_bitmap_at_cert,
            active_bitmap(&portfolio),
            "{label} cert active set current after pre-crank"
        );
    }

    env.svm.expire_blockhash();
    let batch_cu = env
        .send(
            ProgInstruction::BatchTradeCpi { legs: reduce_legs },
            batch_accounts(&env),
            &[&long_owner],
        )
        .expect("public pre-crank must restore the stale 8-leg BatchTradeCpi exit path");
    println!(
        "v16 stale 8-leg BatchTradeCpi after pre-crank CU: batch={batch_cu}, max_crank={max_crank_cu}"
    );
    assert!(
        batch_cu < 1_400_000,
        "post-pre-crank 8-leg BatchTradeCpi CU {batch_cu} must fit the tx limit"
    );

    let long = env.portfolio_state(long_account);
    let short = env.portfolio_state(short_account);
    for asset_index in 0..LEGS {
        assert_eq!(
            active_leg_for_asset(&long, asset_index).basis_pos_q,
            (9 * POS_SCALE) as i128
        );
        assert_eq!(
            active_leg_for_asset(&short, asset_index).basis_pos_q,
            -((9 * POS_SCALE) as i128)
        );
    }
    assert_eq!(
        env.market_state().1.vault as u64,
        env.token_amount(env.vault)
    );
}

#[test]
fn v16_attack_stale_tradecpi_recovers_after_public_precrank_with_max_tail() {
    const LEGS: usize = 8;
    const MAX_TAIL: usize = percolator_prog::constants::MAX_MATCHER_TAIL_ACCOUNTS;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(LEGS as u16, 1_000, 1_000, 500);
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 100_000);
    env.deposit(&short_owner, short_account, 100_000);
    env.seed_n_leg_position_for_benchmark(long_account, short_account, LEGS);
    env.svm.warp_to_slot(16);

    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let (ctx, delegate, _) =
        env.init_auth_matcher_context(matcher_program, &short_owner, short_account);
    let max_tail: Vec<Pubkey> = (0..MAX_TAIL)
        .map(|_| {
            let key = Pubkey::new_unique();
            env.svm
                .set_account(
                    key,
                    Account {
                        lamports: 1_000_000_000,
                        data: vec![0u8; 8],
                        owner: Pubkey::default(),
                        executable: false,
                        rent_epoch: 0,
                    },
                )
                .unwrap();
            key
        })
        .collect();
    let trade_accounts = |env: &V16CuEnv| {
        let mut accounts = vec![
            AccountMeta::new(long_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(long_account, false),
            AccountMeta::new(short_account, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ];
        accounts.extend(
            max_tail
                .iter()
                .copied()
                .map(|key| AccountMeta::new_readonly(key, false)),
        );
        accounts
    };

    let market_before = env.svm.get_account(&env.market).unwrap();
    let long_before = env.svm.get_account(&long_account).unwrap();
    let short_before = env.svm.get_account(&short_account).unwrap();
    let ctx_before = env.svm.get_account(&ctx).unwrap();
    env.svm.expire_blockhash();
    let stale_trade = env.send(
        ProgInstruction::TradeCpi {
            asset_index: 0,
            size_q: -(POS_SCALE as i128),
            fee_bps: 0,
            limit_price: 0,
        },
        trade_accounts(&env),
        &[&long_owner],
    );
    let stale_err = stale_trade.expect_err("8-leg stale TradeCpi must require public pre-crank");
    assert!(
        stale_err.contains("Custom(19)") || stale_err.contains("custom program error: 0x13"),
        "stale TradeCpi should reject as EngineStale before pre-crank, got: {stale_err}"
    );
    assert!(
        !stale_err.contains("exceeded CUs"),
        "stale TradeCpi should reject before CU exhaustion: {stale_err}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "stale TradeCpi rejection must not mutate the market"
    );
    assert_eq!(
        env.svm.get_account(&long_account).unwrap(),
        long_before,
        "stale TradeCpi rejection must not mutate the taker"
    );
    assert_eq!(
        env.svm.get_account(&short_account).unwrap(),
        short_before,
        "stale TradeCpi rejection must not mutate the LP or bump req_id"
    );
    assert_eq!(
        env.svm.get_account(&ctx).unwrap(),
        ctx_before,
        "stale TradeCpi rejection must not reach the matcher"
    );

    let mut max_crank_cu = 0;
    for portfolio in [long_account, short_account] {
        for asset_index in 0..LEGS as u16 {
            for _ in 0..2 {
                env.svm.expire_blockhash();
                let crank_cu = env.crank_steps_after_market_catchup(
                    portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 16,
                        observations: crank_observations(asset_index),
                    },
                    1,
                );
                max_crank_cu = max_crank_cu.max(crank_cu);
                assert!(
                    crank_cu < 1_400_000,
                    "TradeCpi stale pre-crank CU {crank_cu} must fit the tx limit"
                );
            }
        }
    }
    for portfolio in [long_account, short_account] {
        env.svm.expire_blockhash();
        let crank_cu = env.crank_steps_after_market_catchup(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 16,
                observations: crank_observations(0),
            },
            1,
        );
        max_crank_cu = max_crank_cu.max(crank_cu);
        assert!(
            crank_cu < 1_400_000,
            "TradeCpi stale catch-up pre-crank CU {crank_cu} must fit the tx limit"
        );
    }

    let (_, refreshed_group) = env.market_state();
    for (label, portfolio) in [
        ("long", env.portfolio_state(long_account)),
        ("short", env.portfolio_state(short_account)),
    ] {
        assert_eq!(portfolio.stale_state, 0, "{label} stale flag cleared");
        assert_eq!(portfolio.b_stale_state, 0, "{label} B-stale flag cleared");
        let cert = health_cert(&portfolio);
        assert!(cert.valid, "{label} health cert valid after pre-crank");
        assert_eq!(
            cert.cert_oracle_epoch, refreshed_group.oracle_epoch,
            "{label} oracle cert current after pre-crank"
        );
        assert_eq!(
            cert.active_bitmap_at_cert,
            active_bitmap(&portfolio),
            "{label} cert active set current after pre-crank"
        );
    }

    env.svm.expire_blockhash();
    let trade_cu = env
        .send(
            ProgInstruction::TradeCpi {
                asset_index: 0,
                size_q: -(POS_SCALE as i128),
                fee_bps: 0,
                limit_price: 0,
            },
            trade_accounts(&env),
            &[&long_owner],
        )
        .expect("public pre-crank must restore the stale 8-leg TradeCpi exit path");
    println!(
        "v16 stale 8-leg TradeCpi max-tail after pre-crank CU: trade={trade_cu}, max_crank={max_crank_cu}"
    );
    assert!(
        trade_cu < 1_400_000,
        "post-pre-crank 8-leg TradeCpi max-tail CU {trade_cu} must fit the tx limit"
    );

    let long = env.portfolio_state(long_account);
    let short = env.portfolio_state(short_account);
    assert_eq!(
        active_leg_for_asset(&long, 0).basis_pos_q,
        (9 * POS_SCALE) as i128,
        "single-CPI exit reduces the stale taker leg after pre-crank"
    );
    assert_eq!(
        active_leg_for_asset(&short, 0).basis_pos_q,
        -((9 * POS_SCALE) as i128),
        "single-CPI exit reduces the stale LP leg after pre-crank"
    );
    assert_eq!(
        env.market_state().1.vault as u64,
        env.token_amount(env.vault)
    );
}

#[test]
fn v16_attack_stale_tradenocpi_recovers_after_public_precrank() {
    const LEGS: usize = 8;
    let mut env = V16CuEnv::new_with_market_params_and_price_move(LEGS as u16, 1_000, 1_000, 500);
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 100_000);
    env.deposit(&short_owner, short_account, 100_000);
    env.seed_n_leg_position_for_benchmark(long_account, short_account, LEGS);
    env.svm.warp_to_slot(16);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let long_before = env.svm.get_account(&long_account).unwrap();
    let short_before = env.svm.get_account(&short_account).unwrap();
    let stale_trade = env.try_trade_asset_with_cu(
        0,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        -(POS_SCALE as i128),
        95,
        0,
    );
    let stale_err = stale_trade.expect_err("8-leg stale TradeNoCpi must require public pre-crank");
    assert!(
        stale_err.contains("Custom(19)") || stale_err.contains("custom program error: 0x13"),
        "stale TradeNoCpi should reject as EngineStale before pre-crank, got: {stale_err}"
    );
    assert!(
        !stale_err.contains("exceeded CUs"),
        "stale TradeNoCpi should reject before CU exhaustion: {stale_err}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "stale TradeNoCpi rejection must not mutate the market"
    );
    assert_eq!(
        env.svm.get_account(&long_account).unwrap(),
        long_before,
        "stale TradeNoCpi rejection must not mutate the taker"
    );
    assert_eq!(
        env.svm.get_account(&short_account).unwrap(),
        short_before,
        "stale TradeNoCpi rejection must not mutate the counterparty"
    );

    let mut max_crank_cu = 0;
    for portfolio in [long_account, short_account] {
        for asset_index in 0..LEGS as u16 {
            for _ in 0..2 {
                env.svm.expire_blockhash();
                let crank_cu = env.crank_steps_after_market_catchup(
                    portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 16,
                        observations: crank_observations(asset_index),
                    },
                    1,
                );
                max_crank_cu = max_crank_cu.max(crank_cu);
                assert!(
                    crank_cu < 1_400_000,
                    "TradeNoCpi stale pre-crank CU {crank_cu} must fit the tx limit"
                );
            }
        }
    }
    for portfolio in [long_account, short_account] {
        env.svm.expire_blockhash();
        let crank_cu = env.crank_steps_after_market_catchup(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 16,
                observations: crank_observations(0),
            },
            1,
        );
        max_crank_cu = max_crank_cu.max(crank_cu);
        assert!(
            crank_cu < 1_400_000,
            "TradeNoCpi stale catch-up pre-crank CU {crank_cu} must fit the tx limit"
        );
    }

    let (_, refreshed_group) = env.market_state();
    for (label, portfolio) in [
        ("long", env.portfolio_state(long_account)),
        ("short", env.portfolio_state(short_account)),
    ] {
        assert_eq!(portfolio.stale_state, 0, "{label} stale flag cleared");
        assert_eq!(portfolio.b_stale_state, 0, "{label} B-stale flag cleared");
        let cert = health_cert(&portfolio);
        assert!(cert.valid, "{label} health cert valid after pre-crank");
        assert_eq!(
            cert.cert_oracle_epoch, refreshed_group.oracle_epoch,
            "{label} oracle cert current after pre-crank"
        );
        assert_eq!(
            cert.active_bitmap_at_cert,
            active_bitmap(&portfolio),
            "{label} cert active set current after pre-crank"
        );
    }

    env.svm.expire_blockhash();
    let trade_cu = env.trade_asset_with_cu(
        0,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        -(POS_SCALE as i128),
        95,
        0,
    );
    println!(
        "v16 stale 8-leg TradeNoCpi after pre-crank CU: trade={trade_cu}, max_crank={max_crank_cu}"
    );
    assert!(
        trade_cu < 1_400_000,
        "post-pre-crank 8-leg TradeNoCpi CU {trade_cu} must fit the tx limit"
    );

    let long = env.portfolio_state(long_account);
    let short = env.portfolio_state(short_account);
    assert_eq!(
        active_leg_for_asset(&long, 0).basis_pos_q,
        (9 * POS_SCALE) as i128,
        "direct exit reduces the stale taker leg after pre-crank"
    );
    assert_eq!(
        active_leg_for_asset(&short, 0).basis_pos_q,
        -((9 * POS_SCALE) as i128),
        "direct exit reduces the stale counterparty leg after pre-crank"
    );
    assert_eq!(
        env.market_state().1.vault as u64,
        env.token_amount(env.vault)
    );
}

#[test]
fn v16_attack_non_base_local_stale_owner_reduce_remains_live() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 1_000, 1_000, 500);
    env.configure_permissionless_resolve_with_cu(5, 5);

    env.activate_asset(1, 0, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 0, 100);

    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, 100);

    let taker = Keypair::new();
    let lp = Keypair::new();
    let taker_portfolio = env.create_portfolio(&taker);
    let lp_portfolio = env.create_portfolio(&lp);
    env.deposit(&taker, taker_portfolio, 1_000_000);
    env.deposit(&lp, lp_portfolio, 1_000_000);

    env.svm.warp_to_slot(4);
    env.push_auth_mark_with_cu(4, 100);
    env.trade_asset_with_cu(
        1,
        &taker,
        taker_portfolio,
        &lp,
        lp_portfolio,
        POS_SCALE as i128,
        100,
        0,
    );
    assert!(
        has_active_leg_for_asset(&env.portfolio_state(taker_portfolio), 1),
        "probe setup must open an asset-1 leg before the local profile goes stale"
    );

    env.svm.warp_to_slot(6);
    env.svm.expire_blockhash();
    let stale_trade = env.try_trade_asset_with_cu(
        1,
        &taker,
        taker_portfolio,
        &lp,
        lp_portfolio,
        POS_SCALE as i128,
        100,
        0,
    );
    assert!(
        stale_trade.is_err(),
        "probe setup must make fresh asset-1 trading reject on the locally stale profile"
    );

    env.svm.expire_blockhash();
    let reduce = env.send(
        ProgInstruction::RebalanceReduce {
            portfolio_id: env.portfolio_id(taker_portfolio),
            position_epoch: env.portfolio_position_epoch(taker_portfolio),
            asset_index: 1,
            reduce_q: POS_SCALE,
        },
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker_portfolio, false),
        ],
        &[&taker],
    );
    assert!(
        reduce.is_ok(),
        "owner reduce-only exit must remain live while only the local asset profile is stale: {reduce:?}"
    );
    assert!(
        !has_active_leg_for_asset(&env.portfolio_state(taker_portfolio), 1),
        "owner reduce-only exit should clear the stale local leg"
    );
}
