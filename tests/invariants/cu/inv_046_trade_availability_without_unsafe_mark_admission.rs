//! INV-046 - Trade availability without unsafe mark admission.
//!
//! Normative obligation: Unsafe raw prices cannot poison state or remove every bounded user exit route.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_attack_nocpi_high_notional_ewma_exit_not_dosed_by_extreme_reported_price`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_attack_nocpi_high_notional_ewma_exit_not_dosed_by_extreme_reported_price() {
    const MARK: u64 = 1_000_000;
    const CAP_BPS: u64 = 50;
    const DEPOSIT: u128 = 4_900_000_000_000;
    const OPEN_Q: i128 = 4_800_000_000_000;
    const CLOSE_Q: i128 = -(POS_SCALE as i128);

    for path in [
        NoCpiReportedPricePath::Single,
        NoCpiReportedPricePath::Batch,
    ] {
        let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
            initial_price: MARK,
            h_max: 20,
            max_trading_fee_bps: 37,
            max_price_move_bps_per_slot: CAP_BPS,
            max_accrual_dt_slots: 20,
            min_funding_lifetime_slots: 20,
            ..V16CuMarketParams::default()
        });
        env.svm.warp_to_slot(1);
        env.configure_ewma_mark_with_cu(1, MARK, 1, 0);
        env.svm.warp_to_slot(5);
        let (owner_a, account_a, owner_b, account_b) =
            funded_no_cpi_reported_price_pair(&mut env, DEPOSIT);

        try_no_cpi_reported_price_trade_with_cu(
            &mut env, path, &owner_a, account_a, &owner_b, account_b, OPEN_Q, MARK, 0,
        )
        .unwrap_or_else(|err| panic!("{path:?}: high-notional setup open failed: {err}"));
        let (_, opened_group) = env.market_state();
        assert_eq!(
            opened_group.assets[0].oi_eff_long_q,
            OPEN_Q.unsigned_abs(),
            "{path:?}: setup creates high-notional long OI"
        );
        assert_eq!(
            opened_group.assets[0].oi_eff_short_q,
            OPEN_Q.unsigned_abs(),
            "{path:?}: setup creates high-notional short OI"
        );
        assert!(
            opened_group.vault < percolator::MAX_VAULT_TVL,
            "{path:?}: setup stays public-reachable under the vault cap"
        );

        env.svm.expire_blockhash();
        let exit = try_no_cpi_reported_price_trade_with_cu(
            &mut env,
            path,
            &owner_a,
            account_a,
            &owner_b,
            account_b,
            CLOSE_Q,
            percolator::MAX_ORACLE_PRICE,
            0,
        );
        assert!(
            exit.is_ok(),
            "{path:?}: high-notional EWMA exit must not be DoSed by valid extreme reported price: {exit:?}"
        );
        let (_, after) = env.market_state();
        assert_eq!(
            after.assets[0].oi_eff_long_q,
            opened_group.assets[0].oi_eff_long_q - POS_SCALE,
            "{path:?}: high-notional exit reduces long OI"
        );
        assert_eq!(
            after.assets[0].oi_eff_short_q,
            opened_group.assets[0].oi_eff_short_q - POS_SCALE,
            "{path:?}: high-notional exit reduces short OI"
        );
        assert_eq!(
            after.vault as u64,
            env.token_amount(env.vault),
            "{path:?}: high-notional EWMA exit keeps vault accounting tied to SPL custody"
        );
        assert!(
            after.vault >= after.c_tot + after.insurance,
            "{path:?}: high-notional EWMA exit preserves senior conservation"
        );
    }
}

#[test]
fn v16_bpf_tradenocpi_allows_off_mark_strict_reduction_without_value_extraction() {
    let mut env = V16CuEnv::new();
    env.top_up_insurance(1_000_000);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, 100);

    let extractor_owner = Keypair::new();
    let probe_owner = Keypair::new();
    let extractor = env.create_portfolio(&extractor_owner);
    let probe = env.create_portfolio(&probe_owner);
    env.deposit(&extractor_owner, extractor, 10_000);
    env.deposit(&probe_owner, probe, 1_000);
    env.trade_with_cu(
        &extractor_owner,
        extractor,
        &probe_owner,
        probe,
        (10 * POS_SCALE) as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, 300);
    env.crank(
        probe,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
    );
    env.svm.warp_to_slot(3);
    env.crank(
        probe,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
    );
    let before_market = env.svm.get_account(&env.market).unwrap();
    let before_probe = env.svm.get_account(&probe).unwrap();
    let (_, before_group) = state::read_market(&before_market.data).unwrap();
    assert_eq!(before_group.insurance, 1_000_000);
    let before_probe_state = state::read_portfolio(&before_probe.data).unwrap();
    let vault_tokens_before = env.token_amount(env.vault);
    assert!(
        health_cert(&before_probe_state).certified_liq_deficit != 0,
        "probe must be liquidatable before the attempted recycling trade"
    );

    let close_cu = env
        .try_trade_asset_with_cu(
            0,
            &extractor_owner,
            extractor,
            &probe_owner,
            probe,
            -((10 * POS_SCALE) as i128),
            500,
            0,
        )
        .expect("an extreme reported price cannot block a bilateral strict reduction");
    assert_cu_within(
        "off-mark TradeNoCpi strict reduction",
        close_cu,
        TRADE_CU_LIMIT,
    );
    let (_, after_group) = env.market_state();
    let after_extractor = env.portfolio_state(extractor);
    let after_probe = env.portfolio_state(probe);
    let pair_equity_after = after_extractor.capital.get() as i128
        + after_extractor.pnl.get()
        + after_probe.capital.get() as i128
        + after_probe.pnl.get();
    assert!(!has_active_leg_for_asset(&after_extractor, 0));
    assert!(!has_active_leg_for_asset(&after_probe, 0));
    assert_eq!(after_group.assets[0].oi_eff_long_q, 0);
    assert_eq!(after_group.assets[0].oi_eff_short_q, 0);
    assert_eq!(
        after_group.assets[0].effective_price,
        before_group.assets[0].effective_price
    );
    assert_eq!(
        after_group.assets[0].raw_oracle_target_price,
        before_group.assets[0].raw_oracle_target_price
    );
    assert_eq!(after_group.insurance, before_group.insurance);
    assert_eq!(after_group.vault, before_group.vault);
    assert_eq!(env.token_amount(env.vault), vault_tokens_before);
    let fair_gain = 10 * (before_group.assets[0].effective_price as i128 - 100);
    assert_eq!(after_extractor.pnl.get(), fair_gain);
    assert_eq!(after_probe.pnl.get(), -(fair_gain - 1_000));
    assert_eq!(pair_equity_after, 11_000);
    assert!(after_group.vault >= after_group.c_tot + after_group.insurance);
}

#[test]
fn v16_bpf_tradecpi_allows_off_mark_strict_reduction_without_value_extraction() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    env.top_up_insurance(1_000_000);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, 100);

    let extractor_owner = Keypair::new();
    let probe_owner = Keypair::new();
    let extractor = env.create_portfolio(&extractor_owner);
    let probe = env.create_portfolio(&probe_owner);
    env.deposit(&extractor_owner, extractor, 10_000);
    env.deposit(&probe_owner, probe, 1_000);
    env.trade_with_cu(
        &extractor_owner,
        extractor,
        &probe_owner,
        probe,
        (10 * POS_SCALE) as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, 300);
    env.crank(
        probe,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
    );
    env.svm.warp_to_slot(3);
    env.crank(
        probe,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
    );
    let (matcher_ctx, matcher_delegate, _) = env
        .init_matcher_context_with_passive_spread_authorized(
            matcher_program,
            &extractor_owner,
            extractor,
            9_000,
            9_000,
        );
    let before_market = env.svm.get_account(&env.market).unwrap();
    let before_probe = env.svm.get_account(&probe).unwrap();
    let before_matcher = env.svm.get_account(&matcher_ctx).unwrap();
    let (before_cfg, before_group) = state::read_market(&before_market.data).unwrap();
    let before_probe_state = state::read_portfolio(&before_probe.data).unwrap();
    let vault_tokens_before = env.token_amount(env.vault);
    assert!(
        health_cert(&before_probe_state).certified_liq_deficit != 0,
        "probe must be liquidatable before the attempted matcher recycling trade"
    );

    let close_cu = env
        .try_trade_cpi_with_cu_on_asset(
            &probe_owner,
            probe,
            &extractor_owner,
            extractor,
            matcher_program,
            matcher_ctx,
            matcher_delegate,
            0,
            (10 * POS_SCALE) as i128,
            0,
        )
        .expect("an extreme matcher quote cannot block a bilateral strict reduction");
    assert_cu_within(
        "off-mark TradeCpi strict reduction",
        close_cu,
        TRADE_CU_LIMIT,
    );
    let (after_cfg, after_group) = env.market_state();
    let after_extractor = env.portfolio_state(extractor);
    let after_probe = env.portfolio_state(probe);
    let pair_equity_after = after_extractor.capital.get() as i128
        + after_extractor.pnl.get()
        + after_probe.capital.get() as i128
        + after_probe.pnl.get();
    assert!(!has_active_leg_for_asset(&after_extractor, 0));
    assert!(!has_active_leg_for_asset(&after_probe, 0));
    assert_eq!(after_group.assets[0].oi_eff_long_q, 0);
    assert_eq!(after_group.assets[0].oi_eff_short_q, 0);
    assert_eq!(
        after_group.assets[0].effective_price,
        before_group.assets[0].effective_price
    );
    assert_eq!(
        after_group.assets[0].raw_oracle_target_price,
        before_group.assets[0].raw_oracle_target_price
    );
    assert_eq!(after_group.insurance, before_group.insurance);
    assert_eq!(after_group.vault, before_group.vault);
    assert_eq!(env.token_amount(env.vault), vault_tokens_before);
    let fair_gain = 10 * (before_group.assets[0].effective_price as i128 - 100);
    assert_eq!(after_extractor.pnl.get(), fair_gain);
    assert_eq!(after_probe.pnl.get(), -(fair_gain - 1_000));
    assert_eq!(pair_equity_after, 11_000);
    assert!(after_group.vault >= after_group.c_tot + after_group.insurance);
    assert_eq!(after_cfg.matcher_req_seq, before_cfg.matcher_req_seq + 1);
    assert_ne!(
        env.svm.get_account(&matcher_ctx).unwrap().data,
        before_matcher.data,
        "the successful CPI fill commits its matcher response"
    );
}

#[test]
fn v16_bpf_batch_tradecpi_allows_off_mark_strict_reduction_without_value_extraction() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    env.top_up_insurance(1_000_000);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, 100);

    let extractor_owner = Keypair::new();
    let probe_owner = Keypair::new();
    let extractor = env.create_portfolio(&extractor_owner);
    let probe = env.create_portfolio(&probe_owner);
    env.deposit(&extractor_owner, extractor, 10_000);
    env.deposit(&probe_owner, probe, 1_000);
    env.trade_with_cu(
        &extractor_owner,
        extractor,
        &probe_owner,
        probe,
        (10 * POS_SCALE) as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, 300);
    env.crank(
        probe,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
    );
    env.svm.warp_to_slot(3);
    env.crank(
        probe,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
    );

    let (matcher_ctx, matcher_delegate, _) =
        env.init_auth_matcher_context(matcher_program, &extractor_owner, extractor);
    let mut configure_spread = vec![4u8];
    configure_spread.extend_from_slice(&9_000u64.to_le_bytes());
    configure_spread.extend_from_slice(&9_000u64.to_le_bytes());
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        Instruction {
            program_id: matcher_program,
            accounts: vec![
                AccountMeta::new_readonly(extractor_owner.pubkey(), true),
                AccountMeta::new(matcher_ctx, false),
            ],
            data: configure_spread,
        },
        &[&extractor_owner],
    )
    .expect("configure auth matcher spread");

    let before_market = env.svm.get_account(&env.market).unwrap();
    let before_probe = env.svm.get_account(&probe).unwrap();
    let (_, before_group) = state::read_market(&before_market.data).unwrap();
    let before_probe_state = state::read_portfolio(&before_probe.data).unwrap();
    let vault_tokens_before = env.token_amount(env.vault);
    assert!(
        health_cert(&before_probe_state).certified_liq_deficit != 0,
        "probe must be liquidatable before the attempted batch matcher recycling trade"
    );

    env.svm.expire_blockhash();
    let close_cu = env
        .send(
            env.batch_trade_cpi_ix(
                probe,
                extractor,
                vec![BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: env.asset_market_id(0),
                    size_q: (10 * POS_SCALE) as i128,
                    fee_bps: 0,
                    limit_price: 0,
                }],
            ),
            vec![
                AccountMeta::new(probe_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(probe, false),
                AccountMeta::new(extractor, false),
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new(matcher_ctx, false),
                AccountMeta::new_readonly(matcher_delegate, false),
            ],
            &[&probe_owner],
        )
        .expect(
            "a one-leg batch CPI extreme matcher quote cannot block a bilateral strict reduction",
        );
    assert_cu_within(
        "off-mark BatchTradeCpi strict reduction",
        close_cu,
        TRADE_CU_LIMIT,
    );

    let (_, after_group) = env.market_state();
    let after_extractor = env.portfolio_state(extractor);
    let after_probe = env.portfolio_state(probe);
    let pair_equity_after = after_extractor.capital.get() as i128
        + after_extractor.pnl.get()
        + after_probe.capital.get() as i128
        + after_probe.pnl.get();
    assert!(!has_active_leg_for_asset(&after_extractor, 0));
    assert!(!has_active_leg_for_asset(&after_probe, 0));
    assert_eq!(after_group.assets[0].oi_eff_long_q, 0);
    assert_eq!(after_group.assets[0].oi_eff_short_q, 0);
    assert_eq!(
        after_group.assets[0].effective_price,
        before_group.assets[0].effective_price
    );
    assert_eq!(
        after_group.assets[0].raw_oracle_target_price,
        before_group.assets[0].raw_oracle_target_price
    );
    assert_eq!(after_group.insurance, before_group.insurance);
    assert_eq!(after_group.vault, before_group.vault);
    assert_eq!(env.token_amount(env.vault), vault_tokens_before);
    let fair_gain = 10 * (before_group.assets[0].effective_price as i128 - 100);
    assert_eq!(after_extractor.pnl.get(), fair_gain);
    assert_eq!(after_probe.pnl.get(), -(fair_gain - 1_000));
    assert_eq!(pair_equity_after, 11_000);
    assert!(after_group.vault >= after_group.c_tot + after_group.insurance);
}
