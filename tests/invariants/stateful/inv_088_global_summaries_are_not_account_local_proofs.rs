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
//!
//! Guarantee boundary: the shared oracle now independently rebuilds every persisted stock/count
//! aggregate with an account, asset, domain, or bucket census. This focused route closes the
//! positive-PnL and pending-obligation/loss-weight families in a one-asset topology. Complete
//! public-writer route coverage and larger adversarial asset/account touch-order cross-products
//! remain.

use crate::support::fuzz_model::{
    run_cure_pending_obligation_dos_probe, run_materialized_portfolio_lifecycle_census,
};

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
