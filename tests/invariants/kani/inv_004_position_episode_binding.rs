//! INV-004 - Position episode binding.
//!
//! Normative obligation: wrapper-owned position episodes advance monotonically without changing
//! the legacy matcher-enabled bit or the deployed portfolio account layout.
//!
//! Evidence in this file (P): Kani checks the exact packed-control transition used by production
//! over every `u64` control word. It proves exact increment and matcher-bit preservation whenever
//! another episode is representable, and fail-closed behavior at the exhausted state.
//!
//! Guarantee boundary: this is a local wrapper-layout proof. Public-route episode coverage and
//! exact rollback are exercised independently by the INV-004 stateful LiteSVM matrices.

use super::*;
use percolator_prog::state;

#[kani::proof]
fn kani_v16_position_epoch_control_is_monotonic_and_preserves_matcher_state() {
    let control: u64 = kani::any();
    let epoch = control >> 1;
    let matcher_enabled = control & 1;
    let result = state::next_portfolio_position_control(control);

    if epoch == (u64::MAX >> 1) {
        assert!(result.is_err());
    } else {
        let (next_epoch, next_control) = result.unwrap();
        assert_eq!(next_epoch, epoch + 1);
        assert_eq!(next_control >> 1, next_epoch);
        assert_eq!(next_control & 1, matcher_enabled);
    }
}

#[kani::proof]
fn kani_v16_matcher_toggle_preserves_position_epoch() {
    let control: u64 = kani::any();
    let enabled: u8 = kani::any();
    kani::assume(enabled <= 1);
    let mut config = state::PortfolioMatcherConfigV16 {
        control,
        ..state::PortfolioMatcherConfigV16::default()
    };
    let epoch = config.position_epoch();

    config.set_enabled(enabled).unwrap();

    assert_eq!(config.enabled(), u64::from(enabled));
    assert_eq!(config.position_epoch(), epoch);
}
