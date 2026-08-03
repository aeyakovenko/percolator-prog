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
        ProgInstruction::ConvertReleasedPnl { amount: RELEASED },
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
