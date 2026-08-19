//! INV-078 - Permissionless recovery coverage.
//!
//! Normative obligation: Every ordinary-progress failure class retains a permissionless senior-preserving terminal route,
//! including stale-oracle expired-close recovery driven by authenticated clock state.
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

// stale price-managed oracle; otherwise the public crank loses the engine's no-DoS guarantee.
#[test]
fn v16_program_auto_crank_expired_close_recovery_not_blocked_by_stale_oracle() {
    let mut env = V16CuEnv::new();
    env.configure_permissionless_resolve_with_cu(5, 5);
    env.configure_auth_mark_with_cu(0, 100);

    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 100);
    env.seed_cancellable_close_progress(portfolio);

    env.svm.warp_to_slot(40);
    env.mutate_market(|_, group| {
        group.current_slot = 40;
    });
    let (cfg, group_before) = env.market_state();
    assert_eq!(group_before.mode, MarketModeV16::Live);
    assert!(
        oracle_v16::permissionless_stale_matured(&cfg, 40),
        "test setup must make the oracle stale enough to reject observation-bound live routes"
    );
    assert!(
        env.portfolio_state(portfolio)
            .close_progress
            .max_close_slot
            .get()
            < group_before.current_slot,
        "test setup must make the close-progress ledger expired"
    );
    let vault_before = group_before.vault;
    let c_tot_before = group_before.c_tot;
    let insurance_before = group_before.insurance;

    env.svm.expire_blockhash();
    let cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 0,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[],
        )
        .expect("expired close recovery auto-crank must not require a fresh oracle");
    assert_cu_within(
        "PermissionlessCrank expired-close recovery",
        cu,
        CRANK_CU_LIMIT,
    );

    let (_, group_after) = env.market_state();
    assert_eq!(group_after.mode, MarketModeV16::Recovery);
    assert_eq!(
        group_after.recovery_reason,
        Some(PermissionlessRecoveryReasonV16::ActiveBankruptCloseCannotProgress)
    );
    assert_eq!(
        group_after.vault, vault_before,
        "recovery declaration moves no custody"
    );
    assert_eq!(
        group_after.c_tot, c_tot_before,
        "recovery declaration mints no capital"
    );
    assert_eq!(
        group_after.insurance, insurance_before,
        "recovery declaration spends no insurance"
    );
}

// engine proves needs no oracle observation. The wrapper must not pre-block that path on a
#[test]
fn v16_program_auto_crank_expired_close_uses_authenticated_slot_not_stale_market_slot() {
    let mut env = V16CuEnv::new();
    env.configure_permissionless_resolve_with_cu(5, 5);
    env.configure_auth_mark_with_cu(0, 100);

    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 100);
    env.seed_cancellable_close_progress(portfolio);

    let (_, group_before) = env.market_state();
    assert_eq!(group_before.current_slot, 0);
    assert!(
        env.portfolio_state(portfolio)
            .close_progress
            .max_close_slot
            .get()
            > group_before.current_slot,
        "setup keeps the market slot stale enough that the old summary would not classify expiration"
    );

    env.svm.warp_to_slot(40);
    env.svm.expire_blockhash();
    let cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 40,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[],
        )
        .expect(
            "expired close recovery must use the authenticated slot, not stale market current_slot",
        );
    assert_cu_within(
        "PermissionlessCrank expired-close authenticated-slot recovery",
        cu,
        CRANK_CU_LIMIT,
    );

    let (_, group_after) = env.market_state();
    assert_eq!(group_after.mode, MarketModeV16::Recovery);
    assert_eq!(
        group_after.recovery_reason,
        Some(PermissionlessRecoveryReasonV16::ActiveBankruptCloseCannotProgress)
    );
}

#[test]
fn v16_bpf_permissionless_market_shutdown_force_closes_recovers_and_reuses_slot() {
    let mut env = V16CuEnv::new();
    let attacker = Keypair::new();
    let cranker = Keypair::new();
    let insurance_authority = Keypair::new();
    let insurance_operator = Keypair::new();
    let backing_authority = Keypair::new();
    env.svm
        .airdrop(&insurance_operator.pubkey(), 1_000_000_000)
        .unwrap();
    env.configure_permissionless_resolve_with_cu(100, 5);
    env.update_market_init_fee_policy_with_cu(25);

    env.svm.warp_to_slot(1);
    let (init_fee_source, init_cu) = env.activate_permissionless_asset_with_fee(
        &attacker,
        1,
        1,
        100,
        insurance_authority.pubkey(),
        insurance_operator.pubkey(),
        backing_authority.pubkey(),
        env.admin.pubkey(),
        25,
    );
    println!("v16 permissionless asset create BPF CU: {init_cu}");
    assert_eq!(env.token_amount(init_fee_source), 0);
    assert_eq!(env.token_amount(env.vault), 25);
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (cfg_after_create, group_after_create) = state::read_market(&market_data).unwrap();
    assert_eq!(cfg_after_create.permissionless_market_init_fee, 25);
    assert_eq!(
        group_after_create.assets[1].lifecycle,
        AssetLifecycleV16::Active
    );
    assert_eq!(group_after_create.insurance, 25);
    assert_eq!(group_after_create.vault, 25);
    assert_eq!(group_after_create.insurance_domain_budget[0], 12);
    assert_eq!(group_after_create.insurance_domain_budget[1], 13);
    let old_market_id = group_after_create.assets[1].market_id;
    let expected_created_slot = canonical_active_engine_slot(
        old_market_id,
        100,
        1,
        group_after_create.insurance_domain_budget[2],
        group_after_create.insurance_domain_budget[3],
    );
    assert_eq!(
        market_engine_slot_bytes(&market_data, 1),
        bytemuck::bytes_of(&expected_created_slot),
        "fresh permissionless asset slot must start canonical before the shutdown/reuse loop"
    );

    env.top_up_insurance_domain_with_authority(&insurance_authority, 2, 6);
    env.top_up_insurance_domain_with_authority(&insurance_authority, 3, 4);
    env.top_up_backing_bucket_with_authority(&backing_authority, 2, 20, 20);
    env.top_up_backing_bucket_with_authority(&backing_authority, 3, 25, 20);
    assert_eq!(env.token_amount(env.vault), 80);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 10_000);
    env.deposit(&short_owner, short_account, 10_000);
    env.trade_asset_with_cu(
        1,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        (2 * POS_SCALE) as i128,
        100,
        0,
    );
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, opened_group) = state::read_market(&market_data).unwrap();
    assert_eq!(opened_group.assets[1].oi_eff_long_q, 2 * POS_SCALE);
    assert_eq!(opened_group.assets[1].oi_eff_short_q, 2 * POS_SCALE);
    assert_eq!(env.token_amount(env.vault), 20_080);

    env.svm.warp_to_slot(2);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
        1,
        2,
        0,
    );
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, shutdown_group) = state::read_market(&market_data).unwrap();
    let shutdown_profile = state::read_asset_oracle_profile(&market_data, 1).unwrap();
    assert_eq!(
        shutdown_group.assets[1].lifecycle,
        AssetLifecycleV16::Recovery
    );
    assert_eq!(shutdown_profile.last_good_oracle_slot, 2);
    assert_eq!(shutdown_group.assets[1].effective_price, 100);

    env.trade_asset_with_cu(
        1,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        -(POS_SCALE as i128),
        100,
        0,
    );
    let long_data = env.svm.get_account(&long_account).unwrap().data;
    let short_data = env.svm.get_account(&short_account).unwrap().data;
    let long_after_exit_window = state::read_portfolio(&long_data).unwrap();
    let short_after_exit_window = state::read_portfolio(&short_data).unwrap();
    assert_eq!(
        active_leg_for_asset(&long_after_exit_window, 1)
            .basis_pos_q
            .unsigned_abs(),
        POS_SCALE
    );
    assert_eq!(
        active_leg_for_asset(&short_after_exit_window, 1)
            .basis_pos_q
            .unsigned_abs(),
        POS_SCALE
    );

    env.svm.warp_to_slot(6);
    let before_timeout_market = env.svm.get_account(&env.market).unwrap().data;
    let before_timeout_long = env.svm.get_account(&long_account).unwrap().data;
    let before_timeout_short = env.svm.get_account(&short_account).unwrap().data;
    let too_early = env.try_force_close_abandoned_asset_with_cu(
        &cranker,
        long_account,
        short_account,
        1,
        6,
        POS_SCALE,
    );
    assert!(
        too_early.is_err(),
        "force-close must be rejected before the shutdown timeout"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before_timeout_market
    );
    assert_eq!(
        env.svm.get_account(&long_account).unwrap().data,
        before_timeout_long
    );
    assert_eq!(
        env.svm.get_account(&short_account).unwrap().data,
        before_timeout_short
    );

    env.svm.warp_to_slot(7);
    let force_close_cu = env.force_close_abandoned_asset_with_cu(
        &cranker,
        long_account,
        short_account,
        1,
        7,
        POS_SCALE,
    );
    println!("v16 abandoned asset force close BPF CU: {force_close_cu}");
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let long_data = env.svm.get_account(&long_account).unwrap().data;
    let short_data = env.svm.get_account(&short_account).unwrap().data;
    let (_, liquidated_group) = state::read_market(&market_data).unwrap();
    let long_closed = state::read_portfolio(&long_data).unwrap();
    let short_closed = state::read_portfolio(&short_data).unwrap();
    assert_eq!(liquidated_group.assets[1].oi_eff_long_q, 0);
    assert_eq!(liquidated_group.assets[1].oi_eff_short_q, 0);
    assert!(!has_active_leg_for_asset(&long_closed, 1));
    assert!(!has_active_leg_for_asset(&short_closed, 1));

    let admin_key = env.admin.pubkey();
    let admin_recovery = env.token_account(admin_key, 0);
    for (domain, amount) in [(2u8, 6u128), (3u8, 4u128)] {
        env.withdraw_insurance_domain_to_admin_token_with_cu(admin_recovery, domain.into(), amount);
    }
    for (domain, amount) in [(2u8, 20u128), (3u8, 25u128)] {
        env.withdraw_backing_bucket_to_admin_token_with_cu(admin_recovery, domain.into(), amount);
    }
    assert_eq!(
        env.token_amount(admin_recovery),
        55,
        "admin must recover asset-domain insurance and backing funds"
    );
    assert_eq!(env.token_amount(env.vault), 20_025);

    env.top_up_insurance_from_admin_token_with_cu(admin_recovery, 10);
    env.top_up_backing_bucket_from_admin_token_with_cu(admin_recovery, 0, 45, 20);
    assert_eq!(
        env.token_amount(admin_recovery),
        0,
        "recovered funds should be re-deposited into market-0 buckets"
    );
    assert_eq!(env.token_amount(env.vault), 20_080);
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, recovered_group) = state::read_market(&market_data).unwrap();
    assert_eq!(recovered_group.insurance_domain_budget[2], 0);
    assert_eq!(recovered_group.insurance_domain_budget[3], 0);
    assert_eq!(
        recovered_group.source_backing_buckets[2].fresh_unliened_backing_num,
        0
    );
    assert_eq!(
        recovered_group.source_backing_buckets[3].fresh_unliened_backing_num,
        0
    );
    assert_eq!(recovered_group.insurance_domain_budget[0], 17);
    assert_eq!(recovered_group.insurance_domain_budget[1], 18);
    assert_eq!(
        recovered_group.source_backing_buckets[0].fresh_unliened_backing_num,
        45 * BOUND_SCALE
    );

    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_RETIRE,
        1,
        7,
        0,
    );
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (retired_cfg, retired_group) = state::read_market(&market_data).unwrap();
    assert_eq!(retired_cfg.free_market_slot_count, 1);
    assert_eq!(
        retired_group.assets[1].lifecycle,
        AssetLifecycleV16::Retired
    );
    let expected_retired_slot = canonical_retired_engine_slot(
        old_market_id,
        100,
        1,
        7,
        retired_group.insurance_domain_budget[2],
        retired_group.insurance_domain_budget[3],
    );
    assert_eq!(
        market_engine_slot_bytes(&market_data, 1),
        bytemuck::bytes_of(&expected_retired_slot),
        "retired permissionless slot must be canonical and not retain stale open-position, backing, insurance, or reservation bytes"
    );
    assert_eq!(
        changed_byte_offsets(
            bytemuck::bytes_of(&expected_created_slot),
            market_engine_slot_bytes(&market_data, 1),
        ),
        changed_byte_offsets(
            bytemuck::bytes_of(&expected_created_slot),
            bytemuck::bytes_of(&expected_retired_slot),
        ),
        "retire must change only the canonical retired-slot bytes"
    );
    let reuse_market_id = retired_group.next_market_id;
    assert!(reuse_market_id > old_market_id);

    env.svm.warp_to_slot(8);
    let (reuse_source, reuse_cu) = env.activate_permissionless_asset_with_fee(
        &attacker,
        1,
        8,
        250,
        insurance_authority.pubkey(),
        insurance_operator.pubkey(),
        backing_authority.pubkey(),
        env.admin.pubkey(),
        25,
    );
    println!("v16 permissionless asset reuse BPF CU: {reuse_cu}");
    assert_eq!(env.token_amount(reuse_source), 0);
    assert_eq!(env.token_amount(env.vault), 20_105);
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (reused_cfg, reused_group) = state::read_market(&market_data).unwrap();
    assert_eq!(reused_cfg.free_market_slot_count, 0);
    assert_eq!(reused_group.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(reused_group.assets[1].market_id, reuse_market_id);
    assert!(reused_group.assets[1].market_id > old_market_id);
    assert_eq!(reused_group.assets[1].effective_price, 250);
    let expected_reused_slot = canonical_active_engine_slot(
        reuse_market_id,
        250,
        8,
        reused_group.insurance_domain_budget[2],
        reused_group.insurance_domain_budget[3],
    );
    assert_eq!(
        market_engine_slot_bytes(&market_data, 1),
        bytemuck::bytes_of(&expected_reused_slot),
        "reused permissionless slot must be byte-identical to a canonical fresh active slot"
    );
    assert_eq!(
        changed_byte_offsets(
            bytemuck::bytes_of(&expected_retired_slot),
            market_engine_slot_bytes(&market_data, 1),
        ),
        changed_byte_offsets(
            bytemuck::bytes_of(&expected_retired_slot),
            bytemuck::bytes_of(&expected_reused_slot),
        ),
        "reuse must change only the canonical fresh-activation bytes and no stale retired-slot residue may leak"
    );
}

#[test]
fn v16_bpf_asset0_shutdown_force_closes_preserves_insurance_and_restarts() {
    let mut env = V16CuEnv::new();
    let marketauth = env.admin.insecure_clone();
    let asset_admin = Keypair::new();
    let insurance_authority = Keypair::new();
    let insurance_operator = Keypair::new();
    let backing_bucket_authority = Keypair::new();
    let new_oracle = Keypair::new();
    let cranker = Keypair::new();
    env.configure_permissionless_resolve_with_cu(100, 5);
    env.configure_auth_mark_with_cu(0, 100);
    assert_eq!(
        env.market_state().0.force_close_delay_slots,
        5,
        "auth-mark setup must preserve the force-close delay policy"
    );

    env.try_update_per_asset_authority_with_cu(
        &marketauth,
        Some(&asset_admin),
        0,
        processor::ASSET_AUTH_ADMIN,
        asset_admin.pubkey().to_bytes(),
    )
    .expect("asset-0 admin rotates to a cold key");
    env.try_update_per_asset_authority_with_cu(
        &asset_admin,
        Some(&insurance_authority),
        0,
        processor::ASSET_AUTH_INSURANCE,
        insurance_authority.pubkey().to_bytes(),
    )
    .expect("asset-0 admin rotates asset-0 insurance authority");
    env.try_update_per_asset_authority_with_cu(
        &asset_admin,
        Some(&insurance_operator),
        0,
        processor::ASSET_AUTH_INSURANCE_OPERATOR,
        insurance_operator.pubkey().to_bytes(),
    )
    .expect("asset-0 admin rotates asset-0 insurance operator");
    env.try_update_per_asset_authority_with_cu(
        &asset_admin,
        Some(&backing_bucket_authority),
        0,
        processor::ASSET_AUTH_BACKING_BUCKET,
        backing_bucket_authority.pubkey().to_bytes(),
    )
    .expect("asset-0 admin rotates asset-0 backing bucket authority");
    env.top_up_insurance_domain_with_authority(&insurance_authority, 0, 500);
    let insurance_before_shutdown = env.market_state().1.insurance;
    let vault_before_shutdown = env.token_amount(env.vault);
    assert_eq!(insurance_before_shutdown, 500);
    let asset_market_id = env.asset_market_id(0);

    env.svm.expire_blockhash();
    let ordinary_retire_asset0 = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateAssetLifecycle {
            action: processor::ASSET_ACTION_RETIRE,
            asset_index: 0,
            market_id: asset_market_id,
            now_slot: 1,
            initial_price: 0,
            max_init_fee: u128::MAX,
            insurance_authority: marketauth.pubkey().to_bytes(),
            insurance_operator: marketauth.pubkey().to_bytes(),
            backing_bucket_authority: marketauth.pubkey().to_bytes(),
            oracle_authority: marketauth.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(marketauth.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&marketauth],
    );
    assert!(
        ordinary_retire_asset0.is_err(),
        "ordinary UpdateAssetLifecycle RETIRE must not retire asset 0"
    );
    assert_eq!(
        env.market_state().1.assets[0].lifecycle,
        AssetLifecycleV16::Active
    );
    assert_eq!(env.market_state().1.insurance_domain_budget[0], 500);
    let clean_start_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, clean_start_group) = state::read_market(&clean_start_data).unwrap();
    let clean_start_asset = &clean_start_group.assets[0];
    assert_eq!(clean_start_asset.raw_oracle_target_price, 100);
    assert_eq!(clean_start_asset.effective_price, 100);
    assert_eq!(clean_start_asset.fund_px_last, 100);
    let expected_clean_start_slot = canonical_active_engine_slot(
        clean_start_asset.market_id,
        100,
        clean_start_asset.slot_last,
        clean_start_group.insurance_domain_budget[0],
        clean_start_group.insurance_domain_budget[1],
    );
    assert_eq!(
        market_engine_slot_bytes(&clean_start_data, 0),
        bytemuck::bytes_of(&expected_clean_start_slot),
        "pre-shutdown asset-0 engine slot must start canonical before the lifecycle loop"
    );

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 10_000);
    env.deposit(&short_owner, short_account, 10_000);
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        100,
        0,
    );
    let old_market_id = env.market_state().1.assets[0].market_id;

    env.svm.warp_to_slot(2);
    env.svm.expire_blockhash();
    let stranger = Keypair::new();
    env.ensure_signer_account(stranger.pubkey());
    let before_stranger_shutdown = env.svm.get_account(&env.market).unwrap().data;
    let stranger_shutdown = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateAssetLifecycle {
            action: processor::ASSET_ACTION_SHUTDOWN,
            asset_index: 0,
            market_id: old_market_id,
            now_slot: 2,
            initial_price: 0,
            max_init_fee: u128::MAX,
            insurance_authority: stranger.pubkey().to_bytes(),
            insurance_operator: stranger.pubkey().to_bytes(),
            backing_bucket_authority: stranger.pubkey().to_bytes(),
            oracle_authority: stranger.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(stranger.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&stranger],
    );
    assert!(
        stranger_shutdown.is_err(),
        "asset shutdown must require either marketauth or the local asset_admin"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before_stranger_shutdown,
        "rejected asset-0 shutdown by stranger must leave market bytes unchanged"
    );

    env.update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_SHUTDOWN, 0, 2, 0);
    let (_, shutdown_group) = env.market_state();
    let shutdown_profile =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap();
    assert_eq!(
        shutdown_group.assets[0].lifecycle,
        AssetLifecycleV16::Recovery
    );
    assert_eq!(shutdown_group.assets[0].effective_price, 100);
    assert_eq!(shutdown_profile.last_good_oracle_slot, 2);
    assert_eq!(
        shutdown_profile.asset_admin,
        asset_admin.pubkey().to_bytes()
    );
    assert_eq!(
        shutdown_profile.insurance_authority,
        insurance_authority.pubkey().to_bytes()
    );
    assert_eq!(
        shutdown_profile.insurance_operator,
        insurance_operator.pubkey().to_bytes()
    );
    assert_eq!(
        shutdown_profile.backing_bucket_authority,
        backing_bucket_authority.pubkey().to_bytes()
    );

    env.svm.expire_blockhash();
    let restart_with_positions = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::RestartAssetOracle {
            market_id: 0,
            asset_index: 0,
            now_slot: 3,
            initial_price: 250,
            observation_sequence: 2,
        },
        vec![
            AccountMeta::new(marketauth.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&marketauth],
    );
    assert!(
        restart_with_positions.is_err(),
        "asset-0 restart must reject until every asset-0 position is closed"
    );

    env.svm.warp_to_slot(6);
    let early = env.try_force_close_abandoned_asset_with_cu(
        &cranker,
        long_account,
        short_account,
        0,
        6,
        POS_SCALE,
    );
    assert!(
        early.is_err(),
        "asset-0 force-close must respect the same shutdown timeout"
    );

    env.svm.warp_to_slot(7);
    env.force_close_abandoned_asset_with_cu(&cranker, long_account, short_account, 0, 7, POS_SCALE);
    let (_, force_closed_group) = env.market_state();
    assert_eq!(force_closed_group.assets[0].oi_eff_long_q, 0);
    assert_eq!(force_closed_group.assets[0].oi_eff_short_q, 0);
    assert!(!has_active_leg_for_asset(
        &env.portfolio_state(long_account),
        0
    ));
    assert!(!has_active_leg_for_asset(
        &env.portfolio_state(short_account),
        0
    ));
    assert_eq!(
        force_closed_group.insurance_domain_budget[0], 500,
        "asset-0 shutdown keeps its insurance budget in place"
    );
    assert_eq!(force_closed_group.insurance, insurance_before_shutdown);
    assert_eq!(env.token_amount(env.vault), vault_before_shutdown + 20_000);

    assert!(
        env.try_withdraw_insurance_domain_with_authority(&marketauth, 0, 100)
            .is_err(),
        "marketauth cannot use asset-0 shutdown as an insurance drain bypass"
    );
    assert_eq!(env.market_state().1.insurance_domain_budget[0], 500);

    env.try_update_per_asset_authority_with_cu(
        &asset_admin,
        Some(&new_oracle),
        0,
        processor::ASSET_AUTH_ORACLE,
        new_oracle.pubkey().to_bytes(),
    )
    .expect("asset-0 admin rotates oracle before restart");
    env.svm.warp_to_slot(8);
    env.svm.expire_blockhash();
    let restart_by_marketauth = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::RestartAssetOracle {
            market_id: 0,
            asset_index: 0,
            now_slot: 8,
            initial_price: 250,
            observation_sequence: 2,
        },
        vec![
            AccountMeta::new(marketauth.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&marketauth],
    );
    assert!(
        restart_by_marketauth.is_err(),
        "marketauth may force-shutdown asset 0 but cannot restart after asset_admin is rotated away"
    );
    env.svm.expire_blockhash();
    let restart = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::RestartAssetOracle {
            market_id: 0,
            asset_index: 0,
            now_slot: 8,
            initial_price: 250,
            observation_sequence: 2,
        },
        vec![
            AccountMeta::new(asset_admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&asset_admin],
    );
    assert!(
        restart.is_ok(),
        "local asset_admin restarts empty asset 0: {restart:?}"
    );
    let restarted_data = env.svm.get_account(&env.market).unwrap().data;
    let (restarted_cfg, restarted_group) = state::read_market(&restarted_data).unwrap();
    let restarted_profile = state::read_asset_oracle_profile(&restarted_data, 0).unwrap();
    assert_eq!(
        restarted_group.assets[0].lifecycle,
        AssetLifecycleV16::Active
    );
    assert_eq!(restarted_group.assets[0].effective_price, 250);
    assert_ne!(restarted_group.assets[0].market_id, old_market_id);
    let expected_asset0_slot = canonical_active_engine_slot(
        restarted_group.assets[0].market_id,
        250,
        8,
        restarted_group.insurance_domain_budget[0],
        restarted_group.insurance_domain_budget[1],
    );
    assert_eq!(
        market_engine_slot_bytes(&restarted_data, 0),
        bytemuck::bytes_of(&expected_asset0_slot),
        "after RestartAssetOracle, the raw asset-0 engine slot bytes must match a canonical fresh active slot with only market_id/price/slot and preserved insurance budgets set"
    );
    let actual_changed_offsets = changed_byte_offsets(
        market_engine_slot_bytes(&clean_start_data, 0),
        market_engine_slot_bytes(&restarted_data, 0),
    );
    let expected_changed_offsets = changed_byte_offsets(
        bytemuck::bytes_of(&expected_clean_start_slot),
        bytemuck::bytes_of(&expected_asset0_slot),
    );
    assert_eq!(
        actual_changed_offsets, expected_changed_offsets,
        "shutdown/force-close/restart must change exactly the canonical asset-0 engine-slot bytes and no hidden engine state"
    );
    assert_eq!(
        restarted_group.insurance_domain_budget[0], 500,
        "asset-0 restart preserves the funded insurance domain"
    );
    assert_eq!(restarted_group.insurance, insurance_before_shutdown);
    assert_eq!(
        restarted_profile.asset_admin,
        asset_admin.pubkey().to_bytes()
    );
    assert_eq!(
        restarted_profile.insurance_authority,
        insurance_authority.pubkey().to_bytes()
    );
    assert_eq!(
        restarted_profile.insurance_operator,
        insurance_operator.pubkey().to_bytes()
    );
    assert_eq!(
        restarted_profile.backing_bucket_authority,
        backing_bucket_authority.pubkey().to_bytes()
    );
    assert_eq!(
        restarted_profile.oracle_authority,
        new_oracle.pubkey().to_bytes(),
        "restart must preserve the oracle rotation from UpdateAssetAuthority"
    );
    assert_eq!(restarted_cfg.mark_ewma_e6, 250);
    assert_eq!(restarted_cfg.oracle_target_price_e6, 250);

    env.svm.expire_blockhash();
    let old_oracle_reconfigure = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 8,
            initial_mark_e6: 250,
        },
        vec![
            AccountMeta::new(marketauth.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&marketauth],
    );
    assert!(
        old_oracle_reconfigure.is_err(),
        "old marketauth key is no longer asset-0 oracle authority"
    );
    env.configure_auth_mark_for_asset_with_authority(0, &new_oracle, 8, 250);
    env.svm.warp_to_slot(9);
    env.push_auth_mark_for_asset_with_authority(0, &new_oracle, 9, 260);

    env.trade_asset_with_cu(
        0,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        260,
        0,
    );
    let long_after_restart = env.portfolio_state(long_account);
    let restarted_leg = active_leg_for_asset(&long_after_restart, 0);
    assert_eq!(
        restarted_leg.market_id,
        env.market_state().1.assets[0].market_id,
        "new asset-0 trades bind to the restarted market id"
    );
    assert_eq!(
        env.market_state().1.assets[0].lifecycle,
        AssetLifecycleV16::Active
    );
}

// LIVENESS: no trader can block marketauth's asset cleanup. After force_close_delay_slots, the
// permissionless force-close winds down a retired asset by netting a long against a short at the
// frozen mark. A griefer who sits on a maximally-complex (14-leg) STALE portfolio cannot stall this:
// a one-shot force-close of two such accounts would exceed the 1.4M tx CU limit (it must settle all
// 28 stale legs inline before netting), but the permissionless PermissionlessCrank Refresh settles
// each account's stale legs in its own bounded tx first, after which the force-close of the now-fresh
// pair fits comfortably. Cleanup is therefore reachable in bounded, permissionless steps regardless
// of how stale or how many legs the griefer holds.
#[test]
fn v16_bpf_force_close_liveness_survives_14_stale_leg_grief_via_precrank() {
    // (A) one-shot force-close on two fully-stale 14-leg accounts: too heavy for a single tx.
    {
        let mut env = V16CuEnv::new_with_market_params_and_price_move(14, 1_000, 1_000, 500);
        env.configure_permissionless_resolve_with_cu(100, 5);
        let lo = Keypair::new();
        let so = Keypair::new();
        let pa = env.create_portfolio(&lo);
        let pb = env.create_portfolio(&so);
        env.deposit(&lo, pa, 1_000_000);
        env.deposit(&so, pb, 1_000_000);
        env.seed_n_leg_position_for_benchmark(pa, pb, 14);
        env.svm.warp_to_slot(16);
        env.update_asset_lifecycle_as_admin_with_cu(
            percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
            13,
            16,
            0,
        );
        env.svm.warp_to_slot(22);
        env.svm.expire_blockhash();
        let cranker = Keypair::new();
        let one_shot = env.try_force_close_abandoned_asset_with_cu(
            &cranker,
            pa,
            pb,
            13,
            22,
            (10 * POS_SCALE) as u128,
        );
        assert!(
            one_shot.is_err(),
            "one-shot force-close of two 14-stale-leg accounts is expected to exceed the tx CU \
             limit; cleanup must go through the pre-crank path"
        );
    }

    // (B) the supported liveness path: permissionless Refresh settles each account, then force-close.
    let mut env = V16CuEnv::new_with_market_params_and_price_move(14, 1_000, 1_000, 500);
    env.configure_permissionless_resolve_with_cu(100, 5);
    let lo = Keypair::new();
    let so = Keypair::new();
    let pa = env.create_portfolio(&lo);
    let pb = env.create_portfolio(&so);
    env.deposit(&lo, pa, 1_000_000);
    env.deposit(&so, pb, 1_000_000);
    env.seed_n_leg_position_for_benchmark(pa, pb, 14);
    env.svm.warp_to_slot(16);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
        13,
        16,
        0,
    );
    env.svm.warp_to_slot(22);
    let refresh = |slot: u64| ProgInstruction::PermissionlessCrank {
        now_slot: slot,
        observations: crank_observations(0),
    };
    let c_long = env.crank_steps_after_market_catchup(pa, refresh(22), 1);
    let c_short = env.crank_steps_after_market_catchup(pb, refresh(22), 1);
    assert!(
        c_long < 1_400_000 && c_short < 1_400_000,
        "each permissionless Refresh fits a tx: long={c_long} short={c_short}"
    );
    env.svm.expire_blockhash();
    let cranker = Keypair::new();
    let fc = env
        .try_force_close_abandoned_asset_with_cu(&cranker, pa, pb, 13, 22, (10 * POS_SCALE) as u128)
        .expect("force-close of the refreshed pair must succeed (permissionless liveness)");
    println!("v16 force-close-after-precrank worst-case CU: {fc}");
    assert!(
        fc < 1_400_000,
        "force-close of the refreshed 14-leg pair must fit the tx CU limit: {fc}"
    );
    // the shut-down asset's book is wound down to zero on both sides.
    let g = env.market_state().1;
    assert_eq!(g.assets[13].oi_eff_long_q, 0);
    assert_eq!(g.assets[13].oi_eff_short_q, 0);
}
