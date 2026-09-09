//! Row 412: permissionless liquidation revokes a retained sibling-asset CPI capability.
//!
//! Unlike the direct issue-406 regression, retain signed bytes before liquidation
//! and require a post-liquidation economic control on a still-live sibling asset.
//! No owner reduction/conversion, matcher-program replacement, or generation reuse.

use super::*;
use percolator_prog::ix::CrankObservationHint;

const LP: usize = 1;
const TAKER: usize = 2;
const PRICE: u64 = 1_000_000;
const OPEN_Q: i128 = 2 * POS_SCALE as i128;
const FILL_Q: i128 = POS_SCALE as i128 / 100;
const CAP: u16 = 37;

#[derive(Default, Debug)]
struct Evidence {
    histories: usize,
    transactions: usize,
    liquidations: usize,
    stale_rejections: usize,
    revoked_rejections: usize,
    fresh_fills: usize,
    max_cu: u64,
}

fn checked(
    env: &mut V16Svm,
    frame_history: &AuthorizationHistory,
    evidence: &mut Evidence,
    error: Option<PercolatorError>,
    execute: impl FnOnce(&mut V16Svm) -> Result<TxSuccess, String>,
) {
    let before = capability_frame(env, frame_history);
    env.begin_public_trace();
    let result = execute(env);
    let trace = env.finish_public_trace();
    trace
        .validate_public_execution()
        .expect("public value/rollback trace");
    assert_eq!(trace.steps.len(), 1, "no hidden setup or reauthorization");
    if let Some(error) = error {
        let expected = format!("Custom({})", error as u32);
        let actual = result.expect_err("retained authority must reject");
        assert!(actual.contains(&expected), "expected {expected}: {actual}");
        assert!(
            capability_frame(env, frame_history) == before,
            "full economic frame rollback"
        );
    } else {
        let success = result.expect("public control must remain live");
        evidence.max_cu = evidence.max_cu.max(success.compute_units);
    }
    assert_eq!(env.token_supply_observed(), env.initial_token_supply);
    evidence.transactions += 1;
}

fn position(env: &V16Svm, actor: usize, asset: u16) -> i128 {
    env.primary_portfolio(actor)
        .legs
        .iter()
        .map(|leg| leg.try_to_runtime().unwrap())
        .filter(|leg| leg.active && leg.asset_index == u32::from(asset))
        .map(|leg| leg.basis_pos_q)
        .sum()
}

fn equity(env: &V16Svm, actor: usize) -> i128 {
    let portfolio = env.primary_portfolio(actor);
    i128::try_from(portfolio.capital.get()).unwrap() + portfolio.pnl.get()
}

fn consumer(env: &mut V16Svm, route: CpiRoute, sign: i128) -> Transaction {
    match route {
        CpiRoute::Single => env.build_retained_cpi_trade(TAKER, LP, 1, sign * FILL_Q, 0),
        CpiRoute::Batch => env.build_retained_batch_cpi_trade(TAKER, LP, 1, sign * FILL_Q, 0),
    }
}

fn run_case(route: CpiRoute, sign: i128, evidence: &mut Evidence) {
    let mut env = V16Svm::new(
        [0x7c; 32],
        MarketConfig {
            initial_price: PRICE,
            min_nonzero_mm_req: 599,
            min_nonzero_im_req: 600,
            maintenance_margin_bps: 2_000,
            initial_margin_bps: 2_000,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [100_000_000, 500_000, 100_000_000, 100_000_000, 100_000_000],
            actor_token_balances: [200_000_000; 5],
            ..MarketConfig::default()
        },
    );
    // Used only to enumerate the complete account frame, not to infer revocation
    // from a grant bit or to label a permissionless liquidation an owner event.
    let frame_history = AuthorizationHistory::new(&env);
    checked(&mut env, &frame_history, evidence, None, |env| {
        env.configure_auth_mark(false, 0, 1, PRICE)
    });
    checked(&mut env, &frame_history, evidence, None, |env| {
        env.trade_cpi(0, LP, 0, sign * OPEN_Q, 0, 0)
    });
    checked(&mut env, &frame_history, evidence, None, |env| {
        env.set_matcher_config_with_trade_fee_cap(LP, 1, CAP)
    });
    assert_eq!(position(&env, LP, 0), -sign * OPEN_Q);
    assert_eq!(equity(&env, LP), 500_000);
    let original_grant =
        state::read_portfolio_matcher_config(&env.primary_portfolio_data(LP)).unwrap();
    assert_eq!(original_grant.enabled(), 1);
    assert_eq!(original_grant.trade_fee_cap_bps(), CAP);
    assert_eq!(
        state::read_portfolio_matcher_expiry(&env.primary_portfolio_data(LP)).unwrap(),
        u64::MAX
    );
    let epoch = env.primary_portfolio_position_epoch(LP);
    let sequence = env.primary_portfolio_matcher_sequence(LP);
    let ids: Vec<_> = (0..env.actors.len())
        .map(|actor| env.primary_portfolio_id(actor))
        .collect();
    let generations = env
        .primary_market_state()
        .1
        .assets
        .into_iter()
        .map(|asset| asset.market_id)
        .collect::<Vec<_>>();
    let matcher_before = env.all_matcher_context_data();
    let tokens_before = env.all_token_account_data();
    let untouched: Vec<_> = [0, TAKER, 3, 4]
        .map(|actor| {
            let key = env.actors[actor].portfolio;
            (key, env.svm.get_account(&key).unwrap())
        })
        .into();
    let retained = consumer(&mut env, route, sign);
    let retained_bytes = bincode::serialize(&retained).unwrap();
    // Two deliveries signed before liquidation keep the runtime signature cache
    // from substituting AlreadyProcessed for the program's authorization check.
    let retained_after_regrant = consumer(&mut env, route, sign);
    let regrant_retry_bytes = bincode::serialize(&retained_after_regrant).unwrap();
    assert_eq!(
        retained.message.instructions.last().unwrap().data,
        retained_after_regrant
            .message
            .instructions
            .last()
            .unwrap()
            .data
    );
    assert_ne!(retained.signatures, retained_after_regrant.signatures);
    assert!(
        !retained.message.account_keys[..retained.message.header.num_required_signatures as usize]
            .contains(&env.actors[LP].signer.pubkey()),
        "consumer has no LP signature"
    );
    let before = capability_frame(&env, &frame_history);
    env.svm
        .simulate_transaction(retained.clone().into())
        .expect("exact retained bytes authorize a nonzero fill before liquidation");
    env.svm
        .simulate_transaction(retained_after_regrant.clone().into())
        .expect("second retained delivery is initially live too");
    assert!(
        capability_frame(&env, &frame_history) == before,
        "simulation frame"
    );

    env.warp_to_slot(2);
    let adverse_price = (PRICE as i128 + sign * 80_000) as u64;
    for (asset, price) in [(0, adverse_price), (1, PRICE)] {
        checked(&mut env, &frame_history, evidence, None, |env| {
            env.push_auth_mark(asset, 2, price)
        });
    }
    // Refresh then liquidate at one authenticated slot. Bound the public work;
    // do not silently skip a world that never reaches a real partial liquidation.
    for _ in 0..8 {
        checked(&mut env, &frame_history, evidence, None, |env| {
            env.crank(
                LP,
                2,
                vec![CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                }],
            )
        });
        if position(&env, LP, 0) != -sign * OPEN_Q {
            break;
        }
    }
    let remaining = position(&env, LP, 0);
    assert_eq!(
        remaining.signum(),
        -sign,
        "partial liquidation retains the side"
    );
    assert!(
        remaining.unsigned_abs() < OPEN_Q as u128,
        "liquidation must reduce real exposure"
    );
    assert_eq!(
        equity(&env, LP),
        500_000 - 2 * 80_000,
        "two lots lose the input-derived mark delta with no liquidation fee"
    );
    assert_eq!(env.primary_portfolio_position_epoch(LP), epoch + 1);
    assert_eq!(
        env.primary_portfolio_matcher_sequence(LP),
        sequence,
        "automatic revocation is not owner reauthorization"
    );
    let revoked = state::read_portfolio_matcher_config(&env.primary_portfolio_data(LP)).unwrap();
    assert_eq!(revoked.enabled(), 0);
    assert_eq!(revoked.trade_fee_cap_bps(), CAP);
    assert_eq!(
        state::read_portfolio_matcher_expiry(&env.primary_portfolio_data(LP)).unwrap(),
        0
    );
    assert_eq!(
        env.all_matcher_context_data(),
        matcher_before,
        "keeper cannot reconcile external matcher state"
    );
    assert_eq!(env.all_token_account_data(), tokens_before);
    for (key, account) in untouched {
        assert_eq!(
            env.svm.get_account(&key).unwrap(),
            account,
            "unrelated portfolio frame"
        );
    }
    assert_eq!(bincode::serialize(&retained).unwrap(), retained_bytes);
    checked(
        &mut env,
        &frame_history,
        evidence,
        Some(PercolatorError::EngineStale),
        |env| env.land_retained(retained),
    );
    evidence.stale_rejections += 1;
    evidence.liquidations += 1;

    let wallet = env.token_amount(env.actors[LP].source_token);
    let vault = env.token_amount(env.vault);
    let capital = env.primary_portfolio(LP).capital.get();
    checked(&mut env, &frame_history, evidence, None, |env| {
        env.deposit_primary(LP, 5_000_000)
    });
    assert_eq!(
        env.token_amount(env.actors[LP].source_token),
        wallet - 5_000_000
    );
    assert_eq!(env.token_amount(env.vault), vault + 5_000_000);
    assert_eq!(env.primary_portfolio(LP).capital.get(), capital + 5_000_000);
    let fresh_without_grant = consumer(&mut env, route, sign);
    checked(
        &mut env,
        &frame_history,
        evidence,
        Some(PercolatorError::Unauthorized),
        |env| env.land_retained(fresh_without_grant),
    );
    evidence.revoked_rejections += 1;

    checked(&mut env, &frame_history, evidence, None, |env| {
        env.set_matcher_config_with_trade_fee_cap(LP, 1, CAP)
    });
    assert_eq!(env.primary_portfolio_matcher_sequence(LP), sequence + 2);
    assert_eq!(env.primary_portfolio_position_epoch(LP), epoch + 1);
    assert_eq!(env.all_matcher_context_data(), matcher_before);
    // Identical scope returning does not restore old consent. Refreshing a signed
    // request and explicitly renewing the grant are separately necessary.
    assert_eq!(
        bincode::serialize(&retained_after_regrant).unwrap(),
        regrant_retry_bytes
    );
    checked(
        &mut env,
        &frame_history,
        evidence,
        Some(PercolatorError::EngineStale),
        |env| env.land_retained(retained_after_regrant),
    );
    evidence.stale_rejections += 1;
    let fresh = consumer(&mut env, route, sign);
    let tokens = env.all_token_account_data();
    let capital = [TAKER, LP].map(|actor| env.primary_portfolio(actor).capital.get());
    let equities = [TAKER, LP].map(|actor| equity(&env, actor));
    checked(&mut env, &frame_history, evidence, None, |env| {
        env.land_retained(fresh)
    });
    assert_eq!(position(&env, TAKER, 1), sign * FILL_Q);
    assert_eq!(position(&env, LP, 1), -sign * FILL_Q);
    assert_eq!(
        position(&env, LP, 0),
        remaining,
        "sibling fill cannot reissue liquidated basis"
    );
    assert_eq!(
        [TAKER, LP].map(|actor| env.primary_portfolio(actor).capital.get()),
        capital
    );
    assert_eq!(env.all_token_account_data(), tokens);
    assert_eq!(
        [TAKER, LP].map(|actor| equity(&env, actor)),
        equities,
        "zero-fee at-mark sibling fill cannot transfer either owner's quote value"
    );
    let final_grant =
        state::read_portfolio_matcher_config(&env.primary_portfolio_data(LP)).unwrap();
    assert_eq!(final_grant.enabled(), 1);
    assert_eq!(final_grant.trade_fee_cap_bps(), CAP);
    assert_eq!(final_grant.matcher_program, original_grant.matcher_program);
    assert_eq!(final_grant.matcher_context, original_grant.matcher_context);
    assert_eq!(
        final_grant.matcher_delegate,
        original_grant.matcher_delegate
    );
    assert_eq!(final_grant.position_epoch(), epoch + 2);
    assert_eq!(
        (0..env.actors.len())
            .map(|actor| env.primary_portfolio_id(actor))
            .collect::<Vec<_>>(),
        ids
    );
    assert_eq!(
        env.primary_market_state()
            .1
            .assets
            .into_iter()
            .map(|asset| asset.market_id)
            .collect::<Vec<_>>(),
        generations
    );
    evidence.fresh_fills += 1;
    evidence.histories += 1;
}

#[test]
fn v16_program_row412_liquidation_revokes_retained_sibling_asset_capability() {
    let mut evidence = Evidence::default();
    for route in [CpiRoute::Single, CpiRoute::Batch] {
        for sign in [-1, 1] {
            eprintln!("liquidation capability world: {route:?}, sign={sign}");
            run_case(route, sign, &mut evidence);
        }
    }
    assert_eq!(evidence.histories, 4);
    assert_eq!(evidence.transactions, 52);
    assert_eq!(evidence.liquidations, 4);
    assert_eq!(evidence.stale_rejections, 8);
    assert_eq!(evidence.revoked_rejections, 4);
    assert_eq!(evidence.fresh_fills, 4);
    assert!(evidence.max_cu <= 300_000, "bounded-shape CU ceiling");
    println!("INV-012 permissionless liquidation: {evidence:?}");
}
