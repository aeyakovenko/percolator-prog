//! INV-038 - Rounding and ratio conservation.
//!
//! Normative obligation: Every rounded allocation plus explicit residue equals its exact source amount.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_pr329_pr381_composite_rounding_preserves_exact_price_and_health`, `v16_program_pr253_omitted_rounded_funding_rejects_and_recovers`, `v16_program_pr365_fractional_cap_floor_changes_terminal_payouts`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: PR329/381 and PR253 are fixed-pin public-route certifications. The
//! fractional-cap test remains a quarantined counterexample and does not certify that sub-route.

use super::*;

#[test]
fn v16_program_pr329_pr381_composite_rounding_preserves_exact_price_and_health() {
    for case in [
        CompositeRoundingCase::Pr329LargeMove,
        CompositeRoundingCase::Pr381MicroMove,
    ] {
        let reproduction = reproduce_composite_oracle_rounding([0x29; 32], case)
            .unwrap_or_else(|error| panic!("{case:?} fixed route failed: {error}"));
        assert_eq!(reproduction.blocker, KnownBlocker::CompositeOracleRounding);
        assert_eq!(reproduction.rounded_target, reproduction.exact_mark);
        assert_eq!(reproduction.rounded_mark, reproduction.exact_mark);
        assert_eq!(reproduction.certified_liq_deficit, 0);
        assert_eq!(reproduction.victim_capital_loss, 0);
        assert_eq!(reproduction.oi_reduction_q, 0);
        assert_eq!(reproduction.cranker_reward, 0);
        assert_eq!(reproduction.extracted_tokens, 0);
    }
}

#[test]
fn v16_program_pr253_omitted_rounded_funding_rejects_and_recovers() {
    let reproduction = reproduce_rounded_funding_omission([0x53; 32])
        .unwrap_or_else(|error| panic!("PR 253 fixed trace failed: {error}"));
    assert_eq!(reproduction.blocker, KnownBlocker::RoundedFundingOmission);
    assert!(reproduction.omitted_rejected_nonprogress);
    assert!(reproduction.omitted_exact_rollback);
    assert!(reproduction.control_f_long_num > 0);
    assert!(reproduction.control_f_short_num < 0);
    assert_eq!(
        reproduction.attack_f_long_num,
        reproduction.control_f_long_num
    );
    assert_eq!(
        reproduction.attack_f_short_num,
        reproduction.control_f_short_num
    );
    assert_eq!(reproduction.victim_payout_loss, 0);
    assert_eq!(reproduction.attacker_payout_gain, 0);
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
