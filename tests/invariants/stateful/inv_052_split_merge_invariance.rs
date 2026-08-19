//! INV-052 - Split/merge invariance.
//!
//! Normative obligation: Partitioning an authorized operation must not improve or otherwise alter
//! its normalized economic result, except for explicitly bounded conservative rounding.
//!
//! Evidence in this file (generated F over public I routes): three generated properties build
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
//! preserving authenticated event order.
//!
//! Guarantee boundary: target publication order and target lifetime are identical across the
//! compared executions. Reordering authenticated observations is a different economic history,
//! not a crank partition. Gross paid/received funding counters are cadence-dependent telemetry and
//! must not be consumed as a partition-invariant reward basis; the fixed regression proves their
//! net and all economic state remain equal. Deterministic maximum-shape, Hybrid/Pyth, terminal SPL
//! settlement, and wrapper/engine arithmetic proofs live in the INV-052 CU and Kani files. Exact
//! staleness boundaries and other operation families listed in the coverage matrix remain open.

use super::*;
use crate::support::v16_svm::{MarketConfig, V16Svm, TX_CU_LIMIT};
use percolator::POS_SCALE;
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
    // amount, status, source attribution, or custody. Raw expiries are retained
    // and verified separately by `backing_expiry_partition_envelope`.
    for bucket in &mut group.source_backing_buckets {
        if bucket.status == percolator::BackingBucketStatusV16::Fresh {
            bucket.expiry_slot = 0;
        }
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
    format!(
        "prefix {index}: wrapper_equal={}; group_differences={:?}; profiles_equal={}; controls_equal={}; long_equal={}; short_equal={}; tokens_equal={}",
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
                let account = env.primary_portfolio(actor);
                if !percolator::active_bitmap_is_empty(state::portfolio_active_bitmap(&account)) {
                    return Err(format!(
                        "Recovery forfeit retained actor {actor} active exposure"
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
        prop_assert_eq!(&eager.post_suffix, &irregular.post_suffix);
        prop_assert_eq!(&eager.post_suffix, &delayed.post_suffix);
        prop_assert_eq!(eager.destination_payouts, irregular.destination_payouts);
        prop_assert_eq!(eager.destination_payouts, delayed.destination_payouts);
        prop_assert_eq!(eager.prefixes.len(), episodes.len());
        prop_assert!(eager.saw_price_movement);
        prop_assert!(eager.saw_funding);
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
        prop_assert_eq!(&eager.post_suffix, &irregular.post_suffix);
        prop_assert_eq!(&eager.post_suffix, &delayed.post_suffix);
        prop_assert_eq!(eager.destination_payouts, irregular.destination_payouts);
        prop_assert_eq!(eager.destination_payouts, delayed.destination_payouts);
        prop_assert!(eager.destination_payouts.iter().all(|payout| *payout != 0));
        prop_assert!(eager.saw_price_movement);
        prop_assert!(eager.saw_funding);
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
        prop_assert!(eager.saw_funding);
        prop_assert!(eager.max_compute_units < TX_CU_LIMIT);
        prop_assert!(irregular.max_compute_units < TX_CU_LIMIT);
        prop_assert!(delayed.max_compute_units < TX_CU_LIMIT);
    }
}
