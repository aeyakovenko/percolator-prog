//! INV-085 - Proven arithmetic equals deployed arithmetic.
//!
//! Normative obligation: arithmetic used by proofs, reference models, and the
//! deployed wrapper must agree on adversarial boundary partitions.
//!
//! Evidence in this file (executable arithmetic differential): deployed policy
//! and oracle movement helpers are compared against small independent widened
//! integer oracles over zero, one, max-minus-one, max, overflow-to-None, and
//! saturation cases. This does not close the full wide-arithmetic proof gap, but
//! it gives INV-085 an invariant-owned executable corpus instead of relying only
//! on route tests that happen to touch arithmetic.

use super::*;

fn inv_085_price_move_bps_ceil_oracle(old: u64, new: u64) -> Option<u64> {
    if old == 0 || old == new {
        return Some(0);
    }
    let diff = old.abs_diff(new) as u128;
    let numerator = diff.checked_mul(10_000)?.checked_add(old as u128 - 1)?;
    u64::try_from(numerator / old as u128).ok()
}

fn inv_085_clamp_toward_oracle(anchor: u64, target: u64, cap_bps: u64, dt: u64) -> u64 {
    if anchor == 0 || target == 0 {
        return target;
    }
    if cap_bps == 0 || dt == 0 {
        return anchor;
    }
    let max_delta = (anchor as u128)
        .saturating_mul(cap_bps as u128)
        .saturating_mul(dt as u128)
        / 10_000;
    let max_delta = max_delta.min(u64::MAX as u128) as u64;
    if target > anchor {
        target.min(anchor.saturating_add(max_delta))
    } else {
        target.max(anchor.saturating_sub(max_delta))
    }
}

fn inv_085_premium_funding_rate_oracle(
    mark: u64,
    index: u64,
    max_abs_rate_e9: u64,
) -> Option<i128> {
    if max_abs_rate_e9 == 0 || mark == 0 || index == 0 || mark == index {
        return Some(0);
    }
    let premium =
        (mark.abs_diff(index) as u128).checked_mul(percolator::FUNDING_DEN)? / index as u128;
    let bounded = premium.min(max_abs_rate_e9 as u128);
    let signed = i128::try_from(bounded).ok()?;
    Some(if mark > index { signed } else { -signed })
}

fn inv_085_ceil_div_u128_oracle(num: u128, den: u128) -> Option<u128> {
    if den == 0 {
        return None;
    }
    Some(num.checked_add(den.checked_sub(1)?)? / den)
}

fn inv_085_two_sided_trade_fee_paid_oracle(notional: u128, fee_bps: u64) -> Option<u128> {
    if notional == 0 || fee_bps == 0 {
        return Some(0);
    }
    let one_side = inv_085_ceil_div_u128_oracle(notional.checked_mul(fee_bps as u128)?, 10_000)?;
    let paid = one_side.checked_mul(2)?;
    if paid > u64::MAX as u128 {
        return None;
    }
    Some(paid)
}

fn inv_085_ewma_update_oracle(
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
    let alpha_bps = 10_000u128 * dt as u128 / (dt as u128 + halflife_slots as u128);
    let alpha_bps = if mark_min_fee == 0 || fee_paid >= mark_min_fee {
        alpha_bps
    } else {
        alpha_bps.saturating_mul(fee_paid as u128) / mark_min_fee as u128
    };
    let old128 = old as u128;
    let price128 = price as u128;
    let out = if price >= old {
        old128 + ((price128 - old128) * alpha_bps / 10_000)
    } else {
        old128 - ((old128 - price128) * alpha_bps / 10_000)
    };
    out.min(u64::MAX as u128) as u64
}

fn inv_085_collected_fee_supported_mark_oracle(
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
    Some(inv_085_clamp_toward_oracle(
        old_mark_e6,
        quoted_mark_e6,
        u64::try_from(supported_move_bps).unwrap_or(u64::MAX),
        1,
    ))
}

fn inv_085_dynamic_fee_bps_bruteforce_oracle(
    base_fee_bps: u64,
    old_mark_e6: u64,
    clamped_exec_e6: u64,
    halflife_slots: u64,
    last_mark_slot: u64,
    now_slot: u64,
    trade_notional: u128,
    mark_externality_notional: u128,
    mark_min_fee: u64,
    min_externality_bps: u64,
) -> Option<u64> {
    if base_fee_bps > percolator_prog::constants::MAX_DYNAMIC_TRADE_FEE_BPS {
        return None;
    }
    for fee_bps in base_fee_bps..=percolator_prog::constants::MAX_DYNAMIC_TRADE_FEE_BPS {
        let fee_paid = u64::try_from(inv_085_two_sided_trade_fee_paid_oracle(
            trade_notional,
            fee_bps,
        )?)
        .ok()?;
        let next_mark = inv_085_ewma_update_oracle(
            old_mark_e6,
            clamped_exec_e6,
            halflife_slots,
            last_mark_slot,
            now_slot,
            fee_paid,
            mark_min_fee,
        );
        let mark_move_bps = inv_085_price_move_bps_ceil_oracle(old_mark_e6, next_mark)?;
        let charged_move_bps = mark_move_bps.max(min_externality_bps);
        let base_paid = inv_085_two_sided_trade_fee_paid_oracle(trade_notional, base_fee_bps)?;
        let mark_fee = inv_085_ceil_div_u128_oracle(
            mark_externality_notional.checked_mul(charged_move_bps as u128)?,
            10_000,
        )?;
        let required = base_paid.checked_add(mark_fee)?;
        let denom = trade_notional.checked_mul(2)?;
        let needed = inv_085_ceil_div_u128_oracle(required.checked_mul(10_000)?, denom)?;
        if needed <= fee_bps as u128 {
            return Some(fee_bps);
        }
    }
    None
}

#[test]
fn v16_program_price_move_bps_matches_widened_oracle_on_boundaries() {
    let cases = [
        (0, 0),
        (0, 1),
        (1, 0),
        (1, 1),
        (1, 2),
        (2, 1),
        (100, 101),
        (100, 99),
        (1_000_000, 997_600),
        (u64::MAX, u64::MAX - 1),
        (u64::MAX - 1, u64::MAX),
        (1, u64::MAX),
        (u64::MAX, 1),
    ];
    for (old, new) in cases {
        assert_eq!(
            percolator_prog::policy_v16::price_move_bps_ceil(old, new),
            inv_085_price_move_bps_ceil_oracle(old, new),
            "price_move_bps_ceil({old}, {new}) diverged from widened oracle"
        );
    }
}

#[test]
fn v16_program_price_clamp_matches_widened_oracle_on_boundaries() {
    let cases = [
        (0, 1, 10_000, 1),
        (1, 0, 10_000, 1),
        (100, 200, 0, 1),
        (100, 200, 10_000, 0),
        (100, 200, 500, 1),
        (100, 1, 500, 1),
        (100, 200, 500, 4),
        (u64::MAX - 10, u64::MAX, 10_000, 1),
        (u64::MAX, 1, u64::MAX, u64::MAX),
        (1, u64::MAX, u64::MAX, u64::MAX),
    ];
    for (anchor, target, cap_bps, dt) in cases {
        assert_eq!(
            oracle_v16::clamp_toward_engine_dt(anchor, target, cap_bps, dt),
            inv_085_clamp_toward_oracle(anchor, target, cap_bps, dt),
            "clamp_toward_engine_dt({anchor}, {target}, {cap_bps}, {dt}) diverged"
        );
    }
}

#[test]
fn v16_program_premium_funding_rate_matches_widened_oracle_on_boundaries() {
    let cases = [
        (0, 100, 1_000),
        (100, 0, 1_000),
        (100, 100, 1_000),
        (101, 100, 1_000_000_000),
        (99, 100, 1_000_000_000),
        (u64::MAX, 1, u64::MAX),
        (1, u64::MAX, u64::MAX),
    ];
    for (mark, index, cap) in cases {
        assert_eq!(
            percolator_prog::policy_v16::premium_funding_rate_e9(mark, index, cap),
            inv_085_premium_funding_rate_oracle(mark, index, cap),
            "premium_funding_rate_e9({mark}, {index}, {cap}) diverged"
        );
    }
}

#[test]
fn v16_program_collected_fee_supported_mark_matches_widened_oracle_on_boundaries() {
    let cases = [
        (100, 200, 0, 10_000, 0, 0),
        (100, 200, 10, 10_000, 10, 10),
        (100, 200, 10, 10_000, 110, 10),
        (100, 1, 10, 10_000, 60, 60),
        (100, 0, 10, 10_000, u128::MAX, u128::MAX),
        (100, 200, 10, 0, 60, 60),
        (u64::MAX - 10, u64::MAX, 0, 1, u128::MAX, u128::MAX),
    ];
    for (old, quoted, base_fee, notional, fee_a, fee_b) in cases {
        assert_eq!(
            percolator_prog::policy_v16::collected_fee_supported_mark(
                old, quoted, base_fee, notional, fee_a, fee_b,
            ),
            inv_085_collected_fee_supported_mark_oracle(
                old, quoted, base_fee, notional, fee_a, fee_b,
            ),
            "collected_fee_supported_mark({old}, {quoted}, {base_fee}, {notional}, {fee_a}, {fee_b}) diverged"
        );
    }
}

#[test]
fn v16_program_dynamic_externality_fee_matches_bruteforce_oracle_on_boundaries() {
    let cases = [
        (0, 100, 100, 10, 0, 1, 1_000, 10_000, 0, 0),
        (7, 100, 200, 10, 0, 1, 1_000, 10_000, 0, 0),
        (7, 100, 1, 10, 0, 1, 1_000, 10_000, 0, 0),
        (7, 100, 200, 10, 0, 5, 1_000, 10_000, 50, 3),
        (7, 100, 200, 0, 0, 5, 1_000, 10_000, 0, 0),
        (10_001, 100, 200, 10, 0, 1, 1_000, 10_000, 0, 0),
        (0, 100, 200, 10, 0, 1, 0, 10_000, 0, 0),
        (0, 100, 200, 10, 0, 1, u128::MAX, 10_000, 0, 0),
        (5, 0, 200, 10, 0, 1, 1_000, 10_000, 0, 0),
        (5, 100, 0, 10, 0, 5, 1_000, 10_000, 0, 0),
    ];
    for (
        base_fee_bps,
        old_mark_e6,
        clamped_exec_e6,
        halflife_slots,
        last_mark_slot,
        now_slot,
        trade_notional,
        mark_externality_notional,
        mark_min_fee,
        min_externality_bps,
    ) in cases
    {
        assert_eq!(
            percolator_prog::policy_v16::dynamic_fee_bps_with_externality_floor(
                base_fee_bps,
                old_mark_e6,
                clamped_exec_e6,
                halflife_slots,
                last_mark_slot,
                now_slot,
                trade_notional,
                mark_externality_notional,
                mark_min_fee,
                min_externality_bps,
            ),
            inv_085_dynamic_fee_bps_bruteforce_oracle(
                base_fee_bps,
                old_mark_e6,
                clamped_exec_e6,
                halflife_slots,
                last_mark_slot,
                now_slot,
                trade_notional,
                mark_externality_notional,
                mark_min_fee,
                min_externality_bps,
            ),
            "dynamic_fee_bps_with_externality_floor({base_fee_bps}, {old_mark_e6}, {clamped_exec_e6}, {halflife_slots}, {last_mark_slot}, {now_slot}, {trade_notional}, {mark_externality_notional}, {mark_min_fee}, {min_externality_bps}) diverged"
        );
    }
}
