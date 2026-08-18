//! INV-013 - Destructive-consent scope.
//!
//! These proofs cover the pure binding predicate used by the deployed ClosePortfolio handler and
//! the shared sequence transition consumed by Deposit. Public-SBF composition is owned by the
//! corresponding `public_sbf/inv_013_destructive_consent_scope.rs` module.

use percolator_prog::state;

#[kani::proof]
fn kani_close_binding_accepts_exactly_one_state_tuple() {
    let current_portfolio_id: u64 = kani::any();
    let current_sequence: u64 = kani::any();
    let current_position_epoch: u64 = kani::any();
    let expected_portfolio_id: u64 = kani::any();
    let expected_sequence: u64 = kani::any();
    let expected_position_epoch: u64 = kani::any();

    let accepted = state::portfolio_close_binding_matches(
        current_portfolio_id,
        current_sequence,
        current_position_epoch,
        expected_portfolio_id,
        expected_sequence,
        expected_position_epoch,
    );
    assert_eq!(
        accepted,
        current_portfolio_id == expected_portfolio_id
            && current_sequence == expected_sequence
            && current_position_epoch == expected_position_epoch
    );
}

#[kani::proof]
fn kani_successful_deposit_sequence_invalidates_prior_close_binding() {
    let portfolio_id: u64 = kani::any();
    let current_sequence: u64 = kani::any();
    let position_epoch: u64 = kani::any();
    kani::assume(portfolio_id != 0);
    kani::assume(current_sequence < u64::MAX);

    let next_sequence = state::next_portfolio_matcher_sequence(current_sequence, current_sequence)
        .expect("nonmaximal matching sequence advances");
    assert!(state::portfolio_close_binding_matches(
        portfolio_id,
        current_sequence,
        position_epoch,
        portfolio_id,
        current_sequence,
        position_epoch,
    ));
    assert!(!state::portfolio_close_binding_matches(
        portfolio_id,
        next_sequence,
        position_epoch,
        portfolio_id,
        current_sequence,
        position_epoch,
    ));
    assert!(state::portfolio_close_binding_matches(
        portfolio_id,
        next_sequence,
        position_epoch,
        portfolio_id,
        next_sequence,
        position_epoch,
    ));
}
