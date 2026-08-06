//! INV-019 - CPI invocation and return-data binding.
//!
//! Normative obligation: CPI results bind the exact invocation, matcher context, and economic request.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_attack_tradecpi_rejects_program_owned_matcher_tail_before_cpi`, `v16_attack_matcher_context_replay_after_lp_close_reinit_rejects`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_attack_tradecpi_rejects_program_owned_matcher_tail_before_cpi() {
    #[derive(Clone, Copy)]
    enum Route {
        Single,
        Batch,
    }

    for route in [Route::Single, Route::Batch] {
        let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
        env.configure_auth_mark_for_asset_as_admin(1, 1, 100);

        let matcher_program = Pubkey::new_unique();
        let matcher_bytes =
            std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
        env.svm.add_program(matcher_program, &matcher_bytes);
        let taker = Keypair::new();
        let lp = Keypair::new();
        let taker_account = env.create_portfolio(&taker);
        let lp_account = env.create_portfolio(&lp);
        env.deposit(&taker, taker_account, 3_000_000);
        env.deposit(&lp, lp_account, 3_000_000);
        let (ctx, delegate, _) = env.init_auth_matcher_context(matcher_program, &lp, lp_account);

        let wrapper_owned_tail = Pubkey::new_unique();
        env.svm
            .set_account(
                wrapper_owned_tail,
                Account {
                    lamports: 1_000_000,
                    data: vec![0u8; 16],
                    owner: env.program_id,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let benign_tail = Pubkey::new_unique();
        env.svm
            .set_account(
                benign_tail,
                Account {
                    lamports: 1_000_000,
                    data: vec![0u8; 16],
                    owner: Pubkey::default(),
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();

        let accounts = |env: &V16CuEnv, tail: Pubkey| {
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker_account, false),
                AccountMeta::new(lp_account, false),
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new(ctx, false),
                AccountMeta::new_readonly(delegate, false),
                AccountMeta::new_readonly(tail, false),
            ]
        };
        let send_route = |env: &mut V16CuEnv, tail: Pubkey| match route {
            Route::Single => env.send(
                ProgInstruction::TradeCpi {
                    asset_index: 0,
                    market_id: first_generation_market_id(0),
                    size_q: (5 * POS_SCALE) as i128,
                    fee_bps: 100,
                    limit_price: 0,
                },
                accounts(env, tail),
                &[&taker],
            ),
            Route::Batch => env.send(
                ProgInstruction::BatchTradeCpi {
                    legs: vec![
                        BatchTradeCpiLeg {
                            asset_index: 0,
                            market_id: first_generation_market_id(0),
                            size_q: (5 * POS_SCALE) as i128,
                            fee_bps: 100,
                            limit_price: 0,
                        },
                        BatchTradeCpiLeg {
                            asset_index: 1,
                            market_id: first_generation_market_id(1),
                            size_q: -(5 * POS_SCALE as i128),
                            fee_bps: 100,
                            limit_price: 0,
                        },
                    ],
                },
                accounts(env, tail),
                &[&taker],
            ),
        };

        let ctx_before = env.svm.get_account(&ctx).unwrap();
        env.svm.expire_blockhash();
        let rejected = send_route(&mut env, wrapper_owned_tail)
            .expect_err("program-owned matcher tail must reject before CPI");
        assert!(
            rejected.contains("Custom(9)") || rejected.contains("custom program error: 0x9"),
            "program-owned matcher tail must reject as InvalidInstruction, got {rejected}"
        );
        assert_eq!(
            env.svm.get_account(&ctx).unwrap(),
            ctx_before,
            "program-owned matcher tail must reject before the external matcher writes context"
        );
        assert!(
            !has_active_leg_for_asset(&env.portfolio_state(taker_account), 0),
            "rejected program-owned tail must not fill the taker"
        );

        env.svm.expire_blockhash();
        let accepted = send_route(&mut env, benign_tail);
        assert!(
            accepted.is_ok(),
            "same CPI route must remain live with a benign external tail: {accepted:?}"
        );
        let taker_after = env.portfolio_state(taker_account);
        assert!(
            has_active_leg_for_asset(&taker_after, 0),
            "benign-tail control fills a real taker leg"
        );
        if let Route::Batch = route {
            assert!(
                has_active_leg_for_asset(&taker_after, 1),
                "benign-tail batch control fills the second leg"
            );
        }
    }
}

#[test]
fn v16_attack_matcher_context_replay_after_lp_close_reinit_rejects() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    let hostile = Pubkey::new_unique();
    env.svm.add_program(
        hostile,
        &std::fs::read(hostile_matcher_program_path()).unwrap(),
    );
    let taker = Keypair::new();
    let lp = Keypair::new();
    let taker_account = env.create_portfolio(&taker);
    let lp_account = env.create_portfolio(&lp);
    env.deposit(&taker, taker_account, 10_000_000);
    env.deposit(&lp, lp_account, 10_000_000);
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
    ctx_data[0] = 10; // valid full fill; leaves a valid req_id=1 response in ctx[0..64].
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

    env.try_trade_cpi_with_cu_on_asset(
        &taker,
        taker_account,
        &lp,
        lp_account,
        hostile,
        ctx,
        delegate,
        0,
        (2 * POS_SCALE) as i128,
        100,
    )
    .expect("first hostile matcher call writes a valid partial response");
    assert_eq!(env.market_state().0.matcher_req_seq, 1);
    assert_eq!(
        u64::from_le_bytes(
            env.svm.get_account(&ctx).unwrap().data[32..40]
                .try_into()
                .unwrap()
        ),
        1,
        "test setup leaves a stale valid matcher response in ctx[0..64]"
    );

    env.trade_asset_with_cu(
        0,
        &taker,
        taker_account,
        &lp,
        lp_account,
        -((2 * POS_SCALE) as i128),
        100,
        100,
    );
    env.push_auth_mark_for_asset_as_admin(0, env.svm.get_sysvar::<Clock>().slot, 100);
    let lp_capital = env.portfolio_state(lp_account).capital.get();
    assert!(lp_capital > 0, "LP should still have withdrawable capital");
    env.withdraw(&lp, lp_account, lp_capital);
    env.close_portfolio_with_cu(&lp, lp_account);

    env.svm
        .set_account(
            lp_account,
            Account {
                lamports: 1_000_000_000,
                data: vec![0u8; env.portfolio_account_len],
                owner: env.program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(lp.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(lp_account, false),
        ],
        &[&lp],
    )
    .expect("LP reinitializes the same portfolio account");
    assert_eq!(
        env.market_state().0.matcher_req_seq,
        1,
        "LP close/reinit must not reset the market-level matcher request sequence"
    );
    env.deposit(&lp, lp_account, 10_000_000);
    env.set_matcher_config(hostile, &lp, lp_account, ctx, delegate, 1);

    let mut stale_no_write_ctx = env.svm.get_account(&ctx).unwrap();
    stale_no_write_ctx.data[64] = 13;
    stale_no_write_ctx.data[65] = 1; // force no-write on the next hostile matcher call.
    env.svm.set_account(ctx, stale_no_write_ctx).unwrap();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker_account).unwrap();
    let lp_before = env.svm.get_account(&lp_account).unwrap();
    let ctx_before = env.svm.get_account(&ctx).unwrap();
    env.svm.expire_blockhash();
    let replay = env.try_trade_cpi_with_cu_on_asset(
        &taker,
        taker_account,
        &lp,
        lp_account,
        hostile,
        ctx,
        delegate,
        0,
        (2 * POS_SCALE) as i128,
        100,
    );
    assert!(
        replay.is_err(),
        "LP close/reinit must not reset matcher freshness enough to replay stale ctx[0..64]: {replay:?}"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker_account).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp_account).unwrap(), lp_before);
    assert_eq!(env.svm.get_account(&ctx).unwrap(), ctx_before);
}
