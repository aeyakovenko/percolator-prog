//! INV-047 - Equivalent-route semantics.
//!
//! Normative obligation: the same authorized economic intent must produce the same normalized
//! state delta through CPI/no-CPI and single/batch public trade routes. Route-specific matcher
//! transport state may differ, but protocol value, positions, OI, fees, custody, and unrelated
//! state may not.
//!
//! Evidence (generated F over public I routes with M comparison): each case creates four identical
//! public LiteSVM worlds, installs the same LP-consented nonzero market base fee, and executes one
//! trade through `TradeNoCpi`, `TradeCpi`, `BatchTradeNoCpi`, and `BatchTradeCpi`. The comparison is
//! byte-exact for both markets, every portfolio, the backing ledger, every SPL account, economic
//! lamports, and token supply after normalizing three documented transport/capability differences:
//! the CPI-only matcher request sequence, the single-CPI matcher's 64-byte ABI return cache, and
//! the LP matcher-enabled bit retained after a matcher-synchronized fill but revoked by a bilateral
//! fill. Matcher tuple, fee cap, position epoch, and every other byte remain exact. The fixed matrix
//! covers minimum, interior, and maximum fee rates; the generated matrix varies seed, size, side,
//! and fee rate.
//!
//! Guarantee boundary: this closes the one-leg trade-route and nonzero-fee partition. It does not
//! establish wrapper/engine transition equivalence for every public instruction or equivalence
//! between unrelated direct and composite lifecycle operations.

use super::env_usize;
use crate::support::{
    fuzz_model::{execute_trade_route, TradeRoute},
    v16_svm::{MarketConfig, V16Svm, INITIAL_PRICE, TX_CU_LIMIT},
};
use percolator::POS_SCALE;
use percolator_prog::state;
use proptest::prelude::*;

const TAKER: usize = 0;
const MAKER: usize = 1;
const MATCHER_RETURN_CACHE_LEN: usize = 64;
const ROUTES: [TradeRoute; 4] = [
    TradeRoute::NoCpi,
    TradeRoute::Cpi,
    TradeRoute::BatchNoCpi,
    TradeRoute::BatchCpi,
];

#[derive(Clone, Debug, PartialEq, Eq)]
struct NormalizedTradeRouteFrame {
    primary_market: Vec<u8>,
    foreign_market: Vec<u8>,
    primary_portfolios: Vec<Vec<u8>>,
    foreign_portfolio: Vec<u8>,
    backing_ledger: Vec<u8>,
    token_accounts: Vec<(solana_sdk::pubkey::Pubkey, Vec<u8>)>,
    matcher_contexts: Vec<Vec<u8>>,
    economic_lamports: Vec<(solana_sdk::pubkey::Pubkey, u64)>,
    token_supply: u128,
}

fn normalized_matcher_contexts(env: &V16Svm) -> Result<Vec<Vec<u8>>, String> {
    let mut contexts = env.all_matcher_context_data();
    let maker = contexts
        .get_mut(MAKER)
        .ok_or("INV-047 missing maker matcher context")?;
    if maker.len() < MATCHER_RETURN_CACHE_LEN {
        return Err(format!(
            "INV-047 matcher context is only {} bytes",
            maker.len()
        ));
    }
    maker[..MATCHER_RETURN_CACHE_LEN].fill(0);
    Ok(contexts)
}

fn normalized_primary_market(env: &V16Svm) -> Result<Vec<u8>, String> {
    let mut market = env.market_data(false);
    let (mut config, _) = state::read_market(&market)
        .map_err(|error| format!("INV-047 decode primary market: {error:?}"))?;
    config.matcher_req_seq = 0;
    state::write_wrapper_config(&mut market, &config)
        .map_err(|error| format!("INV-047 normalize matcher request sequence: {error:?}"))?;
    Ok(market)
}

fn normalized_primary_portfolios(env: &V16Svm) -> Result<Vec<Vec<u8>>, String> {
    env.all_primary_portfolio_data()
        .into_iter()
        .enumerate()
        .map(|(index, mut portfolio)| {
            let mut matcher =
                state::read_portfolio_matcher_config(&portfolio).map_err(|error| {
                    format!("INV-047 decode portfolio {index} matcher config: {error:?}")
                })?;
            matcher.set_enabled(0).map_err(|error| {
                format!("INV-047 normalize portfolio {index} matcher state: {error:?}")
            })?;
            state::write_portfolio_matcher_config(&mut portfolio, &matcher).map_err(|error| {
                format!("INV-047 write portfolio {index} matcher normalization: {error:?}")
            })?;
            Ok(portfolio)
        })
        .collect()
}

fn normalized_trade_route_frame(env: &V16Svm) -> Result<NormalizedTradeRouteFrame, String> {
    Ok(NormalizedTradeRouteFrame {
        primary_market: normalized_primary_market(env)?,
        foreign_market: env.market_data(true),
        primary_portfolios: normalized_primary_portfolios(env)?,
        foreign_portfolio: env.foreign_portfolio_data(),
        backing_ledger: env.backing_domain_ledger_data(),
        token_accounts: env.all_token_account_data(),
        matcher_contexts: normalized_matcher_contexts(env)?,
        economic_lamports: env.all_economic_account_lamports(),
        token_supply: env.token_supply_observed(),
    })
}

fn run_nonzero_fee_route(
    seed: [u8; 32],
    route: TradeRoute,
    lots: u8,
    account_a_long: bool,
    fee_bps: u16,
) -> Result<NormalizedTradeRouteFrame, String> {
    if lots == 0 || fee_bps == 0 || fee_bps > 10_000 {
        return Err("INV-047 generated case is outside the nonzero-fee domain".into());
    }
    let mut env = V16Svm::new(seed, MarketConfig::default());
    env.update_trade_fee_policy(u64::from(fee_bps))
        .map_err(|error| format!("INV-047 install common base fee: {error}"))?;
    let initial_contexts = env.all_matcher_context_data();
    let insurance_before = env.primary_market_state().1.insurance;
    let signed_lots = if account_a_long {
        i128::from(lots)
    } else {
        -i128::from(lots)
    };
    let size_q = signed_lots
        .checked_mul(POS_SCALE as i128)
        .ok_or("INV-047 generated size overflow")?;

    env.begin_public_trace();
    let landed = execute_trade_route(
        &mut env,
        route,
        TAKER,
        MAKER,
        0,
        size_q,
        INITIAL_PRICE,
        u64::from(fee_bps),
    )
    .map_err(|error| format!("INV-047 {route:?} trade failed: {error}"))?;
    if landed.compute_units >= TX_CU_LIMIT {
        return Err(format!(
            "INV-047 {route:?} consumed {} CU",
            landed.compute_units
        ));
    }
    let trace = env.finish_public_trace();
    trace
        .validate_public_execution()
        .map_err(|error| format!("INV-047 {route:?} public trace: {error}"))?;
    if trace.out_of_band_economic_mutations != 0
        || trace.steps.len() != 1
        || !trace.steps[0].succeeded
    {
        return Err(format!(
            "INV-047 {route:?} was not one successful public transition: {trace:?}"
        ));
    }

    let (_, group) = env.primary_market_state();
    if group.insurance <= insurance_before {
        return Err(format!(
            "INV-047 {route:?} nonzero fee did not fund insurance: {}/{}",
            group.insurance, insurance_before
        ));
    }
    if group.vault != group.c_tot + group.insurance
        || u128::from(env.token_amount(env.vault)) != group.vault
    {
        return Err(format!(
            "INV-047 {route:?} custody mismatch: vault={}, c_tot={}, insurance={}, SPL={}",
            group.vault,
            group.c_tot,
            group.insurance,
            env.token_amount(env.vault)
        ));
    }
    let taker = env.primary_portfolio(TAKER);
    let maker = env.primary_portfolio(MAKER);
    let taker_position = taker
        .legs
        .iter()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .find(|leg| leg.active && leg.asset_index == 0)
        .map(|leg| leg.basis_pos_q)
        .ok_or_else(|| format!("INV-047 {route:?} did not create the taker leg"))?;
    let maker_position = maker
        .legs
        .iter()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .find(|leg| leg.active && leg.asset_index == 0)
        .map(|leg| leg.basis_pos_q)
        .ok_or_else(|| format!("INV-047 {route:?} did not create the maker leg"))?;
    if taker_position != size_q || maker_position != -size_q {
        return Err(format!(
            "INV-047 {route:?} position mismatch: {taker_position}/{maker_position}, expected {size_q}/{}",
            -size_q
        ));
    }

    let contexts = env.all_matcher_context_data();
    for (index, (before, after)) in initial_contexts.iter().zip(&contexts).enumerate() {
        if index != MAKER && after != before {
            return Err(format!(
                "INV-047 {route:?} mutated unrelated matcher context {index}"
            ));
        }
        if index == MAKER
            && (after.len() < MATCHER_RETURN_CACHE_LEN
                || after[MATCHER_RETURN_CACHE_LEN..] != before[MATCHER_RETURN_CACHE_LEN..])
        {
            return Err(format!(
                "INV-047 {route:?} mutated matcher state outside the ABI return cache"
            ));
        }
    }

    normalized_trade_route_frame(&env)
}

fn verify_nonzero_fee_route_equivalence(
    seed: [u8; 32],
    lots: u8,
    account_a_long: bool,
    fee_bps: u16,
) -> Result<(), String> {
    let mut expected = None;
    for route in ROUTES {
        let actual = run_nonzero_fee_route(seed, route, lots, account_a_long, fee_bps)?;
        if let Some((expected_route, expected_frame)) = &expected {
            if actual != *expected_frame {
                return Err(format!(
                    "INV-047 {route:?} diverged from {expected_route:?} for lots={lots}, account_a_long={account_a_long}, fee_bps={fee_bps}"
                ));
            }
        } else {
            expected = Some((route, actual));
        }
    }
    Ok(())
}

#[test]
fn v16_program_nonzero_fee_trade_routes_are_byte_exact_after_transport_normalization() {
    for (case, lots, account_a_long, fee_bps) in [
        (0x47, 1, true, 1),
        (0x48, 7, false, 333),
        (0x49, 8, true, 10_000),
    ] {
        verify_nonzero_fee_route_equivalence([case; 32], lots, account_a_long, fee_bps)
            .unwrap_or_else(|error| panic!("fixed INV-047 route matrix failed: {error}"));
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 4) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 32) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_047_nonzero_fee_route_equivalence.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_generated_nonzero_fee_trade_routes_are_economically_equivalent(
        seed in any::<[u8; 32]>(),
        lots in 1u8..=8,
        account_a_long in any::<bool>(),
        fee_bps in 1u16..=10_000,
    ) {
        prop_assert!(
            verify_nonzero_fee_route_equivalence(seed, lots, account_a_long, fee_bps).is_ok(),
            "generated INV-047 route matrix diverged"
        );
    }
}
