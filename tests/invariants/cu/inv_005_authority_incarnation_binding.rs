//! INV-005 - Authority incarnation binding.
//!
//! Normative obligation: Retained authority is scoped to the configured role and cannot be
//! exercised by an untrusted public caller.
//!
//! Evidence in this file (public SBF I plus exact rollback):
//! `v16_program_privileged_policy_boundary_matrix_rejects_untrusted_callers` submits the two
//! authority-only instruction families implicated by privileged deadline and insurance claims.
//! It requires both to reject before changing the market, SPL vault, or attacker destination.
//! `v16_program_authority_handoffs_share_one_incoming_key_validator` source-locks both authority
//! handoff handlers to one validator: the market authority cannot be burned, while the asset-admin
//! role retains its explicitly authorized burn path.
//!
//! Guarantee boundary: this proves the alleged transition is not an unprivileged public attack.
//! It does not protect users from a compromised configured authority; operational deployments
//! must place that role behind their chosen multisignature or governance policy.

use super::*;

#[test]
fn v16_program_authority_handoffs_share_one_incoming_key_validator() {
    let production = include_str!("../../../src/v16_program.rs");

    assert_eq!(
        production.matches("expect_incoming_authority(").count(),
        3,
        "the production surface must contain one validator definition and exactly two handoff calls"
    );

    let market_start = production
        .find("fn handle_update_authority<'a>(")
        .expect("market-authority handler remains mounted");
    let asset_start = production
        .find("fn handle_update_asset_authority<'a>(")
        .expect("asset-authority handler remains mounted");
    let market_handler = &production[market_start..asset_start];
    assert!(
        market_handler.contains("expect_incoming_authority(new_authority, &new_pubkey, false)?;"),
        "market-authority handoff must use the shared validator with burn disabled"
    );

    let asset_end = production[asset_start..]
        .find("fn handle_update_base_unit_mints<'a>(")
        .map(|offset| asset_start + offset)
        .expect("next handler remains mounted");
    let asset_handler = &production[asset_start..asset_end];
    assert!(
        asset_handler.contains("expect_incoming_authority(new_authority, &new_pubkey, true)?;"),
        "asset-authority handoff must use the shared validator with its explicit burn policy"
    );

    let validator_start = production
        .find("fn expect_incoming_authority(")
        .expect("shared incoming-authority validator remains mounted");
    let validator_end = production[validator_start..]
        .find("\n    fn live_authority_matches(")
        .map(|offset| validator_start + offset)
        .unwrap_or(production.len());
    let validator = &production[validator_start..validator_end];
    assert!(validator.contains("expect_signer(authority)?;"));
    assert!(validator.contains("authority.key.to_bytes() != *new_pubkey"));
}

#[test]
fn v16_program_privileged_policy_boundary_matrix_rejects_untrusted_callers() {
    let mut env = V16CuEnv::new();
    env.top_up_insurance(1_000);

    let attacker = Keypair::new();
    env.ensure_signer_account(attacker.pubkey());
    let attacker_dest = env.token_account(attacker.pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let destination_before = env.svm.get_account(&attacker_dest).unwrap();

    env.svm.expire_blockhash();
    let withdrawal = env.send(
        ProgInstruction::WithdrawInsuranceAsset {
            market_id: 0,
            asset_index: 0,
            amount: 1,
        },
        vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(attacker_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&attacker],
    );
    assert!(
        withdrawal.is_err(),
        "an untrusted caller must not exercise the insurance-operator route"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    assert_eq!(
        env.svm.get_account(&attacker_dest).unwrap(),
        destination_before
    );

    env.svm.expire_blockhash();
    let policy = env.send(
        ProgInstruction::ConfigurePermissionlessResolve {
            asset_generation_frontier: 0,
            policy_sequence: u64::MAX,
            stale_slots: 1,
            force_close_delay_slots: 1,
        },
        vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&attacker],
    );
    assert!(
        policy.is_err(),
        "an untrusted caller must not rewrite stale or recovery deadlines"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    assert_eq!(
        env.svm.get_account(&attacker_dest).unwrap(),
        destination_before
    );
    assert_eq!(env.token_amount(attacker_dest), 0);
}

#[test]
fn v16_attack_privileged_reactivate_rekeys_retired_slot_authorities() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let old_creator = Keypair::new();
    let new_insurance = Keypair::new();
    let new_operator = Keypair::new();
    let new_backing = Keypair::new();
    let new_oracle = Keypair::new();
    env.update_market_init_fee_policy_with_cu(1);

    env.svm.warp_to_slot(1);
    env.activate_permissionless_asset_with_fee(
        &old_creator,
        1,
        1,
        100,
        old_creator.pubkey(),
        old_creator.pubkey(),
        old_creator.pubkey(),
        old_creator.pubkey(),
        1,
    );
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let old_profile = state::read_asset_oracle_profile(&market_data, 1).unwrap();
    assert_eq!(
        old_profile.oracle_authority,
        old_creator.pubkey().to_bytes(),
        "setup: old permissionless creator owns the retired slot before reuse"
    );

    env.svm.warp_to_slot(3);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_RETIRE,
        1,
        3,
        0,
    );

    env.svm.warp_to_slot(4);
    env.svm.expire_blockhash();
    let activation_market_id = env.market_state().1.next_market_id;
    env.send(
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
            market_id: activation_market_id,
            now_slot: 4,
            initial_price: 250,
            max_init_fee: u128::MAX,
            insurance_authority: new_insurance.pubkey().to_bytes(),
            insurance_operator: new_operator.pubkey().to_bytes(),
            backing_bucket_authority: new_backing.pubkey().to_bytes(),
            oracle_authority: new_oracle.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    )
    .expect("admin reactivates the retired slot with fresh authorities");

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let reused_profile = state::read_asset_oracle_profile(&market_data, 1).unwrap();
    assert_eq!(
        reused_profile.asset_admin,
        admin.pubkey().to_bytes(),
        "admin reactivation must bootstrap asset_admin to the current activator"
    );
    assert_eq!(
        reused_profile.insurance_authority,
        new_insurance.pubkey().to_bytes(),
        "admin reactivation must install the new insurance authority"
    );
    assert_eq!(
        reused_profile.insurance_operator,
        new_operator.pubkey().to_bytes(),
        "admin reactivation must install the new insurance operator"
    );
    assert_eq!(
        reused_profile.backing_bucket_authority,
        new_backing.pubkey().to_bytes(),
        "admin reactivation must install the new backing authority"
    );
    assert_eq!(
        reused_profile.oracle_authority,
        new_oracle.pubkey().to_bytes(),
        "admin reactivation must not leave the old creator in oracle control"
    );

    env.svm.warp_to_slot(5);
    let market_before = env.svm.get_account(&env.market).unwrap();
    env.svm.expire_blockhash();
    let old_oracle_reconfig = env.send(
        ProgInstruction::ConfigureAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 1,
            now_slot: 5,
            initial_mark_e6: 300,
        },
        vec![
            AccountMeta::new(old_creator.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&old_creator],
    );
    assert!(
        old_oracle_reconfig.is_err(),
        "old permissionless creator must not retain oracle control over the reused market slot"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected stale-authority oracle reconfig must not mutate the reused market"
    );

    env.ensure_signer_account(new_oracle.pubkey());
    env.svm.expire_blockhash();
    let new_oracle_reconfig = env.send(
        ProgInstruction::ConfigureAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 1,
            now_slot: 5,
            initial_mark_e6: 300,
        },
        vec![
            AccountMeta::new(new_oracle.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&new_oracle],
    );
    assert!(
        new_oracle_reconfig.is_ok(),
        "new oracle authority must control the reused market slot: {new_oracle_reconfig:?}"
    );
}

#[test]
fn v16_attack_retired_asset_domain_authority_cannot_refund_slot_and_block_reuse() {
    let mut env = V16CuEnv::new();
    env.update_market_init_fee_policy_with_cu(1);
    let old_creator = Keypair::new();
    let rekeyed_insurance_authority = Keypair::new();
    let rekeyed_backing_authority = Keypair::new();
    env.svm.warp_to_slot(1);
    env.activate_permissionless_asset_with_fee(
        &old_creator,
        1,
        1,
        100,
        old_creator.pubkey(),
        old_creator.pubkey(),
        old_creator.pubkey(),
        old_creator.pubkey(),
        1,
    );

    env.svm.warp_to_slot(3);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_RETIRE,
        1,
        3,
        0,
    );
    let (retired_cfg, retired_group) = env.market_state();
    assert_eq!(retired_cfg.free_market_slot_count, 1);
    assert_eq!(
        retired_group.assets[1].lifecycle,
        AssetLifecycleV16::Retired
    );

    env.try_update_per_asset_authority_with_cu(
        &old_creator,
        Some(&rekeyed_insurance_authority),
        1,
        processor::ASSET_AUTH_INSURANCE,
        rekeyed_insurance_authority.pubkey().to_bytes(),
    )
    .expect("retired insurance authority can self-rotate but must not reactivate the domain");
    env.try_update_per_asset_authority_with_cu(
        &old_creator,
        Some(&rekeyed_backing_authority),
        1,
        processor::ASSET_AUTH_BACKING_BUCKET,
        rekeyed_backing_authority.pubkey().to_bytes(),
    )
    .expect("retired backing authority can self-rotate but must not reactivate the domain");

    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let insurance_source = env.token_account(rekeyed_insurance_authority.pubkey(), 77);
    let insurance_source_before = env.svm.get_account(&insurance_source).unwrap();
    env.svm.expire_blockhash();
    let insurance_topup = env.send(
        ProgInstruction::TopUpInsuranceDomain {
            intent_id: 0,
            market_id: 0,
            domain: 2,
            amount: 77,
        },
        vec![
            AccountMeta::new(rekeyed_insurance_authority.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(insurance_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&rekeyed_insurance_authority],
    );
    assert!(
        insurance_topup.is_err(),
        "rekeyed retired-slot insurance authority must not be able to refund an inactive domain"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    assert_eq!(
        env.svm.get_account(&insurance_source).unwrap(),
        insurance_source_before
    );

    let backing_source = env.token_account(rekeyed_backing_authority.pubkey(), 88);
    let backing_source_before = env.svm.get_account(&backing_source).unwrap();
    env.svm.expire_blockhash();
    let backing_topup = env.send(
        ProgInstruction::TopUpBackingBucket {
            intent_id: 0,
            market_id: 0,
            domain: 2,
            amount: 88,
            expiry_slot: 10,
        },
        vec![
            AccountMeta::new(rekeyed_backing_authority.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(backing_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&rekeyed_backing_authority],
    );
    assert!(
        backing_topup.is_err(),
        "rekeyed retired-slot backing authority must not be able to refund an inactive domain"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    assert_eq!(
        env.svm.get_account(&backing_source).unwrap(),
        backing_source_before
    );

    let new_creator = Keypair::new();
    env.svm.warp_to_slot(4);
    env.activate_permissionless_asset_with_fee(
        &new_creator,
        1,
        4,
        250,
        new_creator.pubkey(),
        new_creator.pubkey(),
        new_creator.pubkey(),
        new_creator.pubkey(),
        1,
    );
    let (reused_cfg, reused_group) = env.market_state();
    assert_eq!(reused_cfg.free_market_slot_count, 0);
    assert_eq!(reused_group.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(reused_group.assets[1].effective_price, 250);
    let reused_profile =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 1)
            .unwrap();
    assert_eq!(
        reused_profile.insurance_authority,
        new_creator.pubkey().to_bytes(),
        "reused slot must overwrite any rekeyed retired insurance authority"
    );
    assert_eq!(
        reused_profile.backing_bucket_authority,
        new_creator.pubkey().to_bytes(),
        "reused slot must overwrite any rekeyed retired backing authority"
    );
}

// security.md sweep - RestartAssetOracle cross-asset admin isolation (#6/#37/#48): restart rewrites an
// empty RECOVERY asset back to ACTIVE with a fresh market id and price. A key that is a valid admin for
// asset 1 must not be able to restart asset 0 at an attacker-selected price.
#[test]
fn v16_attack_cross_asset_admin_cannot_restart_other_asset_oracle() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let asset1_admin = Keypair::new();
    env.configure_permissionless_resolve_with_cu(100, 5);
    env.configure_auth_mark_with_cu(0, 100);
    env.activate_asset(1, 2, 100);
    env.try_update_per_asset_authority_with_cu(
        &admin,
        Some(&asset1_admin),
        1,
        processor::ASSET_AUTH_ADMIN,
        asset1_admin.pubkey().to_bytes(),
    )
    .expect("rotate asset-1 admin to a distinct key");

    env.svm.warp_to_slot(3);
    env.update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_SHUTDOWN, 0, 3, 0);
    env.svm.warp_to_slot(4);
    env.try_shutdown_asset_with_authority(&asset1_admin, 1, 4)
        .expect("asset-1 admin shuts down its own asset");
    let (_, recovery_group) = env.market_state();
    assert_eq!(
        recovery_group.assets[0].lifecycle,
        AssetLifecycleV16::Recovery
    );
    assert_eq!(
        recovery_group.assets[1].lifecycle,
        AssetLifecycleV16::Recovery
    );

    let market_before = env.svm.get_account(&env.market).unwrap();
    env.svm.warp_to_slot(5);
    env.svm.expire_blockhash();
    let rejected = env.try_restart_asset_oracle_with_authority(&asset1_admin, 0, 5, 777);
    assert!(
        rejected.is_err(),
        "asset-1 admin must not restart asset-0's recovery oracle"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected cross-asset restart leaves market bytes unchanged"
    );
    assert_eq!(
        env.market_state().1.assets[0].lifecycle,
        AssetLifecycleV16::Recovery,
        "asset-0 remains in recovery after rejected cross-asset restart"
    );

    env.svm.expire_blockhash();
    env.try_restart_asset_oracle_with_authority(&asset1_admin, 1, 5, 250)
        .expect("asset-1 admin restarts its own recovery asset");
    let data = env.svm.get_account(&env.market).unwrap().data;
    let (_, after) = state::read_market(&data).unwrap();
    let asset1_profile = state::read_asset_oracle_profile(&data, 1).unwrap();
    assert_eq!(after.assets[0].lifecycle, AssetLifecycleV16::Recovery);
    assert_eq!(after.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(after.assets[1].effective_price, 250);
    assert_eq!(asset1_profile.asset_admin, asset1_admin.pubkey().to_bytes());
}

// security.md sweep — UpdateAuthority new-authority binding (#6): setting an authority to a non-zero
// key requires THAT key to co-sign (handle_update_authority: expect_signer(new_authority) + key match).
// Otherwise an admin (or attacker) could assign an authority to a key nobody controls (griefing/brick).
#[test]
fn v16_attack_update_authority_requires_new_authority_signature() {
    let mut env = V16CuEnv::new();
    let victim = Keypair::new(); // a key that will NOT sign
    let (cfg0, _) = env.market_state();
    // --- market-wide handler (single `marketauth` key) ---
    // marketauth tries to set itself to `victim` without victim signing -> reject.
    env.svm.expire_blockhash();
    let r = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateAuthority {
            new_pubkey: victim.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new_readonly(victim.pubkey(), false),
            AccountMeta::new(env.market, false),
        ],
        &[&env.admin],
    );
    assert!(
        r.is_err(),
        "setting the market authority to a non-signing key must reject"
    );
    let (cfg1, _) = env.market_state();
    assert_eq!(
        cfg1.marketauth, cfg0.marketauth,
        "market authority unchanged by the rejected update"
    );

    // with the new authority co-signing, the update succeeds.
    let new_asset = Keypair::new();
    env.ensure_signer_account(new_asset.pubkey());
    env.svm.expire_blockhash();
    let r_ok = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateAuthority {
            new_pubkey: new_asset.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(new_asset.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&env.admin, &new_asset],
    );
    assert!(
        r_ok.is_ok(),
        "co-signed authority update succeeds: {:?}",
        r_ok
    );
    assert_eq!(
        env.market_state().0.marketauth,
        new_asset.pubkey().to_bytes(),
        "market authority updated to the co-signing key"
    );
    assert_eq!(
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap()
            .asset_admin,
        new_asset.pubkey().to_bytes(),
        "default asset-0 admin follows the co-signed market authority handoff"
    );
    env.svm.expire_blockhash();
    let stale_old_key = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateAuthority {
            new_pubkey: env.admin.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&env.admin],
    );
    assert!(
        stale_old_key.is_err(),
        "the previous market authority must lose rotation power after handoff"
    );
    assert_eq!(
        env.market_state().0.marketauth,
        new_asset.pubkey().to_bytes(),
        "old authority replay did not rotate marketauth back",
    );

    // --- per-asset handler for ASSET 0 (insurance authority now rotates via UpdateAssetAuthority) ---
    let prof0 = |env: &V16CuEnv| {
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap()
    };
    let ins_before = prof0(&env).insurance_authority;
    let asset_market_id = env.asset_market_id(0);
    // The current asset-0 admin tries to set asset-0 insurance to a non-signing key -> reject.
    env.svm.expire_blockhash();
    let r2 = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateAssetAuthority {
            asset_index: 0,
            market_id: asset_market_id,
            kind: processor::ASSET_AUTH_INSURANCE,
            new_pubkey: victim.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(new_asset.pubkey(), true),
            AccountMeta::new_readonly(victim.pubkey(), false),
            AccountMeta::new(env.market, false),
        ],
        &[&new_asset],
    );
    assert!(
        r2.is_err(),
        "asset-0 insurance rotation to a non-signing key must reject"
    );
    assert_eq!(
        prof0(&env).insurance_authority,
        ins_before,
        "asset-0 insurance unchanged by the rejected update"
    );
    // with the new authority co-signing, the asset-0 rotation succeeds.
    let new_ins = Keypair::new();
    env.ensure_signer_account(new_ins.pubkey());
    env.svm.expire_blockhash();
    let r2_ok = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateAssetAuthority {
            asset_index: 0,
            market_id: asset_market_id,
            kind: processor::ASSET_AUTH_INSURANCE,
            new_pubkey: new_ins.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(new_asset.pubkey(), true),
            AccountMeta::new(new_ins.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&new_asset, &new_ins],
    );
    assert!(
        r2_ok.is_ok(),
        "co-signed asset-0 insurance rotation succeeds: {:?}",
        r2_ok
    );
    assert_eq!(
        prof0(&env).insurance_authority,
        new_ins.pubkey().to_bytes(),
        "asset-0 insurance rotated to the co-signing key"
    );
}

// security.md sweep - stale marketauth policy replay (#6/#33): after marketauth is handed off, the
// previous key must lose operational policy power, not just rotation power. Otherwise a stale admin
// could later grief reward shares, fee redirects, permissionless-create cost, or stale-resolve timing.
#[test]
fn v16_attack_rotated_marketauth_cannot_replay_policy_updates() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 10_000, 10_000, 10_000, 58,
    );
    let old_admin = env.admin.insecure_clone();
    let new_admin = Keypair::new();
    env.update_asset_authority_with_cu(&new_admin);
    assert_eq!(
        env.market_state().0.marketauth,
        new_admin.pubkey().to_bytes(),
        "test setup rotates marketauth away from the old key"
    );
    let cfg_after_rotation = env.market_state().0;

    let mut old_attempt = |ix: ProgInstruction, label: &str| {
        env.svm.expire_blockhash();
        let rejected = send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ix,
            vec![
                AccountMeta::new(old_admin.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&old_admin],
        );
        assert!(
            rejected.is_err(),
            "{label} must reject for stale marketauth"
        );
        let cfg = env.market_state().0;
        assert_eq!(cfg.marketauth, cfg_after_rotation.marketauth);
        assert_eq!(
            cfg.liquidation_cranker_fee_share_bps,
            cfg_after_rotation.liquidation_cranker_fee_share_bps
        );
        assert_eq!(
            cfg.maintenance_cranker_fee_share_bps,
            cfg_after_rotation.maintenance_cranker_fee_share_bps
        );
        assert_eq!(
            cfg.fee_redirect_to_market_0_bps,
            cfg_after_rotation.fee_redirect_to_market_0_bps
        );
        assert_eq!(
            cfg.permissionless_market_init_fee,
            cfg_after_rotation.permissionless_market_init_fee
        );
        assert_eq!(
            cfg.permissionless_resolve_stale_slots,
            cfg_after_rotation.permissionless_resolve_stale_slots
        );
        assert_eq!(
            cfg.force_close_delay_slots,
            cfg_after_rotation.force_close_delay_slots
        );
    };

    old_attempt(
        ProgInstruction::UpdateLiquidationFeePolicy {
            policy_sequence: u64::MAX,
            cranker_share_bps: 5_000,
        },
        "liquidation policy replay",
    );
    old_attempt(
        ProgInstruction::UpdateMaintenanceFeePolicy {
            policy_sequence: u64::MAX,
            cranker_share_bps: 4_000,
        },
        "maintenance policy replay",
    );
    old_attempt(
        ProgInstruction::UpdateFeeRedirectPolicy {
            policy_sequence: u64::MAX,
            redirect_bps: 2_000,
        },
        "fee redirect replay",
    );
    old_attempt(
        ProgInstruction::UpdateMarketInitFeePolicy {
            min_init_fee: 40,
            policy_sequence: u64::MAX,
        },
        "permissionless init-fee replay",
    );
    old_attempt(
        ProgInstruction::ConfigurePermissionlessResolve {
            asset_generation_frontier: 0,
            policy_sequence: u64::MAX,
            stale_slots: 100,
            force_close_delay_slots: 5,
        },
        "permissionless resolve policy replay",
    );

    let mut new_update = |ix: ProgInstruction, label: &str| {
        env.svm.expire_blockhash();
        send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ix,
            vec![
                AccountMeta::new(new_admin.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&new_admin],
        )
        .unwrap_or_else(|err| panic!("{label} by new marketauth must succeed: {err}"));
    };

    new_update(
        ProgInstruction::UpdateLiquidationFeePolicy {
            policy_sequence: u64::MAX,
            cranker_share_bps: 5_000,
        },
        "liquidation policy update",
    );
    new_update(
        ProgInstruction::UpdateMaintenanceFeePolicy {
            policy_sequence: u64::MAX,
            cranker_share_bps: 4_000,
        },
        "maintenance policy update",
    );
    new_update(
        ProgInstruction::UpdateFeeRedirectPolicy {
            policy_sequence: u64::MAX,
            redirect_bps: 2_000,
        },
        "fee redirect update",
    );
    new_update(
        ProgInstruction::UpdateMarketInitFeePolicy {
            min_init_fee: 40,
            policy_sequence: u64::MAX,
        },
        "permissionless init-fee update",
    );
    new_update(
        ProgInstruction::ConfigurePermissionlessResolve {
            asset_generation_frontier: 0,
            policy_sequence: u64::MAX,
            stale_slots: 100,
            force_close_delay_slots: 5,
        },
        "permissionless resolve policy update",
    );
    let cfg = env.market_state().0;
    assert_eq!(cfg.liquidation_cranker_fee_share_bps, 5_000);
    assert_eq!(cfg.maintenance_cranker_fee_share_bps, 4_000);
    assert_eq!(cfg.fee_redirect_to_market_0_bps, 2_000);
    assert_eq!(cfg.permissionless_market_init_fee, 40);
    assert_eq!(cfg.permissionless_resolve_stale_slots, 100);
    assert_eq!(cfg.force_close_delay_slots, 5);

    let payer_owner = Keypair::new();
    let cranker_owner = Keypair::new();
    let payer_portfolio = env.create_portfolio(&payer_owner);
    let cranker_portfolio = env.create_portfolio(&cranker_owner);
    env.deposit(&payer_owner, payer_portfolio, 100_000_000);
    env.svm.warp_to_slot(10);
    env.sync_maintenance_fee_with_cu(payer_portfolio, Some(cranker_portfolio), 10);
    let cranker = env.portfolio_state(cranker_portfolio);
    assert_eq!(
        cranker.capital.get(),
        232,
        "new marketauth's maintenance share pays the public cranker path"
    );
}

// security.md sweep — WithdrawInsuranceAsset operator authorization (#6): a per-asset insurance
// withdrawal must be signed by THAT asset's insurance_operator. A non-operator must reject — no
// draining an asset's insurance by an unauthorized caller.
#[test]
fn v16_attack_withdraw_insurance_asset_operator_gated() {
    let mut env = V16CuEnv::new();
    env.top_up_insurance(1_000_000);
    let mallory = Keypair::new();
    env.ensure_signer_account(mallory.pubkey());
    let (_, g0) = env.market_state();
    let dest = env.token_account_for_mint(env.mint, mallory.pubkey(), 0);
    // non-operator attempts a domain insurance withdrawal -> reject.
    env.svm.expire_blockhash();
    let r = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawInsuranceAsset {
            market_id: 0,
            asset_index: 0,
            amount: 500_000,
        },
        vec![
            AccountMeta::new(mallory.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&mallory],
    );
    assert!(
        r.is_err(),
        "non-operator asset insurance withdrawal must reject"
    );
    assert_eq!(
        env.token_amount(dest),
        0,
        "no insurance drained by non-operator"
    );
    let (_, g1) = env.market_state();
    assert_eq!(g1.insurance, g0.insurance, "insurance unchanged");
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}

// security.md sweep — live asset insurance has a two-key split. The insurance_authority can fund
// the domain and recover terminal insurance after resolution; live asset withdrawals are gated to the
// insurance_operator. This keeps the key split explicit instead of accidentally letting the funding
// authority act as the hot withdrawal operator.
#[test]
fn v16_attack_live_asset_insurance_withdraw_uses_operator_not_authority() {
    let mut env = V16CuEnv::new();
    let insurance_authority = Keypair::new();
    let insurance_operator = Keypair::new();
    env.ensure_signer_account(insurance_operator.pubkey());
    env.activate_asset_with_authorities(
        1,
        1,
        100,
        insurance_authority.pubkey(),
        insurance_operator.pubkey(),
        env.admin.pubkey(),
        env.admin.pubkey(),
    );
    env.top_up_insurance_domain_with_authority(&insurance_authority, 2, 100);
    let (_, group_before) = env.market_state();
    assert_eq!(
        group_before.insurance_domain_budget[2], 100,
        "authority-funded domain makes the withdrawal check non-vacuous"
    );

    let authority_live_withdraw =
        env.try_withdraw_insurance_domain_with_authority(&insurance_authority, 2, 40);
    assert!(
        authority_live_withdraw.is_err(),
        "insurance_authority alone must not be the live domain withdrawal operator"
    );
    let (_, group_after_reject) = env.market_state();
    assert_eq!(
        group_after_reject.insurance_domain_budget[2], 100,
        "rejected authority withdrawal leaves domain budget intact"
    );
    assert_eq!(
        group_after_reject.insurance, group_before.insurance,
        "rejected authority withdrawal leaves insurance intact"
    );
    assert_eq!(
        env.token_amount(env.vault),
        group_before.vault as u64,
        "rejected authority withdrawal leaves real vault tokens intact"
    );

    let operator_dest = env.token_account(insurance_operator.pubkey(), 0);
    let ledger = env.insurance_ledger_account();
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawInsuranceAsset {
            market_id: 0,
            asset_index: 1,
            amount: 40,
        },
        vec![
            AccountMeta::new(insurance_operator.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(operator_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        &[&insurance_operator],
    )
    .expect("insurance_operator can live-withdraw the funded domain");
    assert_eq!(
        env.token_amount(operator_dest),
        40,
        "operator receives the live domain withdrawal"
    );
    let ledger_state = state::read_insurance_ledger(&env.svm.get_account(&ledger).unwrap().data)
        .expect("operator withdrawal initializes the insurance ledger");
    assert_eq!(
        ledger_state.authority,
        insurance_authority.pubkey().to_bytes(),
        "domain withdrawal ledger is keyed to the cold insurance authority, not the hot operator"
    );
    assert_eq!(ledger_state.total_withdrawn_atoms, 40);
    assert_eq!(ledger_state.last_observed_insurance_atoms, 60);
    let (_, group_after_operator) = env.market_state();
    assert_eq!(group_after_operator.insurance_domain_budget[2], 60);
    assert_eq!(
        group_after_operator.insurance,
        group_before.insurance - 40,
        "operator withdrawal debits only the funded domain insurance"
    );
    assert_eq!(
        env.token_amount(env.vault),
        group_after_operator.vault as u64,
        "engine vault accounting matches SPL vault after operator withdrawal"
    );
}

// security.md sweep — TopUpInsuranceDomain authorization (#6): a per-domain insurance top-up is gated
// to the domain's insurance_authority (v16_program.rs:6577 expect_live_authority). A non-authority
// must reject — no manipulating a domain's insurance/budget accounting by an unauthorized caller.
#[test]
fn v16_attack_topup_insurance_domain_authority_gated() {
    let mut env = V16CuEnv::new();
    let (_, g0) = env.market_state();
    // a non-authority donor tries to top up domain 0's insurance -> reject (Unauthorized).
    let donor = Keypair::new();
    env.ensure_signer_account(donor.pubkey());
    let src = env.token_account_for_mint(env.mint, donor.pubkey(), 500);
    env.svm.expire_blockhash();
    let r = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpInsuranceDomain {
            intent_id: 0,
            market_id: 0,
            domain: 0,
            amount: 500,
        },
        vec![
            AccountMeta::new(donor.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(src, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&donor],
    );
    assert!(
        r.is_err(),
        "non-authority domain insurance top-up must reject"
    );
    assert_eq!(env.token_amount(src), 500, "donor source untouched");
    let (_, g1) = env.market_state();
    assert_eq!(
        g1.insurance, g0.insurance,
        "insurance unchanged by unauthorized top-up"
    );
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}

// security.md sweep — recovery-tool gating (#6): ForfeitRecoveryLeg is owner-gated
// (with_one_portfolio_view enforces owner signs + matches the portfolio). FinalizeResetSide is
// market-only and permissionless, so a bogus victim-portfolio account list must not be accepted.
#[test]
fn v16_attack_recovery_tools_owner_gated() {
    let mut env = V16CuEnv::new();
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    let basis0 = env.portfolio_state(pa).legs[0].basis_pos_q.get();
    let (_, g0) = env.market_state();
    let mallory = Keypair::new();
    env.ensure_signer_account(mallory.pubkey());

    // non-owner ForfeitRecoveryLeg on la's portfolio -> reject (owner mismatch).
    env.svm.expire_blockhash();
    let r1 = env.send(
        ProgInstruction::ForfeitRecoveryLeg {
            portfolio_id: env.portfolio_id(pa),
            position_epoch: env.portfolio_position_epoch(pa),
            asset_index: 0,
            b_delta_budget: 1,
        },
        vec![
            AccountMeta::new(mallory.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(pa, false),
        ],
        &[&mallory],
    );
    assert!(r1.is_err(), "non-owner ForfeitRecoveryLeg must reject");

    // malformed FinalizeResetSide with a victim portfolio account list -> reject.
    env.svm.expire_blockhash();
    let r2 = env.send(
        ProgInstruction::FinalizeResetSide {
            asset_index: 0,
            side: 0,
        },
        vec![
            AccountMeta::new(mallory.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(pa, false),
        ],
        &[&mallory],
    );
    assert!(
        r2.is_err(),
        "FinalizeResetSide must reject the wrong account layout"
    );

    // victim's position untouched, conservation.
    assert_eq!(
        env.portfolio_state(pa).legs[0].basis_pos_q.get(),
        basis0,
        "victim's position untouched by recovery-tool griefing"
    );
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert_eq!(
        g1.assets[0].oi_eff_long_q, g1.assets[0].oi_eff_short_q,
        "OI still balanced"
    );
}

// security.md sweep — TopUpBackingBucket authorization (#6) + vault pinning (#44): funding a backing
// bucket is gated to the domain's backing_bucket_authority and routes only to the canonical vault
// (F-VAULT-FRAG fix). A non-authority must reject.
#[test]
fn v16_attack_topup_backing_bucket_authority_gated() {
    let mut env = V16CuEnv::new();
    let (_, g0) = env.market_state();
    let donor = Keypair::new();
    env.ensure_signer_account(donor.pubkey());
    let src = env.token_account_for_mint(env.mint, donor.pubkey(), 500);
    // non-authority backing top-up -> reject.
    env.svm.expire_blockhash();
    let r = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpBackingBucket {
            intent_id: 0,
            market_id: 0,
            domain: 0,
            amount: 500,
            expiry_slot: 10_000,
        },
        vec![
            AccountMeta::new(donor.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(src, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&donor],
    );
    assert!(
        r.is_err(),
        "non-authority backing bucket top-up must reject"
    );
    assert_eq!(env.token_amount(src), 500, "donor source untouched");
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting vault == real vault"
    );
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}

// security.md sweep - cross-asset backing withdrawal isolation (#6/#33/#48): a backing authority for
// asset 1 is a real privileged key, but it must only control asset 1's domains. A wrong target-domain
// authority lookup would let it drain asset 0's funded backing bucket.
#[test]
fn v16_attack_cross_asset_backing_authority_cannot_withdraw_other_asset_bucket() {
    let mut env = V16CuEnv::new();
    let asset1_backing = Keypair::new();
    env.activate_asset_with_authorities(
        1,
        1,
        100,
        env.admin.pubkey(),
        env.admin.pubkey(),
        asset1_backing.pubkey(),
        env.admin.pubkey(),
    );
    env.top_up_backing_bucket(0, 500, 10_000);
    env.top_up_backing_bucket_with_authority(&asset1_backing, 2, 300, 10_000);
    let asset0_market_id = env.asset_market_id(0);
    let asset1_market_id = env.asset_market_id(1);
    let (_, funded) = env.market_state();
    assert_eq!(
        funded.source_backing_buckets[0].fresh_unliened_backing_num,
        500 * BOUND_SCALE
    );
    assert_eq!(
        funded.source_backing_buckets[2].fresh_unliened_backing_num,
        300 * BOUND_SCALE
    );

    let dest = env.token_account_for_mint(env.mint, asset1_backing.pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let dest_before = env.svm.get_account(&dest).unwrap();

    env.svm.expire_blockhash();
    let rejected = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawBackingBucket {
            domain: 0,
            market_id: asset0_market_id,
            amount: 100,
        },
        vec![
            AccountMeta::new(asset1_backing.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&asset1_backing],
    );
    assert!(
        rejected.is_err(),
        "asset-1 backing authority must not withdraw asset-0 backing"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected cross-asset backing withdrawal leaves market accounting unchanged"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected cross-asset backing withdrawal leaves the canonical vault untouched"
    );
    assert_eq!(
        env.svm.get_account(&dest).unwrap(),
        dest_before,
        "rejected cross-asset backing withdrawal pays no tokens"
    );

    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawBackingBucket {
            domain: 2,
            market_id: asset1_market_id,
            amount: 100,
        },
        vec![
            AccountMeta::new(asset1_backing.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&asset1_backing],
    )
    .expect("asset-1 backing authority withdraws its own domain");
    assert_eq!(env.token_amount(dest), 100);
    let (_, after) = env.market_state();
    assert_eq!(
        after.source_backing_buckets[0].fresh_unliened_backing_num,
        500 * BOUND_SCALE,
        "asset-0 backing remains fully funded"
    );
    assert_eq!(
        after.source_backing_buckets[2].fresh_unliened_backing_num,
        200 * BOUND_SCALE,
        "asset-1 own-domain backing was debited"
    );
    assert_eq!(after.vault as u64, env.token_amount(env.vault));
}

// security.md sweep - cross-asset backing earnings isolation (#6/#33/#48): provider-fee earnings use
// a separate public withdrawal rail from principal backing buckets, with a mandatory domain ledger.
// An asset-1 backing authority must not be able to spend asset-0 earnings or rewrite asset-0 ledger
// counters, even when the target ledger is otherwise valid for asset 0.
#[test]
fn v16_attack_cross_asset_backing_authority_cannot_withdraw_other_asset_earnings() {
    let mut env = V16CuEnv::new();
    let asset1_backing = Keypair::new();
    env.activate_asset_with_authorities(
        1,
        1,
        100,
        env.admin.pubkey(),
        env.admin.pubkey(),
        asset1_backing.pubkey(),
        env.admin.pubkey(),
    );
    env.ensure_signer_account(asset1_backing.pubkey());

    let ledger0 = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger0, 0, 500, 10_000);

    let ledger2 = env.backing_domain_ledger_account();
    let source2 = env.token_account(asset1_backing.pubkey(), 300);
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpBackingBucket {
            intent_id: 0,
            market_id: 0,
            domain: 2,
            amount: 300,
            expiry_slot: 10_000,
        },
        vec![
            AccountMeta::new(asset1_backing.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(source2, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger2, false),
        ],
        &[&asset1_backing],
    )
    .expect("asset-1 backing authority tops up its own domain with ledger");

    env.mutate_market(|_, group| {
        group.source_backing_buckets[0].utilization_fee_earnings = 40;
        group.source_backing_buckets[2].utilization_fee_earnings = 25;
        group.vault += 65;
    });
    let (_, funded) = env.market_state();
    env.set_token_account_amount(
        env.vault,
        env.mint,
        env.vault_authority,
        funded.vault as u64,
    );
    let asset0_market_id = env.asset_market_id(0);
    let asset1_market_id = env.asset_market_id(1);
    assert_eq!(
        funded.source_backing_buckets[0].utilization_fee_earnings,
        40
    );
    assert_eq!(
        funded.source_backing_buckets[2].utilization_fee_earnings,
        25
    );

    let dest = env.token_account_for_mint(env.mint, asset1_backing.pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let dest_before = env.svm.get_account(&dest).unwrap();
    let ledger0_before = env.svm.get_account(&ledger0).unwrap();
    let ledger2_before = env.svm.get_account(&ledger2).unwrap();

    env.svm.expire_blockhash();
    let rejected = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawBackingBucketEarnings {
            domain: 0,
            market_id: asset0_market_id,
            amount: 10,
        },
        vec![
            AccountMeta::new(asset1_backing.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ledger0, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&asset1_backing],
    );
    assert!(
        rejected.is_err(),
        "asset-1 backing authority must not withdraw asset-0 backing earnings"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected cross-asset earnings withdrawal leaves market accounting unchanged"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected cross-asset earnings withdrawal leaves the canonical vault untouched"
    );
    assert_eq!(
        env.svm.get_account(&dest).unwrap(),
        dest_before,
        "rejected cross-asset earnings withdrawal pays no tokens"
    );
    assert_eq!(
        env.svm.get_account(&ledger0).unwrap(),
        ledger0_before,
        "rejected cross-asset earnings withdrawal rewrites no target-domain ledger"
    );
    assert_eq!(
        env.svm.get_account(&ledger2).unwrap(),
        ledger2_before,
        "rejected cross-asset earnings withdrawal rewrites no attacker-domain ledger"
    );

    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawBackingBucketEarnings {
            domain: 2,
            market_id: asset1_market_id,
            amount: 10,
        },
        vec![
            AccountMeta::new(asset1_backing.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ledger2, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&asset1_backing],
    )
    .expect("asset-1 backing authority withdraws its own earnings");
    assert_eq!(env.token_amount(dest), 10);
    let (_, after) = env.market_state();
    assert_eq!(
        after.source_backing_buckets[0].utilization_fee_earnings, 40,
        "asset-0 earnings remain fully withdrawable"
    );
    assert_eq!(
        after.source_backing_buckets[2].utilization_fee_earnings, 15,
        "asset-1 own-domain earnings were debited"
    );
    assert_eq!(after.vault as u64, env.token_amount(env.vault));
    let ledger2_after =
        state::read_backing_domain_ledger(&env.svm.get_account(&ledger2).unwrap().data).unwrap();
    assert_eq!(ledger2_after.total_earnings_atoms, 25);
    assert_eq!(ledger2_after.total_earnings_withdrawn_atoms, 10);
    assert_eq!(ledger2_after.last_observed_bucket_earnings_atoms, 15);
}

// security.md sweep — InitPortfolio account-owner validation (#44/#45): initializing a portfolio on an
// account NOT owned by the program must reject (the program can't safely realloc/write a foreign
// security.md sweep — asset RETIRE authorization (#6/#48): RETIRE is gated to the asset_authority (or
// admin). A non-authority must NOT be able to retire an asset (which, if it held positions, could
// strand them). The engine additionally requires the asset to be EMPTY before retiring.
#[test]
fn v16_attack_retire_asset_authority_gated() {
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
    .expect("cfg mark");
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    env.trade_asset_with_cu(1, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    assert!(
        env.market_state().1.assets[1].oi_eff_long_q > 0,
        "asset 1 has open positions"
    );
    // a NON-authority tries to retire asset 1 -> reject.
    let mallory = Keypair::new();
    env.ensure_signer_account(mallory.pubkey());
    env.svm.warp_to_slot(5);
    let market_id = env.asset_market_id(1);
    let r = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_RETIRE,
            asset_index: 1,
            market_id,
            now_slot: 5,
            initial_price: 0,
            max_init_fee: u128::MAX,
            insurance_authority: [0u8; 32],
            insurance_operator: [0u8; 32],
            backing_bucket_authority: [0u8; 32],
            oracle_authority: [0u8; 32],
        },
        vec![
            AccountMeta::new(mallory.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&mallory],
    );
    assert!(r.is_err(), "non-authority asset RETIRE must reject");
    // positions intact, not stranded.
    assert!(
        env.market_state().1.assets[1].oi_eff_long_q > 0,
        "asset 1 positions NOT stranded by rejected retire"
    );
    let (_, g) = env.market_state();
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
}

// security.md sweep — backing-fee policy authorization (#6) [fee-routing #7]: UpdateBackingFeePolicy
// (per-domain backing fee + insurance share) is gated to the domain's insurance_authority. A non-
// authority must reject, and an out-of-range share must reject. No unauthorized fee-policy tampering.
#[test]
fn v16_attack_backing_fee_policy_authority_gated() {
    let mut env = V16CuEnv::new();
    let (cfg0, _) = env.market_state();
    let mallory = Keypair::new();
    env.ensure_signer_account(mallory.pubkey());
    // non-authority sets the backing fee policy -> reject.
    env.svm.expire_blockhash();
    let r = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateBackingFeePolicy {
            market_id: 0,
            policy_sequence: u64::MAX,
            domain: 0,
            fee_bps: 77,
            insurance_share_bps: 5_000,
        },
        vec![
            AccountMeta::new(mallory.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&mallory],
    );
    assert!(
        r.is_err(),
        "non-authority backing fee policy update must reject"
    );
    // out-of-range insurance share (>10000) by the real authority -> reject.
    env.svm.expire_blockhash();
    let r_oob = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateBackingFeePolicy {
            market_id: 0,
            policy_sequence: u64::MAX,
            domain: 0,
            fee_bps: 77,
            insurance_share_bps: 20_000,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&env.admin],
    );
    assert!(r_oob.is_err(), "insurance_share_bps > 10000 must reject");
    // policy unchanged by the rejected updates.
    let (cfg1, _) = env.market_state();
    assert_eq!(
        cfg1.backing_trade_fee_bps_long, cfg0.backing_trade_fee_bps_long,
        "backing fee policy unchanged"
    );
    assert_eq!(
        cfg1.backing_trade_fee_insurance_share_bps_long,
        cfg0.backing_trade_fee_insurance_share_bps_long,
        "insurance share unchanged"
    );
    // the real authority CAN set it (control).
    env.update_backing_fee_policy_with_cu(0, 77, 5_000);
    assert_eq!(
        env.market_state()
            .0
            .backing_trade_fee_insurance_share_bps_long,
        5_000,
        "authority sets the insurance share"
    );
}

// security.md sweep - cross-asset fee-policy isolation (#6/#104): UpdateBackingFeePolicy is a
// shared config write guarded by the target domain's insurance_authority. An authority for asset 1
// must not be able to mutate asset 0's backing fee policy or the global nonzero-policy counter.
#[test]
fn v16_attack_cross_asset_insurance_authority_cannot_update_other_backing_fee_policy() {
    let mut env = V16CuEnv::new();
    let asset1_insurance = Keypair::new();
    env.activate_asset_with_authorities(
        1,
        1,
        100,
        asset1_insurance.pubkey(),
        env.admin.pubkey(),
        env.admin.pubkey(),
        env.admin.pubkey(),
    );
    env.ensure_signer_account(asset1_insurance.pubkey());

    let market_before = env.svm.get_account(&env.market).unwrap().data;
    env.svm.expire_blockhash();
    let rejected = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateBackingFeePolicy {
            market_id: 0,
            policy_sequence: u64::MAX,
            domain: 0,
            fee_bps: 91,
            insurance_share_bps: 5_000,
        },
        vec![
            AccountMeta::new(asset1_insurance.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&asset1_insurance],
    );
    assert!(
        rejected.is_err(),
        "asset-1 insurance authority must not update asset-0 backing fee policy"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        market_before,
        "cross-asset policy rejection must not mutate shared config/profile bytes"
    );

    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateBackingFeePolicy {
            market_id: 0,
            policy_sequence: u64::MAX,
            domain: 2,
            fee_bps: 91,
            insurance_share_bps: 5_000,
        },
        vec![
            AccountMeta::new(asset1_insurance.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&asset1_insurance],
    )
    .expect("asset-1 insurance authority updates its own backing fee policy");

    let market_after = env.svm.get_account(&env.market).unwrap();
    let (cfg_after, _) = env.market_state();
    let profile0 = state::read_asset_oracle_profile(&market_after.data, 0).unwrap();
    let profile1 = state::read_asset_oracle_profile(&market_after.data, 1).unwrap();
    assert_eq!(cfg_after.backing_trade_fee_policy_count, 1);
    assert_eq!(
        cfg_after.backing_trade_fee_bps_long, 0,
        "asset-0 backing fee policy remains unset"
    );
    assert_eq!(
        profile0.backing_trade_fee_bps_long, 0,
        "asset-0 profile remains untouched"
    );
    assert_eq!(profile1.backing_trade_fee_bps_long, 91);
    assert_eq!(profile1.backing_trade_fee_insurance_share_bps_long, 5_000);
}

// security.md sweep - trade-fee authority isolation (#6/#33): UpdateTradeFeePolicy is a
// market-wide economic knob, but the code intentionally gates it to asset-0's insurance authority.
// After asset-0 insurance is rotated away from marketauth, stale marketauth must not be able to raise
// the fee floor and grief trading; the new asset-0 insurance key can set it, and the fee is charged.
#[test]
fn v16_attack_trade_fee_policy_follows_asset0_insurance_authority() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let new_insurance = Keypair::new();
    env.try_update_per_asset_authority_with_cu(
        &admin,
        Some(&new_insurance),
        0,
        processor::ASSET_AUTH_INSURANCE,
        new_insurance.pubkey().to_bytes(),
    )
    .expect("rotate asset-0 insurance authority away from marketauth");
    assert_eq!(
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap()
            .insurance_authority,
        new_insurance.pubkey().to_bytes()
    );

    let market_before = env.svm.get_account(&env.market).unwrap();
    env.svm.expire_blockhash();
    let stale_marketauth = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateTradeFeePolicy {
            policy_sequence: u64::MAX,
            trade_fee_base_bps: 500,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        stale_marketauth.is_err(),
        "old marketauth must not control the trade fee floor after asset-0 insurance rotates"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected stale marketauth fee update leaves market bytes unchanged"
    );
    assert_eq!(env.market_state().0.trade_fee_base_bps, 0);

    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateTradeFeePolicy {
            policy_sequence: u64::MAX,
            trade_fee_base_bps: 500,
        },
        vec![
            AccountMeta::new(new_insurance.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&new_insurance],
    )
    .expect("current asset-0 insurance authority updates the trade fee floor");
    assert_eq!(env.market_state().0.trade_fee_base_bps, 500);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 1_000_000);
    env.deposit(&short_owner, short, 1_000_000);
    let insurance_before = env.market_state().1.insurance;

    env.svm.expire_blockhash();
    env.try_trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        100,
        500,
    )
    .expect("trade succeeds after both owners consent to the authority-set fee floor");
    let (_, group) = env.market_state();
    assert!(
        group.insurance > insurance_before,
        "trade paid the asset-0-insurance-authorized fee floor"
    );
    assert_eq!(
        group.vault,
        group.c_tot + group.insurance,
        "fee-floor trade remains exactly conserved"
    );
}

// security.md sweep - permissionless asset authority isolation (#6/#33): the creator of asset N owns
// that asset's local domain authorities, but must not be able to mutate market-wide knobs or market 0
// policies. Discriminating control: the same creator CAN update asset N's own backing-fee domain.
#[test]
fn v16_attack_permissionless_asset_authority_cannot_update_marketwide_policies() {
    let mut env = V16CuEnv::new();
    env.update_market_init_fee_policy_with_cu(1);
    let creator = Keypair::new();
    env.svm.warp_to_slot(1);
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
    let cfg_before = env.market_state().0;
    let profile = |env: &V16CuEnv, asset_index: usize| {
        state::read_asset_oracle_profile(
            &env.svm.get_account(&env.market).unwrap().data,
            asset_index,
        )
        .unwrap()
    };
    assert_eq!(
        profile(&env, 1).insurance_authority,
        creator.pubkey().to_bytes()
    );

    let mut attempt = |ix: ProgInstruction| -> Result<u64, String> {
        env.svm.expire_blockhash();
        send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ix,
            vec![
                AccountMeta::new(creator.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&creator],
        )
    };

    assert!(
        attempt(ProgInstruction::UpdateTradeFeePolicy {
            policy_sequence: u64::MAX,
            trade_fee_base_bps: 123
        })
        .is_err(),
        "asset-1 authority must not control the market-wide trade fee"
    );
    assert!(
        attempt(ProgInstruction::UpdateFeeRedirectPolicy {
            policy_sequence: u64::MAX,
            redirect_bps: 5_000
        })
        .is_err(),
        "asset-1 authority must not control market-0 fee redirect"
    );
    assert!(
        attempt(ProgInstruction::UpdateMarketInitFeePolicy {
            min_init_fee: 9,
            policy_sequence: u64::MAX,
        })
        .is_err(),
        "asset-1 authority must not control permissionless init fee"
    );
    assert!(
        attempt(ProgInstruction::UpdateLiquidationFeePolicy {
            policy_sequence: u64::MAX,
            cranker_share_bps: 2_500
        })
        .is_err(),
        "asset-1 authority must not control global liquidation-fee policy"
    );
    assert!(
        attempt(ProgInstruction::UpdateMaintenanceFeePolicy {
            policy_sequence: u64::MAX,
            cranker_share_bps: 2_500
        })
        .is_err(),
        "asset-1 authority must not control global maintenance-fee policy"
    );
    assert!(
        attempt(ProgInstruction::UpdateBackingFeePolicy {
            market_id: 0,
            policy_sequence: u64::MAX,
            domain: 0,
            fee_bps: 55,
            insurance_share_bps: 5_000,
        })
        .is_err(),
        "asset-1 authority must not control market-0 backing-fee policy"
    );

    let cfg_after_rejects = env.market_state().0;
    assert_eq!(
        cfg_after_rejects.trade_fee_base_bps,
        cfg_before.trade_fee_base_bps
    );
    assert_eq!(
        cfg_after_rejects.fee_redirect_to_market_0_bps,
        cfg_before.fee_redirect_to_market_0_bps
    );
    assert_eq!(
        cfg_after_rejects.permissionless_market_init_fee,
        cfg_before.permissionless_market_init_fee
    );
    assert_eq!(
        cfg_after_rejects.liquidation_cranker_fee_share_bps,
        cfg_before.liquidation_cranker_fee_share_bps
    );
    assert_eq!(
        cfg_after_rejects.maintenance_cranker_fee_share_bps,
        cfg_before.maintenance_cranker_fee_share_bps
    );
    assert_eq!(
        cfg_after_rejects.backing_trade_fee_bps_long,
        cfg_before.backing_trade_fee_bps_long
    );
    assert_eq!(
        cfg_after_rejects.backing_trade_fee_policy_count,
        cfg_before.backing_trade_fee_policy_count
    );

    env.svm.expire_blockhash();
    let local_ok = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateBackingFeePolicy {
            market_id: 0,
            policy_sequence: u64::MAX,
            domain: 2,
            fee_bps: 111,
            insurance_share_bps: 5_000,
        },
        vec![
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&creator],
    );
    assert!(
        local_ok.is_ok(),
        "same asset-1 authority may update its own backing-fee domain: {local_ok:?}"
    );
    let cfg_after_local = env.market_state().0;
    let asset1_profile = profile(&env, 1);
    assert_eq!(asset1_profile.backing_trade_fee_bps_long, 111);
    assert_eq!(
        asset1_profile.backing_trade_fee_insurance_share_bps_long,
        5_000
    );
    assert_eq!(
        cfg_after_local.backing_trade_fee_policy_count,
        cfg_before.backing_trade_fee_policy_count + 1
    );
    assert_eq!(
        cfg_after_local.trade_fee_base_bps, cfg_before.trade_fee_base_bps,
        "local domain update did not mutate market-wide trade fee"
    );
}

// security.md sweep — UpdateAuthority current-authority gating / anti-takeover (#6): rotating an authority
// requires the CURRENT holder of that authority to sign (expect_live_authority(cfg.<auth>, current)).
// Attacker goal: a non-admin seizes the admin (or any) authority — a full protocol takeover. Protection:
// the current-authority check rejects anyone who isn't the present holder; the authority is unchanged.
#[test]
fn v16_attack_update_authority_non_holder_cannot_rotate() {
    let mut env = V16CuEnv::new();
    let (cfg0, _) = env.market_state();
    // mallory is NOT any authority; she tries to seize ADMIN, co-signing as the incoming admin.
    let mallory = Keypair::new();
    env.ensure_signer_account(mallory.pubkey());

    // ATTACK: rotate marketauth with mallory as the CURRENT authority -> reject (mallory != cfg.marketauth).
    env.svm.expire_blockhash();
    let r_admin = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateAuthority {
            new_pubkey: mallory.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(mallory.pubkey(), true),
            AccountMeta::new(mallory.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&mallory],
    );
    assert!(
        r_admin.is_err(),
        "a non-holder seizing the market authority must reject"
    );

    // and rotating ASSET 0's insurance authority (now a per-asset op) by a non-holder also rejects:
    // mallory is neither asset-0's asset_admin nor its insurance authority.
    let prof0 = |env: &V16CuEnv| {
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap()
    };
    let a0_ins_before = prof0(&env).insurance_authority;
    let asset_market_id = env.asset_market_id(0);
    env.svm.expire_blockhash();
    let r_a0_ins = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateAssetAuthority {
            asset_index: 0,
            market_id: asset_market_id,
            kind: processor::ASSET_AUTH_INSURANCE,
            new_pubkey: mallory.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(mallory.pubkey(), true),
            AccountMeta::new(mallory.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&mallory],
    );
    assert!(
        r_a0_ins.is_err(),
        "a non-holder rotating asset-0's insurance authority must reject"
    );
    assert_eq!(
        prof0(&env).insurance_authority,
        a0_ins_before,
        "asset-0 insurance authority unchanged"
    );

    // the market authority is byte-identical to the start — no takeover.
    let (cfg1, _) = env.market_state();
    assert_eq!(
        cfg1.marketauth, cfg0.marketauth,
        "market authority unchanged (no takeover)"
    );

    // CONTROL: the genuine current marketauth CAN rotate it (two-party handoff with the new key co-signing).
    let new_admin = Keypair::new();
    env.ensure_signer_account(new_admin.pubkey());
    env.svm.expire_blockhash();
    let ok = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateAuthority {
            new_pubkey: new_admin.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(new_admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&env.admin, &new_admin],
    );
    assert!(
        ok.is_ok(),
        "the genuine market authority rotates itself (co-signed): {:?}",
        ok
    );
    assert_eq!(
        env.market_state().0.marketauth,
        new_admin.pubkey().to_bytes(),
        "market authority rotated by the genuine holder"
    );
}

// security.md sweep — marketauth renounce anti-brick (#6/#30/#48): renouncing marketauth to zero
// permanently disables CloseSlab, so the market slab lamports and any direct vault dust can no longer
// be recovered even if permissionless resolve/force-close fallback exists. Rotation is supported, but
// burn-to-zero is always rejected.
#[test]
fn v16_attack_marketauth_renounce_rejected_even_with_fallback() {
    let mut env = V16CuEnv::new(); // default: permissionless_resolve_stale_slots == 0 (no fallback)
    let (cfg0, _) = env.market_state();
    let zero = Pubkey::default();

    // ATTACK/FOOTGUN: renounce marketauth (-> zero) with NO permissionless fallback -> reject.
    env.svm.expire_blockhash();
    let r = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateAuthority {
            new_pubkey: [0u8; 32],
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new_readonly(zero, false),
            AccountMeta::new(env.market, false),
        ],
        &[&env.admin],
    );
    assert!(
        r.is_err(),
        "renouncing market authority with no permissionless fallback must reject (anti-brick)"
    );
    assert_eq!(
        env.market_state().0.marketauth,
        cfg0.marketauth,
        "market authority unchanged — market not bricked"
    );

    // configure a permissionless fallback (stale + force-close delay both > 0).
    env.configure_permissionless_resolve_with_cu(100, 100);

    // Even with fallback, renounce still rejects: CloseSlab/final slab reclaim requires a live marketauth.
    env.svm.expire_blockhash();
    let with_fallback = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateAuthority {
            new_pubkey: [0u8; 32],
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new_readonly(zero, false),
            AccountMeta::new(env.market, false),
        ],
        &[&env.admin],
    );
    assert!(
        with_fallback.is_err(),
        "renouncing market authority with fallback still bricks final CloseSlab"
    );
    assert_eq!(
        env.market_state().0.marketauth,
        cfg0.marketauth,
        "fallback config does not make marketauth burnable"
    );
}

// security.md sweep — auth-mark push is authority-gated (#6/#37): pushing the settlement mark requires
// the signer to be the asset's oracle/mark authority (expect_live_authority(authorities.oracle_authority,
// signer), src/v16_program.rs:9868). Attacker goal: a non-authority pushes an extreme mark to manipulate
// settlement (induce liquidations / print PnL). Protection: a non-authority push rejects; mark unchanged.
#[test]
fn v16_attack_non_authority_cannot_push_auth_mark() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100); // auth-mark mode, oracle_authority defaults to admin
    let g0 = env.market_state().1;
    assert_eq!(g0.assets[0].effective_price, 100, "mark starts at 100");

    // ATTACK: a non-authority (mallory) pushes an extreme mark -> reject.
    let mallory = Keypair::new();
    env.ensure_signer_account(mallory.pubkey());
    env.svm.warp_to_slot(2);
    env.svm.expire_blockhash();
    let r = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::PushAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 2,
            mark_e6: 9_999_999,
        },
        vec![
            AccountMeta::new(mallory.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&mallory],
    );
    assert!(r.is_err(), "a non-authority auth-mark push must reject");

    // the mark was NOT moved by the attacker.
    assert_eq!(
        env.market_state().1.assets[0].effective_price,
        100,
        "mark unchanged by the rejected push"
    );

    // CONTROL: the genuine authority (admin) — the SAME push — is ACCEPTED (proving the rejection
    // above was the authority gate, not an unrelated failure).
    let admin = env.admin.insecure_clone();
    env.svm.expire_blockhash();
    let ok = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::PushAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 2,
            mark_e6: 150,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        ok.is_ok(),
        "the genuine mark authority's push is accepted: {:?}",
        ok
    );
}

// security.md sweep - oracle reconfiguration authority (#6/#37): changing oracle MODE/anchor is as
// sensitive as pushing a mark. A non-oracle-authority must not switch an empty market between EWMA,
// AUTH_MARK, or HYBRID modes and thereby seize future price control before users arrive.
#[test]
fn v16_attack_non_authority_cannot_reconfigure_oracle_modes() {
    let mut env = V16CuEnv::new();
    let mallory = Keypair::new();
    env.ensure_signer_account(mallory.pubkey());
    env.svm.warp_to_slot(1);

    let before = env.svm.get_account(&env.market).unwrap().data;
    env.svm.expire_blockhash();
    let ewma = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureEwmaMark {
            market_id: 0,
            observation_sequence: 1,
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: 200,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
        },
        vec![
            AccountMeta::new(mallory.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&mallory],
    );
    assert!(
        ewma.is_err(),
        "non-oracle-authority must not switch the asset to EWMA mode"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before,
        "rejected EWMA reconfiguration must not mutate market state"
    );

    env.svm.expire_blockhash();
    let auth_mark = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: 200,
        },
        vec![
            AccountMeta::new(mallory.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&mallory],
    );
    assert!(
        auth_mark.is_err(),
        "non-oracle-authority must not switch the asset to AuthMark mode"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before,
        "rejected AuthMark reconfiguration must not mutate market state"
    );

    let feed = [7u8; 32];
    let pyth = env.set_pyth_price(&feed, 200, 0, 1);
    let mut feeds = [[0u8; 32]; percolator_prog::constants::ORACLE_LEG_CAP];
    feeds[0] = feed;
    env.svm.expire_blockhash();
    let hybrid = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureHybridOracle {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 1,
            now_unix_ts: 1,
            oracle_leg_count: 1,
            oracle_leg_flags: 0,
            max_staleness_secs: 60,
            hybrid_soft_stale_slots: 3,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            invert: 0,
            unit_scale: 0,
            conf_filter_bps: 500,
            oracle_leg_feeds: feeds,
        },
        vec![
            AccountMeta::new(mallory.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new_readonly(pyth, false),
        ],
        &[&mallory],
    );
    assert!(
        hybrid.is_err(),
        "non-oracle-authority must not switch the asset to Hybrid mode"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before,
        "rejected Hybrid reconfiguration must not mutate market state"
    );

    env.svm.expire_blockhash();
    let admin = env.admin.insecure_clone();
    let control = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureEwmaMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: 200,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        control.is_ok(),
        "the configured oracle authority can perform the same well-formed reconfiguration: {control:?}"
    );
}

// Product spec — per-asset cold-storage admin keys (governance): EVERY asset (including asset 0) has
// its OWN admin that can rotate that asset's domain authorities (insurance/operator/backing/oracle)
// and itself, and the asset admin can be BURNED to 0; isolated — it can never act on another asset. Asset 0's
// asset_admin is bootstrapped to the market admin at InitMarket and is rotated/burned through the same
// UpdateAssetAuthority path as assets 1..N. Each domain authority can also self-rotate through a
// co-signed handoff, but cannot be burned to zero after activation.
#[test]
fn v16_attack_per_asset_admin_rotates_keys_isolated_and_burnable() {
    let mut env = V16CuEnv::new(); // 1 slot (asset 0); asset 1 is APPENDED permissionlessly below
    env.configure_auth_mark_with_cu(0, 100);
    env.activate_asset(1, 2, 100); // APPEND asset 1 -> profile.asset_admin bootstraps to the activator (admin)
    let prof = |env: &V16CuEnv, ai: usize| {
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, ai)
            .unwrap()
    };
    assert_eq!(
        prof(&env, 1).asset_admin,
        env.admin.pubkey().to_bytes(),
        "asset-1 admin = activator"
    );
    // asset 0 carries a real stored profile whose asset_admin is the market admin (bootstrapped at init).
    assert_eq!(
        prof(&env, 0).asset_admin,
        env.admin.pubkey().to_bytes(),
        "asset-0 admin = market admin"
    );
    let admin = env.admin.insecure_clone();
    let upd = |env: &mut V16CuEnv,
               signer: &Keypair,
               co: Option<&Keypair>,
               ai: u16,
               kind: u8,
               new: [u8; 32]| {
        env.ensure_signer_account(signer.pubkey());
        let mut signers = vec![signer];
        let co_key = co
            .map(|k| {
                env.ensure_signer_account(k.pubkey());
                k.pubkey()
            })
            .unwrap_or(env.payer.pubkey());
        if let Some(k) = co {
            signers.push(k);
        }
        let market_id = env.asset_market_id(ai);
        env.svm.expire_blockhash();
        send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::UpdateAssetAuthority {
                asset_index: ai,
                market_id,
                kind,
                new_pubkey: new,
            },
            vec![
                AccountMeta::new(signer.pubkey(), true),
                AccountMeta::new(co_key, co.is_some()),
                AccountMeta::new(env.market, false),
            ],
            &signers,
        )
    };

    // 1) the per-asset admin rotates the asset's ORACLE authority (new key co-signs).
    let new_oracle = Keypair::new();
    assert!(
        upd(
            &mut env,
            &admin,
            Some(&new_oracle),
            1,
            processor::ASSET_AUTH_ORACLE,
            new_oracle.pubkey().to_bytes()
        )
        .is_ok(),
        "per-asset admin rotates the asset's oracle authority"
    );
    assert_eq!(
        prof(&env, 1).oracle_authority,
        new_oracle.pubkey().to_bytes(),
        "oracle authority rotated"
    );

    // 2) a non-admin / non-holder cannot rotate.
    let mallory = Keypair::new();
    let m2 = Keypair::new();
    assert!(
        upd(
            &mut env,
            &mallory,
            Some(&m2),
            1,
            processor::ASSET_AUTH_INSURANCE,
            m2.pubkey().to_bytes()
        )
        .is_err(),
        "non-admin cannot rotate the asset's authorities"
    );

    // 3) ASSET 0 uses the SAME per-asset path: its asset_admin (the market admin) rotates asset-0's
    //    insurance authority — and this is ISOLATED, leaving asset 1's authorities byte-identical.
    let a0_ins = Keypair::new();
    let a1_oracle_before = prof(&env, 1).oracle_authority;
    let a1_ins_before = prof(&env, 1).insurance_authority;
    assert!(
        upd(
            &mut env,
            &admin,
            Some(&a0_ins),
            0,
            processor::ASSET_AUTH_INSURANCE,
            a0_ins.pubkey().to_bytes()
        )
        .is_ok(),
        "asset-0 admin rotates asset-0 insurance authority via UpdateAssetAuthority"
    );
    assert_eq!(
        prof(&env, 0).insurance_authority,
        a0_ins.pubkey().to_bytes(),
        "asset-0 insurance authority rotated"
    );
    assert_eq!(
        prof(&env, 1).oracle_authority,
        a1_oracle_before,
        "asset-1 oracle UNTOUCHED by asset-0 rotation"
    );
    assert_eq!(
        prof(&env, 1).insurance_authority,
        a1_ins_before,
        "asset-1 insurance UNTOUCHED by asset-0 rotation"
    );

    // 3b) ISOLATION the other way: the asset-1 admin cannot reach asset 0.
    let a0_prof_before = prof(&env, 0);
    assert!(
        upd(
            &mut env,
            &admin,
            Some(&new_oracle),
            1,
            processor::ASSET_AUTH_ORACLE,
            new_oracle.pubkey().to_bytes()
        )
        .is_ok(),
        "asset-1 admin rotates asset-1 oracle (re-establish a known holder)"
    );
    assert_eq!(
        prof(&env, 0).insurance_authority,
        a0_prof_before.insurance_authority,
        "asset-0 insurance UNTOUCHED by asset-1 rotation"
    );
    assert_eq!(
        prof(&env, 0).asset_admin,
        a0_prof_before.asset_admin,
        "asset-0 admin UNTOUCHED by asset-1 rotation"
    );

    // 4) BURN the per-asset admin to 0 (asset becomes credibly admin-free).
    assert!(
        upd(
            &mut env,
            &admin,
            None,
            1,
            processor::ASSET_AUTH_ADMIN,
            [0u8; 32]
        )
        .is_ok(),
        "asset admin can be burned"
    );
    assert_eq!(prof(&env, 1).asset_admin, [0u8; 32], "asset-1 admin burned");

    // 5) after burn the admin can't be revived (no live admin to sign)...
    assert!(
        upd(
            &mut env,
            &admin,
            Some(&admin),
            1,
            processor::ASSET_AUTH_ADMIN,
            admin.pubkey().to_bytes()
        )
        .is_err(),
        "a burned asset admin cannot be revived"
    );
    // ...but a domain authority still self-rotates (its current holder signs).
    let cold = Keypair::new();
    assert!(
        upd(
            &mut env,
            &new_oracle,
            Some(&cold),
            1,
            processor::ASSET_AUTH_ORACLE,
            cold.pubkey().to_bytes()
        )
        .is_ok(),
        "domain authority self-rotates even after the asset admin is burned"
    );
    assert_eq!(
        prof(&env, 1).oracle_authority,
        cold.pubkey().to_bytes(),
        "oracle self-rotated post-burn"
    );

    // 6) asset-0 admin can also BURN asset-0's own admin (same as 1..N).
    assert!(
        upd(
            &mut env,
            &admin,
            None,
            0,
            processor::ASSET_AUTH_ADMIN,
            [0u8; 32]
        )
        .is_ok(),
        "asset-0 admin can be burned"
    );
    assert_eq!(prof(&env, 0).asset_admin, [0u8; 32], "asset-0 admin burned");
}

// security.md sweep — zero required authority anti-brick (#6/#30/#48): activation rejects zero domain
// authorities because they can strand domain funds or oracle liveness during terminal wind-down.
// UpdateAssetAuthority must preserve that invariant too: an admin/operator cannot burn the
// insurance/operator/backing/oracle authorities to zero after activation. The domains remain
// withdrawable after resolve.
#[test]
fn v16_attack_update_asset_authority_rejects_zero_domain_authority() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    env.top_up_insurance_domain_with_authority(&admin, 0, 500);
    env.top_up_backing_bucket_with_authority(&admin, 0, 300, 100_000);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let (_, group_before) = env.market_state();
    assert_eq!(
        group_before.insurance_domain_budget[0], 500,
        "funded domain makes the test non-vacuous"
    );
    assert_eq!(
        group_before.source_backing_buckets[0].fresh_unliened_backing_num,
        300 * BOUND_SCALE,
        "funded backing bucket makes the backing-authority case non-vacuous"
    );

    for (kind, label) in [
        (processor::ASSET_AUTH_INSURANCE, "insurance authority"),
        (
            processor::ASSET_AUTH_INSURANCE_OPERATOR,
            "insurance operator",
        ),
        (processor::ASSET_AUTH_BACKING_BUCKET, "backing authority"),
        (processor::ASSET_AUTH_ORACLE, "oracle authority"),
    ] {
        let market_id = env.asset_market_id(0);
        env.svm.expire_blockhash();
        let burn = send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::UpdateAssetAuthority {
                asset_index: 0,
                market_id,
                kind,
                new_pubkey: [0u8; 32],
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new_readonly(Pubkey::default(), false),
                AccountMeta::new(env.market, false),
            ],
            &[&admin],
        );
        assert!(
            burn.is_err(),
            "burning the {label} would strand funds or oracle liveness after terminal resolve"
        );
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_before,
            "rejected {label} burn leaves market state unchanged"
        );
        assert_eq!(
            env.svm.get_account(&env.vault).unwrap(),
            vault_before,
            "rejected {label} burn does not touch real vault tokens"
        );
    }

    env.resolve();
    let (insurance_dest, insurance_cu) =
        env.withdraw_terminal_insurance_with_authority(&admin, 500);
    assert_cu_within(
        "terminal insurance after rejected zero-authority burn",
        insurance_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        env.token_amount(insurance_dest),
        500,
        "original insurance authority can recover the funded domain"
    );
    assert_eq!(
        env.market_state().1.insurance,
        0,
        "insurance fully drained for CloseSlab"
    );
    let backing_dest = env.token_account(admin.pubkey(), 0);
    let backing_cu = env.withdraw_backing_bucket_to_admin_token_with_cu(backing_dest, 0, 300);
    assert_cu_within(
        "terminal backing after rejected zero-authority burn",
        backing_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        env.token_amount(backing_dest),
        300,
        "original backing authority can recover the funded bucket"
    );
    env.close_slab_with_cu();
}

// security.md sweep — asset-0 / market admin is bounded (#5 / README L85): even the market-wide admin
// cannot reach into a PERMISSIONLESSLY-created asset's own domain insurance (that is gated by that
// asset's operator), nor can it withdraw a user's portfolio collateral (gated by the portfolio owner).
#[test]
fn v16_attack_market_admin_cannot_drain_foreign_asset_or_user_collateral() {
    let mut env = V16CuEnv::new();
    let market = env.market;
    let vault = env.vault;
    let vault_authority = env.vault_authority;
    let admin = env.admin.insecure_clone();
    env.update_market_init_fee_policy_with_cu(10);
    env.svm.warp_to_slot(1);

    // A stranger permissionlessly creates asset 1, owning ALL of its domain authorities.
    let stranger = Keypair::new();
    env.svm.airdrop(&stranger.pubkey(), 1_000_000_000).unwrap();
    let sp = stranger.pubkey();
    env.activate_permissionless_asset_with_fee(&stranger, 1, 1, 100, sp, sp, sp, sp, 10);
    env.top_up_insurance_domain_with_authority(&stranger, 2, 500); // asset-1 long domain
    let (_, g0) = env.market_state();
    assert!(
        g0.insurance_domain_budget[2] >= 500,
        "asset-1 domain insurance funded by its own operator"
    );

    // BOUND 1: the market admin CANNOT withdraw asset-1's domain insurance (it is not the operator and
    // the asset is healthy/live, so the admin-shutdown-drain path does not apply).
    let admin_dest = env.token_account(admin.pubkey(), 0);
    env.svm.expire_blockhash();
    let r_foreign = env.send(
        ProgInstruction::WithdrawInsuranceAsset {
            market_id: 0,
            asset_index: 1,
            amount: 500,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market, false),
            AccountMeta::new(admin_dest, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        r_foreign.is_err(),
        "market admin must NOT drain a foreign asset's domain insurance"
    );
    assert_eq!(env.token_amount(admin_dest), 0, "nothing drained to admin");
    assert!(
        env.market_state().1.insurance_domain_budget[2] >= 500,
        "asset-1 insurance intact"
    );

    // POSITIVE CONTROL: asset-1's own operator CAN withdraw its domain insurance.
    let strn_dest = env.token_account(stranger.pubkey(), 0);
    env.svm.expire_blockhash();
    let r_owner = env.send(
        ProgInstruction::WithdrawInsuranceAsset {
            market_id: 0,
            asset_index: 1,
            amount: 200,
        },
        vec![
            AccountMeta::new(stranger.pubkey(), true),
            AccountMeta::new(market, false),
            AccountMeta::new(strn_dest, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&stranger],
    );
    assert!(
        r_owner.is_ok(),
        "the asset's own operator may withdraw its domain insurance: {r_owner:?}"
    );
    assert_eq!(
        env.token_amount(strn_dest),
        200,
        "operator received exactly its withdrawal"
    );

    // BOUND 2: the market admin cannot withdraw a USER's portfolio collateral (owner-gated).
    let user = Keypair::new();
    let p = env.create_portfolio(&user);
    env.deposit(&user, p, 10_000);
    let admin_steal = env.token_account(admin.pubkey(), 0);
    env.svm.expire_blockhash();
    let r_user = env.send(
        env.withdraw_ix(p, 10_000),
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(admin_steal, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        r_user.is_err(),
        "market admin must NOT withdraw a user's collateral"
    );
    assert_eq!(
        env.token_amount(admin_steal),
        0,
        "no user collateral drained to admin"
    );
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        10_000,
        "user capital intact"
    );
}

// Regression for percolator-cli#82 (fixed by 05a8f845): UpdateAssetLifecycle(Activate) must NOT let a
// non-admin install attacker-controlled per-asset authorities. Exact exploit shape: a non-admin calls
// Activate on a slot with ITSELF as oracle/insurance/operator/backing authority (written verbatim from
// instruction data), then ConfigureAuthMark + PushAuthMark an extreme price and extracts PnL. The
// fee/admin gate (v16_program.rs:8623) rejects step 1 on a default (fee=0) market: non-admin => Unauthorized.
#[test]
fn v16_attack_non_admin_activate_cannot_install_authorities() {
    let mut env = V16CuEnv::new(); // default permissionless_market_init_fee == 0 => admin-only activation
    let market = env.market;
    env.svm.warp_to_slot(1);

    // Attacker installs ITSELF as every per-asset authority (the exact bounty shape).
    let mallory = Keypair::new();
    env.ensure_signer_account(mallory.pubkey());
    let atk = mallory.pubkey().to_bytes();
    let activation_market_id = env.market_state().1.next_market_id;

    env.svm.expire_blockhash();
    let r_activate = env.send(
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
            market_id: activation_market_id,
            now_slot: 1,
            initial_price: 100,
            max_init_fee: u128::MAX,
            insurance_authority: atk,
            insurance_operator: atk,
            backing_bucket_authority: atk,
            oracle_authority: atk,
        },
        vec![
            AccountMeta::new(mallory.pubkey(), true),
            AccountMeta::new(market, false),
        ],
        &[&mallory],
    );
    assert!(r_activate.is_err(), "non-admin Activate installing attacker authorities must be rejected (fee=0 => Unauthorized)");

    // No asset was created; the attacker is not an authority for anything.
    let g = env.market_state().1;
    assert_ne!(
        g.assets.get(1).map(|a| a.lifecycle),
        Some(AssetLifecycleV16::Active),
        "attacker created no asset"
    );

    // Exploit follow-through is dead at step 1: the attacker cannot drive the asset's mark, because the
    // activation that would have installed it as oracle_authority never happened.
    env.svm.expire_blockhash();
    let r_mark = env.send(
        ProgInstruction::ConfigureAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 1,
            now_slot: 1,
            initial_mark_e6: 1_000_000,
        },
        vec![
            AccountMeta::new(mallory.pubkey(), true),
            AccountMeta::new(market, false),
        ],
        &[&mallory],
    );
    assert!(
        r_mark.is_err(),
        "attacker must not be able to push the mark on an asset it could not activate"
    );

    // Positive control: the market's asset authority (admin) CAN activate.
    let admin = env.admin.insecure_clone();
    let adm = admin.pubkey().to_bytes();
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
            market_id: activation_market_id,
            now_slot: 1,
            initial_price: 100,
            max_init_fee: u128::MAX,
            insurance_authority: adm,
            insurance_operator: adm,
            backing_bucket_authority: adm,
            oracle_authority: adm,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market, false),
        ],
        &[&admin],
    )
    .expect("the asset authority may activate");
    assert_eq!(
        env.market_state().1.assets[1].lifecycle,
        AssetLifecycleV16::Active,
        "admin activation succeeds"
    );
}

// security.md sweep - stale-resolve junior/senior drift (#30/#33/#35/#48):
// ConvertReleasedPnl is owner-gated, but it still moves source-backed junior PnL into senior
// withdrawable capital. Once the market is resolve-matured, that conversion must freeze before the
// #66 guard: WithdrawCollateral must be blocked once the market is resolve-matured (oracle stale past the
// permissionless-resolve threshold) -- reject_permissionless_resolve_matured_live_view (v16_program 4738, the
// #66 fix) is applied to withdraw (5972) + 7 other value-out ops, but its rejection on a value-out op was never
// security.md sweep - stale Withdraw legacy realloc rollback (#5/#30/#44/#48):
// Withdraw grows legacy portfolio storage before the stale-market freeze check
// and then performs a signed vault transfer after the engine debit. A stale
// security.md sweep - stale-resolve freeze (#30/#35/#48): once the authenticated clock has passed
// permissionless_resolve_stale_slots, the market is still mode=Live until ResolveStalePermissionless
// runs, but live value paths must already be frozen. Otherwise an attacker can alter capital, domain
// security.md sweep - stale-resolve materialization DoS (#30/#48): InitPortfolio is public and
// increments materialized_portfolio_count. Once the authenticated clock reaches the permissionless
// resolve threshold, a new empty portfolio must not be materializable in the stale window, or an
// security.md sweep - stale InitPortfolio undersized realloc rollback (#5/#44/#48):
// InitPortfolio is public and safely grows undersized program-owned accounts
// before it initializes them. Once the stale-resolve window is reached, that
// pre-init realloc must also roll back; otherwise a cranker could leave junk
// security.md sweep - stale-resolve lifecycle drift (#30/#35/#48): permissionless asset activation
// reallocs the market, installs new domain authorities, credits the market-init fee, and transfers
// SPL collateral. Once the base market is stale enough for ResolveStalePermissionless, that live
// security.md sweep - stale-resolve asset shutdown drift (#30/#35/#48): a permissionless
// asset admin can shut down its own live asset and freeze its oracle/profile state. Once the
// security.md sweep - stale-resolve privileged lifecycle drift (#30/#35/#48): the market authority
// can normally move an empty asset into DrainOnly or Retired without token movement. Once the base
// market is resolve-matured, those live lifecycle mutations must freeze too; otherwise the terminal
// security.md sweep - stale-resolve oracle restart drift (#30/#37/#48): a permissionless
// asset admin can restart its own Recovery asset and install a fresh per-asset oracle profile.
// Once the base market is resolve-matured, that lifecycle/oracle restart must freeze; otherwise a
// security.md sweep - stale-resolve fee drift (#30/#33/#48): SyncMaintenanceFee is public and can
// debit user capital while crediting market insurance. Once the market is resolve-matured, fee sync
// security.md sweep - stale SyncMaintenanceFee legacy cranker rollback (#5/#30/#44/#48):
// SyncMaintenanceFee is permissionless and grows both the charged portfolio and
// optional cranker reward portfolio before the stale-market freeze check. Once
// resolve-matured, a cranker must not leave either legacy account expanded,
// security.md sweep - stale-resolve recovery-leg drift (#30/#35/#48): ForfeitRecoveryLeg
// is owner-gated, but it can still clear an asset-recovery leg while the overall market
// remains Live. Once the base oracle is resolve-matured, that live recovery-tool path must
// security.md sweep - stale-resolve exposure drift (#30/#35/#48): RebalanceReduce is an
// owner-gated public instruction, but it still mutates live exposure. Once the market is
// resolve-matured, an owner must not be able to reduce their position against stale marks
// security.md sweep - stale-resolve close-progress drift (#30/#35/#48): CureAndCancelClose
// is owner-gated but can cancel forced-close progress and optionally transfer fresh collateral.
// Once the market is resolve-matured, that live cure path must freeze atomically.
// security.md sweep - stale CureAndCancelClose legacy realloc rollback (#5/#30/#44/#48):
// CureAndCancelClose grows legacy portfolio storage before the stale-market
// freeze check, then would cancel close-progress and optionally pull collateral.
// Once resolve-matured, the rejected cure must not leave the legacy account
// security.md sweep - stale-resolve value-out drift (#30/#35/#48): live domain
// withdrawals move SPL custody out of the market's insurance/backing layers. Once
// the market is resolve-matured, insurance, backing principal, and backing-provider
// Per-asset oracle-authority isolation: PushAuthMark validates the signer against THE TARGET ASSET's
// oracle_authority (handle_push_managed_mark reads asset_index's profile -> domain_authorities_from_profile ->
// expect_live_authority, v16_program ~10188). A key that is a *valid* oracle_authority for asset 1 must NOT be
// able to push asset 0's mark. Distinct from v16_attack_non_authority_cannot_push_auth_mark (random key): a
// wrong-asset-index authority read would still reject a random key but WRONGLY ACCEPT asset-1's real authority.
#[test]
fn v16_attack_cross_asset_oracle_authority_cannot_push_other_asset_mark() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100); // asset 0 auth-mark, oracle_authority = admin
                                             // Activate asset 1 with a DISTINCT oracle_authority A1 (a real per-asset authority).
    let a1 = Keypair::new();
    env.ensure_signer_account(a1.pubkey());
    env.activate_asset_with_authorities(
        1,
        1,
        100,
        a1.pubkey(),
        a1.pubkey(),
        a1.pubkey(),
        a1.pubkey(),
    );

    // ATTACK: A1 (asset-1's oracle_authority) pushes ASSET 0's mark -> reject (per-asset isolation).
    env.svm.warp_to_slot(2);
    env.svm.expire_blockhash();
    let r = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::PushAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 2,
            mark_e6: 9_999_999,
        },
        vec![
            AccountMeta::new(a1.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&a1],
    );
    assert!(
        r.is_err(),
        "asset-1's oracle_authority must NOT push asset-0's mark (per-asset isolation)"
    );
    assert_eq!(
        env.market_state().1.assets[0].effective_price,
        100,
        "asset-0 mark unchanged by cross-asset push"
    );

    // CONTROL: asset-0's own authority (admin) pushes asset 0 -> accepted (proves the rejection was the
    // per-asset authority gate, not an unrelated failure).
    let admin = env.admin.insecure_clone();
    env.svm.expire_blockhash();
    let ok = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::PushAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 2,
            mark_e6: 150,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(ok.is_ok(), "asset-0's own authority pushes asset-0 (proves the rejection was the per-asset gate): {ok:?}");
}

// Per-asset oracle-authority isolation for the separate EWMA push handler. PushAuthMark has the same
// boundary covered above, but PushEwmaMark is its own public entrypoint: a real oracle_authority for
// asset 1 must not be able to steer asset 0's EWMA mark and cause cross-asset liquidations/LoF.
#[test]
fn v16_attack_cross_asset_oracle_authority_cannot_push_other_asset_ewma_mark() {
    let mut env = V16CuEnv::new();
    env.configure_ewma_mark_with_cu(0, 100, 10, 0); // asset 0 EWMA, oracle_authority = admin

    // Activate asset 1 with a distinct real oracle_authority.
    let a1 = Keypair::new();
    env.ensure_signer_account(a1.pubkey());
    env.activate_asset_with_authorities(
        1,
        1,
        100,
        a1.pubkey(),
        a1.pubkey(),
        a1.pubkey(),
        a1.pubkey(),
    );

    // Non-vacuous control: a1 is accepted as the real oracle_authority for asset 1.
    env.svm.expire_blockhash();
    let own_asset_config = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureEwmaMark {
            market_id: 0,
            observation_sequence: 1,
            asset_index: 1,
            now_slot: 1,
            initial_mark_e6: 100,
            mark_ewma_halflife_slots: 10,
            mark_min_fee: 0,
        },
        vec![
            AccountMeta::new(a1.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&a1],
    );
    assert!(
        own_asset_config.is_ok(),
        "asset-1's own oracle_authority configures asset-1 EWMA: {own_asset_config:?}"
    );
    env.svm.warp_to_slot(2);
    env.svm.expire_blockhash();
    let own_asset_push = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::PushEwmaMark {
            market_id: 0,
            observation_sequence: 2,
            asset_index: 1,
            now_slot: 2,
            mark_e6: 150,
        },
        vec![
            AccountMeta::new(a1.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&a1],
    );
    assert!(
        own_asset_push.is_ok(),
        "asset-1's own oracle_authority pushes asset-1 EWMA: {own_asset_push:?}"
    );

    // ATTACK: the same valid asset-1 oracle_authority pushes asset 0's EWMA mark -> reject.
    env.svm.warp_to_slot(2);
    env.svm.expire_blockhash();
    let r = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::PushEwmaMark {
            market_id: 0,
            observation_sequence: 2,
            asset_index: 0,
            now_slot: 2,
            mark_e6: 9_999_999,
        },
        vec![
            AccountMeta::new(a1.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&a1],
    );
    assert!(
        r.is_err(),
        "asset-1's oracle_authority must not push asset-0's EWMA mark"
    );
    assert_eq!(
        env.market_state().0.mark_ewma_e6,
        100,
        "asset-0 EWMA mark unchanged by cross-asset push"
    );

    // CONTROL: asset 0's own authority can push asset 0.
    let admin = env.admin.insecure_clone();
    env.svm.expire_blockhash();
    let ok = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::PushEwmaMark {
            market_id: 0,
            observation_sequence: 2,
            asset_index: 0,
            now_slot: 2,
            mark_e6: 150,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        ok.is_ok(),
        "asset-0's own authority pushes asset-0 EWMA mark: {ok:?}"
    );
    assert!(
        env.market_state().0.mark_ewma_e6 > 100,
        "control push moved the EWMA mark"
    );
}

// Per-asset oracle-authority isolation for reconfiguration, not just mark pushes. A key that is a
// valid oracle_authority for asset 1 must not be accepted when the target asset_index is 0; otherwise
// a wrong-profile authority lookup could let a permissionless asset steer the base market's oracle
// mode before exposure arrives. Each rejected attack snapshots the market account, and the same key
// then configures asset 1 through the identical public handler to prove the signer is non-vacuously
// valid for its own asset.
#[test]
fn v16_attack_cross_asset_oracle_authority_cannot_reconfigure_other_asset_modes() {
    let setup = || {
        let mut env = V16CuEnv::new();
        env.configure_auth_mark_with_cu(0, 100);
        let a1 = Keypair::new();
        env.ensure_signer_account(a1.pubkey());
        env.activate_asset_with_authorities(
            1,
            1,
            100,
            a1.pubkey(),
            a1.pubkey(),
            a1.pubkey(),
            a1.pubkey(),
        );
        (env, a1)
    };

    {
        let (mut env, a1) = setup();
        let before = env.svm.get_account(&env.market).unwrap();
        env.svm.expire_blockhash();
        let attack = send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::ConfigureAuthMark {
                market_id: 0,
                observation_sequence: u64::MAX,
                asset_index: 0,
                now_slot: 1,
                initial_mark_e6: 250,
            },
            vec![
                AccountMeta::new(a1.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&a1],
        );
        assert!(
            attack.is_err(),
            "asset-1 oracle_authority must not configure asset-0 AuthMark"
        );
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            before,
            "rejected cross-asset AuthMark reconfiguration is atomic"
        );

        env.svm.expire_blockhash();
        let own_asset = send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::ConfigureAuthMark {
                market_id: 0,
                observation_sequence: u64::MAX,
                asset_index: 1,
                now_slot: 1,
                initial_mark_e6: 250,
            },
            vec![
                AccountMeta::new(a1.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&a1],
        );
        assert!(
            own_asset.is_ok(),
            "the same key configures its own asset's AuthMark: {own_asset:?}"
        );
        let profile =
            state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 1)
                .unwrap();
        assert_eq!(
            profile.oracle_mode,
            percolator_prog::constants::ORACLE_MODE_AUTH_MARK
        );
        assert_eq!(profile.oracle_target_price_e6, 250);
    }

    {
        let (mut env, a1) = setup();
        let before = env.svm.get_account(&env.market).unwrap();
        env.svm.expire_blockhash();
        let attack = send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::ConfigureEwmaMark {
                market_id: 0,
                observation_sequence: u64::MAX,
                asset_index: 0,
                now_slot: 1,
                initial_mark_e6: 250,
                mark_ewma_halflife_slots: 10,
                mark_min_fee: 0,
            },
            vec![
                AccountMeta::new(a1.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&a1],
        );
        assert!(
            attack.is_err(),
            "asset-1 oracle_authority must not configure asset-0 EWMA"
        );
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            before,
            "rejected cross-asset EWMA reconfiguration is atomic"
        );

        env.svm.expire_blockhash();
        let own_asset = send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::ConfigureEwmaMark {
                market_id: 0,
                observation_sequence: u64::MAX,
                asset_index: 1,
                now_slot: 1,
                initial_mark_e6: 250,
                mark_ewma_halflife_slots: 10,
                mark_min_fee: 0,
            },
            vec![
                AccountMeta::new(a1.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&a1],
        );
        assert!(
            own_asset.is_ok(),
            "the same key configures its own asset's EWMA: {own_asset:?}"
        );
        let profile =
            state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 1)
                .unwrap();
        assert_eq!(
            profile.oracle_mode,
            percolator_prog::constants::ORACLE_MODE_EWMA_MARK
        );
        assert_eq!(profile.mark_ewma_e6, 250);
    }

    {
        let (mut env, a1) = setup();
        set_test_clock(&mut env, 1, 1);
        let feed = [91u8; 32];
        let pyth = env.set_pyth_price(&feed, 200_000, -6, 1);
        let mut feeds = [[0u8; 32]; percolator_prog::constants::ORACLE_LEG_CAP];
        feeds[0] = feed;
        let before = env.svm.get_account(&env.market).unwrap();
        env.svm.expire_blockhash();
        let attack = send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::ConfigureHybridOracle {
                market_id: 0,
                observation_sequence: u64::MAX,
                asset_index: 0,
                now_slot: 1,
                now_unix_ts: 1,
                oracle_leg_count: 1,
                oracle_leg_flags: 0,
                max_staleness_secs: 60,
                hybrid_soft_stale_slots: 3,
                mark_ewma_halflife_slots: 10,
                mark_min_fee: 0,
                invert: 0,
                unit_scale: 0,
                conf_filter_bps: 500,
                oracle_leg_feeds: feeds,
            },
            vec![
                AccountMeta::new(a1.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new_readonly(pyth, false),
            ],
            &[&a1],
        );
        assert!(
            attack.is_err(),
            "asset-1 oracle_authority must not configure asset-0 Hybrid"
        );
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            before,
            "rejected cross-asset Hybrid reconfiguration is atomic"
        );

        env.svm.expire_blockhash();
        let own_asset = send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::ConfigureHybridOracle {
                market_id: 0,
                observation_sequence: u64::MAX,
                asset_index: 1,
                now_slot: 1,
                now_unix_ts: 1,
                oracle_leg_count: 1,
                oracle_leg_flags: 0,
                max_staleness_secs: 60,
                hybrid_soft_stale_slots: 3,
                mark_ewma_halflife_slots: 10,
                mark_min_fee: 0,
                invert: 0,
                unit_scale: 0,
                conf_filter_bps: 500,
                oracle_leg_feeds: feeds,
            },
            vec![
                AccountMeta::new(a1.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new_readonly(pyth, false),
            ],
            &[&a1],
        );
        assert!(
            own_asset.is_ok(),
            "the same key configures its own asset's Hybrid oracle: {own_asset:?}"
        );
        let profile =
            state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 1)
                .unwrap();
        assert_eq!(
            profile.oracle_mode,
            percolator_prog::constants::ORACLE_MODE_HYBRID_AFTER_HOURS
        );
        assert_eq!(profile.oracle_target_price_e6, 200_000);
    }
}

// [from pr125]
// LoF sweep — oracle-authority rotation actually transfers control (revokes old, grants new). Rotating an
// asset's oracle authority via UpdateAssetAuthority must REVOKE the previous holder's mark-push power and
// GRANT it to the new key. If rotation only updated state cosmetically (old key still able to push), a
// rotated-out / compromised key could keep injecting marks to manipulate settlement (LoF); if the new key
// could not push, the asset's oracle would be bricked (DoS). Drives the functional transfer end-to-end:
// admin (the bootstrapped oracle authority) pushes once, the asset_admin rotates the oracle authority to a
// fresh key, then the OLD key's push rejects (Unauthorized) while the NEW key's push succeeds. The
// existing rotation test checks state-level isolation/burnability, not the functional push transfer.
#[test]
fn v16_attack_oracle_authority_rotation_revokes_old_grants_new() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100); // admin bootstraps as asset 0's oracle authority; mark = 100
    let admin = env.admin.insecure_clone();

    // Baseline: the current oracle authority (admin) can push a mark.
    env.svm.warp_to_slot(2);
    env.svm.expire_blockhash();
    let r0 = env.send(
        ProgInstruction::PushAuthMark {
            market_id: 0,
            observation_sequence: 2,
            asset_index: 0,
            now_slot: 2,
            mark_e6: 110,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        r0.is_ok(),
        "admin (oracle authority) can push before rotation: {r0:?}"
    );

    // Rotate the ORACLE authority admin -> newauth. admin signs as asset_admin; newauth co-signs (proves
    // control of the incoming key).
    let newauth = Keypair::new();
    env.ensure_signer_account(newauth.pubkey());
    let asset_market_id = env.asset_market_id(0);
    env.svm.expire_blockhash();
    let rot = env.send(
        ProgInstruction::UpdateAssetAuthority {
            asset_index: 0,
            market_id: asset_market_id,
            kind: percolator_prog::processor::ASSET_AUTH_ORACLE,
            new_pubkey: newauth.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(newauth.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin, &newauth],
    );
    assert!(
        rot.is_ok(),
        "asset_admin rotates the oracle authority: {rot:?}"
    );

    env.svm.warp_to_slot(3);
    // OLD authority (admin) is now REVOKED: its push must reject as Unauthorized.
    env.svm.expire_blockhash();
    let r_old = env.send(
        ProgInstruction::PushAuthMark {
            market_id: 0,
            observation_sequence: 3,
            asset_index: 0,
            now_slot: 3,
            mark_e6: 120,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        r_old.is_err(),
        "the OLD oracle authority must be revoked after rotation"
    );
    assert!(
        r_old.unwrap_err().contains("Custom(8)"),
        "revoked old authority push must be Unauthorized (Custom 8)"
    );

    // NEW authority can push: rotation GRANTED control.
    env.svm.expire_blockhash();
    let r_new = env.send(
        ProgInstruction::PushAuthMark {
            market_id: 0,
            observation_sequence: 3,
            asset_index: 0,
            now_slot: 3,
            mark_e6: 120,
        },
        vec![
            AccountMeta::new(newauth.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&newauth],
    );
    assert!(
        r_new.is_ok(),
        "the NEW oracle authority can push after rotation: {r_new:?}"
    );
}

// [from pr114]
// full-interface sweep: live asset-0 insurance withdrawal must follow the current hot
// insurance_operator, not stale marketauth/asset-admin privilege. This is value-moving: after the
// operator is rekeyed, the old market authority must not drain the funded asset-0 insurance domain.
#[test]
fn v16_attack_asset0_operator_rotation_rekeys_live_insurance_withdraw() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let new_operator = Keypair::new();

    env.try_update_per_asset_authority_with_cu(
        &admin,
        Some(&new_operator),
        0,
        processor::ASSET_AUTH_INSURANCE_OPERATOR,
        new_operator.pubkey().to_bytes(),
    )
    .expect("asset-0 admin rotates the live insurance operator");
    env.top_up_insurance_domain_with_authority(&admin, 0, 500);

    let (_, group_before) = env.market_state();
    assert_eq!(
        group_before.insurance_domain_budget[0], 500,
        "funded asset-0 insurance domain makes the stale-operator attempt non-vacuous"
    );
    let stale_dest = env.token_account(admin.pubkey(), 0);
    let stale_dest_before = env.svm.get_account(&stale_dest).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let market_before = env.svm.get_account(&env.market).unwrap();

    env.svm.expire_blockhash();
    let stale_admin = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawInsuranceAsset {
            market_id: 0,
            asset_index: 0,
            amount: 100,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(stale_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        stale_admin.is_err(),
        "stale marketauth/asset-admin must not retain live asset-0 insurance-operator power"
    );
    assert_eq!(
        env.svm.get_account(&stale_dest).unwrap(),
        stale_dest_before,
        "stale operator attempt pays no tokens"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "stale operator attempt does not debit the vault"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "stale operator attempt leaves market state unchanged"
    );

    let operator_dest = env.token_account(new_operator.pubkey(), 0);
    env.svm.expire_blockhash();
    let operator_withdraw = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawInsuranceAsset {
            market_id: 0,
            asset_index: 0,
            amount: 100,
        },
        vec![
            AccountMeta::new(new_operator.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(operator_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&new_operator],
    );
    assert!(
        operator_withdraw.is_ok(),
        "current asset-0 insurance_operator can withdraw live insurance: {operator_withdraw:?}"
    );
    assert_eq!(
        env.token_amount(operator_dest),
        100,
        "current operator receives the withdrawal"
    );
    let (_, group_after) = env.market_state();
    assert_eq!(group_after.insurance_domain_budget[0], 400);
    assert_eq!(group_after.insurance, group_before.insurance - 100);
    assert_eq!(
        env.token_amount(env.vault),
        group_after.vault as u64,
        "engine vault accounting matches SPL vault after rekeyed-operator withdrawal"
    );
}

// [from pr114]
// full-interface sweep: WithdrawBackingBucket has a two-stage authority check where marketauth passes
// preflight, but live withdrawal must still require the current backing_bucket_authority unless the
// domain is in shutdown-drain. After asset-0 backing authority is rekeyed, stale marketauth must not
// drain live provider backing.
#[test]
fn v16_attack_asset0_backing_rotation_rekeys_live_bucket_withdraw() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let new_backing = Keypair::new();

    env.try_update_per_asset_authority_with_cu(
        &admin,
        Some(&new_backing),
        0,
        processor::ASSET_AUTH_BACKING_BUCKET,
        new_backing.pubkey().to_bytes(),
    )
    .expect("asset-0 admin rotates the backing-bucket authority");
    env.top_up_backing_bucket_with_authority(&new_backing, 0, 500, 100_000);
    let asset0_market_id = env.asset_market_id(0);

    let (_, group_before) = env.market_state();
    assert_eq!(
        group_before.source_backing_buckets[0].fresh_unliened_backing_num,
        500 * BOUND_SCALE,
        "funded asset-0 backing bucket makes the stale-authority attempt non-vacuous"
    );
    let stale_dest = env.token_account(admin.pubkey(), 0);
    let stale_dest_before = env.svm.get_account(&stale_dest).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let market_before = env.svm.get_account(&env.market).unwrap();

    env.svm.expire_blockhash();
    let stale_admin = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawBackingBucket {
            domain: 0,
            market_id: asset0_market_id,
            amount: 100,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(stale_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        stale_admin.is_err(),
        "stale marketauth must not drain live backing after asset-0 backing authority rotates"
    );
    assert_eq!(
        env.svm.get_account(&stale_dest).unwrap(),
        stale_dest_before,
        "stale backing-withdraw attempt pays no tokens"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "stale backing-withdraw attempt does not debit the vault"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "stale backing-withdraw attempt leaves market state unchanged"
    );

    let backing_dest = env.token_account(new_backing.pubkey(), 0);
    env.svm.expire_blockhash();
    let backing_withdraw = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawBackingBucket {
            domain: 0,
            market_id: asset0_market_id,
            amount: 100,
        },
        vec![
            AccountMeta::new(new_backing.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(backing_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&new_backing],
    );
    assert!(
        backing_withdraw.is_ok(),
        "current asset-0 backing authority can withdraw live backing: {backing_withdraw:?}"
    );
    assert_eq!(
        env.token_amount(backing_dest),
        100,
        "current backing authority receives the withdrawal"
    );
    let (_, group_after) = env.market_state();
    assert_eq!(
        group_after.source_backing_buckets[0].fresh_unliened_backing_num,
        400 * BOUND_SCALE
    );
    assert_eq!(
        env.token_amount(env.vault),
        group_after.vault as u64,
        "engine vault accounting matches SPL vault after rekeyed backing withdrawal"
    );
}

// [from pr114]
// full-interface sweep: the market-wide handoff also owns base-unit reserve swaps. A stale former
// marketauth must not keep a value-moving `SwapSecondaryForPrimary` path after rotation, or it could
// drain the secondary reserve by providing primary collateral under its own control.
#[test]
fn v16_attack_update_authority_handoff_rekeys_secondary_swap_authority() {
    let mut env = V16CuEnv::new();
    let old_marketauth = env.admin.insecure_clone();
    let new_marketauth = Keypair::new();
    env.ensure_signer_account(new_marketauth.pubkey());

    let secondary_mint = env.create_mint();
    env.update_base_unit_mints_with_cu(env.mint, secondary_mint);
    let secondary_vault = canonical_vault_ata(env.vault_authority, secondary_mint);
    env.svm
        .set_account(
            secondary_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(secondary_mint, env.vault_authority, 40),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let stale_primary_source = env.token_account_for_mint(env.mint, old_marketauth.pubkey(), 20);
    let stale_secondary_dest =
        env.token_account_for_mint(secondary_mint, old_marketauth.pubkey(), 0);
    let fresh_primary_source = env.token_account_for_mint(env.mint, new_marketauth.pubkey(), 20);
    let fresh_secondary_dest =
        env.token_account_for_mint(secondary_mint, new_marketauth.pubkey(), 0);

    env.update_asset_authority_with_cu(&new_marketauth);
    assert_eq!(
        env.market_state().0.marketauth,
        new_marketauth.pubkey().to_bytes(),
        "market authority rotated to the new signer"
    );

    let market_before_stale = env.svm.get_account(&env.market).unwrap().data;
    let primary_vault_before = env.token_amount(env.vault);
    let secondary_vault_before = env.token_amount(secondary_vault);
    env.svm.expire_blockhash();
    let stale_swap = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::SwapSecondaryForPrimary { amount: 20 },
        vec![
            AccountMeta::new(old_marketauth.pubkey(), true),
            AccountMeta::new_readonly(env.market, false),
            AccountMeta::new(stale_primary_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new(stale_secondary_dest, false),
            AccountMeta::new(secondary_vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&old_marketauth],
    );
    assert!(
        stale_swap.is_err(),
        "stale marketauth must not retain the value-moving secondary-swap path"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        market_before_stale,
        "rejected stale swap does not rewrite market config"
    );
    assert_eq!(
        env.token_amount(stale_primary_source),
        20,
        "stale signer primary source is not debited"
    );
    assert_eq!(
        env.token_amount(stale_secondary_dest),
        0,
        "stale signer receives no secondary collateral"
    );
    assert_eq!(
        env.token_amount(env.vault),
        primary_vault_before,
        "primary vault unchanged after stale swap"
    );
    assert_eq!(
        env.token_amount(secondary_vault),
        secondary_vault_before,
        "secondary reserve unchanged after stale swap"
    );

    env.svm.expire_blockhash();
    let fresh_swap = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::SwapSecondaryForPrimary { amount: 20 },
        vec![
            AccountMeta::new(new_marketauth.pubkey(), true),
            AccountMeta::new_readonly(env.market, false),
            AccountMeta::new(fresh_primary_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new(fresh_secondary_dest, false),
            AccountMeta::new(secondary_vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&new_marketauth],
    );
    assert!(
        fresh_swap.is_ok(),
        "new marketauth can execute the value-moving secondary swap after handoff: {fresh_swap:?}"
    );
    assert_eq!(env.token_amount(fresh_primary_source), 0);
    assert_eq!(
        env.token_amount(env.vault),
        primary_vault_before + 20,
        "new authority's primary collateral lands in the primary vault"
    );
    assert_eq!(
        env.token_amount(fresh_secondary_dest),
        20,
        "new authority receives secondary collateral"
    );
    assert_eq!(
        env.token_amount(secondary_vault),
        secondary_vault_before - 20,
        "secondary reserve debited exactly once"
    );
}

// full-interface sweep: UpdateAuthority used to rotate only `cfg.marketauth`; asset 0's default
// `asset_admin` stayed pinned to the previous key. That stale key could still force-shutdown asset 0
// after handoff, even though policy/resolve/base-unit powers had moved to the new market authority.
#[test]
fn v16_attack_update_authority_handoff_rekeys_asset0_lifecycle_admin() {
    let mut env = V16CuEnv::new();
    let old_marketauth = env.admin.insecure_clone();
    let new_marketauth = Keypair::new();
    env.configure_auth_mark_with_cu(0, 100);
    env.configure_permissionless_resolve_with_cu(100, 5);

    env.update_asset_authority_with_cu(&new_marketauth);
    assert_eq!(
        env.market_state().0.marketauth,
        new_marketauth.pubkey().to_bytes(),
        "market authority rotated to the new signer"
    );
    assert_eq!(
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap()
            .asset_admin,
        new_marketauth.pubkey().to_bytes(),
        "default asset-0 admin follows the market authority handoff"
    );

    let asset_market_id = env.asset_market_id(0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    env.svm.warp_to_slot(2);
    env.svm.expire_blockhash();
    let stale_shutdown = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
            asset_index: 0,
            market_id: asset_market_id,
            now_slot: 2,
            initial_price: 0,
            max_init_fee: u128::MAX,
            insurance_authority: old_marketauth.pubkey().to_bytes(),
            insurance_operator: old_marketauth.pubkey().to_bytes(),
            backing_bucket_authority: old_marketauth.pubkey().to_bytes(),
            oracle_authority: old_marketauth.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(old_marketauth.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&old_marketauth],
    );
    assert!(
        stale_shutdown.is_err(),
        "stale marketauth must not retain asset-0 lifecycle-admin power after handoff"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected stale asset-0 shutdown leaves market bytes unchanged"
    );
    assert_eq!(
        env.market_state().1.assets[0].lifecycle,
        AssetLifecycleV16::Active,
        "asset 0 remains active after stale shutdown attempt"
    );

    env.svm.expire_blockhash();
    let fresh_shutdown = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
            asset_index: 0,
            market_id: asset_market_id,
            now_slot: 2,
            initial_price: 0,
            max_init_fee: u128::MAX,
            insurance_authority: new_marketauth.pubkey().to_bytes(),
            insurance_operator: new_marketauth.pubkey().to_bytes(),
            backing_bucket_authority: new_marketauth.pubkey().to_bytes(),
            oracle_authority: new_marketauth.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(new_marketauth.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&new_marketauth],
    );
    assert!(
        fresh_shutdown.is_ok(),
        "current marketauth can still shut down asset 0 after handoff: {fresh_shutdown:?}"
    );
    assert_eq!(
        env.market_state().1.assets[0].lifecycle,
        AssetLifecycleV16::Recovery
    );
}

#[test]
fn v16_attack_update_authority_handoff_rekeys_asset0_default_runtime_authorities() {
    let mut env = V16CuEnv::new();
    let old_marketauth = env.admin.insecure_clone();
    let new_marketauth = Keypair::new();
    env.ensure_signer_account(new_marketauth.pubkey());
    env.configure_auth_mark_with_cu(0, 100);

    env.update_asset_authority_with_cu(&new_marketauth);
    let profile =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap();
    assert_eq!(
        profile.insurance_authority,
        new_marketauth.pubkey().to_bytes(),
        "default asset-0 insurance authority follows the market authority handoff"
    );
    assert_eq!(
        profile.insurance_operator,
        new_marketauth.pubkey().to_bytes(),
        "default asset-0 insurance operator follows the market authority handoff"
    );
    assert_eq!(
        profile.backing_bucket_authority,
        new_marketauth.pubkey().to_bytes(),
        "default asset-0 backing authority follows the market authority handoff"
    );
    assert_eq!(
        profile.oracle_authority,
        new_marketauth.pubkey().to_bytes(),
        "default asset-0 oracle authority follows the market authority handoff"
    );

    let market_before_old_fee = env.svm.get_account(&env.market).unwrap();
    env.svm.expire_blockhash();
    let stale_fee = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateTradeFeePolicy {
            policy_sequence: u64::MAX,
            trade_fee_base_bps: 500,
        },
        vec![
            AccountMeta::new(old_marketauth.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&old_marketauth],
    );
    assert!(
        stale_fee.is_err(),
        "stale marketauth must not retain asset-0-insurance fee-floor power after handoff"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_old_fee,
        "rejected stale fee update leaves market bytes unchanged"
    );

    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateTradeFeePolicy {
            policy_sequence: u64::MAX,
            trade_fee_base_bps: 500,
        },
        vec![
            AccountMeta::new(new_marketauth.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&new_marketauth],
    )
    .expect("current marketauth inherits asset-0 insurance fee-floor power");
    assert_eq!(env.market_state().0.trade_fee_base_bps, 500);

    let target_before_old_push = env.market_state().0.oracle_target_price_e6;
    env.svm.warp_to_slot(2);
    env.svm.expire_blockhash();
    let stale_push = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::PushAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 2,
            mark_e6: 777,
        },
        vec![
            AccountMeta::new(old_marketauth.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&old_marketauth],
    );
    assert!(
        stale_push.is_err(),
        "stale marketauth must not retain asset-0 oracle push power after handoff"
    );
    assert_eq!(
        env.market_state().0.oracle_target_price_e6,
        target_before_old_push,
        "rejected stale oracle push leaves the asset-0 oracle target unchanged"
    );

    env.svm.expire_blockhash();
    env.push_auth_mark_for_asset_with_authority(0, &new_marketauth, 2, 777);
    assert_eq!(
        env.market_state().0.oracle_target_price_e6,
        777,
        "current marketauth inherits asset-0 oracle target push power"
    );
    assert_eq!(
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap()
            .oracle_target_price_e6,
        777,
        "current marketauth updates the stored asset-0 oracle profile"
    );
}

// security.md sweep — admin-instruction authorization (#6): privileged ops (ResolveMarket,
// ConfigureAuthMark, policy updates) must reject a non-admin signer. A permissionless resolve would be
// a catastrophic griefing/wind-down trigger.
#[test]
fn v16_attack_non_admin_cannot_resolve_or_configure() {
    let mut env = V16CuEnv::new();
    let mallory = Keypair::new();
    env.ensure_signer_account(mallory.pubkey());
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    env.deposit(&la, pa, 1_000_000);

    // non-admin ResolveMarket -> reject; market stays Live.
    env.svm.expire_blockhash();
    let r_res = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ResolveMarket {
            asset_generation_frontier: 0,
        },
        vec![
            AccountMeta::new(mallory.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&mallory],
    );
    assert!(r_res.is_err(), "non-admin ResolveMarket must reject");
    let (cfg0, g0) = env.market_state();
    // mode 0 == Live: a successful attacker-resolve would flip it.
    // (read via raw mode to avoid coupling; vault/positions still operable below proves Live.)

    // non-admin ConfigureAuthMark -> reject.
    env.svm.expire_blockhash();
    let r_cfg = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 0,
            initial_mark_e6: 999_999,
        },
        vec![
            AccountMeta::new(mallory.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&mallory],
    );
    assert!(r_cfg.is_err(), "non-admin ConfigureAuthMark must reject");

    // Proof the market is still Live & operable: the owner can still withdraw (rejected if resolved).
    env.svm.expire_blockhash();
    let (dest, _) = env.withdraw_with_cu(&la, pa, 500_000);
    assert_eq!(
        env.token_amount(dest),
        500_000,
        "market still Live: owner withdraw works (not resolved by attacker)"
    );
    let (_, g1) = env.market_state();
    assert_eq!(
        g1.vault,
        g0.vault - 500_000,
        "only the legit withdraw moved funds"
    );
    let _ = cfg0;
}

// security.md sweep - stale marketauth final reclaim (#6/#48): CloseSlab is the terminal authority
// path that moves raw vault dust, closes the SPL vault, and reclaims the market slab. A rotated-out
// market authority must not be able to replay this final close after handoff.
#[test]
fn v16_attack_close_slab_rejects_stale_marketauth_after_rotation() {
    let mut env = V16CuEnv::new();
    let old_admin = env.admin.insecure_clone();
    let new_admin = Keypair::new();
    env.update_asset_authority_with_cu(&new_admin);
    assert_eq!(
        env.market_state().0.marketauth,
        new_admin.pubkey().to_bytes(),
        "test setup rotates marketauth away from the old key"
    );

    env.svm.expire_blockhash();
    let resolve = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ResolveMarket {
            asset_generation_frontier: 0,
        },
        vec![
            AccountMeta::new(new_admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&new_admin],
    );
    assert!(
        resolve.is_ok(),
        "the rotated-in market authority can resolve before final close: {resolve:?}"
    );
    env.set_token_account_amount(env.vault, env.mint, env.vault_authority, 7);

    let stale_dest = env.token_account(old_admin.pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let stale_dest_before = env.svm.get_account(&stale_dest).unwrap();

    env.svm.expire_blockhash();
    let stale_close = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::CloseSlab,
        vec![
            AccountMeta::new(old_admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new(stale_dest, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&old_admin],
    );
    assert!(
        stale_close.is_err(),
        "rotated-out marketauth must not reclaim the final slab"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "stale CloseSlab must not zero or reclaim the market slab"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "stale CloseSlab must not transfer or close the primary vault"
    );
    assert_eq!(
        env.svm.get_account(&stale_dest).unwrap(),
        stale_dest_before,
        "stale CloseSlab must not send dust to the old authority"
    );

    let new_dest = env.token_account(new_admin.pubkey(), 0);
    env.svm.expire_blockhash();
    let good_close = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::CloseSlab,
        vec![
            AccountMeta::new(new_admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new(new_dest, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&new_admin],
    );
    assert!(
        good_close.is_ok(),
        "rotated-in marketauth can still reclaim the final slab: {good_close:?}"
    );
    assert_eq!(
        env.token_amount(new_dest),
        7,
        "final vault dust recovered by the current market authority"
    );
    let closed_market = env.svm.get_account(&env.market).unwrap();
    assert_eq!(
        closed_market.lamports, 0,
        "market lamports reclaimed after current-authority CloseSlab"
    );
    assert!(
        closed_market.data.iter().all(|b| *b == 0),
        "market data zeroed only after current-authority CloseSlab"
    );
}
