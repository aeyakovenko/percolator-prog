//! INV-089 - activation, reactivation, and initialization equivalence.
//!
//! Retired-slot reuse must apply the same safety envelope as fresh activation:
//! valid nonzero authorities, valid initial price, authenticated slot, fresh asset
//! generation, empty backing/source-credit ledgers, and no stale-authority carryover.
//! These tests exercise the public `UpdateAssetLifecycle` route only.

use super::*;

#[test]
fn v16_program_reuse_rejects_every_zero_authority_before_mutation() {
    let creator = Keypair::new();
    let valid = creator.pubkey().to_bytes();
    for (label, insurance, operator, backing, oracle) in [
        (
            "zero insurance authority",
            Pubkey::default().to_bytes(),
            valid,
            valid,
            valid,
        ),
        (
            "zero insurance operator",
            valid,
            Pubkey::default().to_bytes(),
            valid,
            valid,
        ),
        (
            "zero backing authority",
            valid,
            valid,
            Pubkey::default().to_bytes(),
            valid,
        ),
        (
            "zero oracle authority",
            valid,
            valid,
            valid,
            Pubkey::default().to_bytes(),
        ),
    ] {
        let mut env = V16CuEnv::new();
        env.update_market_init_fee_policy_with_cu(1);
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
        env.svm.warp_to_slot(3);
        env.update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_RETIRE, 1, 3, 0);
        let market_before = env.svm.get_account(&env.market).unwrap();
        let vault_before = env.svm.get_account(&env.vault).unwrap();
        let source = env.token_account(creator.pubkey(), 1);
        let source_before = env.svm.get_account(&source).unwrap();

        env.svm.warp_to_slot(4);
        env.svm.expire_blockhash();
        let rejected = env.send(
            ProgInstruction::UpdateAssetLifecycle {
                action: processor::ASSET_ACTION_ACTIVATE,
                asset_index: 1,
                now_slot: 4,
                initial_price: 250,
                max_init_fee: u128::MAX,
                insurance_authority: insurance,
                insurance_operator: operator,
                backing_bucket_authority: backing,
                oracle_authority: oracle,
            },
            vec![
                AccountMeta::new(creator.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&creator],
        );
        assert!(
            rejected.is_err(),
            "{label} must reject on retired-slot reuse"
        );
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_before,
            "{label} must not consume the reusable slot or rewrite authorities",
        );
        assert_eq!(
            env.svm.get_account(&env.vault).unwrap(),
            vault_before,
            "{label} must not transfer the activation fee",
        );
        assert_eq!(
            env.svm.get_account(&source).unwrap(),
            source_before,
            "{label} must leave the creator's source token untouched",
        );

        env.svm.warp_to_slot(4);
        env.activate_permissionless_asset_with_fee(
            &creator,
            1,
            4,
            250,
            creator.pubkey(),
            creator.pubkey(),
            creator.pubkey(),
            creator.pubkey(),
            1,
        );
        assert_eq!(
            env.market_state().1.assets[1].lifecycle,
            AssetLifecycleV16::Active
        );
    }
}

#[test]
fn v16_program_reuse_matches_fresh_activation_envelope_and_drops_old_authority() {
    let fresh_creator = Keypair::new();
    let mut fresh_env = V16CuEnv::new();
    fresh_env.update_market_init_fee_policy_with_cu(1);
    fresh_env.svm.warp_to_slot(5);
    fresh_env.activate_permissionless_asset_with_fee(
        &fresh_creator,
        1,
        5,
        250,
        fresh_creator.pubkey(),
        fresh_creator.pubkey(),
        fresh_creator.pubkey(),
        fresh_creator.pubkey(),
        1,
    );

    let old_creator = Keypair::new();
    let new_creator = Keypair::new();
    let mut reused_env = V16CuEnv::new();
    reused_env.update_market_init_fee_policy_with_cu(1);
    reused_env.svm.warp_to_slot(1);
    reused_env.activate_permissionless_asset_with_fee(
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
    let old_market_id = reused_env.market_state().1.assets[1].market_id;
    reused_env.svm.warp_to_slot(3);
    reused_env.update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_RETIRE, 1, 3, 0);
    reused_env.svm.warp_to_slot(5);
    reused_env.activate_permissionless_asset_with_fee(
        &new_creator,
        1,
        5,
        250,
        new_creator.pubkey(),
        new_creator.pubkey(),
        new_creator.pubkey(),
        new_creator.pubkey(),
        1,
    );

    let (_, fresh_group) = fresh_env.market_state();
    let (_, reused_group) = reused_env.market_state();
    let fresh_asset = fresh_group.assets[1];
    let reused_asset = reused_group.assets[1];
    assert_ne!(
        reused_asset.market_id, old_market_id,
        "retired-slot reuse must assign a fresh asset generation",
    );
    assert_eq!(reused_asset.lifecycle, AssetLifecycleV16::Active);
    assert_eq!(fresh_asset.lifecycle, reused_asset.lifecycle);
    assert_eq!(fresh_asset.effective_price, reused_asset.effective_price);
    assert_eq!(
        fresh_asset.raw_oracle_target_price,
        reused_asset.raw_oracle_target_price
    );
    assert_eq!(fresh_asset.fund_px_last, reused_asset.fund_px_last);
    assert_eq!(fresh_asset.slot_last, reused_asset.slot_last);
    assert_eq!(reused_asset.oi_eff_long_q, 0);
    assert_eq!(reused_asset.oi_eff_short_q, 0);
    assert_eq!(
        reused_group.source_backing_buckets[2].status,
        BackingBucketStatusV16::Empty
    );
    assert_eq!(
        reused_group.source_backing_buckets[3].status,
        BackingBucketStatusV16::Empty
    );
    assert_eq!(
        reused_group.source_backing_buckets[2].fresh_unliened_backing_num,
        0
    );
    assert_eq!(
        reused_group.source_backing_buckets[3].fresh_unliened_backing_num,
        0
    );
    assert_eq!(
        reused_group.vault as u64,
        reused_env.token_amount(reused_env.vault)
    );

    let reused_data = reused_env.svm.get_account(&reused_env.market).unwrap().data;
    let reused_profile = state::read_asset_oracle_profile(&reused_data, 1).unwrap();
    assert_eq!(reused_profile.asset_admin, new_creator.pubkey().to_bytes());
    assert_eq!(
        reused_profile.insurance_authority,
        new_creator.pubkey().to_bytes()
    );
    assert_eq!(
        reused_profile.insurance_operator,
        new_creator.pubkey().to_bytes()
    );
    assert_eq!(
        reused_profile.backing_bucket_authority,
        new_creator.pubkey().to_bytes()
    );
    assert_eq!(
        reused_profile.oracle_authority,
        new_creator.pubkey().to_bytes()
    );

    reused_env.ensure_signer_account(old_creator.pubkey());
    let before = reused_env.svm.get_account(&reused_env.market).unwrap();
    reused_env.svm.expire_blockhash();
    let stale_oracle = reused_env.send(
        ProgInstruction::ConfigureAuthMark {
            market_id: reused_asset.market_id,
            observation_sequence: u64::MAX,
            asset_index: 1,
            now_slot: 6,
            initial_mark_e6: 300,
        },
        vec![
            AccountMeta::new(old_creator.pubkey(), true),
            AccountMeta::new(reused_env.market, false),
        ],
        &[&old_creator],
    );
    assert!(
        stale_oracle.is_err(),
        "old authority from the retired generation must not control the reused slot",
    );
    assert_eq!(
        reused_env.svm.get_account(&reused_env.market).unwrap(),
        before
    );

    reused_env.ensure_signer_account(new_creator.pubkey());
    reused_env.svm.expire_blockhash();
    reused_env
        .send(
            ProgInstruction::ConfigureAuthMark {
                market_id: reused_asset.market_id,
                observation_sequence: u64::MAX,
                asset_index: 1,
                now_slot: 6,
                initial_mark_e6: 300,
            },
            vec![
                AccountMeta::new(new_creator.pubkey(), true),
                AccountMeta::new(reused_env.market, false),
            ],
            &[&new_creator],
        )
        .expect("new authority controls the reused slot");
}

#[test]
fn v16_attack_marketauth_cannot_reactivate_or_rekey_active_slot_with_open_interest() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 5_000, 10_000, 1_000);
    let admin = env.admin.insecure_clone();
    env.activate_asset(1, 1, 100);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 1_000_000);
    env.trade_asset_with_cu(
        1,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        100,
        0,
    );

    let market_before = env.svm.get_account(&env.market).unwrap();
    let long_before = env.svm.get_account(&long_account).unwrap();
    let short_before = env.svm.get_account(&short_account).unwrap();
    let before_profile = state::read_asset_oracle_profile(&market_before.data, 1).unwrap();
    let (_, before_group) = env.market_state();
    assert_eq!(before_group.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(before_group.assets[1].oi_eff_long_q, POS_SCALE);
    assert_eq!(before_group.assets[1].oi_eff_short_q, POS_SCALE);

    let new_insurance = Keypair::new();
    let new_operator = Keypair::new();
    let new_backing = Keypair::new();
    let new_oracle = Keypair::new();
    env.svm.warp_to_slot(2);
    env.svm.expire_blockhash();
    let reactivation = env.send(
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
            now_slot: 2,
            initial_price: 999,
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
    );
    assert!(
        reactivation.is_err(),
        "marketauth must not reactivate/rekey an active slot with live open interest"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected active-slot reactivation must leave market bytes unchanged"
    );
    assert_eq!(env.svm.get_account(&long_account).unwrap(), long_before);
    assert_eq!(env.svm.get_account(&short_account).unwrap(), short_before);

    let market_after = env.svm.get_account(&env.market).unwrap();
    let after_profile = state::read_asset_oracle_profile(&market_after.data, 1).unwrap();
    assert_eq!(
        after_profile, before_profile,
        "rejected active-slot reactivation must not install new authorities or reset oracle state"
    );
    let (_, after_group) = env.market_state();
    assert_eq!(after_group.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(after_group.assets[1].effective_price, 100);
    assert_eq!(after_group.assets[1].oi_eff_long_q, POS_SCALE);
    assert_eq!(after_group.assets[1].oi_eff_short_q, POS_SCALE);
}

#[test]
fn v16_attack_privileged_reactivate_invalid_price_keeps_retired_slot_reusable() {
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
    env.svm.warp_to_slot(3);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_RETIRE,
        1,
        3,
        0,
    );

    let market_before = env.svm.get_account(&env.market).unwrap();
    let profile_before = state::read_asset_oracle_profile(&market_before.data, 1).unwrap();
    let (cfg_before, group_before) = state::read_market(&market_before.data).unwrap();
    assert_eq!(cfg_before.free_market_slot_count, 1);
    assert_eq!(group_before.assets[1].lifecycle, AssetLifecycleV16::Retired);

    for (label, bad_price) in [
        ("zero privileged reactivation price", 0),
        (
            "privileged reactivation price above MAX_ORACLE_PRICE",
            percolator::MAX_ORACLE_PRICE + 1,
        ),
    ] {
        env.svm.warp_to_slot(4);
        env.svm.expire_blockhash();
        let rejected = env.send(
            ProgInstruction::UpdateAssetLifecycle {
                action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
                asset_index: 1,
                now_slot: 4,
                initial_price: bad_price,
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
        );
        assert!(rejected.is_err(), "{label} must reject");
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_before,
            "{label} must not consume the retired slot or overwrite authorities"
        );
        let market_after = env.svm.get_account(&env.market).unwrap();
        let profile_after = state::read_asset_oracle_profile(&market_after.data, 1).unwrap();
        let (cfg_after, group_after) = state::read_market(&market_after.data).unwrap();
        assert_eq!(profile_after, profile_before);
        assert_eq!(cfg_after.free_market_slot_count, 1);
        assert_eq!(group_after.assets[1].lifecycle, AssetLifecycleV16::Retired);
        assert_eq!(
            group_after.assets[1].market_id,
            group_before.assets[1].market_id
        );
    }

    env.svm.warp_to_slot(4);
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
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
    .expect("valid privileged reactivation still succeeds after rejected bad prices");

    let market_after = env.svm.get_account(&env.market).unwrap();
    let profile_after = state::read_asset_oracle_profile(&market_after.data, 1).unwrap();
    let (cfg_after, group_after) = state::read_market(&market_after.data).unwrap();
    assert_eq!(cfg_after.free_market_slot_count, 0);
    assert_eq!(group_after.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(group_after.assets[1].effective_price, 250);
    assert_eq!(
        profile_after.asset_admin,
        admin.pubkey().to_bytes(),
        "valid privileged reactivation must install admin ownership only after valid pricing"
    );
    assert_eq!(
        profile_after.insurance_authority,
        new_insurance.pubkey().to_bytes()
    );
    assert_eq!(
        profile_after.insurance_operator,
        new_operator.pubkey().to_bytes()
    );
    assert_eq!(
        profile_after.backing_bucket_authority,
        new_backing.pubkey().to_bytes()
    );
    assert_eq!(
        profile_after.oracle_authority,
        new_oracle.pubkey().to_bytes()
    );
}

// security.md sweep - retired-slot reuse invalid oracle price rollback (#24/#35/#48):
// permissionless reuse is a separate branch from append. A bad initial price must not consume the
// reusable-slot counter, overwrite the canonical retired slot, or pull the creator's init fee.
#[test]
fn v16_attack_permissionless_reuse_invalid_price_keeps_slot_reusable() {
    const FEE: u128 = 40;
    let mut env = V16CuEnv::new();
    let creator = Keypair::new();
    env.ensure_signer_account(creator.pubkey());
    env.update_market_init_fee_policy_with_cu(FEE);

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
        FEE,
    );
    env.svm.warp_to_slot(3);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_RETIRE,
        1,
        3,
        0,
    );
    let (cfg_retired, group_retired) = env.market_state();
    assert_eq!(cfg_retired.free_market_slot_count, 1);
    assert_eq!(
        group_retired.assets[1].lifecycle,
        AssetLifecycleV16::Retired
    );
    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    for (label, bad_price) in [
        ("zero reuse price", 0),
        (
            "reuse price above MAX_ORACLE_PRICE",
            percolator::MAX_ORACLE_PRICE + 1,
        ),
    ] {
        let source = env.token_account(creator.pubkey(), FEE as u64);
        let source_before = env.svm.get_account(&source).unwrap();
        env.svm.warp_to_slot(4);
        env.svm.expire_blockhash();
        let rejected = env.send(
            ProgInstruction::UpdateAssetLifecycle {
                action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
                asset_index: 1,
                now_slot: 4,
                initial_price: bad_price,
                max_init_fee: u128::MAX,
                insurance_authority: creator.pubkey().to_bytes(),
                insurance_operator: creator.pubkey().to_bytes(),
                backing_bucket_authority: creator.pubkey().to_bytes(),
                oracle_authority: creator.pubkey().to_bytes(),
            },
            vec![
                AccountMeta::new(creator.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&creator],
        );
        assert!(rejected.is_err(), "{label} must reject");
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_before,
            "{label} must leave the retired slot and reusable counter byte-identical"
        );
        assert_eq!(
            env.svm.get_account(&env.vault).unwrap(),
            vault_before,
            "{label} must not move canonical vault custody"
        );
        assert_eq!(
            env.svm.get_account(&source).unwrap(),
            source_before,
            "{label} must not pull the creator's reuse fee"
        );
        let (cfg_after, group_after) = env.market_state();
        assert_eq!(cfg_after.free_market_slot_count, 1);
        assert_eq!(group_after.assets[1].lifecycle, AssetLifecycleV16::Retired);
    }

    let valid_source = env.token_account(creator.pubkey(), FEE as u64);
    env.svm.warp_to_slot(4);
    env.svm.expire_blockhash();
    let accepted = env.send(
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
            now_slot: 4,
            initial_price: 250,
            max_init_fee: u128::MAX,
            insurance_authority: creator.pubkey().to_bytes(),
            insurance_operator: creator.pubkey().to_bytes(),
            backing_bucket_authority: creator.pubkey().to_bytes(),
            oracle_authority: creator.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(valid_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&creator],
    );
    assert!(
        accepted.is_ok(),
        "valid permissionless reuse should still succeed after rejected bad prices: {accepted:?}"
    );
    let (cfg_reused, group_reused) = env.market_state();
    assert_eq!(cfg_reused.free_market_slot_count, 0);
    assert_eq!(group_reused.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(group_reused.assets[1].effective_price, 250);
    assert_eq!(env.token_amount(valid_source), 0);
}

// ConfigurePermissionlessResolve gating + input bounds. Sets the resolve timer (too short -> premature
// permissionless resolution winding users out on a brief oracle blip; absurd values -> stuck). The non-
// admin gating wasn't tested (v16_attack_non_admin_cannot_resolve_or_configure covers ResolveMarket +
// ConfigureAuthMark only), nor the stale_slots==0 / >MAX bounds.
// security.md sweep - stale-resolve config drift (#30/#48): once the market is already old enough
// for ResolveStalePermissionless, marketauth must not be able to move the resolve threshold forward.
// Otherwise the fallback can be DoSed in the stale window by reconfiguring stale_slots before the
// Asset-activation cooldown (anti-churn / anti-bloat) is engine-enforced (elapsed < asset_activation_cooldown_
// slots -> LockActive, v16.rs ~4954) but UNGUARDED by tests -- v16_attack_market_exceeds_64_assets works AROUND
// it (advances slots) rather than asserting the rejection. Without the cooldown an attacker could churn assets
// rapidly (append/reuse) to bloat the slot count faster. Default cooldown = 1 slot.
// security.md sweep - retired-slot reuse must respect the same activation cooldown as append
// (#30/#48). Otherwise an attacker could churn a single reusable slot through market ids/epochs in a
// tight loop. Rejected permissionless reuse must also leave the creator's init-fee source untouched.
#[test]
fn v16_attack_permissionless_reuse_respects_activation_cooldown_and_fee_atomicity() {
    const FEE: u128 = 40;
    let mut env = V16CuEnv::new();
    let creator = Keypair::new();
    env.ensure_signer_account(creator.pubkey());
    env.update_market_init_fee_policy_with_cu(FEE);

    env.svm.warp_to_slot(5);
    env.activate_asset(1, 5, 100);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_RETIRE,
        1,
        5,
        0,
    );
    let (cfg_retired, retired_group) = env.market_state();
    assert_eq!(
        retired_group.assets[1].lifecycle,
        AssetLifecycleV16::Retired,
        "slot is reusable after same-slot retire"
    );
    assert_eq!(cfg_retired.free_market_slot_count, 1);

    let source = env.token_account(creator.pubkey(), FEE as u64);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let source_before = env.svm.get_account(&source).unwrap();

    env.svm.expire_blockhash();
    let same_slot_reuse = env.send(
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
            now_slot: 5,
            initial_price: 250,
            max_init_fee: u128::MAX,
            insurance_authority: creator.pubkey().to_bytes(),
            insurance_operator: creator.pubkey().to_bytes(),
            backing_bucket_authority: creator.pubkey().to_bytes(),
            oracle_authority: creator.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&creator],
    );
    assert!(
        same_slot_reuse.is_err(),
        "permissionless retired-slot reuse in the activation cooldown window must reject"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected cooldown reuse must not reactivate the retired slot"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected cooldown reuse must not credit the vault"
    );
    assert_eq!(
        env.svm.get_account(&source).unwrap(),
        source_before,
        "rejected cooldown reuse must not pull the creator's fee"
    );

    env.svm.warp_to_slot(6);
    let ok_source = env.token_account(creator.pubkey(), FEE as u64);
    env.svm.expire_blockhash();
    let reuse_after_cooldown = env.send(
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
            now_slot: 6,
            initial_price: 250,
            max_init_fee: u128::MAX,
            insurance_authority: creator.pubkey().to_bytes(),
            insurance_operator: creator.pubkey().to_bytes(),
            backing_bucket_authority: creator.pubkey().to_bytes(),
            oracle_authority: creator.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ok_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&creator],
    );
    assert!(
        reuse_after_cooldown.is_ok(),
        "permissionless reuse should succeed once the cooldown elapses: {reuse_after_cooldown:?}"
    );
    let (cfg_after, group_after) = env.market_state();
    assert_eq!(cfg_after.free_market_slot_count, 0);
    assert_eq!(group_after.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(group_after.assets[1].effective_price, 250);
    assert_eq!(env.token_amount(ok_source), 0);
    assert_eq!(env.token_amount(env.vault), FEE as u64);
    assert_eq!(group_after.vault, FEE);
    assert_eq!(group_after.insurance, FEE);
    assert_eq!(
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 1)
            .unwrap()
            .asset_admin,
        creator.pubkey().to_bytes(),
        "successful reuse installs fresh creator-scoped authorities"
    );
}

// security.md sweep — permissionless asset-create fee gate (#5 / README L52): when the configured
// permissionless market-init fee is ZERO, asset creation is NOT permissionless — only the market-wide
// asset authority may append a new asset; a stranger is rejected with Unauthorized.
#[test]
fn v16_attack_permissionless_create_requires_nonzero_fee() {
    let mut env = V16CuEnv::new(); // default permissionless_market_init_fee == 0
    let market = env.market;
    env.svm.warp_to_slot(1);
    let append = |env: &mut V16CuEnv, signer: &Keypair| -> Result<u64, String> {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::UpdateAssetLifecycle {
                action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
                asset_index: 1,
                now_slot: 1,
                initial_price: 100,
                max_init_fee: u128::MAX,
                insurance_authority: signer.pubkey().to_bytes(),
                insurance_operator: signer.pubkey().to_bytes(),
                backing_bucket_authority: signer.pubkey().to_bytes(),
                oracle_authority: signer.pubkey().to_bytes(),
            },
            vec![
                AccountMeta::new(signer.pubkey(), true),
                AccountMeta::new(market, false),
            ],
            &[signer],
        )
    };
    let stranger = Keypair::new();
    env.ensure_signer_account(stranger.pubkey());
    assert!(
        append(&mut env, &stranger).is_err(),
        "fee=0: a stranger must NOT be able to append an asset"
    );
    let (_, g_after_reject) = env.market_state();
    assert_ne!(
        g_after_reject.assets.get(1).map(|a| a.lifecycle),
        Some(AssetLifecycleV16::Active),
        "no asset created by the rejected stranger"
    );
    // The market-wide asset authority (admin) appends for free.
    let admin = env.admin.insecure_clone();
    append(&mut env, &admin).expect("the asset authority may append for free when fee=0");
    assert_eq!(
        env.market_state().1.assets[1].lifecycle,
        AssetLifecycleV16::Active,
        "authority append activated asset 1"
    );
}

// security.md sweep — permissionless create fee preflight (#5 / README L59): an underfunded
// permissionless creator must not grow the market, install asset authorities, or credit market-0
// insurance. The funded control proves the rejection is the fee gate, not a dead activation path.
#[test]
fn v16_attack_permissionless_create_underfunded_fee_does_not_activate_or_credit() {
    const FEE: u128 = 40;
    let mut env = V16CuEnv::new();
    env.update_market_init_fee_policy_with_cu(FEE);
    env.svm.warp_to_slot(1);

    let creator = Keypair::new();
    env.ensure_signer_account(creator.pubkey());
    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let (_, group_before) = env.market_state();
    assert_eq!(
        group_before.config.max_market_slots, 1,
        "starts as a one-asset market"
    );

    let underfunded_source = env.token_account(creator.pubkey(), FEE as u64 - 1);
    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
            now_slot: 1,
            initial_price: 100,
            max_init_fee: u128::MAX,
            insurance_authority: creator.pubkey().to_bytes(),
            insurance_operator: creator.pubkey().to_bytes(),
            backing_bucket_authority: creator.pubkey().to_bytes(),
            oracle_authority: creator.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(underfunded_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&creator],
    );
    assert!(
        rejected.is_err(),
        "underfunded permissionless init fee must reject"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "market account unchanged by rejected create"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "vault token account unchanged by rejected create"
    );
    assert_eq!(
        env.token_amount(underfunded_source),
        FEE as u64 - 1,
        "rejected create pulls no fee"
    );
    let (_, rejected_group) = env.market_state();
    assert_eq!(
        rejected_group.config.max_market_slots, 1,
        "rejected create did not append a slot"
    );
    assert_eq!(
        rejected_group.insurance, group_before.insurance,
        "rejected create did not credit insurance"
    );
    assert_eq!(
        rejected_group.vault, group_before.vault,
        "rejected create did not credit accounting vault"
    );
    assert_eq!(
        rejected_group.insurance_domain_budget,
        group_before.insurance_domain_budget
    );
    assert_domain_budget_remaining_total_consistent(&rejected_group, "underfunded create rollback");

    let funded_source = env.token_account(creator.pubkey(), FEE as u64);
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
            now_slot: 1,
            initial_price: 100,
            max_init_fee: u128::MAX,
            insurance_authority: creator.pubkey().to_bytes(),
            insurance_operator: creator.pubkey().to_bytes(),
            backing_bucket_authority: creator.pubkey().to_bytes(),
            oracle_authority: creator.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(funded_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&creator],
    )
    .expect("properly funded permissionless create succeeds");
    let (_, funded_group) = env.market_state();
    assert_eq!(
        env.token_amount(funded_source),
        0,
        "funded create pulls the fee"
    );
    assert_eq!(
        funded_group.config.max_market_slots, 2,
        "funded create appends exactly one slot"
    );
    assert_eq!(funded_group.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(
        funded_group.insurance - group_before.insurance,
        FEE,
        "fee credited to market-0 insurance"
    );
    assert_eq!(
        funded_group.vault - group_before.vault,
        FEE,
        "fee credited to accounting vault"
    );
    assert_domain_budget_remaining_total_consistent(&funded_group, "funded permissionless create");
}

// security.md sweep — dynamic append rollback (#44/#48): the append path reallocs the market account
// before activate_dynamic_asset_slot rejects zero authorities. A rejected append must roll back account
// length, slot counters, fee collection, and all market-0 budget credits.
#[test]
fn v16_attack_permissionless_append_zero_authority_rolls_back_realloc_and_fee() {
    const FEE: u128 = 40;
    let mut env = V16CuEnv::new();
    env.update_market_init_fee_policy_with_cu(FEE);
    env.svm.warp_to_slot(1);

    let creator = Keypair::new();
    env.ensure_signer_account(creator.pubkey());
    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let source = env.token_account(creator.pubkey(), FEE as u64);
    let (_, group_before) = env.market_state();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
            now_slot: 1,
            initial_price: 100,
            max_init_fee: u128::MAX,
            insurance_authority: [0u8; 32],
            insurance_operator: creator.pubkey().to_bytes(),
            backing_bucket_authority: creator.pubkey().to_bytes(),
            oracle_authority: creator.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&creator],
    );
    assert!(
        rejected.is_err(),
        "permissionless append with a zero authority must reject"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected append rolls back the pre-write market realloc"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "vault token account unchanged"
    );
    assert_eq!(
        env.token_amount(source),
        FEE as u64,
        "fee was not pulled by rejected append"
    );
    let (_, rejected_group) = env.market_state();
    assert_eq!(
        rejected_group.config.max_market_slots,
        group_before.config.max_market_slots
    );
    assert_eq!(rejected_group.insurance, group_before.insurance);
    assert_eq!(rejected_group.vault, group_before.vault);
    assert_eq!(
        rejected_group.insurance_domain_budget,
        group_before.insurance_domain_budget
    );

    let valid_source = env.token_account(creator.pubkey(), FEE as u64);
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
            now_slot: 1,
            initial_price: 100,
            max_init_fee: u128::MAX,
            insurance_authority: creator.pubkey().to_bytes(),
            insurance_operator: creator.pubkey().to_bytes(),
            backing_bucket_authority: creator.pubkey().to_bytes(),
            oracle_authority: creator.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(valid_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&creator],
    )
    .expect("valid permissionless append succeeds after rejected zero-authority attempt");
    let (_, valid_group) = env.market_state();
    assert_eq!(env.token_amount(valid_source), 0, "valid append pulls fee");
    assert_eq!(
        valid_group.config.max_market_slots,
        group_before.config.max_market_slots + 1
    );
    assert_eq!(valid_group.assets[1].lifecycle, AssetLifecycleV16::Active);
}

// security.md sweep - permissionless append invalid oracle price rollback (#5/#24/#44/#48):
// asset activation accepts a caller-supplied initial oracle price and reaches the dynamic append
// path after validating the fee accounts. A zero or over-MAX price must not install an unusable
// asset, grow the market, or pull the permissionless init fee.
#[test]
fn v16_attack_permissionless_append_invalid_price_rolls_back_realloc_and_fee() {
    const FEE: u128 = 40;
    let mut env = V16CuEnv::new();
    env.update_market_init_fee_policy_with_cu(FEE);
    env.svm.warp_to_slot(1);

    let creator = Keypair::new();
    env.ensure_signer_account(creator.pubkey());
    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let (_, group_before) = env.market_state();

    for (label, bad_price) in [
        ("zero initial price", 0),
        (
            "initial price above MAX_ORACLE_PRICE",
            percolator::MAX_ORACLE_PRICE + 1,
        ),
    ] {
        let source = env.token_account(creator.pubkey(), FEE as u64);
        let source_before = env.svm.get_account(&source).unwrap();
        env.svm.expire_blockhash();
        let rejected = env.send(
            ProgInstruction::UpdateAssetLifecycle {
                action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
                asset_index: 1,
                now_slot: 1,
                initial_price: bad_price,
                max_init_fee: u128::MAX,
                insurance_authority: creator.pubkey().to_bytes(),
                insurance_operator: creator.pubkey().to_bytes(),
                backing_bucket_authority: creator.pubkey().to_bytes(),
                oracle_authority: creator.pubkey().to_bytes(),
            },
            vec![
                AccountMeta::new(creator.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&creator],
        );
        assert!(rejected.is_err(), "{label} must reject");
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_before,
            "{label} must roll back the dynamic-append market realloc"
        );
        assert_eq!(
            env.svm.get_account(&env.vault).unwrap(),
            vault_before,
            "{label} must not move canonical vault custody"
        );
        assert_eq!(
            env.svm.get_account(&source).unwrap(),
            source_before,
            "{label} must not pull the creator's init fee"
        );
        let (_, rejected_group) = env.market_state();
        assert_eq!(
            rejected_group.config.max_market_slots, group_before.config.max_market_slots,
            "{label} must not append a market slot"
        );
        assert_eq!(
            rejected_group.insurance, group_before.insurance,
            "{label} must not credit market insurance"
        );
        assert_eq!(
            rejected_group.vault, group_before.vault,
            "{label} must not credit accounting vault"
        );
    }

    let valid_source = env.token_account(creator.pubkey(), FEE as u64);
    env.svm.expire_blockhash();
    let accepted = env.send(
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
            now_slot: 1,
            initial_price: 100,
            max_init_fee: u128::MAX,
            insurance_authority: creator.pubkey().to_bytes(),
            insurance_operator: creator.pubkey().to_bytes(),
            backing_bucket_authority: creator.pubkey().to_bytes(),
            oracle_authority: creator.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(valid_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&creator],
    );
    assert!(
        accepted.is_ok(),
        "valid permissionless append still succeeds after rejected bad prices: {accepted:?}"
    );
    let (_, group_after) = env.market_state();
    assert_eq!(group_after.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(group_after.assets[1].effective_price, 100);
    assert_eq!(env.token_amount(valid_source), 0);
}

#[test]
fn v16_bpf_restart_asset_oracle_is_uniform_for_local_asset_admins() {
    let mut env = V16CuEnv::new();
    let marketauth = env.admin.insecure_clone();
    let asset0_admin = Keypair::new();
    let creator = Keypair::new();
    let creator_pubkey = creator.pubkey();
    env.configure_permissionless_resolve_with_cu(100, 5);
    env.configure_auth_mark_with_cu(0, 100);
    env.try_update_per_asset_authority_with_cu(
        &marketauth,
        Some(&asset0_admin),
        0,
        processor::ASSET_AUTH_ADMIN,
        asset0_admin.pubkey().to_bytes(),
    )
    .expect("asset-0 admin rotates to local key");

    env.svm.warp_to_slot(2);
    env.svm.expire_blockhash();
    env.try_shutdown_asset_with_authority(&asset0_admin, 0, 2)
        .expect("local asset-0 admin can shut down asset 0");
    assert_eq!(
        env.market_state().1.assets[0].lifecycle,
        AssetLifecycleV16::Recovery
    );
    assert!(
        env.try_restart_asset_oracle_with_authority(&marketauth, 0, 3, 111)
            .is_err(),
        "marketauth cannot restart asset 0 after asset_admin is delegated"
    );
    env.svm.warp_to_slot(3);
    env.try_restart_asset_oracle_with_authority(&asset0_admin, 0, 3, 111)
        .expect("local asset-0 admin can restart empty asset 0");
    let asset0_profile =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap();
    assert_eq!(asset0_profile.asset_admin, asset0_admin.pubkey().to_bytes());
    assert_eq!(env.market_state().1.assets[0].effective_price, 111);

    let mut env = V16CuEnv::new();
    let marketauth = env.admin.insecure_clone();
    env.configure_permissionless_resolve_with_cu(100, 5);
    env.update_market_init_fee_policy_with_cu(10);
    env.activate_permissionless_asset_with_fee(
        &creator,
        1,
        4,
        200,
        creator_pubkey,
        creator_pubkey,
        creator_pubkey,
        creator_pubkey,
        10,
    );
    let asset1_before = env.market_state().1.assets[1].market_id;
    env.svm.warp_to_slot(5);
    env.svm.expire_blockhash();
    env.try_shutdown_asset_with_authority(&creator, 1, 5)
        .expect("permissionless asset creator/admin can shut down its own asset");
    assert_eq!(
        env.market_state().1.assets[1].lifecycle,
        AssetLifecycleV16::Recovery
    );
    assert!(
        env.try_restart_asset_oracle_with_authority(&marketauth, 1, 6, 250)
            .is_err(),
        "marketauth can force-shutdown but cannot restart another admin's asset"
    );
    env.svm.warp_to_slot(6);
    env.try_restart_asset_oracle_with_authority(&creator, 1, 6, 250)
        .expect("permissionless asset admin can restart empty own asset");
    let data = env.svm.get_account(&env.market).unwrap().data;
    let (_, group) = state::read_market(&data).unwrap();
    let asset1_profile = state::read_asset_oracle_profile(&data, 1).unwrap();
    assert_eq!(group.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(group.assets[1].effective_price, 250);
    assert_ne!(
        group.assets[1].market_id, asset1_before,
        "restart assigns a fresh market id"
    );
    assert_eq!(asset1_profile.asset_admin, creator_pubkey.to_bytes());
    assert_eq!(asset1_profile.oracle_authority, creator_pubkey.to_bytes());
    assert_eq!(
        market_engine_slot_bytes(&data, 1),
        bytemuck::bytes_of(&canonical_active_engine_slot(
            group.assets[1].market_id,
            250,
            6,
            group.insurance_domain_budget[2],
            group.insurance_domain_budget[3],
        )),
        "nonzero restart leaves a canonical fresh engine slot with only insurance budgets preserved",
    );
}

// Coverage probe (audit, Finding F): the permissionless retired-slot REUSE branch
// of UpdateAssetLifecycle (v16_program.rs:8651) writes the four domain authorities
// straight from caller args with NO zero-check, unlike the append path which
// rejects zero authorities (v16_program.rs:1475). A permissionless creator can
// reuse a retired slot with insurance_authority = 0; fees later accrued to that
// asset's domain are withdrawable by nobody (terminal_insurance_remaining rejects
// a zero authority) -> CloseSlab permanently bricked. This asserts the CORRECT
// behavior (reuse with a zero authority is REJECTED).
// GREEN regression: Finding F fixed in the wrapper reuse branch (v16_program.rs:8651
// now rejects zero domain authorities, mirroring the append path).
#[test]
fn v16_audit_permissionless_reuse_rejects_zero_insurance_authority() {
    let mut env = V16CuEnv::new();
    let attacker = Keypair::new();
    env.update_market_init_fee_policy_with_cu(1);

    // Permissionlessly append asset 1 with valid authorities, then retire it so the
    // slot becomes reusable (free_market_slot_count == 1).
    env.svm.warp_to_slot(1);
    env.activate_permissionless_asset_with_fee(
        &attacker,
        1,
        1,
        100,
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        1,
    );
    env.svm.warp_to_slot(3);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_RETIRE,
        1,
        3,
        0,
    );
    let (_, retired_group) = env.market_state();
    assert_eq!(
        retired_group.assets[1].lifecycle,
        AssetLifecycleV16::Retired
    );

    // Reuse the retired slot with insurance_authority = ZERO.
    env.svm.warp_to_slot(4);
    let source = env.token_account(attacker.pubkey(), 1);
    let result = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
            now_slot: 4,
            initial_price: 250,
            max_init_fee: u128::MAX,
            insurance_authority: Pubkey::default().to_bytes(), // ZERO -> unrecoverable
            insurance_operator: attacker.pubkey().to_bytes(),
            backing_bucket_authority: attacker.pubkey().to_bytes(),
            oracle_authority: attacker.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&attacker],
    );
    assert!(
        result.is_err(),
        "reusing a retired slot with a zero insurance_authority must be rejected; accepting it \
         strands that domain's insurance (no authority can withdraw) and permanently bricks CloseSlab",
    );
}
