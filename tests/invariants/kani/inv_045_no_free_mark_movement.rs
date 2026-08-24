//! INV-045 - No free mark movement.
//!
//! Normative obligation: Symbolic fee and funding inputs preserve directional clamps and prevent
//! unsupported fee-funded mark movement.
//!
//! Evidence in this file (P): Kani executes the deployed wrapper arithmetic, decoder, or
//! matcher-validation code over symbolic inputs. These leaf/local proofs do not establish
//! wrapper-plus-engine whole-route conservation or liveness on their own.
//!
//! Guarantee boundary: these are local policy proofs. Whole-route trade availability, fee
//! collection, engine mark mutation, and route equivalence remain separate obligations.

use super::*;

#[kani::proof]
fn kani_v16_premium_funding_rate_is_clamped_and_signed() {
    let mark_raw: u16 = kani::any();
    let index_raw: u16 = kani::any();
    let cap_raw: u16 = kani::any();
    let mark = mark_raw as u64 + 1;
    let index = index_raw as u64 + 1;
    let cap = cap_raw as u64;

    let rate = policy_v16::premium_funding_rate_e9(mark, index, cap).unwrap();
    let abs_rate = if rate < 0 {
        (-rate) as u128
    } else {
        rate as u128
    };
    kani::cover!(cap == 0 || mark == index, "zero premium has zero rate");
    kani::cover!(cap > 0 && mark > index, "positive premium is reachable");
    kani::cover!(cap > 0 && mark < index, "negative premium is reachable");
    assert!(abs_rate <= cap as u128);

    if cap == 0 || mark == index {
        assert_eq!(rate, 0);
    } else if mark > index {
        assert!(rate > 0);
    } else {
        assert!(rate < 0);
    }
}

#[kani::proof]
fn kani_v16_fee_supported_mark_clamp_is_directional_and_zero_support_is_noop() {
    let old_mark = kani::any::<u64>();
    let quoted_mark = kani::any::<u64>();
    let supported_move_bps = kani::any::<u64>();
    kani::assume(old_mark > 0 && quoted_mark > 0);
    let mark =
        policy_v16::clamp_mark_to_supported_move_bps(old_mark, quoted_mark, supported_move_bps);

    kani::cover!(
        supported_move_bps == 0,
        "zero paid support cannot move mark"
    );
    kani::cover!(
        quoted_mark > old_mark && mark > old_mark,
        "upward paid movement"
    );
    kani::cover!(
        quoted_mark < old_mark && mark < old_mark,
        "downward paid movement"
    );

    assert!(mark >= old_mark.min(quoted_mark));
    assert!(mark <= old_mark.max(quoted_mark));
    if supported_move_bps == 0 {
        assert_eq!(mark, old_mark);
    }
}

#[kani::proof]
fn kani_v16_collected_base_fee_cannot_fund_mark_movement() {
    let old_mark = kani::any::<u64>();
    let quoted_mark = kani::any::<u64>();
    let fee_a = kani::any::<u32>() as u128;
    let fee_b = kani::any::<u32>() as u128;
    let base_fee_paid = fee_a + fee_b + kani::any::<u32>() as u128;
    let mark_externality_notional = kani::any::<u32>() as u128 + 1;
    kani::assume(old_mark > 0 && quoted_mark > 0);

    let mark = policy_v16::collected_fee_supported_mark(
        old_mark,
        quoted_mark,
        base_fee_paid,
        mark_externality_notional,
        fee_a,
        fee_b,
    )
    .unwrap();

    kani::cover!(
        quoted_mark != old_mark && fee_a > 0 && fee_b > 0,
        "both counterparties pay only base fee against a moving quote"
    );
    kani::cover!(
        base_fee_paid > fee_a + fee_b,
        "collected fee does not fully cover base fee"
    );
    assert_eq!(mark, old_mark);
}

#[kani::proof]
fn kani_v16_one_sided_externality_fee_cannot_fund_mark_movement() {
    let old_mark = kani::any::<u64>();
    let quoted_mark = kani::any::<u64>();
    let base_fee_per_side = kani::any::<u32>() as u128;
    let paying_side_externality = kani::any::<u32>() as u128;
    let mark_externality_notional = kani::any::<u32>() as u128 + 1;
    kani::assume(old_mark > 0 && quoted_mark > 0);

    let mark_a_underfunded = policy_v16::collected_fee_supported_mark(
        old_mark,
        quoted_mark,
        base_fee_per_side * 2,
        mark_externality_notional,
        base_fee_per_side,
        base_fee_per_side + paying_side_externality,
    )
    .unwrap();
    let mark_b_underfunded = policy_v16::collected_fee_supported_mark(
        old_mark,
        quoted_mark,
        base_fee_per_side * 2,
        mark_externality_notional,
        base_fee_per_side + paying_side_externality,
        base_fee_per_side,
    )
    .unwrap();

    kani::cover!(
        quoted_mark != old_mark && paying_side_externality > 0,
        "one counterparty pays an externality fee against a moving quote"
    );
    assert_eq!(mark_a_underfunded, old_mark);
    assert_eq!(mark_b_underfunded, old_mark);
}
