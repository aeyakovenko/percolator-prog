//! INV-020 - Authenticated clock, slot, and oracle provenance.
//!
//! Normative obligation: Time and oracle observations are authenticated, coherent, and cannot be caller-rewound.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_attack_recovery_oracle_push_cannot_extend_force_close_deadline`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_attack_recovery_oracle_push_cannot_extend_force_close_deadline() {
    const SHUTDOWN_SLOT: u64 = 2;
    const FORCE_CLOSE_SLOT: u64 = 7;

    let mut env = V16CuEnv::new();
    let oracle_authority = Keypair::new();
    let cranker = Keypair::new();
    env.configure_permissionless_resolve_with_cu(100, 5);
    env.activate_asset_with_authorities(
        1,
        1,
        100,
        env.admin.pubkey(),
        env.admin.pubkey(),
        env.admin.pubkey(),
        oracle_authority.pubkey(),
    );
    env.configure_auth_mark_for_asset_with_authority(1, &oracle_authority, 1, 100);

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

    env.svm.warp_to_slot(SHUTDOWN_SLOT);
    env.update_asset_lifecycle_as_admin_with_cu(
        processor::ASSET_ACTION_SHUTDOWN,
        1,
        SHUTDOWN_SLOT,
        0,
    );
    let shutdown_market = env.svm.get_account(&env.market).unwrap();
    let shutdown_profile = state::read_asset_oracle_profile(&shutdown_market.data, 1).unwrap();
    assert_eq!(shutdown_profile.last_good_oracle_slot, SHUTDOWN_SLOT);
    assert_eq!(
        env.market_state().1.assets[1].lifecycle,
        AssetLifecycleV16::Recovery
    );

    // Try one slot before the original deadline. If this push were accepted, it
    // would move the force-close deadline from slot 7 to slot 11.
    env.svm.warp_to_slot(FORCE_CLOSE_SLOT - 1);
    env.svm.expire_blockhash();
    let push = env.send(
        ProgInstruction::PushAuthMark {
            asset_index: 1,
            now_slot: FORCE_CLOSE_SLOT - 1,
            mark_e6: 101,
        },
        vec![
            AccountMeta::new(oracle_authority.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&oracle_authority],
    );
    assert!(
        push.is_err(),
        "Recovery asset must reject oracle timestamp refresh"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        shutdown_market,
        "rejected push must preserve the original shutdown epoch"
    );

    env.svm.warp_to_slot(FORCE_CLOSE_SLOT);
    let force_close_cu = env.force_close_abandoned_asset_with_cu(
        &cranker,
        long,
        short,
        1,
        FORCE_CLOSE_SLOT,
        POS_SCALE,
    );
    assert_cu_within(
        "force close at immutable Recovery deadline",
        force_close_cu,
        TRADE_CU_LIMIT,
    );
    let after = env.market_state().1;
    assert_eq!(after.assets[1].oi_eff_long_q, 0);
    assert_eq!(after.assets[1].oi_eff_short_q, 0);
    assert!(!has_active_leg_for_asset(&env.portfolio_state(long), 1));
    assert!(!has_active_leg_for_asset(&env.portfolio_state(short), 1));
}
