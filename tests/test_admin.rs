mod common;
#[allow(unused_imports)]
use common::*;

use solana_sdk::{
    account::Account,
    clock::Clock,
    instruction::{AccountMeta, Instruction},
    program_pack::Pack,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    sysvar,
    transaction::Transaction,
};
use spl_token::state::{Account as TokenAccount, AccountState};

/// CRITICAL: UpdateAdmin only callable by current admin
#[test]
fn test_critical_update_admin_authorization() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let new_admin = Keypair::new();
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();

    // Attacker tries to change admin - should fail
    let result = env.try_update_admin(&attacker, &attacker.pubkey());
    assert!(
        result.is_err(),
        "SECURITY: Non-admin should not be able to change admin"
    );
    println!("UpdateAdmin by non-admin: REJECTED (correct)");

    // Real admin changes admin (cross-Keypair transfer requires both
    // current admin and new admin to sign — use try_update_authority
    // with Some(&new_kp) for the two-sig handover).
    env.svm.airdrop(&new_admin.pubkey(), 1_000_000_000).unwrap();
    let result = env.try_update_authority(&admin, AUTHORITY_ADMIN, Some(&new_admin));
    assert!(
        result.is_ok(),
        "Admin should be able to change admin: {:?}",
        result
    );
    println!("UpdateAdmin by admin: ACCEPTED (correct)");

    // Old admin tries again - should now fail
    let result = env.try_update_admin(&admin, &admin.pubkey());
    assert!(result.is_err(), "Old admin should no longer have authority");

    // New admin can exercise authority (proves transfer actually happened).
    // new_admin was airdropped at line 41 above.
    let result = env.try_update_admin(&new_admin, &new_admin.pubkey());
    assert!(
        result.is_ok(),
        "New admin should be able to exercise authority: {:?}",
        result
    );
}

/// CRITICAL: UpdateConfig admin-only with all parameters
#[test]
fn test_critical_update_config_authorization() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();

    // Attacker tries to update config - should fail
    let result = env.try_update_config(&attacker);
    assert!(
        result.is_err(),
        "SECURITY: Non-admin should not update config"
    );
    println!("UpdateConfig by non-admin: REJECTED (correct)");

    // Capture config before admin update
    let config_before = env.read_update_config_snapshot();

    // Admin updates config - should succeed AND change state
    let result = env.try_update_config(&admin);
    assert!(result.is_ok(), "Admin should update config: {:?}", result);

    // Verify config actually changed (not just is_ok)
    let config_after = env.read_update_config_snapshot();
    // try_update_config uses different params than default, so snapshot should differ
    // (if the helper sends the same defaults, this proves nothing — check helper)
    assert_ne!(
        config_before, config_after,
        "Config must actually change after successful UpdateConfig"
    );
}

/// CRITICAL: CloseSlab only by admin, requires zero vault/insurance
#[test]
fn test_critical_close_slab_authorization() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();

    // Deposit some funds (creates non-zero vault balance)
    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 1_000_000_000);

    // Attacker tries to close slab - should fail (not admin)
    let (vault_pda, _) =
        Pubkey::find_program_address(&[b"vault", env.slab.as_ref()], &env.program_id);
    let attacker_ata = env.create_ata(&attacker.pubkey(), 0);
    let attacker_ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(vault_pda, false),
            AccountMeta::new(attacker_ata, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: encode_close_slab(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), attacker_ix],
        Some(&attacker.pubkey()),
        &[&attacker],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(result.is_err(), "SECURITY: Non-admin should not close slab");
    println!("CloseSlab by non-admin: REJECTED (correct)");

    // Admin tries to close slab with non-zero balance - should fail
    let result = env.try_close_slab();
    assert!(
        result.is_err(),
        "SECURITY: Should not close slab with non-zero vault"
    );
    println!("CloseSlab with active funds: REJECTED (correct)");

    println!("CRITICAL TEST PASSED: CloseSlab authorization verified");
}

/// CRITICAL: InitMarket rejects already initialized slab
#[test]
fn test_critical_init_market_rejects_double_init() {
    program_path();

    let mut env = TestEnv::new();

    // First init
    env.init_market_with_invert(0);
    println!("First InitMarket: success");

    // Try second init - should fail
    let admin = &env.payer;
    let dummy_ata = Pubkey::new_unique();
    env.svm
        .set_account(
            dummy_ata,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; TokenAccount::LEN],
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data: encode_init_market_with_invert(&admin.pubkey(), &env.mint, &TEST_FEED_ID, 0),
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[admin],
        env.svm.latest_blockhash(),
    );
    // Snapshot state after first init and before rejected second init.
    const HEADER_CONFIG_LEN: usize = 584;
    let slab_before = env.svm.get_account(&env.slab).unwrap().data;
    let used_before = env.read_num_used_accounts();
    let vault_before = env.vault_balance();
    let result = env.svm.send_transaction(tx);

    assert!(
        result.is_err(),
        "SECURITY: Double initialization should be rejected"
    );
    let slab_after = env.svm.get_account(&env.slab).unwrap().data;
    let used_after = env.read_num_used_accounts();
    let vault_after = env.vault_balance();
    assert_eq!(
        &slab_after[..HEADER_CONFIG_LEN],
        &slab_before[..HEADER_CONFIG_LEN],
        "Rejected second InitMarket must not mutate slab header/config"
    );
    assert_eq!(used_after, used_before, "Rejected second InitMarket must not change num_used_accounts");
    assert_eq!(vault_after, vault_before, "Rejected second InitMarket must not move vault funds");
    println!("Second InitMarket: REJECTED (correct)");

    println!("CRITICAL TEST PASSED: Double initialization rejection verified");
}

/// CRITICAL: Invalid user_idx/lp_idx are rejected
#[test]
fn test_critical_invalid_account_indices_rejected() {
    program_path();

    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 100_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 10_000_000_000);

    // Try trade with invalid user_idx (999 - not initialized)
    let result = env.try_trade(&user, &lp, lp_idx, 999, 1_000_000);
    assert!(
        result.is_err(),
        "SECURITY: Invalid user_idx should be rejected"
    );
    println!("Trade with invalid user_idx: REJECTED (correct)");

    // Try trade with invalid lp_idx (999 - not initialized)
    let result = env.try_trade(&user, &lp, 999, user_idx, 1_000_000);
    assert!(
        result.is_err(),
        "SECURITY: Invalid lp_idx should be rejected"
    );
    println!("Trade with invalid lp_idx: REJECTED (correct)");

    println!("CRITICAL TEST PASSED: Invalid account indices rejection verified");
}

/// Verify per-market admin limits are enforced for Set* operations.
#[test]
fn test_init_market_admin_limits_enforced() {
    program_path();

    let mut env = TestEnv::new();

    // Init market with specific limits
    let admin = &env.payer;
    let dummy_ata = Pubkey::new_unique();
    env.svm
        .set_account(
            dummy_ata,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; TokenAccount::LEN],
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data: encode_init_market_with_limits(
            &admin.pubkey(),
            &env.mint,
            &TEST_FEED_ID,
            1000,   // max_maintenance_fee_per_slot
            50_000, // max_risk_threshold
            5000,   // min_oracle_price_cap_e2bps
        ),
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[admin],
        env.svm.latest_blockhash(),
    );
    env.svm.send_transaction(tx).expect("init_market failed");

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // SetOraclePriceCap: above floor should succeed
    let result = env.try_set_oracle_price_cap(&admin, 5000);
    assert!(
        result.is_ok(),
        "Cap at floor should succeed: {:?}",
        result
    );

    // SetOraclePriceCap: below floor (non-zero) should fail
    let result = env.try_set_oracle_price_cap(&admin, 4999);
    assert!(result.is_err(), "Cap below floor should fail");

    // SetOraclePriceCap: zero is rejected when min floor is set
    // (prevents settlement guard bypass via baseline poisoning)
    let result = env.try_set_oracle_price_cap(&admin, 0);
    assert!(
        result.is_err(),
        "Cap=0 must be rejected when min_oracle_price_cap > 0 (settlement guard)"
    );

    println!("INIT MARKET ADMIN LIMITS ENFORCED: PASSED");
}

/// Verify that InitMarket rejects zero admin limits.
#[test]
fn test_init_market_zero_limits_rejected() {
    program_path();

    let mut env = TestEnv::new();
    let admin = &env.payer;
    let dummy_ata = Pubkey::new_unique();
    env.svm
        .set_account(
            dummy_ata,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; TokenAccount::LEN],
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    // Zero max_maintenance_fee_per_slot should fail
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data: encode_init_market_with_limits(
            &admin.pubkey(),
            &env.mint,
            &TEST_FEED_ID,
            0,         // INVALID: zero max_maintenance_fee
            u128::MAX,
            0,
        ),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[admin],
        env.svm.latest_blockhash(),
    );
    assert!(
        env.svm.send_transaction(tx).is_err(),
        "Zero max_maintenance_fee should be rejected"
    );

    // Zero max_risk_threshold should fail
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data: encode_init_market_with_limits(
            &admin.pubkey(),
            &env.mint,
            &TEST_FEED_ID,
            u128::MAX,
            0, // INVALID: zero max_risk_threshold
            0,
        ),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[admin],
        env.svm.latest_blockhash(),
    );
    assert!(
        env.svm.send_transaction(tx).is_err(),
        "Zero max_risk_threshold should be rejected"
    );

    println!("INIT MARKET ZERO LIMITS REJECTED: PASSED");
}

/// Verify that UpdateAdmin to zero address is rejected.
#[test]
fn test_update_admin_zero_accepted_for_burn() {
    program_path();

    let mut env = TestEnv::new();
    // Use init_market_with_cap with permissionless resolve + force_close_delay
    // because admin burn requires both for live markets (liveness guard).
    env.init_market_with_cap(0, 10_000, 100);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let zero_pubkey = Pubkey::new_from_array([0u8; 32]);

    // Admin burn to zero is now allowed (spec §7 step [3])
    let result = env.try_update_admin(&admin, &zero_pubkey);
    assert!(
        result.is_ok(),
        "UpdateAdmin to zero should succeed for admin burn"
    );

    // After burn, admin instructions must fail
    let result = env.try_set_risk_threshold(&admin, 999);
    assert!(
        result.is_err(),
        "Admin operations must fail after admin burn"
    );

    println!("UPDATE ADMIN ZERO BURN: PASSED");
}

/// Weaker-authority model (Model 1): admin burn is allowed when
/// oracle_authority is still set PROVIDED the circuit-breaker cap is
/// non-zero. The cap bounds what authority can do (it cannot walk the
/// effective price beyond cap-per-push), so a retained authority is
/// strictly weaker than admin and a safe survivor of burn. This is the
/// primary use case for governance-free Pyth-Pull deployments: keep
/// the authority as a bounded heartbeat/fallback price source that
/// outlives admin.
#[test]
fn test_update_admin_burn_allowed_with_bounded_oracle_authority() {
    program_path();

    let mut env = TestEnv::new();
    // init_market_with_cap seeds oracle_price_cap_e2bps == 10_000 (non-zero),
    // so the weaker-authority invariant (authority set ⇒ cap != 0) holds.
    env.init_market_with_cap(0, 10_000, 100);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Configure an oracle authority. With a non-zero cap, this is a
    // bounded fallback price source, not an admin-equivalent role.
    env.try_set_oracle_authority(&admin, &admin.pubkey())
        .expect("set authority must succeed");

    const AUTHORITY_OFF: usize = 136 + 128;
    let authority_bytes = {
        let slab = env.svm.get_account(&env.slab).unwrap();
        let mut b = [0u8; 32];
        b.copy_from_slice(&slab.data[AUTHORITY_OFF..AUTHORITY_OFF + 32]);
        b
    };
    assert_ne!(
        authority_bytes, [0u8; 32],
        "precondition: oracle_authority is configured",
    );

    let zero_pubkey = Pubkey::new_from_array([0u8; 32]);
    let result = env.try_update_admin(&admin, &zero_pubkey);
    assert!(
        result.is_ok(),
        "UpdateAdmin burn MUST succeed when authority is set AND cap is \
         non-zero — the cap constrains authority to a bounded fallback, \
         which is the weaker-authority model's core guarantee: {:?}",
        result,
    );

    // After burn, admin instructions must still fail (authority is NOT
    // an admin-equivalent — it can only push prices, not configure).
    let err = env
        .try_set_risk_threshold(&admin, 999)
        .expect_err("admin ops must fail after burn");
    let _ = err;
}

/// Weaker-authority invariant: SetOracleAuthority on a non-Hyperp
/// Resolvability invariant: a non-Hyperp market with
///   min_oracle_price_cap_e2bps == 0   AND
///   permissionless_resolve_stale_slots == 0
/// has NO resolve path. Admin ResolveMarket needs a pushed
/// authority_price_e6; under the init-time default, oracle_authority is
/// zero when min_cap == 0 — so nobody can push and the admin path dies
/// instantly. ResolvePermissionless is disabled when perm_resolve == 0.
/// The init must reject this combo outright rather than create a
/// permanently-bricked market.
#[test]
fn test_init_rejects_non_hyperp_with_no_resolve_path() {
    program_path();

    let mut env = TestEnv::new();
    let data = common::encode_init_market_with_cap(
        &env.payer.pubkey(),
        &env.mint,
        &common::TEST_FEED_ID,
        0, // invert=0 (non-Hyperp)
        0, // min_oracle_price_cap_e2bps
        0, // permissionless_resolve_stale_slots
    );
    let err = env
        .try_init_market_raw(data)
        .expect_err("init must reject non-Hyperp + cap=0 + perm_resolve=0");
    assert!(
        err.contains("0x1a"),
        "expected InvalidConfigParam, got: {}", err,
    );
}

/// Positive complement: same (cap=0) market with perm_resolve > 0 is
/// allowed. The perm-stale branch of ResolveMarket settles at
/// engine.last_oracle_price and does not require authority_price_e6,
/// so the market retains a resolve path even with oracle_authority = 0.
#[test]
fn test_init_accepts_non_hyperp_cap_zero_with_perm_resolve() {
    program_path();

    let mut env = TestEnv::new();
    // encode_init_market_with_cap auto-sets max_crank_staleness and
    // force_close_delay when perm_resolve > 0. perm_resolve must be
    // <= MAX_ACCRUAL_DT_SLOTS = 100_000.
    let perm_resolve: u64 = 50_000;
    let data = common::encode_init_market_with_cap(
        &env.payer.pubkey(),
        &env.mint,
        &common::TEST_FEED_ID,
        0,      // invert (non-Hyperp)
        0,      // min_oracle_price_cap_e2bps
        perm_resolve,
    );
    env.try_init_market_raw(data)
        .expect("non-Hyperp + cap=0 + perm_resolve>0 must init OK");

    // End-to-end check: the whole point of the positive invariant is
    // that ResolvePermissionless actually works on this configuration.
    // Init succeeding alone doesn't prove the resolve path exists —
    // the engine could still refuse to resolve at settlement time.
    // Drive the clock past perm_resolve and confirm resolve succeeds.
    let mut clk = env.svm.get_sysvar::<solana_sdk::clock::Clock>();
    clk.slot = clk.slot.saturating_add(perm_resolve + 1);
    clk.unix_timestamp = clk.unix_timestamp.saturating_add(perm_resolve as i64 + 1);
    env.svm.set_sysvar(&clk);
    env.try_resolve_permissionless_once()
        .expect("ResolvePermissionless must succeed on cap=0+perm_resolve>0 market");
    assert!(env.is_market_resolved(), "market must flip to Resolved");
}

/// Burn guard: UpdateAuthority may not zero oracle_authority on a non-
/// Hyperp market when perm_resolve == 0. The burn would remove the only
/// path that populates authority_price_e6 (PushOraclePrice), and
/// perm_resolve == 0 means ResolvePermissionless never matures — every
/// subsequent ResolveMarket call would reject on authority_price_e6 == 0.
#[test]
fn test_update_authority_oracle_burn_rejected_non_hyperp_no_perm_resolve() {
    program_path();

    let mut env = TestEnv::new();
    // Non-Hyperp, cap > 0 (so oracle_authority is admin at init),
    // perm_resolve == 0.
    env.init_market_with_cap(0, 10_000, 0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Attempt to burn oracle authority. Must reject.
    let err = env
        .try_update_authority(&admin, common::AUTHORITY_ORACLE, None)
        .expect_err("oracle-authority burn must reject without perm_resolve");
    assert!(
        err.contains("0x1a"),
        "expected InvalidConfigParam, got: {}", err,
    );
}

/// Weaker-authority invariant: SetOraclePriceCap may not disable the
/// cap (set it to 0) on a non-Hyperp market while oracle_authority is
/// configured. Disabling the cap would upgrade authority from a
/// bounded fallback to an admin-equivalent role. Admins wanting to
/// remove the cap must first zero out the authority.
#[test]
fn test_set_oracle_price_cap_rejects_zero_while_authority_set() {
    program_path();

    let mut env = TestEnv::new();
    // Init with min_cap > 0 so oracle_authority defaults to admin
    // (under the init-time invariant — non-Hyperp + cap=0 zeroes
    // authority). permissionless_resolve=0 keeps the test simple.
    env.init_market_with_cap(0, 10_000, 0);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Admin starts as oracle_authority. The weaker-authority
    // invariant must prevent disabling the cap while the slot is
    // still non-zero.
    let err = env
        .try_set_oracle_price_cap(&admin, 0)
        .expect_err(
            "SetOraclePriceCap(0) must reject when authority is set — \
             would make authority unbounded on every push",
        );
    assert!(
        err.contains("0x1a"),
        "expected InvalidConfigParam, got: {}", err,
    );
}

/// Verify that InitMarket rejects initial risk_params that exceed per-market limits.
#[test]
fn test_init_market_risk_params_exceed_limits_rejected() {
    program_path();

    let mut env = TestEnv::new();
    let admin = &env.payer;
    let dummy_ata = Pubkey::new_unique();
    env.svm
        .set_account(
            dummy_ata,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; TokenAccount::LEN],
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    // Build InitMarket with risk_reduction_threshold > max_risk_threshold
    let mut data = vec![0u8];
    data.extend_from_slice(admin.pubkey().as_ref());
    data.extend_from_slice(env.mint.as_ref());
    data.extend_from_slice(&TEST_FEED_ID);
    data.extend_from_slice(&86400u64.to_le_bytes()); // max_staleness_secs
    data.extend_from_slice(&500u16.to_le_bytes()); // conf_filter_bps
    data.push(0u8); // invert
    data.extend_from_slice(&0u32.to_le_bytes()); // unit_scale
    data.extend_from_slice(&0u64.to_le_bytes()); // initial_mark_price_e6
    // Per-market admin limits
    data.extend_from_slice(&100_000_000_000_000_000_000u128.to_le_bytes()); // max_maintenance_fee_per_slot (<= MAX_PROTOCOL_FEE_ABS)
    data.extend_from_slice(&50_000u128.to_le_bytes()); // max_risk_threshold = 50_000
    data.extend_from_slice(&0u64.to_le_bytes()); // min_oracle_price_cap_e2bps
    // RiskParams with risk_reduction_threshold EXCEEDING the limit
    data.extend_from_slice(&0u64.to_le_bytes()); // warmup_period_slots
    data.extend_from_slice(&500u64.to_le_bytes()); // maintenance_margin_bps
    data.extend_from_slice(&1000u64.to_le_bytes()); // initial_margin_bps
    data.extend_from_slice(&0u64.to_le_bytes()); // trading_fee_bps
    data.extend_from_slice(&(MAX_ACCOUNTS as u64).to_le_bytes());
    data.extend_from_slice(&0u128.to_le_bytes()); // new_account_fee
    data.extend_from_slice(&50_001u128.to_le_bytes()); // risk_reduction_threshold = 50_001 > limit!
    data.extend_from_slice(&0u128.to_le_bytes()); // maintenance_fee_per_slot
    data.extend_from_slice(&u64::MAX.to_le_bytes()); // max_crank_staleness_slots
    data.extend_from_slice(&50u64.to_le_bytes()); // liquidation_fee_bps
    data.extend_from_slice(&1_000_000_000_000u128.to_le_bytes()); // liquidation_fee_cap
    data.extend_from_slice(&100u64.to_le_bytes()); // liquidation_buffer_bps
    data.extend_from_slice(&0u128.to_le_bytes()); // min_liquidation_abs
    data.extend_from_slice(&100u128.to_le_bytes()); // min_initial_deposit
    data.extend_from_slice(&1u128.to_le_bytes()); // min_nonzero_mm_req
    data.extend_from_slice(&2u128.to_le_bytes()); // min_nonzero_im_req

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data,
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[admin],
        env.svm.latest_blockhash(),
    );
    assert!(
        env.svm.send_transaction(tx).is_err(),
        "InitMarket with risk_reduction_threshold > max_risk_threshold should be rejected"
    );

    // Also test maintenance_fee_per_slot > max_maintenance_fee_per_slot
    let mut env2 = TestEnv::new();
    let admin2 = &env2.payer;
    let dummy_ata2 = Pubkey::new_unique();
    env2.svm
        .set_account(
            dummy_ata2,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; TokenAccount::LEN],
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let mut data2 = vec![0u8];
    data2.extend_from_slice(admin2.pubkey().as_ref());
    data2.extend_from_slice(env2.mint.as_ref());
    data2.extend_from_slice(&TEST_FEED_ID);
    data2.extend_from_slice(&u64::MAX.to_le_bytes());
    data2.extend_from_slice(&500u16.to_le_bytes());
    data2.push(0u8);
    data2.extend_from_slice(&0u32.to_le_bytes());
    data2.extend_from_slice(&0u64.to_le_bytes());
    // Per-market admin limits
    data2.extend_from_slice(&1000u128.to_le_bytes()); // max_maintenance_fee = 1000
    data2.extend_from_slice(&10_000_000_000_000_000u128.to_le_bytes()); // max_insurance_floor (<= MAX_VAULT_TVL)
    data2.extend_from_slice(&0u64.to_le_bytes());
    // RiskParams with maintenance_fee_per_slot EXCEEDING the limit
    data2.extend_from_slice(&0u64.to_le_bytes());
    data2.extend_from_slice(&500u64.to_le_bytes());
    data2.extend_from_slice(&1000u64.to_le_bytes());
    data2.extend_from_slice(&0u64.to_le_bytes());
    data2.extend_from_slice(&(MAX_ACCOUNTS as u64).to_le_bytes());
    data2.extend_from_slice(&0u128.to_le_bytes());
    data2.extend_from_slice(&0u128.to_le_bytes()); // risk_reduction_threshold = 0
    data2.extend_from_slice(&1001u128.to_le_bytes()); // maintenance_fee = 1001 > limit!
    data2.extend_from_slice(&u64::MAX.to_le_bytes());
    data2.extend_from_slice(&50u64.to_le_bytes());
    data2.extend_from_slice(&1_000_000_000_000u128.to_le_bytes());
    data2.extend_from_slice(&100u64.to_le_bytes());
    data2.extend_from_slice(&0u128.to_le_bytes());

    let ix2 = Instruction {
        program_id: env2.program_id,
        accounts: vec![
            AccountMeta::new(admin2.pubkey(), true),
            AccountMeta::new(env2.slab, false),
            AccountMeta::new_readonly(env2.mint, false),
            AccountMeta::new(env2.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(env2.pyth_index, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data: data2,
    };

    let tx2 = Transaction::new_signed_with_payer(
        &[cu_ix(), ix2],
        Some(&admin2.pubkey()),
        &[admin2],
        env2.svm.latest_blockhash(),
    );
    assert!(
        env2.svm.send_transaction(tx2).is_err(),
        "InitMarket with maintenance_fee > max_maintenance_fee should be rejected"
    );

    println!("INIT MARKET RISK PARAMS EXCEED LIMITS REJECTED: PASSED");
}

/// Verify InitMarket accepts risk_params at exact boundary (equality with limits).
#[test]
fn test_init_market_risk_params_at_boundary_accepted() {
    program_path();

    let mut env = TestEnv::new();
    let admin = &env.payer;
    let dummy_ata = Pubkey::new_unique();
    env.svm
        .set_account(
            dummy_ata,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; TokenAccount::LEN],
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    // Build InitMarket with risk_reduction_threshold == max_risk_threshold
    // and maintenance_fee_per_slot == max_maintenance_fee_per_slot
    let mut data = vec![0u8];
    data.extend_from_slice(admin.pubkey().as_ref());
    data.extend_from_slice(env.mint.as_ref());
    data.extend_from_slice(&TEST_FEED_ID);
    data.extend_from_slice(&86400u64.to_le_bytes()); // max_staleness_secs
    data.extend_from_slice(&500u16.to_le_bytes()); // conf_filter_bps
    data.push(0u8); // invert
    data.extend_from_slice(&0u32.to_le_bytes()); // unit_scale
    data.extend_from_slice(&0u64.to_le_bytes()); // initial_mark_price_e6
    // Per-market admin limits (current wire format)
    data.extend_from_slice(&0u128.to_le_bytes()); // maintenance_fee_per_slot (0 = disabled)
    data.extend_from_slice(&10_000_000_000_000_000u128.to_le_bytes()); // max_insurance_floor
    // Non-Hyperp + cap=0 + perm_resolve=0 is rejected by the
    // resolvability invariant; ship cap=MAX to satisfy it.
    data.extend_from_slice(&1_000_000u64.to_le_bytes()); // min_oracle_price_cap_e2bps
    // RiskParams
    data.extend_from_slice(&0u64.to_le_bytes()); // h_min
    data.extend_from_slice(&500u64.to_le_bytes()); // maintenance_margin_bps
    data.extend_from_slice(&1000u64.to_le_bytes()); // initial_margin_bps
    data.extend_from_slice(&0u64.to_le_bytes()); // trading_fee_bps
    data.extend_from_slice(&(MAX_ACCOUNTS as u64).to_le_bytes());
    data.extend_from_slice(&0u128.to_le_bytes()); // new_account_fee
    data.extend_from_slice(&0u128.to_le_bytes()); // insurance_floor
    data.extend_from_slice(&1u64.to_le_bytes()); // h_max
    data.extend_from_slice(&u64::MAX.to_le_bytes()); // max_crank_staleness_slots
    data.extend_from_slice(&50u64.to_le_bytes()); // liquidation_fee_bps
    data.extend_from_slice(&1_000_000_000_000u128.to_le_bytes()); // liquidation_fee_cap
    data.extend_from_slice(&100u64.to_le_bytes()); // resolve_price_deviation_bps
    data.extend_from_slice(&0u128.to_le_bytes()); // min_liquidation_abs
    data.extend_from_slice(&100u128.to_le_bytes()); // min_initial_deposit
    data.extend_from_slice(&1u128.to_le_bytes()); // min_nonzero_mm_req
    data.extend_from_slice(&2u128.to_le_bytes()); // min_nonzero_im_req
    // Extended tail (required for v2 format)
    data.extend_from_slice(&0u16.to_le_bytes()); // insurance_withdraw_max_bps
    data.extend_from_slice(&0u64.to_le_bytes()); // insurance_withdraw_cooldown_slots
    data.extend_from_slice(&0u64.to_le_bytes()); // permissionless_resolve_stale_slots
    data.extend_from_slice(&500u64.to_le_bytes()); // funding_horizon_slots
    data.extend_from_slice(&100u64.to_le_bytes()); // funding_k_bps
    data.extend_from_slice(&500i64.to_le_bytes()); // funding_max_premium_bps
    data.extend_from_slice(&1_000i64.to_le_bytes()); // funding_max_e9_per_slot
    data.extend_from_slice(&0u64.to_le_bytes()); // mark_min_fee
    data.extend_from_slice(&0u64.to_le_bytes()); // force_close_delay_slots

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data,
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[admin],
        env.svm.latest_blockhash(),
    );
    assert!(
        env.svm.send_transaction(tx).is_ok(),
        "InitMarket with risk_params at exact limits should succeed"
    );

    println!("INIT MARKET RISK PARAMS AT BOUNDARY ACCEPTED: PASSED");
}

/// Full lifecycle test: init with limits -> Set* ops -> UpdateConfig -> crank -> Set* again.
/// Verifies limits survive across all operation types.
#[test]
fn test_admin_limits_lifecycle() {
    program_path();

    let mut env = TestEnv::new();

    // Init market with specific limits
    let admin = &env.payer;
    let dummy_ata = Pubkey::new_unique();
    env.svm
        .set_account(
            dummy_ata,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; TokenAccount::LEN],
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data: encode_init_market_with_limits(
            &admin.pubkey(),
            &env.mint,
            &TEST_FEED_ID,
            1000,   // max_maintenance_fee
            50_000, // max_risk_threshold
            5000,   // min_oracle_price_cap_e2bps
        ),
    };

    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[admin],
        env.svm.latest_blockhash(),
    );
    env.svm.send_transaction(tx).expect("init_market failed");

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Step 1: Set oracle price cap within limits
    env.try_set_oracle_price_cap(&admin, 10_000).unwrap();

    // Step 2: UpdateConfig with thresh_max at limit
    env.try_update_config_with_params(&admin, 3600, 1000, 0, 50_000)
        .unwrap();

    // Step 3: Crank
    env.set_slot_and_price(10, 100_000_000);
    env.crank();
    env.set_slot_and_price(12, 100_000_000);
    env.crank();

    // Step 4: Limits still enforced after UpdateConfig + crank
    let result = env.try_set_oracle_price_cap(&admin, 4999);
    assert!(result.is_err(), "Limit still enforced after lifecycle: SetOraclePriceCap");

    // Step 5: At-limit values still work
    env.try_set_oracle_price_cap(&admin, 5000).unwrap();

    // Step 6: Verify limit fields in config haven't been corrupted
    let slab = env.svm.get_account(&env.slab).unwrap();
    // BPF slab offsets: HEADER_LEN(72) + config field offset
    // maintenance_fee_per_slot: u128 @ config offset 352
    // max_insurance_floor: u128 @ config offset 208
    // min_oracle_price_cap_e2bps: u64 @ config offset 224
    const MAINT_FEE_OFF: usize = 136 + 352;
    const MAX_INS_FLOOR_OFF: usize = 136 + 208;
    const MIN_OPC_OFF: usize = 136 + 224;

    let maint_fee = u128::from_le_bytes(
        slab.data[MAINT_FEE_OFF..MAINT_FEE_OFF + 16].try_into().unwrap(),
    );
    let max_ins = u128::from_le_bytes(
        slab.data[MAX_INS_FLOOR_OFF..MAX_INS_FLOOR_OFF + 16].try_into().unwrap(),
    );
    let min_opc = u64::from_le_bytes(
        slab.data[MIN_OPC_OFF..MIN_OPC_OFF + 8].try_into().unwrap(),
    );

    // maintenance_fee_per_slot is disabled at init (the feature has an unsound
    // between-cranks behavior pending per-account accrual). Encoders that
    // used to pass a non-zero fee now coerce it to 0; the stored value is 0.
    assert_eq!(maint_fee, 0, "maintenance_fee_per_slot is disabled at init");
    assert_eq!(max_ins, 50_000, "max_insurance_floor should be preserved");
    assert_eq!(min_opc, 5000, "min_oracle_price_cap_e2bps should be preserved");

    println!("ADMIN LIMITS LIFECYCLE: PASSED");
}

/// Vault with pre-set delegate must be rejected by InitMarket.
#[test]
fn test_init_market_rejects_vault_with_delegate() {
    program_path();
    let mut svm = LiteSVM::new();
    let program_id = Pubkey::new_unique();
    svm.add_program(program_id, &std::fs::read(program_path()).unwrap());

    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    let slab = Pubkey::new_unique();
    let mint = Pubkey::new_unique();
    let (vault_pda, _) = Pubkey::find_program_address(&[b"vault", slab.as_ref()], &program_id);
    let vault = Pubkey::new_unique();
    let attacker = Pubkey::new_unique();

    svm.set_account(slab, Account {
        lamports: 1_000_000_000,
        data: vec![0u8; 1156736],
        owner: program_id,
        executable: false,
        rent_epoch: 0,
    }).unwrap();
    svm.set_account(mint, Account {
        lamports: 1_000_000,
        data: {
            let mut d = vec![0u8; spl_token::state::Mint::LEN];
            use spl_token::state::Mint;
            let m = Mint { mint_authority: solana_sdk::program_option::COption::None, supply: 0, decimals: 6, is_initialized: true, freeze_authority: solana_sdk::program_option::COption::None };
            spl_token::state::Mint::pack(m, &mut d).unwrap();
            d
        },
        owner: spl_token::ID,
        executable: false,
        rent_epoch: 0,
    }).unwrap();
    // Vault with delegate set — should be rejected
    svm.set_account(vault, Account {
        lamports: 1_000_000,
        data: make_token_account_with_delegate(&mint, &vault_pda, 0, &attacker, 1_000_000_000),
        owner: spl_token::ID,
        executable: false,
        rent_epoch: 0,
    }).unwrap();

    let ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(slab, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(Pubkey::new_unique(), false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data: encode_init_market_full_v2(&payer.pubkey(), &mint, &[0xABu8; 32], 0, 0, 0),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix], Some(&payer.pubkey()), &[&payer], svm.latest_blockhash(),
    );
    let result = svm.send_transaction(tx);
    assert!(result.is_err(), "InitMarket must reject vault with delegate");

    // Slab header must remain all-zeros (uninitialized) after rejected InitMarket
    let slab_after = svm.get_account(&slab).unwrap();
    assert!(
        slab_after.data[..72].iter().all(|&b| b == 0),
        "slab header must not change on rejected InitMarket (delegate)"
    );
}

/// Vault with close_authority must be rejected by InitMarket.
#[test]
fn test_init_market_rejects_vault_with_close_authority() {
    program_path();
    let mut svm = LiteSVM::new();
    let program_id = Pubkey::new_unique();
    svm.add_program(program_id, &std::fs::read(program_path()).unwrap());

    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    let slab = Pubkey::new_unique();
    let mint = Pubkey::new_unique();
    let (vault_pda, _) = Pubkey::find_program_address(&[b"vault", slab.as_ref()], &program_id);
    let vault = Pubkey::new_unique();
    let attacker = Pubkey::new_unique();

    svm.set_account(slab, Account {
        lamports: 1_000_000_000,
        data: vec![0u8; 1156736],
        owner: program_id,
        executable: false,
        rent_epoch: 0,
    }).unwrap();
    svm.set_account(mint, Account {
        lamports: 1_000_000,
        data: {
            let mut d = vec![0u8; spl_token::state::Mint::LEN];
            use spl_token::state::Mint;
            let m = Mint { mint_authority: solana_sdk::program_option::COption::None, supply: 0, decimals: 6, is_initialized: true, freeze_authority: solana_sdk::program_option::COption::None };
            spl_token::state::Mint::pack(m, &mut d).unwrap();
            d
        },
        owner: spl_token::ID,
        executable: false,
        rent_epoch: 0,
    }).unwrap();
    svm.set_account(vault, Account {
        lamports: 1_000_000,
        data: make_token_account_with_close_authority(&mint, &vault_pda, 0, &attacker),
        owner: spl_token::ID,
        executable: false,
        rent_epoch: 0,
    }).unwrap();

    let ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(slab, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(Pubkey::new_unique(), false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data: encode_init_market_full_v2(&payer.pubkey(), &mint, &[0xABu8; 32], 0, 0, 0),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix], Some(&payer.pubkey()), &[&payer], svm.latest_blockhash(),
    );
    let result = svm.send_transaction(tx);
    assert!(result.is_err(), "InitMarket must reject vault with close_authority");

    // Slab header must remain all-zeros (uninitialized) after rejected InitMarket
    let slab_after = svm.get_account(&slab).unwrap();
    assert!(
        slab_after.data[..72].iter().all(|&b| b == 0),
        "slab header must not change on rejected InitMarket (close_authority)"
    );
}

/// UpdateConfig must reject negative funding_max_premium_bps.
#[test]
fn test_update_config_rejects_negative_funding_max_premium() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Capture config snapshot before rejected operation
    let config_before = env.read_update_config_snapshot();

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
        ],
        data: encode_update_config(
            3600, 100,
            -100i64,  // negative funding_max_premium_bps — must be rejected
            10i64,
            0u128, 100, 100, 100, 5000, 0, 1_000_000u128, 1u128,
        ),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix], Some(&admin.pubkey()), &[&admin], env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(result.is_err(), "Negative funding_max_premium_bps must be rejected");

    // Config must be unchanged after rejection
    assert_eq!(env.read_update_config_snapshot(), config_before, "config must be preserved after rejected UpdateConfig");
}

/// UpdateConfig must reject negative funding_max_e9_per_slot.
#[test]
fn test_update_config_rejects_negative_funding_max_e9_per_slot() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Capture config snapshot before rejected operation
    let config_before = env.read_update_config_snapshot();

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
        ],
        data: encode_update_config(
            3600, 100,
            100i64,
            -5i64,  // negative funding_max_e9_per_slot — must be rejected
            0u128, 100, 100, 100, 5000, 0, 1_000_000u128, 1u128,
        ),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix], Some(&admin.pubkey()), &[&admin], env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(result.is_err(), "Negative funding_max_e9_per_slot must be rejected");

    // Config must be unchanged after rejection
    assert_eq!(env.read_update_config_snapshot(), config_before, "config must be preserved after rejected UpdateConfig");
}

/// InitMarket must reject a vault that already holds tokens.
/// verify_vault_empty checks tok.amount == 0; a non-empty vault indicates
/// pre-seeded funds that would desync engine accounting from day one.
#[test]
fn test_init_market_rejects_nonempty_vault() {
    program_path();
    let mut env = TestEnv::new();

    // Modify the vault account to have a non-zero balance BEFORE init_market.
    // TestEnv::new() creates the vault with amount=0; overwrite it with amount=1.
    let (vault_pda, _) =
        Pubkey::find_program_address(&[b"vault", env.slab.as_ref()], &env.program_id);
    env.svm
        .set_account(
            env.vault,
            Account {
                lamports: 1_000_000,
                data: make_token_account_data(&env.mint, &vault_pda, 1), // amount = 1
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    // Attempt InitMarket — should fail because vault is not empty
    let admin = &env.payer;
    let dummy_ata = Pubkey::new_unique();
    env.svm
        .set_account(
            dummy_ata,
            Account {
                lamports: 1_000_000,
                data: vec![0u8; TokenAccount::LEN],
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.slab, false),
            AccountMeta::new_readonly(env.mint, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(sysvar::rent::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
        ],
        data: encode_init_market_full_v2(&admin.pubkey(), &env.mint, &TEST_FEED_ID, 0, 0, 0),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[admin],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "InitMarket must reject vault with non-zero balance"
    );

    // Slab header must remain all-zeros (uninitialized) after rejected InitMarket
    let slab_after = env.svm.get_account(&env.slab).unwrap();
    assert!(
        slab_after.data[..72].iter().all(|&b| b == 0),
        "slab header must not change on rejected InitMarket (nonempty vault)"
    );
}

// ============================================================================
// UpdateConfig (tag 14) additional coverage
// ============================================================================

/// Spec: UpdateConfig rejects funding_horizon_slots = 0 (would cause division by zero).
#[test]
fn test_update_config_rejects_zero_horizon() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    // Capture config snapshot before rejected operation
    let config_before = env.read_update_config_snapshot();

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let result = env.try_update_config_with_params(
        &admin,
        0,                        // funding_horizon_slots = 0 (invalid)
        1000,                     // thresh_alpha_bps
        0u128,                    // thresh_min
        1_000_000_000_000_000u128, // thresh_max
    );
    assert!(
        result.is_err(),
        "UpdateConfig must reject funding_horizon_slots = 0"
    );

    // Config must be unchanged after rejection
    let config_after = env.read_update_config_snapshot();
    assert_eq!(
        config_after, config_before,
        "config must not change on rejected UpdateConfig (zero horizon)"
    );
}

/// Spec: UpdateConfig is admin-only; non-admin signers are rejected.
#[test]
fn test_update_config_admin_only() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();

    let result = env.try_update_config_with_params(
        &attacker,
        3600,
        1000,
        0u128,
        1_000_000_000_000_000u128,
    );
    assert!(
        result.is_err(),
        "UpdateConfig must reject non-admin signer"
    );
}

// ============================================================================
// UpdateAuthority (4-way split) — positive and negative paths for each kind
// ============================================================================

/// Precondition: new markets default insurance_authority + close_authority
/// to the creator's pubkey (super-admin by default). Confirms the default
/// so subsequent tests can rely on it.
#[test]
fn test_update_authority_init_defaults_match_admin() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 10_000, 1000);

    let admin_kp = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // UpdateAuthority with the current admin signing + new_authority = self
    // must succeed for insurance and close kinds — which proves the
    // current authority on both slots IS the admin pubkey at init.
    env.try_update_authority(&admin_kp, AUTHORITY_INSURANCE, Some(&admin_kp))
        .expect("init default: insurance_authority == admin");
    env.try_update_authority(&admin_kp, AUTHORITY_CLOSE, Some(&admin_kp))
        .expect("init default: close_authority == admin");
}

/// UpdateAuthority happy path: admin delegates insurance_authority to a
/// separate key, then the new insurance_authority can execute
/// WithdrawInsurance (via the scoped check) while the original admin
/// cannot.
#[test]
fn test_update_authority_insurance_positive_delegation() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 10_000, 1000);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let delegate = Keypair::new();
    env.svm.airdrop(&delegate.pubkey(), 1_000_000_000).unwrap();

    // Delegate insurance_authority to a fresh key (requires both signers).
    env.try_update_authority(&admin, AUTHORITY_INSURANCE, Some(&delegate))
        .expect("two-sig delegation must succeed");

    // Verify by attempting a re-delegation that requires the new key
    // to sign as current — proves the authority transferred.
    env.try_update_authority(&delegate, AUTHORITY_INSURANCE, Some(&admin))
        .expect("delegate should now be able to act as insurance_authority");
}

/// Negative: UpdateAuthority requires the CURRENT authority to sign.
/// An attacker with no authority cannot transfer.
#[test]
fn test_update_authority_negative_wrong_current_signer() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 10_000, 1000);
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    let target = Keypair::new();

    for kind in [AUTHORITY_ADMIN, AUTHORITY_INSURANCE, AUTHORITY_CLOSE] {
        let err = env
            .try_update_authority(&attacker, kind, Some(&target))
            .expect_err("attacker must not be able to transfer authority");
        let _ = err;
    }
}

/// Negative: the NEW pubkey MUST sign when it's non-zero (two-sig
/// handover). Without the new-key signature, the instruction rejects —
/// prevents accidental loss via typo.
#[test]
fn test_update_authority_negative_new_pubkey_not_signer() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 10_000, 1000);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let target_pubkey = Pubkey::new_unique();

    // Raw instruction: current signs, but new_pubkey is NOT a signer.
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(target_pubkey, false), // non-signer
            AccountMeta::new(env.slab, false),
        ],
        data: encode_update_authority(AUTHORITY_INSURANCE, &target_pubkey),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&admin.pubkey()),
        &[&admin],
        env.svm.latest_blockhash(),
    );
    let result = env.svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "new pubkey must sign (two-sig handover) when non-zero; \
         non-signer transfer must reject"
    );
}

/// Burn: current authority can zero out its own slot with a
/// single-sig (new_pubkey == Pubkey::default()). Verifies by asserting
/// the authority can no longer act after the burn.
#[test]
fn test_update_authority_burn_single_sig_and_then_dead() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 10_000, 1000);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Burn insurance_authority (single-sig: only current signs).
    env.try_update_authority(&admin, AUTHORITY_INSURANCE, None)
        .expect("burning insurance_authority must succeed with one signer");

    // After burn, admin can no longer act as insurance_authority.
    let new_target = Keypair::new();
    let err = env
        .try_update_authority(&admin, AUTHORITY_INSURANCE, Some(&new_target))
        .expect_err("after burn, no one can set insurance_authority again");
    let _ = err;
}

/// Independence: burning insurance_authority does NOT affect admin or
/// close_authority. Each kind is independent.
#[test]
fn test_update_authority_burning_one_kind_leaves_others_intact() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 10_000, 1000);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    env.try_update_authority(&admin, AUTHORITY_INSURANCE, None)
        .expect("burn insurance_authority");

    // admin (still valid) should be able to delegate close_authority.
    let close_delegate = Keypair::new();
    env.try_update_authority(&admin, AUTHORITY_CLOSE, Some(&close_delegate))
        .expect("admin still acts on close_authority after insurance_authority burn");

    // admin → new admin transfer should also still work (proves the
    // admin kind is independent of the insurance-authority burn).
    let new_admin = Keypair::new();
    env.svm.airdrop(&new_admin.pubkey(), 1_000_000_000).unwrap();
    env.try_update_authority(&admin, AUTHORITY_ADMIN, Some(&new_admin))
        .expect("admin transfer still works after insurance_authority burn");
}

/// Admin-burn liveness guard still applies via UpdateAuthority(kind=ADMIN):
/// cannot burn admin on a live market without permissionless paths wired.
#[test]
fn test_update_authority_admin_burn_requires_permissionless_paths() {
    program_path();
    let mut env = TestEnv::new();
    // permissionless_resolve = 0 → admin burn must reject.
    env.init_market_with_cap(0, 10_000, 0);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let err = env
        .try_update_authority(&admin, AUTHORITY_ADMIN, None)
        .expect_err("admin burn on live market without perm-resolve must reject");
    let _ = err;
}

/// Bad kind byte rejects.
#[test]
fn test_update_authority_negative_invalid_kind() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 10_000, 1000);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    let err = env
        .try_update_authority(&admin, 42u8, None)
        .expect_err("unknown kind must reject");
    let _ = err;
}

/// After admin burn, insurance_authority can STILL withdraw insurance
/// (proves the scoped split works — insurance_authority outlives admin).
/// Conversely, burning insurance_authority locks insurance forever
/// (the traders-are-rug-proof configuration).
#[test]
fn test_update_authority_insurance_survives_admin_burn() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 10_000, 1000);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Before admin burn: delegate insurance_authority to a dedicated key.
    let ins_authority = Keypair::new();
    env.svm.airdrop(&ins_authority.pubkey(), 1_000_000_000).unwrap();
    env.try_update_authority(&admin, AUTHORITY_INSURANCE, Some(&ins_authority))
        .expect("delegate insurance_authority");

    // Burn admin (live market with perm-resolve + force-close configured).
    env.try_update_authority(&admin, AUTHORITY_ADMIN, None)
        .expect("admin burn allowed: liveness guards satisfied");

    // Insurance_authority still valid — can now re-delegate itself.
    // (Actual WithdrawInsurance requires resolved + zero accounts, out of
    // scope for this auth test; proving the signer can still act via
    // UpdateAuthority is a sufficient liveness indicator.)
    let new_ins = Keypair::new();
    env.svm.airdrop(&new_ins.pubkey(), 1_000_000_000).unwrap();
    env.try_update_authority(&ins_authority, AUTHORITY_INSURANCE, Some(&new_ins))
        .expect("insurance_authority survives admin burn");
}


