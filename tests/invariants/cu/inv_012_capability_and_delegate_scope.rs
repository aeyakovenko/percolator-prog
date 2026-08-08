//! INV-012 - capability and delegate scope.
//!
//! Matcher capability is portfolio-local and explicit: disabling the LP's tuple
//! must block both CPI trade rails, and a non-owner signer must not be able to
//! revoke or rewrite another LP's matcher authorization.

use super::*;

#[test]
fn v16_program_disabled_lp_matcher_config_blocks_all_cpi_fills() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&lp_owner, lp, 1_000_000);
    let (ctx, delegate, _) = env.init_auth_matcher_context(matcher_program, &lp_owner, lp);

    env.set_matcher_config(matcher_program, &lp_owner, lp, ctx, delegate, 0);
    assert_eq!(env.portfolio_matcher_config(lp).enabled(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker).unwrap();
    let lp_before = env.svm.get_account(&lp).unwrap();
    let ctx_before = env.svm.get_account(&ctx).unwrap();

    env.svm.expire_blockhash();
    let single = env.send(
        env.trade_cpi_ix(taker, lp, 0, (5 * POS_SCALE) as i128, 100, 0),
        vec![
            AccountMeta::new(taker_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker, false),
            AccountMeta::new(lp, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ],
        &[&taker_owner],
    );
    assert!(single.is_err(), "disabled LP tuple must block TradeCpi");
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before);
    assert_eq!(env.svm.get_account(&ctx).unwrap(), ctx_before);

    env.svm.expire_blockhash();
    let batch = env.send(
        env.batch_trade_cpi_ix(
            taker,
            lp,
            vec![BatchTradeCpiLeg {
                asset_index: 0,
                market_id: first_generation_market_id(0),
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
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ],
        &[&taker_owner],
    );
    assert!(batch.is_err(), "disabled LP tuple must block BatchTradeCpi");
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before);
    assert_eq!(env.svm.get_account(&ctx).unwrap(), ctx_before);

    env.set_matcher_config(matcher_program, &lp_owner, lp, ctx, delegate, 1);
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
        (5 * POS_SCALE) as i128,
        100,
    )
    .expect("LP owner can re-enable the exact matcher tuple");
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(taker), 0).basis_pos_q,
        (5 * POS_SCALE) as i128
    );
}

#[test]
fn v16_program_non_owner_cannot_revoke_lp_matcher_capability() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let attacker = Keypair::new();
    env.ensure_signer_account(attacker.pubkey());
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&lp_owner, lp, 1_000_000);
    let (ctx, delegate, _) = env.init_auth_matcher_context(matcher_program, &lp_owner, lp);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let lp_before = env.svm.get_account(&lp).unwrap();
    let ctx_before = env.svm.get_account(&ctx).unwrap();
    let portfolio_id = env.portfolio_id(lp);
    let expected_sequence = env.portfolio_matcher_sequence(lp);

    env.svm.expire_blockhash();
    let revoke = env.send(
        ProgInstruction::SetMatcherConfig {
            portfolio_id,
            expected_sequence,
            enabled: 0,
            trade_fee_cap_bps: 0,
        },
        vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new_readonly(env.market, false),
            AccountMeta::new(lp, false),
        ],
        &[&attacker],
    );
    assert!(
        revoke.is_err(),
        "a non-owner signer must not change an LP matcher capability"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before);
    assert_eq!(env.svm.get_account(&ctx).unwrap(), ctx_before);
    assert_eq!(env.portfolio_matcher_config(lp).enabled(), 1);

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
        (5 * POS_SCALE) as i128,
        100,
    )
    .expect("failed non-owner revoke must not DoS authorized matcher fills");
}

#[test]
fn v16_program_tradecpi_requires_exact_lp_authorized_matcher_tuple() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    let honest = Pubkey::new_unique();
    let auth_matcher_bytes =
        std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(honest, &auth_matcher_bytes);
    let hostile = Pubkey::new_unique();
    env.svm.add_program(
        hostile,
        &std::fs::read(hostile_matcher_program_path()).unwrap(),
    );
    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&lp_owner, lp, 1_000_000);

    let (honest_ctx, honest_delegate, _) = env.init_auth_matcher_context(honest, &lp_owner, lp);
    let hostile_ctx = Pubkey::new_unique();
    let hostile_delegate = matcher_delegate_key(
        &env.program_id,
        &env.market,
        &lp,
        &lp_owner.pubkey(),
        &hostile,
        &hostile_ctx,
    );
    env.svm
        .set_account(
            hostile_delegate,
            Account {
                lamports: 1_000_000_000,
                data: vec![],
                owner: Pubkey::default(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let mut hostile_ctx_data = vec![0u8; MATCHER_CONTEXT_LEN];
    hostile_ctx_data[0] = 9;
    env.svm
        .set_account(
            hostile_ctx,
            Account {
                lamports: 1_000_000_000,
                data: hostile_ctx_data,
                owner: hostile,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let send = |env: &mut V16CuEnv,
                matcher_program: Pubkey,
                matcher_context: Pubkey,
                matcher_delegate: Pubkey| {
        env.svm.expire_blockhash();
        env.send(
            env.trade_cpi_ix(taker, lp, 0, (5 * POS_SCALE) as i128, 100, 0),
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
        )
    };

    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker).unwrap();
    let lp_before = env.svm.get_account(&lp).unwrap();
    let ctx_before = env.svm.get_account(&hostile_ctx).unwrap();
    let replay = send(&mut env, hostile, hostile_ctx, hostile_delegate);
    assert!(
        replay.is_err(),
        "LP capability for one tuple must not authorize different TradeCpi args"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before);
    assert_eq!(env.svm.get_account(&hostile_ctx).unwrap(), ctx_before);
    assert_eq!(env.market_state().1.assets[0].oi_eff_long_q, 0);
    assert_eq!(env.portfolio_state(taker).legs[0].basis_pos_q.get(), 0);
    assert_eq!(env.portfolio_state(lp).legs[0].basis_pos_q.get(), 0);

    send(&mut env, honest, honest_ctx, honest_delegate)
        .expect("the exact LP-authorized matcher tuple still fills");
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(taker), 0).basis_pos_q,
        (5 * POS_SCALE) as i128
    );
}

#[test]
fn v16_program_batch_tradecpi_requires_exact_lp_authorized_matcher_tuple() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    let honest = Pubkey::new_unique();
    let auth_matcher_bytes =
        std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(honest, &auth_matcher_bytes);
    let hostile = Pubkey::new_unique();
    env.svm.add_program(
        hostile,
        &std::fs::read(hostile_matcher_program_path()).unwrap(),
    );
    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&lp_owner, lp, 1_000_000);

    let (honest_ctx, honest_delegate, _) = env.init_auth_matcher_context(honest, &lp_owner, lp);
    let hostile_ctx = Pubkey::new_unique();
    let hostile_delegate = matcher_delegate_key(
        &env.program_id,
        &env.market,
        &lp,
        &lp_owner.pubkey(),
        &hostile,
        &hostile_ctx,
    );
    env.svm
        .set_account(
            hostile_delegate,
            Account {
                lamports: 1_000_000_000,
                data: vec![],
                owner: Pubkey::default(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let mut hostile_ctx_data = vec![0u8; MATCHER_CONTEXT_LEN];
    hostile_ctx_data[0] = 9;
    env.svm
        .set_account(
            hostile_ctx,
            Account {
                lamports: 1_000_000_000,
                data: hostile_ctx_data,
                owner: hostile,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker).unwrap();
    let lp_before = env.svm.get_account(&lp).unwrap();
    let ctx_before = env.svm.get_account(&hostile_ctx).unwrap();
    let sz = (5 * POS_SCALE) as i128;
    env.svm.expire_blockhash();
    let rejected = env.send(
        env.batch_trade_cpi_ix(
            taker,
            lp,
            vec![
                BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: first_generation_market_id(0),
                    size_q: sz,
                    fee_bps: 100,
                    limit_price: 0,
                },
                BatchTradeCpiLeg {
                    asset_index: 1,
                    market_id: first_generation_market_id(1),
                    size_q: -sz,
                    fee_bps: 100,
                    limit_price: 0,
                },
            ],
        ),
        vec![
            AccountMeta::new(taker_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker, false),
            AccountMeta::new(lp, false),
            AccountMeta::new_readonly(hostile, false),
            AccountMeta::new(hostile_ctx, false),
            AccountMeta::new_readonly(hostile_delegate, false),
        ],
        &[&taker_owner],
    );
    assert!(
        rejected.is_err(),
        "LP capability for one tuple must not authorize different BatchTradeCpi args"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before);
    assert_eq!(env.svm.get_account(&hostile_ctx).unwrap(), ctx_before);
    assert_eq!(env.market_state().1.assets[0].oi_eff_long_q, 0);
    assert_eq!(env.market_state().1.assets[1].oi_eff_long_q, 0);

    env.svm.expire_blockhash();
    env.send(
        env.batch_trade_cpi_ix(
            taker,
            lp,
            vec![BatchTradeCpiLeg {
                asset_index: 0,
                market_id: first_generation_market_id(0),
                size_q: sz,
                fee_bps: 100,
                limit_price: 0,
            }],
        ),
        vec![
            AccountMeta::new(taker_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker, false),
            AccountMeta::new(lp, false),
            AccountMeta::new_readonly(honest, false),
            AccountMeta::new(honest_ctx, false),
            AccountMeta::new_readonly(honest_delegate, false),
        ],
        &[&taker_owner],
    )
    .expect("the exact LP-configured matcher tuple still batch-fills");
}

#[test]
fn v16_attack_cross_lp_cannot_overwrite_lp_matcher_config() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new();
    let victim_owner = Keypair::new();
    let attacker_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let victim_lp = env.create_portfolio(&victim_owner);
    let attacker_lp = env.create_portfolio(&attacker_owner);
    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&victim_owner, victim_lp, 1_000_000);
    env.deposit(&attacker_owner, attacker_lp, 1_000_000);
    let (victim_ctx, victim_delegate, _) =
        env.init_auth_matcher_context(matcher_program, &victim_owner, victim_lp);
    let (attacker_ctx, attacker_delegate, _) =
        env.init_auth_matcher_context(matcher_program, &attacker_owner, attacker_lp);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let victim_before = env.svm.get_account(&victim_lp).unwrap();
    let attacker_before = env.svm.get_account(&attacker_lp).unwrap();
    let portfolio_id = env.portfolio_id(victim_lp);
    let expected_sequence = env.portfolio_matcher_sequence(victim_lp);

    env.svm.expire_blockhash();
    let overwrite = env.send(
        ProgInstruction::SetMatcherConfig {
            portfolio_id,
            expected_sequence,
            enabled: 0,
            trade_fee_cap_bps: 0,
        },
        vec![
            AccountMeta::new(attacker_owner.pubkey(), true),
            AccountMeta::new_readonly(env.market, false),
            AccountMeta::new(victim_lp, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new_readonly(attacker_ctx, false),
            AccountMeta::new_readonly(attacker_delegate, false),
        ],
        &[&attacker_owner],
    );
    assert!(
        overwrite.is_err(),
        "one LP must not overwrite another LP's matcher config with its own tuple"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&victim_lp).unwrap(), victim_before);
    assert_eq!(env.svm.get_account(&attacker_lp).unwrap(), attacker_before);

    env.svm.expire_blockhash();
    let victim_fill = env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &victim_owner,
        victim_lp,
        matcher_program,
        victim_ctx,
        victim_delegate,
        0,
        (5 * POS_SCALE) as i128,
        100,
    );
    assert!(
        victim_fill.is_ok(),
        "failed cross-LP overwrite must not DoS the victim LP's authorized matcher fills: {victim_fill:?}"
    );
}

#[test]
fn v16_attack_set_lp_matcher_config_cannot_target_protocol_accounts() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let lp_owner = Keypair::new();
    let lp = env.create_portfolio(&lp_owner);
    let ctx = Pubkey::new_unique();
    let delegate = matcher_delegate_key(
        &env.program_id,
        &env.market,
        &lp,
        &lp_owner.pubkey(),
        &matcher_program,
        &ctx,
    );
    env.try_init_auth_matcher_context_with_delegate(matcher_program, &lp_owner, lp, ctx, delegate)
        .expect("init auth matcher context without setting percolator auth");
    let portfolio_id = env.portfolio_id(lp);
    let expected_sequence = env.portfolio_matcher_sequence(lp);

    let send_with_lp_account = |env: &mut V16CuEnv, lp_account: Pubkey| {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::SetMatcherConfig {
                portfolio_id,
                expected_sequence,
                enabled: 1,
                trade_fee_cap_bps: 10_000,
            },
            vec![
                AccountMeta::new(lp_owner.pubkey(), true),
                AccountMeta::new_readonly(env.market, false),
                AccountMeta::new(lp_account, false),
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new_readonly(ctx, false),
                AccountMeta::new_readonly(delegate, false),
            ],
            &[&lp_owner],
        )
    };

    let market = env.market;
    let market_before = env.svm.get_account(&market).unwrap();
    let lp_before = env.svm.get_account(&lp).unwrap();
    let market_alias = send_with_lp_account(&mut env, market);
    assert!(
        market_alias.is_err(),
        "SetMatcherConfig must not treat the market as an LP account"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before);

    env.set_matcher_config(matcher_program, &lp_owner, lp, ctx, delegate, 1);
    let auth_state = env.portfolio_matcher_config(lp);
    assert_eq!(
        auth_state.enabled(),
        1,
        "a real LP account stores the matcher program/context config"
    );
    assert_eq!(auth_state.matcher_program, matcher_program.to_bytes());
    assert_eq!(auth_state.matcher_context, ctx.to_bytes());
    assert_eq!(auth_state.matcher_delegate, delegate.to_bytes());
}

#[test]
fn v16_attack_permissionless_lp_cpi_rejects_wrong_delegate_owner_or_account_binding() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let wrong_lp_owner = Keypair::new();
    env.ensure_signer_account(wrong_lp_owner.pubkey());
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    let other_lp_same_owner = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&lp_owner, lp, 1_000_000);
    env.deposit(&lp_owner, other_lp_same_owner, 1_000_000);
    let (ctx, _delegate, _) = env.init_auth_matcher_context(matcher_program, &lp_owner, lp);
    let (other_ctx, other_delegate, _) =
        env.init_auth_matcher_context(matcher_program, &lp_owner, other_lp_same_owner);
    let wrong_owner_delegate = matcher_delegate_key(
        &env.program_id,
        &env.market,
        &lp,
        &wrong_lp_owner.pubkey(),
        &matcher_program,
        &ctx,
    );
    env.svm
        .set_account(
            wrong_owner_delegate,
            Account {
                lamports: 1_000_000_000,
                data: vec![],
                owner: Pubkey::default(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let sz = (5 * POS_SCALE) as i128;

    env.svm.expire_blockhash();
    let wrong_delegate_single = env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &wrong_lp_owner,
        lp,
        matcher_program,
        ctx,
        wrong_owner_delegate,
        0,
        sz,
        100,
    );
    assert!(
        wrong_delegate_single.is_err(),
        "single TradeCpi must reject a delegate derived from the wrong LP owner"
    );

    env.svm.expire_blockhash();
    let wrong_account_single = env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &lp_owner,
        lp,
        matcher_program,
        other_ctx,
        other_delegate,
        0,
        sz,
        100,
    );
    assert!(
        wrong_account_single.is_err(),
        "single TradeCpi must reject a delegate/context bound to a different LP portfolio"
    );

    let batch_ix = env.batch_trade_cpi_ix(
        taker,
        lp,
        vec![BatchTradeCpiLeg {
            asset_index: 0,
            market_id: first_generation_market_id((0) as u16),
            size_q: sz,
            fee_bps: 100,
            limit_price: 0,
        }],
    );
    let taker_owner_key = taker_owner.pubkey();
    let market = env.market;
    let metas = |context: Pubkey, del: Pubkey| {
        vec![
            AccountMeta::new(taker_owner_key, true),
            AccountMeta::new(market, false),
            AccountMeta::new(taker, false),
            AccountMeta::new(lp, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new(context, false),
            AccountMeta::new_readonly(del, false),
        ]
    };

    env.svm.expire_blockhash();
    let wrong_delegate_batch = env.send(
        batch_ix.clone(),
        metas(ctx, wrong_owner_delegate),
        &[&taker_owner],
    );
    assert!(
        wrong_delegate_batch.is_err(),
        "BatchTradeCpi must reject a delegate derived from the wrong LP owner"
    );

    env.svm.expire_blockhash();
    let wrong_account_batch = env.send(batch_ix, metas(other_ctx, other_delegate), &[&taker_owner]);
    assert!(
        wrong_account_batch.is_err(),
        "BatchTradeCpi must reject a delegate/context bound to a different LP portfolio"
    );

    let group = env.market_state().1;
    assert_eq!(
        group.assets[0].oi_eff_long_q, 0,
        "no OI created by rejected CPI attempts"
    );
    assert_eq!(
        env.portfolio_state(taker).legs[0].basis_pos_q.get(),
        0,
        "taker untouched"
    );
    assert_eq!(
        env.portfolio_state(lp).legs[0].basis_pos_q.get(),
        0,
        "LP untouched"
    );
}

#[test]
fn v16_attack_nocpi_trades_still_require_lp_owner_signature() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&lp_owner, lp, 1_000_000);

    env.svm.expire_blockhash();
    let single = env.send(
        env.trade_no_cpi_ix(taker, lp, 0, (10 * POS_SCALE) as i128, 100, 0),
        vec![
            AccountMeta::new(taker_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker, false),
            AccountMeta::new(lp, false),
        ],
        &[&taker_owner],
    );
    assert!(
        single.is_err(),
        "TradeNoCpi without the LP owner signature must reject"
    );

    env.svm.expire_blockhash();
    let batch = env.send(
        env.batch_trade_no_cpi_ix(
            taker,
            lp,
            vec![
                BatchTradeLeg {
                    asset_index: 0,
                    market_id: first_generation_market_id((0) as u16),
                    size_q: (5 * POS_SCALE) as i128,
                    exec_price: 100,
                    fee_bps: 0,
                },
                BatchTradeLeg {
                    asset_index: 1,
                    market_id: first_generation_market_id((1) as u16),
                    size_q: -(5 * POS_SCALE as i128),
                    exec_price: 100,
                    fee_bps: 0,
                },
            ],
        ),
        vec![
            AccountMeta::new(taker_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker, false),
            AccountMeta::new(lp, false),
        ],
        &[&taker_owner],
    );
    assert!(
        batch.is_err(),
        "BatchTradeNoCpi without the LP owner signature must reject"
    );

    let group = env.market_state().1;
    assert_eq!(group.assets[0].oi_eff_long_q, 0);
    assert_eq!(group.assets[1].oi_eff_long_q, 0);
    assert_eq!(env.portfolio_state(taker).legs[0].basis_pos_q.get(), 0);
    assert_eq!(env.portfolio_state(lp).legs[0].basis_pos_q.get(), 0);
}

// full-interface sweep / issue: removing the LP signer from TradeCpi is only safe if Percolator
// verifies that the LP owner explicitly authorized this matcher program/context. A hostile matcher can
// otherwise return a perfectly well-formed oracle-priced fill and force a victim LP portfolio into a
// position. This is the single-fill reproducer: no LP signature and no Percolator matcher config
// must reject before any position is opened.
#[test]
fn v16_attack_tradecpi_rejects_unapproved_unsigned_lp_matcher() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
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
    let mut ctx_data = vec![0u8; MATCHER_CONTEXT_LEN];
    ctx_data[0] = 9; // hostile fixture faithful mode: returns a valid fill.
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

    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&ta).unwrap();
    let lp_before = env.svm.get_account(&la).unwrap();
    let ctx_before = env.svm.get_account(&ctx).unwrap();

    env.svm.expire_blockhash();
    let r = env.send(
        env.trade_cpi_ix(ta, la, 0, (5 * POS_SCALE) as i128, 100, 0),
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
    );
    assert!(
        r.is_err(),
        "unauthorized matcher must not be able to force an unsigned LP TradeCpi fill"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&ta).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&la).unwrap(), lp_before);
    assert_eq!(env.svm.get_account(&ctx).unwrap(), ctx_before);
    assert!(!has_active_leg_for_asset(
        &state::read_portfolio(&env.svm.get_account(&la).unwrap().data).unwrap(),
        0
    ));
}

// Same config boundary for the batched matcher CPI path. A hostile matcher that emits valid
// return-data for every leg is still unapproved for the LP unless the LP stored that matcher tuple
// on its portfolio.
#[test]
fn v16_attack_batch_tradecpi_rejects_unapproved_unsigned_lp_matcher() {
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
    let mut ctx_data = vec![0u8; MATCHER_CONTEXT_LEN];
    ctx_data[0] = 9; // faithful batch replies.
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

    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&ta).unwrap();
    let lp_before = env.svm.get_account(&la).unwrap();
    let ctx_before = env.svm.get_account(&ctx).unwrap();
    let sz = (5 * POS_SCALE) as i128;

    env.svm.expire_blockhash();
    let r = env.send(
        env.batch_trade_cpi_ix(
            ta,
            la,
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
    );
    assert!(
        r.is_err(),
        "unauthorized matcher must not be able to force an unsigned LP BatchTradeCpi fill"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&ta).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&la).unwrap(), lp_before);
    assert_eq!(env.svm.get_account(&ctx).unwrap(), ctx_before);
    let lp_state = state::read_portfolio(&env.svm.get_account(&la).unwrap().data).unwrap();
    assert!(!has_active_leg_for_asset(&lp_state, 0));
    assert!(!has_active_leg_for_asset(&lp_state, 1));
}

#[test]
fn v16_bpf_tradecpi_permissionless_lp_fill_does_not_need_lp_owner_signature() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&lp_owner, lp, 1_000_000);
    let (matcher_ctx, matcher_delegate, init_cu) =
        env.init_auth_matcher_context_via_system_create(matcher_program, &lp_owner, lp);

    env.svm.expire_blockhash();
    let cu = env
        .try_trade_cpi_with_cu_on_asset(
            &taker_owner,
            taker,
            &lp_owner,
            lp,
            matcher_program,
            matcher_ctx,
            matcher_delegate,
            0,
            (10 * POS_SCALE) as i128,
            100,
        )
        .expect("matcher CPI fill succeeds with only the taker signing");
    println!("v16 permissionless LP matcher system-init CU: {init_cu}, TradeCpi CU: {cu}");

    let taker_state = env.portfolio_state(taker);
    let lp_state = env.portfolio_state(lp);
    assert_eq!(active_leg_for_asset(&taker_state, 0).side, SideV16::Long);
    assert_eq!(active_leg_for_asset(&lp_state, 0).side, SideV16::Short);
    assert_eq!(
        active_leg_for_asset(&taker_state, 0).basis_pos_q,
        (10 * POS_SCALE) as i128
    );
}
