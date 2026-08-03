//! INV-038 - Rounding and ratio conservation.
//!
//! Normative obligation: Every rounded allocation plus explicit residue equals its exact source amount.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_pr329_pr381_composite_rounding_false_liquidates`, `v16_program_pr253_omitted_rounded_funding_transfers_spl_value`, `v16_program_pr365_fractional_cap_floor_changes_terminal_payouts`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_program_pr329_pr381_composite_rounding_false_liquidates() {
    for case in [
        CompositeRoundingCase::Pr329LargeMove,
        CompositeRoundingCase::Pr381MicroMove,
    ] {
        let reproduction = reproduce_composite_oracle_rounding([0x29; 32], case)
            .unwrap_or_else(|error| panic!("{case:?} no longer reproduces: {error}"));
        assert_eq!(reproduction.blocker, KnownBlocker::CompositeOracleRounding);
        assert_ne!(reproduction.rounded_target, reproduction.exact_mark);
        assert_ne!(reproduction.rounded_mark, reproduction.exact_mark);
        assert!(reproduction.victim_capital_loss > 0);
        assert!(reproduction.oi_reduction_q > 0);
        assert_eq!(
            reproduction.cranker_reward,
            u128::from(reproduction.extracted_tokens)
        );
    }
}

#[test]
fn v16_program_pr253_omitted_rounded_funding_transfers_spl_value() {
    let reproduction = reproduce_rounded_funding_omission([0x53; 32])
        .unwrap_or_else(|error| panic!("PR 253 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::RoundedFundingOmission);
    assert!(reproduction.control_f_long_num > 0);
    assert!(reproduction.control_f_short_num < 0);
    assert_eq!(reproduction.attack_f_long_num, 0);
    assert_eq!(reproduction.attack_f_short_num, 0);
    assert_eq!(
        reproduction.victim_payout_loss,
        reproduction.attacker_payout_gain
    );
}

#[test]
fn v16_program_pr365_fractional_cap_floor_changes_terminal_payouts() {
    let reproduction = reproduce_fractional_cap_settlement([0x65; 32])
        .unwrap_or_else(|error| panic!("PR 365 no longer reproduces: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::FractionalCapSettlement);
    assert_eq!(reproduction.target_price, 1);
    assert!(reproduction.stalled_price > reproduction.target_price);
    assert!(reproduction.successful_cranks > 0);
    assert_eq!(
        reproduction.long_overpayment,
        reproduction.short_underpayment
    );
    assert!(reproduction.short_underpayment > 0);
    assert_eq!(
        u128::from(reproduction.long_payout) + u128::from(reproduction.short_payout),
        2_000_000
    );
}
