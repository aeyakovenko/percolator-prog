//! INV-085 - Proven arithmetic equals deployed arithmetic.
//!
//! Normative obligation: proof/reference arithmetic must match the arithmetic
//! that ships in the wrapper.
//!
//! Evidence in this file (P): symbolic bounded Kani proofs compare deployed
//! policy/oracle helpers against independent widened reference formulas. This
//! upgrades INV-085 beyond fixed boundary examples while intentionally staying
//! below the known wide-arithmetic SAT wall.
//!
//! Guarantee boundary: this proves bounded symbolic agreement for selected
//! wrapper arithmetic helpers. It does not prove every deployed wide multiply,
//! divide, BPF lowering, or engine arithmetic path equivalent to a bigint model.

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
