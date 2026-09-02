//! INV-011 - Signed aggregate economic bounds.
//!
//! These proofs own the full-width branch logic for the batch-CPI aggregate quote-atom caps.
//! INV-085 owns equivalence of the shared deployed notional scaler to widened arithmetic, while
//! the public LiteSVM owner composes both caps through matcher CPI and exact rollback.

use percolator_prog::policy_v16;

#[kani::proof]
fn kani_v16_adverse_slippage_direction_is_exact() {
    let size_q: i128 = kani::any();
    let exec_price: u64 = kani::any();
    let authenticated_price: u64 = kani::any();
    let got = policy_v16::adverse_trade_price_delta(size_q, exec_price, authenticated_price);

    if size_q == i128::MIN {
        assert_eq!(got, None);
    } else if size_q > 0 {
        assert_eq!(got, Some(exec_price.saturating_sub(authenticated_price)));
    } else if size_q < 0 {
        assert_eq!(got, Some(authenticated_price.saturating_sub(exec_price)));
    } else {
        assert_eq!(got, Some(0));
    }

    kani::cover!(size_q == i128::MIN, "the rejected magnitude is reachable");
    kani::cover!(size_q > 0, "the long direction is reachable");
    kani::cover!(
        size_q < 0 && size_q != i128::MIN,
        "the short direction is reachable"
    );
    kani::cover!(size_q == 0, "the zero-size boundary is reachable");
}

#[kani::proof]
fn kani_v16_aggregate_slippage_accumulator_is_exact_and_fail_closed() {
    let total: u128 = kani::any();
    let amount: u128 = kani::any();
    let cap: u128 = kani::any();
    let got = policy_v16::accumulate_with_cap(total, amount, cap);
    let expected = total
        .checked_add(amount)
        .and_then(|sum| (sum <= cap).then_some(sum));

    assert_eq!(got, expected);
    kani::cover!(got.is_some(), "an in-budget aggregate is admitted");
    kani::cover!(total.checked_add(amount).is_none(), "overflow fails closed");
    kani::cover!(
        total.checked_add(amount).is_some() && got.is_none(),
        "a representable over-budget aggregate fails closed"
    );
}
