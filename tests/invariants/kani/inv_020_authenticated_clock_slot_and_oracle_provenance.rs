//! INV-020 - Authenticated clock, slot, and oracle provenance.
//!
//! Normative obligation: every selected leg in one composite price shares one authenticated
//! observation epoch. The deployed wrapper predicate must accept one-leg prices and reject every
//! two- or three-leg timestamp disagreement.
//!
//! Evidence in this file (P): Kani exhausts all full-width `i64` timestamp triples through the
//! exact production predicate. Public SBF tests separately bind this predicate to account parsing,
//! rollback/ignore semantics, terminal payout, and owner exit.

use percolator_prog::oracle_v16::oracle_publish_times_are_coherent;

#[kani::proof]
fn kani_v16_composite_oracle_epochs_are_exactly_coherent() {
    let publish_times: [i64; 3] = kani::any();

    assert!(oracle_publish_times_are_coherent(&publish_times[..1]));
    assert_eq!(
        oracle_publish_times_are_coherent(&publish_times[..2]),
        publish_times[0] == publish_times[1]
    );
    assert_eq!(
        oracle_publish_times_are_coherent(&publish_times),
        publish_times[0] == publish_times[1] && publish_times[0] == publish_times[2]
    );
}

#[kani::proof]
fn kani_v16_empty_composite_epoch_is_invalid() {
    assert!(!oracle_publish_times_are_coherent(&[]));
}
