//! INV-031 - No double use of claim, backing, or insurance atoms.
//!
//! Normative obligation: A backing or claim atom cannot support two economic obligations.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_two_source_claims_preserve_source_backing_single_use` creates equal positive claims
//! in an unfunded and an overfunded source domain, then partitions aggregate conversion. The
//! independent ledger oracle requires each claim to consume only its own source backing and checks
//! that a retry cannot consume the funded atoms again. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//! `v16_program_haircut_conversion_retries_cannot_reuse_claim_or_backing` closes the retained
//! partial-payout retry gap across every public trade route and requires exact rejection rollback.
//! `v16_program_live_source_lien_route_pairs_preserve_single_backing_ownership` crosses every
//! ordered pair of public trade routes and both source sides. It grows a real live source lien in
//! multiple strict steps, requires exact account/source/bucket ownership after every mutation,
//! proves the alternate route cannot bypass the same admission frontier, and then requires bounded
//! release of the exact backing atoms.
//! `v16_program_two_accounts_cannot_reserve_the_same_source_backing_atoms` adds the missing
//! multi-account composition. Two portfolios hold claims on one source domain while taking risk
//! in different assets. Across all four trade routes, both source sides, and both account orders,
//! an independent sum of account-local liens must equal the one source aggregate and backing
//! bucket after every mutation. Both accounts reach the shared admission frontier, reject with
//! exact rollback, and release the exact original pool through bounded public cranks.
//! `v16_program_shared_lien_partial_consumption_retries_preserve_sibling_claim` extends that
//! shared-lien prefix with interleaved release, conversion, payout, refill and retained retries.
//! Its suffix ledger checks both owners separately while the sibling still owns a live lien,
//! then requires its exact claim conversion and payout. This is eight no-CPI histories, not
//! expiry/impairment, arbitrary histories, insurance-lien reachability or engine-proof closure.
//! `v16_program_shared_lien_expiry_refill_preserves_owner_attribution` crosses the same public
//! shared-lien frontier with authenticated exact/late expiry. The old valid total must become
//! impaired exactly once, each owner must release only its own amount while the sibling remains
//! byte-exact, and replacement backing must reject until both old liens clear. Only the newly
//! transferred aggregate or split refill may then become fresh.
//! The haircut-conversion matrix also submits a cap one atom below the independently known
//! conversion amount. The deployed handler reaches its post-conversion cap rejection, and SVM
//! rollback must restore the claim, backing bucket, portfolio, custody, and every auxiliary
//! account before the identical full-cap request consumes the tranche exactly once.
//! The same trace is reused by INV-027: the externally withdrawn tranche must equal the original
//! loser's principal debit while a separately funded portfolio remains byte- and SPL-exact.
//!
//! Guarantee boundary: this certifies the generated two-domain conversion family on the fixed
//! engine pin. Other backing and insurance lifecycle compositions remain covered by the route
//! matrices below.

use super::*;
use crate::support::{
    fuzz_model::{
        assert_public_encumbrance_census, assert_public_stock_census,
        assert_source_credit_rate_transition, assert_source_credit_rates, execute_trade_route,
    },
    v16_svm::{MarketConfig, TxSuccess, V16Svm, PRIMARY_ACTOR_COUNT},
};
use percolator::{BackingBucketStatusV16, BOUND_SCALE, POS_SCALE};
use percolator_prog::ix::CrankObservationHint;

#[derive(Debug, PartialEq, Eq)]
struct EconomicSnapshot {
    markets: [Vec<u8>; 2],
    backing_ledger: Vec<u8>,
    portfolios: Vec<Vec<u8>>,
    foreign_portfolio: Vec<u8>,
    token_accounts: Vec<(solana_sdk::pubkey::Pubkey, Vec<u8>)>,
    matcher_contexts: Vec<Vec<u8>>,
    lamports: Vec<(solana_sdk::pubkey::Pubkey, u64)>,
}

fn economic_snapshot(env: &V16Svm) -> EconomicSnapshot {
    EconomicSnapshot {
        markets: [env.market_data(false), env.market_data(true)],
        backing_ledger: env.backing_domain_ledger_data(),
        portfolios: env.all_primary_portfolio_data(),
        foreign_portfolio: env.foreign_portfolio_data(),
        token_accounts: env.all_token_account_data(),
        matcher_contexts: env.all_matcher_context_data(),
        lamports: env.all_economic_account_lamports(),
    }
}

fn counterparty_lien_backing(env: &V16Svm, actor: usize, source_domain: usize) -> u128 {
    env.primary_portfolio(actor)
        .source_domains
        .iter()
        .find(|source| source.is_occupied() && source.domain.get() as usize == source_domain)
        .map(|source| source.source_lien_counterparty_backing_num.get())
        .unwrap_or(0)
}

fn assert_inv_031_censuses(label: &str, env: &V16Svm) -> Result<(), String> {
    assert_public_stock_census(label, env)?;
    assert_public_encumbrance_census(label, env)
}

fn portfolio_certificate_is_current(env: &V16Svm, actor: usize) -> bool {
    let (_, group) = env.primary_market_state();
    let account = env.primary_portfolio(actor);
    let Ok(cert) = account.health_cert.try_to_runtime() else {
        return false;
    };
    cert.valid
        && cert.cert_oracle_epoch == group.oracle_epoch
        && cert.cert_funding_epoch == group.funding_epoch
        && cert.cert_risk_epoch == group.risk_epoch
        && cert.cert_asset_set_epoch == group.asset_set_epoch
        && cert.active_bitmap_at_cert == account.active_bitmap.map(|word| word.get())
}

fn recertify_lien_world_actor(label: &str, env: &mut V16Svm, actor: usize) -> Result<(), String> {
    for step in 0..8 {
        if portfolio_certificate_is_current(env, actor) {
            return Ok(());
        }
        env.crank(actor, 2, vec![])
            .map_err(|error| format!("{label} recertify actor {actor} step {step}: {error}"))?;
        assert_inv_031_censuses(&format!("{label} recertify actor {actor} step {step}"), env)?;
    }
    Err(format!(
        "{label} actor {actor} did not reach a certificate fixed point"
    ))
}

fn verify_live_lien_route_pair_preserves_single_ownership(
    first_route: TradeRoute,
    retry_route: TradeRoute,
    winner_long: bool,
) -> Result<(), String> {
    const WINNER: usize = 0;
    const COUNTERPARTY: usize = 1;
    const MARKET_CRANKER: usize = 4;
    const WINNING_ASSET: u16 = 0;
    const ADVERSE_ASSET: u16 = 1;
    const START_PRICE: u64 = 100;
    const WINNING_SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const ADVERSE_SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const RISK_INCREMENT_Q: i128 = (POS_SCALE / 10) as i128;
    const BACKING_ATOMS: u128 = 6;
    const WINNER_DEPOSIT: u128 = 363;

    let direction = if winner_long { 1i128 } else { -1i128 };
    let winning_mark = if winner_long { 105 } else { 95 };
    let adverse_mark = if winner_long { 95 } else { 105 };
    let source_domain = if winner_long { 1usize } else { 0usize };
    let label =
        format!("INV-031 first={first_route:?} retry={retry_route:?} winner_long={winner_long}");
    let route_index = |route| match route {
        TradeRoute::NoCpi => 0u8,
        TradeRoute::Cpi => 1,
        TradeRoute::BatchNoCpi => 2,
        TradeRoute::BatchCpi => 3,
    };
    let mut seed = [0x31; 32];
    seed[0] ^= route_index(first_route);
    seed[1] ^= route_index(retry_route) << 2;
    seed[2] ^= u8::from(winner_long);
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: START_PRICE,
            h_max: 4,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            max_abs_funding_e9_per_slot: 0,
            min_funding_lifetime_slots: 1,
            maintenance_fee_per_slot: 0,
            actor_deposits: [WINNER_DEPOSIT, 1_000, 1, 1, 1],
            actor_token_balances: [WINNER_DEPOSIT as u64, 1_000, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    env.begin_public_trace();
    env.top_up_backing_bucket(source_domain as u16, BACKING_ATOMS, 100)
        .map_err(|error| format!("{label} backing top-up: {error}"))?;
    execute_trade_route(
        &mut env,
        first_route,
        WINNER,
        COUNTERPARTY,
        WINNING_ASSET,
        direction * WINNING_SIZE_Q,
        START_PRICE,
        0,
    )
    .map_err(|error| format!("{label} winning-leg open: {error}"))?;
    execute_trade_route(
        &mut env,
        first_route,
        WINNER,
        COUNTERPARTY,
        ADVERSE_ASSET,
        direction * ADVERSE_SIZE_Q,
        START_PRICE,
        0,
    )
    .map_err(|error| format!("{label} adverse-leg open: {error}"))?;

    env.warp_to_slot(2);
    env.push_auth_mark(WINNING_ASSET, 2, winning_mark)
        .map_err(|error| format!("{label} winning mark: {error}"))?;
    env.push_auth_mark(ADVERSE_ASSET, 2, adverse_mark)
        .map_err(|error| format!("{label} adverse mark: {error}"))?;
    let observations = [WINNING_ASSET, ADVERSE_ASSET]
        .into_iter()
        .map(|asset_index| CrankObservationHint {
            asset_index,
            oracle_accounts: env.primary_profile(asset_index as usize).oracle_leg_count,
        })
        .collect::<Vec<_>>();
    for actor in [MARKET_CRANKER, COUNTERPARTY, WINNER] {
        env.crank(actor, 2, observations.clone())
            .map_err(|error| format!("{label} settle actor {actor}: {error}"))?;
    }
    let expected_claim = (WINNING_SIZE_Q.unsigned_abs() / POS_SCALE as u128)
        .checked_mul(u128::from(START_PRICE.abs_diff(winning_mark)))
        .ok_or_else(|| format!("{label} winning PnL oracle overflow"))?;
    let expected_loss = (ADVERSE_SIZE_Q.unsigned_abs() / POS_SCALE as u128)
        .checked_mul(u128::from(START_PRICE.abs_diff(adverse_mark)))
        .ok_or_else(|| format!("{label} adverse PnL oracle overflow"))?;
    let expected_claim_i128 = i128::try_from(expected_claim)
        .map_err(|_| format!("{label} source claim does not fit i128"))?;
    let expected_capital = WINNER_DEPOSIT
        .checked_sub(expected_loss)
        .ok_or_else(|| format!("{label} adverse loss exceeds funded capital"))?;
    let winner = env.primary_portfolio(WINNER);
    if winner.pnl.get() != expected_claim_i128 || winner.capital.get() != expected_capital {
        return Err(format!(
            "{label} did not preserve gross source claim and disjoint capital loss: capital={}, pnl={}, expected_capital={expected_capital}, expected_claim={expected_claim}",
            winner.capital.get(),
            winner.pnl.get(),
        ));
    }
    assert_inv_031_censuses(&format!("{label} before lien"), &env)?;
    let (_, before_reservations) = env.primary_market_state();
    let backing_before_reservations =
        before_reservations.source_backing_buckets[source_domain].fresh_unliened_backing_num;
    let mut accepted_increments = 0u128;
    let mut lien_growth_steps = 0usize;
    let mut prior_lien = 0u128;
    let mut canonical_frontier_reached = false;
    for step in 0..128 {
        let before_attempt = economic_snapshot(&env);
        match execute_trade_route(
            &mut env,
            first_route,
            WINNER,
            COUNTERPARTY,
            ADVERSE_ASSET,
            direction * RISK_INCREMENT_Q,
            adverse_mark,
            0,
        ) {
            Ok(_) => {
                accepted_increments = accepted_increments
                    .checked_add(1)
                    .ok_or_else(|| format!("{label} accepted-increment count overflow"))?;
                let current_lien = counterparty_lien_backing(&env, WINNER, source_domain);
                if current_lien > prior_lien {
                    lien_growth_steps += 1;
                }
                if current_lien < prior_lien {
                    return Err(format!(
                        "{label} risk increase {step} released live lien ownership: {prior_lien} -> {current_lien}"
                    ));
                }
                let (_, current) = env.primary_market_state();
                if current.source_credit[source_domain].valid_liened_backing_num != current_lien
                    || current.source_backing_buckets[source_domain].valid_liened_backing_num
                        != current_lien
                {
                    return Err(format!(
                        "{label} risk increase {step} did not singly attribute its lien: account={current_lien}, source={:?}, bucket={:?}",
                        current.source_credit[source_domain],
                        current.source_backing_buckets[source_domain]
                    ));
                }
                prior_lien = current_lien;
                assert_inv_031_censuses(&format!("{label} reservation increment {step}"), &env)?;
                recertify_lien_world_actor(&label, &mut env, COUNTERPARTY)?;
                recertify_lien_world_actor(&label, &mut env, WINNER)?;
            }
            Err(error) => {
                if !error.contains("Custom(21)") && !error.contains("custom program error: 0x15") {
                    return Err(format!(
                        "{label} canonical-route frontier rejected for an unrelated reason: {error}"
                    ));
                }
                if economic_snapshot(&env) != before_attempt {
                    return Err(format!(
                        "{label} canonical-route frontier did not roll back exactly"
                    ));
                }
                canonical_frontier_reached = true;
                break;
            }
        }
    }
    let (_, frontier_group) = env.primary_market_state();
    let frontier_account = env.primary_portfolio(WINNER);
    let frontier_source = frontier_account
        .source_domains
        .iter()
        .find(|source| source.is_occupied() && source.domain.get() as usize == source_domain)
        .ok_or_else(|| format!("{label} frontier account lost its source attribution"))?;
    if !canonical_frontier_reached
        || accepted_increments == 0
        || lien_growth_steps < 2
        || prior_lien == 0
    {
        return Err(format!(
            "{label} did not reach a nonvacuous live-lien admission frontier: accepted={accepted_increments}, lien_steps={lien_growth_steps}, lien={prior_lien}, source={frontier_source:?}, bucket={:?}",
            frontier_group.source_backing_buckets[source_domain]
        ));
    }

    if matches!(retry_route, TradeRoute::Cpi | TradeRoute::BatchCpi) {
        env.ensure_primary_matcher_enabled(COUNTERPARTY)
            .map_err(|error| format!("{label} prepare alternate matcher capability: {error}"))?;
        assert_inv_031_censuses(&format!("{label} alternate matcher prepared"), &env)?;
    }
    let before_retry = economic_snapshot(&env);
    let retry = execute_trade_route(
        &mut env,
        retry_route,
        WINNER,
        COUNTERPARTY,
        ADVERSE_ASSET,
        direction * RISK_INCREMENT_Q,
        adverse_mark,
        0,
    );
    let retry_error =
        retry.expect_err("an alternate route must not bypass the canonical live-lien frontier");
    if !retry_error.contains("Custom(21)") && !retry_error.contains("custom program error: 0x15") {
        return Err(format!(
            "{label} alternate-route reservation rejected for an unrelated reason: {retry_error}"
        ));
    }
    if economic_snapshot(&env) != before_retry {
        return Err(format!(
            "{label} rejected alternate-route reservation did not roll back every economic account"
        ));
    }
    assert_inv_031_censuses(&format!("{label} after rejected alternate reuse"), &env)?;

    execute_trade_route(
        &mut env,
        first_route,
        WINNER,
        COUNTERPARTY,
        ADVERSE_ASSET,
        -direction
            * (ADVERSE_SIZE_Q
                + i128::try_from(accepted_increments)
                    .map_err(|_| format!("{label} accepted-increment conversion overflow"))?
                    * RISK_INCREMENT_Q),
        adverse_mark,
        0,
    )
    .map_err(|error| format!("{label} flatten adverse leg: {error}"))?;
    execute_trade_route(
        &mut env,
        first_route,
        WINNER,
        COUNTERPARTY,
        WINNING_ASSET,
        -direction * WINNING_SIZE_Q,
        winning_mark,
        0,
    )
    .map_err(|error| format!("{label} flatten winning leg: {error}"))?;

    let mut release_steps = 0usize;
    for step in 0..16 {
        if counterparty_lien_backing(&env, WINNER, source_domain) == 0 {
            break;
        }
        let before = economic_snapshot(&env);
        match env.crank(WINNER, 2, observations.clone()) {
            Ok(_) => {
                release_steps += 1;
                if economic_snapshot(&env) == before {
                    return Err(format!("{label} release crank {step} committed a no-op"));
                }
                assert_inv_031_censuses(&format!("{label} release step {step}"), &env)?;
            }
            Err(error) => {
                return Err(format!(
                    "{label} lien remained but release crank {step} rejected: {error}"
                ));
            }
        }
    }
    let (_, released) = env.primary_market_state();
    if release_steps == 0
        || counterparty_lien_backing(&env, WINNER, source_domain) != 0
        || released.source_credit[source_domain].valid_liened_backing_num != 0
        || released.source_backing_buckets[source_domain].valid_liened_backing_num != 0
        || released.source_backing_buckets[source_domain].fresh_unliened_backing_num
            != backing_before_reservations
        || released.source_backing_buckets[source_domain].status != BackingBucketStatusV16::Fresh
    {
        return Err(format!(
            "{label} bounded release did not return the exact backing atom ownership: steps={release_steps}, source={:?}, bucket={:?}",
            released.source_credit[source_domain],
            released.source_backing_buckets[source_domain]
        ));
    }
    assert_inv_031_censuses(&format!("{label} after release"), &env)?;

    let trace = env.finish_public_trace();
    trace
        .validate_public_execution()
        .map_err(|error| format!("{label} invalid public trace: {error}"))?;
    if trace.out_of_band_economic_mutations != 0
        || trace.steps.iter().filter(|step| !step.succeeded).count() != 2
    {
        return Err(format!(
            "{label} route-pair trace did not isolate both rollback-exact frontier rejections: {trace:?}"
        ));
    }
    Ok(())
}

#[test]
fn v16_program_live_source_lien_route_pairs_preserve_single_backing_ownership() {
    for first_route in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ] {
        for retry_route in [
            TradeRoute::NoCpi,
            TradeRoute::Cpi,
            TradeRoute::BatchNoCpi,
            TradeRoute::BatchCpi,
        ] {
            for winner_long in [false, true] {
                verify_live_lien_route_pair_preserves_single_ownership(
                    first_route,
                    retry_route,
                    winner_long,
                )
                .unwrap_or_else(|error| panic!("{error}"));
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum ConcurrentLienSuffix {
    ReleaseOnly,
    PartialConsumption { split_refill: bool },
    ExpiryRefill { late: bool, split_refill: bool },
}

fn verify_two_account_concurrent_lien_ownership(
    route: TradeRoute,
    reverse_order: bool,
    winner_long: bool,
    suffix: ConcurrentLienSuffix,
) -> Result<(), String> {
    const WINNERS: [usize; 2] = [0, 1];
    const COUNTERPARTIES: [usize; 2] = [2, 3];
    const WINNING_ASSET: u16 = 0;
    const ADVERSE_ASSETS: [u16; 2] = [1, 2];
    const MARKET_CRANKER: usize = 4;
    const START_PRICE: u64 = 100;
    const WINNING_SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const ADVERSE_SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const RISK_INCREMENT_Q: i128 = (POS_SCALE / 10) as i128;
    const BACKING_ATOMS: u128 = 12;
    const WINNER_DEPOSIT: u128 = 363;

    let direction = if winner_long { 1i128 } else { -1i128 };
    let winning_mark = if winner_long { 105 } else { 95 };
    let adverse_mark = if winner_long { 95 } else { 105 };
    let source_domain = if winner_long { 1usize } else { 0usize };
    let actor_order = if reverse_order { [1usize, 0] } else { [0, 1] };
    let route_index = match route {
        TradeRoute::NoCpi => 0u8,
        TradeRoute::Cpi => 1,
        TradeRoute::BatchNoCpi => 2,
        TradeRoute::BatchCpi => 3,
    };
    let label = format!(
        "INV-031 concurrent route={route:?} reverse={reverse_order} winner_long={winner_long} suffix={suffix:?}"
    );
    let mut seed = [0x31; 32];
    seed[0] ^= 0xa0 | route_index;
    seed[1] ^= u8::from(reverse_order) | (u8::from(winner_long) << 1);
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: START_PRICE,
            h_max: 4,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            max_abs_funding_e9_per_slot: 0,
            min_funding_lifetime_slots: 1,
            maintenance_fee_per_slot: 0,
            actor_deposits: [WINNER_DEPOSIT, WINNER_DEPOSIT, 1_000, 1_000, 1],
            actor_token_balances: [
                WINNER_DEPOSIT as u64,
                WINNER_DEPOSIT as u64,
                1_000,
                1_000,
                1,
            ],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.begin_public_trace();
    let backing_expiry = match suffix {
        ConcurrentLienSuffix::ExpiryRefill { .. } => 5,
        ConcurrentLienSuffix::ReleaseOnly | ConcurrentLienSuffix::PartialConsumption { .. } => 100,
    };
    env.top_up_backing_bucket(source_domain as u16, BACKING_ATOMS, backing_expiry)
        .map_err(|error| format!("{label} backing top-up: {error}"))?;

    for pair in 0..WINNERS.len() {
        execute_trade_route(
            &mut env,
            route,
            WINNERS[pair],
            COUNTERPARTIES[pair],
            WINNING_ASSET,
            direction * WINNING_SIZE_Q,
            START_PRICE,
            0,
        )
        .map_err(|error| format!("{label} pair {pair} winning-leg open: {error}"))?;
        execute_trade_route(
            &mut env,
            route,
            WINNERS[pair],
            COUNTERPARTIES[pair],
            ADVERSE_ASSETS[pair],
            direction * ADVERSE_SIZE_Q,
            START_PRICE,
            0,
        )
        .map_err(|error| format!("{label} pair {pair} adverse-leg open: {error}"))?;
    }

    env.warp_to_slot(2);
    env.push_auth_mark(WINNING_ASSET, 2, winning_mark)
        .map_err(|error| format!("{label} winning mark: {error}"))?;
    for asset_index in ADVERSE_ASSETS {
        env.push_auth_mark(asset_index, 2, adverse_mark)
            .map_err(|error| format!("{label} adverse mark {asset_index}: {error}"))?;
    }
    let observations = [WINNING_ASSET, ADVERSE_ASSETS[0], ADVERSE_ASSETS[1]]
        .into_iter()
        .map(|asset_index| CrankObservationHint {
            asset_index,
            oracle_accounts: env.primary_profile(asset_index as usize).oracle_leg_count,
        })
        .collect::<Vec<_>>();
    for actor in [
        MARKET_CRANKER,
        COUNTERPARTIES[0],
        COUNTERPARTIES[1],
        WINNERS[0],
        WINNERS[1],
    ] {
        env.crank(actor, 2, observations.clone())
            .map_err(|error| format!("{label} settle actor {actor}: {error}"))?;
    }
    let expected_claim = (WINNING_SIZE_Q.unsigned_abs() / POS_SCALE as u128)
        .checked_mul(u128::from(START_PRICE.abs_diff(winning_mark)))
        .ok_or_else(|| format!("{label} winning PnL oracle overflow"))?;
    let expected_loss = (ADVERSE_SIZE_Q.unsigned_abs() / POS_SCALE as u128)
        .checked_mul(u128::from(START_PRICE.abs_diff(adverse_mark)))
        .ok_or_else(|| format!("{label} adverse PnL oracle overflow"))?;
    let expected_claim_i128 = i128::try_from(expected_claim)
        .map_err(|_| format!("{label} source claim does not fit i128"))?;
    let expected_capital = WINNER_DEPOSIT
        .checked_sub(expected_loss)
        .ok_or_else(|| format!("{label} adverse loss exceeds funded capital"))?;
    for winner in WINNERS {
        let account = env.primary_portfolio(winner);
        if account.pnl.get() != expected_claim_i128 || account.capital.get() != expected_capital {
            return Err(format!(
                "{label} winner {winner} did not preserve gross source claim and disjoint capital loss: capital={}, pnl={}, expected_capital={expected_capital}, expected_claim={expected_claim}",
                account.capital.get(),
                account.pnl.get(),
            ));
        }
    }
    assert_inv_031_censuses(&format!("{label} before concurrent liens"), &env)?;
    let (_, before_reservations) = env.primary_market_state();
    let backing_before_reservations =
        before_reservations.source_backing_buckets[source_domain].fresh_unliened_backing_num;
    let mut accepted_increments = [0u128; 2];
    let mut frontier_reached = [false; 2];

    for step in 0..128 {
        for pair in actor_order {
            if frontier_reached[pair] {
                continue;
            }
            let before_attempt = economic_snapshot(&env);
            match execute_trade_route(
                &mut env,
                route,
                WINNERS[pair],
                COUNTERPARTIES[pair],
                ADVERSE_ASSETS[pair],
                direction * RISK_INCREMENT_Q,
                adverse_mark,
                0,
            ) {
                Ok(_) => {
                    accepted_increments[pair] = accepted_increments[pair]
                        .checked_add(1)
                        .ok_or_else(|| format!("{label} pair {pair} increment overflow"))?;
                    let local_total = WINNERS.iter().try_fold(0u128, |sum, winner| {
                        sum.checked_add(counterparty_lien_backing(&env, *winner, source_domain))
                            .ok_or_else(|| format!("{label} local lien sum overflow"))
                    })?;
                    let (_, group) = env.primary_market_state();
                    if group.source_credit[source_domain].valid_liened_backing_num != local_total
                        || group.source_backing_buckets[source_domain].valid_liened_backing_num
                            != local_total
                    {
                        return Err(format!(
                            "{label} step {step} pair {pair} reused or lost backing ownership: local={local_total}, source={:?}, bucket={:?}",
                            group.source_credit[source_domain],
                            group.source_backing_buckets[source_domain]
                        ));
                    }
                    assert_inv_031_censuses(
                        &format!("{label} reservation step {step} pair {pair}"),
                        &env,
                    )?;
                    for actor in [COUNTERPARTIES[0], COUNTERPARTIES[1], WINNERS[0], WINNERS[1]] {
                        recertify_lien_world_actor(&label, &mut env, actor)?;
                    }
                }
                Err(error) => {
                    if !error.contains("Custom(21)")
                        && !error.contains("custom program error: 0x15")
                    {
                        return Err(format!(
                            "{label} pair {pair} frontier rejected for an unrelated reason: {error}"
                        ));
                    }
                    if economic_snapshot(&env) != before_attempt {
                        return Err(format!(
                            "{label} pair {pair} frontier rejection did not roll back exactly"
                        ));
                    }
                    frontier_reached[pair] = true;
                }
            }
        }
        if frontier_reached == [true; 2] {
            break;
        }
    }

    let local_liens = WINNERS.map(|winner| counterparty_lien_backing(&env, winner, source_domain));
    let local_total = local_liens[0]
        .checked_add(local_liens[1])
        .ok_or_else(|| format!("{label} frontier lien sum overflow"))?;
    let (_, frontier_group) = env.primary_market_state();
    if frontier_reached != [true; 2]
        || accepted_increments.iter().any(|count| *count == 0)
        || local_liens.iter().any(|lien| *lien == 0)
        || frontier_group.source_credit[source_domain].valid_liened_backing_num != local_total
        || frontier_group.source_backing_buckets[source_domain].valid_liened_backing_num
            != local_total
    {
        return Err(format!(
            "{label} did not reach a shared nonvacuous reservation frontier: accepted={accepted_increments:?}, frontiers={frontier_reached:?}, local={local_liens:?}, source={:?}, bucket={:?}",
            frontier_group.source_credit[source_domain],
            frontier_group.source_backing_buckets[source_domain]
        ));
    }

    let extra_rejections = if let ConcurrentLienSuffix::PartialConsumption { split_refill } = suffix
    {
        verify_shared_lien_partial_consumption_suffix(
            &label,
            &mut env,
            winner_long,
            actor_order,
            accepted_increments,
            split_refill,
        )?;
        3
    } else if let ConcurrentLienSuffix::ExpiryRefill { late, split_refill } = suffix {
        verify_shared_lien_expiry_refill_suffix(
            &label,
            &mut env,
            source_domain,
            actor_order,
            winner_long,
            accepted_increments,
            late,
            split_refill,
        )?;
        2
    } else {
        for pair in actor_order {
            let accepted_q = i128::try_from(accepted_increments[pair])
                .map_err(|_| format!("{label} pair {pair} increment conversion overflow"))?
                .checked_mul(RISK_INCREMENT_Q)
                .ok_or_else(|| format!("{label} pair {pair} increment quantity overflow"))?;
            execute_trade_route(
                &mut env,
                route,
                WINNERS[pair],
                COUNTERPARTIES[pair],
                ADVERSE_ASSETS[pair],
                -direction * (ADVERSE_SIZE_Q + accepted_q),
                adverse_mark,
                0,
            )
            .map_err(|error| format!("{label} pair {pair} flatten adverse leg: {error}"))?;
            execute_trade_route(
                &mut env,
                route,
                WINNERS[pair],
                COUNTERPARTIES[pair],
                WINNING_ASSET,
                -direction * WINNING_SIZE_Q,
                winning_mark,
                0,
            )
            .map_err(|error| format!("{label} pair {pair} flatten winning leg: {error}"))?;
        }

        let mut release_steps = 0usize;
        for step in 0..32 {
            let remaining = WINNERS.iter().try_fold(0u128, |sum, winner| {
                sum.checked_add(counterparty_lien_backing(&env, *winner, source_domain))
                    .ok_or_else(|| format!("{label} remaining lien sum overflow"))
            })?;
            if remaining == 0 {
                break;
            }
            let mut progressed = false;
            for pair in actor_order {
                if counterparty_lien_backing(&env, WINNERS[pair], source_domain) == 0 {
                    continue;
                }
                let before = economic_snapshot(&env);
                env.crank(WINNERS[pair], 2, observations.clone())
                    .map_err(|error| {
                        format!("{label} pair {pair} release step {step} rejected: {error}")
                    })?;
                if economic_snapshot(&env) == before {
                    return Err(format!(
                        "{label} pair {pair} release step {step} committed a no-op"
                    ));
                }
                progressed = true;
                release_steps += 1;
                assert_inv_031_censuses(&format!("{label} release step {step} pair {pair}"), &env)?;
            }
            if !progressed {
                return Err(format!(
                    "{label} retained {remaining} backing atoms without a progressing release"
                ));
            }
        }

        let final_local_total = WINNERS.iter().try_fold(0u128, |sum, winner| {
            sum.checked_add(counterparty_lien_backing(&env, *winner, source_domain))
                .ok_or_else(|| format!("{label} final lien sum overflow"))
        })?;
        let (_, released) = env.primary_market_state();
        if release_steps < 2
            || final_local_total != 0
            || released.source_credit[source_domain].valid_liened_backing_num != 0
            || released.source_backing_buckets[source_domain].valid_liened_backing_num != 0
            || released.source_backing_buckets[source_domain].fresh_unliened_backing_num
                != backing_before_reservations
            || released.source_backing_buckets[source_domain].status
                != BackingBucketStatusV16::Fresh
        {
            return Err(format!(
            "{label} did not release the exact shared backing ownership: steps={release_steps}, local={final_local_total}, source={:?}, bucket={:?}",
            released.source_credit[source_domain],
            released.source_backing_buckets[source_domain]
        ));
        }
        assert_inv_031_censuses(&format!("{label} after concurrent release"), &env)?;
        0
    };
    if env.token_supply_observed() != supply_before {
        return Err(format!("{label} changed SPL token supply"));
    }
    let trace = env.finish_public_trace();
    trace
        .validate_public_execution()
        .map_err(|error| format!("{label} invalid public trace: {error}"))?;
    if trace.out_of_band_economic_mutations != 0
        || trace.steps.iter().filter(|step| !step.succeeded).count() != 2 + extra_rejections
    {
        return Err(format!(
            "{label} did not isolate the expected shared-frontier/history rejections: {trace:?}"
        ));
    }
    Ok(())
}

#[test]
fn v16_program_two_accounts_cannot_reserve_the_same_source_backing_atoms() {
    for route in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ] {
        for reverse_order in [false, true] {
            for winner_long in [false, true] {
                verify_two_account_concurrent_lien_ownership(
                    route,
                    reverse_order,
                    winner_long,
                    ConcurrentLienSuffix::ReleaseOnly,
                )
                .unwrap_or_else(|error| panic!("{error}"));
            }
        }
    }
}

fn verify_shared_lien_expiry_refill_suffix(
    label: &str,
    env: &mut V16Svm,
    domain: usize,
    actor_order: [usize; 2],
    winner_long: bool,
    accepted_increments: [u128; 2],
    late: bool,
    split_refill: bool,
) -> Result<(), String> {
    const EXPIRY_SLOT: u64 = 5;
    const NEXT_EXPIRY_SLOT: u64 = 9;
    const REFILL_ATOMS: u128 = 37;

    let before = env.primary_market_state().1;
    let bucket_before = before.source_backing_buckets[domain];
    let source_before = before.source_credit[domain];
    let local_liens = actor_order.map(|actor| counterparty_lien_backing(env, actor, domain));
    let valid_before = local_liens.iter().sum::<u128>();
    if bucket_before.status != BackingBucketStatusV16::Fresh
        || bucket_before.expiry_slot != EXPIRY_SLOT
        || local_liens.iter().any(|lien| *lien == 0)
        || bucket_before.valid_liened_backing_num != valid_before
        || source_before.valid_liened_backing_num != valid_before
        || bucket_before.impaired_liened_backing_num != 0
        || source_before.impaired_liened_backing_num != 0
    {
        return Err(format!(
            "{label} did not reach a fresh shared-lien expiry frontier: local={local_liens:?}, source={source_before:?}, bucket={bucket_before:?}"
        ));
    }

    let provider_before = env.token_amount(env.provider_source_token);
    let vault_before = env.token_amount(env.vault);
    let landing_slot = EXPIRY_SLOT + u64::from(late);
    env.warp_to_slot(landing_slot);

    shared_lien_suffix_step(
        &format!("{label} rejected pre-normalization refill"),
        env,
        Some(21),
        |env| env.top_up_backing_bucket(domain as u16, REFILL_ATOMS + 1, NEXT_EXPIRY_SLOT),
    )?;

    let winning_mark = if winner_long { 105 } else { 95 };
    let adverse_mark = if winner_long { 95 } else { 105 };
    for (asset, mark) in [(0, winning_mark), (1, adverse_mark), (2, adverse_mark)] {
        shared_lien_suffix_step(&format!("{label} refresh mark {asset}"), env, None, |env| {
            env.push_auth_mark(asset, landing_slot, mark)
        })?;
    }
    let observations = [0u16, 1, 2]
        .into_iter()
        .map(|asset_index| CrankObservationHint {
            asset_index,
            oracle_accounts: env.primary_profile(asset_index as usize).oracle_leg_count,
        })
        .collect::<Vec<_>>();
    let mut normalization_steps = 0usize;
    while {
        let group = env.primary_market_state().1;
        group.source_backing_buckets[domain].status == BackingBucketStatusV16::Fresh
            || group.source_credit[domain].valid_liened_backing_num != 0
    } {
        if normalization_steps == 16 {
            return Err(format!("{label} shared expiry did not normalize boundedly"));
        }
        let actor = actor_order[normalization_steps % actor_order.len()];
        shared_lien_suffix_step(
            &format!("{label} normalization step {normalization_steps} actor {actor}"),
            env,
            None,
            |env| env.crank(actor, landing_slot, observations.clone()),
        )?;
        normalization_steps += 1;
    }
    if normalization_steps == 0 {
        return Err(format!("{label} shared expiry normalization was vacuous"));
    }

    let normalized = env.primary_market_state().1;
    let normalized_source = normalized.source_credit[domain];
    let normalized_bucket = normalized.source_backing_buckets[domain];
    if normalized_bucket.status == BackingBucketStatusV16::Fresh
        || normalized_bucket.fresh_unliened_backing_num != 0
        || normalized_source.fresh_reserved_backing_num != 0
        || normalized_bucket.valid_liened_backing_num != 0
        || normalized_source.valid_liened_backing_num != 0
        || normalized_bucket.impaired_liened_backing_num != valid_before
        || normalized_source.impaired_liened_backing_num != valid_before
    {
        return Err(format!(
            "{label} shared expiry did not impair each old lien exactly once: local={:?}, source={normalized_source:?}, bucket={normalized_bucket:?}",
            actor_order.map(|actor| counterparty_lien_backing(env, actor, domain))
        ));
    }

    let direction = if winner_long { 1 } else { -1 };
    let mut impaired_remaining = valid_before;
    for (order_index, actor) in actor_order.into_iter().enumerate() {
        let sibling = actor_order[1 - order_index];
        let sibling_before = env.primary_portfolio_data(sibling);
        let actor_lien = counterparty_lien_backing(env, actor, domain);
        let adverse_q =
            10 * POS_SCALE as i128 + accepted_increments[actor] as i128 * (POS_SCALE / 10) as i128;
        for (asset, size, price) in [
            (1 + actor as u16, adverse_q, adverse_mark),
            (0, 20 * POS_SCALE as i128, winning_mark),
        ] {
            shared_lien_suffix_step(
                &format!("{label} owner {actor} risk reduction asset {asset}"),
                env,
                None,
                |env| {
                    execute_trade_route(
                        env,
                        TradeRoute::NoCpi,
                        actor,
                        actor + 2,
                        asset,
                        -direction * size,
                        price,
                        0,
                    )
                },
            )?;
        }
        if !percolator::active_bitmap_is_empty(
            env.primary_portfolio(actor)
                .active_bitmap
                .map(|word| word.get()),
        ) {
            return Err(format!(
                "{label} owner {actor} did not flatten after impairment"
            ));
        }
        let mut release_steps = 0usize;
        while counterparty_lien_backing(env, actor, domain) != 0 {
            if release_steps == 8 {
                return Err(format!(
                    "{label} owner {actor} impaired lien did not release boundedly"
                ));
            }
            shared_lien_suffix_step(
                &format!("{label} owner {actor} impaired release {release_steps}"),
                env,
                None,
                |env| env.crank(actor, landing_slot, observations.clone()),
            )?;
            release_steps += 1;
        }
        if release_steps == 0 {
            return Err(format!(
                "{label} owner {actor} impaired release was vacuous"
            ));
        }
        impaired_remaining = impaired_remaining
            .checked_sub(actor_lien)
            .ok_or_else(|| format!("{label} owner {actor} impaired lien exceeded aggregate"))?;
        let group = env.primary_market_state().1;
        let source = group.source_credit[domain];
        let bucket = group.source_backing_buckets[domain];
        if source.valid_liened_backing_num != 0
            || bucket.valid_liened_backing_num != 0
            || source.impaired_liened_backing_num != impaired_remaining
            || bucket.impaired_liened_backing_num != impaired_remaining
            || (order_index == 0 && env.primary_portfolio_data(sibling) != sibling_before)
            || (order_index == 0
                && counterparty_lien_backing(env, sibling, domain) != local_liens[1])
        {
            return Err(format!(
                "{label} owner {actor} release changed sibling attribution: remaining={impaired_remaining}, source={source:?}, bucket={bucket:?}"
            ));
        }
        if order_index == 0 {
            shared_lien_suffix_step(
                &format!("{label} rejected refill with sibling impairment"),
                env,
                Some(21),
                |env| env.top_up_backing_bucket(domain as u16, REFILL_ATOMS + 2, NEXT_EXPIRY_SLOT),
            )?;
        }
    }

    let parts: &[u128] = if split_refill { &[17, 20] } else { &[37] };
    let mut refilled = 0u128;
    for &amount in parts {
        shared_lien_suffix_step(
            &format!("{label} refill amount {amount}"),
            env,
            None,
            |env| env.top_up_backing_bucket(domain as u16, amount, NEXT_EXPIRY_SLOT),
        )?;
        refilled += amount;
        let group = env.primary_market_state().1;
        let source = group.source_credit[domain];
        let bucket = group.source_backing_buckets[domain];
        if bucket.status != BackingBucketStatusV16::Fresh
            || bucket.expiry_slot != NEXT_EXPIRY_SLOT
            || bucket.fresh_unliened_backing_num != refilled * BOUND_SCALE
            || source.fresh_reserved_backing_num != refilled * BOUND_SCALE
            || bucket.valid_liened_backing_num != 0
            || source.valid_liened_backing_num != 0
            || bucket.impaired_liened_backing_num != 0
            || source.impaired_liened_backing_num != 0
            || actor_order
                .iter()
                .any(|actor| counterparty_lien_backing(env, *actor, domain) != 0)
            || env.token_amount(env.provider_source_token)
                != provider_before - u64::try_from(refilled).expect("bounded refill")
            || env.token_amount(env.vault)
                != vault_before + u64::try_from(refilled).expect("bounded refill")
        {
            return Err(format!(
                "{label} expiry/refill did not preserve shared ownership: refilled={refilled}, local={:?}, source={source:?}, bucket={bucket:?}",
                actor_order.map(|actor| counterparty_lien_backing(env, actor, domain))
            ));
        }
    }
    if refilled != REFILL_ATOMS {
        return Err(format!("{label} refill partition totaled {refilled}"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SharedLienOwnerValue {
    capital: u128,
    pnl: i128,
    destination: u64,
    face: u128,
    lien: u128,
}

fn shared_lien_owner_values(
    env: &V16Svm,
    domain: usize,
) -> [SharedLienOwnerValue; PRIMARY_ACTOR_COUNT] {
    std::array::from_fn(|actor| {
        let account = env.primary_portfolio(actor);
        let local = account
            .source_domains
            .iter()
            .find(|entry| entry.is_occupied() && entry.domain.get() as usize == domain);
        SharedLienOwnerValue {
            capital: account.capital.get(),
            pnl: account.pnl.get(),
            destination: env.token_amount(env.actors[actor].destination_token),
            face: local
                .map(|entry| entry.source_claim_bound_num.get())
                .unwrap_or(0),
            lien: counterparty_lien_backing(env, actor, domain),
        }
    })
}

struct SharedLienSuffixOracle {
    domain: usize,
    capital: [u128; PRIMARY_ACTOR_COUNT],
    pnl: [i128; PRIMARY_ACTOR_COUNT],
    paid: [u64; PRIMARY_ACTOR_COUNT],
    liens: [u128; 2],
    fresh: u128,
    spent: u128,
    receivable: u128,
    refill: u64,
    source_tokens: [u64; PRIMARY_ACTOR_COUNT],
    destination_tokens: [u64; PRIMARY_ACTOR_COUNT],
    provider_tokens: u64,
    vault: u64,
    foreign_market: Vec<u8>,
    foreign_portfolio: Vec<u8>,
    unrelated_portfolio: Vec<u8>,
    untouched_owner: (usize, Vec<u8>),
}

impl SharedLienSuffixOracle {
    fn owners_match(&self, observed: &[SharedLienOwnerValue; PRIMARY_ACTOR_COUNT]) -> bool {
        observed.iter().enumerate().all(|(actor, value)| {
            *value
                == SharedLienOwnerValue {
                    capital: self.capital[actor],
                    pnl: self.pnl[actor],
                    destination: self.destination_tokens[actor] + self.paid[actor],
                    face: if actor < 2 {
                        self.pnl[actor] as u128 * percolator::BOUND_SCALE
                    } else {
                        0
                    },
                    lien: if actor < 2 { self.liens[actor] } else { 0 },
                }
        })
    }

    fn check(&self, label: &str, env: &V16Svm) -> Result<(), String> {
        use percolator::BOUND_SCALE;
        let group = env.primary_market_state().1;
        let source = group.source_credit[self.domain];
        let bucket = group.source_backing_buckets[self.domain];
        let owners = shared_lien_owner_values(env, self.domain);
        if !self.owners_match(&owners) {
            return Err(format!(
                "{label} owner claim/lien/value attribution: {owners:?}"
            ));
        }
        for actor in 0..PRIMARY_ACTOR_COUNT {
            if env.token_amount(env.actors[actor].source_token) != self.source_tokens[actor] {
                return Err(format!("{label} actor {actor} source tokens changed"));
            }
        }
        let lien_total: u128 = self.liens.iter().sum();
        let face = (self.pnl[0] + self.pnl[1]) as u128 * BOUND_SCALE;
        if source.positive_claim_bound_num != face
            || source.exact_positive_claim_num != face
            || source.fresh_reserved_backing_num != self.fresh * BOUND_SCALE
            || source.spent_backing_num != self.spent * BOUND_SCALE
            || source.provider_receivable_num != self.receivable * BOUND_SCALE
            || source.valid_liened_backing_num != lien_total
            || bucket.valid_liened_backing_num != lien_total
            || bucket.fresh_unliened_backing_num != self.fresh * BOUND_SCALE - lien_total
            || bucket.consumed_liened_backing_num != self.receivable * BOUND_SCALE
            || source.impaired_liened_backing_num != 0
            || bucket.impaired_liened_backing_num != 0
            || source.insurance_credit_reserved_num != 0
            || source.valid_liened_insurance_num != 0
            || source.impaired_liened_insurance_num != 0
            || self.fresh + self.spent != 212 + u128::from(self.refill)
        {
            return Err(format!(
                "{label} shared backing attribution: {source:?}, {bucket:?}"
            ));
        }
        let paid: u64 = self.paid.iter().sum();
        if env.token_amount(env.provider_source_token) != self.provider_tokens - self.refill
            || env.token_amount(env.vault) != self.vault + self.refill - paid
            || group.vault != u128::from(self.vault + self.refill - paid)
            || env.market_data(true) != self.foreign_market
            || env.foreign_portfolio_data() != self.foreign_portfolio
            || env.primary_portfolio_data(4) != self.unrelated_portfolio
            || env.primary_portfolio_data(self.untouched_owner.0) != self.untouched_owner.1
        {
            return Err(format!("{label} custody/provider/unrelated frame changed"));
        }
        assert_inv_031_censuses(label, env)
    }
}

fn shared_lien_suffix_step(
    label: &str,
    env: &mut V16Svm,
    expected_error: Option<u32>,
    action: impl FnOnce(&mut V16Svm) -> Result<TxSuccess, String>,
) -> Result<(), String> {
    let before = economic_snapshot(env);
    let before_group = env.primary_market_state().1;
    match (action(env), expected_error) {
        (Ok(_), None) => {
            if economic_snapshot(env) == before {
                return Err(format!("{label} successful suffix transaction was a no-op"));
            }
        }
        (Err(error), Some(code)) if error.contains(&format!("Custom({code})")) => {
            if economic_snapshot(env) != before {
                return Err(format!(
                    "{label} rejection did not restore every economic account"
                ));
            }
        }
        (result, expected) => {
            return Err(format!(
                "{label} expected error {expected:?}, got {result:?}"
            ));
        }
    }
    let group = env.primary_market_state().1;
    assert_inv_031_censuses(label, env)?;
    assert_source_credit_rates(label, &group)?;
    assert_source_credit_rate_transition(label, &before_group, &group)
}

fn release_shared_lien_owner(
    label: &str,
    env: &mut V16Svm,
    oracle: &mut SharedLienSuffixOracle,
    actor: usize,
    winner_long: bool,
    accepted_increments: u128,
) -> Result<usize, String> {
    let direction = if winner_long { 1 } else { -1 };
    let winning_mark = if winner_long { 105 } else { 95 };
    let adverse_mark = if winner_long { 95 } else { 105 };
    let adverse_q = 10 * POS_SCALE as i128 + accepted_increments as i128 * (POS_SCALE / 10) as i128;
    for (asset, size, price) in [
        (1 + actor as u16, adverse_q, adverse_mark),
        (0, 20 * POS_SCALE as i128, winning_mark),
    ] {
        shared_lien_suffix_step(label, env, None, |env| {
            execute_trade_route(
                env,
                TradeRoute::NoCpi,
                actor,
                actor + 2,
                asset,
                -direction * size,
                price,
                0,
            )
        })?;
        oracle.check(label, env)?;
    }
    if !percolator::active_bitmap_is_empty(
        env.primary_portfolio(actor).active_bitmap.map(|w| w.get()),
    ) {
        return Err(format!(
            "{label} owner {actor} did not flatten before release"
        ));
    }
    let mut steps = 2;
    for _ in 0..8 {
        if oracle.liens[actor] == 0 && portfolio_certificate_is_current(env, actor) {
            return Ok(steps);
        }
        shared_lien_suffix_step(label, env, None, |env| env.crank(actor, 2, vec![]))?;
        steps += 1;
        // The bounded selector may refresh before releasing. Only complete release of this
        // flat owner's checkpoint lien is allowed; the sibling's ledger remains unchanged.
        let remaining = counterparty_lien_backing(env, actor, oracle.domain);
        if remaining != 0 && remaining != oracle.liens[actor] {
            return Err(format!(
                "{label} owner {actor} released an inexact lien amount"
            ));
        }
        if remaining == 0 {
            oracle.liens[actor] = 0;
        }
        oracle.check(label, env)?;
    }
    Err(format!(
        "{label} owner {actor} failed bounded release/recertification"
    ))
}

fn verify_shared_lien_partial_consumption_suffix(
    label: &str,
    env: &mut V16Svm,
    winner_long: bool,
    actor_order: [usize; 2],
    accepted_increments: [u128; 2],
    split_refill: bool,
) -> Result<(), String> {
    let domain = usize::from(winner_long);
    // The public prefix earns each winner 100 and debits 50 of disjoint principal.
    // Counterparties each pay 100 of principal and retain a separate 50-atom claim.
    // Thus the shared source owns 12 provider + 200 counterparty backing atoms.
    let mut oracle = SharedLienSuffixOracle {
        domain,
        capital: [313, 313, 900, 900, 1],
        pnl: [100, 100, 50, 50, 0],
        paid: [0; PRIMARY_ACTOR_COUNT],
        liens: [0, 1].map(|actor| counterparty_lien_backing(env, actor, domain)),
        fresh: 212,
        spent: 0,
        receivable: 0,
        refill: 0,
        source_tokens: std::array::from_fn(|actor| {
            env.token_amount(env.actors[actor].source_token)
        }),
        destination_tokens: std::array::from_fn(|actor| {
            env.token_amount(env.actors[actor].destination_token)
        }),
        provider_tokens: env.token_amount(env.provider_source_token),
        vault: env.token_amount(env.vault),
        foreign_market: env.market_data(true),
        foreign_portfolio: env.foreign_portfolio_data(),
        unrelated_portfolio: env.primary_portfolio_data(4),
        untouched_owner: (actor_order[1], env.primary_portfolio_data(actor_order[1])),
    };
    oracle.check(label, env)?;
    let [first, sibling] = actor_order;
    let mut steps = release_shared_lien_owner(
        label,
        env,
        &mut oracle,
        first,
        winner_long,
        accepted_increments[first],
    )?;
    let retained_before_refill = env.build_retained_convert_released_pnl(first, 100);
    let retained_after_refill = env.build_retained_convert_released_pnl(first, 100);
    assert_ne!(
        retained_before_refill.signatures,
        retained_after_refill.signatures
    );
    shared_lien_suffix_step(label, env, None, |env| env.convert_released_pnl(first, 100))?;
    oracle.capital[first] += 100;
    oracle.pnl[first] = 0;
    oracle.fresh -= 100;
    oracle.spent += 100;
    oracle.receivable += 100;
    oracle.check(label, env)?;
    shared_lien_suffix_step(label, env, None, |env| env.withdraw_primary(first, 413))?;
    oracle.capital[first] = 0;
    oracle.paid[first] = 413;
    oracle.check(label, env)?;
    // Mutate observations only, never program state. Each wrong-owner control conserves
    // aggregate value or encumbrance, so a stock-only oracle would miss the attribution error.
    let observed = shared_lien_owner_values(env, domain);
    let mut wrong = observed;
    wrong[sibling].capital -= 1;
    wrong[first].capital += 1;
    assert!(
        !oracle.owners_match(&wrong),
        "{label} accepted wrong-owner capital"
    );
    wrong = observed;
    wrong[first].destination -= 1;
    wrong[sibling].destination += 1;
    assert!(
        !oracle.owners_match(&wrong),
        "{label} accepted wrong-owner payout"
    );
    wrong = observed;
    wrong[first].lien = wrong[sibling].lien;
    wrong[sibling].lien = 0;
    assert!(
        !oracle.owners_match(&wrong),
        "{label} accepted wrong-owner lien"
    );
    shared_lien_suffix_step(label, env, Some(16), |env| {
        env.land_retained(retained_before_refill)
    })?;
    oracle.check(label, env)?;
    steps += 3;

    let parts: &[u128] = if split_refill { &[17, 20] } else { &[37] };
    for &amount in parts {
        shared_lien_suffix_step(label, env, None, |env| {
            env.top_up_backing_bucket(domain as u16, amount, 100)
        })?;
        oracle.fresh += amount;
        oracle.receivable -= amount;
        oracle.refill += amount as u64;
        oracle.check(label, env)?;
        steps += 1;
    }
    shared_lien_suffix_step(label, env, Some(16), |env| {
        env.land_retained(retained_after_refill)
    })?;
    oracle.check(label, env)?;
    steps += 1;
    if oracle.liens[sibling] == 0 {
        return Err(format!(
            "{label} first owner/refill/retries changed the live sibling"
        ));
    }

    oracle.untouched_owner = (first, env.primary_portfolio_data(first));
    steps += release_shared_lien_owner(
        label,
        env,
        &mut oracle,
        sibling,
        winner_long,
        accepted_increments[sibling],
    )?;
    shared_lien_suffix_step(label, env, Some(21), |env| {
        env.convert_released_pnl(sibling, 99)
    })?;
    oracle.check(label, env)?;
    shared_lien_suffix_step(label, env, None, |env| {
        env.convert_released_pnl(sibling, 100)
    })?;
    oracle.capital[sibling] += 100;
    oracle.pnl[sibling] = 0;
    oracle.fresh -= 100;
    oracle.spent += 100;
    oracle.receivable += 100;
    oracle.check(label, env)?;
    shared_lien_suffix_step(label, env, None, |env| env.withdraw_primary(sibling, 413))?;
    oracle.capital[sibling] = 0;
    oracle.paid[sibling] = 413;
    oracle.check(label, env)?;
    steps += 3;
    if oracle.liens != [0, 0]
        || oracle.fresh != 49
        || oracle.spent != 200
        || oracle.receivable != 163
        || oracle.paid[..2] != [413, 413]
    {
        return Err(format!(
            "{label} shared consumption history did not complete"
        ));
    }
    eprintln!("{label}: {steps} checked suffix transactions, 3 exact rejections, 2 owner payouts");
    Ok(())
}

#[test]
fn v16_program_shared_lien_partial_consumption_retries_preserve_sibling_claim() {
    for reverse_order in [false, true] {
        for winner_long in [false, true] {
            for split_refill in [false, true] {
                verify_two_account_concurrent_lien_ownership(
                    TradeRoute::NoCpi,
                    reverse_order,
                    winner_long,
                    ConcurrentLienSuffix::PartialConsumption { split_refill },
                )
                .unwrap_or_else(|error| panic!("{error}"));
            }
        }
    }
}

#[test]
fn v16_program_shared_lien_expiry_refill_preserves_owner_attribution() {
    for late in [false, true] {
        for reverse_order in [false, true] {
            for winner_long in [false, true] {
                for split_refill in [false, true] {
                    verify_two_account_concurrent_lien_ownership(
                        TradeRoute::NoCpi,
                        reverse_order,
                        winner_long,
                        ConcurrentLienSuffix::ExpiryRefill { late, split_refill },
                    )
                    .unwrap_or_else(|error| panic!("{error}"));
                }
            }
        }
    }
}

fn verify_haircut_conversion_retry(route: TradeRoute, seed_tag: u8) -> Result<(), String> {
    const WINNER: usize = 0;
    const OPEN_COUNTERPARTY: usize = 1;
    const CLOSE_COUNTERPARTY: usize = 2;
    const ASSET: u16 = 0;
    const SOURCE_DOMAIN: usize = 1;
    const START_PRICE: u64 = 100;
    const SETTLED_PRICE: u64 = 150;
    const POSITION_Q: i128 = 40 * POS_SCALE as i128;
    const DEPOSIT: u128 = 1_000;
    const CLAIM_ATOMS: u128 = 2_000;
    const BACKING_TRANCHE_ATOMS: u128 = 1_000;

    let mut seed = [0x31; 32];
    seed[0] = seed_tag;
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: START_PRICE,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [DEPOSIT; PRIMARY_ACTOR_COUNT],
            ..MarketConfig::default()
        },
    );
    let label = format!("INV-031 {route:?}");
    let supply_before = env.token_supply_observed();
    let loser_capital_before = env.primary_portfolio(OPEN_COUNTERPARTY).capital.get();
    let unrelated_account_before = env.primary_portfolio_data(3);
    let unrelated_source_before = env.token_amount(env.actors[3].source_token);
    let unrelated_destination_before = env.token_amount(env.actors[3].destination_token);
    let winner_destination_before = env.token_amount(env.actors[WINNER].destination_token);
    env.begin_public_trace();

    execute_trade_route(
        &mut env,
        route,
        WINNER,
        OPEN_COUNTERPARTY,
        ASSET,
        POSITION_Q,
        START_PRICE,
        0,
    )
    .map_err(|error| format!("{label} open claim-bearing position: {error}"))?;
    // Repeated bounded marks can move farther than initial margin while the losing
    // side remains in the historical cohort. The winner settles each generation;
    // the loser settles once at the end, contributing only its finite capital as
    // source backing and leaving a genuinely half-backed positive claim.
    for (offset, price) in (105..=SETTLED_PRICE).step_by(5).enumerate() {
        let slot = 2 + offset as u64;
        env.warp_to_slot(slot);
        env.push_auth_mark(ASSET, slot, price)
            .map_err(|error| format!("{label} authenticate favorable mark {price}: {error}"))?;
        env.crank(
            WINNER,
            slot,
            vec![CrankObservationHint {
                asset_index: ASSET,
                oracle_accounts: env.primary_profile(ASSET as usize).oracle_leg_count,
            }],
        )
        .map_err(|error| format!("{label} settle winner at mark {price}: {error}"))?;
    }
    let settlement_slot = 1 + ((SETTLED_PRICE - START_PRICE) / 5);
    // The favorable move created a K/F settlement cohort containing both original
    // counterparties. A fresh account must not inherit the unsettled loser's debit,
    // so discharge that cohort through the sole public crank before novating risk.
    env.crank(
        OPEN_COUNTERPARTY,
        settlement_slot,
        vec![CrankObservationHint {
            asset_index: ASSET,
            oracle_accounts: env.primary_profile(ASSET as usize).oracle_leg_count,
        }],
    )
    .map_err(|error| format!("{label} settle original counterparty cohort: {error}"))?;
    execute_trade_route(
        &mut env,
        route,
        WINNER,
        CLOSE_COUNTERPARTY,
        ASSET,
        -POSITION_Q,
        SETTLED_PRICE,
        0,
    )
    .map_err(|error| format!("{label} flatten winner: {error}"))?;

    let before_first_conversion = env.primary_market_state().1;
    if env.primary_portfolio(WINNER).pnl.get() != CLAIM_ATOMS as i128
        || before_first_conversion.source_credit[SOURCE_DOMAIN].positive_claim_bound_num
            != CLAIM_ATOMS * percolator::BOUND_SCALE
        || before_first_conversion.source_credit[SOURCE_DOMAIN].fresh_reserved_backing_num
            != BACKING_TRANCHE_ATOMS * percolator::BOUND_SCALE
    {
        return Err(format!(
            "{label} did not create one claim over one half-sized backing tranche: pnl={}, source={:?}",
            env.primary_portfolio(WINNER).pnl.get(),
            before_first_conversion.source_credit[SOURCE_DOMAIN],
        ));
    }

    // The 50% source-credit rate pays one backing atom for every two claim atoms. A successful
    // conversion therefore consumes the only backing tranche and burns the complete claim face.
    // First force the wrapper's post-conversion cap check to fail by one atom. The engine call
    // precedes that check, so exact transaction rollback is what prevents partial claim/backing
    // consumption. The identical state then succeeds with the exact cap.
    let before_undersized_cap = economic_snapshot(&env);
    let undersized_error = env
        .convert_released_pnl(WINNER, BACKING_TRANCHE_ATOMS - 1)
        .expect_err("undersized conversion cap must reject after computing the full conversion");
    if !undersized_error.contains("Custom(21)")
        && !undersized_error.contains("custom program error: 0x15")
    {
        return Err(format!(
            "{label} undersized conversion cap rejected for an unrelated reason: {undersized_error}"
        ));
    }
    if economic_snapshot(&env) != before_undersized_cap {
        return Err(format!(
            "{label} undersized post-conversion cap committed partial claim/backing consumption"
        ));
    }
    assert_inv_031_censuses(
        &format!("{label} after undersized conversion rollback"),
        &env,
    )?;

    // The retained request must not reuse either class after the full conversion lands.
    let retained_retry = env.build_retained_convert_released_pnl(WINNER, BACKING_TRANCHE_ATOMS);
    env.convert_released_pnl(WINNER, BACKING_TRANCHE_ATOMS)
        .map_err(|error| format!("{label} first conversion: {error}"))?;
    let after_first_conversion = env.primary_market_state().1;
    if env.primary_portfolio(WINNER).capital.get() != DEPOSIT + BACKING_TRANCHE_ATOMS
        || env.primary_portfolio(WINNER).pnl.get() != 0
        || after_first_conversion.source_backing_buckets[SOURCE_DOMAIN].consumed_liened_backing_num
            != BACKING_TRANCHE_ATOMS * percolator::BOUND_SCALE
        || after_first_conversion.source_credit[SOURCE_DOMAIN].fresh_reserved_backing_num != 0
        || after_first_conversion.source_credit[SOURCE_DOMAIN].positive_claim_bound_num != 0
    {
        return Err(format!(
            "{label} first conversion classification mismatch: capital={}, pnl={}, claim={}, fresh={}, consumed={}",
            env.primary_portfolio(WINNER).capital.get(),
            env.primary_portfolio(WINNER).pnl.get(),
            after_first_conversion.source_credit[SOURCE_DOMAIN].positive_claim_bound_num,
            after_first_conversion.source_credit[SOURCE_DOMAIN].fresh_reserved_backing_num,
            after_first_conversion.source_backing_buckets[SOURCE_DOMAIN].consumed_liened_backing_num,
        ));
    }

    let loser_capital_after = env.primary_portfolio(OPEN_COUNTERPARTY).capital.get();
    let losing_episode_debit = loser_capital_before
        .checked_sub(loser_capital_after)
        .ok_or_else(|| format!("{label} losing episode increased loser capital"))?;
    env.withdraw_primary(WINNER, BACKING_TRANCHE_ATOMS)
        .map_err(|error| format!("{label} withdraw converted backing: {error}"))?;
    let winner_payout = env
        .token_amount(env.actors[WINNER].destination_token)
        .checked_sub(winner_destination_before)
        .ok_or_else(|| format!("{label} winner destination decreased"))?;
    if u128::from(winner_payout) != BACKING_TRANCHE_ATOMS
        || u128::from(winner_payout) != losing_episode_debit
        || env.primary_portfolio_data(3) != unrelated_account_before
        || env.token_amount(env.actors[3].source_token) != unrelated_source_before
        || env.token_amount(env.actors[3].destination_token) != unrelated_destination_before
    {
        return Err(format!(
            "{label} half-backed payout violated principal seniority: payout={winner_payout}, losing_debit={losing_episode_debit}, unrelated_capital={}",
            env.primary_portfolio(3).capital.get()
        ));
    }

    let before_retry = economic_snapshot(&env);
    let retry = env.land_retained(retained_retry);
    let retry_error = match retry {
        Ok(_) => {
            return Err(format!(
                "{label} retained conversion reused a fully consumed backing tranche"
            ))
        }
        Err(error) => error,
    };
    if !retry_error.contains("Custom(16)") {
        return Err(format!(
            "{label} retained conversion did not reject at the consumed position-episode boundary: {retry_error}"
        ));
    }
    if economic_snapshot(&env) != before_retry {
        return Err(format!(
            "{label} rejected retained conversion did not roll back every tracked economic account"
        ));
    }

    env.top_up_backing_bucket(SOURCE_DOMAIN as u16, BACKING_TRANCHE_ATOMS, 100)
        .map_err(|error| format!("{label} independent replacement backing: {error}"))?;
    let before_fresh_retry = economic_snapshot(&env);
    let fresh_retry_error = match env.convert_released_pnl(WINNER, BACKING_TRANCHE_ATOMS) {
        Ok(_) => {
            return Err(format!(
                "{label} fresh backing revived an already-consumed claim face"
            ))
        }
        Err(error) => error,
    };
    if !fresh_retry_error.contains("Custom(19)") {
        return Err(format!(
            "{label} fresh retry did not reach the program's LockActive rejection: {fresh_retry_error}"
        ));
    }
    if economic_snapshot(&env) != before_fresh_retry {
        return Err(format!(
            "{label} rejected fresh retry did not roll back every tracked economic account"
        ));
    }
    let terminal = env.primary_market_state().1;
    if env.primary_portfolio(WINNER).capital.get() != DEPOSIT
        || env.primary_portfolio(WINNER).pnl.get() != 0
        || terminal.source_credit[SOURCE_DOMAIN].spent_backing_num
            != BACKING_TRANCHE_ATOMS * percolator::BOUND_SCALE
        || terminal.source_credit[SOURCE_DOMAIN].fresh_reserved_backing_num
            != BACKING_TRANCHE_ATOMS * percolator::BOUND_SCALE
        || terminal.source_credit[SOURCE_DOMAIN].positive_claim_bound_num != 0
        || terminal.source_backing_buckets[SOURCE_DOMAIN].consumed_liened_backing_num != 0
    {
        return Err(format!(
            "{label} replacement backing changed consumed-claim ownership: capital={}, pnl={}, claim={}, fresh={}, spent={}, live-bucket-consumed={}",
            env.primary_portfolio(WINNER).capital.get(),
            env.primary_portfolio(WINNER).pnl.get(),
            terminal.source_credit[SOURCE_DOMAIN].positive_claim_bound_num,
            terminal.source_credit[SOURCE_DOMAIN].fresh_reserved_backing_num,
            terminal.source_credit[SOURCE_DOMAIN].spent_backing_num,
            terminal.source_backing_buckets[SOURCE_DOMAIN].consumed_liened_backing_num,
        ));
    }
    if env.primary_portfolio_data(3) != unrelated_account_before
        || env.token_amount(env.actors[3].source_token) != unrelated_source_before
        || env.token_amount(env.actors[3].destination_token) != unrelated_destination_before
        || env.token_supply_observed() != supply_before
    {
        return Err(format!(
            "{label} replacement backing or retries changed unrelated principal or token supply"
        ));
    }

    let trace = env.finish_public_trace();
    trace
        .validate_public_execution()
        .expect("claim/backing retry trace must be public and rollback-exact");
    let rejected = trace
        .steps
        .iter()
        .filter(|step| !step.succeeded)
        .collect::<Vec<_>>();
    if trace.out_of_band_economic_mutations != 0
        || rejected.len() != 3
        || rejected.iter().any(|step| {
            step.program_id != env.program_id
                || step.rejected_exact_writable_rollback != Some(true)
                || step.rejected_no_program_lamport_delta != Some(true)
                || step.token_deltas.iter().any(|(_, delta)| *delta != 0)
        })
    {
        return Err(format!(
            "{label} public trace did not prove all three exact rollbacks without out-of-band mutation: {trace:?}"
        ));
    }
    Ok(())
}

#[test]
fn v16_program_haircut_conversion_retries_cannot_reuse_claim_or_backing() {
    for (seed_tag, route) in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ]
    .into_iter()
    .enumerate()
    {
        verify_haircut_conversion_retry(route, 0x31 ^ seed_tag as u8)
            .unwrap_or_else(|error| panic!("{error}"));
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_031_cross_domain_backing_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_two_source_claims_preserve_source_backing_single_use(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_cross_domain_backing_single_use(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert!(
            discovery.preserves_single_use(),
            "source backing attribution or retry safety failed: {:?}",
            discovery
        );
        prop_assert_eq!(discovery.victim_loss_atoms, 0);
        prop_assert_eq!(discovery.unauthorized_gain_atoms, 0);
        let progressing_or_bounded = matches!(
            discovery.terminal_classification,
            crate::support::v16_svm::PublicTerminalClassification::Progressing
                | crate::support::v16_svm::PublicTerminalClassification::BoundedExit
        );
        prop_assert!(progressing_or_bounded);
    }
}
