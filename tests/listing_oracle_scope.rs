//! Permissionless listing must not require the whole market to be flat.
//!
//! Drives the deployed SBF through LiteSVM with public instructions only.

#[allow(dead_code)]
mod support;

use support::v16_svm::{MarketConfig, V16Svm};

/// Permissionless listing must not require the whole market to be flat: configuring the oracle of
/// a freshly activated, empty asset is target-scoped, so it succeeds while traders on another
/// asset hold open PnL. Asset 0 (mirrored into the base config) keeps the market-wide rule.
#[test]
fn listing_new_asset_oracle_while_other_asset_has_open_pnl() {
    const MARK: u64 = 1_000_000;
    let mut env = V16Svm::new([71; 32], MarketConfig { initial_price: MARK, ..MarketConfig::default() });
    env.set_clock(1, 100);
    let feed0 = [0xa0u8; 32];
    let pyth0 = env.set_pyth_price(&feed0, MARK as i64, -6, 0, 100);
    env.configure_hybrid_oracle(0, 1, 100, 0, [feed0, [0; 32], [0; 32]], &[pyth0], 10, 0)
        .expect("configure asset 0 while the market is flat");
    env.trade_no_cpi(0, 1, 0, percolator::POS_SCALE as i128, MARK, 0)
        .expect("open a position on asset 0");
    env.set_clock(2, 101);
    env.update_pyth_price(pyth0, &feed0, (MARK + MARK / 100) as i64, -6, 0, 101);
    let hint = vec![percolator_prog::ix::CrankObservationHint { asset_index: 0, oracle_accounts: 1 }];
    env.crank_with_oracles(0, 2, hint, std::slice::from_ref(&pyth0))
        .expect("accrue the price move on asset 0");
    let (_, group) = env.primary_market_state();
    assert!(
        group.pnl_pos_tot != 0 || group.negative_pnl_account_count != 0,
        "precondition: a trader holds open PnL on asset 0"
    );

    env.update_market_init_fee_policy(1)
        .expect("enable permissionless activation");
    let asset = env.primary_market_state().1.config.max_market_slots as u16;
    env.activate_permissionless_asset(2, asset, 2, MARK, 1)
        .expect("permissionless activation appends a new asset");
    let feed1 = [0xa1u8; 32];
    let pyth1 = env.set_pyth_price(&feed1, MARK as i64, -6, 0, 101);
    env.configure_hybrid_oracle(asset, 2, 101, 0, [feed1, [0; 32], [0; 32]], &[pyth1], 10, 0)
        .expect("a new, empty asset's oracle is configurable while another asset has open PnL");

    let before = env.market_data(false);
    assert!(
        env.configure_hybrid_oracle(0, 2, 101, 0, [feed0, [0; 32], [0; 32]], &[pyth0], 10, 0)
            .is_err(),
        "asset 0 still requires a quiet market"
    );
    assert_eq!(env.market_data(false), before, "rejection is byte-atomic");
}
