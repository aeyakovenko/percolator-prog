//! Retained requests bind the whole LP position episode, not just the requested asset's leg.
//! All position and capability changes use public instructions; the matcher context is
//! system-created and owner-initialized. The existing CU fixture supplies blank wrapper
//! accounts and token balances before public initialization/deposit.

use super::*;

#[test]
fn v16_program_retained_exit_rejects_cross_asset_episode_under_live_matcher_grant() {
    const DEPOSIT: u128 = 1_000_000;
    const PRICE: u64 = 100;
    const SIZE: i128 = POS_SCALE as i128;
    const EXPIRY: u64 = 100;
    const FEE_CAP: u16 = 37;

    for batch in [false, true] {
        let route = if batch { "BatchTradeCpi" } else { "TradeCpi" };
        let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
        for asset in 0..2 {
            env.configure_auth_mark_for_asset_as_admin(asset, 1, PRICE);
        }
        let matcher_program = Pubkey::new_unique();
        env.svm.add_program(
            matcher_program,
            &std::fs::read(auth_matcher_program_path()).expect("read auth matcher SBF"),
        );
        let taker_owner = Keypair::new();
        let lp_owner = Keypair::new();
        let sibling_owner = Keypair::new();
        let taker = env.create_portfolio(&taker_owner);
        let lp = env.create_portfolio(&lp_owner);
        let sibling = env.create_portfolio(&sibling_owner);
        let taker_source = env.deposit(&taker_owner, taker, DEPOSIT);
        let lp_source = env.deposit(&lp_owner, lp, DEPOSIT);
        let sibling_source = env.deposit(&sibling_owner, sibling, DEPOSIT);
        let (context, delegate, _) =
            env.init_auth_matcher_context_via_system_create(matcher_program, &lp_owner, lp);
        env.try_set_matcher_config_with_trade_fee_cap_and_expiry(
            matcher_program,
            &lp_owner,
            lp,
            context,
            delegate,
            1,
            FEE_CAP,
            EXPIRY,
        )
        .expect("owner installs bounded matcher grant before any retained request");

        let trade_ix = |env: &V16CuEnv, account_a: Pubkey, asset_index: u16, size_q: i128| {
            if batch {
                env.batch_trade_cpi_ix_with_caps(
                    account_a,
                    lp,
                    vec![BatchTradeCpiLeg {
                        asset_index,
                        market_id: env.asset_market_id(asset_index),
                        size_q,
                        fee_bps: 0,
                        limit_price: PRICE,
                    }],
                    0,
                    0,
                )
            } else {
                env.trade_cpi_ix(account_a, lp, asset_index, size_q, 0, PRICE)
            }
        };
        let transaction =
            |env: &V16CuEnv, ix: &ProgInstruction, owner: &Keypair, account_a: Pubkey| {
                Transaction::new_signed_with_payer(
                    &[
                        heap_ix(),
                        cu_ix(),
                        Instruction {
                            program_id: env.program_id,
                            accounts: vec![
                                AccountMeta::new(owner.pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(account_a, false),
                                AccountMeta::new(lp, false),
                                AccountMeta::new_readonly(matcher_program, false),
                                AccountMeta::new(context, false),
                                AccountMeta::new_readonly(delegate, false),
                            ],
                            data: ix.encode(),
                        },
                    ],
                    Some(&env.payer.pubkey()),
                    &[&env.payer, owner],
                    env.svm.latest_blockhash(),
                )
            };
        // The control word also contains the position epoch, so compare grant scope separately.
        let grant_scope = |env: &V16CuEnv| {
            let config = env.portfolio_matcher_config(lp);
            (
                config.matcher_program,
                config.matcher_context,
                config.matcher_delegate,
                config.enabled(),
                config.trade_fee_cap_bps(),
                env.portfolio_matcher_sequence(lp),
                env.portfolio_matcher_expiry(lp),
            )
        };
        let original_grant = grant_scope(&env);
        assert_eq!(original_grant.3, 1);
        assert_eq!(original_grant.4, FEE_CAP);
        assert_eq!(original_grant.6, EXPIRY);
        let open = transaction(&env, &trade_ix(&env, taker, 0, SIZE), &taker_owner, taker);
        env.svm
            .send_transaction(open)
            .expect("matcher opens the asset-0 position to be exited");
        assert_eq!(grant_scope(&env), original_grant);
        let requested_leg = active_leg_for_asset(&env.portfolio_state(lp), 0);
        assert_eq!(requested_leg.basis_pos_q, -SIZE);
        let retained_ix = trade_ix(&env, taker, 0, -SIZE);
        let retained = transaction(&env, &retained_ix, &taker_owner, taker);
        assert_eq!(
            retained.signatures.len(),
            2,
            "payer and taker only, no LP signer"
        );
        assert!(
            bincode::serialized_size(&retained).unwrap()
                <= solana_sdk::packet::PACKET_DATA_SIZE as u64
        );
        let retained_blockhash = retained.message.recent_blockhash;
        let retained_slot = env.svm.get_sysvar::<Clock>().slot;
        assert!(retained_slot < EXPIRY);
        let old_lp_epoch = env.portfolio_position_epoch(lp);
        let old_taker_epoch = env.portfolio_position_epoch(taker);
        let portfolio_ids = [taker, lp, sibling].map(|key| env.portfolio_id(key));
        let generations = [0, 1].map(|asset| env.asset_market_id(asset));
        let unchanged_taker = env.svm.get_account(&taker).unwrap();
        let frame_keys = [
            env.market,
            taker,
            lp,
            sibling,
            context,
            env.vault,
            env.mint,
            taker_source,
            lp_source,
            sibling_source,
            taker_owner.pubkey(),
            lp_owner.pubkey(),
            sibling_owner.pubkey(),
        ];
        // Exclude only the separate network fee payer, not any economic account's lamports.
        let snapshot = |env: &V16CuEnv| frame_keys.map(|key| env.svm.get_account(&key).unwrap());
        let before_simulation = snapshot(&env);
        let live = env
            .svm
            .simulate_transaction(retained.clone().into())
            .expect("the exact signed retained exit is initially executable");
        assert!(live
            .logs
            .iter()
            .any(|line| line == &format!("Program {matcher_program} success")));
        assert_eq!(snapshot(&env), before_simulation);

        let mut writer_cu = Vec::new();
        // Only asset 1 and the third taker change. Returning to the original position vector
        // must not revive asset-0 consent, even when every mutation used this same matcher.
        for (step, size) in [SIZE, -SIZE].into_iter().enumerate() {
            let writer = transaction(
                &env,
                &trade_ix(&env, sibling, 1, size),
                &sibling_owner,
                sibling,
            );
            let meta = env
                .svm
                .send_transaction(writer)
                .expect("public sibling-asset matcher fill");
            assert!(meta
                .logs
                .iter()
                .any(|line| line == &format!("Program {matcher_program} success")));
            writer_cu.push(meta.compute_units_consumed);
            assert_eq!(
                env.portfolio_position_epoch(lp),
                old_lp_epoch + step as u64 + 1
            );
            assert_eq!(env.portfolio_position_epoch(taker), old_taker_epoch);
            assert_eq!(env.svm.get_account(&taker).unwrap(), unchanged_taker);
            assert_eq!(
                active_leg_for_asset(&env.portfolio_state(lp), 0),
                requested_leg
            );
            assert_eq!(
                grant_scope(&env),
                original_grant,
                "{route}: no grant invalidation or renewal"
            );
            for portfolio in [lp, sibling] {
                let state = env.portfolio_state(portfolio);
                assert_eq!(has_active_leg_for_asset(&state, 1), step == 0);
                if step == 0 {
                    assert_eq!(
                        active_leg_for_asset(&state, 1).basis_pos_q,
                        if portfolio == lp { -SIZE } else { SIZE }
                    );
                }
            }
        }
        assert_eq!(
            [taker, lp, sibling].map(|key| env.portfolio_id(key)),
            portfolio_ids
        );
        assert_eq!([0, 1].map(|asset| env.asset_market_id(asset)), generations);
        assert_eq!(env.svm.get_sysvar::<Clock>().slot, retained_slot);
        assert_eq!(env.svm.latest_blockhash(), retained_blockhash);
        assert_eq!(
            percolator::active_bitmap_count_ones(active_bitmap(&env.portfolio_state(lp))),
            1
        );

        let before_rejection = snapshot(&env);
        let rejected = env
            .svm
            .send_transaction(retained)
            .expect_err("old LP episode must reject");
        assert_eq!(
            rejected.err,
            solana_sdk::transaction::TransactionError::InstructionError(
                2,
                solana_sdk::instruction::InstructionError::Custom(
                    PercolatorError::EngineStale as u32
                )
            ),
            "{route}: the application episode guard, not transport or expiry, must reject"
        );
        assert!(
            rejected
                .meta
                .logs
                .iter()
                .all(|line| !line.starts_with(&format!("Program {matcher_program} invoke"))),
            "{route}: reject before matcher CPI, not only transaction rollback"
        );
        assert_eq!(
            snapshot(&env),
            before_rejection,
            "{route}: exact stale-request rollback"
        );

        // Re-sign with only the LP episode refreshed: same asset, leg, grant, accounts, and bounds.
        let mut fresh_ix = retained_ix;
        match &mut fresh_ix {
            ProgInstruction::TradeCpi {
                account_b_position_epoch,
                ..
            }
            | ProgInstruction::BatchTradeCpi {
                account_b_position_epoch,
                ..
            } => {
                *account_b_position_epoch = env.portfolio_position_epoch(lp);
            }
            _ => unreachable!(),
        }
        let fresh = transaction(&env, &fresh_ix, &taker_owner, taker);
        let fresh = env
            .svm
            .send_transaction(fresh)
            .expect("current LP episode must still exit");
        assert!(fresh
            .logs
            .iter()
            .any(|line| line == &format!("Program {matcher_program} success")));
        assert_eq!(grant_scope(&env), original_grant);
        assert_eq!(env.portfolio_position_epoch(lp), old_lp_epoch + 3);
        assert_eq!(env.portfolio_position_epoch(taker), old_taker_epoch + 1);
        for portfolio in [taker, lp, sibling] {
            let state = env.portfolio_state(portfolio);
            assert_eq!(
                percolator::active_bitmap_count_ones(active_bitmap(&state)),
                0
            );
            assert_eq!(state.capital.get(), DEPOSIT);
            assert_eq!(state.pnl.get(), 0);
        }
        let group = env.market_state().1;
        for asset in &group.assets[..2] {
            assert_eq!(asset.oi_eff_long_q, 0);
            assert_eq!(asset.oi_eff_short_q, 0);
        }
        assert_eq!(group.c_tot, 3 * DEPOSIT);
        assert_eq!(group.vault, 3 * DEPOSIT);
        assert_eq!(env.token_amount(env.vault), (3 * DEPOSIT) as u64);
        for (key, before) in frame_keys.iter().zip(&before_rejection) {
            if ![env.market, taker, lp, context].contains(key) {
                assert_eq!(
                    &env.svm.get_account(key).unwrap(),
                    before,
                    "{route}: unrelated account {key}"
                );
            }
        }
        assert_cu_within(
            "cross-asset stale episode preflight",
            rejected.meta.compute_units_consumed,
            CUSTODY_CU_LIMIT,
        );
        for cu in [live.compute_units_consumed, fresh.compute_units_consumed] {
            assert_cu_within(
                "cross-asset retained/current exit",
                cu,
                MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
            );
        }
        println!(
            "INV-012 cross-asset episode {route} CU: live={}, sibling={writer_cu:?}, stale={}, fresh={}",
            live.compute_units_consumed, rejected.meta.compute_units_consumed, fresh.compute_units_consumed
        );
    }
}
