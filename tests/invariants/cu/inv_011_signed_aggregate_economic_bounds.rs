//! INV-011 - Signed aggregate economic bounds.
//!
//! The batch-CPI wire carries aggregate adverse-slippage and taker engine-fee
//! caps in addition to exact per-leg quantities and limit prices. This file
//! composes those fields through the matcher CPI, shared engine transition, and
//! transaction rollback boundary:
//!
//! * a single CPI fill cannot exceed the taker's signed limit price; and
//! * a multi-leg CPI batch aborts atomically if any leg would exceed its signed
//!   limit or the aggregate quote-atom caps; and
//! * exact-fill validation makes the signed leg vector the aggregate quantity
//!   and final-position bound for the one-shot instruction.

use super::*;

#[test]
fn v16_program_signed_aggregate_bound_composition_is_source_complete() {
    assert_certified_engine_pin("INV-011 aggregate consent");
    let production = include_str!("../../../src/v16_program.rs");
    let batch_variant = production
        .split_once("BatchTradeCpi {")
        .and_then(|(_, tail)| tail.split_once("},").map(|(body, _)| body))
        .expect("BatchTradeCpi wire variant");
    for field in ["max_slippage_atoms: u128", "max_fee_atoms: u128"] {
        assert!(
            batch_variant.contains(field),
            "missing signed field {field}"
        );
    }
    let handler = production
        .split_once("fn handle_batch_trade_cpi<'a>(")
        .and_then(|(_, tail)| tail.split_once("fn handle_close_portfolio<'a>("))
        .map(|(body, _)| body)
        .expect("BatchTradeCpi handler");
    for guard in [
        "policy_v16::adverse_trade_slippage_atoms(",
        "policy_v16::accumulate_with_cap(",
        "Some(max_fee_atoms)",
    ] {
        assert!(handler.contains(guard), "missing aggregate guard {guard}");
    }
    let executor = production
        .split_once("fn handle_batch_execute_zero_copy<'a>(")
        .and_then(|(_, tail)| tail.split_once("fn handle_trade_nocpi<'a>("))
        .map(|(body, _)| body)
        .expect("shared batch executor");
    assert!(executor.contains("outcome.fee_a > cap"));
    assert!(
        include_str!("inv_011_signed_aggregate_economic_bounds.rs").contains(
            "fn v16_program_batch_cpi_aggregate_quote_caps_abort_matcher_and_wrapper_atomically("
        )
    );
    let decoder_proofs =
        include_str!("../kani/inv_022_instruction_decoding_and_schema_upgrade_safety.rs");
    assert!(decoder_proofs.contains("fn kani_v16_batch_cpi_preserves_aggregate_slippage_cap("));
    assert!(decoder_proofs.contains("fn kani_v16_batch_cpi_preserves_aggregate_fee_cap("));
    let aggregate_proofs = include_str!("../kani/inv_011_signed_aggregate_economic_bounds.rs");
    assert!(aggregate_proofs.contains("fn kani_v16_adverse_slippage_direction_is_exact("));
    assert!(aggregate_proofs
        .contains("fn kani_v16_aggregate_slippage_accumulator_is_exact_and_fail_closed("));
}

#[test]
fn v16_program_tradecpi_limit_price_enforced() {
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
    let (ctx, delegate, _) = env.init_matcher_context_with_passive_spread_authorized(
        matcher_program,
        &maker_owner,
        maker,
        500,
        1_000,
    );
    let do_trade = |env: &mut V16CuEnv, limit: u64| -> Result<u64, String> {
        env.svm.expire_blockhash();
        env.send(
            env.trade_cpi_ix(taker, maker, 0, (10 * POS_SCALE) as i128, 100, limit),
            vec![
                AccountMeta::new(taker_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker, false),
                AccountMeta::new(maker, false),
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new(ctx, false),
                AccountMeta::new_readonly(delegate, false),
            ],
            &[&taker_owner],
        )
    };
    let (_, g0) = env.market_state();
    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker).unwrap();
    let maker_before = env.svm.get_account(&maker).unwrap();

    let tight = do_trade(&mut env, 100);
    assert!(
        tight.is_err(),
        "buy with limit at oracle must reject when matcher fills above it",
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "tight-limit rejection must not mutate market accounting",
    );
    assert_eq!(
        env.svm.get_account(&taker).unwrap(),
        taker_before,
        "tight-limit rejection must not fill the taker",
    );
    assert_eq!(
        env.svm.get_account(&maker).unwrap(),
        maker_before,
        "tight-limit rejection must not fill the maker",
    );
    assert_eq!(
        env.market_state().1.vault,
        g0.vault,
        "vault unchanged by rejected trade",
    );

    let ok = do_trade(&mut env, 1_000_000);
    assert!(ok.is_ok(), "buy with generous limit executes: {ok:?}");
    assert!(
        env.portfolio_state(taker).legs[0].basis_pos_q.get() > 0,
        "taker filled under generous limit",
    );
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g1.c_tot + g1.insurance, "conservation after fill");
}

#[test]
fn v16_program_batch_cpi_per_leg_limit_aborts_whole_batch() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker = Keypair::new();
    let lp = Keypair::new();
    let ta = env.create_portfolio(&taker);
    let la = env.create_portfolio(&lp);
    env.deposit(&taker, ta, 1_000_000);
    env.deposit(&lp, la, 1_000_000);
    let (ctx, delegate, _) = env.init_matcher_context_with_passive_spread_authorized(
        matcher_program,
        &lp,
        la,
        500,
        1_000,
    );
    let size_q = (5 * POS_SCALE) as i128;
    let metas = |env: &V16CuEnv| {
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ta, false),
            AccountMeta::new(la, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ]
    };

    env.svm.expire_blockhash();
    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&ta).unwrap();
    let lp_before = env.svm.get_account(&la).unwrap();
    let rejected = env.send(
        env.batch_trade_cpi_ix(
            ta,
            la,
            vec![
                BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: first_generation_market_id(0),
                    size_q,
                    fee_bps: 100,
                    limit_price: 1_000_000,
                },
                BatchTradeCpiLeg {
                    asset_index: 1,
                    market_id: first_generation_market_id(1),
                    size_q,
                    fee_bps: 100,
                    limit_price: 100,
                },
            ],
        ),
        metas(&env),
        &[&taker],
    );
    assert!(
        rejected.is_err(),
        "a per-leg signed slippage violation must abort the whole batch",
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "batch rejection must not mutate market state",
    );
    assert_eq!(
        env.svm.get_account(&ta).unwrap(),
        taker_before,
        "batch rejection must not partially fill taker legs",
    );
    assert_eq!(
        env.svm.get_account(&la).unwrap(),
        lp_before,
        "batch rejection must not partially fill LP legs",
    );

    env.svm.expire_blockhash();
    let ok = env.send(
        env.batch_trade_cpi_ix(
            ta,
            la,
            vec![
                BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: first_generation_market_id(0),
                    size_q,
                    fee_bps: 100,
                    limit_price: 1_000_000,
                },
                BatchTradeCpiLeg {
                    asset_index: 1,
                    market_id: first_generation_market_id(1),
                    size_q,
                    fee_bps: 100,
                    limit_price: 1_000_000,
                },
            ],
        ),
        metas(&env),
        &[&taker],
    );
    assert!(
        ok.is_ok(),
        "batch with both limits generous must execute: {ok:?}"
    );
    let taker_after = state::read_portfolio(&env.svm.get_account(&ta).unwrap().data).unwrap();
    assert!(
        has_active_leg_for_asset(&taker_after, 0) && has_active_leg_for_asset(&taker_after, 1),
        "both legs filled when every signed leg bound is satisfied",
    );
}

#[test]
fn v16_program_batch_cpi_aggregate_quote_caps_abort_matcher_and_wrapper_atomically() {
    const BASE_SPREAD_BPS: u32 = 500;
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.update_trade_fee_policy_with_cu(100);
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker = Keypair::new();
    let lp = Keypair::new();
    let taker_account = env.create_portfolio(&taker);
    let lp_account = env.create_portfolio(&lp);
    env.deposit(&taker, taker_account, 10_000_000);
    env.deposit(&lp, lp_account, 10_000_000);
    let (ctx, delegate, _) = env.init_matcher_context_with_passive_spread_authorized(
        matcher_program,
        &lp,
        lp_account,
        BASE_SPREAD_BPS,
        BASE_SPREAD_BPS,
    );

    let size_q = (5 * POS_SCALE) as i128;
    let prices = {
        let (_, group) = env.market_state();
        [
            group.assets[0].effective_price,
            group.assets[1].effective_price,
        ]
    };
    let buy_price =
        u64::try_from(u128::from(prices[0]) * (10_000 + u128::from(BASE_SPREAD_BPS)) / 10_000)
            .unwrap();
    let sell_price =
        u64::try_from(u128::from(prices[1]) * (10_000 - u128::from(BASE_SPREAD_BPS)) / 10_000)
            .unwrap();
    let expected_slippage =
        percolator_prog::policy_v16::adverse_trade_slippage_atoms(size_q, buy_price, prices[0])
            .unwrap()
            .checked_add(
                percolator_prog::policy_v16::adverse_trade_slippage_atoms(
                    -size_q, sell_price, prices[1],
                )
                .unwrap(),
            )
            .unwrap();
    assert!(expected_slippage > 0);
    let ceil_div = |numerator: u128, denominator: u128| {
        numerator / denominator + u128::from(numerator % denominator != 0)
    };
    let expected_fee = prices
        .iter()
        .map(|price| {
            let notional = ceil_div(
                size_q.unsigned_abs() * u128::from(*price),
                POS_SCALE as u128,
            );
            ceil_div(notional * 100, 10_000)
        })
        .sum::<u128>();
    assert!(expected_fee > 0);

    let asset_market_ids = [env.asset_market_id(0), env.asset_market_id(1)];
    let market = env.market;
    let taker_pubkey = taker.pubkey();
    let legs = move || {
        vec![
            BatchTradeCpiLeg {
                asset_index: 0,
                market_id: asset_market_ids[0],
                size_q,
                fee_bps: 100,
                limit_price: u64::MAX,
            },
            BatchTradeCpiLeg {
                asset_index: 1,
                market_id: asset_market_ids[1],
                size_q: -size_q,
                fee_bps: 100,
                limit_price: 1,
            },
        ]
    };
    let metas = move || {
        vec![
            AccountMeta::new(taker_pubkey, true),
            AccountMeta::new(market, false),
            AccountMeta::new(taker_account, false),
            AccountMeta::new(lp_account, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ]
    };
    let snapshot = |env: &V16CuEnv| {
        (
            env.svm.get_account(&env.market).unwrap(),
            env.svm.get_account(&taker_account).unwrap(),
            env.svm.get_account(&lp_account).unwrap(),
            env.svm.get_account(&ctx).unwrap(),
        )
    };

    let before = snapshot(&env);
    env.svm.expire_blockhash();
    let slippage_rejected = env.send(
        env.batch_trade_cpi_ix_with_caps(
            taker_account,
            lp_account,
            legs(),
            expected_slippage - 1,
            u128::MAX,
        ),
        metas(),
        &[&taker],
    );
    assert!(slippage_rejected.is_err());
    assert_eq!(
        snapshot(&env),
        before,
        "slippage cap must roll back matcher CPI"
    );

    env.svm.expire_blockhash();
    let fee_rejected = env.send(
        env.batch_trade_cpi_ix_with_caps(
            taker_account,
            lp_account,
            legs(),
            expected_slippage,
            expected_fee - 1,
        ),
        metas(),
        &[&taker],
    );
    assert!(fee_rejected.is_err());
    assert_eq!(snapshot(&env), before, "fee cap must roll back matcher CPI");

    env.svm.expire_blockhash();
    env.send(
        env.batch_trade_cpi_ix_with_caps(
            taker_account,
            lp_account,
            legs(),
            expected_slippage,
            expected_fee,
        ),
        metas(),
        &[&taker],
    )
    .expect("exact aggregate slippage boundary remains live");
    let taker_after = env.portfolio_state(taker_account);
    assert_eq!(active_leg_for_asset(&taker_after, 0).basis_pos_q, size_q);
    assert_eq!(active_leg_for_asset(&taker_after, 1).basis_pos_q, -size_q);
    let (_, group_after) = env.market_state();
    assert_eq!(group_after.assets[0].oi_eff_long_q, size_q.unsigned_abs());
    assert_eq!(group_after.assets[1].oi_eff_short_q, size_q.unsigned_abs());
}

// security.md sweep — §6.2 profit conversion (#33/#35): ConvertReleasedPnl moves source-backed
// released pnl into withdrawable capital. The caller supplies `amount`, but it must only be a CAP:
// a caller must never convert MORE than the engine's release-bounded amount (which would print
// withdrawable capital). Probe both directions: a huge cap converts exactly the released amount
// (not more), and an under-cap rejects (no partial over/under conversion, no value printed).
#[test]
fn v16_attack_convert_released_pnl_respects_caller_cap() {
    const RELEASED: u128 = 40;
    let mut env = V16CuEnv::new();
    let ledger = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger, 1, RELEASED, 10);
    // portfolio A: convert with a huge cap -> must convert exactly RELEASED, never more.
    let a_owner = Keypair::new();
    let a = env.create_portfolio(&a_owner);
    env.add_source_positive_pnl(a, 1, RELEASED);
    env.crank(
        a,
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: crank_observations(0),
        },
    );
    let (_, g0) = env.market_state();
    env.svm.expire_blockhash();
    let ra = env.send(
        env.convert_released_pnl_ix(a, 1_000_000_000),
        vec![
            AccountMeta::new(a_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(a, false),
        ],
        &[&a_owner],
    );
    assert!(ra.is_ok(), "huge-cap convert should succeed: {:?}", ra);
    let acct_a = env.portfolio_state(a);
    assert_eq!(
        acct_a.capital.get(),
        RELEASED,
        "huge cap converts EXACTLY the released amount, not more"
    );

    // portfolio B: same released pnl, but an under-cap (RELEASED-1) -> wrapper rejects (converted > cap).
    let b_owner = Keypair::new();
    let b = env.create_portfolio(&b_owner);
    env.add_source_positive_pnl(b, 1, RELEASED);
    env.crank(
        b,
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: crank_observations(0),
        },
    );
    env.svm.expire_blockhash();
    let rb = env.send(
        env.convert_released_pnl_ix(b, RELEASED - 1),
        vec![
            AccountMeta::new(b_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(b, false),
        ],
        &[&b_owner],
    );
    assert!(
        rb.is_err(),
        "under-cap convert must reject (engine releases {} > cap {})",
        RELEASED,
        RELEASED - 1
    );
    assert_eq!(
        env.portfolio_state(b).capital.get(),
        0,
        "rejected convert moves nothing"
    );

    // zero-amount convert is rejected outright.
    env.svm.expire_blockhash();
    let rz = env.send(
        env.convert_released_pnl_ix(b, 0),
        vec![
            AccountMeta::new(b_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(b, false),
        ],
        &[&b_owner],
    );
    assert!(rz.is_err(), "zero-amount convert rejected");

    let (_, g1) = env.market_state();
    assert!(
        g1.vault >= g1.c_tot + g1.insurance,
        "senior conservation after conversions"
    );
    assert_eq!(
        g1.vault, g0.vault,
        "ConvertReleasedPnl moves no vault tokens"
    );
}
