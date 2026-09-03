//! INV-012 - Capability and delegate scope.
//!
//! A retained CPI trade must bind the exact incarnation of the LP matcher
//! capability it intends to consume. Re-enabling the same program, context,
//! delegate, and fee cap is a new grant, not permission to revive transactions
//! retained under the old grant.
//!
//! This public LiteSVM matrix covers both CPI transports. Each world retains a
//! valid transaction, disables and re-enables the identical matcher tuple, and
//! requires the old transaction to reject with exact program-account, matcher,
//! token-supply, and lamport rollback. A transaction built after re-enable must
//! still execute, excluding an always-rejecting fix.

use crate::support::v16_svm::{MarketConfig, V16Svm};
use percolator::POS_SCALE;
use percolator_prog::error::PercolatorError;

#[derive(Clone, Copy, Debug)]
enum CpiRoute {
    Single,
    Batch,
}

fn run_matcher_capability_aba_case(route: CpiRoute) {
    const TAKER: usize = 0;
    const LP: usize = 1;
    const PRICE: u64 = 100;
    const DEPOSIT: u128 = 1_000_000;
    const SIZE_Q: i128 = POS_SCALE as i128;

    let mut seed = [0x12; 32];
    seed[0] ^= match route {
        CpiRoute::Single => 1,
        CpiRoute::Batch => 2,
    };
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            actor_deposits: [DEPOSIT, DEPOSIT, 0, 0, 0],
            actor_token_balances: [2_000_000, 2_000_000, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    env.configure_auth_mark(false, 0, 1, PRICE)
        .expect("configure authenticated mark");

    let retained = match route {
        CpiRoute::Single => env.build_retained_cpi_trade(TAKER, LP, 0, SIZE_Q, 0),
        CpiRoute::Batch => env.build_retained_batch_cpi_trade(TAKER, LP, 0, SIZE_Q, 0),
    };
    let old_sequence = env.primary_portfolio_matcher_sequence(LP);
    env.set_matcher_config(LP, 0).expect("disable matcher");
    env.set_matcher_config(LP, 1)
        .expect("re-enable identical matcher tuple");
    assert_eq!(
        env.primary_portfolio_matcher_sequence(LP),
        old_sequence + 2,
        "the replacement grant must be a distinct matcher-config incarnation",
    );

    let market_before = env.market_data(false);
    let taker_before = env.primary_portfolio_data(TAKER);
    let lp_before = env.primary_portfolio_data(LP);
    let matcher_before = env.all_matcher_context_data();
    let supply_before = env.token_supply_observed();
    let taker_lamports_before = env.account_lamports(env.actors[TAKER].portfolio);
    let lp_lamports_before = env.account_lamports(env.actors[LP].portfolio);

    let stale_error = env
        .land_retained(retained)
        .expect_err("a transaction retained under the prior matcher grant must reject");
    let expected_error = format!("Custom({})", PercolatorError::EngineStale as u32);
    assert!(
        stale_error.contains(&expected_error),
        "stale matcher grant must fail with {expected_error}, got {stale_error}",
    );
    assert_eq!(env.market_data(false), market_before, "market rollback");
    assert_eq!(
        env.primary_portfolio_data(TAKER),
        taker_before,
        "taker rollback"
    );
    assert_eq!(env.primary_portfolio_data(LP), lp_before, "LP rollback");
    assert_eq!(
        env.all_matcher_context_data(),
        matcher_before,
        "matcher rollback"
    );
    assert_eq!(
        env.token_supply_observed(),
        supply_before,
        "SPL supply rollback"
    );
    assert_eq!(
        env.account_lamports(env.actors[TAKER].portfolio),
        taker_lamports_before,
        "taker lamport rollback",
    );
    assert_eq!(
        env.account_lamports(env.actors[LP].portfolio),
        lp_lamports_before,
        "LP lamport rollback",
    );

    let fresh = match route {
        CpiRoute::Single => env.build_retained_cpi_trade(TAKER, LP, 0, SIZE_Q, 0),
        CpiRoute::Batch => env.build_retained_batch_cpi_trade(TAKER, LP, 0, SIZE_Q, 0),
    };
    env.land_retained(fresh)
        .expect("the current matcher-config incarnation must remain live");
    let taker = env.primary_portfolio(TAKER);
    let lp = env.primary_portfolio(LP);
    assert!(
        taker.active_bitmap.iter().any(|word| word.get() != 0),
        "fresh taker transaction must install real exposure",
    );
    assert!(
        lp.active_bitmap.iter().any(|word| word.get() != 0),
        "fresh LP transaction must install real exposure",
    );
}

#[test]
fn v16_program_cpi_trades_bind_matcher_capability_incarnation() {
    for route in [CpiRoute::Single, CpiRoute::Batch] {
        run_matcher_capability_aba_case(route);
    }
}
