//! INV-057 - Risk-reduction availability.
//!
//! Normative obligation: Every reachable exposed state retains a bounded owner-callable risk-reducing or terminal route.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): public lifecycle and
//! backing-fee gate regressions that reject new risk early while preserving existing-risk exits,
//! plus stale pre-crank recovery routes and non-base local-stale owner reduction. These tests exercise the deployed public
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
            env.batch_trade_cpi_ix(
                batch_taker_account,
                batch_lp_account,
                vec![BatchTradeCpiLeg {
                    asset_index: 1,
                    market_id: recovery_group.assets[1].market_id,
                    size_q: -(POS_SCALE as i128),
                    fee_bps: 0,
                    limit_price: 0,
                }],
            ),
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
            market_id: first_generation_market_id(asset_index),
            size_q: -(POS_SCALE as i128),
            exec_price: 95,
            fee_bps: 0,
        })
        .collect();
    env.svm.expire_blockhash();
    let stale_batch = env.send(
        env.batch_trade_no_cpi_ix(long_account, short_account, reduce_legs.clone()),
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
            env.batch_trade_no_cpi_ix(long_account, short_account, reduce_legs),
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
            market_id: first_generation_market_id(asset_index),
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
        env.batch_trade_cpi_ix(long_account, short_account, reduce_legs.clone()),
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
            env.batch_trade_cpi_ix(long_account, short_account, reduce_legs),
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
        env.trade_cpi_ix(long_account, short_account, 0, -(POS_SCALE as i128), 0, 0),
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
            env.trade_cpi_ix(long_account, short_account, 0, -(POS_SCALE as i128), 0, 0),
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

// the delay; force-close SUCCEEDS after it.
#[test]
fn v16_program_force_shutdown_timeout_lets_traders_exit_before_close() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    const DELAY: u64 = 50;
    env.configure_permissionless_resolve_with_cu(100, DELAY); // force_close_delay_slots = 50

    // open an asset-1 position (a trader is exposed when the shutdown lands).
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    env.trade_asset_with_cu(1, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);

    // GATE: only `marketauth` or the asset's local asset_admin may shut an asset down. A fresh key
    // that is neither is rejected before any state changes.
    const SHUT: u64 = 10;
    env.svm.warp_to_slot(SHUT);
    env.svm.expire_blockhash();
    let mallory = Keypair::new();
    env.ensure_signer_account(mallory.pubkey());
    let stranger = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
            asset_index: 1,
            now_slot: SHUT,
            initial_price: 0,
            max_init_fee: u128::MAX,
            insurance_authority: mallory.pubkey().to_bytes(),
            insurance_operator: mallory.pubkey().to_bytes(),
            backing_bucket_authority: mallory.pubkey().to_bytes(),
            oracle_authority: mallory.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(mallory.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&mallory],
    );
    assert!(
        stranger.is_err(),
        "a signer that is neither marketauth nor asset_admin must NOT be able to force-shutdown an asset"
    );
    assert_eq!(
        env.market_state().1.assets[1].lifecycle,
        AssetLifecycleV16::Active,
        "asset 1 still ACTIVE after the rejected non-marketauth shutdown"
    );

    // marketauth (the init signer) force-shuts-down asset 1 at slot 10 -> RECOVERY (frozen mark, not yet wound down).
    env.svm.expire_blockhash();
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
        1,
        SHUT,
        0,
    );
    assert_eq!(
        env.market_state().1.assets[1].lifecycle,
        AssetLifecycleV16::Recovery,
        "asset 1 is in RECOVERY after shutdown (not instantly force-closed)"
    );

    // WINDOW: before the delay elapses, the permissionless force-close is REJECTED -> traders still have
    // time to exit; the asset is not rugged out from under them.
    let cranker = Keypair::new();
    env.svm.warp_to_slot(SHUT + DELAY - 1);
    let early = env.try_force_close_abandoned_asset_with_cu(
        &cranker,
        pa,
        pb,
        1,
        SHUT + DELAY - 1,
        POS_SCALE,
    );
    assert!(
        early.is_err(),
        "force-close must REJECT before force_close_delay_slots elapses (exit window open)"
    );
    assert_eq!(
        env.market_state().1.assets[1].lifecycle,
        AssetLifecycleV16::Recovery,
        "asset still RECOVERY, not closed"
    );

    // after the delay, the wind-down may proceed.
    env.svm.warp_to_slot(SHUT + DELAY + 5);
    env.svm.expire_blockhash();
    let late = env.try_force_close_abandoned_asset_with_cu(
        &cranker,
        pa,
        pb,
        1,
        SHUT + DELAY + 5,
        POS_SCALE,
    );
    assert!(
        late.is_ok(),
        "force-close succeeds once the delay has elapsed: {:?}",
        late
    );
    let g = env.market_state().1;
    assert!(
        g.vault >= g.c_tot + g.insurance,
        "senior conservation across shutdown + delayed force-close"
    );
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
}

// and still let existing matched positions reduce to zero.
#[test]
fn v16_program_drain_only_blocks_new_risk_but_allows_reduce() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 5_000, 10_000, 1_000);
    let admin = env.admin.insecure_clone();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        100,
        0,
    );
    let before = env.svm.get_account(&env.market).unwrap();

    let stranger = Keypair::new();
    env.ensure_signer_account(stranger.pubkey());
    env.svm.expire_blockhash();
    let unauthorized = env.send(
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_DRAIN_ONLY,
            asset_index: 0,
            now_slot: 0,
            initial_price: 0,
            max_init_fee: u128::MAX,
            insurance_authority: stranger.pubkey().to_bytes(),
            insurance_operator: stranger.pubkey().to_bytes(),
            backing_bucket_authority: stranger.pubkey().to_bytes(),
            oracle_authority: stranger.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(stranger.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&stranger],
    );
    assert!(
        unauthorized.is_err(),
        "non-marketauth DrainOnly must reject"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        before,
        "rejected DrainOnly by non-marketauth leaves the market unchanged"
    );

    env.svm.expire_blockhash();
    let malformed = env.send(
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_DRAIN_ONLY,
            asset_index: 0,
            now_slot: 1,
            initial_price: 0,
            max_init_fee: u128::MAX,
            insurance_authority: admin.pubkey().to_bytes(),
            insurance_operator: admin.pubkey().to_bytes(),
            backing_bucket_authority: admin.pubkey().to_bytes(),
            oracle_authority: admin.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        malformed.is_err(),
        "DrainOnly must reject caller-supplied now_slot/price material"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        before,
        "malformed DrainOnly leaves the market unchanged"
    );

    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_DRAIN_ONLY,
        0,
        0,
        0,
    );
    let (_, drain_group) = env.market_state();
    assert_eq!(
        drain_group.assets[0].lifecycle,
        AssetLifecycleV16::DrainOnly
    );
    assert_eq!(drain_group.assets[0].oi_eff_long_q, POS_SCALE);
    assert_eq!(drain_group.assets[0].oi_eff_short_q, POS_SCALE);

    let new_long_owner = Keypair::new();
    let new_short_owner = Keypair::new();
    let new_long_account = env.create_portfolio(&new_long_owner);
    let new_short_account = env.create_portfolio(&new_short_owner);
    env.deposit(&new_long_owner, new_long_account, 1_000_000);
    env.deposit(&new_short_owner, new_short_account, 1_000_000);
    let open = env.try_trade_asset_with_cu(
        0,
        &new_long_owner,
        new_long_account,
        &new_short_owner,
        new_short_account,
        POS_SCALE as i128,
        100,
        0,
    );
    assert!(
        open.is_err(),
        "DrainOnly must reject a new risk-increasing open"
    );
    assert_eq!(env.market_state().1.assets[0].oi_eff_long_q, POS_SCALE);

    let close = env.try_trade_asset_with_cu(
        0,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        -(POS_SCALE as i128),
        100,
        0,
    );
    assert!(
        close.is_ok(),
        "DrainOnly must still allow existing matched risk to reduce: {close:?}"
    );
    let (_, closed_group) = env.market_state();
    assert_eq!(closed_group.assets[0].oi_eff_long_q, 0);
    assert_eq!(closed_group.assets[0].oi_eff_short_q, 0);
    assert!(closed_group.vault >= closed_group.c_tot + closed_group.insurance);
}

// burn CU on work the wrapper already knows cannot commit.
#[test]
fn v16_program_drain_only_existing_risk_increase_rejects_before_hostile_matcher_cpi() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 5_000, 10_000, 1_000);
    let hostile = Pubkey::new_unique();
    env.svm.add_program(
        hostile,
        &std::fs::read(hostile_matcher_program_path()).unwrap(),
    );
    let taker = Keypair::new();
    let lp = Keypair::new();
    let taker_account = env.create_portfolio(&taker);
    let lp_account = env.create_portfolio(&lp);
    env.deposit(&taker, taker_account, 1_000_000);
    env.deposit(&lp, lp_account, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &taker,
        taker_account,
        &lp,
        lp_account,
        POS_SCALE as i128,
        100,
        0,
    );
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_DRAIN_ONLY,
        0,
        0,
        0,
    );
    assert_eq!(
        env.market_state().1.assets[0].lifecycle,
        AssetLifecycleV16::DrainOnly
    );

    let ctx = Pubkey::new_unique();
    let delegate = matcher_delegate_key(
        &env.program_id,
        &env.market,
        &lp_account,
        &lp.pubkey(),
        &hostile,
        &ctx,
    );
    env.svm
        .set_account(
            delegate,
            Account {
                lamports: 1_000_000_000,
                data: vec![],
                owner: Pubkey::default(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let mut ctx_data = vec![0u8; MATCHER_CONTEXT_LEN];
    ctx_data[0] = 0;
    env.svm
        .set_account(
            ctx,
            Account {
                lamports: 1_000_000_000,
                data: ctx_data,
                owner: hostile,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.set_matcher_config(hostile, &lp, lp_account, ctx, delegate, 1);

    let accounts = |env: &V16CuEnv| {
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker_account, false),
            AccountMeta::new(lp_account, false),
            AccountMeta::new_readonly(hostile, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ]
    };

    for (route, instruction) in [
        (
            "TradeCpi",
            env.trade_cpi_ix(taker_account, lp_account, 0, POS_SCALE as i128, 0, 0),
        ),
        (
            "BatchTradeCpi",
            env.batch_trade_cpi_ix(
                taker_account,
                lp_account,
                vec![BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: first_generation_market_id((0) as u16),
                    size_q: POS_SCALE as i128,
                    fee_bps: 0,
                    limit_price: 0,
                }],
            ),
        ),
    ] {
        let market_before = env.svm.get_account(&env.market).unwrap();
        let taker_before = env.svm.get_account(&taker_account).unwrap();
        let lp_before = env.svm.get_account(&lp_account).unwrap();
        let ctx_before = env.svm.get_account(&ctx).unwrap();
        env.svm.expire_blockhash();
        let rejected = env
            .send(instruction, accounts(&env), &[&taker])
            .expect_err("DrainOnly CPI risk increase must reject before matcher CPI");
        assert!(
            rejected.contains("Custom(21)") || rejected.contains("custom program error: 0x15"),
            "{route} DrainOnly risk-increase should be EngineLockActive, got {rejected}"
        );
        assert!(
            !rejected.contains("InvalidAccountData"),
            "{route} must not reach hostile matcher validation: {rejected}"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&taker_account).unwrap(), taker_before);
        assert_eq!(env.svm.get_account(&lp_account).unwrap(), lp_before);
        assert_eq!(
            env.svm.get_account(&ctx).unwrap(),
            ctx_before,
            "{route} must not give the hostile matcher a writable context"
        );
    }
}

// batch gate rather than a market-wide trade lock.
#[test]
fn v16_program_batch_trades_reject_with_backing_fee_policy() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    env.update_backing_fee_policy_with_cu(0, 77, 5_000);
    let (cfg, _) = env.market_state();
    assert_eq!(
        cfg.backing_trade_fee_policy_count, 1,
        "test setup must activate the backing-fee policy gate"
    );

    let taker = Keypair::new();
    let lp = Keypair::new();
    let ta = env.create_portfolio(&taker);
    let la = env.create_portfolio(&lp);
    env.deposit(&taker, ta, 1_000_000);
    env.deposit(&lp, la, 1_000_000);
    let sz = (5 * POS_SCALE) as i128;

    let market_before = env.svm.get_account(&env.market).unwrap().data;
    let taker_before = env.svm.get_account(&ta).unwrap().data;
    let lp_before = env.svm.get_account(&la).unwrap().data;
    env.svm.expire_blockhash();
    let nocpi = env.send(
        env.batch_trade_no_cpi_ix(
            ta,
            la,
            vec![BatchTradeLeg {
                asset_index: 0,
                market_id: first_generation_market_id((0) as u16),
                size_q: sz,
                exec_price: 100,
                fee_bps: 0,
            }],
        ),
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(lp.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ta, false),
            AccountMeta::new(la, false),
        ],
        &[&taker, &lp],
    );
    assert!(
        nocpi.is_err(),
        "BatchTradeNoCpi must reject while backing-fee policy accounting is active"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        market_before
    );
    assert_eq!(env.svm.get_account(&ta).unwrap().data, taker_before);
    assert_eq!(env.svm.get_account(&la).unwrap().data, lp_before);

    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let (ctx, delegate, _) = env.init_auth_matcher_context(matcher_program, &lp, la);
    let market_before = env.svm.get_account(&env.market).unwrap().data;
    let taker_before = env.svm.get_account(&ta).unwrap().data;
    let lp_before = env.svm.get_account(&la).unwrap().data;
    let ctx_before = env.svm.get_account(&ctx).unwrap().data;
    env.svm.expire_blockhash();
    let cpi = env.send(
        env.batch_trade_cpi_ix(
            ta,
            la,
            vec![BatchTradeCpiLeg {
                asset_index: 0,
                market_id: first_generation_market_id((0) as u16),
                size_q: sz,
                fee_bps: 0,
                limit_price: 0,
            }],
        ),
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ta, false),
            AccountMeta::new(la, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ],
        &[&taker],
    );
    assert!(
        cpi.is_err(),
        "BatchTradeCpi must reject while backing-fee policy accounting is active"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        market_before
    );
    assert_eq!(env.svm.get_account(&ta).unwrap().data, taker_before);
    assert_eq!(env.svm.get_account(&la).unwrap().data, lp_before);
    assert_eq!(env.svm.get_account(&ctx).unwrap().data, ctx_before);

    env.svm.expire_blockhash();
    let single = env.try_trade_asset_with_cu(0, &taker, ta, &lp, la, sz, 100, 0);
    assert!(
        single.is_ok(),
        "single-leg TradeNoCpi must still work under an active backing-fee policy: {single:?}"
    );
    let taker_after = env.portfolio_state(ta);
    let lp_after = env.portfolio_state(la);
    assert_eq!(active_leg_for_asset(&taker_after, 0).basis_pos_q, sz);
    assert_eq!(active_leg_for_asset(&lp_after, 0).basis_pos_q, -sz);
}

// policies must drop the count to zero so batch trading works again.
#[test]
fn v16_program_backing_fee_policy_count_clears_batch_liveness() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);

    env.update_backing_fee_policy_with_cu(0, 77, 5_000);
    let (cfg, _) = env.market_state();
    assert_eq!(cfg.backing_trade_fee_policy_count, 1);
    assert_eq!(cfg.backing_trade_fee_bps_long, 77);

    env.update_backing_fee_policy_with_cu(0, 88, 2_500);
    let (cfg, _) = env.market_state();
    assert_eq!(
        cfg.backing_trade_fee_policy_count, 1,
        "nonzero->nonzero update must not double-count the same domain side"
    );
    assert_eq!(cfg.backing_trade_fee_bps_long, 88);

    env.update_backing_fee_policy_with_cu(1, 44, 1_000);
    let (cfg, _) = env.market_state();
    assert_eq!(
        cfg.backing_trade_fee_policy_count, 2,
        "long and short policy sides are counted independently"
    );
    assert_eq!(cfg.backing_trade_fee_bps_short, 44);

    let taker = Keypair::new();
    let lp = Keypair::new();
    let ta = env.create_portfolio(&taker);
    let la = env.create_portfolio(&lp);
    env.deposit(&taker, ta, 1_000_000);
    env.deposit(&lp, la, 1_000_000);
    let sz = (5 * POS_SCALE) as i128;

    let assert_batch_rejects_without_mutation = |env: &mut V16CuEnv, label: &str| {
        let market_before = env.svm.get_account(&env.market).unwrap().data;
        let taker_before = env.svm.get_account(&ta).unwrap().data;
        let lp_before = env.svm.get_account(&la).unwrap().data;
        env.svm.expire_blockhash();
        let rejected = env.send(
            env.batch_trade_no_cpi_ix(
                ta,
                la,
                vec![BatchTradeLeg {
                    asset_index: 0,
                    market_id: first_generation_market_id((0) as u16),
                    size_q: sz,
                    exec_price: 100,
                    fee_bps: 0,
                }],
            ),
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(lp.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(ta, false),
                AccountMeta::new(la, false),
            ],
            &[&taker, &lp],
        );
        assert!(
            rejected.is_err(),
            "{label}: batch must reject while any backing-fee policy remains"
        );
        assert_eq!(
            env.svm.get_account(&env.market).unwrap().data,
            market_before
        );
        assert_eq!(env.svm.get_account(&ta).unwrap().data, taker_before);
        assert_eq!(env.svm.get_account(&la).unwrap().data, lp_before);
    };
    assert_batch_rejects_without_mutation(&mut env, "two active policies");

    env.update_backing_fee_policy_with_cu(0, 0, 0);
    let (cfg, _) = env.market_state();
    assert_eq!(cfg.backing_trade_fee_policy_count, 1);
    assert_eq!(cfg.backing_trade_fee_bps_long, 0);
    assert_eq!(cfg.backing_trade_fee_bps_short, 44);
    assert_batch_rejects_without_mutation(&mut env, "short policy still active");

    env.update_backing_fee_policy_with_cu(1, 0, 0);
    let (cfg, _) = env.market_state();
    assert_eq!(cfg.backing_trade_fee_policy_count, 0);
    assert_eq!(cfg.backing_trade_fee_bps_long, 0);
    assert_eq!(cfg.backing_trade_fee_bps_short, 0);

    env.svm.expire_blockhash();
    let reopened = env.send(
        env.batch_trade_no_cpi_ix(
            ta,
            la,
            vec![BatchTradeLeg {
                asset_index: 0,
                market_id: first_generation_market_id((0) as u16),
                size_q: sz,
                exec_price: 100,
                fee_bps: 0,
            }],
        ),
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(lp.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ta, false),
            AccountMeta::new(la, false),
        ],
        &[&taker, &lp],
    );
    assert!(
        reopened.is_ok(),
        "clearing every backing-fee policy must restore BatchTradeNoCpi liveness: {reopened:?}"
    );
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(ta), 0).basis_pos_q,
        sz
    );
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(la), 0).basis_pos_q,
        -sz
    );
}

// double-count, clearing one side must leave the gate active if another side remains, and clearing all
#[test]
fn v16_program_non_active_asset_cannot_enable_backing_fee_batch_gate() {
    for (label, action, now_slot, expected_lifecycle) in [
        (
            "DrainOnly",
            percolator_prog::processor::ASSET_ACTION_DRAIN_ONLY,
            0,
            AssetLifecycleV16::DrainOnly,
        ),
        (
            "Recovery",
            percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
            2,
            AssetLifecycleV16::Recovery,
        ),
    ] {
        let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 1_000, 1_000, 500);
        env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
        if action == percolator_prog::processor::ASSET_ACTION_SHUTDOWN {
            env.configure_permissionless_resolve_with_cu(100, 5);
        }
        env.update_market_init_fee_policy_with_cu(1);

        let creator = Keypair::new();
        env.svm.warp_to_slot(1);
        env.activate_permissionless_asset_with_fee(
            &creator,
            1,
            1,
            100,
            creator.pubkey(),
            creator.pubkey(),
            creator.pubkey(),
            creator.pubkey(),
            1,
        );
        if action == percolator_prog::processor::ASSET_ACTION_SHUTDOWN {
            env.try_shutdown_asset_with_authority(&creator, 1, now_slot)
                .expect("asset authority can move its asset to Recovery");
        } else {
            env.update_asset_lifecycle_as_admin_with_cu(action, 1, now_slot, 0);
        }
        let (cfg_after_lifecycle, group_after_lifecycle) = env.market_state();
        assert_eq!(
            group_after_lifecycle.assets[1].lifecycle,
            expected_lifecycle
        );
        assert_eq!(cfg_after_lifecycle.backing_trade_fee_policy_count, 0);

        env.svm.expire_blockhash();
        let policy = send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::UpdateBackingFeePolicy {
                market_id: 0,
                policy_sequence: u64::MAX,
                domain: 2,
                fee_bps: 77,
                insurance_share_bps: 5_000,
            },
            vec![
                AccountMeta::new(creator.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&creator],
        );
        assert!(
            policy.is_err(),
            "{label} asset must not install a new backing-fee policy that globally gates batch trades"
        );
        assert_eq!(
            env.market_state().0.backing_trade_fee_policy_count,
            0,
            "rejected {label} policy update must not enable the global batch gate"
        );

        let taker = Keypair::new();
        let lp = Keypair::new();
        let ta = env.create_portfolio(&taker);
        let la = env.create_portfolio(&lp);
        env.deposit(&taker, ta, 1_000_000);
        env.deposit(&lp, la, 1_000_000);
        let sz = (5 * POS_SCALE) as i128;
        env.svm.expire_blockhash();
        let batch = env.send(
            env.batch_trade_no_cpi_ix(
                ta,
                la,
                vec![BatchTradeLeg {
                    asset_index: 0,
                    market_id: first_generation_market_id((0) as u16),
                    size_q: sz,
                    exec_price: 100,
                    fee_bps: 0,
                }],
            ),
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(lp.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(ta, false),
                AccountMeta::new(la, false),
            ],
            &[&taker, &lp],
        );
        assert!(
            batch.is_ok(),
            "rejected {label} asset policy must leave unrelated asset-0 batch trading live: {batch:?}"
        );
        assert_eq!(
            active_leg_for_asset(&env.portfolio_state(ta), 0).basis_pos_q,
            sz
        );
        assert_eq!(
            active_leg_for_asset(&env.portfolio_state(la), 0).basis_pos_q,
            -sz
        );
    }
}

// any taker could force an external matcher call that is guaranteed to be discarded later.
#[test]
fn v16_program_inactive_asset_tradecpi_rejects_before_hostile_matcher_cpi() {
    for lifecycle_case in ["Retired", "DrainOnly", "Recovery"] {
        let mut env = V16CuEnv::new();
        let creator = Keypair::new();
        env.update_market_init_fee_policy_with_cu(1);
        if lifecycle_case == "Recovery" {
            env.configure_permissionless_resolve_with_cu(100, 5);
        }
        env.svm.warp_to_slot(1);
        env.activate_permissionless_asset_with_fee(
            &creator,
            1,
            1,
            100,
            creator.pubkey(),
            creator.pubkey(),
            creator.pubkey(),
            creator.pubkey(),
            1,
        );

        let hostile = Pubkey::new_unique();
        env.svm.add_program(
            hostile,
            &std::fs::read(hostile_matcher_program_path()).unwrap(),
        );
        let taker = Keypair::new();
        let lp = Keypair::new();
        let taker_account = env.create_portfolio(&taker);
        let lp_account = env.create_portfolio(&lp);
        env.deposit(&taker, taker_account, 1_000_000);
        env.deposit(&lp, lp_account, 1_000_000);
        let ctx = Pubkey::new_unique();
        let delegate = matcher_delegate_key(
            &env.program_id,
            &env.market,
            &lp_account,
            &lp.pubkey(),
            &hostile,
            &ctx,
        );
        env.svm
            .set_account(
                delegate,
                Account {
                    lamports: 1_000_000_000,
                    data: vec![],
                    owner: Pubkey::default(),
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        env.svm
            .set_account(
                ctx,
                Account {
                    lamports: 1_000_000_000,
                    data: vec![0u8; MATCHER_CONTEXT_LEN],
                    owner: hostile,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        env.set_matcher_config(hostile, &lp, lp_account, ctx, delegate, 1);

        let set_hostile_mode = |env: &mut V16CuEnv| {
            let mut data = vec![0u8; MATCHER_CONTEXT_LEN];
            data[0] = 0; // hostile over-fill mode: if CPI occurs, validation fails.
            env.svm
                .set_account(
                    ctx,
                    Account {
                        lamports: 1_000_000_000,
                        data,
                        owner: hostile,
                        executable: false,
                        rent_epoch: 0,
                    },
                )
                .unwrap();
        };
        let accounts = |env: &V16CuEnv| {
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker_account, false),
                AccountMeta::new(lp_account, false),
                AccountMeta::new_readonly(hostile, false),
                AccountMeta::new(ctx, false),
                AccountMeta::new_readonly(delegate, false),
            ]
        };

        set_hostile_mode(&mut env);
        env.svm.expire_blockhash();
        let fresh_err = env
            .send(
                env.trade_cpi_ix(taker_account, lp_account, 1, POS_SCALE as i128, 0, 0),
                accounts(&env),
                &[&taker],
            )
            .expect_err("fresh active TradeCpi sentinel should reach matcher validation");
        assert!(
            fresh_err.contains("InvalidAccountData"),
            "fresh active TradeCpi should fail from hostile matcher validation, got {fresh_err}"
        );

        match lifecycle_case {
            "Retired" => {
                env.svm.warp_to_slot(3);
                env.update_asset_lifecycle_as_admin_with_cu(
                    percolator_prog::processor::ASSET_ACTION_RETIRE,
                    1,
                    3,
                    0,
                );
                assert_eq!(
                    env.market_state().1.assets[1].lifecycle,
                    AssetLifecycleV16::Retired
                );
            }
            "DrainOnly" => {
                env.update_asset_lifecycle_as_admin_with_cu(
                    percolator_prog::processor::ASSET_ACTION_DRAIN_ONLY,
                    1,
                    0,
                    0,
                );
                assert_eq!(
                    env.market_state().1.assets[1].lifecycle,
                    AssetLifecycleV16::DrainOnly
                );
            }
            "Recovery" => {
                env.svm.warp_to_slot(3);
                env.update_asset_lifecycle_as_admin_with_cu(
                    percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
                    1,
                    3,
                    0,
                );
                assert_eq!(
                    env.market_state().1.assets[1].lifecycle,
                    AssetLifecycleV16::Recovery
                );
            }
            _ => unreachable!(),
        }

        for (route, batch) in [("TradeCpi", false), ("BatchTradeCpi", true)] {
            set_hostile_mode(&mut env);
            let market_before = env.svm.get_account(&env.market).unwrap();
            let taker_before = env.svm.get_account(&taker_account).unwrap();
            let lp_before = env.svm.get_account(&lp_account).unwrap();
            let ctx_before = env.svm.get_account(&ctx).unwrap();
            env.svm.expire_blockhash();
            let rejected = if batch {
                env.send(
                    env.batch_trade_cpi_ix(
                        taker_account,
                        lp_account,
                        vec![BatchTradeCpiLeg {
                            asset_index: 1,
                            market_id: first_generation_market_id((1) as u16),
                            size_q: POS_SCALE as i128,
                            fee_bps: 0,
                            limit_price: 0,
                        }],
                    ),
                    accounts(&env),
                    &[&taker],
                )
            } else {
                env.send(
                    env.trade_cpi_ix(taker_account, lp_account, 1, POS_SCALE as i128, 0, 0),
                    accounts(&env),
                    &[&taker],
                )
            }
            .expect_err("inactive-asset CPI trade must reject before matcher CPI");
            assert!(
                rejected.contains("Custom(21)") || rejected.contains("custom program error: 0x15"),
                "{lifecycle_case} {route} rejection should be EngineLockActive, got {rejected}"
            );
            assert!(
                !rejected.contains("InvalidAccountData"),
                "{lifecycle_case} {route} rejection must not reach hostile matcher validation: {rejected}"
            );
            assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
            assert_eq!(env.svm.get_account(&taker_account).unwrap(), taker_before);
            assert_eq!(env.svm.get_account(&lp_account).unwrap(), lp_before);
            assert_eq!(
                env.svm.get_account(&ctx).unwrap(),
                ctx_before,
                "{lifecycle_case} {route} rejection never gives the hostile matcher a writable context"
            );
        }
    }
}

// control reaches hostile matcher validation; the policy-on path must fail at the wrapper gate first.
#[test]
fn v16_program_batch_tradecpi_backing_fee_policy_rejects_before_hostile_matcher_cpi() {
    let mut env = V16CuEnv::new();
    let hostile = Pubkey::new_unique();
    env.svm.add_program(
        hostile,
        &std::fs::read(hostile_matcher_program_path()).unwrap(),
    );
    let taker = Keypair::new();
    let lp = Keypair::new();
    let ta = env.create_portfolio(&taker);
    let la = env.create_portfolio(&lp);
    env.deposit(&taker, ta, 1_000_000);
    env.deposit(&lp, la, 1_000_000);

    let ctx = Pubkey::new_unique();
    let delegate = matcher_delegate_key(
        &env.program_id,
        &env.market,
        &la,
        &lp.pubkey(),
        &hostile,
        &ctx,
    );
    env.svm
        .set_account(
            delegate,
            Account {
                lamports: 1_000_000_000,
                data: vec![],
                owner: Pubkey::default(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm
        .set_account(
            ctx,
            Account {
                lamports: 1_000_000_000,
                data: vec![0u8; MATCHER_CONTEXT_LEN],
                owner: hostile,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.set_matcher_config(hostile, &lp, la, ctx, delegate, 1);

    let send = |env: &mut V16CuEnv| {
        let mut data = vec![0u8; MATCHER_CONTEXT_LEN];
        data[0] = 0; // hostile over-fill mode: if CPI occurs, validation fails InvalidAccountData.
        env.svm
            .set_account(
                ctx,
                Account {
                    lamports: 1_000_000_000,
                    data,
                    owner: hostile,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        env.svm.expire_blockhash();
        env.send(
            env.batch_trade_cpi_ix(
                ta,
                la,
                vec![BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: first_generation_market_id((0) as u16),
                    size_q: (5 * POS_SCALE) as i128,
                    fee_bps: 0,
                    limit_price: 0,
                }],
            ),
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(ta, false),
                AccountMeta::new(la, false),
                AccountMeta::new_readonly(hostile, false),
                AccountMeta::new(ctx, false),
                AccountMeta::new_readonly(delegate, false),
            ],
            &[&taker],
        )
    };

    let no_policy_err = send(&mut env)
        .expect_err("without backing-fee policy, hostile batch should reach matcher validation");
    assert!(
        no_policy_err.contains("InvalidAccountData"),
        "no-policy hostile control should fail from matcher-return validation, got {no_policy_err}"
    );
    assert!(
        !no_policy_err.contains("Custom(9)"),
        "no-policy hostile control must not trip the backing-fee policy gate: {no_policy_err}"
    );

    env.update_backing_fee_policy_with_cu(0, 77, 5_000);
    assert_eq!(
        env.market_state().0.backing_trade_fee_policy_count,
        1,
        "test setup must activate the backing-fee batch gate"
    );
    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&ta).unwrap();
    let lp_before = env.svm.get_account(&la).unwrap();
    let rejected = send(&mut env)
        .expect_err("backing-fee policy BatchTradeCpi must reject before matcher CPI");
    assert!(
        rejected.contains("Custom(9)"),
        "backing-fee policy BatchTradeCpi must fail as InvalidInstruction before hostile matcher validation, got {rejected}"
    );
    assert!(
        !rejected.contains("InvalidAccountData"),
        "backing-fee policy BatchTradeCpi must not reach hostile matcher validation: {rejected}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "policy preflight leaves market bytes unchanged"
    );
    assert_eq!(
        env.svm.get_account(&ta).unwrap(),
        taker_before,
        "policy preflight leaves taker bytes unchanged"
    );
    assert_eq!(
        env.svm.get_account(&la).unwrap(),
        lp_before,
        "policy preflight leaves LP bytes unchanged"
    );
}

#[test]
fn v16_bpf_cross_margin_positive_pnl_allows_trading_negative_leg_before_convert() {
    const INITIAL_PRICE: u64 = 100;
    const ASSET0_MARK: u64 = 105;
    const ASSET1_MARK: u64 = 95;
    const ASSET0_SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const ASSET1_SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const DEPOSIT: u128 = 320;
    const EXPECTED_POSITIVE_PNL: i128 = 100;
    const EXPECTED_NET_PNL_AFTER_NEGATIVE_CLOSE: i128 = 50;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(4, 1_000, 1_000, 500);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, INITIAL_PRICE);
    env.configure_auth_mark_for_asset_as_admin(1, 1, INITIAL_PRICE);

    let cross_owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let cross_account = env.create_portfolio(&cross_owner);
    let counterparty_account = env.create_portfolio(&counterparty_owner);
    env.deposit(&cross_owner, cross_account, DEPOSIT);
    env.deposit(&counterparty_owner, counterparty_account, 1_000);

    env.trade_asset_with_cu(
        0,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        ASSET0_SIZE_Q,
        INITIAL_PRICE,
        0,
    );
    env.trade_asset_with_cu(
        1,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        ASSET1_SIZE_Q,
        INITIAL_PRICE,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, ASSET0_MARK);
    env.push_auth_mark_for_asset_as_admin(1, 2, ASSET1_MARK);

    for (portfolio, asset_index, label) in [
        (
            counterparty_account,
            0,
            "counterparty asset[0] loss refresh",
        ),
        (cross_account, 0, "cross account asset[0] gain refresh"),
        (
            counterparty_account,
            1,
            "counterparty asset[1] gain refresh",
        ),
    ] {
        let cu = env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(asset_index),
            },
        );
        assert_cu_within(label, cu, CRANK_CU_LIMIT);
    }
    let (_, moved_group) = env.market_state();
    assert_eq!(moved_group.assets[0].effective_price, ASSET0_MARK);
    assert_eq!(moved_group.assets[1].effective_price, ASSET1_MARK);

    let cross_before_close = env.portfolio_state(cross_account);
    assert_eq!(cross_before_close.pnl.get(), EXPECTED_POSITIVE_PNL);
    assert_eq!(cross_before_close.capital.get(), DEPOSIT);
    assert_eq!(
        active_leg_for_asset(&cross_before_close, 1).basis_pos_q,
        ASSET1_SIZE_Q,
        "asset[1] is a long leg with negative mark-to-market at the moved price"
    );

    let close_negative_leg_cu = env.trade_asset_with_cu(
        1,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        -ASSET1_SIZE_Q,
        ASSET1_MARK,
        0,
    );
    assert_cu_within(
        "cross-margin close negative leg before pnl convert",
        close_negative_leg_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );

    let cross_after_close = env.portfolio_state(cross_account);
    assert!(
        has_active_leg_for_asset(&cross_after_close, 0),
        "positive-PnL leg should remain open"
    );
    assert!(
        !has_active_leg_for_asset(&cross_after_close, 1),
        "negative-PnL leg should close without converting positive PnL first"
    );
    assert_eq!(cross_after_close.capital.get(), DEPOSIT);
    assert_eq!(
        cross_after_close.pnl.get(),
        EXPECTED_NET_PNL_AFTER_NEGATIVE_CLOSE,
        "asset[1] loss should net against the existing source-backed positive PnL"
    );
}

#[test]
fn v16_bpf_tradenocpi_allows_both_counterparties_with_capitalized_losses_to_risk_reduce() {
    let mut env = V16CuEnv::new();
    env.top_up_insurance(1_000_000);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 10_000);
    env.deposit(&short_owner, short_account, 10_000);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        (10 * POS_SCALE) as i128,
        100,
        0,
    );
    env.force_portfolio_loss_for_security_test(long_account, 500);
    env.force_portfolio_loss_for_security_test(short_account, 500);

    let (_, before_group) = env.market_state();
    let before_long = env.portfolio_state(long_account);
    let before_short = env.portfolio_state(short_account);
    assert!(
        before_long.pnl.get() < 0
            && before_long.capital.get() > before_long.pnl.get().unsigned_abs()
    );
    assert!(
        before_short.pnl.get() < 0
            && before_short.capital.get() > before_short.pnl.get().unsigned_abs()
    );

    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        -((10 * POS_SCALE) as i128),
        100,
        0,
    );

    let (_, after_group) = env.market_state();
    let after_long = env.portfolio_state(long_account);
    let after_short = env.portfolio_state(short_account);
    assert_eq!(
        after_group.insurance, before_group.insurance,
        "capitalized losses must settle from account capital, not insurance"
    );
    assert_eq!(after_long.pnl.get(), 0);
    assert_eq!(after_short.pnl.get(), 0);
    assert!(!has_active_leg_for_asset(&after_long, 0));
    assert!(!has_active_leg_for_asset(&after_short, 0));
}

#[test]
fn v16_bpf_liquidatable_solvent_account_can_risk_reduce_without_insurance_drain() {
    let mut env = V16CuEnv::new();
    env.top_up_insurance(1_000_000);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, 100);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 10_000);
    env.deposit(&short_owner, short_account, 3_000);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        (10 * POS_SCALE) as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, 300);
    env.crank(
        short_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
    );
    env.svm.warp_to_slot(3);
    env.crank(
        short_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
    );

    let (_, before_group) = env.market_state();
    let before_short = env.portfolio_state(short_account);
    assert_eq!(before_group.insurance, 1_000_000);
    assert!(
        health_cert(&before_short).certified_liq_deficit != 0,
        "short account should be liquidatable before the safe risk reduction"
    );
    assert!(
        health_cert(&before_short).certified_equity > 0,
        "short account should still be solvent before the safe risk reduction"
    );

    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        -((10 * POS_SCALE) as i128),
        500,
        0,
    );

    let (_, after_group) = env.market_state();
    let after_long = env.portfolio_state(long_account);
    let after_short = env.portfolio_state(short_account);
    assert_eq!(
        after_group.insurance, before_group.insurance,
        "safe risk reduction must not consume or credit insurance"
    );
    assert_eq!(
        after_group.c_tot + after_group.insurance + after_group.pnl_pos_tot,
        after_group.vault
    );
    assert!(!has_active_leg_for_asset(&after_long, 0));
    assert!(!has_active_leg_for_asset(&after_short, 0));
    assert!(
        health_cert(&after_long).certified_liq_deficit == 0
            && health_cert(&after_short).certified_liq_deficit == 0,
        "both accounts must be non-liquidatable after the risk reduction"
    );
}
