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

struct Inv070TerminalInsuranceWithholdFixture {
    env: V16CuEnv,
    insurance_authority: Keypair,
    cranker: Keypair,
    user_payouts: [u64; 2],
    permissionless_create_cu: u64,
    insurance_top_up_cu: u64,
    resolve_cu: u64,
}

fn inv070_create_rent_funded_portfolio(env: &mut V16CuEnv, owner: &Keypair) -> Pubkey {
    let portfolio = Pubkey::new_unique();
    env.ensure_signer_account(owner.pubkey());
    let configured_assets = env.market_state().1.config.max_market_slots as usize;
    let data_len = state::portfolio_account_len_for_market_slots(configured_assets).unwrap();
    let lamports = env
        .svm
        .get_sysvar::<solana_sdk::rent::Rent>()
        .minimum_balance(data_len);
    env.svm
        .set_account(
            portfolio,
            Account {
                lamports,
                data: vec![0u8; data_len],
                owner: env.program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[owner],
    )
    .expect("initialize rent-funded portfolio through the public wrapper");
    portfolio
}

fn inv070_send_with_metadata(
    env: &mut V16CuEnv,
    instruction: ProgInstruction,
    accounts: Vec<AccountMeta>,
    extra_signers: &[&Keypair],
) -> Result<u64, (String, u64)> {
    let instruction = Instruction {
        program_id: env.program_id,
        accounts,
        data: instruction.encode(),
    };
    let mut signers = Vec::with_capacity(1 + extra_signers.len());
    signers.push(&env.payer);
    signers.extend_from_slice(extra_signers);
    let transaction = Transaction::new_signed_with_payer(
        &[heap_ix(), cu_ix(), instruction],
        Some(&env.payer.pubkey()),
        &signers,
        env.svm.latest_blockhash(),
    );
    match env.svm.send_transaction(transaction) {
        Ok(metadata) => Ok(metadata.compute_units_consumed),
        Err(failure) => Err((
            format!("{:?}", failure.err),
            failure.meta.compute_units_consumed,
        )),
    }
}

fn inv070_terminal_insurance_withhold_fixture() -> Inv070TerminalInsuranceWithholdFixture {
    const CREATE_FEE: u128 = 2;
    const INSURANCE_ATOMS: u128 = 1;
    const USER_PRINCIPAL: u128 = 1_000;

    let mut env = V16CuEnv::new();
    env.update_market_init_fee_policy_with_cu(CREATE_FEE);

    let insurance_authority = Keypair::new();
    let cranker = Keypair::new();
    env.ensure_signer_account(cranker.pubkey());
    let authority = insurance_authority.pubkey();
    let (_, permissionless_create_cu) = env.activate_permissionless_asset_with_fee(
        &insurance_authority,
        1,
        1,
        100,
        authority,
        authority,
        authority,
        authority,
        CREATE_FEE,
    );
    let profile =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 1)
            .unwrap();
    assert_eq!(profile.asset_admin, authority.to_bytes());
    assert_eq!(profile.insurance_authority, authority.to_bytes());
    assert_eq!(profile.insurance_operator, authority.to_bytes());

    let (insurance_source, insurance_top_up_cu) =
        env.top_up_insurance_domain_with_authority_and_cu(&insurance_authority, 2, INSURANCE_ATOMS);
    assert_eq!(env.token_amount(insurance_source), 0);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = inv070_create_rent_funded_portfolio(&mut env, &long_owner);
    let short = inv070_create_rent_funded_portfolio(&mut env, &short_owner);
    env.deposit(&long_owner, long, USER_PRINCIPAL);
    env.deposit(&short_owner, short, USER_PRINCIPAL);
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
    env.trade_asset_with_cu(
        1,
        &long_owner,
        long,
        &short_owner,
        short,
        -(POS_SCALE as i128),
        100,
        0,
    );

    let resolve_cu = env.resolve();
    let long_destination = env.close_resolved(&long_owner, long);
    let short_destination = env.close_resolved(&short_owner, short);
    let user_payouts = [
        env.token_amount(long_destination),
        env.token_amount(short_destination),
    ];
    assert_eq!(user_payouts, [USER_PRINCIPAL as u64; 2]);
    env.close_portfolio_with_cu(&long_owner, long);
    env.close_portfolio_with_cu(&short_owner, short);

    // Remove the permissionless-create fee from asset 0 through its legitimate terminal owner.
    // The only remaining token and accounting stock is the attacker's one-atom asset-1 budget.
    let admin = env.admin.insecure_clone();
    let (asset_zero_destination, _) = env
        .try_withdraw_insurance_asset_with_authority(&admin, 0, CREATE_FEE)
        .expect("asset-0 authority recovers the permissionless-create fee");
    assert_eq!(env.token_amount(asset_zero_destination), CREATE_FEE as u64);
    let terminal = env.market_state().1;
    assert_eq!(terminal.mode, MarketModeV16::Resolved);
    assert_eq!(terminal.materialized_portfolio_count, 0);
    assert_eq!(terminal.c_tot, 0);
    assert_eq!(terminal.vault, INSURANCE_ATOMS);
    assert_eq!(terminal.insurance, INSURANCE_ATOMS);
    assert_eq!(terminal.insurance_domain_budget[2], INSURANCE_ATOMS);
    assert_eq!(
        terminal.insurance_domain_budget_remaining_total,
        INSURANCE_ATOMS
    );
    assert_eq!(env.token_amount(env.vault), INSURANCE_ATOMS as u64);

    Inv070TerminalInsuranceWithholdFixture {
        env,
        insurance_authority,
        cranker,
        user_payouts,
        permissionless_create_cu,
        insurance_top_up_cu,
        resolve_cu,
    }
}

// BLOCKER DoS regression: an unprivileged asset creator could retain a one-atom insurance claim
// forever by withholding its insurance-authority signature. Once all users are paid and the market is
// Resolved, account zero remains bound to the configured authority and the destination remains owned
// by it, but the signature is no longer required to deliver that exact entitlement. Live withdrawals
// remain signed. Every rejected identity/destination variant must roll back accounting and SPL state.
#[test]
fn v16_attack_permissionless_asset_insurance_authority_cannot_withhold_terminal_close() {
    const ASSET_INDEX: u16 = 1;
    const INSURANCE_ATOMS: u128 = 1;

    let mut attack = inv070_terminal_insurance_withhold_fixture();
    let admin = attack.env.admin.insecure_clone();
    let replacement = Keypair::new();
    let insurance_authority = attack.insurance_authority.pubkey();
    let cranker = attack.cranker.insecure_clone();
    let admin_destination = attack.env.token_account(admin.pubkey(), 0);
    let attacker_destination = attack.env.token_account(insurance_authority, 0);
    let wrong_authority_destination = attack.env.token_account(cranker.pubkey(), 0);
    let market = attack.env.market;
    let vault = attack.env.vault;
    let vault_authority = attack.env.vault_authority;
    let mint = attack.env.mint;
    let market_id = attack.env.asset_market_id(ASSET_INDEX);
    let authority_epoch = attack
        .env
        .control_sequences(ASSET_INDEX as usize)
        .authority_epoch;

    let market_before = attack.env.svm.get_account(&market).unwrap();
    let vault_before = attack.env.svm.get_account(&vault).unwrap();
    let admin_destination_before = attack.env.svm.get_account(&admin_destination).unwrap();
    let admin_withdraw = inv070_send_with_metadata(
        &mut attack.env,
        ProgInstruction::WithdrawInsuranceAsset {
            asset_index: ASSET_INDEX,
            market_id,
            authority_epoch,
            amount: INSURANCE_ATOMS,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market, false),
            AccountMeta::new(admin_destination, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        admin_withdraw.is_err(),
        "market authority must not seize the creator's terminal insurance entitlement"
    );
    assert_eq!(
        admin_withdraw.as_ref().unwrap_err().0,
        "InstructionError(2, Custom(8))"
    );
    assert_eq!(attack.env.svm.get_account(&market).unwrap(), market_before);
    assert_eq!(attack.env.svm.get_account(&vault).unwrap(), vault_before);
    assert_eq!(
        attack.env.svm.get_account(&admin_destination).unwrap(),
        admin_destination_before
    );

    attack.env.ensure_signer_account(replacement.pubkey());
    attack.env.svm.expire_blockhash();
    let admin_rotation = inv070_send_with_metadata(
        &mut attack.env,
        ProgInstruction::UpdateAssetAuthority {
            asset_index: ASSET_INDEX,
            market_id,
            authority_epoch,
            kind: processor::ASSET_AUTH_INSURANCE,
            new_pubkey: replacement.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(replacement.pubkey(), true),
            AccountMeta::new(market, false),
        ],
        &[&admin, &replacement],
    );
    assert!(
        admin_rotation.is_err(),
        "market authority must not replace a funded permissionless asset's insurance owner"
    );
    assert_eq!(attack.env.svm.get_account(&market).unwrap(), market_before);

    // The configured authority key alone is insufficient if the payout token account belongs to
    // anyone else. This check runs without the authority signature and must still fail atomically.
    attack.env.svm.expire_blockhash();
    let wrong_destination_before = attack.env.svm.get_account(&admin_destination).unwrap();
    let wrong_destination = inv070_send_with_metadata(
        &mut attack.env,
        ProgInstruction::WithdrawInsuranceAsset {
            asset_index: ASSET_INDEX,
            market_id,
            authority_epoch,
            amount: INSURANCE_ATOMS,
        },
        vec![
            AccountMeta::new_readonly(insurance_authority, false),
            AccountMeta::new(market, false),
            AccountMeta::new(admin_destination, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert_eq!(
        wrong_destination.as_ref().unwrap_err().0,
        "InstructionError(2, Custom(11))"
    );
    assert_eq!(attack.env.svm.get_account(&market).unwrap(), market_before);
    assert_eq!(attack.env.svm.get_account(&vault).unwrap(), vault_before);
    assert_eq!(
        attack.env.svm.get_account(&admin_destination).unwrap(),
        wrong_destination_before
    );

    // A permissionless caller also cannot redirect the budget by substituting its own key and a
    // token account it owns. The supplied key must remain the configured insurance authority.
    attack.env.svm.expire_blockhash();
    let wrong_authority_destination_before = attack
        .env
        .svm
        .get_account(&wrong_authority_destination)
        .unwrap();
    let wrong_authority = inv070_send_with_metadata(
        &mut attack.env,
        ProgInstruction::WithdrawInsuranceAsset {
            asset_index: ASSET_INDEX,
            market_id,
            authority_epoch,
            amount: INSURANCE_ATOMS,
        },
        vec![
            AccountMeta::new_readonly(cranker.pubkey(), false),
            AccountMeta::new(market, false),
            AccountMeta::new(wrong_authority_destination, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert_eq!(
        wrong_authority.as_ref().unwrap_err().0,
        "InstructionError(2, Custom(8))"
    );
    assert_eq!(attack.env.svm.get_account(&market).unwrap(), market_before);
    assert_eq!(attack.env.svm.get_account(&vault).unwrap(), vault_before);
    assert_eq!(
        attack
            .env
            .svm
            .get_account(&wrong_authority_destination)
            .unwrap(),
        wrong_authority_destination_before
    );

    let rent = attack.env.svm.get_sysvar::<solana_sdk::rent::Rent>();
    let terminal_market = attack.env.svm.get_account(&market).unwrap();
    let terminal_vault = attack.env.svm.get_account(&vault).unwrap();
    let market_refund = terminal_market
        .lamports
        .saturating_sub(rent.minimum_balance(percolator_prog::constants::HEADER_LEN));
    let rent_locked = market_refund.saturating_add(terminal_vault.lamports);

    // Any payer may now submit the canonical terminal payout: account zero is the configured
    // authority but is deliberately not a signer, and custody can move only to its token account.
    attack.env.svm.expire_blockhash();
    let unsigned_canonical_payout = inv070_send_with_metadata(
        &mut attack.env,
        ProgInstruction::WithdrawInsuranceAsset {
            asset_index: ASSET_INDEX,
            market_id,
            authority_epoch,
            amount: INSURANCE_ATOMS,
        },
        vec![
            AccountMeta::new_readonly(insurance_authority, false),
            AccountMeta::new(market, false),
            AccountMeta::new(attacker_destination, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    )
    .expect("resolved canonical insurance payout is permissionless");
    let drained = attack.env.market_state().1;
    assert_eq!(drained.insurance_domain_budget[2], 0);
    assert_eq!(drained.insurance_domain_budget_remaining_total, 0);
    assert_eq!(drained.insurance, 0);
    assert_eq!(drained.vault, 0);
    assert_eq!(attack.env.token_amount(vault), 0);
    assert_eq!(
        attack.env.token_amount(attacker_destination),
        INSURANCE_ATOMS as u64
    );

    let close_destination = attack.env.token_account(admin.pubkey(), 0);
    attack.env.svm.expire_blockhash();
    let permissionless_close_cu = inv070_send_with_metadata(
        &mut attack.env,
        ProgInstruction::CloseSlab { authority_epoch: 0 },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market, false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(vault_authority, false),
            AccountMeta::new(close_destination, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(mint, false),
        ],
        &[&admin],
    )
    .expect("permissionless terminal payout makes CloseSlab reachable");
    assert_closed_market_tombstone(&attack.env.svm.get_account(&market).unwrap());

    // Compatibility control: the configured authority may still sign and withdraw exactly as before.
    let mut control = inv070_terminal_insurance_withhold_fixture();
    let (control_destination, attacker_withdraw_cu) =
        control.env.withdraw_terminal_insurance_with_authority(
            &control.insurance_authority,
            ASSET_INDEX,
            INSURANCE_ATOMS,
        );
    assert_eq!(
        control.env.token_amount(control_destination),
        INSURANCE_ATOMS as u64
    );
    let cooperative_close_cu = control.env.close_slab_with_cu();
    assert_closed_market_tombstone(&control.env.svm.get_account(&control.env.market).unwrap());

    // The signer relaxation is terminal-only. In Live mode the configured operator key without its
    // signature still fails at the wrapper boundary, with every touched account byte-exactly rolled
    // back by the failed transaction.
    let mut live = V16CuEnv::new();
    let live_admin = live.admin.insecure_clone();
    live.enable_live_insurance_withdrawal();
    live.top_up_insurance_domain_with_authority_and_cu(&live_admin, 0, INSURANCE_ATOMS);
    let live_destination = live.token_account(live_admin.pubkey(), 0);
    let live_market_before = live.svm.get_account(&live.market).unwrap();
    let live_vault_before = live.svm.get_account(&live.vault).unwrap();
    let live_destination_before = live.svm.get_account(&live_destination).unwrap();
    let live_market_id = live.asset_market_id(0);
    let live_authority_epoch = live.withdrawal_authority_epoch(live_admin.pubkey(), 0, true);
    let live_market = live.market;
    let live_vault = live.vault;
    let live_vault_authority = live.vault_authority;
    live.svm.expire_blockhash();
    let live_unsigned = inv070_send_with_metadata(
        &mut live,
        ProgInstruction::WithdrawInsuranceAsset {
            asset_index: 0,
            market_id: live_market_id,
            authority_epoch: live_authority_epoch,
            amount: INSURANCE_ATOMS,
        },
        vec![
            AccountMeta::new_readonly(live_admin.pubkey(), false),
            AccountMeta::new(live_market, false),
            AccountMeta::new(live_destination, false),
            AccountMeta::new(live_vault, false),
            AccountMeta::new_readonly(live_vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert_eq!(
        live_unsigned.as_ref().unwrap_err().0,
        "InstructionError(2, Custom(6))"
    );
    assert_eq!(
        live.svm.get_account(&live.market).unwrap(),
        live_market_before
    );
    assert_eq!(
        live.svm.get_account(&live.vault).unwrap(),
        live_vault_before
    );
    assert_eq!(
        live.svm.get_account(&live_destination).unwrap(),
        live_destination_before
    );

    println!(
        "INV-070 terminal insurance withholding: users={:?}, create_cu={}, topup_cu={}, resolve_cu={}, admin_withdraw={:?}, admin_rotation={:?}, wrong_destination={:?}, wrong_authority={:?}, unsigned_canonical_payout_cu={}, permissionless_close_cu={}, live_unsigned={:?}, market_lamports={}, market_refund={}, vault_lamports={}, rent_locked={}, attacker_withdraw_cu={}, cooperative_close_cu={}",
        attack.user_payouts,
        attack.permissionless_create_cu,
        attack.insurance_top_up_cu,
        attack.resolve_cu,
        admin_withdraw,
        admin_rotation,
        wrong_destination,
        wrong_authority,
        unsigned_canonical_payout,
        permissionless_close_cu,
        live_unsigned,
        terminal_market.lamports,
        market_refund,
        terminal_vault.lamports,
        rent_locked,
        attacker_withdraw_cu,
        cooperative_close_cu,
    );
}

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
