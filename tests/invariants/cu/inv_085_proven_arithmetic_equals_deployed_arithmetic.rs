//! INV-085 - Proven arithmetic equals deployed arithmetic.
//!
//! Normative obligation: arithmetic used by proofs, reference models, and the
//! deployed wrapper must agree on adversarial boundary partitions.
//!
//! Evidence in this file (executable arithmetic differential): a source-derived
//! roster owns all multiply/divide-bearing production functions and keeps every
//! wrapper fee/notional adapter in one pure policy module. Deployed policy and
//! oracle helpers are compared against independent widened integer oracles over
//! fixed boundaries and 16,384 deterministic full-width words. Separate 512- and
//! 1,024-word corpora compare dynamic-fee and fee-rate searches with exhaustive
//! scans. Public LiteSVM witnesses additionally bind the host policy to deployed
//! activation, fee-share, batch-rounding, and Hybrid quote results. Bigint
//! references cover the canonical wrapper arithmetic and provider scaling;
//! a universal symbolic relational provider-scale theorem remains open.

use super::*;
use num_bigint::BigUint;
use num_traits::ToPrimitive;

fn inv_085_big_to_u128(value: BigUint) -> Option<u128> {
    value.to_u128()
}

fn inv_085_big_mul_div_floor(value: u128, multiplier: u64, denominator: u64) -> Option<u128> {
    if denominator == 0 {
        return None;
    }
    inv_085_big_to_u128(
        BigUint::from(value) * BigUint::from(multiplier) / BigUint::from(denominator),
    )
}

fn inv_085_big_mul_div_ceil(value: u128, multiplier: u64, denominator: u64) -> Option<u128> {
    if denominator == 0 {
        return None;
    }
    let denominator = BigUint::from(denominator);
    let product = BigUint::from(value) * BigUint::from(multiplier);
    inv_085_big_to_u128((&product + &denominator - BigUint::from(1u8)) / denominator)
}

fn inv_085_big_ceil_div(num: u128, den: u128) -> Option<u128> {
    if den == 0 {
        return None;
    }
    let den = BigUint::from(den);
    inv_085_big_to_u128((BigUint::from(num) + &den - BigUint::from(1u8)) / den)
}

fn inv_085_big_two_sided_fee(notional: u128, fee_bps: u64) -> Option<u128> {
    if notional == 0 || fee_bps == 0 {
        return Some(0);
    }
    inv_085_big_mul_div_ceil(notional, fee_bps, 10_000)?.checked_mul(2)
}

fn inv_085_big_risk_notional(size_q: u128, price: u64) -> Option<u128> {
    inv_085_big_mul_div_ceil(size_q, price, percolator::POS_SCALE as u64)
}

fn inv_085_big_batch_leg_fee(abs_size_q: u128, exec_price: u64, fee_bps: u64) -> Option<u128> {
    if abs_size_q == 0 || fee_bps == 0 {
        return Some(0);
    }
    let notional = inv_085_big_risk_notional(abs_size_q, exec_price)?;
    if notional == 0 {
        return Some(0);
    }
    inv_085_big_mul_div_ceil(notional, fee_bps, percolator::MAX_MARGIN_BPS)
}

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
    inv_085_big_ceil_div(num, den)
}

fn inv_085_two_sided_trade_fee_paid_oracle(notional: u128, fee_bps: u64) -> Option<u128> {
    let paid = inv_085_big_two_sided_fee(notional, fee_bps)?;
    if paid > u64::MAX as u128 {
        return None;
    }
    Some(paid)
}

fn inv_085_two_sided_trade_fee_paid_uncapped_oracle(notional: u128, fee_bps: u64) -> Option<u128> {
    inv_085_big_two_sided_fee(notional, fee_bps)
}

fn inv_085_fee_share_floor_oracle(amount: u128, share_bps: u16) -> Option<u128> {
    if amount == 0 || share_bps == 0 {
        return Some(0);
    }
    inv_085_big_mul_div_floor(amount, u64::from(share_bps), 10_000)
}

fn inv_085_market_init_fee_oracle(base_fee: u128, asset_index: usize) -> Option<u128> {
    if base_fee == 0 {
        return Some(0);
    }
    let doublings = asset_index / 32;
    if doublings >= u128::BITS as usize {
        return None;
    }
    let mut fee = base_fee;
    for _ in 0..doublings {
        fee = fee.checked_mul(2)?;
    }
    Some(fee)
}

fn inv_085_risk_notional_ceil_oracle(size_q: u128, price: u64) -> Option<u128> {
    inv_085_big_risk_notional(size_q, price)
}

fn inv_085_batch_leg_fee_oracle(abs_size_q: u128, exec_price: u64, fee_bps: u64) -> Option<u128> {
    inv_085_big_batch_leg_fee(abs_size_q, exec_price, fee_bps)
}

fn inv_085_fee_bps_exhaustive_oracle(
    notional: u128,
    required_paid: u128,
    min_fee_bps: u64,
    max_fee_bps: u64,
) -> Option<u64> {
    if min_fee_bps > max_fee_bps {
        return None;
    }
    if required_paid == 0 || notional == 0 {
        return Some(min_fee_bps);
    }
    for candidate in min_fee_bps..=max_fee_bps {
        if inv_085_two_sided_trade_fee_paid_uncapped_oracle(notional, candidate)? >= required_paid {
            return Some(candidate);
        }
    }
    Some(max_fee_bps)
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

fn inv_085_splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut value = *state;
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn inv_085_next_u128(state: &mut u64) -> u128 {
    (u128::from(inv_085_splitmix64(state)) << 64) | u128::from(inv_085_splitmix64(state))
}

fn inv_085_boundary_u64(state: &mut u64, index: usize) -> u64 {
    const BOUNDARIES: [u64; 10] = [
        0,
        1,
        2,
        9_999,
        10_000,
        1_000_000,
        u32::MAX as u64,
        u64::MAX / 2,
        u64::MAX - 1,
        u64::MAX,
    ];
    if index % 3 == 0 {
        BOUNDARIES[(index / 3) % BOUNDARIES.len()]
    } else {
        inv_085_splitmix64(state)
    }
}

fn inv_085_boundary_u128(state: &mut u64, index: usize) -> u128 {
    const BOUNDARIES: [u128; 10] = [
        0,
        1,
        2,
        9_999,
        10_000,
        u64::MAX as u128,
        u64::MAX as u128 + 1,
        u128::MAX / 2,
        u128::MAX - 1,
        u128::MAX,
    ];
    if index % 3 == 0 {
        BOUNDARIES[(index / 3) % BOUNDARIES.len()]
    } else {
        inv_085_next_u128(state)
    }
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

#[test]
fn v16_program_policy_arithmetic_matches_independent_full_width_corpus() {
    const CASES: usize = 16_384;
    let mut state = 0x8500_5eed_cafe_f00d;

    for index in 0..CASES {
        let old = inv_085_boundary_u64(&mut state, index);
        let new = inv_085_boundary_u64(&mut state, index + 1);
        let cap = inv_085_boundary_u64(&mut state, index + 2);
        let dt = inv_085_boundary_u64(&mut state, index + 3);
        assert_eq!(
            percolator_prog::policy_v16::price_move_bps_ceil(old, new),
            inv_085_price_move_bps_ceil_oracle(old, new),
            "price movement diverged at corpus word {index}"
        );
        assert_eq!(
            oracle_v16::clamp_toward_engine_dt(old, new, cap, dt),
            inv_085_clamp_toward_oracle(old, new, cap, dt),
            "dt clamp diverged at corpus word {index}"
        );
        assert_eq!(
            percolator_prog::policy_v16::premium_funding_rate_e9(old, new, cap),
            inv_085_premium_funding_rate_oracle(old, new, cap),
            "premium funding diverged at corpus word {index}"
        );

        let halflife = inv_085_boundary_u64(&mut state, index + 4);
        let last_slot = inv_085_boundary_u64(&mut state, index + 5);
        let now_slot = inv_085_boundary_u64(&mut state, index + 6);
        let fee_paid = inv_085_boundary_u64(&mut state, index + 7);
        let mark_min_fee = inv_085_boundary_u64(&mut state, index + 8);
        assert_eq!(
            percolator_prog::policy_v16::ewma_update(
                old,
                new,
                halflife,
                last_slot,
                now_slot,
                fee_paid,
                mark_min_fee,
            ),
            inv_085_ewma_update_oracle(
                old,
                new,
                halflife,
                last_slot,
                now_slot,
                fee_paid,
                mark_min_fee,
            ),
            "EWMA diverged at corpus word {index}"
        );

        let base_fee = inv_085_boundary_u128(&mut state, index + 9);
        let externality_notional = inv_085_boundary_u128(&mut state, index + 10);
        let fee_a = inv_085_boundary_u128(&mut state, index + 11);
        let fee_b = inv_085_boundary_u128(&mut state, index + 12);
        assert_eq!(
            percolator_prog::policy_v16::collected_fee_supported_mark(
                old,
                new,
                base_fee,
                externality_notional,
                fee_a,
                fee_b,
            ),
            inv_085_collected_fee_supported_mark_oracle(
                old,
                new,
                base_fee,
                externality_notional,
                fee_a,
                fee_b,
            ),
            "fee-supported mark diverged at corpus word {index}"
        );

        let amount = inv_085_boundary_u128(&mut state, index + 13);
        let share_bps = inv_085_boundary_u64(&mut state, index + 14) as u16;
        assert_eq!(
            percolator_prog::policy_v16::fee_share_floor(amount, share_bps),
            inv_085_fee_share_floor_oracle(amount, share_bps),
            "fee share diverged at corpus word {index}"
        );

        let asset_index = (inv_085_boundary_u64(&mut state, index + 15) % 5_783) as usize;
        assert_eq!(
            percolator_prog::policy_v16::permissionless_market_init_fee_for_asset(
                amount,
                asset_index,
            ),
            inv_085_market_init_fee_oracle(amount, asset_index),
            "market-init fee diverged at corpus word {index}"
        );

        let size_q = inv_085_boundary_u128(&mut state, index + 16);
        let price = inv_085_boundary_u64(&mut state, index + 17);
        let fee_bps = inv_085_boundary_u64(&mut state, index + 18);
        assert_eq!(
            percolator_prog::policy_v16::risk_notional_ceil(size_q, price),
            inv_085_risk_notional_ceil_oracle(size_q, price),
            "risk notional diverged at corpus word {index}"
        );
        assert_eq!(
            percolator_prog::policy_v16::trade_fee_notional_ceil(size_q, price),
            if size_q == 0 || price == 0 {
                Some(0)
            } else {
                inv_085_risk_notional_ceil_oracle(size_q, price)
            },
            "trade-fee notional diverged at corpus word {index}"
        );
        assert_eq!(
            percolator_prog::policy_v16::two_sided_trade_fee_paid(size_q, fee_bps),
            inv_085_two_sided_trade_fee_paid_uncapped_oracle(size_q, fee_bps),
            "two-sided fee diverged at corpus word {index}"
        );
        assert_eq!(
            percolator_prog::policy_v16::batch_leg_fee(size_q, price, fee_bps),
            inv_085_batch_leg_fee_oracle(size_q, price, fee_bps),
            "batch-leg fee diverged at corpus word {index}"
        );

        let denominator = inv_085_boundary_u128(&mut state, index + 19);
        assert_eq!(
            percolator_prog::policy_v16::ceil_div_u128(amount, denominator),
            inv_085_ceil_div_u128_oracle(amount, denominator),
            "ceil division diverged at corpus word {index}"
        );
    }
}

#[test]
fn v16_program_canonical_arithmetic_matches_bigint_on_full_width_boundaries() {
    const VALUES: &[u128] = &[
        0,
        1,
        2,
        9_999,
        10_000,
        u64::MAX as u128,
        u64::MAX as u128 + 1,
        u128::MAX / 2,
        u128::MAX - 1,
        u128::MAX,
    ];
    const U64_FACTORS: &[u64] = &[0, 1, 2, 9_999, 10_000, u32::MAX as u64, u64::MAX];
    const SHARE_BPS: &[u16] = &[0, 1, 9_999, 10_000, 10_001, u16::MAX];

    for &value in VALUES {
        for &share_bps in SHARE_BPS {
            assert_eq!(
                percolator_prog::policy_v16::fee_share_floor(value, share_bps),
                inv_085_big_mul_div_floor(value, u64::from(share_bps), 10_000),
                "fee-share bigint divergence for value={value}, share={share_bps}"
            );
        }
        for &factor in U64_FACTORS {
            assert_eq!(
                percolator_prog::policy_v16::risk_notional_ceil(value, factor),
                inv_085_big_risk_notional(value, factor),
                "risk-notional bigint divergence for value={value}, price={factor}"
            );
            assert_eq!(
                percolator_prog::policy_v16::two_sided_trade_fee_paid(value, factor),
                inv_085_big_two_sided_fee(value, factor),
                "two-sided-fee bigint divergence for value={value}, fee={factor}"
            );
            assert_eq!(
                percolator_prog::policy_v16::batch_leg_fee(value, factor, factor),
                inv_085_big_batch_leg_fee(value, factor, factor),
                "batch-fee bigint divergence for value={value}, factor={factor}"
            );
        }
        for &denominator in VALUES {
            assert_eq!(
                percolator_prog::policy_v16::ceil_div_u128(value, denominator),
                inv_085_big_ceil_div(value, denominator),
                "ceil-div bigint divergence for value={value}, denominator={denominator}"
            );
        }
    }
}

#[test]
fn v16_program_public_arithmetic_envelope_is_strictly_inside_u128() {
    let max_risk_notional =
        inv_085_big_risk_notional(percolator::MAX_TRADE_SIZE_Q, percolator::MAX_ORACLE_PRICE)
            .expect("public max trade notional fits u128");
    assert_eq!(
        percolator_prog::policy_v16::risk_notional_ceil(
            percolator::MAX_TRADE_SIZE_Q,
            percolator::MAX_ORACLE_PRICE,
        ),
        Some(max_risk_notional)
    );
    let max_externality_notional = max_risk_notional
        .checked_mul(2)
        .expect("two-sided public externality fits");
    let max_mark_fee = inv_085_big_mul_div_ceil(
        max_externality_notional,
        percolator::MAX_TRADING_FEE_BPS,
        percolator::MAX_MARGIN_BPS,
    )
    .expect("public mark fee fits u128");
    assert!(
        max_risk_notional < u128::MAX / 1_000_000_000
            && max_externality_notional < u128::MAX / 1_000_000_000
            && max_mark_fee < u128::MAX / 1_000_000_000,
        "public trade arithmetic retains at least nine decimal orders of u128 headroom"
    );
    assert_eq!(
        percolator_prog::policy_v16::fee_share_floor(percolator::MAX_VAULT_TVL, 10_000),
        Some(percolator::MAX_VAULT_TVL)
    );
}

#[test]
fn v16_program_market_init_fee_matches_repeated_doubling_at_supported_and_overflow_edges() {
    let cases = [
        (0, 0usize),
        (0, usize::MAX),
        (1, 0),
        (1, 31),
        (1, 32),
        (3, 63),
        (u128::MAX, 31),
        (u128::MAX, 32),
        (1, 4_095),
        (1, 4_096),
        (1, 5_782),
        (1, usize::MAX),
    ];
    for (base_fee, asset_index) in cases {
        assert_eq!(
            percolator_prog::policy_v16::permissionless_market_init_fee_for_asset(
                base_fee,
                asset_index,
            ),
            inv_085_market_init_fee_oracle(base_fee, asset_index),
            "market-init fee diverged for base {base_fee}, asset {asset_index}"
        );
    }
}

#[test]
fn v16_program_two_sided_fee_rate_search_matches_exhaustive_generated_inputs() {
    const CASES: usize = 1_024;
    let mut state = 0x8500_fee5_ea2c_0001;

    for index in 0..CASES {
        let notional = u128::from(inv_085_splitmix64(&mut state) % 1_000_001);
        let required_paid = u128::from(inv_085_splitmix64(&mut state) % 2_001);
        let bound_a = inv_085_splitmix64(&mut state) % 10_001;
        let bound_b = inv_085_splitmix64(&mut state) % 10_001;
        let (min_fee_bps, max_fee_bps) = if index % 8 == 0 {
            (bound_a.max(bound_b), bound_a.min(bound_b))
        } else {
            (bound_a.min(bound_b), bound_a.max(bound_b))
        };
        assert_eq!(
            percolator_prog::policy_v16::fee_bps_for_two_sided_fee_paid(
                notional,
                required_paid,
                min_fee_bps,
                max_fee_bps,
            ),
            inv_085_fee_bps_exhaustive_oracle(notional, required_paid, min_fee_bps, max_fee_bps,),
            "two-sided fee-rate search diverged at generated word {index}"
        );
    }
}

#[test]
fn v16_program_public_sbf_activation_first_fee_tier_matches_host_policy() {
    const BASE_FEE: u128 = 3;
    const ASSET_INDEX: u16 = 32;

    let mut env =
        V16CuEnv::new_with_init_params_and_market_capacity(V16CuMarketParams::default(), 33);
    env.update_market_init_fee_policy_with_cu(BASE_FEE);
    for asset_index in 1..=ASSET_INDEX {
        let slot = u64::from(asset_index);
        env.svm.warp_to_slot(slot);
        env.activate_asset(asset_index, slot, 100);
    }
    env.svm.warp_to_slot(33);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_RETIRE,
        ASSET_INDEX,
        33,
        0,
    );

    let expected_fee = percolator_prog::policy_v16::permissionless_market_init_fee_for_asset(
        BASE_FEE,
        usize::from(ASSET_INDEX),
    )
    .expect("first activation tier is representable");
    assert_eq!(
        expected_fee,
        BASE_FEE * 2,
        "index 32 is the first doubled tier"
    );

    let creator = Keypair::new();
    let creator_key = creator.pubkey();
    let before = env.market_state().1;
    env.svm.warp_to_slot(34);
    let (source, _) = env.activate_permissionless_asset_with_fee(
        &creator,
        ASSET_INDEX,
        34,
        100,
        creator_key,
        creator_key,
        creator_key,
        creator_key,
        expected_fee,
    );
    let after = env.market_state().1;

    assert_eq!(env.token_amount(source), 0);
    assert_eq!(after.vault - before.vault, expected_fee);
    assert_eq!(after.insurance - before.insurance, expected_fee);
    assert_eq!(
        after.assets[usize::from(ASSET_INDEX)].lifecycle,
        AssetLifecycleV16::Active
    );
}

#[test]
fn v16_program_public_sbf_hybrid_quote_matches_host_policy_with_live_oi() {
    const MARK: u64 = 1_000_000;
    const CAP_BPS: u64 = 50;
    const MAX_FEE_BPS: u64 = 37;
    const TRADE_SLOT: u64 = 5;
    const PASSIVE_SIZE_Q: i128 = (10_000u128 * POS_SCALE) as i128;
    const TRADE_SIZE_Q: i128 = (1_000u128 * POS_SCALE) as i128;
    const RAW_PRICE: u64 = 1_900_000;
    const MARK_MIN_FEE: u64 = 100_000_000;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: MARK,
        h_max: 20,
        max_trading_fee_bps: MAX_FEE_BPS,
        max_price_move_bps_per_slot: CAP_BPS,
        max_accrual_dt_slots: 20,
        min_funding_lifetime_slots: 20,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(1);
    env.configure_ewma_mark_with_cu(1, MARK, 1, MARK_MIN_FEE);

    let passive_long_owner = Keypair::new();
    let passive_short_owner = Keypair::new();
    let passive_long = env.create_portfolio(&passive_long_owner);
    let passive_short = env.create_portfolio(&passive_short_owner);
    env.deposit(&passive_long_owner, passive_long, 20_000_000_000);
    env.deposit(&passive_short_owner, passive_short, 20_000_000_000);
    env.trade_asset_with_cu(
        0,
        &passive_long_owner,
        passive_long,
        &passive_short_owner,
        passive_short,
        PASSIVE_SIZE_Q,
        MARK,
        0,
    );

    let trader_a = Keypair::new();
    let trader_b = Keypair::new();
    let account_a = env.create_portfolio(&trader_a);
    let account_b = env.create_portfolio(&trader_b);
    env.deposit(&trader_a, account_a, 4_000_000_000);
    env.deposit(&trader_b, account_b, 4_000_000_000);
    env.svm.warp_to_slot(TRADE_SLOT);

    let (cfg_before, group_before) = env.market_state();
    let asset_before = group_before.assets[0];
    let accepted_price = oracle_v16::clamp_toward_engine_dt(
        cfg_before.mark_ewma_e6,
        RAW_PRICE,
        CAP_BPS,
        TRADE_SLOT - cfg_before.mark_ewma_last_slot,
    );
    let trade_notional = percolator_prog::policy_v16::trade_fee_notional_ceil(
        TRADE_SIZE_Q.unsigned_abs(),
        accepted_price,
    )
    .expect("trade notional");
    let max_side_oi_q = asset_before.oi_eff_long_q.max(asset_before.oi_eff_short_q);
    let max_side_notional = percolator_prog::policy_v16::risk_notional_ceil(
        max_side_oi_q,
        asset_before.effective_price.max(cfg_before.mark_ewma_e6),
    )
    .expect("live OI notional");
    assert!(
        max_side_notional > trade_notional,
        "the public fixture must exercise the live-OI externality branch"
    );
    let externality_notional = max_side_notional
        .max(trade_notional)
        .checked_mul(2)
        .expect("two-sided externality notional");
    let candidate_mark = percolator_prog::policy_v16::ewma_update(
        cfg_before.mark_ewma_e6,
        accepted_price,
        cfg_before.mark_ewma_halflife_slots,
        cfg_before.mark_ewma_last_slot,
        TRADE_SLOT,
        MARK_MIN_FEE,
        MARK_MIN_FEE,
    );
    let base_fee_paid = percolator_prog::policy_v16::two_sided_trade_fee_paid(
        trade_notional,
        cfg_before.trade_fee_base_bps,
    )
    .expect("base fee");
    let max_fee_paid =
        percolator_prog::policy_v16::two_sided_trade_fee_paid(trade_notional, MAX_FEE_BPS)
            .expect("maximum fee");
    let candidate_move_bps =
        percolator_prog::policy_v16::price_move_bps_ceil(cfg_before.mark_ewma_e6, candidate_mark)
            .expect("candidate movement");
    let fee_supported_move_bps = u64::try_from(
        max_fee_paid
            .saturating_sub(base_fee_paid)
            .checked_mul(10_000)
            .expect("fee support numerator")
            / externality_notional,
    )
    .unwrap_or(u64::MAX);
    let quoted_move_bps = candidate_move_bps
        .min(MAX_FEE_BPS)
        .min(fee_supported_move_bps);
    let expected_mark = oracle_v16::clamp_toward_engine_dt(
        cfg_before.mark_ewma_e6,
        candidate_mark,
        quoted_move_bps,
        1,
    );
    let actual_move_bps =
        percolator_prog::policy_v16::price_move_bps_ceil(cfg_before.mark_ewma_e6, expected_mark)
            .expect("actual movement");
    let mark_fee_paid = percolator_prog::policy_v16::ceil_div_u128(
        externality_notional
            .checked_mul(u128::from(actual_move_bps))
            .expect("mark fee numerator"),
        10_000,
    )
    .expect("mark fee");
    let required_fee_paid = base_fee_paid
        .checked_add(mark_fee_paid)
        .expect("required fee");
    let expected_fee_bps = percolator_prog::policy_v16::fee_bps_for_two_sided_fee_paid(
        trade_notional,
        required_fee_paid,
        cfg_before.trade_fee_base_bps,
        MAX_FEE_BPS,
    )
    .expect("fee-rate search");
    let expected_fee_paid =
        percolator_prog::policy_v16::two_sided_trade_fee_paid(trade_notional, expected_fee_bps)
            .expect("selected two-sided fee");
    assert!(expected_mark > MARK && expected_fee_bps > 0 && expected_fee_bps < MAX_FEE_BPS);

    let insurance_before = group_before.insurance;
    env.trade_asset_with_cu(
        0,
        &trader_a,
        account_a,
        &trader_b,
        account_b,
        TRADE_SIZE_Q,
        RAW_PRICE,
        0,
    );
    let (cfg_after, group_after) = env.market_state();
    assert_eq!(cfg_after.mark_ewma_e6, expected_mark);
    assert_eq!(group_after.insurance - insurance_before, expected_fee_paid);
}

#[test]
fn v16_program_public_sbf_arithmetic_evidence_roster_is_complete() {
    struct PublicEvidence {
        adapter: &'static str,
        witness: &'static str,
        source: &'static str,
    }

    const THIS_SOURCE: &str =
        include_str!("inv_085_proven_arithmetic_equals_deployed_arithmetic.rs");
    const ROWS: &[PublicEvidence] = &[
        PublicEvidence {
            adapter: "fee_share_floor",
            witness: "v16_attack_fee_redirect_split_lands_correctly",
            source: include_str!("inv_036_fee_destination_and_policy_version_integrity.rs"),
        },
        PublicEvidence {
            adapter: "fee_share_floor",
            witness: "v16_program_liquidation_cranker_reward_bounded_by_fee",
            source: include_str!("inv_061_deterministic_bounded_liquidation.rs"),
        },
        PublicEvidence {
            adapter: "fee_share_floor",
            witness: "v16_bpf_sync_maintenance_fee_with_cranker_share_is_bounded",
            source: include_str!("inv_077_bounded_work_and_maximum_shape_compute.rs"),
        },
        PublicEvidence {
            adapter: "permissionless_market_init_fee_for_asset",
            witness: "v16_program_public_sbf_activation_first_fee_tier_matches_host_policy",
            source: THIS_SOURCE,
        },
        PublicEvidence {
            adapter: "batch_leg_fee",
            witness: "v16_attack_batch_subatom_fee_reconstruction_uses_ceil_notional",
            source: include_str!("inv_038_rounding_and_ratio_conservation.rs"),
        },
        PublicEvidence {
            adapter: "trade_fee_notional_ceil",
            witness: "v16_program_public_sbf_hybrid_quote_matches_host_policy_with_live_oi",
            source: THIS_SOURCE,
        },
        PublicEvidence {
            adapter: "risk_notional_ceil",
            witness: "v16_program_public_sbf_hybrid_quote_matches_host_policy_with_live_oi",
            source: THIS_SOURCE,
        },
        PublicEvidence {
            adapter: "two_sided_trade_fee_paid",
            witness: "v16_program_public_sbf_hybrid_quote_matches_host_policy_with_live_oi",
            source: THIS_SOURCE,
        },
        PublicEvidence {
            adapter: "ceil_div_u128",
            witness: "v16_program_public_sbf_hybrid_quote_matches_host_policy_with_live_oi",
            source: THIS_SOURCE,
        },
        PublicEvidence {
            adapter: "fee_bps_for_two_sided_fee_paid",
            witness: "v16_program_public_sbf_hybrid_quote_matches_host_policy_with_live_oi",
            source: THIS_SOURCE,
        },
    ];

    for row in ROWS {
        let marker = format!("fn {}", row.witness);
        let witness_tail = row
            .source
            .split_once(&marker)
            .unwrap_or_else(|| panic!("missing public SBF arithmetic witness {}", row.witness))
            .1;
        let witness_body = witness_tail
            .split("\n#[test]")
            .next()
            .unwrap_or(witness_tail);
        assert!(
            witness_body.contains(&format!("policy_v16::{}", row.adapter)),
            "{} must compare deployed SBF output with host adapter {}",
            row.witness,
            row.adapter
        );
    }
}

#[test]
fn v16_program_dynamic_externality_fee_matches_exhaustive_search_on_generated_inputs() {
    const CASES: usize = 512;
    let mut state = 0x8500_d1ff_e2e0_0001;

    for index in 0..CASES {
        let base_fee_bps = inv_085_splitmix64(&mut state) % 10_001;
        let old_mark_e6 = inv_085_splitmix64(&mut state) % 1_000_001;
        let clamped_exec_e6 = inv_085_splitmix64(&mut state) % 1_000_001;
        let halflife_slots = inv_085_splitmix64(&mut state) % 1_001;
        let last_mark_slot = inv_085_splitmix64(&mut state) % 1_001;
        let now_slot = inv_085_splitmix64(&mut state) % 1_001;
        let trade_notional = u128::from(inv_085_splitmix64(&mut state) % 1_000_001);
        let mark_externality_notional = u128::from(inv_085_splitmix64(&mut state) % 10_000_001);
        let mark_min_fee = inv_085_splitmix64(&mut state) % 10_001;
        let min_externality_bps = inv_085_splitmix64(&mut state) % 10_001;

        let deployed = percolator_prog::policy_v16::dynamic_fee_bps_with_externality_floor(
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
        );
        let reference = inv_085_dynamic_fee_bps_bruteforce_oracle(
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
        );
        assert_eq!(
            deployed, reference,
            "dynamic fee search diverged at generated word {index}"
        );
    }
}

#[test]
fn v16_program_wide_arithmetic_surface_is_source_complete_and_canonically_owned() {
    struct ArithmeticOwner {
        function: &'static str,
        class: &'static str,
        evidence: &'static str,
    }

    const ROWS: &[ArithmeticOwner] = &[
        ArithmeticOwner { function: "accrue_asset_to_not_atomic", class: "ENGINE_HOST_FACADE", evidence: "engine-owned host serialization model" },
        ArithmeticOwner { function: "market_view_mut", class: "STRUCTURAL", evidence: "INV-015" },
        ArithmeticOwner { function: "market_from_wire_boxed", class: "STRUCTURAL", evidence: "INV-015" },
        ArithmeticOwner { function: "write_market_wire", class: "STRUCTURAL", evidence: "INV-025" },
        ArithmeticOwner { function: "scale_decimal_exponent_to_e6", class: "ORACLE", evidence: "v16_program_composite_epoch_coherence_crosses_all_providers_and_transforms" },
        ArithmeticOwner { function: "compose_price_e6", class: "ORACLE", evidence: "v16_program_composite_epoch_coherence_crosses_all_providers_and_transforms" },
        ArithmeticOwner { function: "clamp_toward_engine_dt", class: "ORACLE", evidence: "v16_program_policy_arithmetic_matches_independent_full_width_corpus" },
        ArithmeticOwner { function: "mul_div_u128_by_u64", class: "POLICY", evidence: "v16_program_canonical_arithmetic_matches_bigint_on_full_width_boundaries" },
        ArithmeticOwner { function: "permissionless_market_init_fee_for_asset", class: "POLICY", evidence: "v16_program_market_init_fee_matches_repeated_doubling_at_supported_and_overflow_edges" },
        ArithmeticOwner { function: "ceil_div_u128", class: "POLICY", evidence: "v16_program_canonical_arithmetic_matches_bigint_on_full_width_boundaries" },
        ArithmeticOwner { function: "price_move_bps_ceil", class: "POLICY", evidence: "v16_program_policy_arithmetic_matches_independent_full_width_corpus" },
        ArithmeticOwner { function: "collected_fee_supported_mark", class: "POLICY", evidence: "v16_program_policy_arithmetic_matches_independent_full_width_corpus" },
        ArithmeticOwner { function: "premium_funding_rate_e9", class: "POLICY", evidence: "v16_program_policy_arithmetic_matches_independent_full_width_corpus" },
        ArithmeticOwner { function: "two_sided_trade_fee_paid", class: "POLICY", evidence: "v16_program_policy_arithmetic_matches_independent_full_width_corpus" },
        ArithmeticOwner { function: "ewma_effective_alpha_bps", class: "POLICY", evidence: "v16_program_policy_arithmetic_matches_independent_full_width_corpus" },
        ArithmeticOwner { function: "ewma_update", class: "POLICY", evidence: "v16_program_policy_arithmetic_matches_independent_full_width_corpus" },
        ArithmeticOwner { function: "dynamic_fee_bps_with_externality_floor", class: "POLICY", evidence: "v16_program_dynamic_externality_fee_matches_exhaustive_search_on_generated_inputs" },
        ArithmeticOwner { function: "domain_authorities_from_view", class: "STRUCTURAL", evidence: "INV-034" },
        ArithmeticOwner { function: "require_domain_accepts_live_topup_view", class: "STRUCTURAL", evidence: "INV-034" },
        ArithmeticOwner { function: "handle_batch_execute_zero_copy", class: "STRUCTURAL", evidence: "INV-077" },
        ArithmeticOwner { function: "handle_batch_trade_cpi", class: "STRUCTURAL", evidence: "INV-077" },
        ArithmeticOwner { function: "handle_top_up_insurance", class: "STRUCTURAL", evidence: "INV-034" },
        ArithmeticOwner { function: "backing_domain_parts_view", class: "STRUCTURAL", evidence: "INV-034" },
        ArithmeticOwner { function: "verify_domain_withdrawal_preflight", class: "STRUCTURAL", evidence: "INV-034" },
        ArithmeticOwner { function: "handle_top_up_backing_bucket", class: "STRUCTURAL", evidence: "INV-034" },
        ArithmeticOwner { function: "handle_withdraw_insurance_asset", class: "STRUCTURAL", evidence: "INV-064" },
        ArithmeticOwner { function: "hybrid_trade_fee_quote_view", class: "COMPOSITE", evidence: "v16_attack_repeated_ewma_moves_require_catchup_and_remain_fee_covered" },
    ];

    const RISK_MARKERS: &[&str] = &[
        ".checked_mul(",
        ".checked_div(",
        ".saturating_mul(",
        ".wrapping_mul(",
        ".abs_diff(",
        "10u128.pow(",
        "/ 10_000",
        "/ percolator::",
        "% den",
        "% denominator",
    ];
    const CANONICAL_ADAPTERS: &[&str] = &[
        "fee_share_floor",
        "permissionless_market_init_fee_for_asset",
        "risk_notional_ceil",
        "trade_fee_notional_ceil",
        "ceil_div_u128",
        "two_sided_trade_fee_paid",
        "fee_bps_for_two_sided_fee_paid",
        "batch_leg_fee",
    ];
    const REMOVED_PROCESSOR_COPIES: &[&str] = &[
        "two_sided_trade_fee_paid_view",
        "ceil_div_u128_view",
        "fee_bps_for_two_sided_fee_paid_view",
    ];

    let production = include_str!("../../../src/v16_program.rs");
    let production = production
        .split("    #[cfg(test)]\n    mod tests")
        .next()
        .expect("production prefix exists");
    assert_eq!(
        production
            .matches("scale_decimal_exponent_to_e6(")
            .count(),
        4,
        "one canonical decimal scaler must have exactly the Pyth, Switchboard, and Chainlink callers",
    );
    for provider_call in [
        "scale_decimal_exponent_to_e6(i128::from(msg.price), msg.exponent)",
        "scale_decimal_exponent_to_e6(observation.value, -MAX_EXPO_ABS)",
        "scale_decimal_exponent_to_e6(answer, -i32::from(decimals))",
    ] {
        assert_eq!(
            production.matches(provider_call).count(),
            1,
            "provider scaling must use the canonical exponent plan: {provider_call}",
        );
    }
    assert!(
        !production.contains("fn scale_decimal_to_e6("),
        "the legacy Chainlink-only decimal scaler must not return",
    );
    let mut current_function = "<module>";
    let mut actual = std::collections::BTreeSet::new();
    for line in production.lines() {
        let trimmed = line.trim_start();
        if let Some(fn_offset) = trimmed.find("fn ") {
            let prefix = &trimmed[..fn_offset];
            if prefix.is_empty() || prefix.starts_with("pub") {
                let rest = &trimmed[fn_offset + 3..];
                let end = rest
                    .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                    .unwrap_or(rest.len());
                current_function = &rest[..end];
            }
        }
        if RISK_MARKERS.iter().any(|marker| line.contains(marker)) {
            actual.insert(current_function.to_owned());
        }
    }

    let witness_sources = [
        include_str!("inv_085_proven_arithmetic_equals_deployed_arithmetic.rs"),
        include_str!("inv_020_authenticated_clock_slot_and_oracle_provenance.rs"),
        include_str!("inv_045_no_free_mark_movement.rs"),
    ];
    let mut expected = std::collections::BTreeSet::new();
    for row in ROWS {
        assert!(
            expected.insert(row.function.to_owned()),
            "duplicate arithmetic owner for {}",
            row.function
        );
        match row.class {
            "ORACLE" | "POLICY" | "COMPOSITE" => assert!(
                witness_sources
                    .iter()
                    .any(|source| source.contains(&format!("fn {}", row.evidence))),
                "{} lacks executable arithmetic evidence {}",
                row.function,
                row.evidence
            ),
            "STRUCTURAL" => assert!(row.evidence.starts_with("INV-")),
            "ENGINE_HOST_FACADE" => {
                assert_eq!(row.evidence, "engine-owned host serialization model")
            }
            other => panic!("unknown arithmetic ownership class {other}"),
        }
    }
    assert_eq!(
        actual, expected,
        "every multiply/divide-bearing production function needs one arithmetic owner"
    );

    let policy_source = production
        .split("pub mod policy_v16 {")
        .nth(1)
        .expect("policy module exists")
        .split("pub mod processor {")
        .next()
        .expect("policy module terminates before processor");
    for adapter in CANONICAL_ADAPTERS {
        assert!(
            policy_source.contains(&format!("fn {adapter}")),
            "canonical arithmetic adapter {adapter} escaped policy_v16"
        );
    }
    for removed in REMOVED_PROCESSOR_COPIES {
        assert!(
            !production.contains(&format!("fn {removed}")),
            "processor arithmetic copy {removed} must stay removed"
        );
    }
}
