//! INV-031 - No double use of claim, backing, or insurance atoms.
//!
//! Normative obligation: A backing or claim atom cannot support two economic obligations.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions):
//! `v16_program_cross_domain_backing_is_consumed_once`. The test creates one unfunded and one
//! funded source claim through public trades, converts the funded tranche, and proves a retry
//! rejects with exact rollback instead of consuming the same backing again. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: this is a fixed-seed SBF witness for the generated stateful campaign in
//! the INV-031 file; the broader route matrices cover other claim/backing/insurance lifecycles.

use super::*;

#[test]
fn v16_program_cross_domain_backing_is_consumed_once() {
    let discovery = discover_cross_domain_backing_single_use([0x67; 32])
        .unwrap_or_else(|error| panic!("cross-domain single-use route failed: {error}"));
    assert_eq!(
        discovery.unfunded_claim_before_num,
        100 * percolator::BOUND_SCALE
    );
    assert_eq!(
        discovery.funded_claim_before_num,
        100 * percolator::BOUND_SCALE
    );
    assert_eq!(
        discovery.unfunded_claim_after_first_num,
        100 * percolator::BOUND_SCALE
    );
    assert_eq!(discovery.funded_claim_after_first_num, 0);
    assert_eq!(
        discovery.funded_backing_consumed_num,
        100 * percolator::BOUND_SCALE
    );
    assert_eq!(discovery.winner_capital_gain, 100);
    assert!(discovery.second_conversion_rejected);
    assert!(discovery.second_conversion_exact_rollback);
    assert_eq!(discovery.extracted_tokens, 1_100);
    assert_eq!(discovery.victim_loss_atoms, 0);
    assert_eq!(discovery.unauthorized_gain_atoms, 0);
    assert!(discovery.preserves_single_use(), "{discovery:?}");
}
