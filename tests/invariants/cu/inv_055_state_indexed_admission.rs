//! INV-055 - State-indexed admission.
//!
//! Normative obligation: Each lifecycle mode admits only its explicitly allowed operation set.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_attack_batch_nocpi_mixed_exit_and_fresh_open_rejects_atomically`, `v16_attack_spare_capacity_asset_rejects_public_routes_before_matcher`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_attack_batch_nocpi_mixed_exit_and_fresh_open_rejects_atomically() {
    for lifecycle_case in ["DrainOnly", "Recovery"] {
        let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 5_000, 10_000, 1_000);
        if lifecycle_case == "Recovery" {
            env.configure_permissionless_resolve_with_cu(100, 50);
        }
        env.configure_auth_mark_for_asset_as_admin(1, 1, 100);

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
        assert!(has_active_leg_for_asset(
            &env.portfolio_state(taker_account),
            0
        ));
        assert!(!has_active_leg_for_asset(
            &env.portfolio_state(taker_account),
            1
        ));

        match lifecycle_case {
            "DrainOnly" => {
                for asset_index in [0, 1] {
                    env.update_asset_lifecycle_as_admin_with_cu(
                        percolator_prog::processor::ASSET_ACTION_DRAIN_ONLY,
                        asset_index,
                        0,
                        0,
                    );
                    assert_eq!(
                        env.market_state().1.assets[asset_index as usize].lifecycle,
                        AssetLifecycleV16::DrainOnly
                    );
                }
            }
            "Recovery" => {
                env.svm.warp_to_slot(10);
                for asset_index in [0, 1] {
                    env.update_asset_lifecycle_as_admin_with_cu(
                        percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
                        asset_index,
                        10,
                        0,
                    );
                    assert_eq!(
                        env.market_state().1.assets[asset_index as usize].lifecycle,
                        AssetLifecycleV16::Recovery
                    );
                }
            }
            _ => unreachable!(),
        }

        let market_before = env.svm.get_account(&env.market).unwrap();
        let taker_before = env.svm.get_account(&taker_account).unwrap();
        let lp_before = env.svm.get_account(&lp_account).unwrap();
        env.svm.expire_blockhash();
        let mixed = env.send(
            ProgInstruction::BatchTradeNoCpi {
                legs: vec![
                    BatchTradeLeg {
                        asset_index: 0,
                        market_id: first_generation_market_id(0),
                        size_q: -(POS_SCALE as i128),
                        exec_price: 100,
                        fee_bps: 0,
                    },
                    BatchTradeLeg {
                        asset_index: 1,
                        market_id: first_generation_market_id(1),
                        size_q: POS_SCALE as i128,
                        exec_price: 100,
                        fee_bps: 0,
                    },
                ],
            },
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(lp.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker_account, false),
                AccountMeta::new(lp_account, false),
            ],
            &[&taker, &lp],
        );
        assert!(
            mixed.is_err(),
            "{lifecycle_case} mixed BatchTradeNoCpi exit+open must reject"
        );
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_before,
            "{lifecycle_case} mixed rejection must roll back market state"
        );
        assert_eq!(
            env.svm.get_account(&taker_account).unwrap(),
            taker_before,
            "{lifecycle_case} mixed rejection must roll back taker state"
        );
        assert_eq!(
            env.svm.get_account(&lp_account).unwrap(),
            lp_before,
            "{lifecycle_case} mixed rejection must roll back LP state"
        );

        env.svm.expire_blockhash();
        let reduce_cu = env
            .send(
                ProgInstruction::BatchTradeNoCpi {
                    legs: vec![BatchTradeLeg {
                        asset_index: 0,
                        market_id: first_generation_market_id(0),
                        size_q: -(POS_SCALE as i128),
                        exec_price: 100,
                        fee_bps: 0,
                    }],
                },
                vec![
                    AccountMeta::new(taker.pubkey(), true),
                    AccountMeta::new(lp.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(taker_account, false),
                    AccountMeta::new(lp_account, false),
                ],
                &[&taker, &lp],
            )
            .expect("standalone lifecycle reduction must remain live after mixed rejection");
        assert_cu_within(
            &format!("{lifecycle_case} mixed-reject BatchTradeNoCpi retry"),
            reduce_cu,
            TRADE_CU_LIMIT,
        );
        let (_, group_after) = env.market_state();
        assert_eq!(group_after.assets[0].oi_eff_long_q, 0);
        assert_eq!(group_after.assets[0].oi_eff_short_q, 0);
        assert_eq!(group_after.assets[1].oi_eff_long_q, 0);
        assert_eq!(group_after.assets[1].oi_eff_short_q, 0);
        assert!(!has_active_leg_for_asset(
            &env.portfolio_state(taker_account),
            0
        ));
        assert!(!has_active_leg_for_asset(
            &env.portfolio_state(lp_account),
            0
        ));
        assert!(!has_active_leg_for_asset(
            &env.portfolio_state(taker_account),
            1
        ));
        assert!(!has_active_leg_for_asset(
            &env.portfolio_state(lp_account),
            1
        ));
        assert_eq!(group_after.vault as u64, env.token_amount(env.vault));
        assert!(group_after.vault >= group_after.c_tot + group_after.insurance);
    }
}

#[test]
fn v16_attack_spare_capacity_asset_rejects_public_routes_before_matcher() {
    let params = V16CuMarketParams {
        max_portfolio_assets: 1,
        ..V16CuMarketParams::default()
    };
    let mut env = V16CuEnv::new_with_init_params_and_market_capacity(params, 2);
    let market_account = env.svm.get_account(&env.market).unwrap();
    assert_eq!(
        env.market_state().1.config.max_market_slots,
        1,
        "test setup keeps only asset 0 configured"
    );
    assert_eq!(
        state::market_slot_capacity(&market_account.data).unwrap(),
        2,
        "test setup leaves asset slot 1 as spare stored capacity"
    );

    let taker = Keypair::new();
    let lp = Keypair::new();
    let taker_account = env.create_portfolio(&taker);
    let lp_account = env.create_portfolio(&lp);
    env.deposit(&taker, taker_account, 1_000_000);
    env.deposit(&lp, lp_account, 1_000_000);
    let bad_asset = 1u16;
    let size = POS_SCALE as i128;
    let bad_asset_market_id = env.asset_market_id(bad_asset);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker_account).unwrap();
    let lp_before = env.svm.get_account(&lp_account).unwrap();

    env.svm.expire_blockhash();
    let trade = env.try_trade_asset_with_cu(
        bad_asset,
        &taker,
        taker_account,
        &lp,
        lp_account,
        size,
        100,
        0,
    );
    assert!(
        trade.is_err(),
        "TradeNoCpi against spare capacity asset {bad_asset} must reject"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker_account).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp_account).unwrap(), lp_before);

    env.svm.expire_blockhash();
    let crank = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(bad_asset),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker_account, false),
        ],
        &[],
    );
    assert!(
        crank.is_err(),
        "PermissionlessCrank against spare capacity asset {bad_asset} must reject"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker_account).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp_account).unwrap(), lp_before);

    env.svm.expire_blockhash();
    let batch_nocpi = env.send(
        ProgInstruction::BatchTradeNoCpi {
            legs: vec![BatchTradeLeg {
                asset_index: bad_asset,
                market_id: bad_asset_market_id,
                size_q: size,
                exec_price: 100,
                fee_bps: 0,
            }],
        },
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(lp.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker_account, false),
            AccountMeta::new(lp_account, false),
        ],
        &[&taker, &lp],
    );
    assert!(
        batch_nocpi.is_err(),
        "BatchTradeNoCpi against spare capacity asset {bad_asset} must reject"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker_account).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp_account).unwrap(), lp_before);

    let hostile = Pubkey::new_unique();
    env.svm.add_program(
        hostile,
        &std::fs::read(hostile_matcher_program_path()).unwrap(),
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
    let market_after_matcher_setup = env.svm.get_account(&env.market).unwrap();
    let taker_after_matcher_setup = env.svm.get_account(&taker_account).unwrap();
    let lp_after_matcher_setup = env.svm.get_account(&lp_account).unwrap();
    let ctx_before = env.svm.get_account(&ctx).unwrap();

    env.svm.expire_blockhash();
    let batch_cpi = env.send(
        ProgInstruction::BatchTradeCpi {
            legs: vec![BatchTradeCpiLeg {
                asset_index: bad_asset,
                market_id: bad_asset_market_id,
                size_q: size,
                fee_bps: 0,
                limit_price: 0,
            }],
        },
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker_account, false),
            AccountMeta::new(lp_account, false),
            AccountMeta::new_readonly(hostile, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ],
        &[&taker],
    );
    let err = batch_cpi.expect_err("spare-capacity BatchTradeCpi asset must reject");
    assert!(
        !err.contains("InvalidAccountData"),
        "spare-capacity BatchTradeCpi must not reach hostile matcher validation: {err}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_after_matcher_setup
    );
    assert_eq!(
        env.svm.get_account(&taker_account).unwrap(),
        taker_after_matcher_setup
    );
    assert_eq!(
        env.svm.get_account(&lp_account).unwrap(),
        lp_after_matcher_setup
    );
    assert_eq!(
        env.svm.get_account(&ctx).unwrap(),
        ctx_before,
        "spare-capacity BatchTradeCpi must reject before mutable matcher CPI"
    );
}
