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
//! stale-certificate, B-stale, and negative-PnL account counts. The test separately requires the
//! intermediate obligation and weight to be nonzero so the census cannot pass vacuously.
//!
//! Guarantee boundary: this closes the pending-obligation/loss-weight summary writer family in a
//! one-asset public topology. Materialized-account and resolved-payout blocker counts, larger
//! asset/account touch-order cross-products, and a complete independent model for every remaining
//! aggregate still require coverage.

use crate::support::fuzz_model::run_cure_pending_obligation_dos_probe;

#[test]
fn v16_program_pending_obligation_summaries_match_the_complete_portfolio_census() {
    let evidence = run_cure_pending_obligation_dos_probe()
        .expect("public cure and cleanup must satisfy the complete summary census");
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
