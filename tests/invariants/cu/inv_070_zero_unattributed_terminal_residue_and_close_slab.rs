//! INV-070 - Zero unattributed terminal residue and CloseSlab.
//!
//! `CloseSlab` is the final market-account reclaim path. It may run only after
//! all user claimable value, live encumbrances, unresolved loss, pending
//! receipts, and unexplained accounting have been reduced to zero or explicitly
//! classified as final protocol dust. These tests exercise the deployed wrapper
//! through LiteSVM public instructions and assert the two security-relevant
//! terminal properties:
//!
//! * a live or resolved-but-funded market rejects atomically and remains
//!   recoverable by the user wind-down route; and
//! * after accounting is fully drained, final raw vault dust can be swept only
//!   to the current authority's correct quote-token destination, with wrong
//!   destinations rejected before the vault or market slab changes.
//! `v16_program_recovery_force_close_reaches_zero_residue_and_close_slab` composes the final path
//! with a publicly reached Recovery episode and permissionless force-close. It proves terminal
//! normalization does not rely on a market that stayed Active throughout its lifetime.
//! `v16_program_terminal_stock_and_close_slab_composition_is_source_complete` closes the current
//! surface by binding the exact engine pin's terminal-claim, reservation, recredit, retirement,
//! and bounded-scan proofs to the existing public lifecycle and maximum-shape witnesses. It also
//! locks the wrapper ordering: canonical vault and destination validation precedes the engine
//! transition, and no SPL burn, transfer, close, or market tombstone write can occur before
//! `ReadyToClose`.

use super::*;

#[test]
fn v16_program_recovery_force_close_reaches_zero_residue_and_close_slab() {
    const INITIAL_CAPITAL: u128 = 1_000_000;
    const OPEN_Q: u128 = 2 * POS_SCALE;
    const SHUTDOWN_SLOT: u64 = 2;
    const FORCE_CLOSE_SLOT: u64 = 7;

    let mut env = V16CuEnv::new();
    env.configure_permissionless_resolve_with_cu(100, 5);
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let cranker = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, INITIAL_CAPITAL);
    env.deposit(&short_owner, short, INITIAL_CAPITAL);
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        OPEN_Q as i128,
        100,
        0,
    );
    assert_eq!(env.market_state().1.assets[0].oi_eff_long_q, OPEN_Q);
    assert_eq!(env.market_state().1.assets[0].oi_eff_short_q, OPEN_Q);
    assert_eq!(env.token_amount(env.vault), 2 * INITIAL_CAPITAL as u64);

    env.svm.warp_to_slot(SHUTDOWN_SLOT);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
        0,
        SHUTDOWN_SLOT,
        0,
    );
    assert_eq!(
        env.market_state().1.assets[0].lifecycle,
        AssetLifecycleV16::Recovery
    );

    env.svm.warp_to_slot(FORCE_CLOSE_SLOT);
    let force_close_cu =
        env.force_close_abandoned_asset_with_cu(&cranker, long, short, 0, FORCE_CLOSE_SLOT, OPEN_Q);
    assert_cu_within(
        "Recovery force-close before terminal slab",
        force_close_cu,
        CUSTODY_CU_LIMIT,
    );
    let recovered = env.market_state().1;
    assert_eq!(recovered.assets[0].oi_eff_long_q, 0);
    assert_eq!(recovered.assets[0].oi_eff_short_q, 0);
    assert!(!has_active_leg_for_asset(&env.portfolio_state(long), 0));
    assert!(!has_active_leg_for_asset(&env.portfolio_state(short), 0));
    assert_eq!(recovered.vault, 2 * INITIAL_CAPITAL);
    assert_eq!(recovered.c_tot, 2 * INITIAL_CAPITAL);
    assert_eq!(recovered.vault as u64, env.token_amount(env.vault));

    env.resolve();
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
    let resolved_slot = env.market_state().1.resolved_slot;
    let market_before_timeout = env.svm.get_account(&env.market).unwrap();
    let long_before_timeout = env.svm.get_account(&long).unwrap();
    let vault_before_timeout = env.svm.get_account(&env.vault).unwrap();
    let (_, early_close) = env.try_close_resolved_with_cu(&long_owner, long);
    assert!(
        early_close.is_err(),
        "unsigned CloseResolved must remain owner-gated before the configured timeout",
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_timeout
    );
    assert_eq!(env.svm.get_account(&long).unwrap(), long_before_timeout);
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before_timeout
    );

    env.svm.warp_to_slot(resolved_slot + 5);
    let long_destination = env.close_resolved(&long_owner, long);
    let short_destination = env.close_resolved(&short_owner, short);
    assert_eq!(env.token_amount(long_destination), INITIAL_CAPITAL as u64);
    assert_eq!(env.token_amount(short_destination), INITIAL_CAPITAL as u64);
    env.close_portfolio_with_cu(&long_owner, long);
    env.close_portfolio_with_cu(&short_owner, short);

    let (_, terminal) = env.market_state();
    assert_eq!(terminal.materialized_portfolio_count, 0);
    assert_eq!(terminal.vault, 0);
    assert_eq!(terminal.c_tot, 0);
    assert_eq!(terminal.insurance, 0);
    assert_eq!(env.token_amount(env.vault), 0);

    let admin = env.admin.insecure_clone();
    let admin_destination = env.token_account(admin.pubkey(), 0);
    env.svm.expire_blockhash();
    let close_cu = env
        .send(
            ProgInstruction::CloseSlab { authority_epoch: 0 },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new(admin_destination, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&admin],
        )
        .expect("Recovery-normalized market must reach CloseSlab");
    assert_cu_within("Recovery-normalized CloseSlab", close_cu, CUSTODY_CU_LIMIT);
    assert_eq!(env.token_amount(admin_destination), 0);
    assert_closed_market_tombstone(&env.svm.get_account(&env.market).unwrap());
}

#[test]
fn v16_program_close_slab_rejects_until_market_has_zero_terminal_residue() {
    let mut env = V16CuEnv::new();
    let market = env.market;
    let vault = env.vault;
    let vault_authority = env.vault_authority;
    let admin = env.admin.insecure_clone();
    let admin_dest = env.token_account(admin.pubkey(), 0);

    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000_000);

    let try_close = |env: &mut V16CuEnv| -> Result<u64, String> {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::CloseSlab { authority_epoch: 0 },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(market, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(vault_authority, false),
                AccountMeta::new(admin_dest, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&admin],
        )
    };

    for label in ["live funded market", "resolved funded market"] {
        let before_market = env.svm.get_account(&market).unwrap();
        let before_vault = env.svm.get_account(&vault).unwrap();
        let before_portfolio = env.svm.get_account(&portfolio).unwrap();
        let before_dest = env.svm.get_account(&admin_dest).unwrap();
        assert!(
            try_close(&mut env).is_err(),
            "{label}: CloseSlab must reject before terminal accounting is zero",
        );
        assert_eq!(
            env.svm.get_account(&market).unwrap(),
            before_market,
            "{label}: rejected CloseSlab must not mutate market accounting",
        );
        assert_eq!(
            env.svm.get_account(&vault).unwrap(),
            before_vault,
            "{label}: rejected CloseSlab must not move or close vault custody",
        );
        assert_eq!(
            env.svm.get_account(&portfolio).unwrap(),
            before_portfolio,
            "{label}: rejected CloseSlab must not mutate user portfolio state",
        );
        assert_eq!(
            env.svm.get_account(&admin_dest).unwrap(),
            before_dest,
            "{label}: rejected CloseSlab must not pay final dust destination",
        );

        if label == "live funded market" {
            env.resolve();
            assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
        }
    }

    let user_dest = env.close_resolved(&owner, portfolio);
    assert_eq!(
        env.token_amount(user_dest),
        1_000_000,
        "resolved user wind-down remains available after rejected CloseSlab attempts",
    );
    env.close_portfolio_with_cu(&owner, portfolio);
    let (_, drained) = env.market_state();
    assert_eq!(
        (
            drained.vault,
            drained.insurance,
            drained.c_tot,
            drained.materialized_portfolio_count,
        ),
        (0, 0, 0, 0),
        "all terminal user value and accounting are drained before final slab reclaim",
    );

    assert!(
        try_close(&mut env).is_ok(),
        "fully drained market can be reclaimed by CloseSlab",
    );
    let closed_market = env.svm.get_account(&market).unwrap();
    assert_closed_market_tombstone(&closed_market);
}

// INV-070/073 regression: an unprivileged permissionless-asset creator can choose an effectively
// unbounded backing expiry. Once all user claims are gone, any caller can pay the exact principal
// to a token account owned by the configured provider and release terminal cleanup without the
// provider's signature. All setup and transitions use public instructions; the only fixture writes
// construct ordinary external SPL accounts and their matching mint supply.
#[test]
fn v16_future_backing_principal_is_permissionlessly_payable_at_terminal_close() {
    const INIT_FEE: u128 = 10;
    const USER_CAPITAL: u128 = 1_000;
    const BACKING_PRINCIPAL: u128 = 1;
    const ASSET_INDEX: u16 = 1;
    const LONG_DOMAIN: u16 = 2;
    const ACTIVATION_SLOT: u64 = 1;
    const RESOLVE_SLOT: u64 = 10;
    const PROTOCOL_TIMEOUT_HORIZON: u64 = RESOLVE_SLOT
        + percolator_prog::constants::MAX_PERMISSIONLESS_RESOLVE_STALE_SLOTS
        + percolator_prog::constants::MAX_FORCE_CLOSE_DELAY_SLOTS
        + 1;

    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let attacker = Keypair::new();
    let user = Keypair::new();
    let initial_market_lamports = env.svm.get_account(&env.market).unwrap().lamports;
    let initial_vault_lamports = env.svm.get_account(&env.vault).unwrap().lamports;

    // The LiteSVM token fixtures construct balances directly, so initialize the external SPL mint
    // supply to the exact sum those fixtures will create. This lets terminal insurance burning run
    // through the real SPL program in the non-vacuous cooperation control below.
    let initial_mint_supply = u64::try_from(INIT_FEE + USER_CAPITAL + BACKING_PRINCIPAL).unwrap();
    let mut mint_account = env.svm.get_account(&env.mint).expect("quote mint");
    let mut mint_state = Mint::unpack(&mint_account.data).expect("decode quote mint");
    mint_state.supply = initial_mint_supply;
    Mint::pack(mint_state, &mut mint_account.data).expect("encode quote mint");
    env.svm.set_account(env.mint, mint_account).unwrap();

    env.update_market_init_fee_policy_with_cu(INIT_FEE);
    env.configure_permissionless_resolve_with_cu(5, 5);
    env.svm.warp_to_slot(ACTIVATION_SLOT);
    env.activate_permissionless_asset_with_fee(
        &attacker,
        ASSET_INDEX,
        ACTIVATION_SLOT,
        100,
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        INIT_FEE,
    );
    env.top_up_backing_bucket_with_authority(&attacker, LONG_DOMAIN, BACKING_PRINCIPAL, u64::MAX);

    // Build and fund the user only through InitPortfolio and Deposit after the dynamic append, so
    // the portfolio has the deployed two-market layout without touching program-owned bytes.
    let user_portfolio = Pubkey::new_unique();
    let user_portfolio_len = state::portfolio_account_len_for_market_slots(2).unwrap();
    let user_portfolio_rent = solana_sdk::rent::Rent::default().minimum_balance(user_portfolio_len);
    env.ensure_signer_account(user.pubkey());
    env.svm
        .set_account(
            user_portfolio,
            Account {
                lamports: user_portfolio_rent,
                data: vec![0u8; user_portfolio_len],
                owner: env.program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(user.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(user_portfolio, false),
        ],
        &[&user],
    )
    .expect("initialize the user's exact-rent portfolio through the public route");
    env.deposit(&user, user_portfolio, USER_CAPITAL);

    env.resolve_stale_permissionless_with_cu(RESOLVE_SLOT);
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
    let user_destination = env.token_account(user.pubkey(), 0);
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        vec![
            AccountMeta::new_readonly(user.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(user_portfolio, false),
            AccountMeta::new(user_destination, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&user],
    )
    .expect("the owner closes its resolved portfolio through the signed public route");
    assert_eq!(
        env.token_amount(user_destination),
        USER_CAPITAL as u64,
        "the user's complete claim is paid before the terminal DoS is measured",
    );
    env.close_portfolio_with_cu(&user, user_portfolio);

    let (_, settled_group) = env.market_state();
    assert_eq!(settled_group.materialized_portfolio_count, 0);
    assert_eq!(settled_group.c_tot, 0);
    assert_eq!(settled_group.pnl_pos_tot, 0);
    assert_eq!(settled_group.source_claim_bound_total_num, 0);
    assert_eq!(settled_group.resolved_payout_blocker_count, 0);
    assert_eq!(settled_group.insurance, INIT_FEE);
    assert_eq!(settled_group.vault, INIT_FEE + BACKING_PRINCIPAL);
    assert_eq!(
        env.token_amount(env.vault),
        (INIT_FEE + BACKING_PRINCIPAL) as u64,
    );

    // Exhaust the legitimate protocol-stock continuation first. The permissionless-init fee is
    // budgeted asset-0 insurance and is fully recoverable by the honest configured authority; it
    // must not be misreported as part of the attacker's terminal lock.
    let (protocol_destination, _) = env.withdraw_insurance_with_cu(INIT_FEE);
    assert_eq!(env.token_amount(protocol_destination), INIT_FEE as u64);

    let (terminal_cfg, terminal_group) = env.market_state();
    let poisoned_bucket = terminal_group.source_backing_buckets[LONG_DOMAIN as usize];
    assert_eq!(poisoned_bucket.status, BackingBucketStatusV16::Fresh);
    assert_eq!(poisoned_bucket.expiry_slot, u64::MAX);
    assert_eq!(
        poisoned_bucket.fresh_unliened_backing_num,
        BACKING_PRINCIPAL * BOUND_SCALE,
    );
    assert_eq!(terminal_group.insurance, 0);
    assert_eq!(terminal_group.vault, BACKING_PRINCIPAL);
    assert_eq!(env.token_amount(env.vault), BACKING_PRINCIPAL as u64);
    assert_eq!(terminal_cfg.terminal_slab_scan_progress, 0);

    let admin_destination = env.token_account(admin.pubkey(), 0);
    let close_slab = |env: &mut V16CuEnv| -> Result<u64, String> {
        env.svm.expire_blockhash();
        let authority_epoch = env.control_sequences(0).authority_epoch;
        env.send(
            ProgInstruction::CloseSlab { authority_epoch },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new(admin_destination, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(env.mint, false),
            ],
            &[&admin],
        )
    };

    // The first call can only persist a cursor pointing at the attacker's asset. It transfers no
    // tokens or lamports and does not reduce any encumbrance.
    let market_lamports_before = env.svm.get_account(&env.market).unwrap().lamports;
    let vault_lamports_before = env.svm.get_account(&env.vault).unwrap().lamports;
    let admin_lamports_before = env.svm.get_account(&admin.pubkey()).unwrap().lamports;
    close_slab(&mut env).expect("first bounded CloseSlab scan records the blocking asset");
    let first_close_reached_terminal = env.svm.get_account(&env.market).unwrap().data.len()
        == percolator_prog::constants::HEADER_LEN;
    assert!(!first_close_reached_terminal);
    assert_eq!(env.market_state().0.terminal_slab_scan_progress, 1);
    assert_eq!(env.token_amount(env.vault), BACKING_PRINCIPAL as u64);
    assert_eq!(env.token_amount(admin_destination), 0);
    assert_eq!(
        env.svm.get_account(&admin.pubkey()).unwrap().lamports,
        admin_lamports_before,
    );

    // Once parked on the asset, every repeat is LockActive and rolls back exactly.
    let parked_market = env.svm.get_account(&env.market).unwrap();
    let parked_vault = env.svm.get_account(&env.vault).unwrap();
    let parked_destination = env.svm.get_account(&admin_destination).unwrap();
    let parked = close_slab(&mut env).expect_err("live future backing must park CloseSlab");
    assert!(
        parked.contains("Custom(21)"),
        "unexpected parked error: {parked}"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), parked_market);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), parked_vault);
    assert_eq!(
        env.svm.get_account(&admin_destination).unwrap(),
        parked_destination,
    );

    // Advancing beyond every configured wrapper timeout still leaves the attacker-selected expiry
    // live. Even the final predecessor slot cannot finish terminal cleanup.
    for slot in [PROTOCOL_TIMEOUT_HORIZON, u64::MAX - 1] {
        env.svm.warp_to_slot(slot);
        let before_market = env.svm.get_account(&env.market).unwrap();
        let before_vault = env.svm.get_account(&env.vault).unwrap();
        let blocked = close_slab(&mut env)
            .expect_err("no honest bounded CloseSlab can pass the future-expiry bucket");
        assert!(
            blocked.contains("Custom(21)"),
            "slot {slot}: unexpected CloseSlab error: {blocked}",
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), before_market);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), before_vault);
    }

    // The honest market authority cannot drain the attacker-owned bucket after resolution.
    let admin_backing_destination = env.token_account(admin.pubkey(), 0);
    let before_admin_withdraw_market = env.svm.get_account(&env.market).unwrap();
    let before_admin_withdraw_vault = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let admin_withdraw = env
        .send(
            ProgInstruction::WithdrawBackingBucket {
                domain: LONG_DOMAIN,
                market_id: env.asset_market_id(ASSET_INDEX),
                authority_epoch: env.withdrawal_authority_epoch(
                    admin.pubkey(),
                    ASSET_INDEX as usize,
                    false,
                ),
                amount: BACKING_PRINCIPAL,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(admin_backing_destination, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&admin],
        )
        .expect_err("resolved market authority must not impersonate the backing authority");
    assert!(
        admin_withdraw.contains("Custom(8)"),
        "unexpected market-authority withdrawal error: {admin_withdraw}",
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        before_admin_withdraw_market,
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        before_admin_withdraw_vault,
    );
    assert_eq!(env.token_amount(admin_backing_destination), 0);

    // Permissionless settlement is destination-confined: naming the real provider without its
    // signature does not authorize redirecting that provider's principal to the market admin.
    let before_redirect_market = env.svm.get_account(&env.market).unwrap();
    let before_redirect_vault = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let redirected = env.send(
        ProgInstruction::WithdrawBackingBucket {
            domain: LONG_DOMAIN,
            market_id: env.asset_market_id(ASSET_INDEX),
            authority_epoch: env.withdrawal_authority_epoch(
                attacker.pubkey(),
                ASSET_INDEX as usize,
                false,
            ),
            amount: BACKING_PRINCIPAL,
        },
        vec![
            AccountMeta::new_readonly(attacker.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(admin_backing_destination, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        redirected.is_err(),
        "terminal provider principal cannot be redirected"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        before_redirect_market
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        before_redirect_vault
    );
    assert_eq!(env.token_amount(admin_backing_destination), 0);

    // Nor can the honest market authority rotate a funded per-asset backing role to a rescue key.
    let rescue = Keypair::new();
    env.ensure_signer_account(rescue.pubkey());
    let before_rotation = env.svm.get_account(&env.market).unwrap();
    let rotation = env
        .try_update_per_asset_authority_with_cu(
            &admin,
            Some(&rescue),
            ASSET_INDEX,
            processor::ASSET_AUTH_BACKING_BUCKET,
            rescue.pubkey().to_bytes(),
        )
        .expect_err("marketauth must not seize a funded local backing role");
    assert!(
        rotation.contains("Custom(8)"),
        "unexpected market-authority rotation error: {rotation}",
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), before_rotation);

    let retained_tombstone_lamports =
        solana_sdk::rent::Rent::default().minimum_balance(percolator_prog::constants::HEADER_LEN);
    let stranded_market_lamports = market_lamports_before - retained_tombstone_lamports;
    let stranded_total_lamports = stranded_market_lamports + vault_lamports_before;
    let stranded_initial_market_lamports = initial_market_lamports - retained_tombstone_lamports;
    let stranded_closed_portfolio_lamports = user_portfolio_rent;
    let stranded_protocol_atoms = terminal_group.insurance;
    let recovered_protocol_atoms = env.token_amount(protocol_destination);
    let stranded_user_claim_atoms = terminal_group.c_tot
        + terminal_group.pnl_pos_tot
        + terminal_group.source_claim_bound_total_num / BOUND_SCALE;
    assert_eq!(
        market_lamports_before,
        initial_market_lamports + user_portfolio_rent,
        "ClosePortfolio credits its exact rent to the market slab",
    );
    assert_eq!(vault_lamports_before, initial_vault_lamports);
    assert_eq!(stranded_user_claim_atoms, 0);
    assert_eq!(recovered_protocol_atoms, INIT_FEE as u64);
    assert_eq!(
        stranded_total_lamports,
        stranded_initial_market_lamports
            + stranded_closed_portfolio_lamports
            + initial_vault_lamports,
    );

    // At the same authenticated slot, an honest caller supplies the configured provider pubkey and
    // a provider-owned destination but no provider signature. The payout cannot be redirected, and
    // the next CloseSlab can close custody and refund the previously stranded rent.
    let attacker_destination = env.token_account(attacker.pubkey(), 0);
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::WithdrawBackingBucket {
            domain: LONG_DOMAIN,
            market_id: env.asset_market_id(ASSET_INDEX),
            authority_epoch: env.withdrawal_authority_epoch(
                attacker.pubkey(),
                ASSET_INDEX as usize,
                false,
            ),
            amount: BACKING_PRINCIPAL,
        },
        vec![
            AccountMeta::new_readonly(attacker.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(attacker_destination, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    )
    .expect("any caller can pay the exact terminal principal to the configured provider");
    assert_eq!(
        env.token_amount(attacker_destination),
        BACKING_PRINCIPAL as u64
    );
    assert_eq!(env.market_state().1.vault, 0);

    let mint_supply_before_close = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
        .unwrap()
        .supply;
    close_slab(&mut env).expect("CloseSlab succeeds immediately after attacker cooperation");
    let mint_supply_after_close = Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
        .unwrap()
        .supply;
    assert_eq!(
        mint_supply_before_close - mint_supply_after_close,
        stranded_protocol_atoms as u64,
        "terminal closure burns exactly the permissionless-init protocol insurance",
    );
    assert_closed_market_tombstone(&env.svm.get_account(&env.market).unwrap());
    assert_eq!(
        env.svm.get_account(&admin.pubkey()).unwrap().lamports - admin_lamports_before,
        stranded_total_lamports,
        "successful terminal cleanup refunds exactly market slab plus vault rent",
    );
}

#[test]
fn v16_expired_provider_earnings_are_permissionlessly_payable_at_terminal_close() {
    const INITIAL_PRICE: u64 = 100;
    const SOURCE_POSITION_Q: i128 = 200 * POS_SCALE as i128;
    const HEDGE_POSITION_Q: i128 = 100 * POS_SCALE as i128;
    const LIEN_GROWTH_Q: i128 = 20 * POS_SCALE as i128;
    const INITIAL_CAPITAL: u128 = 3_130;
    const COUNTERPARTY_CAPITAL: u128 = 10_000;
    const MAINTENANCE_FEE: u128 = 530;
    const FIRST_BACKING: u128 = 1_500;
    const SECOND_BACKING: u128 = 50_000;
    const EXTRA_CAPITAL: u128 = 500;
    const DOMAIN: u16 = 1;
    const ASSET_INDEX: u16 = 0;
    const EXPIRY_SLOT: u64 = 10;

    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        4,
        1_000,
        1_000,
        500,
        MAINTENANCE_FEE,
    );
    let admin = env.admin.insecure_clone();
    let provider = Keypair::new();
    let cross_owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let fixture_supply =
        INITIAL_CAPITAL + COUNTERPARTY_CAPITAL + 2 * EXTRA_CAPITAL + FIRST_BACKING + SECOND_BACKING;
    let mut mint_account = env.svm.get_account(&env.mint).expect("quote mint");
    let mut mint_state = Mint::unpack(&mint_account.data).expect("decode quote mint");
    mint_state.supply = u64::try_from(fixture_supply).unwrap();
    Mint::pack(mint_state, &mut mint_account.data).expect("encode quote mint");
    env.svm.set_account(env.mint, mint_account).unwrap();

    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, INITIAL_PRICE);
    env.configure_auth_mark_for_asset_as_admin(1, 1, INITIAL_PRICE);
    env.update_backing_fee_policy_with_cu(DOMAIN, 5_000, 2_500);
    env.try_update_per_asset_authority_with_cu(
        &admin,
        Some(&provider),
        ASSET_INDEX,
        processor::ASSET_AUTH_BACKING_BUCKET,
        provider.pubkey().to_bytes(),
    )
    .expect("install an independent backing provider before value is funded");
    env.svm.expire_blockhash();
    env.configure_auth_mark_for_asset_as_admin(0, 1, INITIAL_PRICE);

    let ledger = env.backing_domain_ledger_account();
    let top_up = |env: &mut V16CuEnv, amount: u128| {
        let source = env.token_account(provider.pubkey(), amount as u64);
        let (backing_fee_bps, insurance_share_bps) = env.backing_fee_policy(DOMAIN);
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::TopUpBackingBucket {
                authority_epoch: 0,
                intent_id: 0,
                market_id: 0,
                domain: DOMAIN,
                backing_fee_bps,
                insurance_share_bps,
                amount,
                expiry_slot: EXPIRY_SLOT,
            },
            vec![
                AccountMeta::new(provider.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(ledger, false),
            ],
            &[&provider],
        )
        .expect("independent provider funds backing through TopUpBackingBucket");
    };
    top_up(&mut env, FIRST_BACKING);

    let cross_portfolio = env.create_portfolio(&cross_owner);
    let counterparty_portfolio = env.create_portfolio(&counterparty_owner);
    env.deposit(&cross_owner, cross_portfolio, INITIAL_CAPITAL);
    env.deposit(
        &counterparty_owner,
        counterparty_portfolio,
        COUNTERPARTY_CAPITAL,
    );
    env.trade_asset_with_cu(
        0,
        &cross_owner,
        cross_portfolio,
        &counterparty_owner,
        counterparty_portfolio,
        SOURCE_POSITION_Q,
        INITIAL_PRICE,
        0,
    );
    env.trade_asset_with_cu(
        1,
        &cross_owner,
        cross_portfolio,
        &counterparty_owner,
        counterparty_portfolio,
        HEDGE_POSITION_Q,
        INITIAL_PRICE,
        0,
    );
    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, 105);
    env.push_auth_mark_for_asset_as_admin(1, 2, 95);
    for (portfolio, asset_index) in [
        (counterparty_portfolio, 0),
        (cross_portfolio, 0),
        (counterparty_portfolio, 1),
    ] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations_for_assets(&[asset_index, 1 - asset_index]),
            },
        );
    }
    top_up(&mut env, SECOND_BACKING);
    env.deposit(&cross_owner, cross_portfolio, EXTRA_CAPITAL);
    env.deposit(&counterparty_owner, counterparty_portfolio, EXTRA_CAPITAL);
    let earnings_before =
        env.market_state().1.source_backing_buckets[DOMAIN as usize].utilization_fee_earnings;
    env.try_trade_asset_with_backing_fee_cap_with_cu(
        1,
        &cross_owner,
        cross_portfolio,
        &counterparty_owner,
        counterparty_portfolio,
        LIEN_GROWTH_Q,
        95,
        0,
        5_000,
    )
    .expect("public risk increase accrues a provider utilization fee");
    let generated_earnings = env.market_state().1.source_backing_buckets[DOMAIN as usize]
        .utilization_fee_earnings
        .checked_sub(earnings_before)
        .expect("provider earnings do not decrease");
    assert!(
        generated_earnings > 0,
        "public route must generate earnings"
    );

    env.resolve();
    let payouts = drain_resolved_cohort(
        &mut env,
        &[
            (&cross_owner, cross_portfolio),
            (&counterparty_owner, counterparty_portfolio),
        ],
        "provider-earnings terminal cohort",
    );
    assert!(payouts.iter().all(|payout| *payout > 0));
    env.close_portfolio_with_cu(&cross_owner, cross_portfolio);
    env.close_portfolio_with_cu(&counterparty_owner, counterparty_portfolio);
    let insurance = env.market_state().1.insurance;
    if insurance != 0 {
        let (insurance_destination, _) = env.withdraw_insurance_with_cu(insurance);
        assert_eq!(env.token_amount(insurance_destination), insurance as u64);
    }
    let claim_free = env.market_state().1;
    assert_eq!(claim_free.materialized_portfolio_count, 0);
    assert_eq!(claim_free.c_tot, 0);
    assert_eq!(claim_free.pnl_pos_tot, 0);
    assert_eq!(claim_free.resolved_payout_blocker_count, 0);
    assert_eq!(claim_free.insurance, 0);
    assert!(claim_free.backing_provider_earnings_total >= generated_earnings);

    let admin_destination = env.token_account(admin.pubkey(), 0);
    let close_slab = |env: &mut V16CuEnv| -> Result<u64, String> {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::CloseSlab {
                authority_epoch: env.control_sequences(0).authority_epoch,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new(admin_destination, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(env.mint, false),
            ],
            &[&admin],
        )
    };

    env.svm.warp_to_slot(EXPIRY_SLOT);
    close_slab(&mut env).expect("terminal scan expires provider principal at its consented expiry");
    let expired = env.market_state().1;
    assert_eq!(
        expired.source_backing_buckets[DOMAIN as usize].status,
        BackingBucketStatusV16::Expired,
    );
    assert_eq!(
        expired.source_backing_buckets[DOMAIN as usize].fresh_unliened_backing_num,
        0,
    );
    assert_eq!(
        expired.source_backing_buckets[DOMAIN as usize].valid_liened_backing_num,
        0,
    );
    assert_eq!(
        expired.backing_provider_earnings_total, claim_free.backing_provider_earnings_total,
        "principal expiry must not silently confiscate provider earnings",
    );

    let blocked_market = env.svm.get_account(&env.market).unwrap();
    let blocked_vault = env.svm.get_account(&env.vault).unwrap();
    let blocked = close_slab(&mut env)
        .expect_err("expired but unclaimed provider earnings must currently block CloseSlab");
    assert!(
        blocked.contains("Custom(21)"),
        "unexpected error: {blocked}"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), blocked_market);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), blocked_vault);

    let admin_earnings_destination = env.token_account(admin.pubkey(), 0);
    env.svm.expire_blockhash();
    let admin_withdraw = env.send(
        ProgInstruction::WithdrawBackingBucketEarnings {
            domain: DOMAIN,
            market_id: env.asset_market_id(ASSET_INDEX),
            authority_epoch: env.withdrawal_authority_epoch(
                admin.pubkey(),
                ASSET_INDEX as usize,
                false,
            ),
            amount: expired.backing_provider_earnings_total,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ledger, false),
            AccountMeta::new(admin_earnings_destination, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(admin_withdraw
        .expect_err("marketauth cannot impersonate the independent provider")
        .contains("Custom(8)"));

    let rescue_provider = Keypair::new();
    let rotation_before = env.svm.get_account(&env.market).unwrap();
    let admin_rotation = env.try_update_per_asset_authority_with_cu(
        &admin,
        Some(&rescue_provider),
        ASSET_INDEX,
        processor::ASSET_AUTH_BACKING_BUCKET,
        rescue_provider.pubkey().to_bytes(),
    );
    assert!(admin_rotation
        .expect_err("asset admin cannot seize a funded provider role")
        .contains("Custom(21)"));
    assert_eq!(env.svm.get_account(&env.market).unwrap(), rotation_before);

    // The permissionless terminal branch pays only the configured provider. A caller cannot pair
    // the provider identity with a market-admin destination and redirect earned fees.
    let redirect_market_before = env.svm.get_account(&env.market).unwrap();
    let redirect_vault_before = env.svm.get_account(&env.vault).unwrap();
    let redirect_ledger_before = env.svm.get_account(&ledger).unwrap();
    env.svm.expire_blockhash();
    let redirected = env.send(
        ProgInstruction::WithdrawBackingBucketEarnings {
            domain: DOMAIN,
            market_id: env.asset_market_id(ASSET_INDEX),
            authority_epoch: env.withdrawal_authority_epoch(
                provider.pubkey(),
                ASSET_INDEX as usize,
                false,
            ),
            amount: expired.backing_provider_earnings_total,
        },
        vec![
            AccountMeta::new_readonly(provider.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ledger, false),
            AccountMeta::new(admin_earnings_destination, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        redirected.is_err(),
        "terminal provider earnings cannot be redirected"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        redirect_market_before
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        redirect_vault_before
    );
    assert_eq!(
        env.svm.get_account(&ledger).unwrap(),
        redirect_ledger_before
    );
    assert_eq!(env.token_amount(admin_earnings_destination), 0);

    // Preserve the provider's earned entitlement while removing its signature as a terminal
    // liveness dependency: any caller may transfer the exact claim only to a provider-owned token
    // account once the market is Resolved and claim-free.
    let provider_destination = env.token_account(provider.pubkey(), 0);
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::WithdrawBackingBucketEarnings {
            domain: DOMAIN,
            market_id: env.asset_market_id(ASSET_INDEX),
            authority_epoch: env.withdrawal_authority_epoch(
                provider.pubkey(),
                ASSET_INDEX as usize,
                false,
            ),
            amount: expired.backing_provider_earnings_total,
        },
        vec![
            AccountMeta::new_readonly(provider.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ledger, false),
            AccountMeta::new(provider_destination, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    )
    .expect("any caller can pay terminal earnings to the configured provider");
    assert_eq!(
        env.token_amount(provider_destination),
        expired.backing_provider_earnings_total as u64,
    );
    close_slab(&mut env).expect("terminal cleanup succeeds after permissionless provider payout");
    assert_closed_market_tombstone(&env.svm.get_account(&env.market).unwrap());
}

#[test]
fn v16_program_close_slab_final_dust_destination_validation_is_atomic() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    env.resolve();
    env.set_token_account_amount(env.vault, env.mint, env.vault_authority, 7);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    let close_slab_to = |env: &mut V16CuEnv, dest: Pubkey| -> Result<u64, String> {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::CloseSlab { authority_epoch: 0 },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new(dest, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&admin],
        )
    };
    let assert_rejected_close_unchanged = |env: &V16CuEnv, label: &str| {
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_before,
            "{label}: market slab must not be zeroed",
        );
        assert_eq!(
            env.svm.get_account(&env.vault).unwrap(),
            vault_before,
            "{label}: primary vault must not be transferred or closed",
        );
    };

    let wrong_mint = Pubkey::new_unique();
    let wrong_mint_dest = env.token_account_for_mint(wrong_mint, admin.pubkey(), 0);
    let wrong_mint_close = close_slab_to(&mut env, wrong_mint_dest);
    assert!(
        wrong_mint_close.is_err(),
        "CloseSlab must reject a wrong-mint primary destination",
    );
    assert_eq!(
        env.token_amount(wrong_mint_dest),
        0,
        "wrong-mint destination receives nothing",
    );
    assert_rejected_close_unchanged(&env, "wrong-mint primary destination rejection");

    let foreign_dest = env.token_account_for_mint(env.mint, Pubkey::new_unique(), 0);
    let foreign_close = close_slab_to(&mut env, foreign_dest);
    assert!(
        foreign_close.is_err(),
        "CloseSlab must reject a third-party primary destination",
    );
    assert_eq!(
        env.token_amount(foreign_dest),
        0,
        "foreign destination receives nothing",
    );
    assert_rejected_close_unchanged(&env, "foreign primary destination rejection");

    let good_dest = env.token_account(admin.pubkey(), 0);
    let good_close = close_slab_to(&mut env, good_dest);
    assert!(
        good_close.is_ok(),
        "valid CloseSlab still recovers final vault dust: {good_close:?}",
    );
    assert_eq!(
        env.token_amount(good_dest),
        7,
        "primary vault dust is recovered to current market authority",
    );
    let closed_market = env.svm.get_account(&env.market).unwrap();
    assert_closed_market_tombstone(&closed_market);
}

// security.md sweep — CloseSlab with secondary collateral (#44/#48): if a secondary collateral mint is
// configured, closing the slab must not zero the market after closing only the primary vault. Any
// secondary reserve must be recovered atomically in the same close, or the PDA-held reserve is stranded.
#[test]
fn v16_attack_close_slab_requires_secondary_vault_recovery() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let secondary_mint = env.create_mint();
    env.update_base_unit_mints_with_cu(env.mint, secondary_mint);
    let secondary_vault = canonical_vault_ata(env.vault_authority, secondary_mint);
    env.svm
        .set_account(
            secondary_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(secondary_mint, env.vault_authority, 50),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.resolve();
    let market_before = env.svm.get_account(&env.market).unwrap();

    let primary_dest = env.token_account(admin.pubkey(), 0);
    env.svm.expire_blockhash();
    let primary_only = env.send(
        ProgInstruction::CloseSlab { authority_epoch: 0 },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new(primary_dest, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        primary_only.is_err(),
        "CloseSlab must reject when a configured secondary vault is omitted"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "omitted-secondary close leaves market intact"
    );
    assert_eq!(
        env.token_amount(secondary_vault),
        50,
        "secondary reserve remains recoverable after rejected close"
    );

    env.svm
        .set_account(
            env.vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, env.vault_authority, 7),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let primary_dest = env.token_account(admin.pubkey(), 0);
    let wrong_secondary_dest = env.token_account_for_mint(secondary_mint, Pubkey::new_unique(), 0);
    env.svm.expire_blockhash();
    let wrong_secondary_dest_close = env.send(
        ProgInstruction::CloseSlab { authority_epoch: 0 },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new(primary_dest, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(secondary_vault, false),
            AccountMeta::new(wrong_secondary_dest, false),
        ],
        &[&admin],
    );
    assert!(
        wrong_secondary_dest_close.is_err(),
        "CloseSlab must reject before closing any vault when the secondary destination is wrong"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "bad secondary destination leaves market intact"
    );
    assert_eq!(
        env.token_amount(env.vault),
        7,
        "primary vault dust remains recoverable"
    );
    assert_eq!(
        env.token_amount(primary_dest),
        0,
        "primary dust not paid before bad secondary validation"
    );
    assert_eq!(
        env.token_amount(secondary_vault),
        50,
        "secondary vault remains recoverable"
    );
    assert_eq!(
        env.token_amount(wrong_secondary_dest),
        0,
        "wrong secondary destination receives nothing"
    );

    let primary_dest = env.token_account(admin.pubkey(), 0);
    let secondary_dest = env.token_account_for_mint(secondary_mint, admin.pubkey(), 0);
    env.svm.expire_blockhash();
    let close_both = env.send(
        ProgInstruction::CloseSlab { authority_epoch: 0 },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new(primary_dest, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(secondary_vault, false),
            AccountMeta::new(secondary_dest, false),
        ],
        &[&admin],
    );
    assert!(
        close_both.is_ok(),
        "CloseSlab closes both configured vaults: {:?}",
        close_both
    );
    assert_eq!(
        env.token_amount(primary_dest),
        7,
        "primary vault dust recovered to admin"
    );
    assert_eq!(
        env.token_amount(secondary_dest),
        50,
        "secondary reserve recovered to admin"
    );
    let closed_market = env.svm.get_account(&env.market).unwrap();
    assert_closed_market_tombstone(&closed_market);
}

// SOL-010 / account-kind confusion: InitPortfolio is permissionless and targets a program-owned
// writable account. Passing the market slab itself as the portfolio target must reject atomically;
// SOL-010 (reinitialization): InitPortfolio targets a program-owned account and SETS its owner. An
// attacker could try to re-init a VICTIM's already-funded portfolio -- which would reset its capital
// and reassign ownership, a severe LOF (victim's vaulted tokens orphaned). The is_initialized guard
// SOL-010 / DoS: an initialized-but-empty portfolio still increments materialized_portfolio_count.
// Reinitializing it must not register the same account twice, or one legitimate ClosePortfolio would
// LOF (fund-stranding): ClosePortfolio zeroes the account and reclaims its rent. If it allowed closing
// a portfolio with non-zero capital, those vaulted tokens would be orphaned -- and this would NOT trip
// conservation (vault still >= c_tot + insurance; the tokens just become unwithdrawable). The closable
// Regression for the marketauth terminal-cleanup privilege: it is only a liveness tool for already
// closable empty portfolios. It must not let marketauth skip CloseResolved and burn a user's pending
// payout/capital during market wind-down.
#[test]
fn v16_attack_marketauth_terminal_close_cannot_skip_resolved_payout() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);
    env.resolve();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let terminal_close = env.send(
        env.close_portfolio_ix(portfolio),
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&admin],
    );
    assert!(
        terminal_close.is_err(),
        "marketauth terminal cleanup must reject a portfolio with unresolved payout/capital"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected terminal ClosePortfolio must not mutate resolved market accounting"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "rejected terminal ClosePortfolio must not dematerialize the user's payout state"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected terminal ClosePortfolio must not move custody"
    );

    let (dest, _) = env.close_resolved_with_cu(&owner, portfolio);
    assert_eq!(
        env.token_amount(dest),
        1_000,
        "owner still recovers through CloseResolved"
    );
    let (_, group) = env.market_state();
    assert_eq!(
        group.vault, 0,
        "resolved payout drains accounted vault value"
    );
    assert_eq!(
        group.materialized_portfolio_count, 1,
        "CloseResolved pays value; ClosePortfolio performs the separate dematerialization step"
    );

    env.svm.expire_blockhash();
    env.send(
        env.close_portfolio_ix(portfolio),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&owner],
    )
    .expect("owner can dematerialize the empty resolved portfolio after payout");
    assert_eq!(
        env.market_state().1.materialized_portfolio_count,
        0,
        "owner ClosePortfolio completes the wind-down after payout"
    );
}

// Same terminal-cleanup boundary, but for the deferred top-up receipt lane: after a partial resolved
// payout the portfolio can have zero capital while still carrying unpaid user value. Marketauth must
// not be able to dematerialize that receipt before ClaimResolvedPayoutTopup finishes it.
#[test]
fn v16_attack_marketauth_terminal_close_cannot_burn_pending_payout_topup() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    {
        let mut market_account = env.svm.get_account(&env.market).expect("market account");
        let mut portfolio_account = env.svm.get_account(&portfolio).expect("portfolio account");
        let (cfg, mut group) = state::read_market(&market_account.data).unwrap();
        let mut account = state::read_portfolio(&portfolio_account.data).unwrap();
        group.mode = MarketModeV16::Resolved;
        group.resolved_slot = 1;
        group.current_slot = 1;
        group.vault = 60;
        group.payout_snapshot_captured = true;
        group.payout_snapshot = 100;
        group.resolved_payout_ledger = ResolvedPayoutLedgerV16 {
            snapshot_residual: 100,
            terminal_claim_exact_receipts_num: 100 * BOUND_SCALE,
            terminal_claim_bound_unreceipted_num: 0,
            current_payout_rate_num: 100 * BOUND_SCALE,
            current_payout_rate_den: 100 * BOUND_SCALE,
            snapshot_slot: 1,
            payout_halted: false,
            finalized: false,
        };
        account.resolved_payout_receipt =
            percolator::ResolvedPayoutReceiptV16Account::from_runtime(&ResolvedPayoutReceiptV16 {
                present: true,
                prior_bound_contribution_num: 100 * BOUND_SCALE,
                live_released_face_at_receipt: 0,
                terminal_positive_claim_face: 100,
                paid_effective: 40,
                finalized: false,
            });
        state::write_market(&mut market_account.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut portfolio_account.data, &account).unwrap();
        env.svm.set_account(env.market, market_account).unwrap();
        env.svm.set_account(portfolio, portfolio_account).unwrap();
    }
    env.set_token_account_amount(env.vault, env.mint, env.vault_authority, 60);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let terminal_close = env.send(
        env.close_portfolio_ix(portfolio),
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&admin],
    );
    assert!(
        terminal_close.is_err(),
        "marketauth terminal cleanup must reject a portfolio with pending payout top-up"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "pending receipt must not be dematerialized"
    );
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    let good_dest = env.token_account_for_mint(env.mint, owner.pubkey(), 0);
    env.claim_resolved_payout_topup_with_cu(owner.pubkey(), portfolio, good_dest);
    assert_eq!(
        env.token_amount(good_dest),
        60,
        "owner receives the pending top-up"
    );
    let account = env.portfolio_state(portfolio);
    assert_eq!(resolved_receipt(&account).paid_effective, 100);
    assert!(resolved_receipt(&account).finalized);

    env.svm.expire_blockhash();
    env.send(
        env.close_portfolio_ix(portfolio),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&owner],
    )
    .expect("owner can close after pending payout top-up is finalized");
    assert_eq!(
        env.market_state().1.materialized_portfolio_count,
        0,
        "receipt-finalized account is closable"
    );
}

#[derive(Clone, Copy)]
struct Inv070TerminalCompositionClass {
    class: &'static str,
    engine_proofs: &'static [&'static str],
    public_witnesses: &'static [&'static str],
}

fn inv070_source_defines_test(source: &str, function: &str) -> bool {
    let marker = format!("fn {function}");
    source.lines().any(|line| {
        line.trim()
            .strip_prefix(&marker)
            .is_some_and(|tail| tail.trim_start().starts_with('('))
    })
}

fn inv070_braced_body_after<'a>(source: &'a str, marker: &str) -> &'a str {
    let start = source
        .find(marker)
        .unwrap_or_else(|| panic!("missing production marker {marker}"));
    let open = start
        + source[start..]
            .find('{')
            .unwrap_or_else(|| panic!("missing body after {marker}"));
    let mut depth = 0i32;
    for (offset, character) in source[open..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &source[(open + 1)..(open + offset)];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated body after {marker}");
}

#[test]
fn v16_program_terminal_stock_and_close_slab_composition_is_source_complete() {
    const ENGINE_PIN: &str = "495a5590c97055bd71c6f94d849ff0298f243145";
    const CLASSES: &[Inv070TerminalCompositionClass] = &[
        Inv070TerminalCompositionClass {
            class: "unsettled accounts, capital, positive claims, and payout receipts",
            engine_proofs: &[
                "proof_v16_public_terminal_insurance_retirement_requires_resolved_ready_accounts",
                "proof_v16_public_terminal_insurance_retirement_rejects_account_capital",
                "proof_v16_public_terminal_insurance_retirement_rejects_positive_source_claim",
            ],
            public_witnesses: &[
                "v16_program_close_slab_rejects_until_market_has_zero_terminal_residue",
                "v16_attack_marketauth_terminal_close_cannot_skip_resolved_payout",
                "v16_attack_marketauth_terminal_close_cannot_burn_pending_payout_topup",
                "v16_program_full_terminal_lifecycle_is_claimant_order_independent",
            ],
        },
        Inv070TerminalCompositionClass {
            class: "fresh backing principal, provider earnings, expiry, and withdrawal",
            engine_proofs: &[
                "proof_v16_public_terminal_insurance_retirement_rejects_provider_earnings",
                "proof_v16_public_terminal_insurance_retirement_rejects_backing_principal",
            ],
            public_witnesses: &[
                "v16_program_resolved_close_normalizes_backing_at_expiry",
                "insurance_spend_composes_through_liquidation_partial_receipt_and_terminal_payout",
                "expired_backing_composes_through_insurance_recredit_and_terminal_slab_cleanup",
            ],
        },
        Inv070TerminalCompositionClass {
            class: "domain insurance budgets, reservations, spent overlap, and recredit",
            engine_proofs: &[
                "proof_v16_public_terminal_insurance_retirement_rejects_every_live_reservation_class",
                "proof_v16_terminal_claim_free_overlap_recredit_is_exactly_bounded",
                "proof_v16_terminal_claim_free_overlap_recredit_updates_only_paired_insurance_domain",
            ],
            public_witnesses: &[
                "v16_program_prior_insurance_frames_all_partial_receipt_orders",
                "expired_backing_composes_through_insurance_recredit_and_terminal_slab_cleanup",
            ],
        },
        Inv070TerminalCompositionClass {
            class: "claim-free protocol surplus and final internal stock retirement",
            engine_proofs: &[
                "proof_v16_terminal_unbudgeted_insurance_retirement_is_exact_and_claim_safe",
                "proof_v16_public_terminal_insurance_retirement_is_exact_and_fully_framed",
            ],
            public_witnesses: &[
                "v16_program_recovery_force_close_reaches_zero_residue_and_close_slab",
                "expired_backing_composes_through_insurance_recredit_and_terminal_slab_cleanup",
                "v16_primary_quote_routes_match_actual_spl_and_internal_accounting_deltas",
            ],
        },
        Inv070TerminalCompositionClass {
            class: "bounded asset scan and persisted strict-progress cursor",
            engine_proofs: &[
                "proof_v16_terminal_slab_asset_step_is_total_and_priority_ordered",
                "proof_v16_terminal_slab_wait_is_error_or_strict_cursor_progress",
            ],
            public_witnesses: &[
                "v16_bpf_terminal_claim_free_surplus_close_stays_bounded_on_10m_market",
                "v16_bpf_terminal_insurance_last_domain_withdraw_stays_bounded_on_10m_market",
            ],
        },
        Inv070TerminalCompositionClass {
            class: "authority, canonical vaults, optional secondary reserve, aliases, and tombstone",
            engine_proofs: &[],
            public_witnesses: &[
                "v16_attack_close_slab_rejects_stale_marketauth_after_rotation",
                "v16_program_close_slab_account_roles_are_exhaustive",
                "v16_attack_close_slab_rejects_foreign_market_vaults",
                "v16_attack_close_slab_requires_secondary_vault_recovery",
            ],
        },
    ];

    let cargo = include_str!("../../../Cargo.toml");
    let lock = include_str!("../../../Cargo.lock");
    assert_eq!(
        cargo.matches(&format!("rev = \"{ENGINE_PIN}\"")).count(),
        2,
        "INV-070 composes exact engine proofs and must reopen on a pin change",
    );
    assert!(
        lock.contains(&format!("rev={ENGINE_PIN}#{ENGINE_PIN}")),
        "Cargo.lock must resolve the same certified engine revision",
    );

    let witness_sources = [
        include_str!("inv_005_authority_incarnation_binding.rs"),
        include_str!("inv_017_signer_writable_role_and_account_alias_safety.rs"),
        include_str!("inv_018_quote_mint_vault_token_program_and_authority_integrity.rs"),
        include_str!("inv_034_domain_and_instance_isolation.rs"),
        include_str!("inv_063_backing_expiry_normalization.rs"),
        include_str!("inv_070_zero_unattributed_terminal_residue_and_close_slab.rs"),
        include_str!("inv_077_bounded_work_and_maximum_shape_compute.rs"),
        include_str!("../stateful/inv_063_backing_expiry_normalization.rs"),
        include_str!("../stateful/inv_066_resolved_payout_fairness_and_order_independence.rs"),
        include_str!("../stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs"),
    ];
    let mut classes = std::collections::BTreeSet::new();
    let mut proofs = std::collections::BTreeSet::new();
    for row in CLASSES {
        assert!(classes.insert(row.class), "duplicate terminal stock class");
        assert!(!row.public_witnesses.is_empty());
        for proof in row.engine_proofs {
            assert!(proofs.insert(*proof), "duplicate engine proof {proof}");
            assert!(proof.starts_with("proof_v16_"));
        }
        for witness in row.public_witnesses {
            assert!(
                witness_sources
                    .iter()
                    .any(|source| inv070_source_defines_test(source, witness)),
                "terminal class '{}' lacks executable public witness {witness}",
                row.class,
            );
        }
    }
    assert_eq!(classes.len(), 6, "terminal stock class roster drift");
    assert_eq!(proofs.len(), 12, "terminal engine proof roster drift");

    let production = include_str!("../../../src/v16_program.rs");
    let production = production
        .split("    #[cfg(test)]\n    mod tests")
        .next()
        .expect("production prefix exists");
    let body = inv070_braced_body_after(production, "fn handle_close_slab<'a>");
    for required in [
        "expect_live_authority(&cfg.marketauth, admin_dest.key)",
        "require_authority_epoch_view(&group, 0, expected_authority_epoch)",
        "group.header.mode != 1",
        "group.header.c_tot.get() != 0",
        "group.header.materialized_portfolio_count.get() != 0",
        "verify_vault_token_account(vault_token, &vault_authority, &primary_mint)",
        "verify_user_token_account(dest_token, admin_dest.key, &primary_mint)",
        ".advance_terminal_slab_not_atomic(authenticated_slot, scan_start)",
        "TerminalSlabOutcomeV16::ScanProgress",
        "TerminalSlabOutcomeV16::BackingExpired",
        "TerminalSlabOutcomeV16::InsuranceRecredited",
        "TerminalSlabOutcomeV16::ReadyToClose",
        ".checked_sub(retired_u64)",
        "burn_tokens_signed(",
        "transfer_tokens_signed(",
        "spl_token::instruction::close_account(",
        "market_ai.realloc(constants::HEADER_LEN, false)",
        "state::write_closed_market_tombstone",
    ] {
        assert!(
            body.contains(required),
            "CloseSlab lost boundary {required}"
        );
    }

    let engine = body
        .find(".advance_terminal_slab_not_atomic(authenticated_slot, scan_start)")
        .expect("terminal engine transition");
    for validation in [
        "verify_vault_token_account(vault_token, &vault_authority, &primary_mint)",
        "verify_user_token_account(dest_token, admin_dest.key, &primary_mint)",
    ] {
        assert!(
            body.find(validation).expect("custody validation") < engine,
            "{validation} must precede terminal engine mutation",
        );
    }
    for effect in [
        "burn_tokens_signed(",
        "transfer_tokens_signed(",
        "spl_token::instruction::close_account(",
        "market_ai.realloc(constants::HEADER_LEN, false)",
        "state::write_closed_market_tombstone",
    ] {
        assert!(
            engine < body.find(effect).expect("terminal external effect"),
            "{effect} must remain after the engine reaches ReadyToClose",
        );
    }
    assert_eq!(
        body.matches(".advance_terminal_slab_not_atomic(").count(),
        1,
        "CloseSlab must have one canonical engine transition",
    );
}
