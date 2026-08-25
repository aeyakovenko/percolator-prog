//! INV-009 - partial-fill and retry accounting.
//!
//! CPI matchers may have less capacity than the signed requested size. A single
//! trade may opt into a short fill through `FLAG_PARTIAL_OK`; it must account only
//! the executed quantity and invalidate the consumed position epoch. A batch has
//! no signed per-leg minimum or remaining-allowance ledger, so matcher-selected
//! short fills must reject atomically rather than silently change the strategy's
//! leg ratio. The cross-route matrix executes a real single-CPI half fill, proves
//! every prebuilt single/batch CPI/no-CPI encoding is stale, then executes the
//! exact residual through every route with cumulative quantity, fee, OI, custody,
//! epoch, rollback, and CU assertions. A programmable hostile matcher adds 14
//! signed integral-ratio and 18 non-integral rounding worlds spanning 1/255,
//! midpoint, and 254/255 boundaries while rotating every route class. An
//! independent ceil-notional/ceil-fee oracle bounds two-fill fragmentation to
//! four atoms. Twelve more worlds execute the public maximum-minus-one and
//! maximum admitted quantities in both directions at 1/255, 127/255, and
//! 254/255, retaining the same replay, residual, accounting, and CU oracle.

use super::*;

const FLAGGED_PARTIAL_MODE: u8 = 15;
const ASYMMETRIC_BATCH_PARTIAL_MODE: u8 = 16;

#[derive(Clone, Copy, Debug)]
enum PartialRetryRoute {
    NoCpi,
    BatchNoCpi,
    Cpi,
    BatchCpi,
}

impl PartialRetryRoute {
    const ALL: [Self; 4] = [Self::NoCpi, Self::BatchNoCpi, Self::Cpi, Self::BatchCpi];
}

fn setup_hostile_partial_env_with_deposit(
    asset_count: u16,
    deposit_atoms: u128,
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
    env.deposit(&taker, taker_account, deposit_atoms);
    env.deposit(&lp, lp_account, deposit_atoms);
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
    setup_hostile_partial_env_with_deposit(asset_count, 1_000_000)
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

fn set_hostile_matcher_ratio(
    env: &mut V16CuEnv,
    ctx: Pubkey,
    matcher_program: Pubkey,
    numerator: u8,
) {
    assert!((1..=254).contains(&numerator));
    let mut data = vec![0u8; MATCHER_CONTEXT_LEN];
    data[64] = 19;
    data[65] = numerator;
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

#[allow(clippy::too_many_arguments)]
fn retained_partial_retry_ix(
    env: &V16CuEnv,
    route: PartialRetryRoute,
    taker_account: Pubkey,
    lp_account: Pubkey,
    size_q: i128,
) -> ProgInstruction {
    const ASSET: u16 = 0;
    const PRICE: u64 = 100;
    const FEE_BPS: u64 = 100;
    let market_id = env.asset_market_id(ASSET);
    match route {
        PartialRetryRoute::NoCpi => {
            env.trade_no_cpi_ix(taker_account, lp_account, ASSET, size_q, PRICE, FEE_BPS)
        }
        PartialRetryRoute::BatchNoCpi => env.batch_trade_no_cpi_ix(
            taker_account,
            lp_account,
            vec![BatchTradeLeg {
                asset_index: ASSET,
                market_id,
                size_q,
                exec_price: PRICE,
                fee_bps: FEE_BPS,
            }],
        ),
        PartialRetryRoute::Cpi => {
            env.trade_cpi_ix(taker_account, lp_account, ASSET, size_q, FEE_BPS, 0)
        }
        PartialRetryRoute::BatchCpi => env.batch_trade_cpi_ix(
            taker_account,
            lp_account,
            vec![BatchTradeCpiLeg {
                asset_index: ASSET,
                market_id,
                size_q,
                fee_bps: FEE_BPS,
                limit_price: 0,
            }],
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn send_partial_retry_route(
    env: &mut V16CuEnv,
    route: PartialRetryRoute,
    ix: ProgInstruction,
    taker: &Keypair,
    lp: &Keypair,
    taker_account: Pubkey,
    lp_account: Pubkey,
    matcher: Pubkey,
    ctx: Pubkey,
    delegate: Pubkey,
) -> Result<u64, String> {
    env.svm.expire_blockhash();
    match route {
        PartialRetryRoute::NoCpi | PartialRetryRoute::BatchNoCpi => env.send(
            ix,
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(lp.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker_account, false),
                AccountMeta::new(lp_account, false),
            ],
            &[taker, lp],
        ),
        PartialRetryRoute::Cpi | PartialRetryRoute::BatchCpi => env.send(
            ix,
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker_account, false),
                AccountMeta::new(lp_account, false),
                AccountMeta::new_readonly(matcher, false),
                AccountMeta::new(ctx, false),
                AccountMeta::new_readonly(delegate, false),
            ],
            &[taker],
        ),
    }
}

fn partial_retry_reference_fee(size_q: i128) -> u128 {
    const PRICE: u128 = 100;
    const FEE_BPS: u128 = 100;
    let ceil_div = |numerator: u128, denominator: u128| {
        numerator / denominator + u128::from(numerator % denominator != 0)
    };
    let notional = ceil_div(size_q.unsigned_abs() * PRICE, POS_SCALE as u128);
    2 * ceil_div(notional * FEE_BPS, 10_000)
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

fn run_partial_fill_route_case_with_deposit(
    total_q: i128,
    partial_q: i128,
    ratio_numerator: Option<u8>,
    stale_route: PartialRetryRoute,
    residual_route: PartialRetryRoute,
    deposit_atoms: u128,
) {
    let total_abs_q = total_q.unsigned_abs();
    assert!(partial_q != 0 && partial_q.signum() == total_q.signum());
    assert!(partial_q.unsigned_abs() < total_abs_q);
    let expected_fee_atoms =
        partial_retry_reference_fee(partial_q) + partial_retry_reference_fee(total_q - partial_q);
    let aggregate_fee_atoms = partial_retry_reference_fee(total_q);
    assert!(expected_fee_atoms >= aggregate_fee_atoms);
    assert!(expected_fee_atoms - aggregate_fee_atoms <= 4);

    let (mut env, taker, lp, taker_account, lp_account, matcher, ctx, delegate) =
        setup_hostile_partial_env_with_deposit(1, deposit_atoms);
    let stale_ix = retained_partial_retry_ix(&env, stale_route, taker_account, lp_account, total_q);
    let (_, market_before) = env.market_state();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let mint_before = env.svm.get_account(&env.mint).unwrap();
    let taker_epoch_before = env.portfolio_position_epoch(taker_account);
    let lp_epoch_before = env.portfolio_position_epoch(lp_account);

    if let Some(numerator) = ratio_numerator {
        set_hostile_matcher_ratio(&mut env, ctx, matcher, numerator);
    } else {
        set_hostile_matcher_mode(&mut env, ctx, matcher, FLAGGED_PARTIAL_MODE);
    }
    let partial_ix = retained_partial_retry_ix(
        &env,
        PartialRetryRoute::Cpi,
        taker_account,
        lp_account,
        total_q,
    );
    let partial_cu = send_partial_retry_route(
        &mut env,
        PartialRetryRoute::Cpi,
        partial_ix,
        &taker,
        &lp,
        taker_account,
        lp_account,
        matcher,
        ctx,
        delegate,
    )
    .unwrap_or_else(|error| {
        panic!(
            "{ratio_numerator:?}/{stale_route:?}->{residual_route:?}: partial fill rejected: {error}"
        )
    });
    assert_cu_within("cross-route flagged partial fill", partial_cu, 1_400_000);
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(taker_account), 0).basis_pos_q,
        partial_q
    );
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(lp_account), 0).basis_pos_q,
        -partial_q
    );

    set_hostile_matcher_mode(&mut env, ctx, matcher, 9);
    let market_before_stale = env.svm.get_account(&env.market).unwrap();
    let taker_before_stale = env.svm.get_account(&taker_account).unwrap();
    let lp_before_stale = env.svm.get_account(&lp_account).unwrap();
    let ctx_before_stale = env.svm.get_account(&ctx).unwrap();
    let stale = send_partial_retry_route(
        &mut env,
        stale_route,
        stale_ix,
        &taker,
        &lp,
        taker_account,
        lp_account,
        matcher,
        ctx,
        delegate,
    );
    assert!(
        stale.is_err(),
        "{ratio_numerator:?}/{stale_route:?}->{residual_route:?}: pre-partial intent replayed"
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
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint_before);

    let residual_ix = retained_partial_retry_ix(
        &env,
        residual_route,
        taker_account,
        lp_account,
        total_q - partial_q,
    );
    let residual_cu = send_partial_retry_route(
        &mut env,
        residual_route,
        residual_ix,
        &taker,
        &lp,
        taker_account,
        lp_account,
        matcher,
        ctx,
        delegate,
    )
    .unwrap_or_else(|error| {
        panic!(
            "{ratio_numerator:?}/{stale_route:?}->{residual_route:?}: fresh residual rejected: {error}"
        )
    });
    assert_cu_within("cross-route fresh residual fill", residual_cu, 1_400_000);

    let taker_after = env.portfolio_state(taker_account);
    let lp_after = env.portfolio_state(lp_account);
    assert_eq!(active_leg_for_asset(&taker_after, 0).basis_pos_q, total_q);
    assert_eq!(active_leg_for_asset(&lp_after, 0).basis_pos_q, -total_q);
    assert_eq!(
        env.portfolio_position_epoch(taker_account),
        taker_epoch_before + 2
    );
    assert_eq!(
        env.portfolio_position_epoch(lp_account),
        lp_epoch_before + 2
    );
    let (_, market_after) = env.market_state();
    assert_eq!(market_after.assets[0].oi_eff_long_q, total_abs_q);
    assert_eq!(market_after.assets[0].oi_eff_short_q, total_abs_q);
    assert_eq!(
        market_after.insurance - market_before.insurance,
        expected_fee_atoms
    );
    assert_eq!(market_before.c_tot - market_after.c_tot, expected_fee_atoms);
    assert_eq!(market_after.vault, market_before.vault);
    assert_eq!(
        market_after.c_tot + market_after.insurance,
        market_after.vault
    );
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint_before);
}

fn run_partial_fill_route_case(
    total_q: i128,
    partial_q: i128,
    ratio_numerator: Option<u8>,
    stale_route: PartialRetryRoute,
    residual_route: PartialRetryRoute,
) {
    run_partial_fill_route_case_with_deposit(
        total_q,
        partial_q,
        ratio_numerator,
        stale_route,
        residual_route,
        1_000_000,
    );
}

fn run_partial_fill_route_matrix(total_q: i128, partial_q: i128, ratio_numerator: Option<u8>) {
    for stale_route in PartialRetryRoute::ALL {
        for residual_route in PartialRetryRoute::ALL {
            run_partial_fill_route_case(
                total_q,
                partial_q,
                ratio_numerator,
                stale_route,
                residual_route,
            );
        }
    }
}

#[test]
fn v16_program_partial_fill_invalidates_every_stale_route_and_allows_every_fresh_residual() {
    const TOTAL_Q: i128 = 10 * POS_SCALE as i128;
    run_partial_fill_route_matrix(TOTAL_Q, TOTAL_Q / 2, None);
}

#[test]
fn v16_program_generated_partial_ratios_preserve_every_cross_route_budget() {
    const TOTAL_ABS_Q: i128 = 255 * POS_SCALE as i128;
    for (direction_index, direction) in [-1i128, 1].into_iter().enumerate() {
        for (ratio_index, numerator) in [1u8, 2, 3, 17, 127, 253, 254].into_iter().enumerate() {
            let case_index = direction_index * 7 + ratio_index;
            run_partial_fill_route_case(
                direction * TOTAL_ABS_Q,
                direction * i128::from(numerator) * POS_SCALE as i128,
                Some(numerator),
                PartialRetryRoute::ALL[case_index % PartialRetryRoute::ALL.len()],
                PartialRetryRoute::ALL[(case_index * 3 + 1) % PartialRetryRoute::ALL.len()],
            );
        }
    }
}

#[test]
fn v16_program_nonintegral_partial_ratios_preserve_rounding_and_cross_route_budget() {
    let scale = POS_SCALE as u128;
    for (residue_index, residue) in [1u128, scale / 2, scale - 1].into_iter().enumerate() {
        let total_abs_q = 255 * scale + residue;
        for (direction_index, direction) in [-1i128, 1].into_iter().enumerate() {
            for (ratio_index, numerator) in [1u8, 127, 254].into_iter().enumerate() {
                let numerator_u128 = u128::from(numerator);
                let partial_abs_q = (total_abs_q / 255) * numerator_u128
                    + ((total_abs_q % 255) * numerator_u128) / 255;
                let case_index = residue_index * 6 + direction_index * 3 + ratio_index;
                run_partial_fill_route_case(
                    direction * total_abs_q as i128,
                    direction * partial_abs_q as i128,
                    Some(numerator),
                    PartialRetryRoute::ALL[case_index % PartialRetryRoute::ALL.len()],
                    PartialRetryRoute::ALL[(case_index * 3 + 1) % PartialRetryRoute::ALL.len()],
                );
            }
        }
    }
}

#[test]
fn v16_program_public_max_quantity_partial_fills_preserve_exact_cumulative_budget() {
    const MAX_SHAPE_DEPOSIT_ATOMS: u128 = 20_000_000_000;
    let public_max_q = percolator::MAX_TRADE_SIZE_Q;
    for (width_index, total_abs_q) in [public_max_q - 1, public_max_q].into_iter().enumerate() {
        for (direction_index, direction) in [-1i128, 1].into_iter().enumerate() {
            for (ratio_index, numerator) in [1u8, 127, 254].into_iter().enumerate() {
                let numerator_u128 = u128::from(numerator);
                let partial_abs_q = (total_abs_q / 255) * numerator_u128
                    + ((total_abs_q % 255) * numerator_u128) / 255;
                assert!(partial_abs_q > 0 && partial_abs_q < total_abs_q);
                let case_index = width_index * 6 + direction_index * 3 + ratio_index;
                run_partial_fill_route_case_with_deposit(
                    direction * i128::try_from(total_abs_q).expect("public max quantity fits i128"),
                    direction
                        * i128::try_from(partial_abs_q)
                            .expect("matcher-selected public quantity fits i128"),
                    Some(numerator),
                    PartialRetryRoute::ALL[case_index % PartialRetryRoute::ALL.len()],
                    PartialRetryRoute::ALL[(case_index * 3 + 1) % PartialRetryRoute::ALL.len()],
                    MAX_SHAPE_DEPOSIT_ATOMS,
                );
            }
        }
    }
}
fn run_flagged_partial_partition(total_units: u128, partial_rounds: usize) {
    let (mut env, taker, _lp, taker_account, lp_account, matcher, ctx, delegate) =
        setup_hostile_partial_env(1);
    let total_q = (total_units * POS_SCALE) as i128;
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

    let (_, market_before) = env.market_state();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let taker_epoch_before = env.portfolio_position_epoch(taker_account);
    let lp_epoch_before = env.portfolio_position_epoch(lp_account);
    let mut remaining_q = total_q;
    let mut cumulative_q = 0i128;

    for round in 0..partial_rounds {
        assert_eq!(
            remaining_q % 2,
            0,
            "partition fixture must request an exactly divisible quantity"
        );
        set_hostile_matcher_mode(&mut env, ctx, matcher, FLAGGED_PARTIAL_MODE);
        let stale_request = env.trade_cpi_ix(taker_account, lp_account, 0, remaining_q, 100, 0);
        env.svm.expire_blockhash();
        let cu = env
            .send(stale_request.clone(), accounts(&env), &[&taker])
            .expect("each flagged partial request must execute");
        assert_cu_within("TradeCpi repeated flagged partial fill", cu, 1_400_000);

        let executed_q = remaining_q / 2;
        cumulative_q += executed_q;
        remaining_q -= executed_q;
        let accepted_steps = (round + 1) as u64;
        let taker_state = env.portfolio_state(taker_account);
        let lp_state = env.portfolio_state(lp_account);
        assert_eq!(
            active_leg_for_asset(&taker_state, 0).basis_pos_q,
            cumulative_q
        );
        assert_eq!(
            active_leg_for_asset(&lp_state, 0).basis_pos_q,
            -cumulative_q
        );
        assert_eq!(
            env.portfolio_position_epoch(taker_account),
            taker_epoch_before + accepted_steps
        );
        assert_eq!(
            env.portfolio_position_epoch(lp_account),
            lp_epoch_before + accepted_steps
        );

        let (_, market_after_partial) = env.market_state();
        let cumulative_fee_atoms = 2 * (cumulative_q as u128 / POS_SCALE);
        assert_eq!(
            market_after_partial.assets[0].oi_eff_long_q,
            cumulative_q as u128
        );
        assert_eq!(
            market_after_partial.assets[0].oi_eff_short_q,
            cumulative_q as u128
        );
        assert_eq!(
            market_after_partial.insurance - market_before.insurance,
            cumulative_fee_atoms
        );
        assert_eq!(
            market_before.c_tot - market_after_partial.c_tot,
            cumulative_fee_atoms
        );
        assert_eq!(market_after_partial.vault, market_before.vault);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

        let signer_before_stale = env.svm.get_account(&taker.pubkey()).unwrap();
        let market_before_stale = env.svm.get_account(&env.market).unwrap();
        let taker_before_stale = env.svm.get_account(&taker_account).unwrap();
        let lp_before_stale = env.svm.get_account(&lp_account).unwrap();
        let ctx_before_stale = env.svm.get_account(&ctx).unwrap();
        env.svm.expire_blockhash();
        let stale = env.send(stale_request, accounts(&env), &[&taker]);
        assert!(
            stale.is_err(),
            "round {round}: a consumed partial-fill request must not replay"
        );
        assert_eq!(
            env.svm.get_account(&taker.pubkey()).unwrap(),
            signer_before_stale
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
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    }

    set_hostile_matcher_mode(&mut env, ctx, matcher, 9);
    env.svm.expire_blockhash();
    let final_cu = env
        .send(
            env.trade_cpi_ix(taker_account, lp_account, 0, remaining_q, 100, 0),
            accounts(&env),
            &[&taker],
        )
        .expect("a fresh final request must execute the exact residual");
    assert_cu_within(
        "TradeCpi full fill after repeated partials",
        final_cu,
        1_400_000,
    );

    let taker_after = env.portfolio_state(taker_account);
    let lp_after = env.portfolio_state(lp_account);
    assert_eq!(active_leg_for_asset(&taker_after, 0).basis_pos_q, total_q);
    assert_eq!(active_leg_for_asset(&lp_after, 0).basis_pos_q, -total_q);
    assert_eq!(
        env.portfolio_position_epoch(taker_account),
        taker_epoch_before + partial_rounds as u64 + 1
    );
    assert_eq!(
        env.portfolio_position_epoch(lp_account),
        lp_epoch_before + partial_rounds as u64 + 1
    );
    let (_, market_after) = env.market_state();
    let aggregate_fee_atoms = 2 * total_units;
    assert_eq!(market_after.assets[0].oi_eff_long_q, total_q as u128);
    assert_eq!(market_after.assets[0].oi_eff_short_q, total_q as u128);
    assert_eq!(
        market_after.insurance - market_before.insurance,
        aggregate_fee_atoms
    );
    assert_eq!(
        market_before.c_tot - market_after.c_tot,
        aggregate_fee_atoms
    );
    assert_eq!(
        market_after.c_tot + market_after.insurance,
        market_after.vault
    );
    assert_eq!(market_after.vault, market_before.vault);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
}

#[test]
fn v16_program_tradecpi_partial_partition_matrix_preserves_cumulative_budget() {
    for (total_units, max_partial_rounds) in [(8u128, 3usize), (16, 4), (32, 5)] {
        for partial_rounds in 1..=max_partial_rounds {
            run_flagged_partial_partition(total_units, partial_rounds);
        }
    }
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
