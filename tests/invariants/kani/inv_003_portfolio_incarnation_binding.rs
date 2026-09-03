//! INV-003 - Portfolio incarnation binding.
//!
//! These contracts execute the deployed portfolio-ID allocator through a Kani-only
//! visibility shim. They prove successful allocations are nonzero and strictly
//! monotonic, including migration from the legacy zero counter, and that an old
//! retained ID cannot equal the ID after two later successful incarnations.

use percolator_prog::state::kani_allocate_portfolio_id;

#[kani::proof]
fn kani_v16_successful_portfolio_id_allocation_is_nonzero_and_strict() {
    let next: u64 = kani::any();
    let allocation = kani_allocate_portfolio_id(next);

    kani::cover!(allocation.is_ok(), "a portfolio ID can be allocated");
    kani::cover!(allocation.is_err(), "an exhausted allocator rejects");

    if let Ok((portfolio_id, following)) = allocation {
        assert_ne!(portfolio_id, 0);
        assert!(following > portfolio_id);
        assert_eq!(portfolio_id, if next == 0 { 1 } else { next });
    }
}

#[kani::proof]
fn kani_v16_successful_portfolio_incarnations_never_reuse_an_old_id() {
    let initial_next: u64 = kani::any();
    let Ok((old_id, after_old)) = kani_allocate_portfolio_id(initial_next) else {
        return;
    };
    let Ok((intermediate_id, after_intermediate)) = kani_allocate_portfolio_id(after_old) else {
        return;
    };
    let Ok((current_id, _)) = kani_allocate_portfolio_id(after_intermediate) else {
        return;
    };

    assert!(old_id < intermediate_id);
    assert!(intermediate_id < current_id);
    assert_ne!(old_id, current_id);
    kani::cover!(initial_next == 0);
    kani::cover!(initial_next == 1);
    kani::cover!(initial_next == u64::MAX - 3);
}
