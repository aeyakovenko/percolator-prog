//! INV-074 - Scope locality.
//!
//! Normative obligation: Scoped state affects only its own asset, side, portfolio, domain, close, or receipt.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_attack_permissionless_asset_bankruptcy_does_not_freeze_base_trading`, `v16_attack_permissionless_oracle_reconfiguration_preserves_unrelated_fee_and_exit_liveness`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_attack_permissionless_asset_bankruptcy_does_not_freeze_base_trading() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    env.update_market_init_fee_policy_with_cu(1);

    let creator = Keypair::new();
    let creator_key = creator.pubkey();
    env.activate_permissionless_asset_with_fee(
        &creator,
        1,
        1,
        100,
        creator_key,
        creator_key,
        creator_key,
        creator_key,
        1,
    );
    env.configure_auth_mark_for_asset_with_authority(1, &creator, 1, 100);

    let attacker_long = Keypair::new();
    let attacker_short = Keypair::new();
    let long_account = env.create_portfolio(&attacker_long);
    let short_account = env.create_portfolio(&attacker_short);
    env.deposit(&attacker_long, long_account, 1_000_000);
    env.deposit(&attacker_short, short_account, 250);
    env.trade_asset_with_cu(
        1,
        &attacker_long,
        long_account,
        &attacker_short,
        short_account,
        POS_SCALE as i128,
        100,
        0,
    );

    for (slot, mark) in [(2u64, 200u64), (3, 400), (4, 800)] {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_for_asset_with_authority(1, &creator, slot, mark);
        for portfolio in [long_account, short_account] {
            env.svm.expire_blockhash();
            let _ = env.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(1),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[],
            );
        }
    }
    env.crank_steps(
        short_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 4,
            observations: crank_observations(1),
        },
        4,
    );

    let (_, failed_asset_group) = env.market_state();
    assert_eq!(failed_asset_group.mode, MarketModeV16::Live);
    assert!(
        failed_asset_group.bankruptcy_hlock_active,
        "probe must activate the cross-market bankruptcy flag"
    );
    assert_eq!(env.portfolio_state(short_account).capital.get(), 0);
    assert_eq!(env.portfolio_state(short_account).pnl.get(), 0);

    let base_long = Keypair::new();
    let base_short = Keypair::new();
    let base_long_account = env.create_portfolio(&base_long);
    let base_short_account = env.create_portfolio(&base_short);
    env.deposit(&base_long, base_long_account, 1_000_000);
    env.deposit(&base_short, base_short_account, 1_000_000);
    env.svm.expire_blockhash();
    let base_trade = env.try_trade_asset_with_cu(
        0,
        &base_long,
        base_long_account,
        &base_short,
        base_short_account,
        POS_SCALE as i128,
        100,
        0,
    );
    assert!(
        base_trade.is_ok(),
        "a permissionless asset's self-bankruptcy must not freeze unrelated base trading: {base_trade:?}"
    );
    let (_, final_group) = env.market_state();
    assert_eq!(final_group.assets[0].oi_eff_long_q, POS_SCALE);
    assert_eq!(final_group.assets[0].oi_eff_short_q, POS_SCALE);
}

#[test]
fn v16_attack_permissionless_oracle_reconfiguration_preserves_unrelated_fee_and_exit_liveness() {
    let mut env =
        V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(1, 1_000, 1_000, 500, 100);
    env.update_market_init_fee_policy_with_cu(1);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, 100);

    let creator = Keypair::new();
    env.activate_permissionless_asset_with_fee(
        &creator,
        1,
        1,
        100,
        creator.pubkey(),
        creator.pubkey(),
        creator.pubkey(),
        creator.pubkey(),
        1,
    );
    env.configure_auth_mark_for_asset_with_authority(1, &creator, 1, 100);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 1_000_000);
    env.deposit(&short_owner, short, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        (100 * POS_SCALE) as i128,
        100,
        0,
    );
    let before = env.market_state().1;
    assert_eq!(before.assets[0].slot_last, 1);
    assert_eq!(before.slot_last, 1);
    assert_eq!(before.assets[1].oi_eff_long_q, 0);
    assert_eq!(before.pnl_pos_tot, 0);

    env.svm.warp_to_slot(100);
    let configure_cu = env.configure_auth_mark_for_asset_with_authority(1, &creator, 100, 200);
    let after_configure = env.market_state().1;
    assert_eq!(after_configure.current_slot, 100);
    assert_eq!(after_configure.slot_last, 100);
    assert_eq!(after_configure.assets[0].slot_last, 1);
    assert_cu_within(
        "permissionless cross-asset oracle reconfiguration",
        configure_cu,
        CUSTODY_CU_LIMIT,
    );

    let cap_before = env.portfolio_state(long).capital.get();
    let fee_slot_before = env.portfolio_state(long).last_fee_slot.get();
    let insurance_before = env.market_state().1.insurance;
    let sync_cu = env
        .try_sync_maintenance_fee_with_cu(long, None, 100)
        .expect("cross-asset fee sync remains live");
    let long_after = env.portfolio_state(long);
    let cap_after = long_after.capital.get();
    let insurance_after = env.market_state().1.insurance;
    assert_cu_within(
        "cross-asset loss-safe maintenance sync",
        sync_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(cap_after, cap_before);
    assert_eq!(long_after.last_fee_slot.get(), fee_slot_before);
    assert_eq!(insurance_after, insurance_before);

    let close_cu = env
        .try_trade_asset_with_cu(
            0,
            &long_owner,
            long,
            &short_owner,
            short,
            -(100 * POS_SCALE as i128),
            100,
            0,
        )
        .expect("unrelated oracle reconfiguration must not block a signed risk-reducing exit");
    assert_cu_within("cross-asset stale-position exit", close_cu, TRADE_CU_LIMIT);
    let after_close = env.market_state().1;
    assert_eq!(after_close.assets[0].oi_eff_long_q, 0);
    assert_eq!(after_close.assets[0].oi_eff_short_q, 0);
}
