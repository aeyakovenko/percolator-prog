//! INV-020 - Authenticated clock, slot, and oracle provenance.
//!
//! Normative obligation: Time and oracle observations are authenticated, coherent, and cannot be caller-rewound.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_pr331_temporally_skewed_composite_liquidates_at_false_price`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_program_pr331_temporally_skewed_composite_liquidates_at_false_price() {
    let reproduction = reproduce_composite_oracle_time_skew([0x31; 32])
        .unwrap_or_else(|error| panic!("PR 331 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::CompositeOracleTimeSkew);
    assert_eq!(reproduction.coherent_price, 1_500_000);
    assert!(reproduction.skewed_target > reproduction.coherent_price);
    assert!(reproduction.skewed_mark > reproduction.coherent_price);
    assert!(reproduction.victim_capital_loss > 0);
    assert!(reproduction.oi_reduction_q > 0);
    assert!(reproduction.cranker_reward > 0);
    assert_eq!(
        u128::from(reproduction.extracted_tokens),
        reproduction.cranker_reward
    );
    assert!(reproduction.max_crank_cu < support::v16_svm::TX_CU_LIMIT);
}
