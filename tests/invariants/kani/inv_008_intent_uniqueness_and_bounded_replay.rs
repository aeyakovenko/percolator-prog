//! INV-008 - Intent uniqueness and bounded replay.
//!
//! Normative obligation: a successful retained portfolio operation consumes the exact persisted
//! owner sequence, position episode, or per-asset top-up intent carried on its wire. The old
//! binding must then be false.
//!
//! Evidence in this file (P): these proofs exhaust all wrapper `u64` sequence values and all
//! packed position-control words used by the deployed handlers. They establish fail-closed
//! overflow, exact one-step consumption, and invalidation of both single-account and paired-trade
//! bindings. The top-up contract proves all full-width watermark/proposal pairs and is shared by
//! both insurance entrypoints. Public SBF tests prove handler composition, exact rollback, and
//! fresh-call liveness.

use percolator_prog::state;

#[kani::proof]
fn kani_v16_top_up_intent_accepts_only_a_strictly_newer_watermark() {
    let current: u64 = kani::any();
    let proposed: u64 = kani::any();

    let accepted = state::require_newer_control_sequence(current, proposed).is_ok();
    kani::cover!(accepted, "a strictly newer intent is accepted");
    kani::cover!(!accepted, "a stale intent is rejected");
    assert_eq!(accepted, proposed > current);
    if accepted {
        assert!(state::require_newer_control_sequence(proposed, proposed).is_err());
        assert!(state::require_newer_control_sequence(proposed, current).is_err());
    }
}

#[kani::proof]
fn kani_v16_owner_sequence_success_consumes_exactly_once() {
    let current: u64 = kani::any();
    let expected: u64 = kani::any();
    let result = state::next_portfolio_matcher_sequence(current, expected);

    kani::cover!(result.is_ok(), "the current sequence advances");
    kani::cover!(result.is_err(), "mismatch or exhaustion rejects");

    match result {
        Ok(next) => {
            assert_eq!(current, expected);
            assert_eq!(next, current + 1);
            assert!(state::next_portfolio_matcher_sequence(next, expected).is_err());
            if next == u64::MAX {
                assert!(state::next_portfolio_matcher_sequence(next, next).is_err());
            } else {
                assert_eq!(
                    state::next_portfolio_matcher_sequence(next, next).unwrap(),
                    next + 1
                );
            }
        }
        Err(_) => assert!(current != expected || current == u64::MAX),
    }
}

#[kani::proof]
fn kani_v16_paired_trade_binding_is_exact_in_both_episodes() {
    let current_a_id: u64 = kani::any();
    let current_a_epoch: u64 = kani::any();
    let current_b_id: u64 = kani::any();
    let current_b_epoch: u64 = kani::any();
    let expected_a_id: u64 = kani::any();
    let expected_a_epoch: u64 = kani::any();
    let expected_b_id: u64 = kani::any();
    let expected_b_epoch: u64 = kani::any();

    let accepted = state::portfolio_position_binding_matches(
        current_a_id,
        current_a_epoch,
        expected_a_id,
        expected_a_epoch,
    ) && state::portfolio_position_binding_matches(
        current_b_id,
        current_b_epoch,
        expected_b_id,
        expected_b_epoch,
    );
    assert_eq!(
        accepted,
        current_a_id == expected_a_id
            && current_a_epoch == expected_a_epoch
            && current_b_id == expected_b_id
            && current_b_epoch == expected_b_epoch
    );
}

#[kani::proof]
fn kani_v16_successful_trade_episode_advance_invalidates_old_pair() {
    let portfolio_a_id: u64 = kani::any();
    let portfolio_b_id: u64 = kani::any();
    let control_a: u64 = kani::any();
    let control_b: u64 = kani::any();

    let (next_a, _) = match state::next_portfolio_position_control(control_a) {
        Ok(next) => next,
        Err(_) => return,
    };
    let (next_b, _) = match state::next_portfolio_position_control(control_b) {
        Ok(next) => next,
        Err(_) => return,
    };
    kani::cover!(
        state::next_portfolio_position_control(control_a).is_ok()
            && state::next_portfolio_position_control(control_b).is_ok(),
        "both trade episodes can advance"
    );
    let cfg_a = state::PortfolioMatcherConfigV16 {
        control: control_a,
        ..state::PortfolioMatcherConfigV16::default()
    };
    let cfg_b = state::PortfolioMatcherConfigV16 {
        control: control_b,
        ..state::PortfolioMatcherConfigV16::default()
    };
    let old_a = cfg_a.position_epoch();
    let old_b = cfg_b.position_epoch();

    assert!(state::portfolio_position_binding_matches(
        portfolio_a_id,
        old_a,
        portfolio_a_id,
        old_a,
    ));
    assert!(state::portfolio_position_binding_matches(
        portfolio_b_id,
        old_b,
        portfolio_b_id,
        old_b,
    ));
    assert!(!state::portfolio_position_binding_matches(
        portfolio_a_id,
        next_a,
        portfolio_a_id,
        old_a,
    ));
    assert!(!state::portfolio_position_binding_matches(
        portfolio_b_id,
        next_b,
        portfolio_b_id,
        old_b,
    ));
}
