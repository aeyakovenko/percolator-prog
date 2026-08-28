//! INV-042 - Recovery fallback envelope.
//!
//! Normative obligation: fallback recovery/force-close uses authenticated
//! timing, only abandoned Recovery assets are eligible, malformed pairs reject
//! atomically, and caller-selected close size is bounded by real exposure before
//! value or OI can move.
//!
//! Evidence in this file (I/C plus a production-source absence guard): deployed LiteSVM public wrapper tests cover
//! healthy-asset rejection, same-side pair rejection with a valid opposite-side
//! control, oversized `close_q` clamping, and the authenticated shutdown-delay
//! clock. Synthetic fallback pricing is reserved in the pinned engine: the public
//! force-close wire carries no price and the handler trades only at the stored
//! effective mark. A future fallback input or config consumer reopens the full
//! price/value-transfer envelope.

use super::*;

#[test]
fn v16_program_force_close_healthy_asset_rejected() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 1,
            now_slot: 0,
            initial_mark_e6: 100,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&env.admin],
    )
    .expect("cfg asset1 mark");
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(1, &la, pa, &lb, pb, POS_SCALE as i128, 100, 100);
    let (_, g0) = env.market_state();
    assert!(g0.assets[1].oi_eff_long_q > 0);
    assert_eq!(g0.assets[1].oi_eff_long_q, g0.assets[1].oi_eff_short_q);

    let cranker = Keypair::new();
    let rejected =
        env.try_force_close_abandoned_asset_with_cu(&cranker, pa, pb, 1, 5_000_000, POS_SCALE);
    assert!(
        rejected.is_err(),
        "force-close of a healthy ACTIVE asset must reject"
    );
    let g1 = env.market_state().1;
    assert_eq!(g1.assets[1].oi_eff_long_q, g0.assets[1].oi_eff_long_q);
    assert_eq!(g1.assets[1].oi_eff_short_q, g0.assets[1].oi_eff_short_q);
    assert_eq!(g1.vault, g0.vault);
    assert!(g1.vault >= g1.c_tot + g1.insurance);
}

#[test]
fn v16_program_force_close_requires_opposite_sides() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    const DELAY: u64 = 5;
    const SHUT: u64 = 10;
    env.configure_permissionless_resolve_with_cu(100, DELAY);

    let long_owner_a = Keypair::new();
    let short_owner_a = Keypair::new();
    let long_owner_b = Keypair::new();
    let short_owner_b = Keypair::new();
    let long_a = env.create_portfolio(&long_owner_a);
    let short_a = env.create_portfolio(&short_owner_a);
    let long_b = env.create_portfolio(&long_owner_b);
    let short_b = env.create_portfolio(&short_owner_b);
    for (owner, portfolio) in [
        (&long_owner_a, long_a),
        (&short_owner_a, short_a),
        (&long_owner_b, long_b),
        (&short_owner_b, short_b),
    ] {
        env.deposit(owner, portfolio, 1_000_000);
    }
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        1,
        &long_owner_a,
        long_a,
        &short_owner_a,
        short_a,
        POS_SCALE as i128,
        100,
        0,
    );
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        1,
        &long_owner_b,
        long_b,
        &short_owner_b,
        short_b,
        POS_SCALE as i128,
        100,
        0,
    );
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(long_a), 1).side,
        SideV16::Long
    );
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(long_b), 1).side,
        SideV16::Long
    );
    assert_eq!(env.market_state().1.assets[1].oi_eff_long_q, 2 * POS_SCALE);

    env.svm.warp_to_slot(SHUT);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
        1,
        SHUT,
        0,
    );
    env.svm.warp_to_slot(SHUT + DELAY + 1);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let long_a_before = env.svm.get_account(&long_a).unwrap();
    let long_b_before = env.svm.get_account(&long_b).unwrap();
    let long_epoch_before = env.portfolio_position_epoch(long_a);
    let short_epoch_before = env.portfolio_position_epoch(short_a);
    let cranker = Keypair::new();
    let same_side = env.try_force_close_abandoned_asset_with_cu(
        &cranker,
        long_a,
        long_b,
        1,
        SHUT + DELAY + 1,
        POS_SCALE,
    );
    assert!(
        same_side.is_err(),
        "force-close must reject two same-side accounts"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&long_a).unwrap(), long_a_before);
    assert_eq!(env.svm.get_account(&long_b).unwrap(), long_b_before);
    assert_eq!(env.portfolio_position_epoch(long_a), long_epoch_before);
    assert_eq!(env.portfolio_position_epoch(short_a), short_epoch_before);

    env.svm.expire_blockhash();
    let ok = env.try_force_close_abandoned_asset_with_cu(
        &cranker,
        long_a,
        short_a,
        1,
        SHUT + DELAY + 1,
        POS_SCALE,
    );
    assert!(
        ok.is_ok(),
        "valid opposite-side force-close still succeeds: {ok:?}"
    );
    assert_eq!(env.portfolio_position_epoch(long_a), long_epoch_before + 1);
    assert_eq!(
        env.portfolio_position_epoch(short_a),
        short_epoch_before + 1
    );
    let group = env.market_state().1;
    assert_eq!(group.assets[1].oi_eff_long_q, POS_SCALE);
    assert_eq!(group.assets[1].oi_eff_short_q, POS_SCALE);
    assert!(!has_active_leg_for_asset(&env.portfolio_state(long_a), 1));
    assert!(has_active_leg_for_asset(&env.portfolio_state(long_b), 1));
    assert_eq!(group.vault as u64, env.token_amount(env.vault));
    assert!(group.vault >= group.c_tot + group.insurance);
}

#[test]
fn v16_program_force_close_oversized_close_q_clamps_before_i128_cast() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    const DELAY: u64 = 5;
    const SHUT: u64 = 10;
    env.configure_permissionless_resolve_with_cu(100, DELAY);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 1_000_000);
    env.deposit(&short_owner, short, 1_000_000);
    env.svm.expire_blockhash();
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
    let before = env.market_state().1;
    assert_eq!(before.assets[1].oi_eff_long_q, POS_SCALE);
    assert_eq!(before.assets[1].oi_eff_short_q, POS_SCALE);

    env.svm.warp_to_slot(SHUT);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
        1,
        SHUT,
        0,
    );
    env.svm.warp_to_slot(SHUT + DELAY + 1);

    let cranker = Keypair::new();
    let close = env.try_force_close_abandoned_asset_with_cu(
        &cranker,
        long,
        short,
        1,
        SHUT + DELAY + 1,
        u128::MAX,
    );
    assert!(
        close.is_ok(),
        "oversized force-close close_q must clamp before i128 cast: {close:?}"
    );
    let after = env.market_state().1;
    assert_eq!(after.assets[1].oi_eff_long_q, 0);
    assert_eq!(after.assets[1].oi_eff_short_q, 0);
    assert!(!has_active_leg_for_asset(&env.portfolio_state(long), 1));
    assert!(!has_active_leg_for_asset(&env.portfolio_state(short), 1));
    assert_eq!(after.vault as u64, env.token_amount(env.vault));
    assert!(after.vault >= after.c_tot + after.insurance);
}

#[test]
fn v16_program_force_close_cannot_bypass_timeout_with_future_now_slot() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    const DELAY: u64 = 50;
    env.configure_permissionless_resolve_with_cu(100, DELAY);
    let la = Keypair::new();
    let lb = Keypair::new();
    let pa = env.create_portfolio(&la);
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(1, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    const SHUT: u64 = 10;
    env.svm.warp_to_slot(SHUT);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
        1,
        u64::MAX,
        0,
    );
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let shutdown_profile = state::read_asset_oracle_profile(&market_data, 1).unwrap();
    assert_eq!(
        shutdown_profile.last_good_oracle_slot, SHUT,
        "shutdown must store the authenticated clock slot"
    );

    env.svm.warp_to_slot(SHUT + 1);
    let cranker = Keypair::new();
    let liar = env.try_force_close_abandoned_asset_with_cu(
        &cranker,
        pa,
        pb,
        1,
        SHUT + DELAY + 10_000,
        POS_SCALE,
    );
    assert!(
        liar.is_err(),
        "caller now_slot cannot bypass an authenticated shutdown delay"
    );
    assert_eq!(
        env.market_state().1.assets[1].lifecycle,
        AssetLifecycleV16::Recovery
    );

    env.svm.warp_to_slot(SHUT + DELAY + 5);
    env.svm.expire_blockhash();
    let ok = env.try_force_close_abandoned_asset_with_cu(
        &cranker,
        pa,
        pb,
        1,
        SHUT + DELAY + 5,
        POS_SCALE,
    );
    assert!(
        ok.is_ok(),
        "force-close succeeds once the authenticated clock crosses the window: {ok:?}"
    );
}

#[test]
fn v16_program_recovery_fallback_pricing_is_absent_and_force_close_uses_frozen_mark() {
    let source = include_str!("../../../src/v16_program.rs");
    let enum_start = source
        .find("ForceCloseAbandonedAsset {")
        .expect("force-close instruction variant");
    let enum_tail = &source[enum_start..];
    let enum_end = enum_tail.find("},").expect("force-close variant end") + 2;
    let wire = &enum_tail[..enum_end];
    assert!(wire.contains("asset_index: u16"));
    assert!(wire.contains("now_slot: u64"));
    assert!(wire.contains("close_q: u128"));
    for forbidden in ["price", "reference", "deviation", "envelope"] {
        assert!(
            !wire.contains(forbidden),
            "reserved fallback field {forbidden} became caller-controlled",
        );
    }

    let handler_start = source
        .find("fn handle_force_close_abandoned_asset")
        .expect("force-close handler");
    let handler_tail = &source[handler_start..];
    let handler_end = handler_tail
        .find("fn matcher_tail_start_or_verify_lp_config")
        .expect("force-close handler end");
    let handler = &handler_tail[..handler_end];
    assert!(handler.contains("let frozen_mark = asset.effective_price.get();"));
    assert!(handler.contains("exec_price: frozen_mark"));
    assert!(handler.contains("if frozen_mark == 0 || frozen_mark > percolator::MAX_ORACLE_PRICE"));
    for forbidden in [
        "max_recovery_fallback_deviation_bps",
        "recovery_fallback_price_enabled",
        "recovery_fallback_envelope_enabled",
        "fallback_recovery_price",
    ] {
        assert!(
            !handler.contains(forbidden),
            "reserved fallback control {forbidden} became active in force-close",
        );
    }

    let lock = include_str!("../../../Cargo.lock");
    assert!(lock.contains(
        "git+https://github.com/aeyakovenko/percolator?rev=9b737fd#\
         9b737fdcec16f3709c0651f4ecc7488b4917f2d8"
    ));
}
