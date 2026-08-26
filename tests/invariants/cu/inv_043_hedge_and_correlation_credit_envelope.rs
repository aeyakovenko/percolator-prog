//! INV-043 - Hedge and correlation credit envelope.
//!
//! Current-surface obligation: numeric hedge/correlation credit is disabled in
//! the pinned v16 public profile. Opposite-direction exposure on another asset
//! must therefore add its full per-leg risk instead of reducing the portfolio's
//! margin requirement. The source guard keeps that profile closed: introducing
//! a wrapper hedge-credit control or consumer reopens the invariant and requires
//! the charter's proof, fuzz, and bounded-reachability envelope before use.

use super::*;

#[test]
fn v16_program_cross_asset_opposite_exposure_receives_zero_hedge_credit() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let counterparty_owner = Keypair::new();
    let counterparty = env.create_portfolio(&counterparty_owner);
    env.deposit(&owner, portfolio, 1_000_000);
    env.deposit(&counterparty_owner, counterparty, 1_000_000);

    env.trade_asset_with_cu(
        0,
        &owner,
        portfolio,
        &counterparty_owner,
        counterparty,
        POS_SCALE as i128,
        100,
        0,
    );
    let one_leg = health_cert(&env.portfolio_state(portfolio));
    assert!(one_leg.certified_initial_req != 0);
    assert!(one_leg.certified_maintenance_req != 0);
    assert!(one_leg.certified_worst_case_loss != 0);

    env.trade_asset_with_cu(
        1,
        &owner,
        portfolio,
        &counterparty_owner,
        counterparty,
        -(POS_SCALE as i128),
        100,
        0,
    );
    let state = env.portfolio_state(portfolio);
    let two_leg = health_cert(&state);
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&state)),
        2,
        "control must hold equal opposite-direction exposure on two assets",
    );
    assert_eq!(
        two_leg.certified_initial_req,
        one_leg.certified_initial_req * 2,
        "initial margin must remain the gross per-leg sum",
    );
    assert_eq!(
        two_leg.certified_maintenance_req,
        one_leg.certified_maintenance_req * 2,
        "maintenance margin must remain the gross per-leg sum",
    );
    assert_eq!(
        two_leg.certified_worst_case_loss,
        one_leg.certified_worst_case_loss * 2,
        "worst-case loss must remain the gross per-leg sum",
    );

    let group = env.market_state().1;
    assert!(group.vault >= group.c_tot + group.insurance);
    assert_eq!(group.vault as u64, env.token_amount(env.vault));
}

#[test]
fn v16_program_hedge_credit_controls_remain_absent_from_the_public_profile() {
    let source = include_str!("../../../src/v16_program.rs");
    let production_end = source
        .rfind("    #[cfg(test)]\n    mod tests")
        .expect("production/test boundary");
    let production = &source[..production_end];
    for forbidden in [
        "hedge_credit",
        "initial_hedge_credit",
        "correlation_credit",
        "cfg_max_offset_bps",
        "hedge_bucket",
    ] {
        assert!(
            !production.contains(forbidden),
            "optional hedge-credit mechanism {forbidden} entered the public wrapper",
        );
    }

    let lock = include_str!("../../../Cargo.lock");
    assert!(lock.contains(
        "git+https://github.com/aeyakovenko/percolator?rev=b10b3454#\
         b10b3454dd03dcf4c04a020dc1a90381ff179200"
    ));
}
