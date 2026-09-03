//! INV-020 - Authenticated clock, slot, and oracle provenance.
//!
//! Normative obligation: Time and oracle observations are authenticated, coherent, and cannot be caller-rewound.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_temporally_skewed_composite_rejects_atomically_and_exit_stays_live`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: this finite public matrix covers both one-leg-fresh directions and an
//! all-legs-fresh cross-epoch report, followed by a coherent control and complete owner exit.

use super::*;

#[test]
fn v16_program_temporally_skewed_composite_rejects_atomically_and_exit_stays_live() {
    let evidence = verify_composite_time_coherence([0x31; 32])
        .unwrap_or_else(|error| panic!("composite-time verification failed: {error}"));
    assert!(evidence.is_protected(), "{evidence:?}");
}
