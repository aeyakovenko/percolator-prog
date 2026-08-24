//! INV-036 - Fee destination and policy-version integrity.
//!
//! The deployed engine returns collected trade fees in physical account order. These proofs
//! exhaustively establish the wrapper's pure normalization into economic long/short domains for
//! every nonzero signed trade direction and every full-width fee pair. Whole-route LiteSVM tests
//! in the matching CU invariant file establish that the proven mapping composes through public
//! single, batch, terminal-payout, and custody transitions.

use percolator_prog::policy_v16::account_fees_to_trade_sides;

#[kani::proof]
fn kani_v16_account_fees_map_to_the_signed_trade_sides() {
    let size_q = kani::any::<i128>();
    let fee_a = kani::any::<u128>();
    let fee_b = kani::any::<u128>();
    kani::assume(size_q != 0);

    let mapped = account_fees_to_trade_sides(size_q, fee_a, fee_b).unwrap();
    kani::cover!(size_q > 0, "long-side fee attribution is reachable");
    kani::cover!(size_q < 0, "short-side fee attribution is reachable");
    if size_q > 0 {
        assert_eq!(mapped, (fee_a, fee_b));
    } else {
        assert_eq!(mapped, (fee_b, fee_a));
    }
}

#[kani::proof]
fn kani_v16_single_and_batch_account_orientations_have_identical_side_fees() {
    let fee_long = kani::any::<u128>();
    let fee_short = kani::any::<u128>();

    let single = account_fees_to_trade_sides(1, fee_long, fee_short).unwrap();
    let batch_positive = account_fees_to_trade_sides(1, fee_long, fee_short).unwrap();
    let batch_negative = account_fees_to_trade_sides(-1, fee_short, fee_long).unwrap();

    assert_eq!(single, (fee_long, fee_short));
    assert_eq!(batch_positive, single);
    assert_eq!(batch_negative, single);
}

#[kani::proof]
fn kani_v16_zero_size_cannot_receive_a_fee_side_attribution() {
    let fee_a = kani::any::<u128>();
    let fee_b = kani::any::<u128>();
    assert!(account_fees_to_trade_sides(0, fee_a, fee_b).is_none());
}
