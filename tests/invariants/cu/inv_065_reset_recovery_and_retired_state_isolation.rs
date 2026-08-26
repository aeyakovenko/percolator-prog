//! INV-065 - reset, recovery, and retired-state isolation.
//!
//! Once a side enters `ResetPending`, it must not admit fresh counterparty risk
//! until the side is actually empty and finalized. The same public sequence must
//! preserve cleanup progress and the innocent counterparty's withdrawable funds.

use super::*;

pub(super) struct PublicEmptyLongResetPendingFixture {
    pub(super) env: V16CuEnv,
    pub(super) long_owner: Keypair,
    pub(super) long: Pubkey,
}

pub(super) fn public_empty_long_reset_pending_fixture() -> PublicEmptyLongResetPendingFixture {
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: 1,
        max_trading_fee_bps: 10,
        max_bankrupt_close_lifetime_slots: 1,
        public_b_chunk_atoms: 1,
        maintenance_fee_per_slot: 20_000,
        ..V16CuMarketParams::default()
    });
    let short_owner = Keypair::new();
    let long_owner = Keypair::new();
    let short = env.create_portfolio(&short_owner);
    let long = env.create_portfolio(&long_owner);
    env.deposit(&short_owner, short, 20_000);
    env.deposit(&long_owner, long, 20_000);

    let open_cu = env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        (10_000 * POS_SCALE) as i128,
        1,
        0,
    );
    assert_cu_within("normal risk-increasing trade", open_cu, TRADE_CU_LIMIT);
    env.svm.warp_to_slot(1);
    env.crank(
        short,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
    );
    env.sync_maintenance_fee_with_cu(short, None, 1);
    env.crank_steps(
        short,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: Vec::new(),
        },
        2,
    );

    let (_, group) = env.market_state();
    let reset_pending = group.assets[0];
    assert_eq!(reset_pending.lifecycle, AssetLifecycleV16::Active);
    assert_eq!(reset_pending.mode_long, SideModeV16::ResetPending);
    assert_eq!(reset_pending.oi_eff_long_q, 0);
    assert_eq!(reset_pending.stored_pos_count_long, 1);

    env.crank(
        long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: Vec::new(),
        },
    );
    let (_, group_after_cleanup) = env.market_state();
    let reset_pending_after_cleanup = group_after_cleanup.assets[0];
    assert_eq!(
        reset_pending_after_cleanup.mode_long,
        SideModeV16::ResetPending
    );
    assert_eq!(reset_pending_after_cleanup.stored_pos_count_long, 0);
    assert!(!has_active_leg_for_asset(&env.portfolio_state(long), 0));

    PublicEmptyLongResetPendingFixture {
        env,
        long_owner,
        long,
    }
}

#[test]
fn v16_program_reset_pending_rejects_fresh_counterparty_and_completes_recovery() {
    let PublicEmptyLongResetPendingFixture {
        mut env,
        long_owner,
        long,
    } = public_empty_long_reset_pending_fixture();
    let reset_pending_after_cleanup = env.market_state().1.assets[0];
    let fresh_short_owner = Keypair::new();
    let fresh_short = env.create_portfolio(&fresh_short_owner);
    env.deposit(&fresh_short_owner, fresh_short, 20_000);

    env.svm.expire_blockhash();
    let market_before = env.svm.get_account(&env.market).unwrap();
    let long_before = env.svm.get_account(&long).unwrap();
    let fresh_short_before = env.svm.get_account(&fresh_short).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let rejected = env
        .try_trade_asset_with_cu(
            0,
            &long_owner,
            long,
            &fresh_short_owner,
            fresh_short,
            (551 * POS_SCALE) as i128,
            1,
            0,
        )
        .expect_err("ResetPending side must reject a fresh counterparty risk increase");
    assert!(
        rejected.contains("Custom(21)") || rejected.contains("custom program error: 0x15"),
        "fresh risk must fail specifically at the engine recovery gate, got {rejected}"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&long).unwrap(), long_before);
    assert_eq!(
        env.svm.get_account(&fresh_short).unwrap(),
        fresh_short_before
    );
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    let (_, group) = env.market_state();
    assert_eq!(group.assets[0], reset_pending_after_cleanup);
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &env.portfolio_state(fresh_short)
    )));

    let finalize_cu = env.finalize_reset_side_with_cu(0, 0);
    assert_cu_within("FinalizeResetSide", finalize_cu, CUSTODY_CU_LIMIT);
    let (_, finalized) = env.market_state();
    assert_eq!(finalized.assets[0].mode_long, SideModeV16::Normal);
    assert_eq!(finalized.assets[0].stored_pos_count_long, 0);
    assert_eq!(finalized.assets[0].stored_pos_count_short, 0);
    assert_eq!(finalized.assets[0].oi_eff_long_q, 0);
    assert_eq!(finalized.assets[0].oi_eff_short_q, 0);

    let fresh_dest = env.withdraw(&fresh_short_owner, fresh_short, 20_000);
    assert_eq!(
        env.token_amount(fresh_dest),
        20_000,
        "rejected counterparty retains fully withdrawable principal"
    );
}

// security.md sweep — permissionless reset finalizer (#31/#44): anyone may finalize a reset-pending
// side, but only after all engine reset blockers for that side are zero. A public finalizer must not
// be able to unlock trading while positions, stale accounts, pending obligations, or domain-loss
// barriers still need recovery/cranking.
#[test]
fn v16_attack_finalize_reset_side_requires_empty_side_counts() {
    let mut env = V16CuEnv::new();
    env.mutate_market(|_, group| {
        group.assets[0].mode_long = SideModeV16::ResetPending;
        group.assets[0].stored_pos_count_long = 1;
    });

    env.svm.expire_blockhash();
    let r_stored = env.send(
        ProgInstruction::FinalizeResetSide {
            asset_index: 0,
            side: 0,
        },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(
        r_stored.is_err(),
        "stored positions must block reset finalization"
    );
    assert_eq!(
        env.market_state().1.assets[0].mode_long,
        SideModeV16::ResetPending,
        "rejected finalization must leave the side locked",
    );

    env.mutate_market(|_, group| {
        group.assets[0].stored_pos_count_long = 0;
        group.assets[0].stale_account_count_long = 1;
    });
    env.svm.expire_blockhash();
    let r_stale = env.send(
        ProgInstruction::FinalizeResetSide {
            asset_index: 0,
            side: 0,
        },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(
        r_stale.is_err(),
        "stale accounts must block reset finalization"
    );
    assert_eq!(
        env.market_state().1.assets[0].mode_long,
        SideModeV16::ResetPending,
        "stale-count rejection must leave the side locked",
    );

    env.mutate_market(|_, group| {
        group.assets[0].mode_long = SideModeV16::ResetPending;
        group.assets[0].stale_account_count_long = 0;
        group.assets[0].pending_obligation_count_long = 1;
    });
    assert_eq!(
        env.market_state().1.assets[0].mode_long,
        SideModeV16::ResetPending,
        "test precondition: side is reset-pending",
    );
    assert_eq!(
        env.market_state().1.assets[0].pending_obligation_count_long,
        1,
        "test precondition: pending obligation is persisted",
    );
    {
        let mut raw = env.svm.get_account(&env.market).unwrap();
        let (_cfg, view) = state::market_view_mut(&mut raw.data).unwrap();
        let asset = view.markets[0].engine.asset.try_to_runtime().unwrap();
        assert_eq!(
            asset.pending_obligation_count_long, 1,
            "test precondition: zero-copy view sees pending obligation",
        );
        assert_eq!(
            asset.mode_long,
            SideModeV16::ResetPending,
            "test precondition: zero-copy view sees reset-pending side",
        );
    }
    env.svm.expire_blockhash();
    let r_obligation = env.send(
        ProgInstruction::FinalizeResetSide {
            asset_index: 0,
            side: 0,
        },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(
        r_obligation.is_err(),
        "pending obligations must block reset finalization",
    );
    assert_eq!(
        env.market_state().1.assets[0].mode_long,
        SideModeV16::ResetPending,
        "pending-obligation rejection must leave the side locked",
    );

    env.mutate_market(|_, group| {
        group.assets[0].mode_long = SideModeV16::ResetPending;
        group.assets[0].pending_obligation_count_long = 0;
        group.pending_domain_loss_barriers[0] = 1;
    });
    assert_eq!(
        env.market_state().1.assets[0].mode_long,
        SideModeV16::ResetPending,
        "test precondition: side is reset-pending",
    );
    assert_eq!(
        env.market_state().1.pending_domain_loss_barriers[0],
        1,
        "test precondition: domain-loss barrier is persisted",
    );
    env.svm.expire_blockhash();
    let r_barrier = env.send(
        ProgInstruction::FinalizeResetSide {
            asset_index: 0,
            side: 0,
        },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(
        r_barrier.is_err(),
        "pending domain-loss barriers must block reset finalization",
    );
    assert_eq!(
        env.market_state().1.assets[0].mode_long,
        SideModeV16::ResetPending,
        "domain-barrier rejection must leave the side locked",
    );

    env.mutate_market(|_, group| {
        group.pending_domain_loss_barriers[0] = 0;
    });
    env.svm.expire_blockhash();
    let r_empty = env.send(
        ProgInstruction::FinalizeResetSide {
            asset_index: 0,
            side: 0,
        },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(
        r_empty.is_ok(),
        "anyone may finalize once the side is empty: {:?}",
        r_empty
    );
    assert_eq!(
        env.market_state().1.assets[0].mode_long,
        SideModeV16::Normal,
        "empty reset-pending side unlocks",
    );
}

// security.md sweep - unsigned reset finalizer must not unlock drain-only sides (#30/#48):
// FinalizeResetSide is intentionally permissionless, and Normal is an idempotent no-op, but DrainOnly
// is a distinct risk throttle. A public caller must not be able to treat DrainOnly like ResetPending
// and reopen risk by finalizing it.
#[test]
fn v16_attack_finalize_reset_side_cannot_unlock_drain_only_modes() {
    let mut env = V16CuEnv::new();
    env.mutate_market(|_, group| {
        group.assets[0].mode_long = SideModeV16::DrainOnly;
        group.assets[0].mode_short = SideModeV16::DrainOnly;
    });
    let before = env.svm.get_account(&env.market).unwrap();

    for (side, label) in [(0u8, "long"), (1u8, "short")] {
        env.svm.expire_blockhash();
        let rejected = env.send(
            ProgInstruction::FinalizeResetSide {
                asset_index: 0,
                side,
            },
            vec![AccountMeta::new(env.market, false)],
            &[],
        );
        assert!(
            rejected.is_err(),
            "permissionless finalizer must reject {label} DrainOnly mode"
        );
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            before,
            "rejected {label} DrainOnly finalization must leave the market unchanged"
        );
    }

    let (_, group) = env.market_state();
    assert_eq!(group.assets[0].mode_long, SideModeV16::DrainOnly);
    assert_eq!(group.assets[0].mode_short, SideModeV16::DrainOnly);

    // Control: the short-side ResetPending path still finalizes when the side is genuinely empty.
    let mut control = V16CuEnv::new();
    control.mutate_market(|_, group| {
        group.assets[0].mode_short = SideModeV16::ResetPending;
    });
    let risk_epoch_before = control.market_state().1.risk_epoch;
    control.svm.expire_blockhash();
    let accepted = control.send(
        ProgInstruction::FinalizeResetSide {
            asset_index: 0,
            side: 1,
        },
        vec![AccountMeta::new(control.market, false)],
        &[],
    );
    assert!(
        accepted.is_ok(),
        "empty short ResetPending side remains permissionlessly finalizable: {accepted:?}"
    );
    let (_, control_group) = control.market_state();
    assert_eq!(control_group.assets[0].mode_short, SideModeV16::Normal);
    assert_eq!(
        control_group.risk_epoch,
        risk_epoch_before + 1,
        "real reset finalization bumps the risk epoch exactly once"
    );
}

// security.md sweep — recovery blocks junior→senior conversion (#6/#19/#33 interaction): ConvertReleasedPnl
// moves backed junior pnl into withdrawable senior capital, and is Live-only (the engine's release path
// requires Live mode). Attacker goal: during a Recovery wind-down, convert junior pnl to senior capital
// to jump the queue / extract ahead of the orderly resolution. Protection: ConvertReleasedPnl rejects in
// Recovery and the account/market state is fully preserved (the junior claim stays junior).
#[test]
fn v16_attack_recovery_blocks_pnl_conversion() {
    let mut env = V16CuEnv::new();
    env.top_up_backing_bucket(1, 40, 10_000);
    let o = Keypair::new();
    let p = env.create_portfolio(&o);
    env.deposit(&o, p, 1_000);
    env.add_source_positive_pnl(p, 1, 40);
    env.crank(
        p,
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: crank_observations(0),
        },
    );
    let pre = env.portfolio_state(p);
    assert!(
        pre.pnl.get() > 0,
        "holder has backed junior pnl to (attempt to) convert, pnl={}",
        pre.pnl.get()
    );

    // transition to Recovery.
    env.mutate_market(|_, group| {
        group.mode = MarketModeV16::Recovery;
        group.recovery_reason = Some(PermissionlessRecoveryReasonV16::BelowProgressFloor);
    });
    let before = env.svm.get_account(&env.market).unwrap();
    let g_pre = env.market_state().1;

    // ATTACK: convert junior pnl -> senior capital during the wind-down.
    env.svm.expire_blockhash();
    let r = env.send(
        env.convert_released_pnl_ix(p, 1_000_000_000),
        vec![
            AccountMeta::new(o.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
        ],
        &[&o],
    );
    assert!(
        r.is_err(),
        "ConvertReleasedPnl must reject in Recovery (Live-only)"
    );

    // ROLLBACK: the junior claim stays junior; account capital/pnl and the market are unchanged.
    let post = env.portfolio_state(p);
    let g_post = env.market_state().1;
    assert_eq!(
        post.capital.get(),
        pre.capital.get(),
        "no junior->senior conversion: capital unchanged"
    );
    assert_eq!(
        post.pnl.get(),
        pre.pnl.get(),
        "junior pnl stays junior (not converted)"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before.data,
        "market state byte-for-byte unchanged"
    );
    assert_eq!(
        g_post.c_tot, g_pre.c_tot,
        "c_tot unchanged (no capital minted from junior pnl)"
    );
    assert_eq!(
        g_post.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(
        g_post.vault >= g_post.c_tot + g_post.insurance,
        "senior conservation in recovery"
    );
}

#[test]
fn v16_bpf_recovery_and_reset_tags_are_bounded_and_update_state() {
    let mut reduce_env = V16CuEnv::new();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = reduce_env.create_portfolio(&long_owner);
    let short_account = reduce_env.create_portfolio(&short_owner);
    reduce_env.deposit(&long_owner, long_account, 10_000);
    reduce_env.deposit(&short_owner, short_account, 10_000);
    reduce_env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        (2 * POS_SCALE) as i128,
        100,
        0,
    );

    let reduce_cu = reduce_env.rebalance_reduce_with_cu(&long_owner, long_account, 0, POS_SCALE);
    assert_cu_within("RebalanceReduce", reduce_cu, CUSTODY_CU_LIMIT);
    let (_, group) = reduce_env.market_state();
    let long = reduce_env.portfolio_state(long_account);
    assert_eq!(long.legs[0].basis_pos_q.get(), POS_SCALE as i128);
    assert_eq!(group.assets[0].oi_eff_long_q, POS_SCALE);

    let mut forfeit_env = V16CuEnv::new();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = forfeit_env.create_portfolio(&long_owner);
    let short_account = forfeit_env.create_portfolio(&short_owner);
    forfeit_env.deposit(&long_owner, long_account, 10_000);
    forfeit_env.deposit(&short_owner, short_account, 10_000);
    forfeit_env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        100,
        0,
    );
    forfeit_env.mutate_market(|_, group| {
        group.mode = MarketModeV16::Recovery;
        group.recovery_reason = Some(PermissionlessRecoveryReasonV16::BelowProgressFloor);
    });
    let forfeit_cu = forfeit_env.forfeit_recovery_leg_with_cu(&long_owner, long_account, 0, 1);
    assert_cu_within("ForfeitRecoveryLeg", forfeit_cu, CUSTODY_CU_LIMIT);
    let (_, group) = forfeit_env.market_state();
    let long = forfeit_env.portfolio_state(long_account);
    assert!(percolator::active_bitmap_is_empty(active_bitmap(&long)));
    assert_eq!(long.legs[0].basis_pos_q.get(), 0);
    assert_eq!(group.assets[0].oi_eff_long_q, 0);

    let mut cure_env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = cure_env.create_portfolio(&owner);
    cure_env.seed_cancellable_close_progress(portfolio);
    let source = cure_env.token_account_for_mint(cure_env.mint, owner.pubkey(), 20);
    let cure_cu = cure_env.cure_and_cancel_close_with_cu(&owner, portfolio, source, 20);
    assert_cu_within("CureAndCancelClose", cure_cu, CUSTODY_CU_LIMIT);
    let (_, group) = cure_env.market_state();
    let account = cure_env.portfolio_state(portfolio);
    assert!(close_progress(&account).canceled);
    assert_eq!(account.capital.get(), 20);
    assert_eq!(group.c_tot, 20);
    assert_eq!(group.vault, 20);
    assert_eq!(group.pending_domain_loss_barriers[0], 0);
    assert_eq!(cure_env.token_amount(source), 0);
    assert_eq!(cure_env.token_amount(cure_env.vault), 20);

    let mut reset_env = V16CuEnv::new();
    reset_env.mutate_market(|_, group| {
        group.assets[0].mode_long = SideModeV16::ResetPending;
    });
    let reset_cu = reset_env.finalize_reset_side_with_cu(0, 0);
    assert_cu_within("FinalizeResetSide", reset_cu, CUSTODY_CU_LIMIT);
    let (_, group) = reset_env.market_state();
    assert_eq!(group.assets[0].mode_long, SideModeV16::Normal);
}
