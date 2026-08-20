//! INV-009 - partial-fill and retry accounting.
//!
//! CPI matchers may have less capacity than the signed requested size. A single
//! trade may opt into a short fill through `FLAG_PARTIAL_OK`; it must account only
//! the executed quantity and invalidate the consumed position epoch. A batch has
//! no signed per-leg minimum or remaining-allowance ledger, so matcher-selected
//! short fills must reject atomically rather than silently change the strategy's
//! leg ratio.

use super::*;

const FLAGGED_PARTIAL_MODE: u8 = 15;
const ASYMMETRIC_BATCH_PARTIAL_MODE: u8 = 16;

fn setup_hostile_partial_env(
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
    env.update_trade_fee_policy_with_cu(100);
    for asset_index in 0..asset_count {
        env.configure_auth_mark_for_asset_as_admin(asset_index, 1, 100);
    }
    let matcher_program = Pubkey::new_unique();
    env.svm.add_program(
        matcher_program,
        &std::fs::read(hostile_matcher_program_path()).expect("read hostile matcher BPF"),
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
        &matcher_program,
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
                owner: matcher_program,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.set_matcher_config(matcher_program, &lp, lp_account, ctx, delegate, 1);
    (
        env,
        taker,
        lp,
        taker_account,
        lp_account,
        matcher_program,
        ctx,
        delegate,
    )
}

fn set_hostile_matcher_mode(env: &mut V16CuEnv, ctx: Pubkey, matcher_program: Pubkey, mode: u8) {
    let mut data = vec![0u8; MATCHER_CONTEXT_LEN];
    data[0] = mode;
    env.svm
        .set_account(
            ctx,
            Account {
                lamports: 1_000_000_000,
                data,
                owner: matcher_program,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
}

#[test]
fn v16_program_tradecpi_short_fill_rejects_atomically_and_retries_cleanly() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new();
    let maker_owner = Keypair::new();
    let taker_account = env.create_portfolio(&taker_owner);
    let maker_account = env.create_portfolio(&maker_owner);
    env.deposit(&taker_owner, taker_account, 1_000_000);
    env.deposit(&maker_owner, maker_account, 1_000_000);

    let cap: u128 = 5 * POS_SCALE;
    let (matcher_ctx, matcher_delegate, _) = env.init_matcher_context_with_data_authorized(
        matcher_program,
        &maker_owner,
        maker_account,
        encode_matcher_init_passive(cap),
    );
    let (_, before) = env.market_state();
    let taker_before = env.svm.get_account(&taker_account).unwrap();
    let maker_before = env.svm.get_account(&maker_account).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    let rejected = env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker_account,
        &maker_owner,
        maker_account,
        matcher_program,
        matcher_ctx,
        matcher_delegate,
        0,
        (10 * POS_SCALE) as i128,
        100,
    );
    assert!(
        rejected.is_err(),
        "a matcher that cannot fully fill the request must reject atomically"
    );
    assert_eq!(env.svm.get_account(&taker_account).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&maker_account).unwrap(), maker_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    let (_, after_reject) = env.market_state();
    assert_eq!(after_reject.assets[0].oi_eff_long_q, 0);
    assert_eq!(after_reject.assets[0].oi_eff_short_q, 0);
    assert_eq!(after_reject.insurance, before.insurance);
    assert_eq!(after_reject.vault, before.vault);

    let retry_cu = env.trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker_account,
        &maker_owner,
        maker_account,
        matcher_program,
        matcher_ctx,
        matcher_delegate,
        0,
        cap as i128,
        100,
    );
    assert_cu_within(
        "TradeCpi exact-cap retry after short-fill reject",
        retry_cu,
        TRADE_CU_LIMIT,
    );
    let taker = env.portfolio_state(taker_account);
    let maker = env.portfolio_state(maker_account);
    assert_eq!(active_leg_for_asset(&taker, 0).basis_pos_q, cap as i128);
    assert_eq!(active_leg_for_asset(&maker, 0).basis_pos_q, -(cap as i128));
    let (_, after_retry) = env.market_state();
    assert_eq!(after_retry.assets[0].oi_eff_long_q, cap);
    assert_eq!(after_retry.assets[0].oi_eff_short_q, cap);
    assert_eq!(after_retry.c_tot + after_retry.insurance, after_retry.vault);
    assert_eq!(after_retry.vault as u64, env.token_amount(env.vault));
}

#[test]
fn v16_program_tradecpi_flagged_partial_accounts_actual_fill_and_requires_fresh_retry() {
    let (mut env, taker, _lp, taker_account, lp_account, matcher, ctx, delegate) =
        setup_hostile_partial_env(1);
    let request_q = (10 * POS_SCALE) as i128;
    let partial_q = request_q / 2;
    let accounts = |env: &V16CuEnv| {
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker_account, false),
            AccountMeta::new(lp_account, false),
            AccountMeta::new_readonly(matcher, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ]
    };

    set_hostile_matcher_mode(&mut env, ctx, matcher, FLAGGED_PARTIAL_MODE);
    let stale_request = env.trade_cpi_ix(taker_account, lp_account, 0, request_q, 100, 0);
    let (_, before) = env.market_state();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let taker_epoch_before = env.portfolio_position_epoch(taker_account);
    let lp_epoch_before = env.portfolio_position_epoch(lp_account);
    env.svm.expire_blockhash();
    let partial_cu = env
        .send(stale_request.clone(), accounts(&env), &[&taker])
        .expect("flagged single partial fill must execute");
    assert_cu_within("TradeCpi flagged partial fill", partial_cu, 1_400_000);

    let taker_state = env.portfolio_state(taker_account);
    let lp_state = env.portfolio_state(lp_account);
    assert_eq!(active_leg_for_asset(&taker_state, 0).basis_pos_q, partial_q);
    assert_eq!(active_leg_for_asset(&lp_state, 0).basis_pos_q, -partial_q);
    assert_eq!(
        env.portfolio_position_epoch(taker_account),
        taker_epoch_before + 1
    );
    assert_eq!(
        env.portfolio_position_epoch(lp_account),
        lp_epoch_before + 1
    );
    let (_, after_partial) = env.market_state();
    assert_eq!(after_partial.assets[0].oi_eff_long_q, partial_q as u128);
    assert_eq!(after_partial.assets[0].oi_eff_short_q, partial_q as u128);
    assert_eq!(after_partial.insurance - before.insurance, 10);
    assert_eq!(before.c_tot - after_partial.c_tot, 10);
    assert_eq!(after_partial.vault, before.vault);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    let market_before_stale = env.svm.get_account(&env.market).unwrap();
    let taker_before_stale = env.svm.get_account(&taker_account).unwrap();
    let lp_before_stale = env.svm.get_account(&lp_account).unwrap();
    let ctx_before_stale = env.svm.get_account(&ctx).unwrap();
    env.svm.expire_blockhash();
    let stale = env.send(stale_request, accounts(&env), &[&taker]);
    assert!(
        stale.is_err(),
        "the consumed pre-partial position epoch must not replay"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_stale
    );
    assert_eq!(
        env.svm.get_account(&taker_account).unwrap(),
        taker_before_stale
    );
    assert_eq!(env.svm.get_account(&lp_account).unwrap(), lp_before_stale);
    assert_eq!(env.svm.get_account(&ctx).unwrap(), ctx_before_stale);

    set_hostile_matcher_mode(&mut env, ctx, matcher, 9);
    env.svm.expire_blockhash();
    env.send(
        env.trade_cpi_ix(taker_account, lp_account, 0, request_q - partial_q, 100, 0),
        accounts(&env),
        &[&taker],
    )
    .expect("a fresh request must be able to fill the remaining quantity");
    let taker_after_retry = env.portfolio_state(taker_account);
    let lp_after_retry = env.portfolio_state(lp_account);
    assert_eq!(
        active_leg_for_asset(&taker_after_retry, 0).basis_pos_q,
        request_q
    );
    assert_eq!(
        active_leg_for_asset(&lp_after_retry, 0).basis_pos_q,
        -request_q
    );
    let (_, after_retry) = env.market_state();
    assert_eq!(after_retry.assets[0].oi_eff_long_q, request_q as u128);
    assert_eq!(after_retry.assets[0].oi_eff_short_q, request_q as u128);
    assert_eq!(after_retry.insurance - before.insurance, 20);
    assert_eq!(before.c_tot - after_retry.c_tot, 20);
    assert_eq!(after_retry.c_tot + after_retry.insurance, after_retry.vault);
}

#[test]
fn v16_program_batch_tradecpi_flagged_partial_cannot_change_atomic_leg_ratio() {
    for mode in [FLAGGED_PARTIAL_MODE, ASYMMETRIC_BATCH_PARTIAL_MODE] {
        let (mut env, taker, _lp, taker_account, lp_account, matcher, ctx, delegate) =
            setup_hostile_partial_env(2);
        set_hostile_matcher_mode(&mut env, ctx, matcher, mode);
        let request_q = (10 * POS_SCALE) as i128;
        let market_before = env.svm.get_account(&env.market).unwrap();
        let taker_before = env.svm.get_account(&taker_account).unwrap();
        let lp_before = env.svm.get_account(&lp_account).unwrap();
        let ctx_before = env.svm.get_account(&ctx).unwrap();
        let vault_before = env.svm.get_account(&env.vault).unwrap();
        env.svm.expire_blockhash();
        let rejected = env.send(
            env.batch_trade_cpi_ix(
                taker_account,
                lp_account,
                vec![
                    BatchTradeCpiLeg {
                        asset_index: 0,
                        market_id: first_generation_market_id(0),
                        size_q: request_q,
                        fee_bps: 100,
                        limit_price: 0,
                    },
                    BatchTradeCpiLeg {
                        asset_index: 1,
                        market_id: first_generation_market_id(1),
                        size_q: -request_q,
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
                AccountMeta::new_readonly(matcher, false),
                AccountMeta::new(ctx, false),
                AccountMeta::new_readonly(delegate, false),
            ],
            &[&taker],
        );
        assert!(
            rejected.is_err(),
            "batch mode {mode} must not let a matcher rewrite signed leg quantities"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&taker_account).unwrap(), taker_before);
        assert_eq!(env.svm.get_account(&lp_account).unwrap(), lp_before);
        assert_eq!(env.svm.get_account(&ctx).unwrap(), ctx_before);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

        set_hostile_matcher_mode(&mut env, ctx, matcher, 9);
        env.svm.expire_blockhash();
        let full = env.send(
            env.batch_trade_cpi_ix(
                taker_account,
                lp_account,
                vec![
                    BatchTradeCpiLeg {
                        asset_index: 0,
                        market_id: first_generation_market_id(0),
                        size_q: request_q,
                        fee_bps: 100,
                        limit_price: 0,
                    },
                    BatchTradeCpiLeg {
                        asset_index: 1,
                        market_id: first_generation_market_id(1),
                        size_q: -request_q,
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
                AccountMeta::new_readonly(matcher, false),
                AccountMeta::new(ctx, false),
                AccountMeta::new_readonly(delegate, false),
            ],
            &[&taker],
        );
        assert!(
            full.is_ok(),
            "rejecting a matcher-selected short fill must not block a full-fill retry: {full:?}"
        );
        let taker_state = env.portfolio_state(taker_account);
        assert_eq!(active_leg_for_asset(&taker_state, 0).basis_pos_q, request_q);
        assert_eq!(
            active_leg_for_asset(&taker_state, 1).basis_pos_q,
            -request_q
        );
    }
}
