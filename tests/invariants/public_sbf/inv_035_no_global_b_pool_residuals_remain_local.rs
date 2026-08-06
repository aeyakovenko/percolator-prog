//! INV-035 - No global B pool; residuals remain local.
//!
//! Normative obligation: Bankruptcy residuals stay in the exact asset and opposing-side domain that created them.
//!
//! Evidence in this file (I plus invariant-specific state assertions):
//! `v16_program_pr281_b_settlement_stays_domain_local_and_owner_can_exit` executes the original
//! public exploit topology and requires exact source-domain attribution plus bounded owner exit.
//!
//! Guarantee boundary: this is the deterministic fixed-pin regression. The randomized,
//! independently implemented oracle lives in the stateful INV-035 file.

use super::*;

#[test]
fn v16_program_pr281_b_settlement_stays_domain_local_and_owner_can_exit() {
    let reproduction = reproduce_cross_domain_b_settlement([0x81; 32])
        .unwrap_or_else(|error| panic!("PR 281 fixed-pin regression failed: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::CrossDomainBSettlement);
    assert!(reproduction.b_target_num > 0);
    assert!(reproduction.pnl_loss > 0);
    assert_eq!(
        reproduction.unfunded_claim_after_num,
        reproduction.unfunded_claim_before_num
    );
    assert!(reproduction.funded_claim_after_num < reproduction.funded_claim_before_num);
    assert_eq!(
        (reproduction.unfunded_claim_before_num - reproduction.unfunded_claim_after_num)
            + (reproduction.funded_claim_before_num - reproduction.funded_claim_after_num),
        reproduction.pnl_loss * percolator::BOUND_SCALE
    );
    assert_eq!(reproduction.wrong_domain_reduction_num, 0);
    assert_eq!(
        reproduction.correct_domain_reduction_num,
        reproduction.pnl_loss * percolator::BOUND_SCALE
    );
    assert!(reproduction.reduction_steps > 0);
    assert_eq!(reproduction.affected_position_after_q, 0);
    assert!(reproduction.principal_withdrawn > 0);
    assert!(reproduction.token_supply_conserved);
}
