//! INV-019 - CPI invocation and return-data binding.
//!
//! Normative obligation: CPI results bind the exact invocation, matcher context, and economic request.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): public
//! CPI tests reject program-owned matcher tails before CPI, stale matcher
//! context replay across LP close/reinit, zero-fill batch atomicity, and hostile single/batch matcher
//! outputs that forge size, sign, asset, oracle, request id, LP id, price, or
//! partial-fill flags. These tests exercise the deployed public
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
                env.trade_cpi_ix(
                    taker_account,
                    lp_account,
                    0,
                    (5 * POS_SCALE) as i128,
                    100,
                    0,
                ),
                accounts(env, tail),
                &[&taker],
            ),
            Route::Batch => env.send(
                env.batch_trade_cpi_ix(
                    taker_account,
                    lp_account,
                    vec![
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
                ),
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

fn setup_hostile_matcher_cpi_env(
    asset_count: u16,
) -> (
    V16CuEnv,
    Keypair,
    Keypair,
    Pubkey,
    Pubkey,
    Pubkey,
    Pubkey,
    Pubkey,
) {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(asset_count, 1_000, 1_000, 500);
    for asset_index in 0..asset_count {
        env.configure_auth_mark_for_asset_as_admin(asset_index, 1, 100);
    }
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
    (
        env,
        taker,
        lp,
        taker_account,
        lp_account,
        hostile,
        ctx,
        delegate,
    )
}

#[test]
fn v16_program_hostile_matcher_batch_returns_all_rejected() {
    let (mut env, taker, _lp, taker_account, lp_account, hostile, ctx, delegate) =
        setup_hostile_matcher_cpi_env(2);
    let size_q = (5 * POS_SCALE) as i128;
    let send_mode = |env: &mut V16CuEnv,
                     mode: u8|
     -> (Result<u64, String>, Account, Account, Account, Account) {
        let mut data = vec![0u8; MATCHER_CONTEXT_LEN];
        data[0] = mode;
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
        let market_before = env.svm.get_account(&env.market).unwrap();
        let taker_before = env.svm.get_account(&taker_account).unwrap();
        let lp_before = env.svm.get_account(&lp_account).unwrap();
        let ctx_before = env.svm.get_account(&ctx).unwrap();
        env.svm.expire_blockhash();
        let result = env.send(
            env.batch_trade_cpi_ix(
                taker_account,
                lp_account,
                vec![
                    BatchTradeCpiLeg {
                        asset_index: 0,
                        market_id: first_generation_market_id(0),
                        size_q,
                        fee_bps: 100,
                        limit_price: 0,
                    },
                    BatchTradeCpiLeg {
                        asset_index: 1,
                        market_id: first_generation_market_id(1),
                        size_q,
                        fee_bps: 100,
                        limit_price: 0,
                    },
                ],
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
        (result, market_before, taker_before, lp_before, ctx_before)
    };

    for (mode, label) in [
        "over-fill",
        "reversed-sign",
        "forged-asset",
        "forged-oracle",
        "forged-req_id",
        "forged-lp",
        "zero-price",
        "unflagged-partial",
        "short-length",
    ]
    .iter()
    .enumerate()
    {
        let (result, market_before, taker_before, lp_before, ctx_before) =
            send_mode(&mut env, mode as u8);
        assert!(
            result.is_err(),
            "hostile batch matcher mode '{label}' must reject"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&taker_account).unwrap(), taker_before);
        assert_eq!(env.svm.get_account(&lp_account).unwrap(), lp_before);
        assert_eq!(env.svm.get_account(&ctx).unwrap(), ctx_before);
        let taker_state =
            state::read_portfolio(&env.svm.get_account(&taker_account).unwrap().data).unwrap();
        assert!(!has_active_leg_for_asset(&taker_state, 0));
        assert!(!has_active_leg_for_asset(&taker_state, 1));
    }

    let (ok, _, _, _, _) = send_mode(&mut env, 9);
    assert!(ok.is_ok(), "faithful matcher reply must execute: {ok:?}");
    let taker_state =
        state::read_portfolio(&env.svm.get_account(&taker_account).unwrap().data).unwrap();
    assert!(has_active_leg_for_asset(&taker_state, 0));
    assert!(has_active_leg_for_asset(&taker_state, 1));
}

#[test]
fn v16_program_batch_tradecpi_uses_only_current_configured_matcher_return_data() {
    for (mode, nested_after_matcher) in [(17u8, false), (18u8, true)] {
        let (mut env, taker, _lp, taker_account, lp_account, hostile, ctx, delegate) =
            setup_hostile_matcher_cpi_env(2);
        let fixture = std::fs::read(hostile_matcher_program_path()).unwrap();
        let nested_program = Pubkey::new_unique();
        env.svm.add_program(nested_program, &fixture);
        let nested_ctx = Pubkey::new_unique();
        let mut nested_ctx_data = vec![0u8; MATCHER_CONTEXT_LEN];
        nested_ctx_data[0] = 9;
        env.svm
            .set_account(
                nested_ctx,
                Account {
                    lamports: 1_000_000_000,
                    data: nested_ctx_data,
                    owner: nested_program,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let mut outer_ctx = vec![0u8; MATCHER_CONTEXT_LEN];
        outer_ctx[0] = mode;
        env.svm
            .set_account(
                ctx,
                Account {
                    lamports: 1_000_000_000,
                    data: outer_ctx,
                    owner: hostile,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();

        let size_q = (5 * POS_SCALE) as i128;
        let market_before = env.svm.get_account(&env.market).unwrap();
        let taker_before = env.svm.get_account(&taker_account).unwrap();
        let lp_before = env.svm.get_account(&lp_account).unwrap();
        let ctx_before = env.svm.get_account(&ctx).unwrap();
        let nested_ctx_before = env.svm.get_account(&nested_ctx).unwrap();
        env.svm.expire_blockhash();
        let result = env.send(
            env.batch_trade_cpi_ix(
                taker_account,
                lp_account,
                vec![
                    BatchTradeCpiLeg {
                        asset_index: 0,
                        market_id: first_generation_market_id(0),
                        size_q,
                        fee_bps: 100,
                        limit_price: 0,
                    },
                    BatchTradeCpiLeg {
                        asset_index: 1,
                        market_id: first_generation_market_id(1),
                        size_q: -size_q,
                        fee_bps: 100,
                        limit_price: 0,
                    },
                ],
            ),
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker_account, false),
                AccountMeta::new(lp_account, false),
                AccountMeta::new_readonly(hostile, false),
                AccountMeta::new(ctx, false),
                AccountMeta::new_readonly(delegate, false),
                AccountMeta::new_readonly(nested_program, false),
                AccountMeta::new(nested_ctx, false),
            ],
            &[&taker],
        );

        if nested_after_matcher {
            assert!(
                result.is_err(),
                "return data replaced after the configured matcher must reject"
            );
            assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
            assert_eq!(env.svm.get_account(&taker_account).unwrap(), taker_before);
            assert_eq!(env.svm.get_account(&lp_account).unwrap(), lp_before);
            assert_eq!(env.svm.get_account(&ctx).unwrap(), ctx_before);
            assert_eq!(env.svm.get_account(&nested_ctx).unwrap(), nested_ctx_before);
        } else {
            let cu = result.expect(
                "the configured matcher's later response must supersede unrelated nested return data",
            );
            assert_cu_within(
                "BatchTradeCpi nested return before configured matcher",
                cu,
                TRADE_CU_LIMIT,
            );
            let taker_after = env.portfolio_state(taker_account);
            assert_eq!(active_leg_for_asset(&taker_after, 0).basis_pos_q, size_q);
            assert_eq!(active_leg_for_asset(&taker_after, 1).basis_pos_q, -size_q);
        }
    }
}

#[test]
fn v16_program_hostile_matcher_single_tradecpi_returns_all_rejected() {
    let (mut env, taker, _lp, taker_account, lp_account, hostile, ctx, delegate) =
        setup_hostile_matcher_cpi_env(2);
    let size_q = (5 * POS_SCALE) as i128;
    let metas = |env: &V16CuEnv| {
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
    let send_mode = |env: &mut V16CuEnv,
                     mode: u8|
     -> (Result<u64, String>, Account, Account, Account, Account) {
        let mut data = vec![0u8; MATCHER_CONTEXT_LEN];
        data[0] = mode;
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
        let market_before = env.svm.get_account(&env.market).unwrap();
        let taker_before = env.svm.get_account(&taker_account).unwrap();
        let lp_before = env.svm.get_account(&lp_account).unwrap();
        let ctx_before = env.svm.get_account(&ctx).unwrap();
        env.svm.expire_blockhash();
        let accounts = metas(env);
        let result = env.send(
            env.trade_cpi_ix(taker_account, lp_account, 0, size_q, 100, 0),
            accounts,
            &[&taker],
        );
        (result, market_before, taker_before, lp_before, ctx_before)
    };

    for (mode, label) in [
        "over-fill",
        "reversed-sign",
        "forged-asset",
        "forged-oracle",
        "forged-req_id",
        "forged-lp",
        "zero-price",
        "unflagged-partial",
    ]
    .iter()
    .enumerate()
    {
        let (result, market_before, taker_before, lp_before, ctx_before) =
            send_mode(&mut env, mode as u8);
        assert!(
            result.is_err(),
            "hostile single matcher mode '{label}' must reject"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&taker_account).unwrap(), taker_before);
        assert_eq!(env.svm.get_account(&lp_account).unwrap(), lp_before);
        assert_eq!(env.svm.get_account(&ctx).unwrap(), ctx_before);
        let taker_state =
            state::read_portfolio(&env.svm.get_account(&taker_account).unwrap().data).unwrap();
        assert!(!has_active_leg_for_asset(&taker_state, 0));
    }

    let (ok, _, _, _, _) = send_mode(&mut env, 9);
    assert!(ok.is_ok(), "faithful matcher reply must execute: {ok:?}");
    let taker_state =
        state::read_portfolio(&env.svm.get_account(&taker_account).unwrap().data).unwrap();
    assert!(has_active_leg_for_asset(&taker_state, 0));
}

#[test]
fn v16_program_hostile_matcher_no_write_cannot_replay_stale_batch_return_data() {
    let (mut env, taker, _lp, taker_account, lp_account, hostile, ctx, delegate) =
        setup_hostile_matcher_cpi_env(2);
    let mut ctx_data = vec![0u8; MATCHER_CONTEXT_LEN];
    ctx_data[64] = 13;
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

    let size_q = POS_SCALE as i128;
    let program_id = env.program_id;
    let market = env.market;
    let taker_key = taker.pubkey();
    let taker_portfolio_id = env.portfolio_id(taker_account);
    let lp_portfolio_id = env.portfolio_id(lp_account);
    let batch_ix = || Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(taker_key, true),
            AccountMeta::new(market, false),
            AccountMeta::new(taker_account, false),
            AccountMeta::new(lp_account, false),
            AccountMeta::new_readonly(hostile, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ],
        data: ProgInstruction::BatchTradeCpi {
            account_a_portfolio_id: taker_portfolio_id,
            account_a_position_epoch: 0,
            account_b_portfolio_id: lp_portfolio_id,
            account_b_position_epoch: 0,
            legs: vec![
                BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: first_generation_market_id(0),
                    size_q,
                    fee_bps: 100,
                    limit_price: 0,
                },
                BatchTradeCpiLeg {
                    asset_index: 1,
                    market_id: first_generation_market_id(1),
                    size_q: -size_q,
                    fee_bps: 100,
                    limit_price: 0,
                },
            ],
        }
        .encode(),
    };
    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker_account).unwrap();
    let lp_before = env.svm.get_account(&lp_account).unwrap();
    let ctx_before = env.svm.get_account(&ctx).unwrap();

    env.svm.expire_blockhash();
    let replay = send_raw_ixs(
        &mut env.svm,
        &env.payer,
        vec![heap_ix(), cu_ix(), batch_ix(), batch_ix()],
        &[&taker],
    );
    assert!(
        replay.is_err(),
        "a matcher that omits second-call return data must not replay stale batch return data"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker_account).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp_account).unwrap(), lp_before);
    assert_eq!(env.svm.get_account(&ctx).unwrap(), ctx_before);
}

#[test]
fn v16_program_hostile_matcher_no_write_cannot_replay_stale_single_context() {
    let (mut env, taker, _lp, taker_account, lp_account, hostile, ctx, delegate) =
        setup_hostile_matcher_cpi_env(1);
    let mut ctx_data = vec![0u8; MATCHER_CONTEXT_LEN];
    ctx_data[0] = 9;
    ctx_data[64] = 13;
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

    let size_q = POS_SCALE as i128;
    let program_id = env.program_id;
    let market = env.market;
    let taker_key = taker.pubkey();
    let taker_portfolio_id = env.portfolio_id(taker_account);
    let lp_portfolio_id = env.portfolio_id(lp_account);
    let single_ix = || Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(taker_key, true),
            AccountMeta::new(market, false),
            AccountMeta::new(taker_account, false),
            AccountMeta::new(lp_account, false),
            AccountMeta::new_readonly(hostile, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ],
        data: ProgInstruction::TradeCpi {
            account_a_portfolio_id: taker_portfolio_id,
            account_a_position_epoch: 0,
            account_b_portfolio_id: lp_portfolio_id,
            account_b_position_epoch: 0,
            asset_index: 0,
            market_id: first_generation_market_id(0),
            size_q,
            fee_bps: 100,
            limit_price: 0,
        }
        .encode(),
    };
    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker_account).unwrap();
    let lp_before = env.svm.get_account(&lp_account).unwrap();
    let ctx_before = env.svm.get_account(&ctx).unwrap();

    env.svm.expire_blockhash();
    let replay = send_raw_ixs(
        &mut env.svm,
        &env.payer,
        vec![heap_ix(), cu_ix(), single_ix(), single_ix()],
        &[&taker],
    );
    assert!(
        replay.is_err(),
        "a matcher that omits second-call context output must not replay stale TradeCpi bytes"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker_account).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp_account).unwrap(), lp_before);
    assert_eq!(env.svm.get_account(&ctx).unwrap(), ctx_before);
}

#[test]
fn v16_program_tradecpi_matcher_req_id_advances_monotonically_on_market() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, 10_000_000);
    env.deposit(&lp_owner, lp, 10_000_000);
    let (ctx, delegate, _) = env.init_auth_matcher_context(matcher_program, &lp_owner, lp);
    let read_ctx_req_id = |env: &V16CuEnv| {
        let ctx_data = env.svm.get_account(&ctx).unwrap().data;
        u64::from_le_bytes(ctx_data[32..40].try_into().unwrap())
    };

    env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &lp_owner,
        lp,
        matcher_program,
        ctx,
        delegate,
        0,
        POS_SCALE as i128,
        100,
    )
    .expect("first matcher fill succeeds");
    assert_eq!(read_ctx_req_id(&env), 1);
    assert_eq!(env.market_state().0.matcher_req_seq, 1);

    env.svm.expire_blockhash();
    env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &lp_owner,
        lp,
        matcher_program,
        ctx,
        delegate,
        0,
        POS_SCALE as i128,
        100,
    )
    .expect("second matcher fill succeeds");
    assert_eq!(read_ctx_req_id(&env), 2);
    assert_eq!(env.market_state().0.matcher_req_seq, 2);
}

// phantom no-op success or partially advance matcher/protocol state.
#[test]
fn v16_program_batch_tradecpi_zero_fill_rejects_atomically() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);

    let taker = Keypair::new();
    let lp = Keypair::new();
    let taker_portfolio = env.create_portfolio(&taker);
    let lp_portfolio = env.create_portfolio(&lp);
    env.deposit(&taker, taker_portfolio, 1_000_000);
    env.deposit(&lp, lp_portfolio, 1_000_000);

    let (ctx, delegate, _) = env.init_matcher_context_with_data_authorized(
        matcher_program,
        &lp,
        lp_portfolio,
        encode_matcher_init_passive(0),
    );
    let sz = (5 * POS_SCALE) as i128;
    let legs = vec![
        BatchTradeCpiLeg {
            asset_index: 0,
            market_id: first_generation_market_id((0) as u16),
            size_q: sz,
            fee_bps: 100,
            limit_price: 0,
        },
        BatchTradeCpiLeg {
            asset_index: 1,
            market_id: first_generation_market_id((1) as u16),
            size_q: sz,
            fee_bps: 100,
            limit_price: 0,
        },
    ];
    let accounts = |env: &V16CuEnv, ctx: Pubkey, delegate: Pubkey| {
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker_portfolio, false),
            AccountMeta::new(lp_portfolio, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ]
    };

    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker_portfolio).unwrap();
    let lp_before = env.svm.get_account(&lp_portfolio).unwrap();
    let ctx_before = env.svm.get_account(&ctx).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        env.batch_trade_cpi_ix(taker_portfolio, lp_portfolio, legs.clone()),
        accounts(&env, ctx, delegate),
        &[&taker],
    );
    assert!(
        rejected.is_err(),
        "BatchTradeCpi must reject when the matcher returns exec_size=0 for a leg"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "zero-fill batch rejection leaves market bytes unchanged"
    );
    assert_eq!(
        env.svm.get_account(&taker_portfolio).unwrap(),
        taker_before,
        "zero-fill batch rejection leaves taker portfolio unchanged"
    );
    assert_eq!(
        env.svm.get_account(&lp_portfolio).unwrap(),
        lp_before,
        "zero-fill batch rejection leaves LP portfolio unchanged"
    );
    assert_eq!(
        env.svm.get_account(&ctx).unwrap(),
        ctx_before,
        "zero-fill batch rejection rolls back matcher context writes"
    );
    let group_after_reject = env.market_state().1;
    assert_eq!(group_after_reject.assets[0].oi_eff_long_q, 0);
    assert_eq!(group_after_reject.assets[1].oi_eff_long_q, 0);
    assert_eq!(
        group_after_reject.insurance, 0,
        "no fee credited on zero-fill batch"
    );

    let (ok_ctx, ok_delegate, _) =
        env.init_matcher_context_authorized(matcher_program, &lp, lp_portfolio);
    env.svm.expire_blockhash();
    let ok = env.send(
        env.batch_trade_cpi_ix(taker_portfolio, lp_portfolio, legs),
        accounts(&env, ok_ctx, ok_delegate),
        &[&taker],
    );
    assert!(
        ok.is_ok(),
        "same BatchTradeCpi fills once the matcher can fully satisfy every leg: {ok:?}"
    );
    let taker_after = env.portfolio_state(taker_portfolio);
    assert!(
        has_active_leg_for_asset(&taker_after, 0) && has_active_leg_for_asset(&taker_after, 1),
        "successful control fills both batch legs"
    );
}

// security.md sweep — TradeCpi matcher identity binding (#44/#49): the matcher_delegate is a PDA
// bound to (slab, maker portfolio, maker owner, matcher_program, matcher_context). Routing a
// TradeCpi through a SPOOFED delegate or a wrong/non-program matcher must reject — no trade
// executes, no value moves.
#[test]
fn v16_attack_tradecpi_spoofed_matcher_binding_rejected() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let maker_owner = Keypair::new();
    let maker = env.create_portfolio(&maker_owner);
    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&maker_owner, maker, 1_000_000);
    let (matcher_ctx, matcher_delegate, _) =
        env.init_matcher_context_authorized(matcher_program, &maker_owner, maker);
    let (_, g0) = env.market_state();

    // ATTACK 1: random (unbound) delegate.
    env.svm.expire_blockhash();
    let r1 = env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &maker_owner,
        maker,
        matcher_program,
        matcher_ctx,
        Pubkey::new_unique(),
        0,
        (10 * POS_SCALE) as i128,
        100,
    );
    assert!(
        r1.is_err(),
        "spoofed (unbound) matcher delegate must reject"
    );

    // ATTACK 2: a delegate bound to a DIFFERENT context.
    let other_program = Pubkey::new_unique();
    env.svm.add_program(other_program, &matcher_bytes);
    let (_other_ctx, other_delegate, _) = env.init_matcher_context(other_program, maker);
    env.svm.expire_blockhash();
    let r2 = env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &maker_owner,
        maker,
        matcher_program,
        matcher_ctx,
        other_delegate,
        0,
        (10 * POS_SCALE) as i128,
        100,
    );
    assert!(
        r2.is_err(),
        "delegate bound to a different matcher/context must reject"
    );

    // ATTACK 3: a non-program account as the matcher program (CPI target bogus).
    env.svm.expire_blockhash();
    let bogus_prog = Pubkey::new_unique();
    let r3 = env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &maker_owner,
        maker,
        bogus_prog,
        matcher_ctx,
        matcher_delegate,
        0,
        (10 * POS_SCALE) as i128,
        100,
    );
    assert!(r3.is_err(), "wrong/non-program matcher must reject");

    // no trade executed, no value moved across all spoof attempts.
    let (_, g1) = env.market_state();
    assert_eq!(
        g1.assets[0].oi_eff_long_q, 0,
        "no OI created by spoofed-matcher TradeCpi"
    );
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert_eq!(g1.c_tot, g0.c_tot, "c_tot unchanged");
    assert_eq!(
        env.portfolio_state(taker).legs[0].basis_pos_q.get(),
        0,
        "taker has no position"
    );
    // the legitimate binding still works (control).
    env.svm.expire_blockhash();
    let ok = env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &maker_owner,
        maker,
        matcher_program,
        matcher_ctx,
        matcher_delegate,
        0,
        (10 * POS_SCALE) as i128,
        100,
    );
    assert!(ok.is_ok(), "correctly-bound matcher executes: {:?}", ok);
}

// security.md sweep - matcher self-CPI / protocol-context isolation (#22/#26/#44): the matcher CPI
// boundary must be an external program/context. An LP must not be able to authorize the Percolator
// program itself with the market slab as matcher_ctx; if a stale/corrupt config already contains that
// tuple, TradeCpi and BatchTradeCpi must still reject before invoking or mutating protocol state.
#[test]
fn v16_attack_matcher_config_and_fills_reject_self_program_context() {
    let mut env = V16CuEnv::new();
    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&lp_owner, lp, 1_000_000);

    let self_delegate = matcher_delegate_key(
        &env.program_id,
        &env.market,
        &lp,
        &lp_owner.pubkey(),
        &env.program_id,
        &env.market,
    );
    env.svm
        .set_account(
            self_delegate,
            Account {
                lamports: 1_000_000_000,
                data: vec![],
                owner: Pubkey::default(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let market_before_config = env.svm.get_account(&env.market).unwrap();
    let lp_before_config = env.svm.get_account(&lp).unwrap();
    let portfolio_id = env.portfolio_id(lp);
    let expected_sequence = env.portfolio_matcher_sequence(lp);
    env.svm.expire_blockhash();
    let self_config = env.send(
        ProgInstruction::SetMatcherConfig {
            portfolio_id,
            expected_sequence,
            enabled: 1,
            trade_fee_cap_bps: 10_000,
        },
        vec![
            AccountMeta::new(lp_owner.pubkey(), true),
            AccountMeta::new_readonly(env.market, false),
            AccountMeta::new(lp, false),
            AccountMeta::new_readonly(env.program_id, false),
            AccountMeta::new_readonly(env.market, false),
            AccountMeta::new_readonly(self_delegate, false),
        ],
        &[&lp_owner],
    );
    assert!(
        self_config.is_err(),
        "SetMatcherConfig must reject Percolator itself as matcher_program/context"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_config,
        "rejected self-matcher config leaves the market slab unchanged"
    );
    assert_eq!(
        env.svm.get_account(&lp).unwrap(),
        lp_before_config,
        "rejected self-matcher config does not arm the LP account"
    );

    let mut corrupt_lp = env.svm.get_account(&lp).unwrap();
    state::write_portfolio_matcher_config(
        &mut corrupt_lp.data,
        &state::PortfolioMatcherConfigV16 {
            matcher_program: env.program_id.to_bytes(),
            matcher_context: env.market.to_bytes(),
            matcher_delegate: self_delegate.to_bytes(),
            control: 1,
        },
    )
    .expect("inject stale self-matcher config for fill-time guard");
    env.svm.set_account(lp, corrupt_lp).unwrap();

    let market_before_fill = env.svm.get_account(&env.market).unwrap();
    let taker_before_fill = env.svm.get_account(&taker).unwrap();
    let lp_before_fill = env.svm.get_account(&lp).unwrap();
    let send_self_single = |env: &mut V16CuEnv| {
        env.svm.expire_blockhash();
        env.send(
            env.trade_cpi_ix(taker, lp, 0, (5 * POS_SCALE) as i128, 100, 0),
            vec![
                AccountMeta::new(taker_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker, false),
                AccountMeta::new(lp, false),
                AccountMeta::new_readonly(env.program_id, false),
                AccountMeta::new(env.market, false),
                AccountMeta::new_readonly(self_delegate, false),
            ],
            &[&taker_owner],
        )
    };
    let self_single = send_self_single(&mut env);
    assert!(
        self_single.is_err(),
        "TradeCpi must reject a self-program matcher tuple before CPI"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_fill
    );
    assert_eq!(env.svm.get_account(&taker).unwrap(), taker_before_fill);
    assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before_fill);

    env.svm.expire_blockhash();
    let self_batch = env.send(
        env.batch_trade_cpi_ix(
            taker,
            lp,
            vec![BatchTradeCpiLeg {
                asset_index: 0,
                market_id: first_generation_market_id((0) as u16),
                size_q: (5 * POS_SCALE) as i128,
                fee_bps: 100,
                limit_price: 0,
            }],
        ),
        vec![
            AccountMeta::new(taker_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker, false),
            AccountMeta::new(lp, false),
            AccountMeta::new_readonly(env.program_id, false),
            AccountMeta::new(env.market, false),
            AccountMeta::new_readonly(self_delegate, false),
        ],
        &[&taker_owner],
    );
    assert!(
        self_batch.is_err(),
        "BatchTradeCpi must reject a self-program matcher tuple before CPI"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_fill
    );
    assert_eq!(env.svm.get_account(&taker).unwrap(), taker_before_fill);
    assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before_fill);

    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let (ctx, delegate, _) = env.init_auth_matcher_context(matcher_program, &lp_owner, lp);
    env.svm.expire_blockhash();
    let ok = env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &lp_owner,
        lp,
        matcher_program,
        ctx,
        delegate,
        0,
        (5 * POS_SCALE) as i128,
        100,
    );
    assert!(
        ok.is_ok(),
        "external matcher tuple still fills after self-program attempts: {ok:?}"
    );
}

// security.md sweep — matcher CPI cannot be handed protocol state (reentrancy, #22/#21): the accounts
// forwarded to the matcher CPI (tail = accounts[8..]) are validated to EXCLUDE the market, both
// portfolios, the program id, and ANY account owned by the percolator program (src/v16_program.rs:6308).
// Attacker goal: pass a percolator-owned account (market/portfolio) into the matcher tail so a malicious
// matcher can reenter and read/write protocol state mid-trade. Protection: any such tail account rejects.
#[test]
fn v16_attack_tradecpi_matcher_tail_cannot_carry_protocol_state() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let to = Keypair::new();
    let t = env.create_portfolio(&to);
    let mo = Keypair::new();
    let m = env.create_portfolio(&mo);
    let vo = Keypair::new();
    let victim = env.create_portfolio(&vo); // a third, unrelated portfolio
    env.deposit(&to, t, 1_000_000);
    env.deposit(&mo, m, 1_000_000);
    env.deposit(&vo, victim, 1_000_000);
    let (ctx, del, _) = env.init_matcher_context_authorized(matcher_program, &mo, m);
    let (_, g0) = env.market_state();

    let market = env.market;
    let base = |extra: Pubkey| {
        vec![
            AccountMeta::new(to.pubkey(), true),
            AccountMeta::new(market, false),
            AccountMeta::new(t, false),
            AccountMeta::new(m, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(del, false),
            AccountMeta::new(extra, false), // tail account[7] -> handed to the matcher CPI
        ]
    };
    let t_portfolio_id = env.portfolio_id(t);
    let m_portfolio_id = env.portfolio_id(m);
    let ix = |asset_index, size_q| ProgInstruction::TradeCpi {
        account_a_portfolio_id: t_portfolio_id,
        account_a_position_epoch: 0,
        account_b_portfolio_id: m_portfolio_id,
        account_b_position_epoch: 0,
        asset_index,
        market_id: first_generation_market_id((asset_index) as u16),
        size_q,
        fee_bps: 100,
        limit_price: 0,
    };

    // ATTACK 1: forward the MARKET account into the matcher tail -> reject.
    env.svm.expire_blockhash();
    let r1 = env.send(ix(0u16, (10 * POS_SCALE) as i128), base(market), &[&to]);
    assert!(
        r1.is_err(),
        "matcher tail carrying the market account must reject"
    );
    // ATTACK 2: forward a third (percolator-owned) portfolio into the tail -> reject (ai.owner == program).
    env.svm.expire_blockhash();
    let r2 = env.send(ix(0u16, (10 * POS_SCALE) as i128), base(victim), &[&to]);
    assert!(
        r2.is_err(),
        "matcher tail carrying a protocol-owned portfolio must reject"
    );

    // no state touched by either rejected attempt.
    let (_, g1) = env.market_state();
    assert_eq!(
        g1.assets[0].oi_eff_long_q, 0,
        "no OI from rejected reentrancy attempts"
    );
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert_eq!(
        env.portfolio_state(victim).capital.get(),
        1_000_000,
        "victim portfolio untouched"
    );
    // control: the SAME trade WITHOUT a poisoned tail fills cleanly.
    env.svm.expire_blockhash();
    let ok = env.try_trade_cpi_with_cu_on_asset(
        &to,
        t,
        &mo,
        m,
        matcher_program,
        ctx,
        del,
        0,
        (10 * POS_SCALE) as i128,
        100,
    );
    assert!(
        ok.is_ok(),
        "clean TradeCpi (no poisoned tail) executes: {:?}",
        ok
    );
}

// security.md sweep - matcher-tail signer isolation (#22/#44/#49): the tail forwarded to an
// external matcher must not include the taker wallet signer. If it does, a hostile matcher can use
// that signer privilege in a nested System Program transfer before returning an otherwise valid fill.
#[test]
fn v16_attack_tradecpi_matcher_tail_cannot_forward_taker_signer() {
    let mut env = V16CuEnv::new();
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
    let mut ctx_data = vec![0u8; MATCHER_CONTEXT_LEN];
    ctx_data[0] = 14; // hostile mode: transfer lamports from tail[0] to tail[1], then return valid fill.
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

    let recipient = Pubkey::new_unique();
    env.svm
        .set_account(
            recipient,
            Account {
                lamports: 1,
                data: vec![],
                owner: solana_sdk::system_program::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker_account).unwrap();
    let lp_before = env.svm.get_account(&lp_account).unwrap();
    let ctx_before = env.svm.get_account(&ctx).unwrap();
    let taker_wallet_before = env.svm.get_account(&taker.pubkey()).unwrap();
    let recipient_before = env.svm.get_account(&recipient).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        env.trade_cpi_ix(
            taker_account,
            lp_account,
            0,
            (5 * POS_SCALE) as i128,
            100,
            0,
        ),
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker_account, false),
            AccountMeta::new(lp_account, false),
            AccountMeta::new_readonly(hostile, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(recipient, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        &[&taker],
    );
    let rejected_err = rejected.expect_err(
        "TradeCpi must not forward the taker wallet signer into a hostile matcher tail",
    );
    assert!(
        rejected_err.contains("Custom(9)") || rejected_err.contains("custom program error: 0x9"),
        "taker-signer matcher tail must reject in wrapper preflight as InvalidInstruction, got {rejected_err}"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker_account).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp_account).unwrap(), lp_before);
    assert_eq!(env.svm.get_account(&ctx).unwrap(), ctx_before);
    assert_eq!(
        env.svm.get_account(&taker.pubkey()).unwrap(),
        taker_wallet_before,
        "rejected matcher-tail signer forwarding must not debit the taker wallet"
    );
    assert_eq!(
        env.svm.get_account(&recipient).unwrap(),
        recipient_before,
        "hostile matcher must not receive lamports from the taker wallet"
    );

    let mut honest_ctx = env.svm.get_account(&ctx).unwrap();
    honest_ctx.data[0] = 9;
    env.svm.set_account(ctx, honest_ctx).unwrap();
    env.svm.expire_blockhash();
    let ok = env.try_trade_cpi_with_cu_on_asset(
        &taker,
        taker_account,
        &lp,
        lp_account,
        hostile,
        ctx,
        delegate,
        0,
        (5 * POS_SCALE) as i128,
        100,
    );
    assert!(
        ok.is_ok(),
        "the same authorized hostile fixture fills when no wallet signer is forwarded: {ok:?}"
    );
}

// security.md sweep - BatchTradeCpi matcher-tail isolation (#9/#27): the batched CPI fill path also
// forwards optional remaining accounts to an external matcher. It must reject the market account and
// any Percolator-owned portfolio in that tail, then still accept a clean permissionless LP fill.
#[test]
fn v16_attack_batch_tradecpi_matcher_tail_cannot_carry_protocol_state() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker = Keypair::new();
    let lp = Keypair::new();
    let victim_owner = Keypair::new();
    let taker_account = env.create_portfolio(&taker);
    let lp_account = env.create_portfolio(&lp);
    let victim = env.create_portfolio(&victim_owner);
    env.deposit(&taker, taker_account, 1_000_000);
    env.deposit(&lp, lp_account, 1_000_000);
    env.deposit(&victim_owner, victim, 1_000_000);
    let (ctx, delegate, _) = env.init_auth_matcher_context(matcher_program, &lp, lp_account);
    let before = env.market_state().1;
    let victim_before = env.portfolio_state(victim);
    let sz = (5 * POS_SCALE) as i128;
    let ix = env.batch_trade_cpi_ix(
        taker_account,
        lp_account,
        vec![
            BatchTradeCpiLeg {
                asset_index: 0,
                market_id: first_generation_market_id((0) as u16),
                size_q: sz,
                fee_bps: 100,
                limit_price: 0,
            },
            BatchTradeCpiLeg {
                asset_index: 1,
                market_id: first_generation_market_id((1) as u16),
                size_q: -sz,
                fee_bps: 100,
                limit_price: 0,
            },
        ],
    );
    let market = env.market;
    let taker_key = taker.pubkey();
    let base = |extra: Option<Pubkey>| {
        let mut metas = vec![
            AccountMeta::new(taker_key, true),
            AccountMeta::new(market, false),
            AccountMeta::new(taker_account, false),
            AccountMeta::new(lp_account, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ];
        if let Some(extra) = extra {
            metas.push(AccountMeta::new(extra, false));
        }
        metas
    };

    env.svm.expire_blockhash();
    let market_tail = env.send(ix.clone(), base(Some(market)), &[&taker]);
    assert!(
        market_tail.is_err(),
        "BatchTradeCpi matcher tail carrying the market account must reject"
    );
    env.svm.expire_blockhash();
    let portfolio_tail = env.send(ix.clone(), base(Some(victim)), &[&taker]);
    assert!(
        portfolio_tail.is_err(),
        "BatchTradeCpi matcher tail carrying any Percolator-owned portfolio must reject"
    );
    let after_reject = env.market_state().1;
    assert_eq!(
        after_reject.assets[0].oi_eff_long_q, 0,
        "no asset-0 OI from rejected tail poison"
    );
    assert_eq!(
        after_reject.assets[1].oi_eff_long_q, 0,
        "no asset-1 OI from rejected tail poison"
    );
    assert_eq!(
        after_reject.vault, before.vault,
        "vault unchanged by rejected tail poison"
    );
    assert_eq!(
        env.portfolio_state(victim).capital.get(),
        victim_before.capital.get(),
        "victim portfolio untouched"
    );

    env.svm.expire_blockhash();
    let ok = env.send(ix, base(None), &[&taker]);
    assert!(
        ok.is_ok(),
        "clean BatchTradeCpi without poisoned tail executes: {ok:?}"
    );
    let taker_state = env.portfolio_state(taker_account);
    assert!(
        has_active_leg_for_asset(&taker_state, 0),
        "clean batch fills asset 0"
    );
    assert!(
        has_active_leg_for_asset(&taker_state, 1),
        "clean batch fills asset 1"
    );
}

// security.md sweep - batch matcher-tail signer isolation (#22/#44/#49): the batched CPI route must
// apply the same signer-forwarding boundary as single TradeCpi. A hostile matcher must not receive
// the taker wallet signer through the tail and use it in a nested System Program transfer.
#[test]
fn v16_attack_batch_tradecpi_matcher_tail_cannot_forward_taker_signer() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
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
    let mut ctx_data = vec![0u8; MATCHER_CONTEXT_LEN];
    ctx_data[0] = 14;
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

    let recipient = Pubkey::new_unique();
    env.svm
        .set_account(
            recipient,
            Account {
                lamports: 1,
                data: vec![],
                owner: solana_sdk::system_program::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker_account).unwrap();
    let lp_before = env.svm.get_account(&lp_account).unwrap();
    let ctx_before = env.svm.get_account(&ctx).unwrap();
    let taker_wallet_before = env.svm.get_account(&taker.pubkey()).unwrap();
    let recipient_before = env.svm.get_account(&recipient).unwrap();
    let legs = vec![
        BatchTradeCpiLeg {
            asset_index: 0,
            market_id: first_generation_market_id((0) as u16),
            size_q: (5 * POS_SCALE) as i128,
            fee_bps: 100,
            limit_price: 0,
        },
        BatchTradeCpiLeg {
            asset_index: 1,
            market_id: first_generation_market_id((1) as u16),
            size_q: -(5 * POS_SCALE as i128),
            fee_bps: 100,
            limit_price: 0,
        },
    ];

    env.svm.expire_blockhash();
    let rejected = env.send(
        env.batch_trade_cpi_ix(taker_account, lp_account, legs.clone()),
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker_account, false),
            AccountMeta::new(lp_account, false),
            AccountMeta::new_readonly(hostile, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(recipient, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        &[&taker],
    );
    let rejected_err = rejected.expect_err(
        "BatchTradeCpi must not forward the taker wallet signer into a hostile matcher tail",
    );
    assert!(
        rejected_err.contains("Custom(9)") || rejected_err.contains("custom program error: 0x9"),
        "batch taker-signer matcher tail must reject in wrapper preflight as InvalidInstruction, got {rejected_err}"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker_account).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp_account).unwrap(), lp_before);
    assert_eq!(env.svm.get_account(&ctx).unwrap(), ctx_before);
    assert_eq!(
        env.svm.get_account(&taker.pubkey()).unwrap(),
        taker_wallet_before,
        "rejected batch signer forwarding must not debit the taker wallet"
    );
    assert_eq!(
        env.svm.get_account(&recipient).unwrap(),
        recipient_before,
        "hostile batch matcher must not receive lamports from the taker wallet"
    );

    let mut honest_ctx = env.svm.get_account(&ctx).unwrap();
    honest_ctx.data[0] = 9;
    env.svm.set_account(ctx, honest_ctx).unwrap();
    env.svm.expire_blockhash();
    let ok = env.send(
        env.batch_trade_cpi_ix(taker_account, lp_account, legs),
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
    assert!(
        ok.is_ok(),
        "the same authorized hostile fixture batch-fills when no wallet signer is forwarded: {ok:?}"
    );
}

// CU/DoS hardening: backing-fee policy intentionally disables batch fee splitting, so BatchTradeCpi
// must reject before the external matcher CPI when any backing-fee policy is active. The no-policy
// security.md sweep - stale BatchTradeNoCpi legacy realloc rollback (#30/#44/#48): the no-CPI batch
// path grows legacy portfolios before the shared stale-market freeze check. A stale-matured batch must
// reject without leaving an attacker-triggered realloc/zero-fill DoS behind; while fresh, the same
// Anti-spoof core: validate_matcher_return is the per-leg gate that bounds a (possibly hostile)
// matcher's reply. Every CPI integration test feeds it HONEST returns; this exercises the REJECTION
// branches directly with crafted hostile replies (over-fill, reversed sign, forged echo, bad flags,
// zero price, unflagged partial). Guards the anti-spoof against regression end-to-end of the struct.
#[test]
fn v16_attack_matcher_return_antispoof_rejections() {
    use percolator_prog::matcher_abi::{
        validate_matcher_return, MatcherReturn, FLAG_BACKING_FEE_CAP_SHIFT, FLAG_PARTIAL_OK,
        FLAG_REJECTED, FLAG_VALID,
    };
    const LP: u64 = 7;
    const ASSET: u16 = 1;
    const ORACLE: u64 = 100;
    const REQID: u64 = 42;
    let req: i128 = (10 * POS_SCALE) as i128; // taker buy, full size 10
    let valid = MatcherReturn {
        abi_version: MATCHER_ABI_VERSION,
        flags: FLAG_VALID,
        exec_price_e6: 100,
        exec_size: req,
        req_id: REQID,
        lp_account_id: LP,
        oracle_price_e6: ORACLE,
        asset_index: ASSET as u64,
    };
    let chk = |r: &MatcherReturn| validate_matcher_return(r, LP, ASSET, ORACLE, req, REQID);
    // baseline: a faithful full fill is accepted.
    assert!(chk(&valid).is_ok(), "faithful full fill must validate");
    // a faithful partial fill (flagged) is accepted.
    let mut partial = valid;
    partial.exec_size = (5 * POS_SCALE) as i128;
    partial.flags = FLAG_VALID | FLAG_PARTIAL_OK;
    assert!(chk(&partial).is_ok(), "flagged partial fill validates");
    let mut capped = valid;
    capped.flags = FLAG_VALID | (5_000u32 << FLAG_BACKING_FEE_CAP_SHIFT);
    assert!(chk(&capped).is_ok(), "an in-range LP fee cap validates");
    assert_eq!(capped.backing_fee_cap_bps(), 5_000);

    // --- hostile replies, each must REJECT ---
    let cases: &[(&str, fn(MatcherReturn) -> MatcherReturn)] = &[
        ("over-fill (exec>req)", |mut r| {
            r.exec_size = (20 * POS_SCALE) as i128;
            r
        }),
        ("reversed sign", |mut r| {
            r.exec_size = -((10 * POS_SCALE) as i128);
            r
        }),
        ("forged asset echo", |mut r| {
            r.asset_index = 2;
            r
        }),
        ("forged oracle echo", |mut r| {
            r.oracle_price_e6 = 99;
            r
        }),
        ("forged req_id", |mut r| {
            r.req_id = 43;
            r
        }),
        ("forged lp_account_id", |mut r| {
            r.lp_account_id = 8;
            r
        }),
        ("bad abi_version", |mut r| {
            r.abi_version = MATCHER_ABI_VERSION + 1;
            r
        }),
        ("REJECTED flag set", |mut r| {
            r.flags = FLAG_VALID | FLAG_REJECTED;
            r
        }),
        ("VALID flag missing", |mut r| {
            r.flags = 0;
            r
        }),
        ("unknown flag bit", |mut r| {
            r.flags = FLAG_VALID | (1 << 31);
            r
        }),
        ("backing fee cap above 10000 bps", |mut r| {
            r.flags = FLAG_VALID | (10_001 << FLAG_BACKING_FEE_CAP_SHIFT);
            r
        }),
        ("zero exec_price", |mut r| {
            r.exec_price_e6 = 0;
            r
        }),
        ("unflagged partial (exec<req, no PARTIAL_OK)", |mut r| {
            r.exec_size = (5 * POS_SCALE) as i128;
            r
        }),
    ];
    for (name, mutate) in cases {
        let bad = mutate(valid);
        assert!(
            chk(&bad).is_err(),
            "hostile matcher return must be rejected: {name}"
        );
    }
}

// CU/DoS hardening: duplicate-asset BatchTradeCpi is structurally invalid and must reject before
// matcher CPI. The existing duplicate test proves rollback; this hostile sentinel proves the wrapper
// does not call the matcher first.
#[test]
fn v16_attack_batch_tradecpi_duplicate_assets_reject_before_hostile_matcher_cpi() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
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

    let send = |env: &mut V16CuEnv, legs: Vec<BatchTradeCpiLeg>| {
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
            env.batch_trade_cpi_ix(ta, la, legs),
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
    let sz = (5 * POS_SCALE) as i128;

    let distinct_err = send(
        &mut env,
        vec![
            BatchTradeCpiLeg {
                asset_index: 0,
                market_id: first_generation_market_id((0) as u16),
                size_q: sz,
                fee_bps: 100,
                limit_price: 0,
            },
            BatchTradeCpiLeg {
                asset_index: 1,
                market_id: first_generation_market_id((1) as u16),
                size_q: -sz,
                fee_bps: 100,
                limit_price: 0,
            },
        ],
    )
    .expect_err("distinct hostile batch should reach matcher-return validation");
    assert!(
        distinct_err.contains("InvalidAccountData"),
        "distinct hostile control should fail from matcher-return validation, got {distinct_err}"
    );
    assert!(
        !distinct_err.contains("Custom(9)"),
        "distinct hostile control must not trip the duplicate gate: {distinct_err}"
    );

    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&ta).unwrap();
    let lp_before = env.svm.get_account(&la).unwrap();
    let duplicate_err = send(
        &mut env,
        vec![
            BatchTradeCpiLeg {
                asset_index: 0,
                market_id: first_generation_market_id((0) as u16),
                size_q: sz,
                fee_bps: 100,
                limit_price: 0,
            },
            BatchTradeCpiLeg {
                asset_index: 0,
                market_id: first_generation_market_id((0) as u16),
                size_q: -sz,
                fee_bps: 100,
                limit_price: 0,
            },
        ],
    )
    .expect_err("duplicate-asset BatchTradeCpi must reject before matcher CPI");
    assert!(
        duplicate_err.contains("Custom(9)"),
        "duplicate-asset BatchTradeCpi must fail as InvalidInstruction before hostile matcher validation, got {duplicate_err}"
    );
    assert!(
        !duplicate_err.contains("InvalidAccountData"),
        "duplicate-asset BatchTradeCpi must not reach hostile matcher validation: {duplicate_err}"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&ta).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&la).unwrap(), lp_before);
}

#[test]
fn v16_bpf_tradecpi_executes_through_external_matcher_and_is_bounded() {
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        trade_fee_base_bps: 100,
        ..V16CuMarketParams::default()
    });
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);

    let taker_owner = Keypair::new();
    let maker_owner = Keypair::new();
    let taker_account = env.create_portfolio(&taker_owner);
    let maker_account = env.create_portfolio(&maker_owner);
    env.deposit(&taker_owner, taker_account, 1_000_000);
    env.deposit(&maker_owner, maker_account, 1_000_000);

    let (matcher_ctx, matcher_delegate, init_matcher_cu) =
        env.init_matcher_context_authorized(matcher_program, &maker_owner, maker_account);
    let trade_cpi_cu = env.trade_cpi_with_cu(
        &taker_owner,
        taker_account,
        &maker_owner,
        maker_account,
        matcher_program,
        matcher_ctx,
        matcher_delegate,
        (10 * POS_SCALE) as i128,
        100,
    );
    println!("v16 matcher init CU: {init_matcher_cu}, TradeCpi BPF CU: {trade_cpi_cu}");
    assert!(
        trade_cpi_cu <= TRADE_CU_LIMIT,
        "TradeCpi CU {} exceeded limit {}",
        trade_cpi_cu,
        TRADE_CU_LIMIT
    );

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let taker_data = env.svm.get_account(&taker_account).unwrap().data;
    let maker_data = env.svm.get_account(&maker_account).unwrap().data;
    let matcher_data = env.svm.get_account(&matcher_ctx).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    let taker = state::read_portfolio(&taker_data).unwrap();
    let maker = state::read_portfolio(&maker_data).unwrap();
    println!(
        "TradeCpi BPF taker_basis={}, maker_basis={}, insurance={}",
        taker.legs[0].basis_pos_q.get(),
        maker.legs[0].basis_pos_q.get(),
        group.insurance
    );
    assert_eq!(group.assets[0].effective_price, 100);
    assert_eq!(taker.legs[0].basis_pos_q.get(), (10 * POS_SCALE) as i128);
    assert_eq!(maker.legs[0].basis_pos_q.get(), -((10 * POS_SCALE) as i128));
    assert_eq!(
        group.insurance, 20,
        "passive matcher fills at oracle price; 100 bps charges 10 to each side"
    );
    assert_eq!(
        u32::from_le_bytes(matcher_data[0..4].try_into().unwrap()),
        MATCHER_ABI_VERSION,
        "LiteSVM matcher path must use the same ABI version as the wrapper"
    );
    assert_eq!(
        u64::from_le_bytes(matcher_data[56..64].try_into().unwrap()),
        0,
        "matcher must echo the requested asset index in the v3 return slot"
    );
    assert_eq!(group.c_tot + group.insurance, group.vault);
}

#[test]
fn v16_bpf_tradecpi_external_matcher_executes_on_added_asset() {
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        trade_fee_base_bps: 100,
        ..V16CuMarketParams::default()
    });
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    env.activate_asset(1, 1, 100);
    env.activate_asset(2, 2, 250);

    let taker_owner = Keypair::new();
    let maker_owner = Keypair::new();
    let taker_account = env.create_portfolio(&taker_owner);
    let maker_account = env.create_portfolio(&maker_owner);
    env.deposit(&taker_owner, taker_account, 1_000_000);
    env.deposit(&maker_owner, maker_account, 1_000_000);

    let (matcher_ctx, matcher_delegate, _) =
        env.init_matcher_context_authorized(matcher_program, &maker_owner, maker_account);
    let trade_cpi_cu = env.trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker_account,
        &maker_owner,
        maker_account,
        matcher_program,
        matcher_ctx,
        matcher_delegate,
        2,
        (10 * POS_SCALE) as i128,
        100,
    );
    println!("v16 TradeCpi BPF nonzero-asset CU: {trade_cpi_cu}");
    assert!(
        trade_cpi_cu <= TRADE_CU_LIMIT,
        "TradeCpi nonzero-asset CU {} exceeded limit {}",
        trade_cpi_cu,
        TRADE_CU_LIMIT
    );

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let taker_data = env.svm.get_account(&taker_account).unwrap().data;
    let maker_data = env.svm.get_account(&maker_account).unwrap().data;
    let matcher_data = env.svm.get_account(&matcher_ctx).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    let taker = state::read_portfolio(&taker_data).unwrap();
    let maker = state::read_portfolio(&maker_data).unwrap();

    assert_eq!(group.assets[0].oi_eff_long_q, 0);
    assert_eq!(group.assets[0].oi_eff_short_q, 0);
    assert_eq!(group.assets[2].effective_price, 250);
    assert_eq!(group.assets[2].oi_eff_long_q, 10 * POS_SCALE);
    assert_eq!(group.assets[2].oi_eff_short_q, 10 * POS_SCALE);
    assert_eq!(active_bitmap(&taker), active_bitmap_with(&[0]));
    assert_eq!(active_bitmap(&maker), active_bitmap_with(&[0]));
    assert_eq!(
        active_leg_for_asset(&taker, 2).basis_pos_q,
        (10 * POS_SCALE) as i128
    );
    assert_eq!(
        active_leg_for_asset(&maker, 2).basis_pos_q,
        -((10 * POS_SCALE) as i128)
    );
    assert_eq!(
        group.insurance, 50,
        "passive matcher fills asset 2 at 250; notional=2500 and 100 bps charges 25 to each side"
    );
    assert_eq!(
        u64::from_le_bytes(matcher_data[56..64].try_into().unwrap()),
        2,
        "external matcher must echo the requested nonzero asset index"
    );
    assert_eq!(group.c_tot + group.insurance, group.vault);
}
