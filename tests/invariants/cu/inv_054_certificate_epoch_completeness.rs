//! INV-054 - Certificate epoch completeness.
//!
//! Normative obligation: Every health-relevant state change invalidates or conservatively downgrades certificates.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_attack_convert_released_pnl_requires_current_cert_and_public_refresh`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_attack_convert_released_pnl_requires_current_cert_and_public_refresh() {
    const RELEASED: u128 = 40;
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    env.top_up_backing_bucket(1, RELEASED, 10_000);

    let crank_long_owner = Keypair::new();
    let crank_short_owner = Keypair::new();
    let crank_long = env.create_portfolio(&crank_long_owner);
    let crank_short = env.create_portfolio(&crank_short_owner);
    env.deposit(&crank_long_owner, crank_long, 1_000_000);
    env.deposit(&crank_short_owner, crank_short, 1_000_000);
    env.trade_with_cu(
        &crank_long_owner,
        crank_long,
        &crank_short_owner,
        crank_short,
        POS_SCALE as i128,
        100,
        0,
    );

    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.add_source_positive_pnl(portfolio, 1, RELEASED);
    env.crank(
        portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: crank_observations(0),
        },
    );
    assert_eq!(
        env.portfolio_state(portfolio).pnl.get(),
        RELEASED as i128,
        "setup must stage real released PnL"
    );
    let (_, fresh_group) = env.market_state();
    assert_eq!(
        health_cert(&env.portfolio_state(portfolio)).cert_oracle_epoch,
        fresh_group.oracle_epoch,
        "setup must start with a current cert"
    );

    env.svm.warp_to_slot(1);
    env.push_auth_mark_with_cu(1, 101);
    env.crank(
        crank_long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
    );
    let (_, stale_group) = env.market_state();
    assert!(
        health_cert(&env.portfolio_state(portfolio)).cert_oracle_epoch < stale_group.oracle_epoch,
        "auth mark update must make the existing cert stale"
    );

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let rejected = env.send(
        env.convert_released_pnl_ix(portfolio, RELEASED),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&owner],
    );
    assert!(
        rejected.is_err(),
        "stale-cert ConvertReleasedPnl must propagate the engine error"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "stale-cert rejection leaves market accounting unchanged"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "stale-cert rejection leaves released PnL and cert bytes unchanged"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "stale-cert rejection moves no custody"
    );

    env.crank(
        portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
    );
    let (_, refreshed_group) = env.market_state();
    assert_eq!(
        health_cert(&env.portfolio_state(portfolio)).cert_oracle_epoch,
        refreshed_group.oracle_epoch,
        "public crank refreshes the stale cert"
    );

    let convert_cu = env.convert_released_pnl_with_cu(&owner, portfolio, RELEASED);
    assert_cu_within(
        "ConvertReleasedPnl after public cert refresh",
        convert_cu,
        CUSTODY_CU_LIMIT,
    );
    let after = env.portfolio_state(portfolio);
    let converted = after.capital.get();
    assert!(
        converted > 0 && converted <= RELEASED,
        "refreshed conversion makes bounded progress without over-converting: {converted}"
    );
    assert!(
        after.pnl.get() >= 0 && after.pnl.get() < RELEASED as i128,
        "conversion consumes released PnL without increasing the claim: pnl={}",
        after.pnl.get()
    );
    assert_eq!(
        env.market_state().1.vault as u64,
        env.token_amount(env.vault),
        "conversion preserves SPL custody parity"
    );
}

#[test]
fn v16_attack_target_only_lag_invalidates_unrelated_single_trade_cert() {
    const PRICE: u64 = 100;
    const TARGET: u64 = 90;
    const ASSET1_SIZE_Q: i128 = (10 * POS_SCALE) as i128;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 500);
    env.configure_auth_mark_for_asset_as_admin(1, 0, PRICE);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000_000);
    env.deposit(&short_owner, short_account, 1_000_000_000);
    env.trade_asset_with_cu(
        1,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        ASSET1_SIZE_Q,
        PRICE,
        0,
    );

    let (_, before_target) = env.market_state();
    let long_before = env.portfolio_state(long_account);
    let short_before = env.portfolio_state(short_account);
    assert_eq!(
        health_cert(&long_before).cert_oracle_epoch,
        before_target.oracle_epoch
    );
    assert_eq!(
        health_cert(&short_before).cert_oracle_epoch,
        before_target.oracle_epoch
    );

    let cranker_owner = Keypair::new();
    let cranker_account = env.create_portfolio(&cranker_owner);
    env.push_auth_mark_for_asset_as_admin(1, 0, TARGET);
    env.crank(
        cranker_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: crank_observations(1),
        },
    );

    let (_, lagged_group) = env.market_state();
    let stale_long = env.portfolio_state(long_account);
    let stale_short = env.portfolio_state(short_account);
    assert_eq!(lagged_group.assets[1].raw_oracle_target_price, TARGET);
    assert_eq!(
        lagged_group.assets[1].effective_price, PRICE,
        "same-slot crank must create target-only lag without an effective-price move"
    );
    assert_eq!(lagged_group.oracle_epoch, before_target.oracle_epoch + 1);
    assert!(
        health_cert(&stale_long).cert_oracle_epoch < lagged_group.oracle_epoch
            && health_cert(&stale_short).cert_oracle_epoch < lagged_group.oracle_epoch,
        "target-only lag must invalidate every prior portfolio certificate in O(1)"
    );

    let trade_cu = env.trade_asset_with_cu(
        0,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        PRICE,
        0,
    );
    assert_cu_within(
        "unrelated single trade after target-only lag",
        trade_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );

    let (_, after_trade_group) = env.market_state();
    let long_after = env.portfolio_state(long_account);
    let short_after = env.portfolio_state(short_account);
    let long_cert = health_cert(&long_after);
    let short_cert = health_cert(&short_after);
    assert_eq!(long_cert.cert_oracle_epoch, after_trade_group.oracle_epoch);
    assert_eq!(short_cert.cert_oracle_epoch, after_trade_group.oracle_epoch);
    assert_eq!(
        long_cert.certified_maintenance_req,
        short_cert.certified_maintenance_req + 100,
        "the unrelated trade must retain asset 1's adverse long lag penalty"
    );
    assert_eq!(long_cert.certified_maintenance_req, 1_200);
    assert_eq!(short_cert.certified_maintenance_req, 1_100);
}
