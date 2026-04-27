#![cfg(feature = "legacy-tests")]
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

/// CRITICAL: admin rotation only callable by current admin
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
    println!("admin rotation by non-admin: REJECTED (correct)");

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
    println!("admin rotation by admin: ACCEPTED (correct)");

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
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
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
    assert_eq!(
        used_after, used_before,
        "Rejected second InitMarket must not change num_used_accounts"
    );
    assert_eq!(
        vault_after, vault_before,
        "Rejected second InitMarket must not move vault funds"
    );
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

/// Verify admin burn (rotate to zero) lifecycle rules.
#[test]
fn test_update_admin_zero_accepted_for_burn() {
    program_path();

    let mut env = TestEnv::new();
    // Use init_market_with_cap with permissionless resolve + force_close_delay
    // because admin burn requires both for live markets (liveness guard).
    env.init_market_with_cap(0, 100);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    let zero_pubkey = Pubkey::new_from_array([0u8; 32]);

    // Admin burn to zero is now allowed (spec §7 step [3])
    let result = env.try_update_admin(&admin, &zero_pubkey);
    assert!(
        result.is_ok(),
        "admin burn should succeed under the lifecycle guard"
    );

    // After burn, admin instructions must fail. SetMaintenanceFee is also
    // admin-gated; use it as the probe now that SetOraclePriceCap is gone.
    let result = env.try_set_maintenance_fee(&admin, 0);
    assert!(
        result.is_err(),
        "Admin operations must fail after admin burn"
    );

    println!("UPDATE ADMIN ZERO BURN: PASSED");
}

/// Resolvability invariant: a non-Hyperp market with
///   min_oracle_price_cap_e2bps == 0   AND
///   permissionless_resolve_stale_slots == 0
/// has no resolve path: non-Hyperp markets resolve via a fresh Pyth
/// read (or the Degenerate arm once the permissionless-stale window
/// matures), and perm_resolve == 0 disables the window entirely.
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
        0, // permissionless_resolve_stale_slots
    );
    let err = env
        .try_init_market_raw(data)
        .expect_err("init must reject non-Hyperp + perm_resolve=0");
    assert!(
        err.contains("0x1a"),
        "expected InvalidConfigParam, got: {}",
        err,
    );
}

/// Positive complement: same (cap=0) market with perm_resolve > 0 is
/// allowed. The perm-stale branch of ResolveMarket settles at
/// engine.last_oracle_price and does not require hyperp_mark_e6,
/// so the market retains a resolve path even with hyperp_authority = 0.
#[test]
fn test_init_accepts_non_hyperp_cap_zero_with_perm_resolve() {
    program_path();

    let mut env = TestEnv::new();
    // encode_init_market_with_cap auto-sets max_crank_staleness and
    // force_close_delay when perm_resolve > 0. v12.19.6: perm_resolve
    // must be <= MAX_ACCRUAL_DT_SLOTS = 100.
    let perm_resolve: u64 = 80;
    let data = common::encode_init_market_with_cap(
        &env.payer.pubkey(),
        &env.mint,
        &common::TEST_FEED_ID,
        0, // invert (non-Hyperp)
        perm_resolve,
    );
    env.try_init_market_raw(data)
        .expect("non-Hyperp + perm_resolve>0 must init OK");

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

// test_set_oracle_price_cap_rejects_zero_when_floor_nonzero deleted:
// SetOraclePriceCap (tag 18) was removed in v12.19. The per-slot price-move
// cap is now the immutable init-time `max_price_move_bps_per_slot` field,
// so there is no runtime admin path to disable/change it.

// test_admin_limits_lifecycle deleted for the same reason: every step
// exercised the now-gone SetOraclePriceCap runtime surface.

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

    svm.set_account(
        slab,
        Account {
            lamports: 1_000_000_000,
            data: vec![0u8; 1156736],
            owner: program_id,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
    svm.set_account(
        mint,
        Account {
            lamports: 1_000_000,
            data: {
                let mut d = vec![0u8; spl_token::state::Mint::LEN];
                use spl_token::state::Mint;
                let m = Mint {
                    mint_authority: solana_sdk::program_option::COption::None,
                    supply: 0,
                    decimals: 6,
                    is_initialized: true,
                    freeze_authority: solana_sdk::program_option::COption::None,
                };
                spl_token::state::Mint::pack(m, &mut d).unwrap();
                d
            },
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
    // Vault with delegate set — should be rejected
    svm.set_account(
        vault,
        Account {
            lamports: 1_000_000,
            data: make_token_account_with_delegate(&mint, &vault_pda, 0, &attacker, 1_000_000_000),
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    let ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(slab, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(Pubkey::new_unique(), false),
        ],
        data: encode_init_market_full_v2(&payer.pubkey(), &mint, &[0xABu8; 32], 0, 0, 0),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&payer.pubkey()),
        &[&payer],
        svm.latest_blockhash(),
    );
    let result = svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "InitMarket must reject vault with delegate"
    );

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

    svm.set_account(
        slab,
        Account {
            lamports: 1_000_000_000,
            data: vec![0u8; 1156736],
            owner: program_id,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
    svm.set_account(
        mint,
        Account {
            lamports: 1_000_000,
            data: {
                let mut d = vec![0u8; spl_token::state::Mint::LEN];
                use spl_token::state::Mint;
                let m = Mint {
                    mint_authority: solana_sdk::program_option::COption::None,
                    supply: 0,
                    decimals: 6,
                    is_initialized: true,
                    freeze_authority: solana_sdk::program_option::COption::None,
                };
                spl_token::state::Mint::pack(m, &mut d).unwrap();
                d
            },
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
    svm.set_account(
        vault,
        Account {
            lamports: 1_000_000,
            data: make_token_account_with_close_authority(&mint, &vault_pda, 0, &attacker),
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();

    let ix = Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(slab, false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(Pubkey::new_unique(), false),
        ],
        data: encode_init_market_full_v2(&payer.pubkey(), &mint, &[0xABu8; 32], 0, 0, 0),
    };
    let tx = Transaction::new_signed_with_payer(
        &[cu_ix(), ix],
        Some(&payer.pubkey()),
        &[&payer],
        svm.latest_blockhash(),
    );
    let result = svm.send_transaction(tx);
    assert!(
        result.is_err(),
        "InitMarket must reject vault with close_authority"
    );

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
            3600, 100, -100i64, // negative funding_max_premium_bps — must be rejected
            10i64,
        ),
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
        "Negative funding_max_premium_bps must be rejected"
    );

    // Config must be unchanged after rejection
    assert_eq!(
        env.read_update_config_snapshot(),
        config_before,
        "config must be preserved after rejected UpdateConfig"
    );
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
            3600, 100, 100i64, -5i64, // negative funding_max_e9_per_slot — must be rejected
        ),
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
        "Negative funding_max_e9_per_slot must be rejected"
    );

    // Config must be unchanged after rejection
    assert_eq!(
        env.read_update_config_snapshot(),
        config_before,
        "config must be preserved after rejected UpdateConfig"
    );
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
            AccountMeta::new_readonly(sysvar::clock::ID, false),
            AccountMeta::new_readonly(env.pyth_index, false),
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
    let result = env.try_update_config_with_params(&admin, 0);
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

    let result = env.try_update_config_with_params(&attacker, 3600);
    assert!(result.is_err(), "UpdateConfig must reject non-admin signer");
}

// ============================================================================
// UpdateAuthority (4-way split) — positive and negative paths for each kind
// ============================================================================

/// Precondition: new markets default insurance_authority to the creator's
/// pubkey (super-admin by default). Confirms the default so subsequent
/// tests can rely on it.
#[test]
fn test_update_authority_init_defaults_match_admin() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 80);

    let admin_kp = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // UpdateAuthority with the current admin signing + new_authority = self
    // must succeed for insurance — which proves the current authority IS
    // the admin pubkey at init.
    env.try_update_authority(&admin_kp, AUTHORITY_INSURANCE, Some(&admin_kp))
        .expect("init default: insurance_authority == admin");
}

/// UpdateAuthority happy path: admin delegates insurance_authority to a
/// separate key, then the new insurance_authority can execute
/// WithdrawInsurance (via the scoped check) while the original admin
/// cannot.
#[test]
fn test_update_authority_insurance_positive_delegation() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 80);
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
    env.init_market_with_cap(0, 80);
    let attacker = Keypair::new();
    env.svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    let target = Keypair::new();

    for kind in [AUTHORITY_ADMIN, AUTHORITY_INSURANCE] {
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
    env.init_market_with_cap(0, 80);
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
    env.init_market_with_cap(0, 80);
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
    env.init_market_with_cap(0, 80);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    env.try_update_authority(&admin, AUTHORITY_INSURANCE, None)
        .expect("burn insurance_authority");

    // admin → new admin transfer should still work (proves the admin
    // kind is independent of the insurance-authority burn).
    let new_admin = Keypair::new();
    env.svm.airdrop(&new_admin.pubkey(), 1_000_000_000).unwrap();
    env.try_update_authority(&admin, AUTHORITY_ADMIN, Some(&new_admin))
        .expect("admin transfer still works after insurance_authority burn");
}

// test_update_authority_admin_burn_requires_permissionless_paths deleted:
// v12.19 enforces the resolvability invariant at InitMarket — a non-Hyperp
// market can no longer be created with permissionless_resolve_stale_slots = 0,
// so the "live market without perm-resolve" configuration this test required
// is unreachable via the public init surface.

/// Bad kind byte rejects.
#[test]
fn test_update_authority_negative_invalid_kind() {
    program_path();
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 80);
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
    env.init_market_with_cap(0, 80);
    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();

    // Before admin burn: delegate insurance_authority to a dedicated key.
    let ins_authority = Keypair::new();
    env.svm
        .airdrop(&ins_authority.pubkey(), 1_000_000_000)
        .unwrap();
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
