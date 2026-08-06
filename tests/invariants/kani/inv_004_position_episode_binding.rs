//! INV-004 - Position episode binding.
//!
//! Normative obligation: wrapper-owned position episodes advance monotonically without changing
//! the matcher-enabled bit, LP fee cap, or deployed portfolio account layout.
//!
//! Evidence in this file (P): Kani checks the exact packed-control transition used by production
//! over every `u64` control word. It proves exact increment and matcher/cap preservation whenever
//! another episode is representable, and fail-closed behavior for invalid caps or exhaustion.
//!
//! Guarantee boundary: this is a local wrapper-layout proof. Public-route episode coverage and
//! exact rollback are exercised independently by the INV-004 stateful LiteSVM matrices.

use super::*;
use percolator_prog::state;

#[kani::proof]
fn kani_v16_position_epoch_control_is_monotonic_and_preserves_matcher_state() {
    let control: u64 = kani::any();
    let config = state::PortfolioMatcherConfigV16 {
        control,
        ..state::PortfolioMatcherConfigV16::default()
    };
    let epoch = config.position_epoch();
    let matcher_enabled = config.enabled();
    let trade_fee_cap_bps = config.trade_fee_cap_bps();
    let result = state::next_portfolio_position_control(control);

    if trade_fee_cap_bps > 10_000 || epoch == state::PortfolioMatcherConfigV16::position_epoch_max()
    {
        assert!(result.is_err());
    } else {
        let (next_epoch, next_control) = result.unwrap();
        let next_config = state::PortfolioMatcherConfigV16 {
            control: next_control,
            ..state::PortfolioMatcherConfigV16::default()
        };
        assert_eq!(next_epoch, epoch + 1);
        assert_eq!(next_config.position_epoch(), next_epoch);
        assert_eq!(next_config.enabled(), matcher_enabled);
        assert_eq!(next_config.trade_fee_cap_bps(), trade_fee_cap_bps);
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
    let trade_fee_cap_bps = config.trade_fee_cap_bps();

    config.set_enabled(enabled).unwrap();

    assert_eq!(config.enabled(), u64::from(enabled));
    assert_eq!(config.position_epoch(), epoch);
    assert_eq!(config.trade_fee_cap_bps(), trade_fee_cap_bps);
}

#[kani::proof]
fn kani_v16_matcher_fee_cap_update_is_bounded_and_preserves_other_control_fields() {
    let control: u64 = kani::any();
    let trade_fee_cap_bps: u16 = kani::any();
    let mut config = state::PortfolioMatcherConfigV16 {
        control,
        ..state::PortfolioMatcherConfigV16::default()
    };
    let epoch = config.position_epoch();
    let matcher_enabled = config.enabled();
    let result = config.set_trade_fee_cap_bps(trade_fee_cap_bps);

    if trade_fee_cap_bps > 10_000 {
        assert!(result.is_err());
        assert_eq!(config.control, control);
    } else {
        result.unwrap();
        assert_eq!(config.trade_fee_cap_bps(), trade_fee_cap_bps);
        assert_eq!(config.position_epoch(), epoch);
        assert_eq!(config.enabled(), matcher_enabled);
    }
}
