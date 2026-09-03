//! INV-012 - Capability and delegate scope.
//!
//! This proof exhausts the full-width enabled, fee-cap, expiry, and current-slot domains for the
//! exact predicate consumed by `SetMatcherConfig`. Public LiteSVM composition in the CU owner
//! proves both CPI routes consume the persisted predicate against the authenticated Clock sysvar.

use percolator_prog::state;

#[kani::proof]
fn kani_v16_matcher_capability_config_is_exact_at_full_width() {
    let enabled: u8 = kani::any();
    let trade_fee_cap_bps: u16 = kani::any();
    let expiry_slot: u64 = kani::any();
    let current_slot: u64 = kani::any();

    let expected = match enabled {
        0 => trade_fee_cap_bps == 0 && expiry_slot == 0,
        1 => trade_fee_cap_bps <= 10_000 && expiry_slot != 0 && current_slot < expiry_slot,
        _ => false,
    };
    assert_eq!(
        state::matcher_capability_config_is_valid(
            enabled,
            trade_fee_cap_bps,
            expiry_slot,
            current_slot,
        ),
        expected
    );

    kani::cover!(
        enabled == 0 && trade_fee_cap_bps == 0 && expiry_slot == 0,
        "a canonical disabled capability is valid"
    );
    kani::cover!(
        enabled == 1 && trade_fee_cap_bps <= 10_000 && current_slot < expiry_slot,
        "a bounded live capability is valid"
    );
    kani::cover!(
        enabled == 1 && expiry_slot == current_slot,
        "the exact expiry boundary rejects"
    );
    kani::cover!(enabled > 1, "non-boolean enabled values reject");
}
