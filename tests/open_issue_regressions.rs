//! Public-route regressions for issues reported against percolator-prog.
//!
//! Each test drives the deployed SBF through LiteSVM with public instructions only and pins the
//! behavior the corresponding issue requires. Retained transactions are built before the state
//! they authorized changes; every build carries a distinct compute-unit price, so a rejected
//! retry is rejected by the program, not by duplicate-signature filtering.

#[allow(dead_code)]
mod support;

use percolator_prog::processor;
use support::v16_svm::{MarketConfig, V16Svm};

fn env(seed: u8, config: MarketConfig) -> V16Svm {
    V16Svm::new([seed; 32], config)
}

/// Issue #402: a retained ClosePortfolio signed while the portfolio was empty must not land after
/// the same portfolio incarnation has accepted and returned new capital.
#[test]
fn issue402_retained_close_rejects_after_empty_state_aba() {
    let mut env = env(40, MarketConfig::default());
    let capital = env.primary_portfolio(0).capital.get();
    env.withdraw_primary(0, capital).expect("empty the portfolio");
    let retained_close = env.build_retained_close_primary_portfolio(0);

    // Same incarnation: deposit, then return to an empty state.
    env.deposit_primary(0, 100).expect("deposit");
    env.withdraw_primary(0, 100).expect("withdraw");
    let before = env.primary_portfolio_data(0);

    assert!(
        env.land_retained(retained_close).is_err(),
        "a close signed before the deposit must not land after it"
    );
    assert_eq!(env.primary_portfolio_data(0), before, "rejection is byte-atomic");

    env.close_primary_portfolio(0)
        .expect("a freshly signed close at the current sequence succeeds");
}

/// Issue #391: a retained asset-authority handoff must not restore a revoked backing key after
/// the role has moved on and an independent provider has deposited.
#[test]
fn issue391_retained_authority_handoff_cannot_restore_revoked_backing_key() {
    const ASSET: u16 = 0;
    const DOMAIN: u16 = 0;
    const OLD: usize = 1;
    const NEW: usize = 2;
    let mut env = env(39, MarketConfig::default());
    let first =
        env.build_retained_asset_authority_handoff_from_admin(ASSET, processor::ASSET_AUTH_BACKING_BUCKET, OLD);
    let withheld =
        env.build_retained_asset_authority_handoff_from_admin(ASSET, processor::ASSET_AUTH_BACKING_BUCKET, OLD);
    env.land_retained(first).expect("first handoff lands");
    env.update_asset_authority_from_admin(ASSET, processor::ASSET_AUTH_BACKING_BUCKET, NEW)
        .expect("rotate backing role to the new provider");
    let far_expiry = env.current_slot() + 1_000_000;
    env.top_up_backing_bucket_for_actor(NEW, DOMAIN, 100, far_expiry)
        .expect("new provider deposits backing");

    let market_before = env.market_data(false);
    assert!(
        env.land_retained(withheld).is_err(),
        "a withheld handoff variant must not restore the revoked key"
    );
    assert_eq!(env.market_data(false), market_before);
    assert!(
        env.withdraw_backing_bucket_for_actor(OLD, DOMAIN, 100).is_err(),
        "the revoked key cannot withdraw the new provider's backing"
    );
    env.withdraw_backing_bucket_for_actor(NEW, DOMAIN, 100)
        .expect("the current provider keeps custody of its deposit");
}
