//! INV-078 - Permissionless recovery coverage.
//!
//! Normative obligation: Every ordinary-progress failure class retains a permissionless senior-preserving terminal route.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_attack_locally_stale_permissionless_asset_can_shutdown_and_force_close`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_attack_locally_stale_permissionless_asset_can_shutdown_and_force_close() {
    const STALE_SLOTS: u64 = 20;
    const DELAY: u64 = 4;
    const CREATE_SLOT: u64 = 1;
    const STALE_SLOT: u64 = 25;
    const FORCE_SLOT: u64 = STALE_SLOT + DELAY + 1;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 1_000, 1_000, 500);
    env.update_market_init_fee_policy_with_cu(1);
    env.configure_permissionless_resolve_with_cu(STALE_SLOTS, DELAY);
    env.configure_auth_mark_with_cu(0, 100);

    let creator = Keypair::new();
    let creator_key = creator.pubkey();
    env.svm.warp_to_slot(CREATE_SLOT);
    env.activate_permissionless_asset_with_fee(
        &creator,
        1,
        CREATE_SLOT,
        100,
        creator_key,
        creator_key,
        creator_key,
        creator_key,
        1,
    );
    env.configure_auth_mark_for_asset_with_authority(1, &creator, CREATE_SLOT, 100);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 1_000_000);
    env.deposit(&short_owner, short, 1_000_000);
    env.trade_asset_with_cu(
        1,
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(STALE_SLOT);
    env.push_auth_mark_with_cu(STALE_SLOT, 100);
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (cfg, group) = state::read_market(&market_data).expect("read market");
    let local_profile =
        state::read_asset_oracle_profile(&market_data, 1).expect("read local profile");
    assert!(
        !oracle_v16::permissionless_stale_matured(&cfg, STALE_SLOT),
        "base market must stay fresh so this is only a local asset-stale probe"
    );
    assert!(
        STALE_SLOT.saturating_sub(local_profile.last_good_oracle_slot) >= STALE_SLOTS,
        "asset-1 local profile must be stale: last_good={} now={STALE_SLOT}",
        local_profile.last_good_oracle_slot
    );
    assert_eq!(group.assets[1].oi_eff_long_q, POS_SCALE);
    assert_eq!(group.assets[1].oi_eff_short_q, POS_SCALE);

    let market_before_trade = env.svm.get_account(&env.market).unwrap();
    let long_before_trade = env.svm.get_account(&long).unwrap();
    let short_before_trade = env.svm.get_account(&short).unwrap();
    env.svm.expire_blockhash();
    let stale_trade = env.try_trade_asset_with_cu(
        1,
        &long_owner,
        long,
        &short_owner,
        short,
        -(POS_SCALE as i128),
        100,
        0,
    );
    assert!(
        stale_trade.is_err(),
        "locally stale permissionless asset must reject new trade mutations"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_trade
    );
    assert_eq!(env.svm.get_account(&long).unwrap(), long_before_trade);
    assert_eq!(env.svm.get_account(&short).unwrap(), short_before_trade);

    env.svm.expire_blockhash();
    let shutdown_cu = env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
        1,
        STALE_SLOT,
        0,
    );
    assert_cu_within(
        "locally stale permissionless asset shutdown",
        shutdown_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        env.market_state().1.assets[1].lifecycle,
        AssetLifecycleV16::Recovery,
        "marketauth can still move the locally stale asset into Recovery"
    );

    let cranker = Keypair::new();
    env.svm.warp_to_slot(FORCE_SLOT);
    env.svm.expire_blockhash();
    let force_cu =
        env.force_close_abandoned_asset_with_cu(&cranker, long, short, 1, FORCE_SLOT, POS_SCALE);
    assert_cu_within(
        "locally stale permissionless asset force-close",
        force_cu,
        TRADE_CU_LIMIT,
    );
    let (_, after) = env.market_state();
    assert_eq!(after.assets[1].oi_eff_long_q, 0);
    assert_eq!(after.assets[1].oi_eff_short_q, 0);
    assert!(!has_active_leg_for_asset(&env.portfolio_state(long), 1));
    assert!(!has_active_leg_for_asset(&env.portfolio_state(short), 1));
    assert!(
        after.vault >= after.c_tot + after.insurance,
        "senior conservation holds after locally stale shutdown and force-close"
    );
    assert_eq!(after.vault as u64, env.token_amount(env.vault));
}
