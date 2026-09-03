//! INV-055 - State-indexed admission.
//!
//! Normative obligation: Each lifecycle mode admits only its explicitly allowed operation set.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions):
//! `v16_attack_batch_nocpi_mixed_exit_and_fresh_open_rejects_atomically`,
//! `v16_attack_spare_capacity_asset_rejects_public_routes_before_matcher`, the public
//! ResetPending and retired/reactivated four-route matrices, and the irreversible-close
//! admission/terminal-progress composition below. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token, rollback,
//! liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

fn inv055_assert_init_portfolio_rejects_current_market_mode(env: &mut V16CuEnv, label: &str) {
    let owner = Keypair::new();
    env.ensure_signer_account(owner.pubkey());
    let portfolio = Pubkey::new_unique();
    env.svm
        .set_account(
            portfolio,
            Account {
                lamports: 1_000_000_000,
                data: vec![0; env.portfolio_account_len],
                owner: env.program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&owner],
    );
    assert!(
        rejected.is_err(),
        "{label}: terminal market admitted a fresh portfolio"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "{label}: rejected init changed market state",
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "{label}: rejected init changed the candidate portfolio",
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "{label}: rejected init changed custody",
    );
}

#[test]
fn v16_program_init_portfolio_rejects_public_recovery_and_resolved_modes() {
    let PublicActiveCloseFixture { mut env, loss, .. } = public_asset1_bankrupt_close_fixture();
    let ledger = close_progress(&env.portfolio_state(loss));
    env.svm.warp_to_slot(ledger.max_close_slot + 1);
    env.crank(
        loss,
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: vec![],
        },
    );
    assert_eq!(
        env.market_state().1.mode,
        MarketModeV16::Recovery,
        "expired public close must expose the market-Recovery admission state",
    );
    inv055_assert_init_portfolio_rejects_current_market_mode(&mut env, "Recovery");

    let mut resolved = V16CuEnv::new();
    resolved.resolve();
    assert_eq!(resolved.market_state().1.mode, MarketModeV16::Resolved);
    inv055_assert_init_portfolio_rejects_current_market_mode(&mut resolved, "Resolved");
}

#[test]
fn v16_program_reset_pending_admission_matrix_rejects_risk_then_restores_trade() {
    const OPEN_Q: u128 = 10 * POS_SCALE;
    const PRICE: u64 = 100;

    let mut env = V16CuEnv::new();
    let reducing_owner = Keypair::new();
    let stale_owner = Keypair::new();
    let reducing = env.create_portfolio(&reducing_owner);
    let stale = env.create_portfolio(&stale_owner);
    env.deposit(&reducing_owner, reducing, 1_000_000);
    env.deposit(&stale_owner, stale, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &reducing_owner,
        reducing,
        &stale_owner,
        stale,
        OPEN_Q as i128,
        PRICE,
        0,
    );
    env.rebalance_reduce_with_cu(&reducing_owner, reducing, 0, OPEN_Q);

    let reset = env.market_state().1;
    assert_eq!(reset.assets[0].mode_short, SideModeV16::ResetPending);
    assert_eq!(reset.assets[0].oi_eff_long_q, 0);
    assert_eq!(reset.assets[0].oi_eff_short_q, 0);
    assert_eq!(reset.assets[0].stored_pos_count_short, 1);

    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&lp_owner, lp, 1_000_000);
    let matcher_program = Pubkey::new_unique();
    env.svm.add_program(
        matcher_program,
        &std::fs::read(auth_matcher_program_path()).unwrap(),
    );
    let (matcher_context, matcher_delegate, _) =
        env.init_auth_matcher_context(matcher_program, &lp_owner, lp);

    for route in ["TradeNoCpi", "BatchTradeNoCpi", "TradeCpi", "BatchTradeCpi"] {
        let market_before = env.svm.get_account(&env.market).unwrap();
        let taker_before = env.svm.get_account(&taker).unwrap();
        let lp_before = env.svm.get_account(&lp).unwrap();
        let stale_before = env.svm.get_account(&stale).unwrap();
        let matcher_before = env.svm.get_account(&matcher_context).unwrap();
        let vault_before = env.svm.get_account(&env.vault).unwrap();
        env.svm.expire_blockhash();
        let result = match route {
            "TradeNoCpi" => env.try_trade_asset_with_cu(
                0,
                &taker_owner,
                taker,
                &lp_owner,
                lp,
                POS_SCALE as i128,
                PRICE,
                0,
            ),
            "BatchTradeNoCpi" => env.send(
                env.batch_trade_no_cpi_ix(
                    taker,
                    lp,
                    vec![BatchTradeLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q: POS_SCALE as i128,
                        exec_price: PRICE,
                        fee_bps: 0,
                    }],
                ),
                vec![
                    AccountMeta::new(taker_owner.pubkey(), true),
                    AccountMeta::new(lp_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(taker, false),
                    AccountMeta::new(lp, false),
                ],
                &[&taker_owner, &lp_owner],
            ),
            "TradeCpi" => env.try_trade_cpi_with_cu_on_asset(
                &taker_owner,
                taker,
                &lp_owner,
                lp,
                matcher_program,
                matcher_context,
                matcher_delegate,
                0,
                POS_SCALE as i128,
                0,
            ),
            "BatchTradeCpi" => env.send(
                env.batch_trade_cpi_ix(
                    taker,
                    lp,
                    vec![BatchTradeCpiLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q: POS_SCALE as i128,
                        fee_bps: 0,
                        limit_price: 0,
                    }],
                ),
                vec![
                    AccountMeta::new(taker_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(taker, false),
                    AccountMeta::new(lp, false),
                    AccountMeta::new_readonly(matcher_program, false),
                    AccountMeta::new(matcher_context, false),
                    AccountMeta::new_readonly(matcher_delegate, false),
                ],
                &[&taker_owner],
            ),
            _ => unreachable!(),
        };
        let error = result.expect_err("ResetPending must reject fresh risk on every trade route");
        assert!(
            error.contains("Custom(21)") || error.contains("custom program error: 0x15"),
            "{route} reached the wrong ResetPending guard: {error}"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&taker).unwrap(), taker_before);
        assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before);
        assert_eq!(env.svm.get_account(&stale).unwrap(), stale_before);
        assert_eq!(
            env.svm.get_account(&matcher_context).unwrap(),
            matcher_before
        );
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    }

    let market_before_finalize = env.svm.get_account(&env.market).unwrap();
    env.svm.expire_blockhash();
    let premature_finalize = env.send(
        ProgInstruction::FinalizeResetSide {
            asset_index: 0,
            side: 1,
        },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(premature_finalize.is_err());
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_finalize
    );

    env.crank(
        stale,
        ProgInstruction::PermissionlessCrank {
            now_slot: env.svm.get_sysvar::<Clock>().slot,
            observations: Vec::new(),
        },
    );
    assert!(!has_active_leg_for_asset(&env.portfolio_state(stale), 0));
    assert_eq!(
        env.market_state().1.assets[0].mode_short,
        SideModeV16::ResetPending
    );
    let finalize_cu = env.finalize_reset_side_with_cu(0, 1);
    assert_cu_within(
        "public ResetPending admission finalization",
        finalize_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        env.market_state().1.assets[0].mode_short,
        SideModeV16::Normal
    );

    let reopened_cu = env.trade_asset_with_cu(
        0,
        &taker_owner,
        taker,
        &lp_owner,
        lp,
        POS_SCALE as i128,
        PRICE,
        0,
    );
    assert_cu_within(
        "fresh trade after ResetPending finalization",
        reopened_cu,
        TRADE_CU_LIMIT,
    );
    let reopened = env.market_state().1;
    assert_eq!(reopened.assets[0].oi_eff_long_q, POS_SCALE);
    assert_eq!(reopened.assets[0].oi_eff_short_q, POS_SCALE);
}

#[test]
fn v16_program_irreversible_close_ledger_admits_only_terminal_progress() {
    let PublicActiveCloseFixture {
        mut env,
        loss_owner,
        loss,
        live_counterparty,
        live_peer,
        ..
    } = public_asset1_bankrupt_close_fixture();
    let active_close = close_progress(&env.portfolio_state(loss));
    assert!(active_close.active && !active_close.canceled && !active_close.finalized);
    assert!(active_close.residual_remaining > 0);

    let lp_owner = Keypair::new();
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&lp_owner, lp, 1_000_000);
    let market_before_trade = env.svm.get_account(&env.market).unwrap();
    let loss_before_trade = env.svm.get_account(&loss).unwrap();
    let lp_before_trade = env.svm.get_account(&lp).unwrap();
    let vault_before_trade = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let rejected_trade = env.try_trade_asset_with_cu(
        0,
        &loss_owner,
        loss,
        &lp_owner,
        lp,
        POS_SCALE as i128,
        100,
        0,
    );
    assert!(
        rejected_trade.is_err(),
        "an account with active close progress must not open unrelated live risk"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_trade
    );
    assert_eq!(env.svm.get_account(&loss).unwrap(), loss_before_trade);
    assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before_trade);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before_trade);

    env.svm.expire_blockhash();
    let rejected_close = env.send(
        env.close_portfolio_ix(loss),
        vec![
            AccountMeta::new(loss_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(loss, false),
        ],
        &[&loss_owner],
    );
    assert!(
        rejected_close.is_err(),
        "ClosePortfolio must not erase an active close ledger"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_trade
    );
    assert_eq!(env.svm.get_account(&loss).unwrap(), loss_before_trade);
    assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before_trade);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before_trade);

    let cure_amount = active_close.residual_remaining + 1_000;
    let cure_source = env.token_account(loss_owner.pubkey(), cure_amount as u64);
    let source_before_cure = env.svm.get_account(&cure_source).unwrap();
    env.svm.expire_blockhash();
    let rejected_cure = env.send(
        ProgInstruction::CureAndCancelClose {
            portfolio_id: env.portfolio_id(loss),
            position_epoch: env.portfolio_position_epoch(loss),
            optional_deposit: cure_amount,
        },
        vec![
            AccountMeta::new(loss_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(loss, false),
            AccountMeta::new(cure_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&loss_owner],
    );
    let cure_error = rejected_cure.expect_err("irreversible close progress cannot be canceled");
    assert!(
        cure_error.contains("Custom(21)") || cure_error.contains("custom program error: 0x15"),
        "irreversible cure reached the wrong guard: {cure_error}"
    );
    assert_eq!(
        env.svm.get_account(&cure_source).unwrap(),
        source_before_cure
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_trade
    );
    assert_eq!(env.svm.get_account(&loss).unwrap(), loss_before_trade);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before_trade);

    let live_counterparty_before = env.svm.get_account(&live_counterparty).unwrap();
    let live_peer_before = env.svm.get_account(&live_peer).unwrap();
    env.svm.warp_to_slot(active_close.max_close_slot + 1);
    env.svm.expire_blockhash();
    let progress_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 0,
                observations: Vec::new(),
            },
            vec![
                AccountMeta::new_readonly(env.payer.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(loss, false),
            ],
            &[],
        )
        .expect("expired irreversible close must retain permissionless terminal progress");
    assert_cu_within(
        "irreversible close terminal continuation",
        progress_cu,
        CRANK_CU_LIMIT,
    );
    assert!(matches!(
        env.market_state().1.mode,
        MarketModeV16::Recovery | MarketModeV16::Resolved
    ));
    assert_eq!(
        env.svm.get_account(&live_counterparty).unwrap(),
        live_counterparty_before,
        "terminal admission changes must not rewrite an unrelated portfolio"
    );
    assert_eq!(
        env.svm.get_account(&live_peer).unwrap(),
        live_peer_before,
        "terminal admission changes must not rewrite an unrelated peer"
    );
}

#[test]
fn v16_program_retired_slot_reactivation_restores_fresh_generation_trade_admission() {
    const ASSET_INDEX: u16 = 1;
    const PRICE: u64 = 100;

    for route in ["TradeNoCpi", "BatchTradeNoCpi", "TradeCpi", "BatchTradeCpi"] {
        let mut env = V16CuEnv::new();
        let old_creator = Keypair::new();
        env.update_market_init_fee_policy_with_cu(1);
        env.svm.warp_to_slot(1);
        env.activate_permissionless_asset_with_fee(
            &old_creator,
            ASSET_INDEX,
            1,
            PRICE,
            old_creator.pubkey(),
            old_creator.pubkey(),
            old_creator.pubkey(),
            old_creator.pubkey(),
            1,
        );

        let taker_owner = Keypair::new();
        let lp_owner = Keypair::new();
        let taker = env.create_portfolio(&taker_owner);
        let lp = env.create_portfolio(&lp_owner);
        env.deposit(&taker_owner, taker, 1_000_000);
        env.deposit(&lp_owner, lp, 1_000_000);
        let matcher_program = Pubkey::new_unique();
        env.svm.add_program(
            matcher_program,
            &std::fs::read(auth_matcher_program_path()).unwrap(),
        );
        let (matcher_context, matcher_delegate, _) =
            env.init_auth_matcher_context(matcher_program, &lp_owner, lp);

        env.svm.warp_to_slot(3);
        env.update_asset_lifecycle_as_admin_with_cu(
            processor::ASSET_ACTION_RETIRE,
            ASSET_INDEX,
            3,
            0,
        );
        let retired_generation = env.asset_market_id(ASSET_INDEX);
        assert_eq!(
            env.market_state().1.assets[ASSET_INDEX as usize].lifecycle,
            AssetLifecycleV16::Retired
        );

        let execute = |env: &mut V16CuEnv| match route {
            "TradeNoCpi" => env.try_trade_asset_with_cu(
                ASSET_INDEX,
                &taker_owner,
                taker,
                &lp_owner,
                lp,
                POS_SCALE as i128,
                PRICE,
                0,
            ),
            "BatchTradeNoCpi" => env.send(
                env.batch_trade_no_cpi_ix(
                    taker,
                    lp,
                    vec![BatchTradeLeg {
                        asset_index: ASSET_INDEX,
                        market_id: env.asset_market_id(ASSET_INDEX),
                        size_q: POS_SCALE as i128,
                        exec_price: PRICE,
                        fee_bps: 0,
                    }],
                ),
                vec![
                    AccountMeta::new(taker_owner.pubkey(), true),
                    AccountMeta::new(lp_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(taker, false),
                    AccountMeta::new(lp, false),
                ],
                &[&taker_owner, &lp_owner],
            ),
            "TradeCpi" => env.try_trade_cpi_with_cu_on_asset(
                &taker_owner,
                taker,
                &lp_owner,
                lp,
                matcher_program,
                matcher_context,
                matcher_delegate,
                ASSET_INDEX,
                POS_SCALE as i128,
                0,
            ),
            "BatchTradeCpi" => env.send(
                env.batch_trade_cpi_ix(
                    taker,
                    lp,
                    vec![BatchTradeCpiLeg {
                        asset_index: ASSET_INDEX,
                        market_id: env.asset_market_id(ASSET_INDEX),
                        size_q: POS_SCALE as i128,
                        fee_bps: 0,
                        limit_price: 0,
                    }],
                ),
                vec![
                    AccountMeta::new(taker_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(taker, false),
                    AccountMeta::new(lp, false),
                    AccountMeta::new_readonly(matcher_program, false),
                    AccountMeta::new(matcher_context, false),
                    AccountMeta::new_readonly(matcher_delegate, false),
                ],
                &[&taker_owner],
            ),
            _ => unreachable!(),
        };

        let market_before = env.svm.get_account(&env.market).unwrap();
        let taker_before = env.svm.get_account(&taker).unwrap();
        let lp_before = env.svm.get_account(&lp).unwrap();
        let matcher_before = env.svm.get_account(&matcher_context).unwrap();
        let vault_before = env.svm.get_account(&env.vault).unwrap();
        env.svm.expire_blockhash();
        let error = execute(&mut env).expect_err("Retired asset must reject fresh risk");
        assert!(
            error.contains("Custom(21)") || error.contains("custom program error: 0x15"),
            "{route} reached the wrong Retired admission guard: {error}"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&taker).unwrap(), taker_before);
        assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before);
        assert_eq!(
            env.svm.get_account(&matcher_context).unwrap(),
            matcher_before
        );
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

        let new_creator = Keypair::new();
        env.svm.warp_to_slot(5);
        env.activate_permissionless_asset_with_fee(
            &new_creator,
            ASSET_INDEX,
            5,
            PRICE,
            new_creator.pubkey(),
            new_creator.pubkey(),
            new_creator.pubkey(),
            new_creator.pubkey(),
            1,
        );
        let fresh_generation = env.asset_market_id(ASSET_INDEX);
        assert_ne!(fresh_generation, retired_generation);
        assert_eq!(
            env.market_state().1.assets[ASSET_INDEX as usize].lifecycle,
            AssetLifecycleV16::Active
        );

        env.svm.expire_blockhash();
        let open_cu = execute(&mut env).expect("fresh generation must restore trade admission");
        assert_cu_within(
            &format!("{route} after retired-slot reactivation"),
            open_cu,
            TRADE_CU_LIMIT,
        );
        let opened = env.market_state().1.assets[ASSET_INDEX as usize];
        assert_eq!(opened.oi_eff_long_q, POS_SCALE);
        assert_eq!(opened.oi_eff_short_q, POS_SCALE);

        let close_cu = env.trade_asset_with_cu(
            ASSET_INDEX,
            &taker_owner,
            taker,
            &lp_owner,
            lp,
            -(POS_SCALE as i128),
            PRICE,
            0,
        );
        assert_cu_within(
            &format!("{route} reactivated-generation exit"),
            close_cu,
            TRADE_CU_LIMIT,
        );
        let closed = env.market_state().1.assets[ASSET_INDEX as usize];
        assert_eq!(closed.oi_eff_long_q, 0);
        assert_eq!(closed.oi_eff_short_q, 0);
        assert!(!has_active_leg_for_asset(
            &env.portfolio_state(taker),
            ASSET_INDEX as usize
        ));
        assert!(!has_active_leg_for_asset(
            &env.portfolio_state(lp),
            ASSET_INDEX as usize
        ));
    }
}

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
            env.batch_trade_no_cpi_ix(
                taker_account,
                lp_account,
                vec![
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
            ),
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
                env.batch_trade_no_cpi_ix(
                    taker_account,
                    lp_account,
                    vec![BatchTradeLeg {
                        asset_index: 0,
                        market_id: first_generation_market_id(0),
                        size_q: -(POS_SCALE as i128),
                        exec_price: 100,
                        fee_bps: 0,
                    }],
                ),
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
        env.batch_trade_no_cpi_ix(
            taker_account,
            lp_account,
            vec![BatchTradeLeg {
                asset_index: bad_asset,
                market_id: bad_asset_market_id,
                size_q: size,
                exec_price: 100,
                fee_bps: 0,
            }],
        ),
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
        env.batch_trade_cpi_ix(
            taker_account,
            lp_account,
            vec![BatchTradeCpiLeg {
                asset_index: bad_asset,
                market_id: bad_asset_market_id,
                size_q: size,
                fee_bps: 0,
                limit_price: 0,
            }],
        ),
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

// security.md sweep — resolved-mode operation gating (#30): once resolved, every Live-only op
// (Deposit, Trade, Withdraw, ConvertReleasedPnl) must reject; only the wind-down path (CloseResolved)
// works. A Live-op leaking through after resolution could corrupt the frozen state.
#[test]
fn v16_attack_resolved_mode_gates_all_live_ops() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    let other = Keypair::new();
    let pq = env.create_portfolio(&other); // create BEFORE resolve
    env.deposit(&owner, p, 1_000_000);
    env.resolve();
    let (_, g0) = env.market_state();

    // Deposit -> reject
    let src = env.token_account_for_mint(env.mint, owner.pubkey(), 100);
    env.svm.expire_blockhash();
    let r_dep = env.send(
        env.deposit_ix(p, 100),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(src, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    assert!(r_dep.is_err(), "Deposit must reject in resolved mode");
    // Withdraw -> reject (must use CloseResolved)
    env.svm.expire_blockhash();
    let dest = Pubkey::new_unique();
    env.svm
        .set_account(
            dest,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, owner.pubkey(), 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let r_wd = env.send(
        env.withdraw_ix(p, 100),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    assert!(r_wd.is_err(), "Withdraw must reject in resolved mode");
    // ConvertReleasedPnl -> reject
    env.svm.expire_blockhash();
    let r_cv = env.send(
        env.convert_released_pnl_ix(p, 1),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
        ],
        &[&owner],
    );
    assert!(
        r_cv.is_err(),
        "ConvertReleasedPnl must reject in resolved mode"
    );
    // Trade -> reject
    env.svm.expire_blockhash();
    let r_tr = env.try_trade_asset_with_cu(0, &owner, p, &other, pq, POS_SCALE as i128, 100, 0);
    assert!(r_tr.is_err(), "Trade must reject in resolved mode");

    // nothing changed; CloseResolved (the wind-down path) works.
    let (_, g1) = env.market_state();
    assert_eq!(
        g1.vault, g0.vault,
        "vault unchanged by all rejected live ops"
    );
    assert_eq!(g1.c_tot, g0.c_tot, "c_tot unchanged");
    let cr = env.close_resolved(&owner, p);
    assert_eq!(
        env.token_amount(cr),
        1_000_000,
        "CloseResolved pays out the resolved capital"
    );
}

// security.md sweep - unsigned top-up legacy realloc rollback (#5/#33/#44/#48):
// ClaimResolvedPayoutTopup is intentionally permissionless and grows legacy
// portfolio storage before validating the destination token account. A cranker
// with a bad destination must not be able to leave the victim's legacy account
// security.md sweep — liquidation of a healthy account (#2): an account above maintenance margin must
// NOT be liquidatable. A permissionless action:1 crank against a healthy account must be a no-op — no
// security.md sweep — permissionless resolve gating (#30 DoS): ResolveStalePermissionless lets ANYONE
// resolve a market, but ONLY when the oracle is genuinely stale-matured. It must reject on a fresh
// market (and when not configured) — otherwise an attacker could force resolution as a griefing DoS.
#[test]
fn v16_attack_permissionless_resolve_rejects_fresh_market() {
    let resolve_stale = |env: &mut V16CuEnv, now_slot: u64| -> Result<u64, String> {
        env.svm.warp_to_slot(now_slot);
        env.send(
            ProgInstruction::ResolveStalePermissionless { now_slot },
            vec![AccountMeta::new(env.market, false)],
            &[],
        )
    };
    // 1) DEFAULT env: permissionless_resolve_stale_slots == 0 -> always disabled. Even a huge future
    //    now_slot can't force resolution (slot is authenticated; staleness not configured).
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    assert!(
        resolve_stale(&mut env, 1_000_000).is_err(),
        "permissionless resolve must reject when not configured"
    );
    // market still Live: owner can withdraw (would fail if resolved).
    let (d, _) = env.withdraw_with_cu(&owner, p, 100_000);
    assert_eq!(
        env.token_amount(d),
        100_000,
        "market still Live after rejected permissionless resolve"
    );

    // 2) CONFIGURED env (stale_slots=5) but oracle FRESH -> still rejects.
    let mut env2 = V16CuEnv::new();
    env2.configure_permissionless_resolve_with_cu(5, 5);
    env2.configure_auth_mark_with_cu(0, 100);
    let o2 = Keypair::new();
    let p2 = env2.create_portfolio(&o2);
    env2.deposit(&o2, p2, 1_000_000);
    // keep the oracle fresh by pushing/cranking at slot 3, then try to resolve only 2 slots later.
    env2.svm.warp_to_slot(3);
    env2.push_auth_mark_with_cu(3, 100);
    env2.svm.expire_blockhash();
    let _ = env2.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env2.payer.pubkey(), true),
            AccountMeta::new(env2.market, false),
            AccountMeta::new(p2, false),
        ],
        &[],
    );
    assert!(
        resolve_stale(&mut env2, 4).is_err(),
        "permissionless resolve must reject while the oracle is fresh (only 1 slot stale < 5)"
    );
    // market still Live: a withdraw succeeds (resolved mode would reject it).
    let (d2, _) = env2.withdraw_with_cu(&o2, p2, 100_000);
    assert_eq!(
        env2.token_amount(d2),
        100_000,
        "market still Live after rejected fresh-oracle resolve"
    );
}

// security.md sweep — ClosePortfolio with parked pnl (#48): an account holding positive (junior) pnl
// must NOT be closeable — closing would discard the pnl and its residual backing. ClosePortfolio
// requires PnL == 0; a portfolio with pnl must reject (the value stays recoverable).
#[test]
fn v16_attack_close_portfolio_with_pnl_rejected() {
    let mut env = V16CuEnv::new();
    let ledger = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger, 1, 40, 10);
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.add_source_positive_pnl(p, 1, 40); // p now has +40 pnl, 0 capital
    env.crank(
        p,
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: crank_observations(0),
        },
    );
    assert!(
        env.portfolio_state(p).pnl.get() > 0,
        "p holds parked positive pnl (non-vacuous)"
    );
    // ClosePortfolio must reject (PnL != 0).
    env.svm.expire_blockhash();
    let r = env.send(
        env.close_portfolio_ix(p),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
        ],
        &[&owner],
    );
    assert!(r.is_err(), "ClosePortfolio with parked pnl must reject");
    // the account and its pnl are intact (not discarded), conservation holds.
    assert!(
        env.portfolio_state(p).pnl.get() > 0,
        "parked pnl NOT discarded by the rejected close"
    );
    let (_, g) = env.market_state();
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
}

// security.md sweep — no-fee liquidation cranker reward (#3): with no liquidation fee configured
// (default), a third-party cranker liquidating an insolvent account must receive ZERO reward — no
// security.md sweep — withdraw requires flat account (#19/#46): withdraw_not_atomic requires the
// account to be FLAT (active_bitmap empty) — ANY open position blocks withdrawal, regardless of how
// small the position or how large the capital. After closing, the full capital is recoverable (no
// permanent lock). This documents the flatness gate (not a margin calc).
#[test]
fn v16_attack_withdraw_requires_flat_regardless_of_size() {
    let mut env = V16CuEnv::new();
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 10_000_000);
    env.deposit(&lb, pb, 10_000_000);
    // TINY position (notional 100) vs huge (10M) capital.
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    // even a tiny withdrawal is blocked while ANY position is open (flatness gate, not margin).
    let try_wd = |env: &mut V16CuEnv, amt: u128| -> bool {
        env.svm.expire_blockhash();
        let dd = Pubkey::new_unique();
        env.svm
            .set_account(
                dd,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(env.mint, la.pubkey(), 0),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        env.send(
            env.withdraw_ix(pa, amt),
            vec![
                AccountMeta::new(la.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(pa, false),
                AccountMeta::new(dd, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&la],
        )
        .is_ok()
    };
    assert!(
        !try_wd(&mut env, 1),
        "tiny withdraw blocked while a (tiny) position is open"
    );
    assert!(
        !try_wd(&mut env, 9_000_000),
        "bulk withdraw also blocked while positioned"
    );
    assert_eq!(
        env.portfolio_state(pa).capital.get(),
        10_000_000,
        "capital intact (no partial debit)"
    );
    // close the position -> full capital recoverable (no permanent lock).
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, -(POS_SCALE as i128), 100, 0);
    assert!(
        percolator::active_bitmap_is_empty(active_bitmap(&env.portfolio_state(pa))),
        "la flat after close"
    );
    let cap = env.portfolio_state(pa).capital.get();
    let (d2, _) = env.withdraw_with_cu(&la, pa, cap);
    assert_eq!(
        env.token_amount(d2) as u128,
        cap,
        "full capital recovered after closing (no permanent lock)"
    );
    let (_, g) = env.market_state();
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
}

// security.md sweep — recovery-mode risk lockout (#9/#19/#30): once a market is in Recovery (winding
// down), no NEW risk may be opened — only reductions/wind-down. Attacker goal: open a fresh position
// (or grow one) during recovery to extract value or corrupt the wind-down accounting. Protection: the
// trade handlers require Live mode, so a TradeNoCpi in Recovery rejects with state fully preserved.
#[test]
fn v16_attack_recovery_mode_blocks_new_risk() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let la = Keypair::new();
    let a = env.create_portfolio(&la);
    let lb = Keypair::new();
    let b = env.create_portfolio(&lb);
    env.deposit(&la, a, 1_000_000);
    env.deposit(&lb, b, 1_000_000);
    env.trade_asset_with_cu(0, &la, a, &lb, b, POS_SCALE as i128, 100, 0);
    // transition the market into Recovery (engine backdoor, mirrors v16_bpf_recovery_and_reset_tags).
    env.mutate_market(|_, group| {
        group.mode = MarketModeV16::Recovery;
        group.recovery_reason = Some(PermissionlessRecoveryReasonV16::BelowProgressFloor);
    });
    let before = env.svm.get_account(&env.market).unwrap();
    let g_pre = env.market_state().1;
    assert_eq!(g_pre.mode, MarketModeV16::Recovery, "market is in recovery");

    // ATTACK 1: grow OI on the existing position during recovery -> must reject (mode != Live).
    let r1 = env.try_trade_asset_with_cu(0, &la, a, &lb, b, POS_SCALE as i128, 100, 0);
    assert!(r1.is_err(), "opening new risk in recovery must reject");

    // ATTACK 2: even initializing a fresh portfolio is locked out during recovery.
    let lc = Keypair::new();
    env.ensure_signer_account(lc.pubkey());
    let c = Pubkey::new_unique();
    let plen = env.portfolio_account_len;
    env.svm
        .set_account(
            c,
            Account {
                lamports: 1_000_000_000,
                data: vec![0u8; plen],
                owner: env.program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    let r2 = env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(lc.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(c, false),
        ],
        &[&lc],
    );
    assert!(
        r2.is_err(),
        "InitPortfolio is locked out in recovery (no new accounts during wind-down)"
    );

    // the rejected trades must not have grown OI or minted value.
    let g_post = env.market_state().1;
    assert_eq!(
        g_post.assets[0].oi_eff_long_q, g_pre.assets[0].oi_eff_long_q,
        "OI not grown by rejected recovery trades"
    );
    assert_eq!(
        g_post.assets[0].oi_eff_long_q, g_post.assets[0].oi_eff_short_q,
        "OI still balanced"
    );
    assert!(
        g_post.vault >= g_post.c_tot + g_post.insurance,
        "senior conservation in recovery"
    );
    // the market-level trade-affected state is unchanged vs before the attacks (deposits to c/d only added capital).
    let _ = before;
}

// hostile public-interface sweep: even the legitimate oracle authority must not be able to
// reconfigure an oracle anchor/mode after traders have live exposure. Otherwise a compromised or
// adversarial authority could reset the official price basis under open positions and cause LoF/DoS.
#[test]
fn v16_attack_oracle_reconfiguration_rejects_after_positions_enter_market() {
    let mut env = V16CuEnv::new();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 10_000);
    env.deposit(&short_owner, short_account, 10_000);
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

    let before = env.svm.get_account(&env.market).unwrap().data;
    let (_, before_group) = state::read_market(&before).unwrap();
    assert_eq!(before_group.assets[0].oi_eff_long_q, POS_SCALE);
    assert_eq!(before_group.assets[0].oi_eff_short_q, POS_SCALE);

    env.svm.warp_to_slot(1);
    env.svm.expire_blockhash();
    let auth_reconfig = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: 500,
            authority_epoch: 0,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&env.admin],
    );
    assert!(
        auth_reconfig.is_err(),
        "AuthMark reconfiguration with live OI must reject"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before,
        "failed AuthMark reconfiguration must not mutate market state"
    );

    env.svm.expire_blockhash();
    let ewma_reconfig = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureEwmaMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: 500,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            authority_epoch: 0,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&env.admin],
    );
    assert!(
        ewma_reconfig.is_err(),
        "EwmaMark reconfiguration with live OI must reject"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before,
        "failed EwmaMark reconfiguration must not mutate market state"
    );

    let feed = [42u8; 32];
    let pyth = env.set_pyth_price(&feed, 500, 0, 1);
    let mut feeds = [[0u8; 32]; percolator_prog::constants::ORACLE_LEG_CAP];
    feeds[0] = feed;
    env.svm.expire_blockhash();
    let hybrid_reconfig = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureHybridOracle {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 1,
            now_unix_ts: 1,
            oracle_leg_count: 1,
            oracle_leg_flags: 0,
            max_staleness_secs: 60,
            hybrid_soft_stale_slots: 3,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            invert: 0,
            unit_scale: 0,
            conf_filter_bps: 500,
            oracle_leg_feeds: feeds,
            authority_epoch: 0,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new_readonly(pyth, false),
        ],
        &[&env.admin],
    );
    assert!(
        hybrid_reconfig.is_err(),
        "Hybrid reconfiguration with live OI must reject"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before,
        "failed Hybrid reconfiguration must not mutate market state"
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Inv055AdmissionOwner {
    Bootstrap,
    PortfolioLifecycle,
    TradeLifecycle,
    AutomaticProgress,
    TerminalSettlement,
    Resolution,
    ReserveLifecycle,
    AssetLifecycle,
    OwnerRiskReduction,
    CloseEpisode,
    OracleLifecycle,
    AuthorityPolicy,
    LedgerReconciliation,
    MatcherCapability,
    CollateralConversion,
}

#[derive(Clone, Copy)]
struct Inv055AdmissionEvidence {
    owner: Inv055AdmissionOwner,
    path: &'static str,
    test: &'static str,
}

fn inv055_evidence(
    owner: Inv055AdmissionOwner,
    path: &'static str,
    test: &'static str,
) -> Inv055AdmissionEvidence {
    Inv055AdmissionEvidence { owner, path, test }
}

fn inv055_public_route_admission(variant: &str) -> Option<Inv055AdmissionEvidence> {
    use Inv055AdmissionOwner::*;

    let evidence = match variant {
        "InitMarket" => inv055_evidence(
            Bootstrap,
            "tests/invariants/cu/inv_083_boundary_completeness.rs",
            "v16_attack_init_market_rejects_grief_config_without_burning_market_account",
        ),
        "InitPortfolio" => inv055_evidence(
            PortfolioLifecycle,
            "tests/invariants/cu/inv_055_state_indexed_admission.rs",
            "v16_program_init_portfolio_rejects_public_recovery_and_resolved_modes",
        ),
        "Deposit" | "Withdraw" => inv055_evidence(
            PortfolioLifecycle,
            "tests/invariants/stateful/inv_055_state_indexed_admission.rs",
            "v16_program_user_operation_lifecycle_admission_matrix",
        ),
        "ClosePortfolio" => inv055_evidence(
            PortfolioLifecycle,
            "tests/invariants/cu/inv_055_state_indexed_admission.rs",
            "v16_attack_close_portfolio_with_pnl_rejected",
        ),
        "ConvertReleasedPnl" => inv055_evidence(
            PortfolioLifecycle,
            "tests/invariants/cu/inv_054_certificate_epoch_completeness.rs",
            "v16_attack_convert_released_pnl_requires_current_cert_and_public_refresh",
        ),
        "SyncMaintenanceFee" => inv055_evidence(
            PortfolioLifecycle,
            "tests/invariants/cu/inv_073_no_permanent_user_lock.rs",
            "v16_program_sync_maintenance_rejects_when_resolve_matured",
        ),
        "TradeNoCpi" | "TradeCpi" | "BatchTradeNoCpi" | "BatchTradeCpi" => inv055_evidence(
            TradeLifecycle,
            "tests/invariants/cu/inv_055_state_indexed_admission.rs",
            "v16_program_reset_pending_admission_matrix_rejects_risk_then_restores_trade",
        ),
        "PermissionlessCrank" => inv055_evidence(
            AutomaticProgress,
            "tests/invariants/cu/inv_072_order_robust_crankability.rs",
            "v16_program_every_auto_crank_plan_and_hint_parser_stratum_has_public_evidence",
        ),
        "CloseResolved" | "ClaimResolvedPayoutTopup" => inv055_evidence(
            TerminalSettlement,
            "tests/invariants/cu/inv_068_receipt_uniqueness_and_monotonic_topups.rs",
            "v16_program_resolved_receipt_replays_extract_no_value_on_any_public_rail",
        ),
        "CloseSlab" => inv055_evidence(
            TerminalSettlement,
            "tests/invariants/cu/inv_070_zero_unattributed_terminal_residue_and_close_slab.rs",
            "v16_program_close_slab_rejects_until_market_has_zero_terminal_residue",
        ),
        "ResolveMarket" => inv055_evidence(
            Resolution,
            "tests/invariants/cu/inv_047_equivalent_route_semantics.rs",
            "v16_program_authority_and_permissionless_resolution_match_at_maturity",
        ),
        "ResolveStalePermissionless" => inv055_evidence(
            Resolution,
            "tests/invariants/cu/inv_073_no_permanent_user_lock.rs",
            "v16_bpf_permissionless_stale_resolve_is_bounded_and_oracle_free",
        ),
        "ConfigurePermissionlessResolve" => inv055_evidence(
            Resolution,
            "tests/invariants/cu/inv_087_no_phantom_controls_or_dead_security_fields.rs",
            "v16_program_configure_permissionless_resolve_gated_and_bounded",
        ),
        "TopUpInsurance" | "TopUpInsuranceDomain" => inv055_evidence(
            ReserveLifecycle,
            "tests/invariants/cu/inv_047_equivalent_route_semantics.rs",
            "v16_program_legacy_insurance_topup_matches_explicit_domain_split",
        ),
        "WithdrawInsuranceAsset" => inv055_evidence(
            ReserveLifecycle,
            "tests/invariants/cu/inv_064_insurance_withdrawal_policy_equivalence.rs",
            "v16_attack_live_insurance_asset_withdraw_uniform_for_asset0_and_permissionless_asset",
        ),
        "TopUpBackingBucket" | "WithdrawBackingBucket" => inv055_evidence(
            ReserveLifecycle,
            "tests/invariants/cu/inv_028_source_domain_realizability_cap.rs",
            "v16_attack_backing_bucket_topup_withdraw_input_gates",
        ),
        "WithdrawBackingBucketEarnings" => inv055_evidence(
            ReserveLifecycle,
            "tests/invariants/cu/inv_018_quote_mint_vault_token_program_and_authority_integrity.rs",
            "v16_public_backing_earnings_withdrawal_matches_spl_and_internal_quote_deltas",
        ),
        "UpdateAssetLifecycle" => inv055_evidence(
            AssetLifecycle,
            "tests/invariants/cu/inv_065_reset_recovery_and_retired_state_isolation.rs",
            "v16_program_reset_pending_rejects_fresh_counterparty_and_completes_recovery",
        ),
        "FinalizeResetSide" => inv055_evidence(
            AssetLifecycle,
            "tests/invariants/cu/inv_065_reset_recovery_and_retired_state_isolation.rs",
            "v16_attack_finalize_reset_side_requires_empty_side_counts",
        ),
        "RestartAssetOracle" => inv055_evidence(
            AssetLifecycle,
            "tests/invariants/cu/inv_069_terminal_normalization_and_retirement.rs",
            "v16_program_spent_only_recovery_asset_can_restart_without_value_drift",
        ),
        "RebalanceReduce" => inv055_evidence(
            OwnerRiskReduction,
            "tests/invariants/cu/inv_057_risk_reduction_availability.rs",
            "v16_attack_non_base_local_stale_owner_reduce_remains_live",
        ),
        "ForfeitRecoveryLeg" => inv055_evidence(
            OwnerRiskReduction,
            "tests/invariants/stateful/inv_081_success_state_validity_over_complete_public_routes.rs",
            "v16_program_owner_recovery_forfeit_strictly_reduces_each_position_episode",
        ),
        "ForceCloseAbandonedAsset" => inv055_evidence(
            OwnerRiskReduction,
            "tests/invariants/cu/inv_078_permissionless_recovery_coverage.rs",
            "v16_attack_locally_stale_permissionless_asset_can_shutdown_and_force_close",
        ),
        "CureAndCancelClose" => inv055_evidence(
            CloseEpisode,
            "tests/invariants/cu/inv_076_close_drift_residual_durability_and_finalization_atomicity.rs",
            "v16_program_public_close_zero_cure_rejects_atomically_and_terminal_progress_remains",
        ),
        "ConfigureHybridOracle" => inv055_evidence(
            OracleLifecycle,
            "tests/invariants/cu/inv_056_hints_are_discovery_only_favorable_actions_fully_refresh.rs",
            "v16_program_external_oracle_hint_and_account_order_is_normalized_or_atomic",
        ),
        "ConfigureEwmaMark" | "PushEwmaMark" => inv055_evidence(
            OracleLifecycle,
            "tests/invariants/cu/inv_020_authenticated_clock_slot_and_oracle_provenance.rs",
            "v16_bpf_configure_and_push_ewma_mark_are_bounded_and_clock_authenticated",
        ),
        "ConfigureAuthMark" | "PushAuthMark" => inv055_evidence(
            OracleLifecycle,
            "tests/invariants/cu/inv_020_authenticated_clock_slot_and_oracle_provenance.rs",
            "v16_bpf_configure_and_push_auth_mark_are_bounded_and_clock_authenticated",
        ),
        "UpdateAuthority" | "UpdateAssetAuthority" => inv055_evidence(
            AuthorityPolicy,
            "tests/invariants/cu/inv_005_authority_incarnation_binding.rs",
            "v16_attack_update_authority_requires_new_authority_signature",
        ),
        "UpdateLiquidationFeePolicy"
        | "UpdateMaintenanceFeePolicy"
        | "UpdateBackingFeePolicy"
        | "UpdateTradeFeePolicy"
        | "UpdateFeeRedirectPolicy"
        | "UpdateMarketInitFeePolicy"
        | "UpdateBaseUnitMints" => inv055_evidence(
            AuthorityPolicy,
            "tests/invariants/cu/inv_087_no_phantom_controls_or_dead_security_fields.rs",
            "v16_bpf_policy_authority_and_base_unit_tags_are_bounded_and_persist",
        ),
        "SyncBackingDomainLedger" | "SyncInsuranceLedger" => inv055_evidence(
            LedgerReconciliation,
            "tests/invariants/cu/inv_025_exact_stock_reconciliation.rs",
            "v16_bpf_accounting_ledger_tags_are_bounded_and_update_state",
        ),
        "SetMatcherConfig" => inv055_evidence(
            MatcherCapability,
            "tests/invariants/stateful/inv_010_out_of_order_safety.rs",
            "v16_program_conflicting_matcher_controls_and_trade_exhaust_all_landing_orders",
        ),
        "SwapSecondaryForPrimary" => inv055_evidence(
            CollateralConversion,
            "tests/invariants/cu/inv_017_signer_writable_role_and_account_alias_safety.rs",
            "v16_attack_swap_secondary_unauthorized_and_bounded",
        ),
        _ => return None,
    };
    Some(evidence)
}

fn inv055_source_defines_test(source: &str, function: &str) -> bool {
    let marker = format!("fn {function}");
    source.lines().any(|line| {
        line.trim()
            .strip_prefix(&marker)
            .is_some_and(|tail| tail.trim_start().starts_with('('))
    })
}

fn inv055_braced_body_after<'a>(source: &'a str, marker: &str) -> &'a str {
    let start = source
        .find(marker)
        .unwrap_or_else(|| panic!("missing production marker {marker}"));
    let open = start
        + source[start..]
            .find('{')
            .unwrap_or_else(|| panic!("missing body after {marker}"));
    let mut depth = 0i32;
    for (offset, character) in source[open..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &source[(open + 1)..(open + offset)];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated body after {marker}");
}

#[test]
fn v16_program_every_public_instruction_has_a_state_admission_owner() {
    const REGISTRY: &str = include_str!("../public_instruction_coverage.tsv");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut variants = std::collections::BTreeSet::new();
    let mut owners = std::collections::BTreeMap::<Inv055AdmissionOwner, usize>::new();
    let mut witness_cache = std::collections::BTreeMap::<&str, String>::new();

    for line in REGISTRY.lines() {
        if line.is_empty() || line.starts_with('#') || line.starts_with("tag\t") {
            continue;
        }
        let fields = line.split('\t').collect::<Vec<_>>();
        assert_eq!(fields.len(), 5, "malformed public instruction row: {line}");
        let variant = fields[1];
        assert!(variants.insert(variant), "duplicate public route {variant}");
        let evidence = inv055_public_route_admission(variant)
            .unwrap_or_else(|| panic!("{variant} has no state-admission owner"));
        *owners.entry(evidence.owner).or_default() += 1;
        let source = witness_cache.entry(evidence.path).or_insert_with(|| {
            std::fs::read_to_string(root.join(evidence.path))
                .unwrap_or_else(|error| panic!("read {}: {error}", evidence.path))
        });
        assert!(
            inv055_source_defines_test(source, evidence.test),
            "{variant} points to missing admission witness {}#{}",
            evidence.path,
            evidence.test,
        );
    }
    assert_eq!(variants.len(), 49, "public instruction roster drift");
    assert_eq!(
        owners.len(),
        15,
        "every admission-owner family must remain represented",
    );

    let production = include_str!("../../../src/v16_program.rs");
    let production = production
        .split("    #[cfg(test)]\n    mod tests")
        .next()
        .expect("production prefix exists");
    for (handler, guard) in [
        ("fn handle_init_portfolio", "mode != MarketModeV16::Live"),
        ("fn handle_deposit", "mode != MarketModeV16::Live"),
        ("fn handle_withdraw", "mode != MarketModeV16::Live"),
        (
            "fn handle_batch_trade_nocpi",
            "mode_pre != MarketModeV16::Live",
        ),
        (
            "fn handle_trade_nocpi<'a>",
            "mode_pre != MarketModeV16::Live",
        ),
        ("fn handle_trade_cpi<'a>", "mode_pre != MarketModeV16::Live"),
        (
            "fn handle_batch_trade_cpi",
            "mode_pre != MarketModeV16::Live",
        ),
        ("fn handle_convert_released_pnl", "group.header.mode != 0"),
        ("fn handle_close_slab", "group.header.mode != 1"),
        ("fn handle_resolve_market", "group.header.mode != 0"),
        (
            "fn handle_update_asset_lifecycle",
            "mode_pre != MarketModeV16::Live",
        ),
        ("fn handle_restart_asset_oracle", "group.header.mode != 0"),
        (
            "fn handle_configure_hybrid_oracle",
            "group.header.mode != 0",
        ),
        ("fn handle_configure_managed_mark", "group.header.mode != 0"),
        ("fn handle_push_managed_mark", "group.header.mode != 0"),
        ("fn handle_close_resolved", "group.header.mode != 1"),
    ] {
        assert!(
            inv055_braced_body_after(production, handler).contains(guard),
            "{handler} lost state-admission guard {guard}",
        );
    }
    for (handler, transition) in [
        (
            "fn handle_permissionless_crank<'a>",
            "handle_permissionless_crank_zero_copy(",
        ),
        (
            "fn handle_forfeit_recovery_leg",
            ".forfeit_recovery_leg_not_atomic(",
        ),
        (
            "fn handle_rebalance_reduce",
            ".rebalance_reduce_position_not_atomic(",
        ),
        (
            "fn handle_finalize_reset_side",
            ".finalize_side_reset_not_atomic(",
        ),
        (
            "fn handle_close_resolved",
            ".permissionless_auto_crank_not_atomic(",
        ),
        (
            "fn handle_claim_resolved_payout_topup",
            ".claim_resolved_payout_topup_not_atomic(",
        ),
    ] {
        assert!(
            inv055_braced_body_after(production, handler).contains(transition),
            "{handler} lost canonical state-machine transition {transition}",
        );
    }
}
