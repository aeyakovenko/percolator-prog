//! INV-034 - Domain and instance isolation.
//!
//! Normative obligation: Value and liabilities cannot cross market instances or source domains without an explicit rule.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_pr290_cross_margin_debt_drains_unrelated_insurance`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_program_pr290_cross_margin_debt_drains_unrelated_insurance() {
    let reproduction = reproduce_cross_margin_insurance_drain([0x90; 32])
        .unwrap_or_else(|error| panic!("PR 290 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::CrossMarginInsuranceDrain
    );
    assert!(reproduction.unrelated_insurance_spent >= 100_000);
    assert!(reproduction.attacker_payout > 20_200);
    assert!(reproduction.attacker_profit > 90_000);
    assert!(reproduction.liquidation_calls > 0);
    assert!(reproduction.loser_close_calls < 512);
    assert!(
        reproduction.counterparty_close_calls > 0 && reproduction.counterparty_close_calls < 512
    );
    assert!(reproduction.winner_close_calls > 0 && reproduction.winner_close_calls < 512);
}
