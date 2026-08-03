//! INV-035 - No global B pool; residuals remain local.
//!
//! Normative obligation: Bankruptcy residuals stay in the exact asset and opposing-side domain that created them.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_pr281_wrong_domain_b_settlement_strands_dust_position`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_program_pr281_wrong_domain_b_settlement_strands_dust_position() {
    let reproduction = reproduce_cross_domain_b_settlement([0x81; 32])
        .unwrap_or_else(|error| panic!("PR 281 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::CrossDomainBSettlement);
    assert!(reproduction.b_target_num > 0);
    assert!(reproduction.pnl_loss > 0);
    assert!(reproduction.unfunded_claim_after_num < reproduction.unfunded_claim_before_num);
    assert!(reproduction.funded_claim_after_num < reproduction.funded_claim_before_num);
    assert_eq!(
        (reproduction.unfunded_claim_before_num - reproduction.unfunded_claim_after_num)
            + (reproduction.funded_claim_before_num - reproduction.funded_claim_after_num),
        reproduction.pnl_loss * percolator::BOUND_SCALE
    );
    assert!(reproduction.wrong_domain_reduction_num > 0);
    assert!(reproduction.correct_domain_reduction_num > 0);
    assert!(reproduction.reduction_steps > 0);
    assert_eq!(reproduction.stranded_position_q, percolator::POS_SCALE);
    assert!(reproduction.failed_terminal_reductions >= 6);
    assert!(reproduction.full_withdraw_rejected);
}
