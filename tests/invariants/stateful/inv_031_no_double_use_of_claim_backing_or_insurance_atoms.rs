//! INV-031 - No double use of claim, backing, or insurance atoms.
//!
//! Normative obligation: A backing or claim atom cannot support two economic obligations.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_two_source_claims_discover_backing_double_consume` creates equal positive claims
//! in an unfunded and an overfunded source domain, then partitions aggregate conversion. The
//! independent ledger oracle requires each claim to consume only its own source backing and checks
//! the final SPL extraction. Direct impact tests remain below. These tests exercise the deployed public
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
//! The haircut-conversion matrix also submits a cap one atom below the independently known
//! conversion amount. The deployed handler reaches its post-conversion cap rejection, and SVM
//! rollback must restore the claim, backing bucket, portfolio, custody, and every auxiliary
//! account before the identical full-cap request consumes the tranche exactly once.
//! The same trace is reused by INV-027: the externally withdrawn tranche must equal the original
//! loser's principal debit while a separately funded portfolio remains byte- and SPL-exact.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;
use crate::support::{
    fuzz_model::{
        assert_public_encumbrance_census, assert_public_stock_census, execute_trade_route,
    },
    v16_svm::{MarketConfig, V16Svm, PRIMARY_ACTOR_COUNT},
};
use percolator::{BackingBucketStatusV16, POS_SCALE};
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
            actor_deposits: [313, 1_000, 1, 1, 1],
            actor_token_balances: [313, 1_000, 1, 1, 1],
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
    if env.primary_portfolio(WINNER).pnl.get() != 50 {
        return Err(format!(
            "{label} did not create the expected source-backed claim: {}",
            env.primary_portfolio(WINNER).pnl.get()
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

fn verify_two_account_concurrent_lien_ownership(
    route: TradeRoute,
    reverse_order: bool,
    winner_long: bool,
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
        "INV-031 concurrent route={route:?} reverse={reverse_order} winner_long={winner_long}"
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
            actor_deposits: [313, 313, 1_000, 1_000, 1],
            actor_token_balances: [313, 313, 1_000, 1_000, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.begin_public_trace();
    env.top_up_backing_bucket(source_domain as u16, BACKING_ATOMS, 100)
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
    for winner in WINNERS {
        if env.primary_portfolio(winner).pnl.get() != 50 {
            return Err(format!(
                "{label} winner {winner} did not create the expected source-backed claim: {}",
                env.primary_portfolio(winner).pnl.get()
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
        || released.source_backing_buckets[source_domain].status != BackingBucketStatusV16::Fresh
    {
        return Err(format!(
            "{label} did not release the exact shared backing ownership: steps={release_steps}, local={final_local_total}, source={:?}, bucket={:?}",
            released.source_credit[source_domain],
            released.source_backing_buckets[source_domain]
        ));
    }
    assert_inv_031_censuses(&format!("{label} after concurrent release"), &env)?;
    if env.token_supply_observed() != supply_before {
        return Err(format!("{label} changed SPL token supply"));
    }
    let trace = env.finish_public_trace();
    trace
        .validate_public_execution()
        .map_err(|error| format!("{label} invalid public trace: {error}"))?;
    if trace.out_of_band_economic_mutations != 0
        || trace.steps.iter().filter(|step| !step.succeeded).count() != 2
    {
        return Err(format!(
            "{label} did not isolate the two exact shared-frontier rejections: {trace:?}"
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
                verify_two_account_concurrent_lien_ownership(route, reverse_order, winner_long)
                    .unwrap_or_else(|error| panic!("{error}"));
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
    fn v16_program_two_source_claims_discover_backing_double_consume(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_cross_domain_backing_violation(seed)
            .map_err(TestCaseError::fail)?;
        eprintln!("independent cross-domain backing discovery: {discovery:?}");
        prop_assert!(
            discovery.is_violation(),
            "vulnerable-pin source backing attribution changed: {:?}",
            discovery
        );
        prop_assert_eq!(discovery.victim_loss_atoms, 100);
        prop_assert_eq!(discovery.unauthorized_gain_atoms, 100);
        let exact_terminal_loss = matches!(
            discovery.terminal_classification,
            crate::support::v16_svm::PublicTerminalClassification::LossOfFunds {
                victim_loss_atoms: 100,
                unauthorized_gain_atoms: 100,
            }
        );
        prop_assert!(exact_terminal_loss);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/v16_program_stateful_fuzz.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_pr267_cross_domain_backing_double_spend_fuzz(
        seed in cross_domain_backing_seed_strategy()
    ) {
        let result = reproduce_cross_domain_backing_double_spend(seed);
        prop_assert!(
            result.is_ok(),
            "PR 267 no longer reproduces for seed {:?}: {}",
            seed,
            result.unwrap_err()
        );
    }
}
