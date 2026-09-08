//! INV-088 - Global summaries are not account-local proofs.
//!
//! Normative obligation: market-level counters and side loss-weight totals must equal an
//! independent census of every materialized portfolio. A cached summary cannot silently omit a
//! zero-basis pending obligation or retain weight after that obligation is released.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_pending_obligation_summaries_match_the_complete_portfolio_census` creates a
//! cancellable bankruptcy close through public trades, authenticated marks, cranks, and the owner
//! cure route. The shared stateful oracle scans every portfolio immediately after the cure while a
//! real zero-basis obligation is present, and after every cleanup crank. It requires exact
//! per-side stored/stale/pending counts, exact loss-weight sums, and exact market-wide
//! stale-certificate, B-stale, and negative-PnL account counts. It also derives the exact positive
//! PnL atom and bound-number totals from raw portfolios, and proves the public transition system
//! cannot create a nonzero matured-PnL summary. The test separately requires the intermediate
//! obligation, weight, and positive PnL to be nonzero so the census cannot pass vacuously.
//! `v16_program_materialized_portfolio_summary_tracks_close_and_recreate` proves the same stock
//! and encumbrance census follows a real same-address portfolio dematerialization/reinitialization
//! while the program-assigned incarnation advances. `v16_program_backing_earnings_summary_tracks_public_accrual_and_withdrawal`
//! creates nonzero provider earnings through a consented CPI trade, independently reconciles the
//! aggregate at funding/accrual/withdrawal, and requires the aggregate decrement to equal the
//! provider's SPL credit.
//! `v16_program_three_domain_claim_histories_are_touch_order_and_instance_local` creates three
//! independently valued source claims in one portfolio through every asset/domain insertion order
//! and all four trade transports. A later risk increase must reserve all three source domains in
//! one canonical allocation. After every phase, an account scan is recomputed into per-domain,
//! per-asset, and market-wide totals and compared with the deployed summaries. A valid foreign
//! withdrawal is inserted at four different prefix boundaries and must frame the primary instance.
//!
//! Guarantee boundary: the shared oracle now independently rebuilds every persisted stock/count
//! aggregate with an account, asset, domain, or bucket census. This focused route closes the
//! positive-PnL, pending-obligation/loss-weight, materialized-account, and backing-earnings writer
//! families in bounded public topologies. The three-domain history adds a maximum fixture-width
//! insertion/allocation product, but it is not an arbitrary-history or larger-account theorem.

use crate::support::fuzz_model::{
    assert_public_encumbrance_census, assert_public_stock_census, execute_trade_route,
    run_cure_pending_obligation_dos_probe, run_materialized_portfolio_lifecycle_census,
    verify_cpi_backing_fee_consent, TradeRoute,
};
use crate::support::v16_svm::{MarketConfig, V16Svm, TX_CU_LIMIT};
use percolator::{SideV16, BOUND_SCALE, POS_SCALE};
use percolator_prog::ix::CrankObservationHint;
use std::collections::BTreeMap;

const INV_088_OPEN_PRICE: u64 = 100;
const INV_088_WINNING_PRICE: u64 = 105;
const INV_088_SOURCE_DOMAINS: [usize; 3] = [1, 3, 5];
const INV_088_POSITION_UNITS: [u128; 3] = [1_000, 800, 600];
const INV_088_CLAIM_ATOMS: [u128; 3] = [5_000, 4_000, 3_000];
const INV_088_FINAL_RISK_Q: i128 = 1_200 * POS_SCALE as i128;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Inv088DomainCensus {
    claim_bound_num: u128,
    claim_liened_num: u128,
    counterparty_liened_num: u128,
    insurance_liened_num: u128,
    impaired_claim_num: u128,
    effective_reserved: u128,
    counterparty_backing_num: u128,
    insurance_backing_num: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Inv088AccountProjection {
    capital: u128,
    pnl: i128,
    fee_credits: i128,
    domains: Vec<Inv088DomainCensus>,
    positions: Vec<(u32, u8, i128, u128)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Inv088MarketDomainProjection {
    positive_claim_bound_num: u128,
    exact_positive_claim_num: u128,
    fresh_reserved_backing_num: u128,
    spent_backing_num: u128,
    provider_receivable_num: u128,
    valid_liened_backing_num: u128,
    impaired_liened_backing_num: u128,
    insurance_credit_reserved_num: u128,
    valid_liened_insurance_num: u128,
    impaired_liened_insurance_num: u128,
    credit_rate_num: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Inv088AssetProjection {
    effective_price: u64,
    stored_count: [u64; 2],
    effective_oi_q: [u128; 2],
    loss_weight_num: [u128; 2],
    b_num: [u128; 2],
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Inv088LocalGlobalProjection {
    accounts: Vec<Inv088AccountProjection>,
    local_domains: Vec<Inv088DomainCensus>,
    market_domains: Vec<Inv088MarketDomainProjection>,
    backing_buckets: Vec<percolator::BackingBucketV16>,
    assets: Vec<Inv088AssetProjection>,
    c_tot: u128,
    positive_pnl_total: u128,
    positive_pnl_bound_total_num: u128,
    source_claim_bound_total_num: u128,
    materialized_portfolio_count: u64,
    negative_pnl_account_count: u64,
    vault: u128,
    spl_vault: u64,
    token_supply: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Inv088ForeignFrame {
    market: Vec<u8>,
    portfolio: Vec<u8>,
    source_tokens: u64,
    destination_tokens: u64,
    vault_tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Inv088PrimaryFrame {
    market: Vec<u8>,
    portfolios: Vec<Vec<u8>>,
    backing_ledger: Vec<u8>,
    matcher_contexts: Vec<Vec<u8>>,
    vault_tokens: u64,
    actor_tokens: Vec<(u64, u64)>,
    provider_tokens: (u64, u64),
}

#[derive(Debug)]
struct Inv088TouchOrderOutcome {
    prefixes: Vec<(u8, Inv088LocalGlobalProjection)>,
    allocation: Inv088LocalGlobalProjection,
    terminal: Inv088LocalGlobalProjection,
    primary_payouts: [u64; 2],
    foreign_payout: u64,
    public_steps: usize,
    max_compute_units: u64,
}

fn inv_088_foreign_frame(env: &V16Svm) -> Inv088ForeignFrame {
    Inv088ForeignFrame {
        market: env.market_data(true),
        portfolio: env.foreign_portfolio_data(),
        source_tokens: env.token_amount(env.foreign_actor.source_token),
        destination_tokens: env.token_amount(env.foreign_actor.destination_token),
        vault_tokens: env.token_amount(env.foreign_vault),
    }
}

fn inv_088_primary_frame(env: &V16Svm) -> Inv088PrimaryFrame {
    Inv088PrimaryFrame {
        market: env.market_data(false),
        portfolios: env.all_primary_portfolio_data(),
        backing_ledger: env.backing_domain_ledger_data(),
        matcher_contexts: env.all_matcher_context_data(),
        vault_tokens: env.token_amount(env.vault),
        actor_tokens: env
            .actors
            .iter()
            .map(|actor| {
                (
                    env.token_amount(actor.source_token),
                    env.token_amount(actor.destination_token),
                )
            })
            .collect(),
        provider_tokens: (
            env.token_amount(env.provider_source_token),
            env.token_amount(env.provider_destination_token),
        ),
    }
}

fn inv_088_checked_add(total: &mut u128, value: u128, label: &str) -> Result<(), String> {
    *total = total
        .checked_add(value)
        .ok_or_else(|| format!("{label}: independent census overflow"))?;
    Ok(())
}

fn inv_088_add_domain(
    total: &mut Inv088DomainCensus,
    value: &Inv088DomainCensus,
    label: &str,
) -> Result<(), String> {
    inv_088_checked_add(&mut total.claim_bound_num, value.claim_bound_num, label)?;
    inv_088_checked_add(&mut total.claim_liened_num, value.claim_liened_num, label)?;
    inv_088_checked_add(
        &mut total.counterparty_liened_num,
        value.counterparty_liened_num,
        label,
    )?;
    inv_088_checked_add(
        &mut total.insurance_liened_num,
        value.insurance_liened_num,
        label,
    )?;
    inv_088_checked_add(
        &mut total.impaired_claim_num,
        value.impaired_claim_num,
        label,
    )?;
    inv_088_checked_add(
        &mut total.effective_reserved,
        value.effective_reserved,
        label,
    )?;
    inv_088_checked_add(
        &mut total.counterparty_backing_num,
        value.counterparty_backing_num,
        label,
    )?;
    inv_088_checked_add(
        &mut total.insurance_backing_num,
        value.insurance_backing_num,
        label,
    )
}

fn inv_088_local_global_projection(
    env: &V16Svm,
    expected_foreign: &Inv088ForeignFrame,
    label: &str,
) -> Result<Inv088LocalGlobalProjection, String> {
    assert_public_stock_census(label, env)?;
    assert_public_encumbrance_census(label, env)?;
    if inv_088_foreign_frame(env) != *expected_foreign {
        return Err(format!(
            "{label}: a primary transition mutated the foreign market instance"
        ));
    }

    let group = env.primary_market_state().1;
    let domain_count = group.source_credit.len();
    let mut local_domains = vec![Inv088DomainCensus::default(); domain_count];
    let mut accounts = Vec::new();
    let mut capital_total = 0u128;
    let mut positive_pnl_total = 0u128;
    let mut negative_pnl_count = 0u64;
    let mut stored_count = vec![[0u64; 2]; group.assets.len()];
    let mut raw_oi_q = vec![[0u128; 2]; group.assets.len()];
    let mut effective_oi_floor_q = vec![[0u128; 2]; group.assets.len()];

    for actor in 0..env.actors.len() {
        if env.primary_portfolio_data(actor).is_empty() {
            continue;
        }
        let account = env.primary_portfolio(actor);
        inv_088_checked_add(&mut capital_total, account.capital.get(), label)?;
        if account.pnl.get() > 0 {
            inv_088_checked_add(&mut positive_pnl_total, account.pnl.get() as u128, label)?;
        } else if account.pnl.get() < 0 {
            negative_pnl_count = negative_pnl_count
                .checked_add(1)
                .ok_or_else(|| format!("{label}: negative-PnL count overflow"))?;
        }

        let mut account_domains = vec![Inv088DomainCensus::default(); domain_count];
        for source in account.source_domains {
            if !source.is_occupied() {
                continue;
            }
            let domain = source.domain.get() as usize;
            let asset = domain / 2;
            if domain >= domain_count || asset >= group.assets.len() {
                return Err(format!(
                    "{label}: actor {actor} names out-of-range source domain {domain}"
                ));
            }
            if source.source_claim_market_id.get() != group.assets[asset].market_id {
                return Err(format!(
                    "{label}: actor {actor} source domain {domain} crossed asset generation"
                ));
            }
            let local = Inv088DomainCensus {
                claim_bound_num: source.source_claim_bound_num.get(),
                claim_liened_num: source.source_claim_liened_num.get(),
                counterparty_liened_num: source.source_claim_counterparty_liened_num.get(),
                insurance_liened_num: source.source_claim_insurance_liened_num.get(),
                impaired_claim_num: source.source_claim_impaired_num.get(),
                effective_reserved: source.source_lien_effective_reserved.get(),
                counterparty_backing_num: source.source_lien_counterparty_backing_num.get(),
                insurance_backing_num: source.source_lien_insurance_backing_num.get(),
            };
            let classified_lien = local
                .counterparty_liened_num
                .checked_add(local.insurance_liened_num)
                .ok_or_else(|| {
                    format!("{label}: actor {actor} domain {domain} lien-face overflow")
                })?;
            if classified_lien != local.claim_liened_num {
                return Err(format!(
                    "{label}: actor {actor} domain {domain} lien face is not singly classified"
                ));
            }
            inv_088_add_domain(&mut account_domains[domain], &local, label)?;
            inv_088_add_domain(&mut local_domains[domain], &local, label)?;
        }

        let mut positions = Vec::new();
        for (slot, encoded_leg) in account.legs.iter().enumerate() {
            let leg = encoded_leg.try_to_runtime().map_err(|error| {
                format!("{label}: actor {actor} leg {slot} failed to decode: {error:?}")
            })?;
            if !leg.active {
                continue;
            }
            let asset = leg.asset_index as usize;
            if asset >= group.assets.len() || leg.a_basis == 0 {
                return Err(format!(
                    "{label}: actor {actor} has malformed active asset {}",
                    leg.asset_index
                ));
            }
            let side = match leg.side {
                SideV16::Long => 0,
                SideV16::Short => 1,
            };
            let abs_q = leg.basis_pos_q.unsigned_abs();
            stored_count[asset][side] = stored_count[asset][side]
                .checked_add(1)
                .ok_or_else(|| format!("{label}: stored-position count overflow"))?;
            inv_088_checked_add(&mut raw_oi_q[asset][side], abs_q, label)?;
            let current_a = if side == 0 {
                group.assets[asset].a_long
            } else {
                group.assets[asset].a_short
            };
            let effective_q = abs_q
                .checked_mul(current_a)
                .ok_or_else(|| format!("{label}: effective-position product overflow"))?
                / leg.a_basis;
            inv_088_checked_add(&mut effective_oi_floor_q[asset][side], effective_q, label)?;
            positions.push((leg.asset_index, side as u8, leg.basis_pos_q, leg.a_basis));
        }
        positions.sort_unstable();
        accounts.push(Inv088AccountProjection {
            capital: account.capital.get(),
            pnl: account.pnl.get(),
            fee_credits: account.fee_credits.get(),
            domains: account_domains,
            positions,
        });
    }

    let materialized_count = u64::try_from(accounts.len())
        .map_err(|_| format!("{label}: materialized account count exceeds u64"))?;
    if capital_total != group.c_tot
        || positive_pnl_total != group.pnl_pos_tot
        || positive_pnl_total.checked_mul(BOUND_SCALE) != Some(group.pnl_pos_bound_tot_num)
        || positive_pnl_total != group.pnl_pos_bound_tot
        || negative_pnl_count != group.negative_pnl_account_count
        || materialized_count != group.materialized_portfolio_count
    {
        return Err(format!(
            "{label}: account-local capital/PnL/count census diverged from global summaries"
        ));
    }

    let mut source_claim_total = 0u128;
    let mut market_domains = Vec::with_capacity(domain_count);
    for (domain, (local, source)) in local_domains
        .iter()
        .zip(group.source_credit.iter())
        .enumerate()
    {
        if local.claim_bound_num != source.positive_claim_bound_num {
            return Err(format!(
                "{label}: domain {domain} local claim {} != domain summary {}",
                local.claim_bound_num, source.positive_claim_bound_num
            ));
        }
        inv_088_checked_add(
            &mut source_claim_total,
            source.positive_claim_bound_num,
            label,
        )?;
        market_domains.push(Inv088MarketDomainProjection {
            positive_claim_bound_num: source.positive_claim_bound_num,
            exact_positive_claim_num: source.exact_positive_claim_num,
            fresh_reserved_backing_num: source.fresh_reserved_backing_num,
            spent_backing_num: source.spent_backing_num,
            provider_receivable_num: source.provider_receivable_num,
            valid_liened_backing_num: source.valid_liened_backing_num,
            impaired_liened_backing_num: source.impaired_liened_backing_num,
            insurance_credit_reserved_num: source.insurance_credit_reserved_num,
            valid_liened_insurance_num: source.valid_liened_insurance_num,
            impaired_liened_insurance_num: source.impaired_liened_insurance_num,
            credit_rate_num: source.credit_rate_num,
        });
    }
    if source_claim_total != group.source_claim_bound_total_num {
        return Err(format!(
            "{label}: per-domain claim census {source_claim_total} != global summary {}",
            group.source_claim_bound_total_num
        ));
    }

    let mut assets = Vec::with_capacity(group.assets.len());
    for (asset_index, asset) in group.assets.iter().enumerate() {
        let engine_oi = [asset.oi_eff_long_q, asset.oi_eff_short_q];
        for side in 0..2 {
            if stored_count[asset_index][side]
                != [asset.stored_pos_count_long, asset.stored_pos_count_short][side]
                || engine_oi[side] > raw_oi_q[asset_index][side]
                || engine_oi[side] < effective_oi_floor_q[asset_index][side]
                || engine_oi[side] - effective_oi_floor_q[asset_index][side]
                    > u128::from(stored_count[asset_index][side])
            {
                return Err(format!(
                    "{label}: asset {asset_index} side {side} local position census diverged from stored summaries"
                ));
            }
        }
        assets.push(Inv088AssetProjection {
            effective_price: asset.effective_price,
            stored_count: [asset.stored_pos_count_long, asset.stored_pos_count_short],
            effective_oi_q: engine_oi,
            loss_weight_num: [asset.loss_weight_sum_long, asset.loss_weight_sum_short],
            b_num: [asset.b_long_num, asset.b_short_num],
        });
    }
    if group.vault != u128::from(env.token_amount(env.vault)) {
        return Err(format!("{label}: engine/SPL primary vault mismatch"));
    }

    Ok(Inv088LocalGlobalProjection {
        accounts,
        local_domains,
        market_domains,
        backing_buckets: group.source_backing_buckets.to_vec(),
        assets,
        c_tot: group.c_tot,
        positive_pnl_total,
        positive_pnl_bound_total_num: group.pnl_pos_bound_tot_num,
        source_claim_bound_total_num: group.source_claim_bound_total_num,
        materialized_portfolio_count: group.materialized_portfolio_count,
        negative_pnl_account_count: group.negative_pnl_account_count,
        vault: group.vault,
        spl_vault: env.token_amount(env.vault),
        token_supply: env.token_supply_observed(),
    })
}

fn inv_088_crank_to_fixed_point(
    env: &mut V16Svm,
    actor: usize,
    asset: u16,
    label: &str,
    max_compute_units: &mut u64,
) -> Result<(), String> {
    let observations = vec![CrankObservationHint {
        asset_index: asset,
        oracle_accounts: env.primary_profile(asset as usize).oracle_leg_count,
    }];
    for _ in 0..8 {
        match env.crank_if_actionable(actor, 2, observations.clone())? {
            Some(success) => *max_compute_units = (*max_compute_units).max(success.compute_units),
            None => return Ok(()),
        }
    }
    Err(format!(
        "{label}: actor {actor} did not reach a public crank fixed point"
    ))
}

fn inv_088_withdraw_foreign_instance(
    env: &mut V16Svm,
    max_compute_units: &mut u64,
    label: &str,
) -> Result<Inv088ForeignFrame, String> {
    let primary_before = inv_088_primary_frame(env);
    let capital = env.foreign_market_state().1.c_tot;
    let destination_before = env.token_amount(env.foreign_actor.destination_token);
    let success = env
        .withdraw_foreign(capital)
        .map_err(|error| format!("{label}: valid foreign withdrawal: {error}"))?;
    *max_compute_units = (*max_compute_units).max(success.compute_units);
    if inv_088_primary_frame(env) != primary_before {
        return Err(format!(
            "{label}: foreign withdrawal mutated the primary market instance"
        ));
    }
    if env
        .token_amount(env.foreign_actor.destination_token)
        .checked_sub(destination_before)
        != u64::try_from(capital).ok()
    {
        return Err(format!("{label}: foreign withdrawal payout mismatch"));
    }
    Ok(inv_088_foreign_frame(env))
}

fn inv_088_assert_completed_claim_prefix(
    projection: &Inv088LocalGlobalProjection,
    touched_mask: u8,
    label: &str,
) {
    let expected_positive = INV_088_CLAIM_ATOMS
        .iter()
        .enumerate()
        .filter(|(asset, _)| touched_mask & (1 << asset) != 0)
        .map(|(_, claim)| *claim)
        .sum::<u128>();
    assert_eq!(projection.positive_pnl_total, expected_positive, "{label}");
    assert_eq!(
        projection.source_claim_bound_total_num,
        expected_positive * BOUND_SCALE,
        "{label}"
    );
    assert_eq!(
        projection.accounts[0].pnl, expected_positive as i128,
        "{label}"
    );
    assert_eq!(
        projection.accounts[1].capital,
        1_000_000 - expected_positive,
        "{label}"
    );
    for asset in 0..3 {
        let expected_num = if touched_mask & (1 << asset) != 0 {
            INV_088_CLAIM_ATOMS[asset] * BOUND_SCALE
        } else {
            0
        };
        let domain = INV_088_SOURCE_DOMAINS[asset];
        assert_eq!(
            projection.accounts[0].domains[domain].claim_bound_num, expected_num,
            "{label}: account-local domain {domain}"
        );
        assert_eq!(
            projection.local_domains[domain].claim_bound_num, expected_num,
            "{label}: complete domain {domain} scan"
        );
        assert_eq!(
            projection.market_domains[domain].positive_claim_bound_num, expected_num,
            "{label}: market domain {domain} summary"
        );
        assert_eq!(
            projection.assets[asset].effective_price,
            if expected_num == 0 {
                INV_088_OPEN_PRICE
            } else {
                INV_088_WINNING_PRICE
            },
            "{label}: asset-local mark"
        );
        assert_eq!(projection.assets[asset].stored_count, [0, 0], "{label}");
        assert_eq!(projection.assets[asset].effective_oi_q, [0, 0], "{label}");
        assert_eq!(projection.assets[asset].loss_weight_num, [0, 0], "{label}");
        assert_eq!(projection.assets[asset].b_num, [0, 0], "{label}");
    }
    assert!(projection
        .local_domains
        .iter()
        .all(|domain| domain.claim_liened_num == 0));
}

fn inv_088_run_three_domain_touch_order(
    route: TradeRoute,
    order: [usize; 3],
    foreign_touch_after: usize,
) -> Result<Inv088TouchOrderOutcome, String> {
    let config = MarketConfig {
        initial_price: INV_088_OPEN_PRICE,
        h_max: 10,
        maintenance_margin_bps: 1_000,
        initial_margin_bps: 5_000,
        max_price_move_bps_per_slot: 500,
        max_accrual_dt_slots: 1,
        max_abs_funding_e9_per_slot: 0,
        min_funding_lifetime_slots: 1,
        maintenance_fee_per_slot: 0,
        actor_deposits: [52_502, 1_000_000, 1, 1, 1],
        ..MarketConfig::default()
    };
    let mut env = V16Svm::new([0x88; 32], config);
    env.warp_to_slot(2);
    env.begin_public_trace();
    let mut expected_foreign = inv_088_foreign_frame(&env);
    let token_supply = env.token_supply_observed();
    let mut max_compute_units = 0u64;

    for &domain in &INV_088_SOURCE_DOMAINS {
        let success = env
            .top_up_backing_bucket_without_ledger(domain as u16, 50_000, 100)
            .map_err(|error| format!("{route:?}/{order:?}: top up domain {domain}: {error}"))?;
        max_compute_units = max_compute_units.max(success.compute_units);
        inv_088_local_global_projection(
            &env,
            &expected_foreign,
            &format!("{route:?}/{order:?} after domain {domain} backing"),
        )?;
    }

    if foreign_touch_after == 0 {
        expected_foreign = inv_088_withdraw_foreign_instance(
            &mut env,
            &mut max_compute_units,
            &format!("{route:?}/{order:?} foreign boundary 0"),
        )?;
    }

    let mut touched_mask = 0u8;
    let mut prefixes = Vec::new();
    for (step, asset) in order.into_iter().enumerate() {
        let asset_u16 = asset as u16;
        let size_q = i128::try_from(INV_088_POSITION_UNITS[asset] * POS_SCALE)
            .map_err(|_| "three-domain position exceeds i128".to_string())?;
        let case = format!("{route:?}/{order:?}/asset={asset}");
        let open = execute_trade_route(
            &mut env,
            route,
            0,
            1,
            asset_u16,
            size_q,
            INV_088_OPEN_PRICE,
            0,
        )
        .map_err(|error| format!("{case}: open claim episode: {error}"))?;
        max_compute_units = max_compute_units.max(open.compute_units);
        inv_088_local_global_projection(&env, &expected_foreign, &format!("{case} open"))?;

        let mark = env
            .push_auth_mark(asset_u16, 2, INV_088_WINNING_PRICE)
            .map_err(|error| format!("{case}: publish winning mark: {error}"))?;
        max_compute_units = max_compute_units.max(mark.compute_units);
        inv_088_local_global_projection(&env, &expected_foreign, &format!("{case} mark"))?;
        inv_088_crank_to_fixed_point(&mut env, 1, asset_u16, &case, &mut max_compute_units)?;
        inv_088_crank_to_fixed_point(&mut env, 0, asset_u16, &case, &mut max_compute_units)?;
        inv_088_local_global_projection(&env, &expected_foreign, &format!("{case} settled mark"))?;

        let close = execute_trade_route(
            &mut env,
            route,
            0,
            1,
            asset_u16,
            -size_q,
            INV_088_WINNING_PRICE,
            0,
        )
        .map_err(|error| format!("{case}: close claim episode: {error}"))?;
        max_compute_units = max_compute_units.max(close.compute_units);
        inv_088_local_global_projection(&env, &expected_foreign, &format!("{case} close"))?;
        inv_088_crank_to_fixed_point(&mut env, 1, asset_u16, &case, &mut max_compute_units)?;
        inv_088_crank_to_fixed_point(&mut env, 0, asset_u16, &case, &mut max_compute_units)?;

        touched_mask |= 1 << asset;
        let prefix =
            inv_088_local_global_projection(&env, &expected_foreign, &format!("{case} completed"))?;
        inv_088_assert_completed_claim_prefix(&prefix, touched_mask, &case);
        prefixes.push((touched_mask, prefix));

        if foreign_touch_after == step + 1 {
            expected_foreign = inv_088_withdraw_foreign_instance(
                &mut env,
                &mut max_compute_units,
                &format!("{route:?}/{order:?} foreign boundary {}", step + 1),
            )?;
            inv_088_local_global_projection(
                &env,
                &expected_foreign,
                &format!("{route:?}/{order:?} after foreign withdrawal"),
            )?;
        }
    }

    let risk_open = execute_trade_route(
        &mut env,
        route,
        0,
        1,
        0,
        INV_088_FINAL_RISK_Q,
        INV_088_WINNING_PRICE,
        0,
    )
    .map_err(|error| format!("{route:?}/{order:?}: open shared supported risk: {error}"))?;
    max_compute_units = max_compute_units.max(risk_open.compute_units);
    let allocation = inv_088_local_global_projection(
        &env,
        &expected_foreign,
        &format!("{route:?}/{order:?} shared allocation"),
    )?;
    let liened_domains = INV_088_SOURCE_DOMAINS
        .iter()
        .filter(|domain| allocation.accounts[0].domains[**domain].claim_liened_num > 0)
        .count();
    if liened_domains != INV_088_SOURCE_DOMAINS.len() {
        return Err(format!(
            "{route:?}/{order:?}: shared risk reserved only {liened_domains} of three source domains"
        ));
    }

    let risk_close = execute_trade_route(
        &mut env,
        route,
        0,
        1,
        0,
        -INV_088_FINAL_RISK_Q,
        INV_088_WINNING_PRICE,
        0,
    )
    .map_err(|error| format!("{route:?}/{order:?}: close shared supported risk: {error}"))?;
    max_compute_units = max_compute_units.max(risk_close.compute_units);
    inv_088_crank_to_fixed_point(&mut env, 1, 0, "shared risk close", &mut max_compute_units)?;
    inv_088_crank_to_fixed_point(&mut env, 0, 0, "shared risk close", &mut max_compute_units)?;
    let released = inv_088_local_global_projection(
        &env,
        &expected_foreign,
        &format!("{route:?}/{order:?} released allocation"),
    )?;
    inv_088_assert_completed_claim_prefix(&released, 0b111, "released shared allocation");

    let total_claim = INV_088_CLAIM_ATOMS.iter().sum::<u128>();
    let conversion = env
        .convert_released_pnl(0, total_claim)
        .map_err(|error| format!("{route:?}/{order:?}: convert all released claims: {error}"))?;
    max_compute_units = max_compute_units.max(conversion.compute_units);
    let converted = inv_088_local_global_projection(
        &env,
        &expected_foreign,
        &format!("{route:?}/{order:?} converted claims"),
    )?;
    if converted.positive_pnl_total != 0
        || converted.source_claim_bound_total_num != 0
        || converted
            .local_domains
            .iter()
            .any(|domain| domain.claim_bound_num != 0 || domain.claim_liened_num != 0)
    {
        return Err(format!(
            "{route:?}/{order:?}: full conversion retained a local or global claim"
        ));
    }

    for actor in [0usize, 1] {
        let capital = env.primary_portfolio(actor).capital.get();
        let withdrawal = env
            .withdraw_primary(actor, capital)
            .map_err(|error| format!("{route:?}/{order:?}: withdraw actor {actor}: {error}"))?;
        max_compute_units = max_compute_units.max(withdrawal.compute_units);
        inv_088_local_global_projection(
            &env,
            &expected_foreign,
            &format!("{route:?}/{order:?} withdrew actor {actor}"),
        )?;
        let close = env
            .close_primary_portfolio(actor)
            .map_err(|error| format!("{route:?}/{order:?}: close actor {actor}: {error}"))?;
        max_compute_units = max_compute_units.max(close.compute_units);
        inv_088_local_global_projection(
            &env,
            &expected_foreign,
            &format!("{route:?}/{order:?} closed actor {actor}"),
        )?;
    }

    let terminal = inv_088_local_global_projection(
        &env,
        &expected_foreign,
        &format!("{route:?}/{order:?} terminal"),
    )?;
    let primary_payouts = [
        env.token_amount(env.actors[0].destination_token),
        env.token_amount(env.actors[1].destination_token),
    ];
    if primary_payouts != [64_502, 988_000]
        || env.token_amount(env.foreign_actor.destination_token) != 100_000_000
        || env.token_supply_observed() != token_supply
    {
        return Err(format!(
            "{route:?}/{order:?}: terminal payout or token-supply mismatch: primary={primary_payouts:?}, foreign={}, supply={}",
            env.token_amount(env.foreign_actor.destination_token),
            env.token_supply_observed()
        ));
    }

    let trace = env.finish_public_trace();
    trace
        .validate_public_execution()
        .map_err(|error| format!("{route:?}/{order:?}: invalid public trace: {error}"))?;
    if trace.out_of_band_economic_mutations != 0
        || trace.steps.iter().any(|step| {
            !step.succeeded
                && (step.rejected_exact_writable_rollback != Some(true)
                    || step.rejected_no_program_lamport_delta != Some(true)
                    || step.token_deltas.iter().any(|(_, delta)| *delta != 0))
        })
    {
        return Err(format!(
            "{route:?}/{order:?}: trace escaped exact public rollback: {trace:?}"
        ));
    }
    max_compute_units = max_compute_units.max(
        trace
            .steps
            .iter()
            .filter_map(|step| step.compute_units)
            .max()
            .unwrap_or(0),
    );

    Ok(Inv088TouchOrderOutcome {
        prefixes,
        allocation,
        terminal,
        primary_payouts,
        foreign_payout: env.token_amount(env.foreign_actor.destination_token),
        public_steps: trace.steps.len(),
        max_compute_units,
    })
}

#[test]
fn v16_program_three_domain_claim_histories_are_touch_order_and_instance_local() {
    const ORDERS: [[usize; 3]; 6] = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    const ROUTES: [TradeRoute; 4] = [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ];

    let mut canonical_prefixes = BTreeMap::<u8, Inv088LocalGlobalProjection>::new();
    let mut canonical_allocation = None;
    let mut canonical_terminal = None;
    let mut worlds = 0usize;
    let mut public_steps = 0usize;
    let mut max_compute_units = 0u64;
    for route in ROUTES {
        for (order_index, order) in ORDERS.into_iter().enumerate() {
            let outcome = inv_088_run_three_domain_touch_order(route, order, order_index % 4)
                .unwrap_or_else(|error| {
                    panic!("INV-034/035/041/074/088 {route:?}/{order:?}: {error}")
                });
            for (mask, prefix) in outcome.prefixes {
                if let Some(canonical) = canonical_prefixes.get(&mask) {
                    assert_eq!(
                        &prefix, canonical,
                        "{route:?}/{order:?}: completed source set {mask:03b} depends on touch order"
                    );
                } else {
                    canonical_prefixes.insert(mask, prefix);
                }
            }
            if let Some(canonical) = canonical_allocation.as_ref() {
                assert_eq!(
                    &outcome.allocation, canonical,
                    "{route:?}/{order:?}: three-domain support allocation depends on insertion order or route"
                );
            } else {
                canonical_allocation = Some(outcome.allocation);
            }
            if let Some(canonical) = canonical_terminal.as_ref() {
                assert_eq!(
                    &outcome.terminal, canonical,
                    "{route:?}/{order:?}: terminal local/global state depends on touch order or route"
                );
            } else {
                canonical_terminal = Some(outcome.terminal);
            }
            assert_eq!(outcome.primary_payouts, [64_502, 988_000]);
            assert_eq!(outcome.foreign_payout, 100_000_000);
            assert!(outcome.public_steps > 0);
            assert!(outcome.max_compute_units < TX_CU_LIMIT);
            worlds += 1;
            public_steps += outcome.public_steps;
            max_compute_units = max_compute_units.max(outcome.max_compute_units);
        }
    }
    assert_eq!(worlds, 24);
    assert_eq!(canonical_prefixes.len(), 7);
    eprintln!(
        "INV-034/035/041/074/088 three-domain histories: worlds={worlds}, public_steps={public_steps}, max_cu={max_compute_units}"
    );
}

#[test]
fn v16_program_materialized_portfolio_summary_tracks_close_and_recreate() {
    let evidence = run_materialized_portfolio_lifecycle_census()
        .expect("public portfolio generations must satisfy the complete aggregate census");
    assert_eq!(
        evidence.after_close_count + 1,
        evidence.initial_count,
        "closing one empty portfolio must remove exactly one materialized account"
    );
    assert_eq!(
        evidence.after_reinitialize_count, evidence.initial_count,
        "reinitializing the same address must add exactly one materialized account"
    );
    assert!(
        evidence.new_portfolio_id > evidence.old_portfolio_id,
        "the replacement portfolio must be a new program-assigned incarnation"
    );
}

#[test]
fn v16_program_pending_obligation_summaries_match_the_complete_portfolio_census() {
    let evidence = run_cure_pending_obligation_dos_probe()
        .expect("public cure and cleanup must satisfy the complete summary census");
    assert!(
        evidence.intermediate_positive_pnl_total > 0,
        "the aggregate census must observe real positive PnL: {evidence:?}"
    );
    assert_eq!(
        evidence.intermediate_positive_pnl_bound_num,
        evidence.intermediate_positive_pnl_total * percolator::BOUND_SCALE,
        "the bound-number aggregate must equal the complete positive-PnL census"
    );
    assert_eq!(
        evidence.intermediate_positive_pnl_atom_bound, evidence.intermediate_positive_pnl_total,
        "the atom-bound aggregate must equal the complete positive-PnL census"
    );
    assert_eq!(
        evidence.intermediate_matured_positive_pnl, 0,
        "no public wrapper route may synthesize matured positive PnL"
    );
    assert!(
        evidence.intermediate_pending_obligation_count > 0,
        "the summary census must observe a real pending obligation: {evidence:?}"
    );
    assert!(
        evidence.intermediate_leg_loss_weight > 0,
        "the pending obligation must retain real social-loss weight: {evidence:?}"
    );
    assert_eq!(
        evidence.intermediate_market_loss_weight, evidence.intermediate_leg_loss_weight,
        "the complete one-obligation portfolio census must equal the market side summary"
    );
    assert_eq!(
        evidence.pending_obligation_count, 0,
        "bounded public cleanup must remove the obligation summary"
    );
    assert_eq!(
        evidence.retained_loss_weight, 0,
        "bounded public cleanup must remove the account-local weight"
    );
}

#[test]
fn v16_program_backing_earnings_summary_tracks_public_accrual_and_withdrawal() {
    let evidence = verify_cpi_backing_fee_consent([0x88; 32])
        .expect("public backing earnings must satisfy the independent aggregate census");
    assert!(
        evidence.provider_earnings > 0,
        "the route must make the backing-earnings writer nonvacuous"
    );
    assert_eq!(
        evidence.provider_earnings,
        u128::from(evidence.extracted_tokens),
        "the aggregate decrement must equal the provider's SPL credit"
    );
    assert_eq!(
        evidence.provider_earnings, evidence.earnings_spl_vault_debit,
        "provider earnings must debit canonical SPL custody exactly"
    );
    assert_eq!(
        evidence.provider_earnings, evidence.earnings_accounted_vault_debit,
        "provider earnings must debit internal quote accounting exactly"
    );
}
