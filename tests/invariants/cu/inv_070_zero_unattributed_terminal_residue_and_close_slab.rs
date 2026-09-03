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

const INV070_NATIVE_TERMINAL_SURPLUS: u64 = 7;

fn inv070_install_canonical_native_mint(svm: &mut LiteSVM) {
    let native_mint = Mint {
        mint_authority: COption::None,
        supply: 0,
        decimals: spl_token::native_mint::DECIMALS,
        is_initialized: true,
        freeze_authority: COption::None,
    };
    let mut data = vec![0u8; Mint::LEN];
    Mint::pack(native_mint, &mut data).expect("pack canonical native mint");
    svm.set_account(
        spl_token::native_mint::id(),
        Account {
            lamports: svm.minimum_balance_for_rent_exemption(Mint::LEN),
            data,
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .expect("install canonical native-mint chain fixture");
}

fn inv070_public_create_account(
    svm: &mut LiteSVM,
    payer: &Keypair,
    account: &Keypair,
    lamports: u64,
    data_len: usize,
    owner: Pubkey,
) -> u64 {
    send_raw_ixs(
        svm,
        payer,
        vec![system_instruction::create_account(
            &payer.pubkey(),
            &account.pubkey(),
            lamports,
            data_len as u64,
            &owner,
        )],
        &[account],
    )
    .expect("public SystemProgram CreateAccount")
}

fn inv070_public_native_env() -> V16CuEnv {
    let params = V16CuMarketParams {
        initial_price: 1_000_000,
        max_trading_fee_bps: 100,
        max_price_move_bps_per_slot: 100,
        max_accrual_dt_slots: 1,
        ..V16CuMarketParams::default()
    };
    let mut svm = LiteSVM::new();
    let program_id = percolator_prog::id();
    svm.add_program(
        program_id,
        &std::fs::read(program_path()).expect("read exact-parent wrapper SBF"),
    );
    inv070_install_canonical_native_mint(&mut svm);

    let payer = Keypair::new();
    let admin = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000_000)
        .expect("fund test payer");
    svm.airdrop(&admin.pubkey(), 1_000_000_000)
        .expect("materialize market authority");

    let market_keypair = Keypair::new();
    let market = market_keypair.pubkey();
    let market_len = state::market_account_len_for_capacity(1).expect("one-asset market length");
    let market_rent = svm.minimum_balance_for_rent_exemption(market_len);
    inv070_public_create_account(
        &mut svm,
        &payer,
        &market_keypair,
        market_rent,
        market_len,
        program_id,
    );

    let mint = spl_token::native_mint::id();
    let vault_authority = Pubkey::find_program_address(&[b"vault", market.as_ref()], &program_id).0;
    let vault = create_ata_for_test(&mut svm, &payer, vault_authority, mint);
    let vault_account = svm.get_account(&vault).expect("native primary vault ATA");
    let vault_state = TokenAccount::unpack(&vault_account.data).expect("native primary vault");
    let vault_rent = svm.minimum_balance_for_rent_exemption(TokenAccount::LEN);
    assert_eq!(vault_state.is_native, COption::Some(vault_rent));
    assert_eq!(vault_state.amount, 0);
    assert_eq!(vault_account.lamports, vault_rent);

    let init_market_cu = send_tx(
        &mut svm,
        program_id,
        &payer,
        init_market_instruction(&params),
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(market, false),
            AccountMeta::new_readonly(mint, false),
        ],
        &[&admin],
    )
    .expect("public native-wSOL InitMarket");

    V16CuEnv {
        svm,
        program_id,
        payer,
        admin,
        init_market_cu,
        market,
        mint,
        vault,
        vault_authority,
        portfolio_account_len: state::portfolio_account_len_for_market_slots(1)
            .expect("one-asset portfolio length"),
        portfolios: Vec::new(),
    }
}

fn inv070_public_create_portfolio(env: &mut V16CuEnv, owner: &Keypair) -> Pubkey {
    env.svm
        .airdrop(&owner.pubkey(), 1_000_000)
        .expect("materialize portfolio owner");
    let portfolio_keypair = Keypair::new();
    let portfolio = portfolio_keypair.pubkey();
    let rent = env
        .svm
        .minimum_balance_for_rent_exemption(env.portfolio_account_len);
    let payer = env.payer.insecure_clone();
    inv070_public_create_account(
        &mut env.svm,
        &payer,
        &portfolio_keypair,
        rent,
        env.portfolio_account_len,
        env.program_id,
    );
    env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[owner],
    )
    .expect("public InitPortfolio");
    env.portfolios.push(portfolio);
    portfolio
}

fn inv070_public_native_token_account(env: &mut V16CuEnv, owner: Pubkey, amount: u64) -> Pubkey {
    let token_keypair = Keypair::new();
    let token = token_keypair.pubkey();
    let rent = env
        .svm
        .minimum_balance_for_rent_exemption(TokenAccount::LEN);
    let lamports = rent
        .checked_add(amount)
        .expect("native token account lamports");
    let create = system_instruction::create_account(
        &env.payer.pubkey(),
        &token,
        lamports,
        TokenAccount::LEN as u64,
        &spl_token::ID,
    );
    let initialize = spl_token::instruction::initialize_account(
        &spl_token::ID,
        &token,
        &spl_token::native_mint::id(),
        &owner,
    )
    .expect("build InitializeAccount for native wSOL");
    let payer = env.payer.insecure_clone();
    send_raw_ixs(
        &mut env.svm,
        &payer,
        vec![create, initialize],
        &[&token_keypair],
    )
    .expect("publicly create rent-correct native SPL account");

    let account = env.svm.get_account(&token).expect("native SPL account");
    let state = TokenAccount::unpack(&account.data).expect("decode native SPL account");
    assert_eq!(state.mint, spl_token::native_mint::id());
    assert_eq!(state.owner, owner);
    assert_eq!(state.is_native, COption::Some(rent));
    assert_eq!(state.amount, amount);
    assert_eq!(account.lamports, lamports);
    token
}

fn inv070_public_native_deposit(
    env: &mut V16CuEnv,
    owner: &Keypair,
    portfolio: Pubkey,
    amount: u128,
) {
    let amount_u64 = u64::try_from(amount).expect("native deposit amount");
    let source = inv070_public_native_token_account(env, owner.pubkey(), amount_u64);
    env.send(
        env.deposit_ix(portfolio, amount),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[owner],
    )
    .expect("public native-wSOL Deposit");
    assert_eq!(env.token_amount(source), 0);
}

fn inv070_public_native_withdraw(
    env: &mut V16CuEnv,
    owner: &Keypair,
    portfolio: Pubkey,
    amount: u128,
) -> Pubkey {
    let destination = inv070_public_native_token_account(env, owner.pubkey(), 0);
    env.send(
        env.withdraw_ix(portfolio, amount),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(destination, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[owner],
    )
    .expect("public native-wSOL Withdraw");
    assert_eq!(env.token_amount(destination), amount as u64);
    destination
}

fn inv070_certificate_is_current(env: &V16CuEnv, portfolio: Pubkey) -> bool {
    let group = env.market_state().1;
    let account = env.portfolio_state(portfolio);
    let cert = health_cert(&account);
    cert.valid
        && cert.cert_oracle_epoch == group.oracle_epoch
        && cert.cert_funding_epoch == group.funding_epoch
        && cert.cert_risk_epoch == group.risk_epoch
        && cert.cert_asset_set_epoch == group.asset_set_epoch
        && cert.active_bitmap_at_cert == active_bitmap(&account)
}

fn inv070_native_close_slab_accounts(
    env: &V16CuEnv,
    destination: Pubkey,
    sink: Pubkey,
) -> Vec<AccountMeta> {
    vec![
        AccountMeta::new(env.admin.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(env.vault, false),
        AccountMeta::new_readonly(env.vault_authority, false),
        AccountMeta::new(destination, false),
        AccountMeta::new_readonly(spl_token::ID, false),
        // Keep the legacy primary-mint position stable. Native retirement adds the sink after it.
        AccountMeta::new_readonly(env.mint, false),
        AccountMeta::new(sink, false),
    ]
}

// INV-070/077/079: this reaches the terminal residue exclusively through public wrapper, System,
// ATA, and SPL Token instructions. Paid mark movement intentionally creates insurance that is not
// assigned to an operator-withdrawable domain budget. Once all users exit and the market resolves,
// CloseSlab must retire that value without returning it to marketauth. Native wSOL cannot use SPL
// Burn, so the only policy-preserving terminal route is: transfer classified external surplus to
// the validated admin token account, then close the nonempty native vault to Solana's canonical
// incinerator. The wrong-sink attempt also pins account ordering and exact SVM rollback.
#[test]
fn v16_program_native_wsol_terminal_unbudgeted_insurance_retires_to_incinerator() {
    const MARK: u64 = 1_000_000;
    const RAW_UP: u64 = 2_000_000;
    const DEPOSIT: u128 = 25_000_000;

    let mut env = inv070_public_native_env();
    env.configure_ewma_mark_with_cu(0, MARK, 1, 0);
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = inv070_public_create_portfolio(&mut env, &long_owner);
    let short = inv070_public_create_portfolio(&mut env, &short_owner);
    inv070_public_native_deposit(&mut env, &long_owner, long, DEPOSIT);
    inv070_public_native_deposit(&mut env, &short_owner, short, DEPOSIT);

    env.svm.warp_to_slot(1);
    env.trade_with_cu(
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        RAW_UP,
        0,
    );
    env.crank(
        long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
    );
    env.update_asset_lifecycle_as_admin_with_cu(processor::ASSET_ACTION_DRAIN_ONLY, 0, 0, 0);
    for _ in 0..6 {
        for portfolio in [long, short] {
            if !inv070_certificate_is_current(&env, portfolio) {
                env.crank(
                    portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 1,
                        observations: vec![],
                    },
                );
            }
        }
        if inv070_certificate_is_current(&env, long) && inv070_certificate_is_current(&env, short) {
            break;
        }
    }
    assert!(
        inv070_certificate_is_current(&env, long) && inv070_certificate_is_current(&env, short),
        "public DrainOnly route reaches current certificates",
    );
    env.trade_with_cu(
        &long_owner,
        long,
        &short_owner,
        short,
        -(POS_SCALE as i128),
        1,
        0,
    );
    let released = env.portfolio_state(long).pnl.get();
    if released > 0 {
        env.convert_released_pnl_with_cu(&long_owner, long, released as u128);
    }
    for (owner, portfolio) in [(&long_owner, long), (&short_owner, short)] {
        let capital = env.portfolio_state(portfolio).capital.get();
        inv070_public_native_withdraw(&mut env, owner, portfolio, capital);
        env.close_portfolio_with_cu(owner, portfolio);
    }

    let terminal = env.market_state().1;
    let retired = terminal.insurance;
    assert!(retired > 0, "paid mark movement creates terminal insurance");
    assert_eq!(terminal.insurance_domain_budget.iter().sum::<u128>(), 0);
    assert_eq!(terminal.vault, retired);
    assert_eq!(terminal.c_tot, 0);
    assert_eq!(terminal.materialized_portfolio_count, 0);

    let payer = env.payer.insecure_clone();
    let admin = env.admin.insecure_clone();
    let surplus_source = inv070_public_native_token_account(
        &mut env,
        admin.pubkey(),
        INV070_NATIVE_TERMINAL_SURPLUS,
    );
    send_raw_tx(
        &mut env.svm,
        &payer,
        spl_token::instruction::transfer(
            &spl_token::ID,
            &surplus_source,
            &env.vault,
            &admin.pubkey(),
            &[],
            INV070_NATIVE_TERMINAL_SURPLUS,
        )
        .expect("build public native-surplus transfer"),
        &[&admin],
    )
    .expect("publicly add unaccounted native surplus");
    env.resolve();

    let destination = inv070_public_native_token_account(&mut env, admin.pubkey(), 0);
    let wrong_sink = Pubkey::new_unique();
    let market_before = env.svm.get_account(&env.market).expect("terminal market");
    let vault_before = env
        .svm
        .get_account(&env.vault)
        .expect("terminal native vault");
    let destination_before = env
        .svm
        .get_account(&destination)
        .expect("admin native destination");
    let wrong_sink_before = env.svm.get_account(&wrong_sink);
    env.svm.expire_blockhash();
    let wrong_sink_result = env.send(
        ProgInstruction::CloseSlab { authority_epoch: 0 },
        inv070_native_close_slab_accounts(&env, destination, wrong_sink),
        &[&admin],
    );
    assert!(
        wrong_sink_result.is_err(),
        "native terminal retirement must require the canonical incinerator",
    );
    assert_eq!(
        env.svm.get_account(&env.market),
        Some(market_before.clone())
    );
    assert_eq!(env.svm.get_account(&env.vault), Some(vault_before.clone()));
    assert_eq!(
        env.svm.get_account(&destination),
        Some(destination_before.clone())
    );
    assert_eq!(env.svm.get_account(&wrong_sink), wrong_sink_before);

    let incinerator = solana_sdk::incinerator::id();
    let mut readonly_sink_accounts =
        inv070_native_close_slab_accounts(&env, destination, incinerator);
    *readonly_sink_accounts
        .last_mut()
        .expect("native retirement sink account") = AccountMeta::new_readonly(incinerator, false);
    env.svm.expire_blockhash();
    let readonly_sink_result = env.send(
        ProgInstruction::CloseSlab { authority_epoch: 0 },
        readonly_sink_accounts,
        &[&admin],
    );
    assert!(
        readonly_sink_result.is_err(),
        "native terminal retirement sink must be writable",
    );
    assert_eq!(
        env.svm.get_account(&env.market),
        Some(market_before.clone())
    );
    assert_eq!(env.svm.get_account(&env.vault), Some(vault_before.clone()));
    assert_eq!(
        env.svm.get_account(&destination),
        Some(destination_before.clone())
    );

    let vault_state_before = TokenAccount::unpack(&vault_before.data).expect("native vault state");
    assert_eq!(
        vault_state_before.amount,
        retired as u64 + INV070_NATIVE_TERMINAL_SURPLUS
    );
    let vault_lamports_before = vault_before.lamports;
    let destination_lamports_before = destination_before.lamports;
    let incinerator_lamports_before = env.svm.get_balance(&incinerator).unwrap_or(0);
    let mint_before = env
        .svm
        .get_account(&env.mint)
        .expect("native mint before close");
    let native_supply_before = Mint::unpack(&mint_before.data)
        .expect("decode native mint")
        .supply;
    let admin_lamports_before = env.svm.get_balance(&admin.pubkey()).unwrap_or(0);
    let market_lamports_before = market_before.lamports;
    let retained_market_lamports = env
        .svm
        .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN);

    env.svm.expire_blockhash();
    let close_cu = env
        .send(
            ProgInstruction::CloseSlab { authority_epoch: 0 },
            inv070_native_close_slab_accounts(&env, destination, incinerator),
            &[&admin],
        )
        .expect("native terminal residue must retire through canonical incinerator");
    eprintln!("INV-070 native terminal CloseSlab CU: {close_cu}");
    assert_cu_within("native terminal CloseSlab", close_cu, CUSTODY_CU_LIMIT);

    assert_closed_market_tombstone(&env.svm.get_account(&env.market).expect("market tombstone"));
    if let Some(closed_vault) = env.svm.get_account(&env.vault) {
        assert_eq!(closed_vault.lamports, 0, "closed vault has no lamports");
        assert!(
            closed_vault.data.is_empty(),
            "closed vault has no token data"
        );
        assert_eq!(
            closed_vault.owner,
            solana_sdk::system_program::ID,
            "closed vault is returned to the System Program",
        );
    }
    let destination_after = env
        .svm
        .get_account(&destination)
        .expect("admin native destination after close");
    let destination_state =
        TokenAccount::unpack(&destination_after.data).expect("admin native destination state");
    assert_eq!(
        destination_state.amount, INV070_NATIVE_TERMINAL_SURPLUS,
        "admin receives only classified external surplus",
    );
    assert_eq!(
        destination_after.lamports,
        destination_lamports_before + INV070_NATIVE_TERMINAL_SURPLUS,
    );
    assert_eq!(
        env.svm.get_balance(&incinerator).unwrap_or(0),
        incinerator_lamports_before + vault_lamports_before - INV070_NATIVE_TERMINAL_SURPLUS,
        "retired insurance and native-vault rent go only to the canonical sink",
    );
    assert_eq!(
        env.svm.get_balance(&admin.pubkey()).unwrap_or(0),
        admin_lamports_before + market_lamports_before - retained_market_lamports,
        "marketauth receives market-account rent only, never retired native insurance or vault rent",
    );
    let mint_after = env
        .svm
        .get_account(&env.mint)
        .expect("native mint after close");
    assert_eq!(
        Mint::unpack(&mint_after.data)
            .expect("decode native mint after close")
            .supply,
        native_supply_before,
        "native supply remains unchanged because wrapped SOL supply is lamport-backed, not minted",
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
