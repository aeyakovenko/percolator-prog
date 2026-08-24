//! INV-085 - Proven arithmetic equals deployed arithmetic.
//!
//! Normative obligation: proof/reference arithmetic must match the arithmetic
//! that ships in the wrapper.
//!
//! Evidence in this file (P): twelve symbolic bounded Kani proofs compare deployed
//! price movement, dt clamping, premium funding, fee-weighted EWMA,
//! fee-supported mark movement, and every canonical wrapper-owned fee/notional
//! adapter against independent widened formulas. Every branch-bearing proof has
//! constructive covers and no new explicit assumption.
//!
//! Guarantee boundary: premium funding uses the complete 8-bit input product;
//! EWMA and fee-supported movement use complete 3-bit products because the
//! 8-bit EWMA division circuit exceeded the isolated five-minute budget. This
//! does not prove every full-width multiply/divide, BPF lowering, composite
//! provider scale, or engine arithmetic path equivalent to a bigint model.

use super::*;

fn inv085_ref_price_move_bps_ceil(old: u64, new: u64) -> Option<u64> {
    if old == 0 || old == new {
        return Some(0);
    }
    let diff = old.abs_diff(new) as u128;
    let numerator = diff.checked_mul(10_000)?.checked_add(old as u128 - 1)?;
    u64::try_from(numerator / old as u128).ok()
}

fn inv085_ref_clamp_toward(anchor: u64, target: u64, cap_bps: u64, dt_slots: u64) -> u64 {
    if anchor == 0 || target == 0 {
        return target;
    }
    if cap_bps == 0 || dt_slots == 0 {
        return anchor;
    }
    let max_delta = (anchor as u128)
        .saturating_mul(cap_bps as u128)
        .saturating_mul(dt_slots as u128)
        / 10_000;
    let max_delta = max_delta.min(u64::MAX as u128) as u64;
    if target > anchor {
        target.min(anchor.saturating_add(max_delta))
    } else {
        target.max(anchor.saturating_sub(max_delta))
    }
}

fn inv085_ref_premium_funding_rate_e9(mark: u64, index: u64, max_abs_rate_e9: u64) -> Option<i128> {
    if max_abs_rate_e9 == 0 || mark == 0 || index == 0 || mark == index {
        return Some(0);
    }
    let premium =
        (mark.abs_diff(index) as u128).checked_mul(percolator::FUNDING_DEN)? / index as u128;
    let bounded = premium.min(max_abs_rate_e9 as u128);
    let signed = i128::try_from(bounded).ok()?;
    Some(if mark > index { signed } else { -signed })
}

fn inv085_ref_ewma_update(
    old: u64,
    price: u64,
    halflife_slots: u64,
    last_slot: u64,
    now_slot: u64,
    fee_paid: u64,
    mark_min_fee: u64,
) -> u64 {
    if old == 0 {
        return if mark_min_fee > 0 && fee_paid < mark_min_fee {
            0
        } else {
            price
        };
    }
    let dt = now_slot.saturating_sub(last_slot);
    if dt == 0 {
        return old;
    }
    if halflife_slots == 0 {
        return price;
    }
    if fee_paid == 0 && mark_min_fee > 0 {
        return old;
    }
    let mut alpha_bps = 10_000u128 * dt as u128 / (dt as u128 + halflife_slots as u128);
    if mark_min_fee != 0 && fee_paid < mark_min_fee {
        alpha_bps = alpha_bps.saturating_mul(fee_paid as u128) / mark_min_fee as u128;
    }
    let old = old as u128;
    let price = price as u128;
    let out = if price >= old {
        old + (price - old) * alpha_bps / 10_000
    } else {
        old - (old - price) * alpha_bps / 10_000
    };
    out.min(u64::MAX as u128) as u64
}

fn inv085_ref_collected_fee_supported_mark(
    old_mark_e6: u64,
    quoted_mark_e6: u64,
    base_fee_paid: u128,
    mark_externality_notional: u128,
    fee_a: u128,
    fee_b: u128,
) -> Option<u64> {
    if quoted_mark_e6 == 0 || mark_externality_notional == 0 {
        return Some(quoted_mark_e6);
    }
    let base_fee_per_side = base_fee_paid / 2;
    let matched_externality_fee = fee_a
        .saturating_sub(base_fee_per_side)
        .min(fee_b.saturating_sub(base_fee_per_side))
        .checked_mul(2)?;
    let supported_move_bps = matched_externality_fee
        .checked_mul(10_000)?
        .checked_div(mark_externality_notional)?;
    Some(inv085_ref_clamp_toward(
        old_mark_e6,
        quoted_mark_e6,
        u64::try_from(supported_move_bps).unwrap_or(u64::MAX),
        1,
    ))
}

fn inv085_ref_fee_share_floor(amount: u8, share_bps: u8) -> u128 {
    u128::from(amount) * u128::from(share_bps) / 10_000
}

fn inv085_ref_permissionless_market_init_fee(base_fee: u8, asset_index: u8) -> u128 {
    let mut fee = u128::from(base_fee);
    let mut doublings = usize::from(asset_index) / 32;
    while doublings != 0 {
        fee *= 2;
        doublings -= 1;
    }
    fee
}

fn inv085_ref_risk_notional_ceil(size_q: u8, price: u8) -> u128 {
    let numerator = u128::from(size_q) * u128::from(price);
    let denominator = percolator::POS_SCALE;
    numerator / denominator + u128::from(numerator % denominator != 0)
}

fn inv085_ref_ceil_div(num: u8, den: u8) -> Option<u128> {
    if den == 0 {
        return None;
    }
    let numerator = u128::from(num);
    let denominator = u128::from(den);
    Some(numerator / denominator + u128::from(numerator % denominator != 0))
}

fn inv085_ref_two_sided_fee(notional: u8, fee_bps: u8) -> u128 {
    let product = u128::from(notional) * u128::from(fee_bps);
    let one_side = product / 10_000 + u128::from(product % 10_000 != 0);
    one_side * 2
}

fn inv085_ref_fee_bps_for_two_sided_fee(
    notional: u8,
    required_paid: u8,
    min_fee_bps: u8,
    max_fee_bps: u8,
) -> Option<u64> {
    if min_fee_bps > max_fee_bps {
        return None;
    }
    if required_paid == 0 || notional == 0 {
        return Some(u64::from(min_fee_bps));
    }
    let mut selected = max_fee_bps;
    let mut candidate = min_fee_bps;
    while candidate <= max_fee_bps {
        if inv085_ref_two_sided_fee(notional, candidate) >= u128::from(required_paid) {
            selected = candidate;
            break;
        }
        if candidate == u8::MAX {
            break;
        }
        candidate += 1;
    }
    Some(u64::from(selected))
}

fn inv085_ref_batch_leg_fee(abs_size_q: u8, exec_price: u8, fee_bps: u8) -> u128 {
    if abs_size_q == 0 || fee_bps == 0 {
        return 0;
    }
    let notional = inv085_ref_risk_notional_ceil(abs_size_q, exec_price);
    let product = notional * u128::from(fee_bps);
    let denominator = u128::from(percolator::MAX_MARGIN_BPS);
    product / denominator + u128::from(product % denominator != 0)
}

#[kani::proof]
fn kani_v16_inv085_price_move_bps_matches_widened_reference_for_small_symbolic_domain() {
    let old_raw: u8 = kani::any();
    let new_raw: u8 = kani::any();
    let old = old_raw as u64;
    let new = new_raw as u64;

    let deployed = policy_v16::price_move_bps_ceil(old, new);
    let reference = inv085_ref_price_move_bps_ceil(old, new);

    kani::cover!(old == 0 && new > 0, "zero old price boundary");
    kani::cover!(old > 0 && new == 0, "zero new price boundary");
    kani::cover!(old > 0 && new > old, "upward move");
    kani::cover!(old > 0 && new < old, "downward move");
    assert_eq!(deployed, reference);
}

#[kani::proof]
fn kani_v16_inv085_clamp_toward_matches_widened_reference_for_small_symbolic_domain() {
    let anchor_raw: u8 = kani::any();
    let target_raw: u8 = kani::any();
    let cap_raw: u8 = kani::any();
    let dt_raw: u8 = kani::any();
    kani::assume(dt_raw <= 15);
    let anchor = anchor_raw as u64;
    let target = target_raw as u64;
    let cap_bps = cap_raw as u64;
    let dt_slots = dt_raw as u64;

    let deployed =
        percolator_prog::oracle_v16::clamp_toward_engine_dt(anchor, target, cap_bps, dt_slots);
    let reference = inv085_ref_clamp_toward(anchor, target, cap_bps, dt_slots);

    kani::cover!(anchor == 0 && target > 0, "zero anchor bypass");
    kani::cover!(target == 0 && anchor > 0, "zero target bypass");
    kani::cover!(
        anchor > 0 && target > anchor && cap_bps > 0 && dt_slots > 0,
        "upward clamp"
    );
    kani::cover!(
        anchor > 0 && target < anchor && cap_bps > 0 && dt_slots > 0,
        "downward clamp"
    );
    assert_eq!(deployed, reference);
}

#[kani::proof]
fn kani_v16_inv085_premium_funding_matches_widened_reference_for_small_symbolic_domain() {
    let mark = u64::from(kani::any::<u8>());
    let index = u64::from(kani::any::<u8>());
    let cap = u64::from(kani::any::<u8>());

    let deployed = policy_v16::premium_funding_rate_e9(mark, index, cap);
    let reference = inv085_ref_premium_funding_rate_e9(mark, index, cap);

    kani::cover!(mark == 0 && index > 0, "zero mark boundary");
    kani::cover!(index == 0 && mark > 0, "zero index boundary");
    kani::cover!(mark > index && cap > 0, "positive premium");
    kani::cover!(mark < index && cap > 0, "negative premium");
    assert_eq!(deployed, reference);
}

#[kani::proof]
fn kani_v16_inv085_ewma_matches_widened_reference_for_small_symbolic_domain() {
    // Masking constructs the complete 3-bit cross-product without an assumption.
    let old = u64::from(kani::any::<u8>() & 7);
    let price = u64::from(kani::any::<u8>() & 7);
    let halflife_slots = u64::from(kani::any::<u8>() & 7);
    let last_slot = u64::from(kani::any::<u8>() & 7);
    let now_slot = u64::from(kani::any::<u8>() & 7);
    let fee_paid = u64::from(kani::any::<u8>() & 7);
    let mark_min_fee = u64::from(kani::any::<u8>() & 7);

    let deployed = policy_v16::ewma_update(
        old,
        price,
        halflife_slots,
        last_slot,
        now_slot,
        fee_paid,
        mark_min_fee,
    );
    let reference = inv085_ref_ewma_update(
        old,
        price,
        halflife_slots,
        last_slot,
        now_slot,
        fee_paid,
        mark_min_fee,
    );

    kani::cover!(
        old == 0 && fee_paid < mark_min_fee,
        "unfunded initialization"
    );
    kani::cover!(now_slot <= last_slot && old > 0, "zero elapsed slots");
    kani::cover!(
        now_slot > last_slot && old > 0 && price > old && halflife_slots > 0,
        "upward weighted move"
    );
    kani::cover!(
        now_slot > last_slot && old > price && halflife_slots > 0,
        "downward weighted move"
    );
    assert_eq!(deployed, reference);
}

#[kani::proof]
fn kani_v16_inv085_collected_fee_mark_matches_widened_reference_for_small_symbolic_domain() {
    // The complete 3-bit cross-product reaches both fee-allocation sides and every early return.
    let old = u64::from(kani::any::<u8>() & 7);
    let quoted = u64::from(kani::any::<u8>() & 7);
    let base_fee = u128::from(kani::any::<u8>() & 7);
    let externality_notional = u128::from(kani::any::<u8>() & 7);
    let fee_a = u128::from(kani::any::<u8>() & 7);
    let fee_b = u128::from(kani::any::<u8>() & 7);

    let deployed = policy_v16::collected_fee_supported_mark(
        old,
        quoted,
        base_fee,
        externality_notional,
        fee_a,
        fee_b,
    );
    let reference = inv085_ref_collected_fee_supported_mark(
        old,
        quoted,
        base_fee,
        externality_notional,
        fee_a,
        fee_b,
    );

    kani::cover!(quoted == 0, "zero quoted mark");
    kani::cover!(externality_notional == 0, "zero externality notional");
    kani::cover!(
        quoted > old && externality_notional > 0 && fee_a > base_fee / 2 && fee_b > base_fee / 2,
        "two-sided upward support"
    );
    kani::cover!(
        quoted < old && externality_notional > 0 && fee_a <= base_fee / 2,
        "one side cannot fund a downward move"
    );
    assert_eq!(deployed, reference);
}

#[kani::proof]
fn kani_v16_inv085_fee_share_matches_widened_reference_for_complete_u8_domain() {
    let amount: u8 = kani::any();
    let share_bps: u8 = kani::any();
    let deployed = policy_v16::fee_share_floor(u128::from(amount), u16::from(share_bps));

    kani::cover!(amount == 0 && share_bps > 0, "zero amount");
    kani::cover!(amount > 0 && share_bps == 0, "zero share");
    kani::cover!(amount > 0 && share_bps > 0, "positive fee share");
    assert_eq!(
        deployed,
        Some(inv085_ref_fee_share_floor(amount, share_bps))
    );
}

#[kani::proof]
#[kani::unwind(9)]
fn kani_v16_inv085_market_init_fee_matches_repeated_doubling_for_complete_u8_domain() {
    let base_fee: u8 = kani::any();
    let asset_index: u8 = kani::any();
    let deployed = policy_v16::permissionless_market_init_fee_for_asset(
        u128::from(base_fee),
        usize::from(asset_index),
    );

    kani::cover!(base_fee == 0 && asset_index >= 32, "zero fee remains zero");
    kani::cover!(base_fee > 0 && asset_index < 32, "base tier");
    kani::cover!(base_fee > 0 && asset_index >= 224, "seventh doubling tier");
    assert_eq!(
        deployed,
        Some(inv085_ref_permissionless_market_init_fee(
            base_fee,
            asset_index
        ))
    );
}

#[kani::proof]
fn kani_v16_inv085_risk_notional_matches_widened_reference_for_complete_u8_domain() {
    let size_q: u8 = kani::any();
    let price: u8 = kani::any();
    let deployed = policy_v16::risk_notional_ceil(u128::from(size_q), u64::from(price));

    kani::cover!(size_q == 0 && price > 0, "zero position size");
    kani::cover!(size_q > 0 && price == 0, "zero price");
    kani::cover!(size_q > 0 && price > 0, "positive rounded notional");
    assert_eq!(deployed, Some(inv085_ref_risk_notional_ceil(size_q, price)));
}

#[kani::proof]
fn kani_v16_inv085_ceil_div_matches_quotient_remainder_for_complete_u8_domain() {
    let num: u8 = kani::any();
    let den: u8 = kani::any();
    let deployed = policy_v16::ceil_div_u128(u128::from(num), u128::from(den));

    kani::cover!(den == 0, "zero denominator rejects");
    kani::cover!(den > 0 && num % den == 0, "exact quotient");
    kani::cover!(den > 0 && num % den != 0, "rounded quotient");
    assert_eq!(deployed, inv085_ref_ceil_div(num, den));
}

#[kani::proof]
fn kani_v16_inv085_two_sided_fee_matches_widened_reference_for_complete_u8_domain() {
    let notional: u8 = kani::any();
    let fee_bps: u8 = kani::any();
    let deployed = policy_v16::two_sided_trade_fee_paid(u128::from(notional), u64::from(fee_bps));

    kani::cover!(notional == 0 && fee_bps > 0, "zero notional");
    kani::cover!(notional > 0 && fee_bps == 0, "zero fee rate");
    kani::cover!(notional > 0 && fee_bps > 0, "two charged sides");
    assert_eq!(deployed, Some(inv085_ref_two_sided_fee(notional, fee_bps)));
}

#[kani::proof]
#[kani::unwind(10)]
fn kani_v16_inv085_fee_rate_search_matches_exhaustive_complete_three_bit_domain() {
    let notional = kani::any::<u8>() & 7;
    let required_paid = kani::any::<u8>() & 7;
    let min_fee_bps = kani::any::<u8>() & 7;
    let max_fee_bps = kani::any::<u8>() & 7;
    let deployed = policy_v16::fee_bps_for_two_sided_fee_paid(
        u128::from(notional),
        u128::from(required_paid),
        u64::from(min_fee_bps),
        u64::from(max_fee_bps),
    );

    kani::cover!(min_fee_bps > max_fee_bps, "invalid bounds reject");
    kani::cover!(notional == 0 && min_fee_bps <= max_fee_bps, "zero notional");
    kani::cover!(
        notional > 0 && required_paid > 0 && min_fee_bps < max_fee_bps,
        "positive bounded search"
    );
    assert_eq!(
        deployed,
        inv085_ref_fee_bps_for_two_sided_fee(notional, required_paid, min_fee_bps, max_fee_bps,)
    );
}

#[kani::proof]
fn kani_v16_inv085_batch_leg_fee_matches_widened_reference_for_complete_u8_domain() {
    let abs_size_q: u8 = kani::any();
    let exec_price: u8 = kani::any();
    let fee_bps: u8 = kani::any();
    let deployed = policy_v16::batch_leg_fee(
        u128::from(abs_size_q),
        u64::from(exec_price),
        u64::from(fee_bps),
    );

    kani::cover!(abs_size_q == 0 && fee_bps > 0, "zero leg size");
    kani::cover!(abs_size_q > 0 && fee_bps == 0, "zero leg fee");
    kani::cover!(
        abs_size_q > 0 && exec_price > 0 && fee_bps > 0,
        "charged leg"
    );
    assert_eq!(
        deployed,
        Some(inv085_ref_batch_leg_fee(abs_size_q, exec_price, fee_bps))
    );
}
