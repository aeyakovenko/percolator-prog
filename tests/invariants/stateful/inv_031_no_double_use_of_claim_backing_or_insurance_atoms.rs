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
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;
use crate::support::{
    fuzz_model::execute_trade_route,
    v16_svm::{MarketConfig, V16Svm, PRIMARY_ACTOR_COUNT},
};
use percolator::POS_SCALE;
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
    // The retained request must not reuse either class after it lands out of order.
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
    if env.primary_portfolio(WINNER).capital.get() != DEPOSIT + BACKING_TRANCHE_ATOMS
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
        || rejected.len() != 2
        || rejected.iter().any(|step| {
            step.program_id != env.program_id
                || step.rejected_exact_writable_rollback != Some(true)
                || step.rejected_no_program_lamport_delta != Some(true)
                || step.token_deltas.iter().any(|(_, delta)| *delta != 0)
        })
    {
        return Err(format!(
            "{label} public trace did not prove both exact rollbacks without out-of-band mutation: {trace:?}"
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
