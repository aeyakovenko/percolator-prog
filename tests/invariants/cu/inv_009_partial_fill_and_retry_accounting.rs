//! INV-009 - partial-fill and retry accounting.
//!
//! CPI matchers may have less capacity than the signed requested size. A single
//! trade may opt into a short fill through `FLAG_PARTIAL_OK`; it must account only
//! the executed quantity and consume the entire one-shot authorization by
//! advancing both position episodes. There is deliberately no persistent residual:
//! any residual fill is a newly signed intent against the new episodes. A batch
//! rejects matcher-selected short fills atomically rather than silently changing
//! the strategy's signed leg ratio. The cross-route matrix executes a real
//! single-CPI half fill, proves
//! every prebuilt single/batch CPI/no-CPI encoding is stale, then executes the
//! exact residual through every route with cumulative quantity, fee, OI, custody,
//! epoch, rollback, and CU assertions. A programmable hostile matcher adds 14
//! signed integral-ratio and 18 non-integral rounding worlds spanning 1/255,
//! midpoint, and 254/255 boundaries while rotating every route class. An
//! independent ceil-notional/ceil-fee oracle bounds two-fill fragmentation to
//! four atoms. Twelve more worlds execute the public maximum-minus-one and
//! maximum admitted quantities in both directions at 1/255, 127/255, and
//! 254/255, retaining the same replay, residual, accounting, and CU oracle.
//! A bounded schedule product inserts repeated failures at every subset of three
//! fill prefixes, retries unchanged unconsumed instructions, and delays consumed
//! instruction replays across multiple partial-fill episodes.
//! A separate seeded, shrinkable history composes variable partial partitions and
//! signed-bound rejections with all four freshly signed residual transports. Its
//! input-derived budget and decoded-fill ledger include adverse residual prices;
//! public matcher controls and every accepted/rejected prefix retain the ledger.

use super::*;

#[path = "inv_009_retained_partial_words.rs"]
mod retained_partial_words;

const FLAGGED_PARTIAL_MODE: u8 = 15;
const ASYMMETRIC_BATCH_PARTIAL_MODE: u8 = 16;

fn inv009_source_block<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    source
        .split_once(start)
        .and_then(|(_, tail)| tail.split_once(end))
        .map(|(body, _)| body)
        .unwrap_or_else(|| panic!("missing production source block {start:?}..{end:?}"))
}

fn inv009_variant_body<'a>(instruction_enum: &'a str, variant: &str) -> &'a str {
    let start = instruction_enum
        .find(&format!("{variant} {{"))
        .unwrap_or_else(|| panic!("missing instruction variant {variant}"));
    let open = start
        + instruction_enum[start..]
            .find('{')
            .expect("instruction variant opening brace");
    let mut depth = 0usize;
    for (offset, byte) in instruction_enum.as_bytes()[open..]
        .iter()
        .copied()
        .enumerate()
    {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &instruction_enum[open + 1..open + offset];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated instruction variant {variant}");
}

#[test]
fn v16_program_one_shot_trade_consent_composition_is_source_complete() {
    assert_certified_engine_pin("INV-008/009/011/059 one-shot trade consent");
    let production = include_str!("../../../src/v16_program.rs");
    let instruction_enum =
        inv009_source_block(production, "pub enum Instruction", "impl Instruction");

    for variant in ["TradeNoCpi", "TradeCpi", "BatchTradeNoCpi", "BatchTradeCpi"] {
        let body = inv009_variant_body(instruction_enum, variant);
        for binding in [
            "account_a_portfolio_id: u64",
            "account_a_position_epoch: u64",
            "account_b_portfolio_id: u64",
            "account_b_position_epoch: u64",
        ] {
            assert!(body.contains(binding), "{variant} omits {binding}");
        }
    }
    let batch_cpi = inv009_variant_body(instruction_enum, "BatchTradeCpi");
    assert!(batch_cpi.contains("max_slippage_atoms: u128"));
    assert!(batch_cpi.contains("max_fee_atoms: u128"));

    let matcher_validator = inv009_source_block(
        production,
        "pub fn validate_matcher_return(",
        "pub fn validate_atomic_batch_matcher_return(",
    );
    for guard in [
        "ret.exec_size.signum() != req_size.signum()",
        "ret.exec_size.unsigned_abs() > req_size.unsigned_abs()",
        "(ret.flags & FLAG_PARTIAL_OK) == 0",
    ] {
        assert!(
            matcher_validator.contains(guard),
            "single CPI omits {guard}"
        );
    }
    let atomic_validator = inv009_source_block(
        production,
        "pub fn validate_atomic_batch_matcher_return(",
        "pub mod oracle_v16",
    );
    assert!(atomic_validator.contains("if ret.exec_size != req_size"));

    let single_cpi = inv009_source_block(
        production,
        "fn handle_trade_cpi<'a>(",
        "fn handle_set_matcher_config<'a>(",
    );
    assert!(single_cpi.contains("matcher_abi::validate_matcher_return("));
    assert!(single_cpi.contains("if ret.exec_size == 0"));
    assert!(single_cpi.contains("handle_trade_nocpi_zero_copy("));

    let batch_cpi_handler = inv009_source_block(
        production,
        "fn handle_batch_trade_cpi<'a>(",
        "fn handle_close_portfolio<'a>(",
    );
    assert!(batch_cpi_handler.contains("matcher_abi::validate_atomic_batch_matcher_return("));
    assert!(batch_cpi_handler.contains("policy_v16::accumulate_with_cap("));
    assert!(batch_cpi_handler.contains("Some(max_fee_atoms)"));

    for (start, end) in [
        (
            "fn handle_trade_nocpi_zero_copy<'a>(",
            "fn portfolio_position_vector_view(",
        ),
        (
            "fn handle_batch_execute_zero_copy<'a>(",
            "fn handle_trade_nocpi<'a>(",
        ),
    ] {
        let executor = inv009_source_block(production, start, end);
        assert!(executor.contains("state::bump_portfolio_position_epoch(&mut account_a_data)?"));
        assert!(executor.contains(
            "state::bump_portfolio_position_epoch_after_matcher_fill(&mut account_b_data)?"
        ));
        assert!(executor.contains("state::bump_portfolio_position_epoch(&mut account_b_data)?"));
    }

    let transaction_envelope =
        include_str!("../public_sbf/inv_006_program_chain_message_type_and_version_binding.rs");
    assert!(transaction_envelope
        .contains("fn retained_transaction_binds_program_market_kind_schema_and_blockhash("));
    assert!(
        transaction_envelope.contains("fn deployed_wrapper_has_no_detached_signature_interpreter(")
    );

    let episode_proof = include_str!("../kani/inv_004_position_episode_binding.rs");
    assert!(episode_proof
        .contains("fn kani_v16_successful_episode_consumption_invalidates_the_old_binding("));
    let partial_proof = include_str!("../kani/inv_009_partial_fill_and_retry_accounting.rs");
    assert!(
        partial_proof.contains("fn kani_v16_atomic_batch_accepts_only_exact_bound_matcher_fill(")
    );
    let aggregate_owner = include_str!("inv_011_signed_aggregate_economic_bounds.rs");
    assert!(aggregate_owner.contains(
        "fn v16_program_batch_cpi_aggregate_quote_caps_abort_matcher_and_wrapper_atomically("
    ));
}

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

fn run_partial_failure_retry_schedule(
    direction: i128,
    stale_route: PartialRetryRoute,
    failure_mask: u8,
) -> u64 {
    let (mut env, taker, lp, taker_account, lp_account, matcher, ctx, delegate) =
        setup_hostile_partial_env(1);
    let total_q = direction * (255 * POS_SCALE + POS_SCALE / 2 + 1) as i128;
    let case = format!("direction={direction}, stale={stale_route:?}, failures={failure_mask:03b}");
    let (_, initial_market) = env.market_state();
    let initial_epochs = [
        env.portfolio_position_epoch(taker_account),
        env.portfolio_position_epoch(lp_account),
    ];
    let initial_custody = [env.vault, env.mint].map(|key| env.svm.get_account(&key).unwrap());
    // Include every trade writable plus custody and the delegate; only the SVM fee payer is excluded.
    let frame = |env: &V16CuEnv| {
        [
            env.market,
            taker_account,
            lp_account,
            ctx,
            env.vault,
            env.mint,
            taker.pubkey(),
            lp.pubkey(),
            delegate,
        ]
        .map(|key| env.svm.get_account(&key).unwrap())
    };
    let send = |env: &mut V16CuEnv, route, ix| {
        send_partial_retry_route(
            env,
            route,
            ix,
            &taker,
            &lp,
            taker_account,
            lp_account,
            matcher,
            ctx,
            delegate,
        )
    };
    let assert_prefix = |env: &V16CuEnv, quantity: i128, fees: u128, fills: u64| {
        let context = format!("{case}, accepted={fills}");
        for (key, epoch, signed_q) in [
            (taker_account, initial_epochs[0], quantity),
            (lp_account, initial_epochs[1], -quantity),
        ] {
            let account = env.portfolio_state(key);
            if signed_q == 0 {
                assert!(!has_active_leg_for_asset(&account, 0), "{context}");
            } else {
                assert_eq!(
                    active_leg_for_asset(&account, 0).basis_pos_q,
                    signed_q,
                    "{context}"
                );
            }
            assert_eq!(account.capital.get(), 1_000_000 - fees / 2, "{context}");
            assert_eq!(
                env.portfolio_position_epoch(key),
                epoch + fills,
                "{context}"
            );
        }
        let (_, market) = env.market_state();
        assert_eq!(
            market.assets[0].oi_eff_long_q,
            quantity.unsigned_abs(),
            "{context}"
        );
        assert_eq!(
            market.assets[0].oi_eff_short_q,
            quantity.unsigned_abs(),
            "{context}"
        );
        assert_eq!(
            market.insurance,
            initial_market.insurance + fees,
            "{context}"
        );
        assert_eq!(market.c_tot + fees, initial_market.c_tot, "{context}");
        assert_eq!(market.vault, initial_market.vault, "{context}");
        assert_eq!(market.c_tot + market.insurance, market.vault, "{context}");
        assert_eq!(
            market.vault,
            u128::from(env.token_amount(env.vault)),
            "{context}"
        );
        assert_eq!(
            [env.vault, env.mint].map(|key| env.svm.get_account(&key).unwrap()),
            initial_custody,
            "{context}"
        );
        let unsplit_fee = partial_retry_reference_fee(quantity);
        assert!(fees >= unsplit_fee, "{context}");
        assert!(
            fees - unsplit_fee <= 4 * u128::from(fills.saturating_sub(1)),
            "{context}"
        );
    };

    let mut quantity = 0i128;
    let mut fees = 0u128;
    let mut consumed = Vec::new();
    let mut max_cu = 0;
    assert_prefix(&env, quantity, fees, 0);
    for (step, numerator) in [Some(127u8), Some(254u8), None].into_iter().enumerate() {
        let remaining = total_q - quantity;
        let route = if numerator.is_some() {
            PartialRetryRoute::Cpi
        } else {
            PartialRetryRoute::BatchCpi
        };
        let current = retained_partial_retry_ix(&env, route, taker_account, lp_account, remaining);
        consumed.push(retained_partial_retry_ix(
            &env,
            stale_route,
            taker_account,
            lp_account,
            remaining,
        ));

        if failure_mask & (1 << step) != 0 {
            if numerator.is_some() {
                set_hostile_matcher_mode(&mut env, ctx, matcher, 7); // Unflagged single short fill.
            } else {
                set_hostile_matcher_ratio(&mut env, ctx, matcher, 127); // Flagged batch short fill.
            }
            for attempt in 0..2 {
                let before = frame(&env);
                assert!(
                    send(&mut env, route, current.clone()).is_err(),
                    "{case}, step={step}, attempt={attempt}: short fill must reject"
                );
                assert_eq!(
                    frame(&env),
                    before,
                    "{case}, step={step}, attempt={attempt}"
                );
                assert_prefix(&env, quantity, fees, step as u64);
            }
        }

        let executed = if let Some(numerator) = numerator {
            set_hostile_matcher_ratio(&mut env, ctx, matcher, numerator);
            // This bounded input fits a direct product, independently of the matcher's div/rem split.
            let magnitude = remaining.unsigned_abs() * u128::from(numerator) / 255;
            assert!(
                magnitude > 0 && magnitude < remaining.unsigned_abs(),
                "{case}"
            );
            assert_ne!(
                magnitude % POS_SCALE,
                0,
                "{case}: exercise nonintegral partials"
            );
            direction * magnitude as i128
        } else {
            set_hostile_matcher_mode(&mut env, ctx, matcher, 9);
            remaining
        };
        // A failure consumes no consent: retry the same instruction, changing only matcher capacity
        // or its partial flag and the transaction's blockhash/signature, never rebinding its epochs.
        let cu = send(&mut env, route, current)
            .unwrap_or_else(|error| panic!("{case}, step={step}: current retry rejected: {error}"));
        assert_cu_within("bounded partial/failure/retry schedule", cu, 1_400_000);
        max_cu = max_cu.max(cu);
        quantity += executed;
        fees += partial_retry_reference_fee(executed);
        assert_prefix(&env, quantity, fees, step as u64 + 1);

        // Full capacity removes short-fill rejection as an alternative reason for a stale error.
        set_hostile_matcher_mode(&mut env, ctx, matcher, 9);
        for (old_step, stale) in consumed.iter().enumerate() {
            let before = frame(&env);
            assert!(
                send(&mut env, stale_route, stale.clone()).is_err(),
                "{case}, step={step}: consumed instruction from step {old_step} replayed"
            );
            assert_eq!(
                frame(&env),
                before,
                "{case}, step={step}, old_step={old_step}"
            );
            assert_prefix(&env, quantity, fees, step as u64 + 1);
        }
    }
    assert_eq!(
        quantity, total_q,
        "{case}: bounded fresh residual must finish"
    );
    max_cu
}

#[test]
fn v16_program_bounded_partial_failure_retry_schedules_preserve_every_prefix() {
    let mut histories = 0;
    let mut max_cu = 0;
    for direction in [-1i128, 1] {
        for stale_route in PartialRetryRoute::ALL {
            for failure_mask in 0u8..8 {
                max_cu = max_cu.max(run_partial_failure_retry_schedule(
                    direction,
                    stale_route,
                    failure_mask,
                ));
                histories += 1;
            }
        }
    }
    assert_eq!(histories, 64);
    println!(
        "INV-009: {histories} bounded histories; 192 fills; 192 short-fill rejections; \
         384 stale rejections; max fill CU={max_cu}"
    );
}

#[derive(Clone, Debug)]
struct PartialBudgetHistory {
    total_q: i128,
    partials: Vec<(u8, u8)>, // Ratio numerator / 255, rejected price-limit attempts.
    residual_route: usize,
    residual_rejections: u8,
}

fn run_partial_budget_history(history: &PartialBudgetHistory) -> [u64; 8] {
    use percolator_prog::matcher_abi::{
        read_matcher_return, FLAG_PARTIAL_OK, MATCHER_RETURN_BYTES,
    };
    use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

    let (mut env, taker, lp, ta, la, matcher, ctx, delegate) = setup_hostile_partial_env(1);
    let residual_matcher = Pubkey::new_unique();
    env.svm.add_program(
        residual_matcher,
        &std::fs::read(matcher_program_path()).expect("read passive matcher SBF"),
    );
    let (residual_ctx, residual_delegate, _) =
        env.init_matcher_context_with_passive_spread(residual_matcher, la, 500, 500);
    let control = |env: &mut V16CuEnv, data: Vec<u8>| {
        env.svm.expire_blockhash();
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            Instruction {
                program_id: matcher,
                accounts: vec![
                    AccountMeta::new_readonly(lp.pubkey(), true),
                    AccountMeta::new(ctx, false),
                ],
                data,
            },
            &[&lp],
        )
        .expect("public partial matcher configuration");
    };
    control(&mut env, vec![10]);

    let route = PartialRetryRoute::ALL[history.residual_route];
    let direction = history.total_q.signum();
    let residual_price = if direction > 0 { 105 } else { 95 };
    let ceil = |n: u128, d: u128| n / d + u128::from(n % d != 0);
    // Quote and slippage describe matcher execution, not an SPL transfer or a moving manual mark.
    // Fee basis remains the authenticated mark (100), including the adverse-price residual.
    let economics = |q: i128, price: u64| {
        let magnitude = q.unsigned_abs();
        [
            if q > 0 {
                ceil(magnitude * u128::from(price), POS_SCALE)
            } else {
                magnitude * u128::from(price) / POS_SCALE
            },
            partial_retry_reference_fee(q) / 2,
            ceil(magnitude * u128::from(price.abs_diff(100)), POS_SCALE),
        ]
    };
    let mut remaining = history.total_q;
    let mut plan = Vec::new();
    for &(numerator, _) in &history.partials {
        let filled = direction * (remaining.unsigned_abs() * u128::from(numerator) / 255) as i128;
        assert!(filled != 0 && filled.unsigned_abs() < remaining.unsigned_abs());
        plan.push((filled, 100));
        remaining -= filled;
    }
    plan.push((remaining, residual_price));
    let planned_prefix = |fills: usize| {
        let mut quantity = 0;
        let mut totals = [0u128; 3];
        for &(q, price) in &plan[..fills] {
            quantity += q;
            for (sum, amount) in totals.iter_mut().zip(economics(q, price)) {
                *sum += amount;
            }
        }
        (quantity, totals)
    };
    let (_, original_budget) = planned_prefix(plan.len());
    let initial_epochs = [ta, la].map(|key| env.portfolio_position_epoch(key));
    let initial_market = env.market_state().1;
    let custody =
        |env: &V16CuEnv| [env.vault, env.mint].map(|key| env.svm.get_account(&key).unwrap());
    let initial_custody = custody(&env);
    let frame = |env: &V16CuEnv| {
        // All trade/control writables and passive custody, including economic lamports.
        // The network-fee payer alone is excluded; no token transfers occur after setup.
        [
            env.market,
            ta,
            la,
            ctx,
            residual_ctx,
            taker.pubkey(),
            lp.pubkey(),
            delegate,
            residual_delegate,
            env.vault,
            env.mint,
            matcher,
            residual_matcher,
        ]
        .map(|key| env.svm.get_account(&key).unwrap())
    };
    let assert_prefix = |env: &V16CuEnv, fills: usize, quantity: i128, observed: [u128; 3]| {
        let (expected_q, expected) = planned_prefix(fills);
        assert_eq!(
            (quantity, observed),
            (expected_q, expected),
            "{history:?}, prefix={fills}"
        );
        assert!(quantity.unsigned_abs() <= history.total_q.unsigned_abs());
        assert!(observed[1] <= original_budget[1] && observed[2] <= original_budget[2]);
        if direction > 0 {
            assert!(
                observed[0] <= original_budget[0],
                "cumulative buy-quote ceiling"
            );
        } else {
            assert!(observed[0] >= expected[0], "executed sell-quote floor");
        }
        for (index, key) in [ta, la].into_iter().enumerate() {
            let account = env.portfolio_state(key);
            assert_eq!(
                account.capital.get(),
                1_000_000 - observed[1],
                "{history:?}"
            );
            assert_eq!(
                account.pnl.get(),
                0,
                "manual mark and funding must remain fixed"
            );
            assert_eq!(
                env.portfolio_position_epoch(key),
                initial_epochs[index] + fills as u64
            );
            if fills == 0 {
                assert!(!has_active_leg_for_asset(&account, 0));
            } else {
                assert_eq!(
                    active_leg_for_asset(&account, 0).basis_pos_q,
                    if index == 0 { quantity } else { -quantity }
                );
            }
        }
        let market = env.market_state().1;
        assert_eq!(market.assets[0].effective_price, 100);
        assert_eq!(market.assets[0].oi_eff_long_q, quantity.unsigned_abs());
        assert_eq!(market.assets[0].oi_eff_short_q, quantity.unsigned_abs());
        assert_eq!(market.insurance, initial_market.insurance + 2 * observed[1]);
        assert_eq!(market.c_tot, initial_market.c_tot - 2 * observed[1]);
        assert_eq!(market.vault, initial_market.vault);
        assert_eq!(market.c_tot + market.insurance, market.vault);
        assert_eq!(market.vault, u128::from(env.token_amount(env.vault)));
        assert_eq!(
            custody(env),
            initial_custody,
            "{history:?}: exact SPL custody"
        );
        let unsplit_fee = partial_retry_reference_fee(quantity) / 2;
        assert!(observed[1] >= unsplit_fee);
        assert!(observed[1] - unsplit_fee <= 2 * fills.saturating_sub(1) as u128);
    };
    let send = |env: &mut V16CuEnv, transport, ix: ProgInstruction, residual: bool| {
        let (program, context, signer) = if residual {
            (residual_matcher, residual_ctx, residual_delegate)
        } else {
            (matcher, ctx, delegate)
        };
        let cpi = matches!(
            transport,
            PartialRetryRoute::Cpi | PartialRetryRoute::BatchCpi
        );
        let accounts = if cpi {
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(ta, false),
                AccountMeta::new(la, false),
                AccountMeta::new_readonly(program, false),
                AccountMeta::new(context, false),
                AccountMeta::new_readonly(signer, false),
            ]
        } else {
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(lp.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(ta, false),
                AccountMeta::new(la, false),
            ]
        };
        let mut signers = vec![&env.payer, &taker];
        if !cpi {
            signers.push(&lp);
        }
        env.svm.expire_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[
                heap_ix(),
                cu_ix(),
                Instruction {
                    program_id: env.program_id,
                    accounts,
                    data: ix.encode(),
                },
            ],
            Some(&env.payer.pubkey()),
            &signers,
            env.svm.latest_blockhash(),
        );
        env.svm.send_transaction(tx)
    };
    let assert_error = |error: &litesvm::types::FailedTransactionMetadata,
                        expected: PercolatorError| {
        assert_eq!(
            error.err,
            TransactionError::InstructionError(2, InstructionError::Custom(expected as u32)),
            "{history:?}: require the wrapper error, not a transaction-cache or CU rejection"
        );
    };

    // Fills, bound rejects, consumed rejects, public controls, integral fills, residue fills,
    // sub-unit residuals, maximum transaction CU. Never reset these or the economic ledger on error.
    let mut evidence = [0u64; 8];
    let mut quantity = 0;
    let mut observed = [0u128; 3];
    let mut retained_first = None;
    assert_prefix(&env, 0, quantity, observed);
    for step in 0..plan.len() {
        let residual = step == history.partials.len();
        let transport = if residual {
            route
        } else {
            PartialRetryRoute::Cpi
        };
        if residual {
            env.set_matcher_config(
                residual_matcher,
                &lp,
                la,
                residual_ctx,
                residual_delegate,
                1,
            );
        } else {
            let before = frame(&env);
            control(&mut env, vec![11, 19, history.partials[step].0]);
            let mut expected = before;
            expected[3].data[64] = 19;
            expected[3].data[65] = history.partials[step].0;
            assert_eq!(
                frame(&env),
                expected,
                "configuration may only change matcher capacity"
            );
        }
        evidence[3] += 1;
        assert_prefix(&env, step, quantity, observed);

        let requested = history.total_q - quantity;
        let price = plan[step].1;
        let mut current = retained_partial_retry_ix(&env, transport, ta, la, requested);
        match &mut current {
            ProgInstruction::TradeCpi { limit_price, .. } => *limit_price = price,
            ProgInstruction::BatchTradeCpi {
                legs,
                max_fee_atoms,
                max_slippage_atoms,
                ..
            } => {
                legs[0].limit_price = price;
                *max_fee_atoms = original_budget[1] - observed[1];
                *max_slippage_atoms = original_budget[2] - observed[2];
                assert_eq!(
                    [*max_fee_atoms, *max_slippage_atoms],
                    [
                        economics(requested, price)[1],
                        economics(requested, price)[2]
                    ]
                );
            }
            ProgInstruction::TradeNoCpi { exec_price, .. } => *exec_price = price,
            ProgInstruction::BatchTradeNoCpi { legs, .. } => legs[0].exec_price = price,
            _ => unreachable!(),
        }
        let attempts = if residual {
            history.residual_rejections
        } else {
            history.partials[step].1
        };
        for attempt in 0..attempts {
            let mut rejected = current.clone();
            match &mut rejected {
                ProgInstruction::TradeCpi { limit_price, .. } => {
                    *limit_price = if direction > 0 { price - 1 } else { price + 1 }
                }
                ProgInstruction::BatchTradeCpi {
                    max_fee_atoms,
                    max_slippage_atoms,
                    ..
                } => {
                    if attempt % 2 == 0 {
                        *max_fee_atoms -= 1;
                    } else {
                        *max_slippage_atoms -= 1;
                    }
                }
                ProgInstruction::TradeNoCpi { fee_bps, .. } => *fee_bps = 99,
                ProgInstruction::BatchTradeNoCpi { legs, .. } => legs[0].fee_bps = 99,
                _ => unreachable!(),
            }
            let before = frame(&env);
            let error = send(&mut env, transport, rejected, residual)
                .expect_err("signed bound must reject");
            assert_error(&error, PercolatorError::InvalidInstruction);
            if matches!(
                transport,
                PartialRetryRoute::Cpi | PartialRetryRoute::BatchCpi
            ) {
                let program = if residual { residual_matcher } else { matcher };
                assert!(
                    error
                        .meta
                        .logs
                        .iter()
                        .any(|line| line == &format!("Program {program} success")),
                    "the matcher must execute before signed-bound rejection"
                );
            }
            assert_eq!(
                frame(&env),
                before,
                "{history:?}, step={step}, reject={attempt}"
            );
            assert_prefix(&env, step, quantity, observed);
            evidence[1] += 1;
            evidence[7] = evidence[7].max(error.meta.compute_units_consumed);
        }

        retained_first.get_or_insert_with(|| current.clone());
        let success =
            send(&mut env, transport, current.clone(), residual).unwrap_or_else(|error| {
                panic!("{history:?}, step={step}: fresh fill rejected: {error:?}")
            });
        let (filled, paid_price) = match transport {
            PartialRetryRoute::Cpi | PartialRetryRoute::BatchCpi => {
                let ret = if matches!(transport, PartialRetryRoute::BatchCpi) {
                    assert_eq!(success.return_data.program_id, residual_matcher);
                    assert_eq!(success.return_data.data.len(), MATCHER_RETURN_BYTES);
                    read_matcher_return(&success.return_data.data).unwrap()
                } else {
                    let context = if residual { residual_ctx } else { ctx };
                    read_matcher_return(&env.svm.get_account(&context).unwrap().data).unwrap()
                };
                assert_eq!(ret.asset_index, 0);
                assert_eq!(ret.oracle_price_e6, 100);
                if !residual {
                    assert_ne!(ret.flags & FLAG_PARTIAL_OK, 0);
                }
                (ret.exec_size, ret.exec_price_e6)
            }
            _ => (requested, price),
        };
        assert_eq!(
            (filled, paid_price),
            plan[step],
            "{history:?}: decoded fill must match independent partition"
        );
        quantity += filled;
        for (sum, amount) in observed.iter_mut().zip(economics(filled, paid_price)) {
            *sum += amount;
        }
        assert_prefix(&env, step + 1, quantity, observed);
        evidence[0] += 1;
        evidence[if filled.unsigned_abs() % POS_SCALE == 0 {
            4
        } else {
            5
        }] += 1;
        evidence[6] += u64::from(residual && filled.unsigned_abs() < POS_SCALE);
        evidence[7] = evidence[7].max(success.compute_units_consumed);

        // The original first partial is delayed across later partials. After the grant switch,
        // retry only the just-consumed residual, so an old grant cannot supply the rejection.
        let stale = if residual {
            current
        } else {
            retained_first.as_ref().unwrap().clone()
        };
        let before = frame(&env);
        let error =
            send(&mut env, transport, stale, residual).expect_err("consumed consent must reject");
        assert_error(&error, PercolatorError::EngineStale);
        assert_eq!(
            frame(&env),
            before,
            "{history:?}: consumed consent rollback"
        );
        assert_prefix(&env, step + 1, quantity, observed);
        evidence[2] += 1;
        evidence[7] = evidence[7].max(error.meta.compute_units_consumed);
    }
    assert_eq!((quantity, observed), (history.total_q, original_budget));
    assert!(
        observed[2] > 0,
        "residual must exercise a nonzero slippage budget"
    );
    assert_cu_within("generated partial/residual history", evidence[7], 1_375_000);
    evidence
}

#[test]
fn v16_program_generated_partial_residual_histories_preserve_signed_budget() {
    use proptest::{
        prelude::*,
        test_runner::{Config, RngAlgorithm, TestRng, TestRunner},
    };

    let counts = std::cell::Cell::new([0u64; 8]);
    let record = |history: PartialBudgetHistory| {
        let evidence = run_partial_budget_history(&history);
        let mut total = counts.get();
        for index in 0..7 {
            total[index] += evidence[index];
        }
        total[7] = total[7].max(evidence[7]);
        counts.set(total);
    };
    // Pin all sign / integral-neighbor / residual-transport cells; the random tail shrinks the
    // partition length, each ratio, rejection placement/count, quantity and residual transport.
    for direction in [-1i128, 1] {
        for (boundary, offset) in [-1i128, 0, 1].into_iter().enumerate() {
            for residual_route in 0..4 {
                record(PartialBudgetHistory {
                    total_q: direction * (255 * POS_SCALE as i128 + offset),
                    partials: [1, 127, 254]
                        .into_iter()
                        .take(1 + (boundary + residual_route) % 3)
                        .enumerate()
                        .map(|(step, ratio)| (ratio, ((step + residual_route) % 3) as u8))
                        .collect(),
                    residual_route,
                    residual_rejections: 2,
                });
            }
        }
    }
    let strategy = (
        255u128..=1024,
        prop::sample::select(vec![0, 1, POS_SCALE / 2, POS_SCALE - 1]),
        any::<bool>(),
        prop::collection::vec((1u8..=254, 0u8..=2), 1..=3),
        0usize..4,
        0u8..=2,
    )
        .prop_map(
            |(units, residue, negative, partials, residual_route, residual_rejections)| {
                PartialBudgetHistory {
                    total_q: (units * POS_SCALE + residue) as i128 * if negative { -1 } else { 1 },
                    partials,
                    residual_route,
                    residual_rejections,
                }
            },
        );
    let mut runner = TestRunner::new_with_rng(
        Config {
            cases: 32,
            max_shrink_iters: 128,
            failure_persistence: Some(Box::new(
                proptest::test_runner::FileFailurePersistence::Direct(
                    "tests/invariants/cu/inv_009_partial_residual_history.proptest-regressions",
                ),
            )),
            ..Config::default()
        },
        TestRng::from_seed(RngAlgorithm::ChaCha, &[0x09; 32]),
    );
    runner
        .run(&strategy, |history| {
            record(history);
            Ok(())
        })
        .unwrap();
    let totals = counts.get();
    assert!(totals[4] > 0 && totals[5] > 0 && totals[6] > 0);
    println!("INV-009/011: 24 boundary histories + 32 seeded shrinkable histories; fills/bound rejects/consumed rejects/public controls/integral fills/residue fills/sub-unit residuals/max CU={totals:?}");
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
