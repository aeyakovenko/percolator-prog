//! INV-052 - Split/merge invariance.
//!
//! Normative obligation: Partitioning an authorized operation must not improve or otherwise alter
//! its normalized economic result, except for explicitly bounded conservative rounding.
//!
//! Evidence in this file (generated F over public I routes): four generated properties build
//! authenticated target-replacement histories. Every episode publishes the same target at the same
//! slot and holds it until a common endpoint, but executes its permissionless crank work eagerly,
//! at a generated irregular subset of slots, or only at the endpoint. They compare the complete
//! decoded economic market, wrapper profiles/control sequences, both exposed portfolios, and every
//! SPL-token account after each common prefix. Absolute refresh-count/version IDs, derived
//! health-certificate caches, and the transaction-time origin of a fresh backing lifetime are
//! normalized. Raw backing expiries are checked separately: every schedule starts the same bounded
//! lifetime inside the episode that first crystallizes the loss, and that expiry is immutable
//! thereafter. Gross paid/received funding telemetry is reduced to its exact net form because
//! settlement timing can split one net flow into offsetting observations; a separate regression
//! below proves that a paid-only rewards consumer is not cadence-safe. Every capital, PnL, backing
//! amount/status, leg, source-domain, lock, lifecycle, funding index, and custody field remains
//! exact. INV-053 independently proves that stale certificate caches cannot authorize favorable
//! work. The same histories then compose through (1) a live close/withdraw suffix, (2) authority
//! resolution plus a bounded multi-call resolved-payout sweep in either claimant order, and (3)
//! asset shutdown plus both owner Recovery forfeits. The fixed direction-reversal regression
//! additionally requires released-PnL conversion to succeed. The oracle target changes, effective
//! price moves, and funding accrues, so equality is nonvacuous. This proves that caller-selected
//! crank cadence cannot change value attribution for these bounded public histories while
//! preserving authenticated event order. A separate public unilateral-reduction regression
//! creates quantity ADL, advances an authenticated mark, settles both sides, and requires exact
//! zero-sum account-value deltas and matching source claim/backing. It fails on the former
//! `ADL_ONE` K/F accrual path. Generated owner-reduction partitions also compare aggregate,
//! split, and reversed-split execution. Every field except the affected side's `A` factor is
//! exact; repeated floor can lower split `A` by at most one unit per extra partition, which is
//! checked against an independent recurrence together with the resulting one-atom effective-OI
//! scan envelope.
//! `v16_program_live_insurance_withdrawal_is_split_merge_invariant` independently funds both live
//! insurance domains through public top-ups, then crosses the domain boundary with one aggregate
//! withdrawal, a generated two-part split, and its reverse. Every successful part moves the exact
//! requested SPL and engine atoms; one atom beyond the remaining asset budget rejects with exact
//! writable and lamport rollback. The three schedules converge byte-for-byte across wrapper/engine
//! state, every portfolio, all SPL accounts, token supply, and the foreign instance while each call
//! remains below the CU ceiling.
//! `v16_program_backed_claim_conversion_is_atomic_under_split_caps` creates a real half-backed
//! user claim through each of the four trade routes. The public conversion API is intentionally
//! all-or-nothing: generated strict sub-caps reject with exact rollback rather than partially
//! consuming claim or backing atoms. Aggregate, split-attempt, and reversed-attempt schedules then
//! execute one complete conversion and converge byte-for-byte; a final retry proves the consumed
//! claim and lien cannot be reused.
//!
//! Guarantee boundary: target publication order and target lifetime are identical across the
//! compared executions. Reordering authenticated observations is a different economic history,
//! not a crank partition. Gross paid/received funding counters are cadence-dependent telemetry and
//! must not be consumed as a partition-invariant reward basis; the fixed regression proves their
//! net and all economic state remain equal. Deterministic maximum-shape, Hybrid/Pyth, terminal SPL
//! settlement, and wrapper/engine arithmetic proofs live in the INV-052 CU and Kani files. Exact
//! staleness boundaries and other operation families listed in the coverage matrix remain open.

use super::*;
use crate::support::{
    fuzz_model::execute_trade_route,
    v16_svm::{MarketConfig, V16Svm, PRIMARY_ACTOR_COUNT, TX_CU_LIMIT},
};
use percolator::{ADL_ONE, BOUND_SCALE, CREDIT_RATE_SCALE, POS_SCALE};
use percolator_prog::ix::CrankObservationHint;
use percolator_prog::state;

const INITIAL_PRICE: u64 = 1_000_000;
const BACKING_FRESHNESS_HORIZON: u64 = 100;

#[derive(Clone, Copy, Debug)]
struct TargetEpisode {
    duration_slots: u64,
    target_price: u64,
    irregular_mask: u16,
}

#[derive(Clone, Copy, Debug)]
enum CrankPartition {
    Eager,
    Irregular,
    EndpointOnly,
}

#[derive(Clone, Copy, Debug)]
enum HistorySuffix {
    LiveClose { convert_released_pnl: bool },
    ResolvedClose { reverse_claimant_order: bool },
    ShutdownForfeit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CanonicalPrefixSnapshot {
    wrapper_config: state::WrapperConfigV16,
    group: state::MarketGroupV16,
    profiles: Vec<state::AssetOracleProfileV16>,
    control_sequences: Vec<state::AssetControlSequencesV16>,
    long_portfolio: state::PortfolioAccountV16,
    short_portfolio: state::PortfolioAccountV16,
    tokens: Vec<(solana_sdk::pubkey::Pubkey, Vec<u8>)>,
}

#[derive(Debug)]
struct TargetHistoryOutcome {
    prefixes: Vec<CanonicalPrefixSnapshot>,
    gross_funding_prefixes: Vec<[u128; 4]>,
    backing_expiry_prefixes: Vec<Vec<Option<u64>>>,
    post_suffix: CanonicalPrefixSnapshot,
    destination_payouts: [u64; 2],
    saw_price_movement: bool,
    saw_funding: bool,
    max_compute_units: u64,
    public_steps: usize,
}

#[derive(Debug)]
struct RebalancePartitionOutcome {
    snapshot: CanonicalPrefixSnapshot,
    final_basis_q: i128,
    final_oi_q: [u128; 2],
    max_compute_units: u64,
    public_steps: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InsuranceWithdrawalFrame {
    wrapper_config: state::WrapperConfigV16,
    group: state::MarketGroupV16,
    primary_portfolios: Vec<Vec<u8>>,
    foreign_market: Vec<u8>,
    foreign_portfolio: Vec<u8>,
    tokens: Vec<(solana_sdk::pubkey::Pubkey, Vec<u8>)>,
    provider_destination_amount: u64,
    token_supply: u128,
}

#[derive(Debug)]
struct InsuranceWithdrawalPartitionOutcome {
    frame: InsuranceWithdrawalFrame,
    max_compute_units: u64,
    public_steps: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BackingConversionFrame {
    markets: [Vec<u8>; 2],
    backing_ledger: Vec<u8>,
    portfolios: Vec<Vec<u8>>,
    foreign_portfolio: Vec<u8>,
    tokens: Vec<(solana_sdk::pubkey::Pubkey, Vec<u8>)>,
    matcher_contexts: Vec<Vec<u8>>,
}

#[derive(Debug)]
struct BackingConversionPartitionOutcome {
    frame: BackingConversionFrame,
    max_compute_units: u64,
    public_steps: usize,
}

fn normalize_funding_counter_pair(
    paid: &mut percolator::V16PodU128,
    received: &mut percolator::V16PodU128,
) {
    let canceled = paid.get().min(received.get());
    *paid = percolator::V16PodU128::new(paid.get() - canceled);
    *received = percolator::V16PodU128::new(received.get() - canceled);
}

fn gross_funding_counters(account: &state::PortfolioAccountV16) -> [u128; 4] {
    [
        account.funding_long_paid_atoms_total.get(),
        account.funding_long_received_atoms_total.get(),
        account.funding_short_paid_atoms_total.get(),
        account.funding_short_received_atoms_total.get(),
    ]
}

fn net_funding_counters(gross: [u128; 4]) -> [i128; 2] {
    [
        i128::try_from(gross[1]).unwrap() - i128::try_from(gross[0]).unwrap(),
        i128::try_from(gross[3]).unwrap() - i128::try_from(gross[2]).unwrap(),
    ]
}

fn net_funding_prefixes(outcome: &TargetHistoryOutcome) -> Vec<[i128; 2]> {
    outcome
        .gross_funding_prefixes
        .iter()
        .copied()
        .map(net_funding_counters)
        .collect()
}

fn fresh_backing_expiries(group: &state::MarketGroupV16) -> Vec<Option<u64>> {
    group
        .source_backing_buckets
        .iter()
        .map(|bucket| {
            (bucket.status == percolator::BackingBucketStatusV16::Fresh)
                .then_some(bucket.expiry_slot)
        })
        .collect()
}

fn canonical_prefix_snapshot(env: &V16Svm) -> Result<CanonicalPrefixSnapshot, String> {
    let (wrapper_config, mut group) = env.primary_market_state();
    for (source, reservation) in group
        .source_credit
        .iter_mut()
        .zip(group.insurance_credit_reservations.iter_mut())
    {
        reservation.source_credit_epoch =
            if *reservation == percolator::InsuranceCreditReservationV16::EMPTY {
                0
            } else {
                u64::from(reservation.source_credit_epoch != source.credit_epoch)
            };
        source.credit_epoch = 0;
    }
    group.risk_epoch = 0;
    // A capital-backed loss starts its configured freshness lifetime when that
    // loss is actually crystallized. Eager and delayed permissionless settlement
    // can therefore choose different, bounded start slots without changing the
    // amount, status, source attribution, or custody. Live Fresh deadlines are
    // verified separately by `backing_expiry_partition_envelope`.
    for bucket in &mut group.source_backing_buckets {
        // The exact start slot is cadence metadata both while the bucket is Fresh and after its
        // terminal status has made the deadline inert. Amount, status, and attribution remain
        // exact; `backing_expiry_partition_envelope` checks every live Fresh deadline separately.
        bucket.expiry_slot = 0;
    }

    let mut long_portfolio = env.primary_portfolio(0);
    long_portfolio.health_cert = Default::default();
    normalize_funding_counter_pair(
        &mut long_portfolio.funding_long_paid_atoms_total,
        &mut long_portfolio.funding_long_received_atoms_total,
    );
    normalize_funding_counter_pair(
        &mut long_portfolio.funding_short_paid_atoms_total,
        &mut long_portfolio.funding_short_received_atoms_total,
    );
    let mut short_portfolio = env.primary_portfolio(1);
    short_portfolio.health_cert = Default::default();
    normalize_funding_counter_pair(
        &mut short_portfolio.funding_long_paid_atoms_total,
        &mut short_portfolio.funding_long_received_atoms_total,
    );
    normalize_funding_counter_pair(
        &mut short_portfolio.funding_short_paid_atoms_total,
        &mut short_portfolio.funding_short_received_atoms_total,
    );

    let profiles = (0..group.assets.len())
        .map(|asset_index| env.primary_profile(asset_index))
        .collect();
    let control_sequences = (0..group.assets.len())
        .map(|asset_index| env.primary_control_sequences(asset_index))
        .collect();
    Ok(CanonicalPrefixSnapshot {
        wrapper_config,
        group,
        profiles,
        control_sequences,
        long_portfolio,
        short_portfolio,
        tokens: env.all_token_account_data(),
    })
}

fn run_rebalance_partition(
    seed: [u8; 32],
    open_q: u128,
    reductions: &[u128],
) -> Result<RebalancePartitionOutcome, String> {
    let mut env = V16Svm::new(seed, MarketConfig::default());
    env.trade_no_cpi(0, 1, 0, open_q as i128, INITIAL_PRICE, 0)
        .map_err(|error| format!("open bilateral rebalance position: {error}"))?;
    let tokens_before = env.all_token_account_data();
    let mut max_compute_units = 0;
    env.begin_public_trace();
    for (index, reduction) in reductions.iter().copied().enumerate() {
        let step = env
            .rebalance_reduce(0, 0, reduction)
            .map_err(|error| format!("rebalance partition {index} reduce {reduction}: {error}"))?;
        max_compute_units = max_compute_units.max(step.compute_units);
    }
    if env.all_token_account_data() != tokens_before {
        return Err("RebalanceReduce moved SPL custody".to_string());
    }
    let trace = env.finish_public_trace();
    if trace.out_of_band_economic_mutations != 0 || trace.steps.iter().any(|step| !step.succeeded) {
        return Err("rebalance partition used a rejected or out-of-band step".to_string());
    }
    let account = env.primary_portfolio(0);
    let group = env.primary_market_state().1;
    Ok(RebalancePartitionOutcome {
        snapshot: canonical_prefix_snapshot(&env)?,
        final_basis_q: account.legs[0].basis_pos_q.get(),
        final_oi_q: [
            group.assets[0].oi_eff_long_q,
            group.assets[0].oi_eff_short_q,
        ],
        max_compute_units,
        public_steps: trace.steps.len(),
    })
}

fn insurance_withdrawal_frame(env: &V16Svm) -> InsuranceWithdrawalFrame {
    let (wrapper_config, group) = env.primary_market_state();
    InsuranceWithdrawalFrame {
        wrapper_config,
        group,
        primary_portfolios: env.all_primary_portfolio_data(),
        foreign_market: env.market_data(true),
        foreign_portfolio: env.foreign_portfolio_data(),
        tokens: env.all_token_account_data(),
        provider_destination_amount: env.token_amount(env.provider_destination_token),
        token_supply: env.token_supply_observed(),
    }
}

fn run_insurance_withdrawal_partition(
    seed: [u8; 32],
    long_budget: u128,
    short_budget: u128,
    parts: &[u128],
) -> Result<InsuranceWithdrawalPartitionOutcome, String> {
    let total = parts.iter().try_fold(0u128, |sum, part| {
        sum.checked_add(*part)
            .ok_or_else(|| "insurance partition total overflow".to_string())
    })?;
    if parts.is_empty()
        || parts.iter().any(|part| *part == 0)
        || total <= long_budget
        || total > long_budget + short_budget
    {
        return Err("insurance partition must cross the long/short domain boundary".to_string());
    }

    let mut env = V16Svm::new(seed, MarketConfig::default());
    let initial = insurance_withdrawal_frame(&env);
    if initial.group.insurance_domain_budget[0] != 0
        || initial.group.insurance_domain_budget[1] != 0
    {
        return Err(
            "insurance partition fixture started with live asset-domain budgets".to_string(),
        );
    }

    env.begin_public_trace();
    let first_top_up = env
        .top_up_insurance_domain(0, long_budget)
        .map_err(|error| format!("top up long insurance domain: {error}"))?;
    let second_top_up = env
        .top_up_insurance_domain(1, short_budget)
        .map_err(|error| format!("top up short insurance domain: {error}"))?;
    let mut max_compute_units = first_top_up.compute_units.max(second_top_up.compute_units);
    let funded = env.primary_market_state().1;
    if funded.insurance_domain_budget[0] != long_budget
        || funded.insurance_domain_budget[1] != short_budget
        || funded.insurance != initial.group.insurance + long_budget + short_budget
        || funded.vault != initial.group.vault + long_budget + short_budget
        || env.token_amount(env.vault) as u128 != funded.vault
    {
        return Err(
            "public insurance top-ups did not create the exact two-domain stock".to_string(),
        );
    }

    let mut cumulative = 0u128;
    for (index, part) in parts.iter().copied().enumerate() {
        let destination_before = env.token_amount(env.provider_destination_token);
        let vault_before = env.token_amount(env.vault);
        let group_before = env.primary_market_state().1;
        let success = env
            .withdraw_insurance_asset_as_admin(0, part)
            .map_err(|error| format!("insurance withdrawal partition {index}: {error}"))?;
        max_compute_units = max_compute_units.max(success.compute_units);
        cumulative = cumulative
            .checked_add(part)
            .ok_or_else(|| "insurance withdrawal cumulative overflow".to_string())?;
        let part_u64 = u64::try_from(part)
            .map_err(|_| "insurance withdrawal part does not fit SPL amount".to_string())?;
        let destination_delta = env
            .token_amount(env.provider_destination_token)
            .checked_sub(destination_before)
            .ok_or_else(|| "insurance withdrawal reduced its destination".to_string())?;
        let vault_delta = vault_before
            .checked_sub(env.token_amount(env.vault))
            .ok_or_else(|| "insurance withdrawal increased its vault".to_string())?;
        let group_after = env.primary_market_state().1;
        if destination_delta != part_u64
            || vault_delta != part_u64
            || group_before.insurance - group_after.insurance != part
            || group_before.vault - group_after.vault != part
        {
            return Err(format!(
                "insurance partition {index} did not debit and credit exactly {part} atoms"
            ));
        }
    }

    if cumulative != total {
        return Err("insurance withdrawal parts did not sum to the authorized total".to_string());
    }
    let remaining = long_budget + short_budget - total;
    let before_overspend = insurance_withdrawal_frame(&env);
    let oversized = remaining
        .checked_add(1)
        .ok_or_else(|| "insurance overspend control overflow".to_string())?;
    if env.withdraw_insurance_asset_as_admin(0, oversized).is_ok()
        || insurance_withdrawal_frame(&env) != before_overspend
    {
        return Err(
            "over-budget insurance withdrawal did not reject with exact economic rollback"
                .to_string(),
        );
    }

    let trace = env.finish_public_trace();
    let successful_steps = 2 + parts.len();
    if trace.out_of_band_economic_mutations != 0
        || trace.steps.len() != successful_steps + 1
        || trace.steps[..successful_steps]
            .iter()
            .any(|step| !step.succeeded)
        || trace.steps[successful_steps].succeeded
        || trace.steps[successful_steps].rejected_exact_writable_rollback != Some(true)
        || trace.steps[successful_steps].rejected_no_program_lamport_delta != Some(true)
    {
        return Err(
            "insurance partition trace was not public, successful, then exactly atomic".to_string(),
        );
    }
    if trace
        .steps
        .iter()
        .filter_map(|step| step.compute_units)
        .any(|compute_units| compute_units >= TX_CU_LIMIT)
    {
        return Err("insurance partition exceeded the transaction CU limit".to_string());
    }

    let frame = insurance_withdrawal_frame(&env);
    let expected_short_remaining = short_budget - (total - long_budget);
    if frame.group.insurance_domain_budget[0] != 0
        || frame.group.insurance_domain_budget[1] != expected_short_remaining
        || frame.group.insurance != initial.group.insurance + remaining
        || frame.group.vault != initial.group.vault + remaining
        || frame.group.c_tot != initial.group.c_tot
        || frame.provider_destination_amount as u128 != total
        || frame.token_supply != initial.token_supply
        || frame.foreign_market != initial.foreign_market
        || frame.foreign_portfolio != initial.foreign_portfolio
        || frame.primary_portfolios != initial.primary_portfolios
    {
        return Err(
            "insurance partition escaped its exact stock, custody, or frame oracle".to_string(),
        );
    }

    Ok(InsuranceWithdrawalPartitionOutcome {
        frame,
        max_compute_units,
        public_steps: trace.steps.len(),
    })
}

fn backing_conversion_frame(env: &V16Svm) -> BackingConversionFrame {
    BackingConversionFrame {
        markets: [env.market_data(false), env.market_data(true)],
        backing_ledger: env.backing_domain_ledger_data(),
        portfolios: env.all_primary_portfolio_data(),
        foreign_portfolio: env.foreign_portfolio_data(),
        tokens: env.all_token_account_data(),
        matcher_contexts: env.all_matcher_context_data(),
    }
}

fn run_backing_conversion_partition(
    seed: [u8; 32],
    route: TradeRoute,
    rejected_caps: &[u128],
) -> Result<BackingConversionPartitionOutcome, String> {
    const WINNER: usize = 0;
    const OPEN_COUNTERPARTY: usize = 1;
    const CLOSE_COUNTERPARTY: usize = 2;
    const ASSET: u16 = 0;
    const SOURCE_DOMAIN: usize = 1;
    const START_PRICE: u64 = 100;
    const SETTLED_PRICE: u64 = 105;
    const POSITION_Q: i128 = 40 * POS_SCALE as i128;
    const DEPOSIT: u128 = 1_000;
    const CLAIM_ATOMS: u128 = 200;
    const BACKING_ATOMS: u128 = 100;

    if rejected_caps
        .iter()
        .any(|cap| *cap == 0 || *cap >= BACKING_ATOMS)
    {
        return Err(
            "fragmented conversion caps must be strict nonzero sub-conversion amounts".to_string(),
        );
    }

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
    env.begin_public_trace();
    let top_up = env
        .top_up_backing_bucket(SOURCE_DOMAIN as u16, BACKING_ATOMS, 100)
        .map_err(|error| format!("fund backing conversion source: {error}"))?;
    let open = execute_trade_route(
        &mut env,
        route,
        WINNER,
        OPEN_COUNTERPARTY,
        ASSET,
        POSITION_Q,
        START_PRICE,
        0,
    )
    .map_err(|error| format!("open backing conversion claim: {error}"))?;
    env.warp_to_slot(2);
    let mark = env
        .push_auth_mark(ASSET, 2, SETTLED_PRICE)
        .map_err(|error| format!("publish backing conversion mark: {error}"))?;
    let crank = env
        .crank(
            WINNER,
            2,
            vec![CrankObservationHint {
                asset_index: ASSET,
                oracle_accounts: env.primary_profile(ASSET as usize).oracle_leg_count,
            }],
        )
        .map_err(|error| format!("settle backing conversion claim: {error}"))?;
    let close = execute_trade_route(
        &mut env,
        route,
        WINNER,
        CLOSE_COUNTERPARTY,
        ASSET,
        -POSITION_Q,
        SETTLED_PRICE,
        0,
    )
    .map_err(|error| format!("flatten backing conversion winner: {error}"))?;
    let mut max_compute_units = [
        top_up.compute_units,
        open.compute_units,
        mark.compute_units,
        crank.compute_units,
        close.compute_units,
    ]
    .into_iter()
    .max()
    .expect("nonempty setup CU set");

    let before_conversion = env.primary_market_state().1;
    let winner_before = env.primary_portfolio(WINNER);
    if winner_before.pnl.get() != CLAIM_ATOMS as i128
        || before_conversion.source_credit[SOURCE_DOMAIN].positive_claim_bound_num
            != CLAIM_ATOMS * BOUND_SCALE
        || before_conversion.source_credit[SOURCE_DOMAIN].fresh_reserved_backing_num
            != BACKING_ATOMS * BOUND_SCALE
    {
        return Err(format!(
            "public conversion fixture did not create the exact half-backed claim: pnl={}, source={:?}",
            winner_before.pnl.get(),
            before_conversion.source_credit[SOURCE_DOMAIN],
        ));
    }
    let tokens_before_conversion = env.all_token_account_data();

    for (index, cap) in rejected_caps.iter().copied().enumerate() {
        let before = backing_conversion_frame(&env);
        if env.convert_released_pnl(WINNER, cap).is_ok() || backing_conversion_frame(&env) != before
        {
            return Err(format!(
                "fragmented conversion cap {index} ({cap}) partially consumed an atomic claim"
            ));
        }
    }

    let conversion = env
        .convert_released_pnl(WINNER, BACKING_ATOMS)
        .map_err(|error| format!("land complete atomic backing conversion: {error}"))?;
    max_compute_units = max_compute_units.max(conversion.compute_units);
    let after_conversion = env.primary_market_state().1;
    let winner_after = env.primary_portfolio(WINNER);
    if winner_after.capital.get() - winner_before.capital.get() != BACKING_ATOMS
        || winner_before.pnl.get() - winner_after.pnl.get() != CLAIM_ATOMS as i128
        || winner_after.pnl.get() != 0
        || after_conversion.source_credit[SOURCE_DOMAIN].positive_claim_bound_num != 0
        || after_conversion.source_credit[SOURCE_DOMAIN].fresh_reserved_backing_num != 0
        || after_conversion.source_backing_buckets[SOURCE_DOMAIN].consumed_liened_backing_num
            != BACKING_ATOMS * BOUND_SCALE
        || env.all_token_account_data() != tokens_before_conversion
    {
        return Err(format!(
            "atomic backing conversion did not consume one claim/backing lifecycle exactly: before={:?}, after={:?}, winner_before={winner_before:?}, winner_after={winner_after:?}",
            before_conversion.source_credit[SOURCE_DOMAIN],
            after_conversion.source_credit[SOURCE_DOMAIN],
        ));
    }

    let before_retry = backing_conversion_frame(&env);
    if env.convert_released_pnl(WINNER, BACKING_ATOMS).is_ok()
        || backing_conversion_frame(&env) != before_retry
    {
        return Err("completed conversion remained reusable".to_string());
    }

    let trace = env.finish_public_trace();
    let failed_steps: Vec<_> = trace.steps.iter().filter(|step| !step.succeeded).collect();
    if trace.out_of_band_economic_mutations != 0
        || failed_steps.len() != rejected_caps.len() + 1
        || failed_steps.iter().any(|step| {
            step.program_id != env.program_id
                || step.rejected_exact_writable_rollback != Some(true)
                || step.rejected_no_program_lamport_delta != Some(true)
                || step.token_deltas.iter().any(|(_, delta)| *delta != 0)
        })
        || trace
            .steps
            .iter()
            .filter_map(|step| step.compute_units)
            .any(|compute_units| compute_units >= TX_CU_LIMIT)
    {
        return Err("backing conversion trace was not public, atomic, and CU-bounded".to_string());
    }

    Ok(BackingConversionPartitionOutcome {
        frame: backing_conversion_frame(&env),
        max_compute_units,
        public_steps: trace.steps.len(),
    })
}

fn rebalance_snapshot_without_adl_rounding(
    mut snapshot: CanonicalPrefixSnapshot,
) -> CanonicalPrefixSnapshot {
    snapshot.group.assets[0].a_short = 0;
    snapshot
}

fn expected_rebalance_a(mut oi_q: u128, reductions: &[u128]) -> u128 {
    let mut a = ADL_ONE;
    for reduction in reductions.iter().copied() {
        let next_oi = oi_q.checked_sub(reduction).unwrap();
        a = a.checked_mul(next_oi).unwrap() / oi_q;
        oi_q = next_oi;
    }
    a
}

fn counterparty_effective_scan(outcome: &RebalancePartitionOutcome) -> u128 {
    let leg = outcome
        .snapshot
        .short_portfolio
        .legs
        .iter()
        .find_map(|leg| leg.try_to_runtime().ok().filter(|leg| leg.active))
        .expect("rebalance world retains the counterparty leg");
    leg.basis_pos_q
        .unsigned_abs()
        .checked_mul(outcome.snapshot.group.assets[0].a_short)
        .unwrap()
        / leg.a_basis
}

fn prefix_difference(
    left: &[CanonicalPrefixSnapshot],
    right: &[CanonicalPrefixSnapshot],
) -> String {
    let Some((index, (left, right))) = left
        .iter()
        .zip(right)
        .enumerate()
        .find(|(_, (left, right))| left != right)
    else {
        return format!("prefix lengths differ: {} != {}", left.len(), right.len());
    };
    format!("prefix {index}: {}", snapshot_difference(left, right))
}

fn snapshot_difference(left: &CanonicalPrefixSnapshot, right: &CanonicalPrefixSnapshot) -> String {
    let backing_bucket_differences: Vec<_> = left
        .group
        .source_backing_buckets
        .iter()
        .zip(right.group.source_backing_buckets.iter())
        .enumerate()
        .filter_map(|(index, (left, right))| (left != right).then_some((index, *left, *right)))
        .collect();
    format!(
        "wrapper_equal={}; group_differences={:?}; backing_bucket_differences={backing_bucket_differences:?}; profiles_equal={}; controls_equal={}; long_equal={}; short_equal={}; tokens_equal={}",
        left.wrapper_config == right.wrapper_config,
        group_differences(&left.group, &right.group),
        left.profiles == right.profiles,
        left.control_sequences == right.control_sequences,
        left.long_portfolio == right.long_portfolio,
        left.short_portfolio == right.short_portfolio,
        left.tokens == right.tokens,
    )
}

fn group_differences(
    left: &state::MarketGroupV16,
    right: &state::MarketGroupV16,
) -> Vec<&'static str> {
    let mut differences = Vec::new();
    macro_rules! compare {
        ($field:ident) => {
            if left.$field != right.$field {
                differences.push(stringify!($field));
            }
        };
    }
    compare!(market_group_id);
    compare!(config);
    compare!(vault);
    compare!(insurance);
    compare!(c_tot);
    compare!(pnl_pos_tot);
    compare!(pnl_pos_bound_tot_num);
    compare!(pnl_pos_bound_tot);
    compare!(pnl_matured_pos_tot);
    compare!(backing_provider_earnings_total);
    compare!(source_claim_bound_total_num);
    compare!(source_insurance_credit_reserved_total_atoms);
    compare!(insurance_domain_budget_remaining_total);
    compare!(resolved_payout_blocker_count);
    compare!(insurance_domain_budget);
    compare!(insurance_domain_spent);
    compare!(pending_domain_loss_barriers);
    compare!(source_credit);
    compare!(source_backing_buckets);
    compare!(insurance_credit_reservations);
    compare!(materialized_portfolio_count);
    compare!(stale_certificate_count);
    compare!(b_stale_account_count);
    compare!(negative_pnl_account_count);
    compare!(risk_epoch);
    compare!(asset_set_epoch);
    compare!(asset_activation_count);
    compare!(last_asset_activation_slot);
    compare!(next_market_id);
    compare!(oracle_epoch);
    compare!(funding_epoch);
    compare!(slot_last);
    compare!(current_slot);
    compare!(assets);
    compare!(bankruptcy_hlock_active);
    compare!(threshold_stress_active);
    compare!(loss_stale_active);
    compare!(recovery_reason);
    compare!(mode);
    compare!(resolved_slot);
    compare!(payout_snapshot);
    compare!(payout_snapshot_pnl_pos_tot);
    compare!(payout_snapshot_captured);
    compare!(resolved_payout_ledger);
    differences
}

fn normalize_target_episodes(raw: &[(u64, u64, bool, u16)]) -> Vec<TargetEpisode> {
    let mut previous = INITIAL_PRICE;
    raw.iter()
        .enumerate()
        .map(
            |(index, &(duration_slots, magnitude, upward, irregular_mask))| {
                let mut target_price = if upward {
                    INITIAL_PRICE + magnitude
                } else {
                    INITIAL_PRICE - magnitude
                };
                if target_price == previous {
                    let adjustment = index as u64 + 1;
                    target_price = if upward {
                        target_price + adjustment
                    } else {
                        target_price - adjustment
                    };
                }
                previous = target_price;
                TargetEpisode {
                    duration_slots,
                    target_price,
                    irregular_mask,
                }
            },
        )
        .collect()
}

fn backing_expiry_partition_envelope(
    episodes: &[TargetEpisode],
    eager: &TargetHistoryOutcome,
    irregular: &TargetHistoryOutcome,
    delayed: &TargetHistoryOutcome,
) -> Result<(), String> {
    let domain_count = eager.backing_expiry_prefixes.first().map_or(0, Vec::len);
    if irregular.backing_expiry_prefixes.len() != episodes.len()
        || delayed.backing_expiry_prefixes.len() != episodes.len()
        || eager.backing_expiry_prefixes.len() != episodes.len()
    {
        return Err("backing expiry prefix count does not match generated episodes".to_string());
    }

    let mut episode_bounds = Vec::with_capacity(episodes.len());
    let mut endpoint = 1u64;
    for episode in episodes {
        let publication = endpoint
            .checked_add(1)
            .ok_or_else(|| "publication slot overflow".to_string())?;
        endpoint = publication
            .checked_add(episode.duration_slots - 1)
            .ok_or_else(|| "episode endpoint overflow".to_string())?;
        episode_bounds.push((publication, endpoint));
    }

    for domain in 0..domain_count {
        let Some(first_prefix) = (0..episodes.len()).find(|&prefix| {
            eager.backing_expiry_prefixes[prefix][domain].is_some()
                || irregular.backing_expiry_prefixes[prefix][domain].is_some()
                || delayed.backing_expiry_prefixes[prefix][domain].is_some()
        }) else {
            continue;
        };
        let first = [
            eager.backing_expiry_prefixes[first_prefix][domain],
            irregular.backing_expiry_prefixes[first_prefix][domain],
            delayed.backing_expiry_prefixes[first_prefix][domain],
        ];
        let [Some(eager_expiry), Some(irregular_expiry), Some(delayed_expiry)] = first else {
            return Err(format!(
                "domain {domain} backing appeared in only some crank partitions at prefix {first_prefix}: {first:?}"
            ));
        };
        let (publication, first_endpoint) = episode_bounds[first_prefix];
        let minimum_expiry = publication
            .checked_add(BACKING_FRESHNESS_HORIZON)
            .ok_or_else(|| "minimum backing expiry overflow".to_string())?;
        let maximum_expiry = first_endpoint
            .checked_add(BACKING_FRESHNESS_HORIZON)
            .ok_or_else(|| "maximum backing expiry overflow".to_string())?;
        if !(minimum_expiry..=maximum_expiry).contains(&eager_expiry)
            || !(minimum_expiry..=maximum_expiry).contains(&irregular_expiry)
            || !(minimum_expiry..=maximum_expiry).contains(&delayed_expiry)
            || eager_expiry > irregular_expiry
            || irregular_expiry > delayed_expiry
        {
            return Err(format!(
                "domain {domain} backing expiry escaped crystallization envelope {minimum_expiry}..={maximum_expiry}: {first:?}"
            ));
        }

        for prefix in first_prefix..episodes.len() {
            let observed = [
                eager.backing_expiry_prefixes[prefix][domain],
                irregular.backing_expiry_prefixes[prefix][domain],
                delayed.backing_expiry_prefixes[prefix][domain],
            ];
            if observed != first {
                return Err(format!(
                    "domain {domain} backing expiry changed after creation at prefix {prefix}: first={first:?} observed={observed:?}"
                ));
            }
            let (_, current_endpoint) = episode_bounds[prefix];
            if observed
                .into_iter()
                .flatten()
                .any(|expiry| expiry <= current_endpoint)
            {
                return Err(format!(
                    "domain {domain} backing expired inside generated history at prefix {prefix}: {observed:?}"
                ));
            }
        }
    }
    Ok(())
}

fn selected_crank_slots(
    partition: CrankPartition,
    start_slot: u64,
    end_slot: u64,
    irregular_mask: u16,
) -> Vec<u64> {
    match partition {
        CrankPartition::Eager => (start_slot..=end_slot).collect(),
        CrankPartition::EndpointOnly => vec![end_slot],
        CrankPartition::Irregular => {
            let mut selected: Vec<_> = (start_slot..end_slot)
                .enumerate()
                .filter_map(|(offset, slot)| ((irregular_mask >> offset) & 1 != 0).then_some(slot))
                .collect();
            selected.push(end_slot);
            selected
        }
    }
}

fn market_accrual_is_pending(env: &V16Svm, slot: u64) -> bool {
    let (_, group) = env.primary_market_state();
    group.assets[0].slot_last < slot
}

fn resolved_portfolio_is_terminal(env: &V16Svm, actor: usize) -> bool {
    let group = env.primary_market_state().1;
    let account = env.primary_portfolio(actor);
    let Ok(receipt) = account.resolved_payout_receipt.try_to_runtime() else {
        return false;
    };
    let Ok(close) = account.close_progress.try_to_runtime() else {
        return false;
    };
    group.mode == percolator::MarketModeV16::Resolved
        && account.capital.get() == 0
        && account.pnl.get() == 0
        && account.reserved_pnl.get() == 0
        && account.fee_credits.get() == 0
        && account.cancel_deposit_escrow.get() == 0
        && percolator::active_bitmap_is_empty(state::portfolio_active_bitmap(&account))
        && account.stale_state == 0
        && account.b_stale_state == 0
        && account.rebalance_lock == 0
        && account.liquidation_lock == 0
        && account.last_fee_slot.get() == group.resolved_slot
        && account.health_cert.valid == 0
        && account
            .source_domains
            .iter()
            .all(|source| !source.is_occupied())
        && (!receipt.present || receipt.finalized)
        && (!close.active || (close.finalized && close.residual_remaining == 0))
}

fn settle_resolved_portfolios(
    env: &mut V16Svm,
    order: [usize; 2],
    max_compute_units: &mut u64,
) -> Result<(), String> {
    const SWEEP_BOUND: usize = 64;
    for sweep in 0..SWEEP_BOUND {
        if order
            .into_iter()
            .all(|actor| resolved_portfolio_is_terminal(env, actor))
        {
            return Ok(());
        }
        let mut sweep_mutated = false;
        for actor in order {
            if resolved_portfolio_is_terminal(env, actor) {
                continue;
            }
            let receipt = env
                .primary_portfolio(actor)
                .resolved_payout_receipt
                .try_to_runtime()
                .map_err(|error| format!("decode actor {actor} resolved receipt: {error:?}"))?;
            let market_before = env.market_data(false);
            let portfolio_before = env.primary_portfolio_data(actor);
            let tokens_before = env.all_token_account_data();
            let destination = env.actors[actor].destination_token;
            let destination_before = env.token_amount(destination);
            let spl_vault_before = env.token_amount(env.vault);
            let engine_vault_before = env.primary_market_state().1.vault;
            let progress = if receipt.present && !receipt.finalized {
                env.claim_resolved_payout_topup_primary(actor)
                    .map_err(|error| format!("claim resolved actor {actor}: {error}"))?
            } else {
                env.close_resolved_primary_signed(actor)
                    .map_err(|error| format!("close resolved actor {actor}: {error}"))?
            };
            *max_compute_units = (*max_compute_units).max(progress.compute_units);
            let destination_after = env.token_amount(destination);
            let spl_vault_after = env.token_amount(env.vault);
            let engine_vault_after = env.primary_market_state().1.vault;
            let payout = destination_after
                .checked_sub(destination_before)
                .ok_or_else(|| format!("actor {actor} resolved payout decreased destination"))?;
            let spl_debit = spl_vault_before
                .checked_sub(spl_vault_after)
                .ok_or_else(|| format!("actor {actor} resolved payout increased SPL vault"))?;
            let engine_debit = engine_vault_before
                .checked_sub(engine_vault_after)
                .ok_or_else(|| format!("actor {actor} resolved payout increased engine vault"))?;
            if payout != spl_debit || u128::from(payout) != engine_debit {
                return Err(format!(
                    "actor {actor} resolved payout mismatch: destination={payout}, SPL debit={spl_debit}, engine debit={engine_debit}"
                ));
            }
            let mutated = env.market_data(false) != market_before
                || env.primary_portfolio_data(actor) != portfolio_before
                || env.all_token_account_data() != tokens_before;
            if !mutated && !resolved_portfolio_is_terminal(env, actor) {
                return Err(format!(
                    "resolved actor {actor} returned a successful no-op at sweep {sweep}"
                ));
            }
            sweep_mutated |= mutated;
        }
        if !sweep_mutated {
            return Err(format!(
                "resolved claimant order {order:?} reached a nonterminal fixed point at sweep {sweep}"
            ));
        }
    }
    Err(format!(
        "resolved claimant order {order:?} did not terminate in {SWEEP_BOUND} sweeps"
    ))
}

fn run_target_history(
    seed: [u8; 32],
    episodes: &[TargetEpisode],
    partition: CrankPartition,
    suffix: HistorySuffix,
) -> Result<TargetHistoryOutcome, String> {
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            h_max: 32,
            max_price_move_bps_per_slot: 25,
            max_accrual_dt_slots: 32,
            max_abs_funding_e9_per_slot: 10_000,
            min_funding_lifetime_slots: 32,
            max_bankrupt_close_lifetime_slots: BACKING_FRESHNESS_HORIZON,
            ..MarketConfig::default()
        },
    );
    env.warp_to_slot(1);
    env.configure_auth_mark(false, 0, 1, INITIAL_PRICE)
        .map_err(|error| format!("configure initial authenticated mark: {error}"))?;
    env.trade_no_cpi(0, 1, 0, 10 * POS_SCALE as i128, INITIAL_PRICE, 0)
        .map_err(|error| format!("open nonvacuous bilateral position: {error}"))?;
    if matches!(suffix, HistorySuffix::ShutdownForfeit) {
        env.configure_permissionless_resolve(1_000, 1)
            .map_err(|error| format!("configure shutdown recovery policy: {error}"))?;
    }

    let setup_tokens = env.all_token_account_data();
    let mut endpoint = 1u64;
    let mut prefixes = Vec::with_capacity(episodes.len());
    let mut gross_funding_prefixes = Vec::with_capacity(episodes.len());
    let mut backing_expiry_prefixes = Vec::with_capacity(episodes.len());
    let mut saw_price_movement = false;
    let mut saw_funding = false;
    let mut max_compute_units = 0u64;
    env.begin_public_trace();

    for (episode_index, episode) in episodes.iter().enumerate() {
        let publication_slot = endpoint
            .checked_add(1)
            .ok_or_else(|| "publication slot overflow".to_string())?;
        endpoint = publication_slot
            .checked_add(episode.duration_slots - 1)
            .ok_or_else(|| "episode endpoint overflow".to_string())?;
        env.warp_to_slot(publication_slot);
        let publication = env
            .push_auth_mark(0, publication_slot, episode.target_price)
            .map_err(|error| {
                format!(
                    "episode {episode_index} publish target {} at slot {publication_slot}: {error}",
                    episode.target_price
                )
            })?;
        max_compute_units = max_compute_units.max(publication.compute_units);

        for slot in selected_crank_slots(
            partition,
            publication_slot,
            endpoint,
            episode.irregular_mask,
        ) {
            env.warp_to_slot(slot);
            if !market_accrual_is_pending(&env, slot) {
                continue;
            }
            let crank = env
                .crank(
                    0,
                    slot,
                    vec![CrankObservationHint {
                        asset_index: 0,
                        oracle_accounts: 0,
                    }],
                )
                .map_err(|error| {
                    format!("episode {episode_index} canonical crank at slot {slot}: {error}")
                })?;
            max_compute_units = max_compute_units.max(crank.compute_units);
        }

        let (_, group) = env.primary_market_state();
        let asset = group.assets[0];
        saw_price_movement |= asset.effective_price != INITIAL_PRICE;
        saw_funding |= asset.f_long_num != 0 || asset.f_short_num != 0;
        gross_funding_prefixes.push(gross_funding_counters(&env.primary_portfolio(0)));
        backing_expiry_prefixes.push(fresh_backing_expiries(&group));
        prefixes.push(canonical_prefix_snapshot(&env)?);
    }

    if env.all_token_account_data() != setup_tokens {
        return Err("oracle publication or crank changed SPL custody".to_string());
    }

    match suffix {
        HistorySuffix::LiveClose {
            convert_released_pnl,
        } => {
            let final_price = env.primary_market_state().1.assets[0].effective_price;
            let close = env
                .trade_no_cpi(0, 1, 0, -(10 * POS_SCALE as i128), final_price, 0)
                .map_err(|error| format!("close generated bilateral position: {error}"))?;
            max_compute_units = max_compute_units.max(close.compute_units);
            for actor in 0..2 {
                let account = env.primary_portfolio(actor);
                if !percolator::active_bitmap_is_empty(state::portfolio_active_bitmap(&account)) {
                    return Err(format!(
                        "actor {actor} retained an active leg after common close"
                    ));
                }
                if account.pnl.get() < 0 {
                    return Err(format!(
                        "actor {actor} retained negative PnL {} after common close",
                        account.pnl.get()
                    ));
                }
                if convert_released_pnl && account.pnl.get() > 0 {
                    let convert = env
                        .convert_released_pnl(actor, account.pnl.get() as u128)
                        .map_err(|error| format!("convert actor {actor} released PnL: {error}"))?;
                    max_compute_units = max_compute_units.max(convert.compute_units);
                }
                let capital = env.primary_portfolio(actor).capital.get();
                let withdraw = env
                    .withdraw_primary(actor, capital)
                    .map_err(|error| format!("withdraw actor {actor} terminal capital: {error}"))?;
                max_compute_units = max_compute_units.max(withdraw.compute_units);
            }
        }
        HistorySuffix::ResolvedClose {
            reverse_claimant_order,
        } => {
            let resolve = env
                .resolve_market()
                .map_err(|error| format!("resolve generated target history: {error}"))?;
            max_compute_units = max_compute_units.max(resolve.compute_units);
            if env.primary_market_state().1.mode != percolator::MarketModeV16::Resolved {
                return Err("ResolveMarket did not enter Resolved mode".to_string());
            }
            let order = if reverse_claimant_order {
                [1usize, 0usize]
            } else {
                [0usize, 1usize]
            };
            settle_resolved_portfolios(&mut env, order, &mut max_compute_units)?;
        }
        HistorySuffix::ShutdownForfeit => {
            let shutdown_slot = endpoint
                .checked_add(1)
                .ok_or_else(|| "shutdown slot overflow".to_string())?;
            env.warp_to_slot(shutdown_slot);
            let shutdown = env
                .shutdown_asset(0, shutdown_slot)
                .map_err(|error| format!("shutdown generated target history: {error}"))?;
            max_compute_units = max_compute_units.max(shutdown.compute_units);
            if env.primary_market_state().1.assets[0].lifecycle
                != percolator::AssetLifecycleV16::Recovery
            {
                return Err("asset shutdown did not enter Recovery".to_string());
            }
            let mut order = [0usize, 1usize];
            order.sort_by_key(|actor| env.primary_portfolio(*actor).pnl.get());
            for actor in order {
                let forfeit = env
                    .forfeit_recovery_leg(actor, 0, u128::MAX)
                    .map_err(|error| format!("forfeit Recovery actor {actor}: {error}"))?;
                max_compute_units = max_compute_units.max(forfeit.compute_units);
            }
            let group = env.primary_market_state().1;
            if group.assets[0].oi_eff_long_q != 0 || group.assets[0].oi_eff_short_q != 0 {
                return Err(format!(
                    "Recovery forfeits retained effective OI: long={} short={}",
                    group.assets[0].oi_eff_long_q, group.assets[0].oi_eff_short_q
                ));
            }
            for actor in 0..2 {
                for _ in 0..8 {
                    let account = env.primary_portfolio(actor);
                    if percolator::active_bitmap_is_empty(state::portfolio_active_bitmap(&account))
                    {
                        break;
                    }
                    let crank = env.crank(actor, shutdown_slot, vec![]).map_err(|error| {
                        format!("settle retained Recovery obligation for actor {actor}: {error}")
                    })?;
                    max_compute_units = max_compute_units.max(crank.compute_units);
                }
                if !percolator::active_bitmap_is_empty(state::portfolio_active_bitmap(
                    &env.primary_portfolio(actor),
                )) {
                    return Err(format!(
                        "Recovery forfeit retained actor {actor} obligation after bounded public cranks"
                    ));
                }
            }
        }
    }
    let destination_payouts = [
        env.token_amount(env.actors[0].destination_token),
        env.token_amount(env.actors[1].destination_token),
    ];
    let combined_payout = u128::from(destination_payouts[0]) + u128::from(destination_payouts[1]);
    let invalid_payout = match suffix {
        HistorySuffix::ShutdownForfeit => destination_payouts != [0, 0],
        HistorySuffix::LiveClose { .. } | HistorySuffix::ResolvedClose { .. } => {
            destination_payouts.contains(&0) || combined_payout > 200_000_000
        }
    };
    if invalid_payout {
        return Err(format!(
            "target history produced an invalid suffix payout: {destination_payouts:?}"
        ));
    }
    let post_suffix = canonical_prefix_snapshot(&env)?;

    let trace = env.finish_public_trace();
    if trace.out_of_band_economic_mutations != 0 {
        return Err(format!(
            "target history used {} out-of-band economic mutations",
            trace.out_of_band_economic_mutations
        ));
    }
    if trace.steps.iter().any(|step| !step.succeeded) {
        return Err("target history contains a rejected public step".to_string());
    }

    Ok(TargetHistoryOutcome {
        prefixes,
        gross_funding_prefixes,
        backing_expiry_prefixes,
        post_suffix,
        destination_payouts,
        saw_price_movement,
        saw_funding,
        max_compute_units,
        public_steps: trace.steps.len(),
    })
}

#[test]
fn v16_program_net_funding_is_partition_invariant_but_paid_only_rewards_are_not() {
    let episodes = [
        TargetEpisode {
            duration_slots: 2,
            target_price: 973_087,
            irregular_mask: 56_261,
        },
        TargetEpisode {
            duration_slots: 5,
            target_price: 1_026_027,
            irregular_mask: 41_421,
        },
        TargetEpisode {
            duration_slots: 6,
            target_price: 990_415,
            irregular_mask: 40_544,
        },
    ];
    let seed = [0x52; 32];
    let suffix = HistorySuffix::LiveClose {
        convert_released_pnl: true,
    };
    let eager = run_target_history(seed, &episodes, CrankPartition::Eager, suffix).unwrap();
    let delayed =
        run_target_history(seed, &episodes, CrankPartition::EndpointOnly, suffix).unwrap();

    assert_ne!(
        eager.gross_funding_prefixes, delayed.gross_funding_prefixes,
        "direction-reversing control must expose cadence-dependent gross observations",
    );
    assert_eq!(net_funding_prefixes(&eager), net_funding_prefixes(&delayed));
    assert_eq!(eager.prefixes, delayed.prefixes);
    assert_eq!(eager.post_suffix, delayed.post_suffix);
    assert_eq!(eager.destination_payouts, delayed.destination_payouts);
    backing_expiry_partition_envelope(&episodes, &eager, &eager, &delayed).unwrap();
}

#[test]
fn v16_program_owner_rebalance_reduction_is_split_merge_invariant() {
    let seed = [0x53; 32];
    let open_q = 10 * POS_SCALE;
    let aggregate = run_rebalance_partition(seed, open_q, &[6 * POS_SCALE]).unwrap();
    let split = run_rebalance_partition(seed, open_q, &[2 * POS_SCALE, 4 * POS_SCALE]).unwrap();

    assert_eq!(aggregate.snapshot, split.snapshot);
    assert_eq!(aggregate.final_basis_q, 4 * POS_SCALE as i128);
    assert_eq!(aggregate.final_basis_q, split.final_basis_q);
    assert_eq!(aggregate.final_oi_q, [4 * POS_SCALE, 4 * POS_SCALE]);
    assert_eq!(aggregate.final_oi_q, split.final_oi_q);
    assert_eq!(aggregate.public_steps, 1);
    assert_eq!(split.public_steps, 2);
    assert!(aggregate.max_compute_units < TX_CU_LIMIT);
    assert!(split.max_compute_units < TX_CU_LIMIT);
}

#[test]
fn v16_program_unilateral_rebalance_adl_keeps_followup_price_settlement_zero_sum() {
    let mut env = V16Svm::new([0x55; 32], MarketConfig::default());
    env.begin_public_trace();
    env.warp_to_slot(1);
    env.configure_auth_mark(false, 0, 1, INITIAL_PRICE).unwrap();
    env.trade_no_cpi(0, 1, 0, 10 * POS_SCALE as i128, INITIAL_PRICE, 0)
        .unwrap();
    env.rebalance_reduce(0, 0, 6 * POS_SCALE).unwrap();
    let before = env.primary_market_state().1;
    assert_eq!(before.assets[0].a_short, 4 * ADL_ONE / 10);
    assert_eq!(
        [
            before.assets[0].oi_eff_long_q,
            before.assets[0].oi_eff_short_q,
        ],
        [4 * POS_SCALE, 4 * POS_SCALE]
    );
    let tokens_before = env.all_token_account_data();
    let values_before = [0usize, 1usize].map(|actor| {
        let account = env.primary_portfolio(actor);
        i128::try_from(account.capital.get()).unwrap() + account.pnl.get()
    });

    env.warp_to_slot(2);
    env.push_auth_mark(0, 2, 900_000).unwrap();
    let observation = || {
        vec![CrankObservationHint {
            asset_index: 0,
            oracle_accounts: 0,
        }]
    };
    let short_crank = env.crank(1, 2, observation()).unwrap();
    let long_crank = env.crank(0, 2, observation()).unwrap();

    let values_after = [0usize, 1usize].map(|actor| {
        let account = env.primary_portfolio(actor);
        i128::try_from(account.capital.get()).unwrap() + account.pnl.get()
    });
    let expected_move = 4 * 100_000i128;
    assert_eq!(values_after[0] - values_before[0], -expected_move);
    assert_eq!(values_after[1] - values_before[1], expected_move);
    assert_eq!(
        (values_after[0] - values_before[0]) + (values_after[1] - values_before[1]),
        0,
        "post-ADL mark settlement must remain exactly zero-sum"
    );
    assert_eq!(env.all_token_account_data(), tokens_before);

    let after = env.primary_market_state().1;
    let source = after.source_credit[0];
    assert_eq!(
        source.positive_claim_bound_num, source.fresh_reserved_backing_num,
        "the winner claim must equal independently crystallized counterparty loss"
    );
    assert_eq!(source.credit_rate_num, CREDIT_RATE_SCALE);
    assert_eq!(after.assets[0].a_short, before.assets[0].a_short);
    assert!(short_crank.compute_units < TX_CU_LIMIT);
    assert!(long_crank.compute_units < TX_CU_LIMIT);

    let trace = env.finish_public_trace();
    assert_eq!(trace.out_of_band_economic_mutations, 0);
    assert!(trace.steps.iter().all(|step| step.succeeded));
    assert_eq!(trace.steps.len(), 6);
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_052_target_history_partition.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_generated_target_histories_are_crank_partition_invariant(
        seed in any::<[u8; 32]>(),
        raw_episodes in prop::collection::vec(
            (1u64..=8, 5_000u64..=50_000, any::<bool>(), any::<u16>()),
            3..=3,
        ),
    ) {
        let episodes = normalize_target_episodes(&raw_episodes);
        let suffix = HistorySuffix::LiveClose {
            convert_released_pnl: false,
        };
        let eager = run_target_history(seed, &episodes, CrankPartition::Eager, suffix)
            .map_err(TestCaseError::fail)?;
        let irregular = run_target_history(seed, &episodes, CrankPartition::Irregular, suffix)
            .map_err(TestCaseError::fail)?;
        let delayed = run_target_history(seed, &episodes, CrankPartition::EndpointOnly, suffix)
            .map_err(TestCaseError::fail)?;

        prop_assert!(
            eager.prefixes == irregular.prefixes,
            "eager/irregular divergence: {}",
            prefix_difference(&eager.prefixes, &irregular.prefixes),
        );
        prop_assert!(
            eager.prefixes == delayed.prefixes,
            "eager/delayed divergence: {}",
            prefix_difference(&eager.prefixes, &delayed.prefixes),
        );
        prop_assert_eq!(net_funding_prefixes(&eager), net_funding_prefixes(&irregular));
        prop_assert_eq!(net_funding_prefixes(&eager), net_funding_prefixes(&delayed));
        backing_expiry_partition_envelope(&episodes, &eager, &irregular, &delayed)
            .map_err(TestCaseError::fail)?;
        prop_assert!(
            eager.post_suffix == irregular.post_suffix,
            "eager/irregular post-live-close divergence: {}",
            snapshot_difference(&eager.post_suffix, &irregular.post_suffix),
        );
        prop_assert!(
            eager.post_suffix == delayed.post_suffix,
            "eager/delayed post-live-close divergence: {}",
            snapshot_difference(&eager.post_suffix, &delayed.post_suffix),
        );
        prop_assert_eq!(eager.destination_payouts, irregular.destination_payouts);
        prop_assert_eq!(eager.destination_payouts, delayed.destination_payouts);
        prop_assert_eq!(eager.prefixes.len(), episodes.len());
        prop_assert!(eager.saw_price_movement);
        prop_assert_eq!(eager.saw_funding, irregular.saw_funding);
        prop_assert_eq!(eager.saw_funding, delayed.saw_funding);
        prop_assert!(eager.public_steps > episodes.len());
        prop_assert!(irregular.public_steps > episodes.len());
        prop_assert!(delayed.public_steps > episodes.len());
        prop_assert!(eager.max_compute_units < TX_CU_LIMIT);
        prop_assert!(irregular.max_compute_units < TX_CU_LIMIT);
        prop_assert!(delayed.max_compute_units < TX_CU_LIMIT);
    }

    #[test]
    fn v16_program_generated_target_histories_preserve_resolved_settlement(
        seed in any::<[u8; 32]>(),
        raw_episodes in prop::collection::vec(
            (1u64..=8, 5_000u64..=50_000, any::<bool>(), any::<u16>()),
            3..=3,
        ),
        reverse_claimant_order in any::<bool>(),
    ) {
        let episodes = normalize_target_episodes(&raw_episodes);
        let suffix = HistorySuffix::ResolvedClose {
            reverse_claimant_order,
        };
        let eager = run_target_history(seed, &episodes, CrankPartition::Eager, suffix)
            .map_err(TestCaseError::fail)?;
        let irregular = run_target_history(seed, &episodes, CrankPartition::Irregular, suffix)
            .map_err(TestCaseError::fail)?;
        let delayed = run_target_history(seed, &episodes, CrankPartition::EndpointOnly, suffix)
            .map_err(TestCaseError::fail)?;

        prop_assert!(
            eager.prefixes == irregular.prefixes,
            "eager/irregular pre-resolve divergence: {}",
            prefix_difference(&eager.prefixes, &irregular.prefixes),
        );
        prop_assert!(
            eager.prefixes == delayed.prefixes,
            "eager/delayed pre-resolve divergence: {}",
            prefix_difference(&eager.prefixes, &delayed.prefixes),
        );
        backing_expiry_partition_envelope(&episodes, &eager, &irregular, &delayed)
            .map_err(TestCaseError::fail)?;
        prop_assert!(
            eager.post_suffix == irregular.post_suffix,
            "eager/irregular post-resolve divergence: {}",
            snapshot_difference(&eager.post_suffix, &irregular.post_suffix),
        );
        prop_assert!(
            eager.post_suffix == delayed.post_suffix,
            "eager/delayed post-resolve divergence: {}",
            snapshot_difference(&eager.post_suffix, &delayed.post_suffix),
        );
        prop_assert_eq!(eager.destination_payouts, irregular.destination_payouts);
        prop_assert_eq!(eager.destination_payouts, delayed.destination_payouts);
        prop_assert!(eager.destination_payouts.iter().all(|payout| *payout != 0));
        prop_assert!(eager.saw_price_movement);
        prop_assert_eq!(eager.saw_funding, irregular.saw_funding);
        prop_assert_eq!(eager.saw_funding, delayed.saw_funding);
        prop_assert!(eager.max_compute_units < TX_CU_LIMIT);
        prop_assert!(irregular.max_compute_units < TX_CU_LIMIT);
        prop_assert!(delayed.max_compute_units < TX_CU_LIMIT);
    }

    #[test]
    fn v16_program_generated_target_histories_preserve_shutdown_recovery_exit(
        seed in any::<[u8; 32]>(),
        raw_episodes in prop::collection::vec(
            (1u64..=8, 5_000u64..=50_000, any::<bool>(), any::<u16>()),
            3..=3,
        ),
    ) {
        let episodes = normalize_target_episodes(&raw_episodes);
        let suffix = HistorySuffix::ShutdownForfeit;
        let eager = run_target_history(seed, &episodes, CrankPartition::Eager, suffix)
            .map_err(TestCaseError::fail)?;
        let irregular = run_target_history(seed, &episodes, CrankPartition::Irregular, suffix)
            .map_err(TestCaseError::fail)?;
        let delayed = run_target_history(seed, &episodes, CrankPartition::EndpointOnly, suffix)
            .map_err(TestCaseError::fail)?;

        prop_assert!(
            eager.prefixes == irregular.prefixes,
            "eager/irregular pre-shutdown divergence: {}",
            prefix_difference(&eager.prefixes, &irregular.prefixes),
        );
        prop_assert!(
            eager.prefixes == delayed.prefixes,
            "eager/delayed pre-shutdown divergence: {}",
            prefix_difference(&eager.prefixes, &delayed.prefixes),
        );
        backing_expiry_partition_envelope(&episodes, &eager, &irregular, &delayed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(&eager.post_suffix, &irregular.post_suffix);
        prop_assert_eq!(&eager.post_suffix, &delayed.post_suffix);
        prop_assert_eq!(eager.destination_payouts, [0, 0]);
        prop_assert_eq!(irregular.destination_payouts, [0, 0]);
        prop_assert_eq!(delayed.destination_payouts, [0, 0]);
        prop_assert!(eager.saw_price_movement);
        prop_assert_eq!(eager.saw_funding, irregular.saw_funding);
        prop_assert_eq!(eager.saw_funding, delayed.saw_funding);
        prop_assert!(eager.max_compute_units < TX_CU_LIMIT);
        prop_assert!(irregular.max_compute_units < TX_CU_LIMIT);
        prop_assert!(delayed.max_compute_units < TX_CU_LIMIT);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_052_backing_conversion_partition.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_backed_claim_conversion_is_atomic_under_split_caps(
        seed in any::<[u8; 32]>(),
        first_cap_raw in any::<u64>(),
    ) {
        const BACKING_ATOMS: u128 = 100;
        let first_cap = 1 + u128::from(first_cap_raw) % (BACKING_ATOMS - 1);
        let second_cap = BACKING_ATOMS - first_cap;
        for route in [
            TradeRoute::NoCpi,
            TradeRoute::Cpi,
            TradeRoute::BatchNoCpi,
            TradeRoute::BatchCpi,
        ] {
            let aggregate = run_backing_conversion_partition(seed, route, &[])
                .map_err(TestCaseError::fail)?;
            let split = run_backing_conversion_partition(
                seed,
                route,
                &[first_cap, second_cap],
            )
            .map_err(TestCaseError::fail)?;
            let reversed = run_backing_conversion_partition(
                seed,
                route,
                &[second_cap, first_cap],
            )
            .map_err(TestCaseError::fail)?;

            prop_assert_eq!(&aggregate.frame, &split.frame);
            prop_assert_eq!(&aggregate.frame, &reversed.frame);
            prop_assert_eq!(split.public_steps, aggregate.public_steps + 2);
            prop_assert_eq!(reversed.public_steps, aggregate.public_steps + 2);
            prop_assert!(aggregate.max_compute_units < TX_CU_LIMIT);
            prop_assert!(split.max_compute_units < TX_CU_LIMIT);
            prop_assert!(reversed.max_compute_units < TX_CU_LIMIT);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_052_insurance_withdrawal_partition.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_live_insurance_withdrawal_is_split_merge_invariant(
        seed in any::<[u8; 32]>(),
        long_budget_raw in 2u64..=500_000,
        short_budget_raw in 2u64..=500_000,
        crossing_raw in any::<u64>(),
        first_part_raw in any::<u64>(),
    ) {
        let long_budget = u128::from(long_budget_raw);
        let short_budget = u128::from(short_budget_raw);
        let crossing = 1 + u128::from(crossing_raw) % short_budget;
        let total = long_budget + crossing;
        let first_part = 1 + u128::from(first_part_raw) % (total - 1);
        let second_part = total - first_part;

        let aggregate = run_insurance_withdrawal_partition(
            seed,
            long_budget,
            short_budget,
            &[total],
        )
        .map_err(TestCaseError::fail)?;
        let split = run_insurance_withdrawal_partition(
            seed,
            long_budget,
            short_budget,
            &[first_part, second_part],
        )
        .map_err(TestCaseError::fail)?;
        let reversed = run_insurance_withdrawal_partition(
            seed,
            long_budget,
            short_budget,
            &[second_part, first_part],
        )
        .map_err(TestCaseError::fail)?;

        prop_assert_eq!(&aggregate.frame, &split.frame);
        prop_assert_eq!(&aggregate.frame, &reversed.frame);
        prop_assert_eq!(aggregate.public_steps, 4);
        prop_assert_eq!(split.public_steps, 5);
        prop_assert_eq!(reversed.public_steps, 5);
        prop_assert!(aggregate.max_compute_units < TX_CU_LIMIT);
        prop_assert!(split.max_compute_units < TX_CU_LIMIT);
        prop_assert!(reversed.max_compute_units < TX_CU_LIMIT);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_052_rebalance_partition.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_generated_owner_rebalance_partitions_are_invariant(
        open_units in 3u64..=32,
        open_dust in 0u64..u64::try_from(POS_SCALE).unwrap(),
        raw_reduction in any::<u64>(),
        raw_first_part in any::<u64>(),
    ) {
        let open_q = u128::from(open_units) * POS_SCALE + u128::from(open_dust);
        let reduction_q = 2 + u128::from(raw_reduction) % (open_q - 2);
        let first_q = 1 + u128::from(raw_first_part) % (reduction_q - 1);
        let second_q = reduction_q - first_q;
        let seed = [0x54; 32];

        let aggregate = run_rebalance_partition(seed, open_q, &[reduction_q])
            .map_err(TestCaseError::fail)?;
        let split = run_rebalance_partition(seed, open_q, &[first_q, second_q])
            .map_err(TestCaseError::fail)?;
        let reversed = run_rebalance_partition(seed, open_q, &[second_q, first_q])
            .map_err(TestCaseError::fail)?;

        let aggregate_a = aggregate.snapshot.group.assets[0].a_short;
        let split_a = split.snapshot.group.assets[0].a_short;
        let reversed_a = reversed.snapshot.group.assets[0].a_short;
        prop_assert_eq!(aggregate_a, expected_rebalance_a(open_q, &[reduction_q]));
        prop_assert_eq!(split_a, expected_rebalance_a(open_q, &[first_q, second_q]));
        prop_assert_eq!(reversed_a, expected_rebalance_a(open_q, &[second_q, first_q]));
        prop_assert!(split_a <= aggregate_a);
        prop_assert!(reversed_a <= aggregate_a);
        prop_assert!(aggregate_a - split_a <= 1);
        prop_assert!(aggregate_a - reversed_a <= 1);

        let aggregate_without_a =
            rebalance_snapshot_without_adl_rounding(aggregate.snapshot.clone());
        let split_without_a = rebalance_snapshot_without_adl_rounding(split.snapshot.clone());
        let reversed_without_a =
            rebalance_snapshot_without_adl_rounding(reversed.snapshot.clone());
        prop_assert!(
            aggregate_without_a == split_without_a,
            "aggregate/split rebalance divergence outside bounded A rounding: {}",
            prefix_difference(
                std::slice::from_ref(&aggregate_without_a),
                std::slice::from_ref(&split_without_a),
            ),
        );
        prop_assert!(
            aggregate_without_a == reversed_without_a,
            "aggregate/reversed rebalance divergence outside bounded A rounding: {}",
            prefix_difference(
                std::slice::from_ref(&aggregate_without_a),
                std::slice::from_ref(&reversed_without_a),
            ),
        );

        let aggregate_effective = counterparty_effective_scan(&aggregate);
        let split_effective = counterparty_effective_scan(&split);
        let reversed_effective = counterparty_effective_scan(&reversed);
        prop_assert!(split_effective <= aggregate_effective);
        prop_assert!(reversed_effective <= aggregate_effective);
        prop_assert!(aggregate_effective - split_effective <= 1);
        prop_assert!(aggregate_effective - reversed_effective <= 1);
        let expected_basis = i128::try_from(open_q - reduction_q).unwrap();
        prop_assert_eq!(aggregate.final_basis_q, expected_basis);
        prop_assert_eq!(aggregate.final_basis_q, split.final_basis_q);
        prop_assert_eq!(aggregate.final_basis_q, reversed.final_basis_q);
        prop_assert_eq!(aggregate.final_oi_q[0], open_q - reduction_q);
        prop_assert_eq!(aggregate.final_oi_q[0], aggregate.final_oi_q[1]);
        prop_assert_eq!(aggregate.final_oi_q, split.final_oi_q);
        prop_assert_eq!(aggregate.final_oi_q, reversed.final_oi_q);
        for effective in [aggregate_effective, split_effective, reversed_effective] {
            prop_assert!(effective <= aggregate.final_oi_q[1]);
            prop_assert!(aggregate.final_oi_q[1] - effective <= 1);
        }
        prop_assert_eq!(aggregate.public_steps, 1);
        prop_assert_eq!(split.public_steps, 2);
        prop_assert_eq!(reversed.public_steps, 2);
        prop_assert!(aggregate.max_compute_units < TX_CU_LIMIT);
        prop_assert!(split.max_compute_units < TX_CU_LIMIT);
        prop_assert!(reversed.max_compute_units < TX_CU_LIMIT);
    }
}
