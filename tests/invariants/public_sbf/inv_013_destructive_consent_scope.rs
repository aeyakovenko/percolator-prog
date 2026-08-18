//! INV-013 - Destructive-consent scope.
//!
//! Normative obligation: a retained close may dematerialize only the exact empty portfolio state
//! the owner authorized. Later deposits, position episodes, and funding telemetry must invalidate
//! that consent even when the portfolio returns to empty in the same incarnation.
//!
//! Evidence in this file (I/F): `v16_program_issue402_delayed_close_cannot_erase_later_funding`
//! builds the deployed SBF state exclusively through public instructions. It retains a close while
//! the portfolio is empty, starts and completes a funded trading episode, records nonzero paid
//! funding, returns all collateral, and submits the retained close. Rejection must preserve exact
//! market, portfolio, SPL, and supply state; a freshly bound close must remain live.
//!
//! Guarantee boundary: the wrapper has no reward-mint route, so this test proves the exact public
//! prerequisite that issue 402's external reward controller consumes: deletion of a nonzero,
//! same-incarnation funding counter. It does not model that controller's fixed-supply allocation.

use crate::support::v16_svm::{MarketConfig, V16Svm};
use percolator::POS_SCALE;
use percolator_prog::ix::CrankObservationHint;

fn funding_observation(env: &V16Svm) -> Vec<CrankObservationHint> {
    vec![CrankObservationHint {
        asset_index: 0,
        oracle_accounts: env.primary_profile(0).oracle_leg_count,
    }]
}

#[test]
fn v16_program_issue402_delayed_close_cannot_erase_later_funding() {
    const VICTIM: usize = 0;
    const COUNTERPARTY: usize = 1;
    const PRICE: u64 = 2;
    const TARGET: u64 = 1;
    const DEPOSIT: u128 = 1_000_000;
    const SIZE_Q: i128 = 100 * POS_SCALE as i128;

    let mut env = V16Svm::new(
        [0x42; 32],
        MarketConfig {
            initial_price: PRICE,
            max_price_move_bps_per_slot: 24,
            max_accrual_dt_slots: 1,
            max_abs_funding_e9_per_slot: 1_000,
            min_funding_lifetime_slots: 1,
            actor_deposits: [0, DEPOSIT, DEPOSIT, DEPOSIT, 1],
            ..MarketConfig::default()
        },
    );
    let portfolio_id = env.primary_portfolio_id(VICTIM);
    let retained_close = env.build_retained_close_primary_portfolio(VICTIM);

    env.deposit_primary(VICTIM, DEPOSIT)
        .expect("start a later funded episode");
    assert_eq!(
        env.primary_portfolio_id(VICTIM),
        portfolio_id,
        "the attack remains inside one portfolio incarnation"
    );
    env.trade_no_cpi(VICTIM, COUNTERPARTY, 0, -SIZE_Q, PRICE, 0)
        .expect("open funding-paying short");

    env.warp_to_slot(2);
    env.push_auth_mark(0, 2, TARGET)
        .expect("stage a nonzero funding premium");
    for actor in [VICTIM, COUNTERPARTY] {
        env.crank(actor, 2, funding_observation(&env))
            .expect("prime funding checkpoint");
    }
    env.warp_to_slot(3);
    for actor in [VICTIM, COUNTERPARTY] {
        env.crank(actor, 3, funding_observation(&env))
            .expect("settle funding telemetry");
    }
    env.trade_no_cpi(VICTIM, COUNTERPARTY, 0, SIZE_Q, PRICE, 0)
        .expect("flatten funding episode");

    let paid = env
        .primary_portfolio(VICTIM)
        .funding_short_paid_atoms_total
        .get();
    assert!(paid > 0, "the later episode records reward-bearing funding");
    let victim_capital = env.primary_portfolio(VICTIM).capital.get();
    env.withdraw_primary(VICTIM, victim_capital)
        .expect("return all remaining victim collateral");
    assert_eq!(env.primary_portfolio(VICTIM).capital.get(), 0);
    assert_eq!(env.primary_portfolio(VICTIM).pnl.get(), 0);

    let market_before = env.market_data(false);
    let portfolio_before = env.primary_portfolio_data(VICTIM);
    let supply_before = env.token_supply_observed();
    let balances_before: Vec<_> = env
        .actors
        .iter()
        .flat_map(|actor| {
            [
                env.token_amount(actor.source_token),
                env.token_amount(actor.destination_token),
            ]
        })
        .collect();

    let stale = env.land_retained(retained_close);
    assert!(
        stale.is_err(),
        "a close signed before the funded episode must not erase later telemetry"
    );
    assert_eq!(env.market_data(false), market_before);
    assert_eq!(env.primary_portfolio_data(VICTIM), portfolio_before);
    assert_eq!(env.token_supply_observed(), supply_before);
    let balances_after: Vec<_> = env
        .actors
        .iter()
        .flat_map(|actor| {
            [
                env.token_amount(actor.source_token),
                env.token_amount(actor.destination_token),
            ]
        })
        .collect();
    assert_eq!(balances_after, balances_before);

    let fresh = env
        .close_primary_portfolio(VICTIM)
        .expect("freshly bound empty close remains live");
    assert!(fresh.compute_units < 1_400_000);
}

#[test]
fn v16_program_failed_deposit_does_not_consume_close_sequence() {
    const OWNER: usize = 0;
    let mut env = V16Svm::new(
        [0x4f; 32],
        MarketConfig {
            actor_deposits: [0, 1, 1, 1, 1],
            actor_token_balances: [1, 1, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    let retained_close = env.build_retained_close_primary_portfolio(OWNER);
    let sequence_before = env.primary_portfolio_matcher_sequence(OWNER);
    let market_before = env.market_data(false);
    let portfolio_before = env.primary_portfolio_data(OWNER);
    let source_before = env.token_amount(env.actors[OWNER].source_token);
    let vault_before = env.token_amount(env.vault);

    let rejected = env.deposit_primary(OWNER, 2);
    assert!(rejected.is_err(), "underfunded deposit must reject");
    assert_eq!(
        env.primary_portfolio_matcher_sequence(OWNER),
        sequence_before,
        "failed deposit must not consume the close/state sequence"
    );
    assert_eq!(env.market_data(false), market_before);
    assert_eq!(env.primary_portfolio_data(OWNER), portfolio_before);
    assert_eq!(
        env.token_amount(env.actors[OWNER].source_token),
        source_before
    );
    assert_eq!(env.token_amount(env.vault), vault_before);

    let close = env
        .land_retained(retained_close)
        .expect("a close remains valid after a fully rolled-back deposit");
    assert!(close.compute_units < 1_400_000);
}
