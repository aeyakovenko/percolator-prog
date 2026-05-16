use percolator::{
    MarketGroupV13, MarketModeV13, PermissionlessRecoveryReasonV13, PortfolioAccountV13, V13Config,
    POS_SCALE,
};
use percolator_prog::{
    constants::{MARKET_ACCOUNT_LEN, PORTFOLIO_ACCOUNT_LEN},
    ix::Instruction,
    processor, state,
};
use solana_program::{
    account_info::AccountInfo, program_error::ProgramError, program_option::COption,
    program_pack::Pack, pubkey::Pubkey,
};
use spl_token::state::{Account as TokenAccount, AccountState, Mint};

struct TestAccount {
    key: Pubkey,
    owner: Pubkey,
    lamports: u64,
    data: Vec<u8>,
    is_signer: bool,
    is_writable: bool,
    executable: bool,
}

impl TestAccount {
    fn new(key: Pubkey, owner: Pubkey, data_len: usize) -> Self {
        Self {
            key,
            owner,
            lamports: 1_000_000,
            data: vec![0u8; data_len],
            is_signer: false,
            is_writable: false,
            executable: false,
        }
    }

    fn new_with_data(key: Pubkey, owner: Pubkey, data: Vec<u8>) -> Self {
        Self {
            key,
            owner,
            lamports: 1_000_000,
            data,
            is_signer: false,
            is_writable: false,
            executable: false,
        }
    }

    fn signer(mut self) -> Self {
        self.is_signer = true;
        self
    }

    fn writable(mut self) -> Self {
        self.is_writable = true;
        self
    }

    fn executable(mut self) -> Self {
        self.is_writable = false;
        self.executable = true;
        self
    }

    fn to_info<'a>(&'a mut self) -> AccountInfo<'a> {
        AccountInfo::new(
            &self.key,
            self.is_signer,
            self.is_writable,
            &mut self.lamports,
            &mut self.data,
            &self.owner,
            self.executable,
            0,
        )
    }
}

fn program_id() -> Pubkey {
    percolator_prog::id()
}

fn signer() -> TestAccount {
    TestAccount::new(Pubkey::new_unique(), Pubkey::new_unique(), 0).signer()
}

fn market_account() -> TestAccount {
    TestAccount::new(Pubkey::new_unique(), program_id(), MARKET_ACCOUNT_LEN).writable()
}

fn portfolio_account() -> TestAccount {
    TestAccount::new(Pubkey::new_unique(), program_id(), PORTFOLIO_ACCOUNT_LEN).writable()
}

fn make_mint_data() -> Vec<u8> {
    let mut data = vec![0u8; Mint::LEN];
    Mint::pack(
        Mint {
            mint_authority: COption::None,
            supply: 0,
            decimals: 0,
            is_initialized: true,
            freeze_authority: COption::None,
        },
        &mut data,
    )
    .unwrap();
    data
}

fn make_token_data(mint: Pubkey, owner: Pubkey, amount: u64) -> Vec<u8> {
    make_token_data_with_controls(mint, owner, amount, COption::None, COption::None)
}

fn make_token_data_with_state(
    mint: Pubkey,
    owner: Pubkey,
    amount: u64,
    state: AccountState,
) -> Vec<u8> {
    make_token_data_full(mint, owner, amount, COption::None, COption::None, state)
}

fn make_token_data_with_controls(
    mint: Pubkey,
    owner: Pubkey,
    amount: u64,
    delegate: COption<Pubkey>,
    close_authority: COption<Pubkey>,
) -> Vec<u8> {
    make_token_data_full(
        mint,
        owner,
        amount,
        delegate,
        close_authority,
        AccountState::Initialized,
    )
}

fn make_token_data_full(
    mint: Pubkey,
    owner: Pubkey,
    amount: u64,
    delegate: COption<Pubkey>,
    close_authority: COption<Pubkey>,
    state: AccountState,
) -> Vec<u8> {
    let delegated_amount = if delegate.is_some() { amount } else { 0 };
    let mut data = vec![0u8; TokenAccount::LEN];
    TokenAccount::pack(
        TokenAccount {
            mint,
            owner,
            amount,
            delegate,
            state,
            is_native: COption::None,
            delegated_amount,
            close_authority,
        },
        &mut data,
    )
    .unwrap();
    data
}

fn mint_account() -> TestAccount {
    TestAccount::new_with_data(Pubkey::new_unique(), spl_token::ID, make_mint_data())
}

fn invalid_mint_account() -> TestAccount {
    TestAccount::new_with_data(Pubkey::new_unique(), Pubkey::new_unique(), make_mint_data())
}

fn user_token_account(owner: Pubkey, mint: Pubkey, amount: u64) -> TestAccount {
    TestAccount::new_with_data(
        Pubkey::new_unique(),
        spl_token::ID,
        make_token_data(mint, owner, amount),
    )
    .writable()
}

fn user_token_account_with_state(
    owner: Pubkey,
    mint: Pubkey,
    amount: u64,
    state: AccountState,
) -> TestAccount {
    TestAccount::new_with_data(
        Pubkey::new_unique(),
        spl_token::ID,
        make_token_data_with_state(mint, owner, amount, state),
    )
    .writable()
}

fn vault_authority(market: &TestAccount) -> Pubkey {
    Pubkey::find_program_address(&[b"vault", market.key.as_ref()], &program_id()).0
}

fn vault_token_account(market: &TestAccount, mint: Pubkey, amount: u64) -> TestAccount {
    TestAccount::new_with_data(
        Pubkey::new_unique(),
        spl_token::ID,
        make_token_data(mint, vault_authority(market), amount),
    )
    .writable()
}

fn vault_token_account_with_state(
    market: &TestAccount,
    mint: Pubkey,
    amount: u64,
    state: AccountState,
) -> TestAccount {
    TestAccount::new_with_data(
        Pubkey::new_unique(),
        spl_token::ID,
        make_token_data_with_state(mint, vault_authority(market), amount, state),
    )
    .writable()
}

fn vault_token_account_with_controls(
    market: &TestAccount,
    mint: Pubkey,
    amount: u64,
    delegate: COption<Pubkey>,
    close_authority: COption<Pubkey>,
) -> TestAccount {
    TestAccount::new_with_data(
        Pubkey::new_unique(),
        spl_token::ID,
        make_token_data_with_controls(
            mint,
            vault_authority(market),
            amount,
            delegate,
            close_authority,
        ),
    )
    .writable()
}

fn vault_authority_account(market: &TestAccount) -> TestAccount {
    TestAccount::new(vault_authority(market), Pubkey::new_unique(), 0)
}

fn token_program_account() -> TestAccount {
    TestAccount::new(spl_token::ID, Pubkey::default(), 0).executable()
}

fn non_executable_token_program_account() -> TestAccount {
    TestAccount::new(spl_token::ID, Pubkey::default(), 0)
}

fn run_ix(ix: Instruction, accounts: &mut [&mut TestAccount]) -> Result<(), ProgramError> {
    let infos: Vec<AccountInfo> = accounts.iter_mut().map(|a| a.to_info()).collect();
    processor::process_instruction(&program_id(), &infos, &ix.encode())
}

fn init_market(admin: &mut TestAccount, market: &mut TestAccount) -> Pubkey {
    let mut mint = mint_account();
    let mint_key = mint.key;
    run_ix(
        Instruction::InitMarket {
            h_min: 0,
            h_max: 10,
            initial_price: 100,
            maintenance_margin_bps: 10_000,
            initial_margin_bps: 10_000,
            max_trading_fee_bps: 10_000,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            maintenance_fee_per_slot: 0,
        },
        &mut [admin, market, &mut mint],
    )
    .unwrap();
    mint_key
}

fn init_portfolio(owner: &mut TestAccount, market: &mut TestAccount, portfolio: &mut TestAccount) {
    run_ix(Instruction::InitPortfolio, &mut [owner, market, portfolio]).unwrap();
}

fn deposit(
    owner: &mut TestAccount,
    market: &mut TestAccount,
    portfolio: &mut TestAccount,
    amount: u128,
) {
    let mint = Pubkey::new_from_array(state::read_market(&market.data).unwrap().0.collateral_mint);
    let amount_u64 = u64::try_from(amount).unwrap();
    let mut source_token = user_token_account(owner.key, mint, amount_u64);
    let mut vault_token = vault_token_account(market, mint, 0);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::Deposit { amount },
        &mut [
            owner,
            market,
            portfolio,
            &mut source_token,
            &mut vault_token,
            &mut token_program,
        ],
    )
    .unwrap();
}

fn withdraw(
    owner: &mut TestAccount,
    market: &mut TestAccount,
    portfolio: &mut TestAccount,
    amount: u128,
) {
    let mint = Pubkey::new_from_array(state::read_market(&market.data).unwrap().0.collateral_mint);
    let amount_u64 = u64::try_from(amount).unwrap();
    let mut dest_token = user_token_account(owner.key, mint, 0);
    let mut vault_token = vault_token_account(market, mint, amount_u64);
    let mut vault_auth = vault_authority_account(market);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::Withdraw { amount },
        &mut [
            owner,
            market,
            portfolio,
            &mut dest_token,
            &mut vault_token,
            &mut vault_auth,
            &mut token_program,
        ],
    )
    .unwrap();
}

fn close_resolved(
    owner: &mut TestAccount,
    market: &mut TestAccount,
    portfolio: &mut TestAccount,
    fee_rate_per_slot: u128,
) {
    let mint = Pubkey::new_from_array(state::read_market(&market.data).unwrap().0.collateral_mint);
    let payout = state::read_portfolio(&portfolio.data).unwrap().capital;
    let payout_u64 = u64::try_from(payout).unwrap();
    let mut dest_token = user_token_account(owner.key, mint, 0);
    let mut vault_token = vault_token_account(market, mint, payout_u64);
    let mut vault_auth = vault_authority_account(market);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::CloseResolved { fee_rate_per_slot },
        &mut [
            owner,
            market,
            portfolio,
            &mut dest_token,
            &mut vault_token,
            &mut vault_auth,
            &mut token_program,
        ],
    )
    .unwrap();
}

fn assert_err_and_market_unchanged(
    result: Result<(), ProgramError>,
    market: &TestAccount,
    before: &[u8],
) {
    assert!(result.is_err(), "instruction should reject");
    assert_eq!(
        market.data, before,
        "failed wrapper instruction must not persist partial market mutation"
    );
}

#[test]
fn v13_wrapper_init_binds_market_and_portfolio_provenance() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);

    let (_, group) = state::read_market(&market.data).unwrap();
    let acct = state::read_portfolio(&portfolio.data).unwrap();
    assert_eq!(group.market_group_id, market.key.to_bytes());
    assert_eq!(group.materialized_portfolio_count, 1);
    assert_eq!(
        acct.provenance_header.market_group_id,
        market.key.to_bytes()
    );
    assert_eq!(
        acct.provenance_header.portfolio_account_id,
        portfolio.key.to_bytes()
    );
    assert_eq!(acct.owner, owner.key.to_bytes());
    assert_eq!(group.validate_account_shape(&acct), Ok(()));

    let mut cfg = V13Config::public_user_fund(1, 0, 10);
    cfg.maintenance_margin_bps = 10_000;
    cfg.initial_margin_bps = 10_000;
    cfg.max_trading_fee_bps = 10_000;
    cfg.max_price_move_bps_per_slot = 10_000;
    cfg.max_accrual_dt_slots = 1;
    cfg.min_funding_lifetime_slots = 1;
    let mut expected = MarketGroupV13::new(market.key.to_bytes(), cfg).unwrap();
    expected.assets[0].raw_oracle_target_price = 100;
    expected.assets[0].effective_price = 100;
    expected.assets[0].fund_px_last = 100;
    expected.materialized_portfolio_count = 1;
    assert_eq!(
        group, expected,
        "wrapper heap init must match canonical engine init shape"
    );
}

#[test]
fn v13_wrapper_account_layout_constants_match_serialized_state() {
    assert_eq!(
        MARKET_ACCOUNT_LEN,
        16 + state::wrapper_config_len_for_test() + core::mem::size_of::<MarketGroupV13>(),
        "market account length must exactly cover header + wrapper config + engine state"
    );
    assert_eq!(
        PORTFOLIO_ACCOUNT_LEN,
        16 + core::mem::size_of::<PortfolioAccountV13>(),
        "portfolio account length must exactly cover header + portfolio state"
    );
    assert!(
        state::alignment_note() <= core::mem::align_of::<MarketGroupV13>(),
        "wrapper alignment note should not exceed the market group alignment"
    );

    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();
    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);

    let (cfg, group) = state::read_market(&market.data).unwrap();
    let account = state::read_portfolio(&portfolio.data).unwrap();
    let market_before = market.data.clone();
    let portfolio_before = portfolio.data.clone();
    state::write_market(&mut market.data, &cfg, &group).unwrap();
    state::write_portfolio(&mut portfolio.data, &account).unwrap();
    assert_eq!(
        market.data, market_before,
        "read/write-copy roundtrip must preserve market account bytes"
    );
    assert_eq!(
        portfolio.data, portfolio_before,
        "read/write-copy roundtrip must preserve portfolio account bytes"
    );
}

#[test]
fn v13_wrapper_init_market_rejects_invalid_mint_and_double_init() {
    let mut admin = signer();
    let mut market = market_account();
    let mut bad_mint = invalid_mint_account();

    let before = market.data.clone();
    let invalid_mint = run_ix(
        Instruction::InitMarket {
            h_min: 0,
            h_max: 10,
            initial_price: 100,
            maintenance_margin_bps: 10_000,
            initial_margin_bps: 10_000,
            max_trading_fee_bps: 10_000,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            maintenance_fee_per_slot: 0,
        },
        &mut [&mut admin, &mut market, &mut bad_mint],
    );
    assert_err_and_market_unchanged(invalid_mint, &market, &before);

    let mut good_mint = mint_account();
    run_ix(
        Instruction::InitMarket {
            h_min: 0,
            h_max: 10,
            initial_price: 100,
            maintenance_margin_bps: 10_000,
            initial_margin_bps: 10_000,
            max_trading_fee_bps: 10_000,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            maintenance_fee_per_slot: 0,
        },
        &mut [&mut admin, &mut market, &mut good_mint],
    )
    .unwrap();
    let initialized = market.data.clone();
    let double_init = run_ix(
        Instruction::InitMarket {
            h_min: 0,
            h_max: 10,
            initial_price: 100,
            maintenance_margin_bps: 10_000,
            initial_margin_bps: 10_000,
            max_trading_fee_bps: 10_000,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            maintenance_fee_per_slot: 0,
        },
        &mut [&mut admin, &mut market, &mut good_mint],
    );
    assert_err_and_market_unchanged(double_init, &market, &initialized);
}

#[test]
fn v13_wrapper_init_and_account_meta_guards_fail_before_mutation() {
    let mut admin = signer();
    let mut unsigned_admin = TestAccount::new(admin.key, Pubkey::new_unique(), 0);
    let mut market = market_account();
    let mut mint = mint_account();

    let before_market = market.data.clone();
    let missing_admin_signature = run_ix(
        Instruction::InitMarket {
            h_min: 0,
            h_max: 10,
            initial_price: 100,
            maintenance_margin_bps: 10_000,
            initial_margin_bps: 10_000,
            max_trading_fee_bps: 10_000,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            maintenance_fee_per_slot: 0,
        },
        &mut [&mut unsigned_admin, &mut market, &mut mint],
    );
    assert_err_and_market_unchanged(missing_admin_signature, &market, &before_market);

    market.is_writable = false;
    let nonwritable_market = run_ix(
        Instruction::InitMarket {
            h_min: 0,
            h_max: 10,
            initial_price: 100,
            maintenance_margin_bps: 10_000,
            initial_margin_bps: 10_000,
            max_trading_fee_bps: 10_000,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            maintenance_fee_per_slot: 0,
        },
        &mut [&mut admin, &mut market, &mut mint],
    );
    assert_err_and_market_unchanged(nonwritable_market, &market, &before_market);
    market.is_writable = true;

    market.owner = Pubkey::new_unique();
    let wrong_market_owner = run_ix(
        Instruction::InitMarket {
            h_min: 0,
            h_max: 10,
            initial_price: 100,
            maintenance_margin_bps: 10_000,
            initial_margin_bps: 10_000,
            max_trading_fee_bps: 10_000,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            maintenance_fee_per_slot: 0,
        },
        &mut [&mut admin, &mut market, &mut mint],
    );
    assert_err_and_market_unchanged(wrong_market_owner, &market, &before_market);
    market.owner = program_id();

    init_market(&mut admin, &mut market);
    let initialized_market = market.data.clone();

    let mut owner = signer();
    let mut portfolio = portfolio_account();
    portfolio.is_writable = false;
    let before_portfolio = portfolio.data.clone();
    let nonwritable_portfolio = run_ix(
        Instruction::InitPortfolio,
        &mut [&mut owner, &mut market, &mut portfolio],
    );
    assert_err_and_market_unchanged(nonwritable_portfolio, &market, &initialized_market);
    assert_eq!(portfolio.data, before_portfolio);

    portfolio.is_writable = true;
    portfolio.owner = Pubkey::new_unique();
    let wrong_portfolio_owner = run_ix(
        Instruction::InitPortfolio,
        &mut [&mut owner, &mut market, &mut portfolio],
    );
    assert_err_and_market_unchanged(wrong_portfolio_owner, &market, &initialized_market);
    assert_eq!(portfolio.data, before_portfolio);
}

#[test]
fn v13_wrapper_init_market_rejects_invalid_engine_params_without_mutation() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();

    let before = market.data.clone();
    let zero_price = run_ix(
        Instruction::InitMarket {
            h_min: 0,
            h_max: 10,
            initial_price: 0,
            maintenance_margin_bps: 10_000,
            initial_margin_bps: 10_000,
            max_trading_fee_bps: 10_000,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            maintenance_fee_per_slot: 0,
        },
        &mut [&mut admin, &mut market, &mut mint],
    );
    assert_err_and_market_unchanged(zero_price, &market, &before);

    let zero_dt = run_ix(
        Instruction::InitMarket {
            h_min: 0,
            h_max: 10,
            initial_price: 100,
            maintenance_margin_bps: 10_000,
            initial_margin_bps: 10_000,
            max_trading_fee_bps: 10_000,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 0,
            maintenance_fee_per_slot: 0,
        },
        &mut [&mut admin, &mut market, &mut mint],
    );
    assert_err_and_market_unchanged(zero_dt, &market, &before);

    let zero_price_move = run_ix(
        Instruction::InitMarket {
            h_min: 0,
            h_max: 10,
            initial_price: 100,
            maintenance_margin_bps: 10_000,
            initial_margin_bps: 10_000,
            max_trading_fee_bps: 10_000,
            max_price_move_bps_per_slot: 0,
            max_accrual_dt_slots: 1,
            maintenance_fee_per_slot: 0,
        },
        &mut [&mut admin, &mut market, &mut mint],
    );
    assert_err_and_market_unchanged(zero_price_move, &market, &before);
}

#[test]
fn v13_wrapper_init_portfolio_requires_signer_and_rejects_double_init_without_mutation() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut unsigned_owner = TestAccount::new(owner.key, Pubkey::new_unique(), 0);
    let mut portfolio = portfolio_account();

    init_market(&mut admin, &mut market);

    let before_market = market.data.clone();
    let before_portfolio = portfolio.data.clone();
    let missing_signature = run_ix(
        Instruction::InitPortfolio,
        &mut [&mut unsigned_owner, &mut market, &mut portfolio],
    );
    assert_err_and_market_unchanged(missing_signature, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);

    init_portfolio(&mut owner, &mut market, &mut portfolio);
    let initialized_market = market.data.clone();
    let initialized_portfolio = portfolio.data.clone();
    let double_init = run_ix(
        Instruction::InitPortfolio,
        &mut [&mut owner, &mut market, &mut portfolio],
    );
    assert_err_and_market_unchanged(double_init, &market, &initialized_market);
    assert_eq!(
        portfolio.data, initialized_portfolio,
        "double init must fail before market materialized-count mutation"
    );
}

#[test]
fn v13_wrapper_top_up_insurance_requires_authority_and_updates_vault() {
    let mut admin = signer();
    let mut market = market_account();
    let mut attacker = signer();

    let mint = init_market(&mut admin, &mut market);
    let mut attacker_src = user_token_account(attacker.key, mint, 777);
    let mut admin_src = user_token_account(admin.key, mint, 777);
    let mut vault = vault_token_account(&market, mint, 0);
    let mut token_program = token_program_account();

    let before = market.data.clone();
    let unauthorized = run_ix(
        Instruction::TopUpInsurance { amount: 777 },
        &mut [
            &mut attacker,
            &mut market,
            &mut attacker_src,
            &mut vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(unauthorized, &market, &before);

    run_ix(
        Instruction::TopUpInsurance { amount: 777 },
        &mut [
            &mut admin,
            &mut market,
            &mut admin_src,
            &mut vault,
            &mut token_program,
        ],
    )
    .unwrap();

    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(group.insurance, 777);
    assert_eq!(group.vault, 777);
}

#[test]
fn v13_wrapper_top_up_insurance_rejects_wrong_mint_and_insufficient_source_balance() {
    let mut admin = signer();
    let mut market = market_account();

    let mint = init_market(&mut admin, &mut market);
    let mut wrong_source = user_token_account(admin.key, Pubkey::new_unique(), 777);
    let mut short_source = user_token_account(admin.key, mint, 776);
    let mut vault = vault_token_account(&market, mint, 0);
    let mut token_program = token_program_account();
    let before = market.data.clone();

    let wrong_mint = run_ix(
        Instruction::TopUpInsurance { amount: 777 },
        &mut [
            &mut admin,
            &mut market,
            &mut wrong_source,
            &mut vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(wrong_mint, &market, &before);

    let short_balance = run_ix(
        Instruction::TopUpInsurance { amount: 777 },
        &mut [
            &mut admin,
            &mut market,
            &mut short_source,
            &mut vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(short_balance, &market, &before);
}

#[test]
fn v13_wrapper_update_authority_rotates_admin_with_dual_signature() {
    let mut admin = signer();
    let mut market = market_account();
    let mut new_admin = signer();
    let mut attacker = signer();

    init_market(&mut admin, &mut market);
    let initialized = market.data.clone();

    let missing_new_sig = {
        let mut unsigned_new_admin = TestAccount::new(new_admin.key, Pubkey::new_unique(), 0);
        run_ix(
            Instruction::UpdateAuthority {
                kind: processor::AUTHORITY_ADMIN,
                new_pubkey: new_admin.key.to_bytes(),
            },
            &mut [&mut admin, &mut unsigned_new_admin, &mut market],
        )
    };
    assert_err_and_market_unchanged(missing_new_sig, &market, &initialized);

    let unauthorized_current = run_ix(
        Instruction::UpdateAuthority {
            kind: processor::AUTHORITY_ADMIN,
            new_pubkey: new_admin.key.to_bytes(),
        },
        &mut [&mut attacker, &mut new_admin, &mut market],
    );
    assert_err_and_market_unchanged(unauthorized_current, &market, &initialized);

    run_ix(
        Instruction::UpdateAuthority {
            kind: processor::AUTHORITY_ADMIN,
            new_pubkey: new_admin.key.to_bytes(),
        },
        &mut [&mut admin, &mut new_admin, &mut market],
    )
    .unwrap();
    let (cfg, _) = state::read_market(&market.data).unwrap();
    assert_eq!(cfg.admin, new_admin.key.to_bytes());

    let rotated = market.data.clone();
    let old_admin_resolve = run_ix(Instruction::ResolveMarket, &mut [&mut admin, &mut market]);
    assert_err_and_market_unchanged(old_admin_resolve, &market, &rotated);
    run_ix(
        Instruction::ResolveMarket,
        &mut [&mut new_admin, &mut market],
    )
    .unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(group.mode, MarketModeV13::Resolved);
}

#[test]
fn v13_wrapper_update_authority_rotates_insurance_keys_and_supports_operator_burn() {
    let mut admin = signer();
    let mut market = market_account();
    let mut insurance = signer();
    let mut operator = signer();

    let mint = init_market(&mut admin, &mut market);
    let (cfg, _) = state::read_market(&market.data).unwrap();
    assert_eq!(cfg.admin, admin.key.to_bytes());
    assert_eq!(cfg.insurance_authority, admin.key.to_bytes());
    assert_eq!(cfg.insurance_operator, admin.key.to_bytes());

    run_ix(
        Instruction::UpdateAuthority {
            kind: processor::AUTHORITY_INSURANCE,
            new_pubkey: insurance.key.to_bytes(),
        },
        &mut [&mut admin, &mut insurance, &mut market],
    )
    .unwrap();
    run_ix(
        Instruction::UpdateAuthority {
            kind: processor::AUTHORITY_INSURANCE_OPERATOR,
            new_pubkey: operator.key.to_bytes(),
        },
        &mut [&mut admin, &mut operator, &mut market],
    )
    .unwrap();

    let (cfg, _) = state::read_market(&market.data).unwrap();
    assert_eq!(cfg.insurance_authority, insurance.key.to_bytes());
    assert_eq!(cfg.insurance_operator, operator.key.to_bytes());

    let mut admin_src = user_token_account(admin.key, mint, 1);
    let mut insurance_src = user_token_account(insurance.key, mint, 1);
    let mut vault = vault_token_account(&market, mint, 0);
    let mut token_program = token_program_account();
    let rotated = market.data.clone();
    let old_insurance_auth = run_ix(
        Instruction::TopUpInsurance { amount: 1 },
        &mut [
            &mut admin,
            &mut market,
            &mut admin_src,
            &mut vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(old_insurance_auth, &market, &rotated);
    run_ix(
        Instruction::TopUpInsurance { amount: 1 },
        &mut [
            &mut insurance,
            &mut market,
            &mut insurance_src,
            &mut vault,
            &mut token_program,
        ],
    )
    .unwrap();

    let mut zero_new = TestAccount::new(Pubkey::default(), Pubkey::new_unique(), 0);
    run_ix(
        Instruction::UpdateAuthority {
            kind: processor::AUTHORITY_INSURANCE_OPERATOR,
            new_pubkey: [0u8; 32],
        },
        &mut [&mut operator, &mut zero_new, &mut market],
    )
    .unwrap();
    let (cfg, group) = state::read_market(&market.data).unwrap();
    assert_eq!(cfg.insurance_operator, [0u8; 32]);
    assert_eq!(group.insurance, 1);

    let mut insurance_src = user_token_account(insurance.key, mint, 1);
    let mut vault = vault_token_account(&market, mint, 1);
    run_ix(
        Instruction::TopUpInsurance { amount: 1 },
        &mut [
            &mut insurance,
            &mut market,
            &mut insurance_src,
            &mut vault,
            &mut token_program,
        ],
    )
    .unwrap();

    let mut zero_new = TestAccount::new(Pubkey::default(), Pubkey::new_unique(), 0);
    run_ix(
        Instruction::UpdateAuthority {
            kind: processor::AUTHORITY_INSURANCE,
            new_pubkey: [0u8; 32],
        },
        &mut [&mut insurance, &mut zero_new, &mut market],
    )
    .unwrap();
    let after_burn = market.data.clone();
    let mut dead_src = user_token_account(insurance.key, mint, 1);
    let dead_insurance_auth = run_ix(
        Instruction::TopUpInsurance { amount: 1 },
        &mut [
            &mut insurance,
            &mut market,
            &mut dead_src,
            &mut vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(dead_insurance_auth, &market, &after_burn);
}

#[test]
fn v13_wrapper_update_authority_rejects_unsupported_kind_and_live_admin_burn() {
    let mut admin = signer();
    let mut market = market_account();
    let mut new_key = signer();

    init_market(&mut admin, &mut market);
    let initialized = market.data.clone();

    let hyperp_not_yet_ported = run_ix(
        Instruction::UpdateAuthority {
            kind: processor::AUTHORITY_HYPERP_MARK,
            new_pubkey: new_key.key.to_bytes(),
        },
        &mut [&mut admin, &mut new_key, &mut market],
    );
    assert_err_and_market_unchanged(hyperp_not_yet_ported, &market, &initialized);

    let unknown = run_ix(
        Instruction::UpdateAuthority {
            kind: 99,
            new_pubkey: new_key.key.to_bytes(),
        },
        &mut [&mut admin, &mut new_key, &mut market],
    );
    assert_err_and_market_unchanged(unknown, &market, &initialized);

    let live_admin_burn = run_ix(
        Instruction::UpdateAuthority {
            kind: processor::AUTHORITY_ADMIN,
            new_pubkey: [0u8; 32],
        },
        &mut [&mut admin, &mut new_key, &mut market],
    );
    assert_err_and_market_unchanged(live_admin_burn, &market, &initialized);
}

#[test]
fn v13_wrapper_update_authority_allows_chained_admin_rotation_without_old_key_reuse() {
    let mut admin = signer();
    let mut market = market_account();
    let mut admin_b = signer();
    let mut admin_c = signer();

    init_market(&mut admin, &mut market);
    run_ix(
        Instruction::UpdateAuthority {
            kind: processor::AUTHORITY_ADMIN,
            new_pubkey: admin_b.key.to_bytes(),
        },
        &mut [&mut admin, &mut admin_b, &mut market],
    )
    .unwrap();
    run_ix(
        Instruction::UpdateAuthority {
            kind: processor::AUTHORITY_ADMIN,
            new_pubkey: admin_c.key.to_bytes(),
        },
        &mut [&mut admin_b, &mut admin_c, &mut market],
    )
    .unwrap();

    let rotated = market.data.clone();
    let old_admin = run_ix(Instruction::ResolveMarket, &mut [&mut admin, &mut market]);
    assert_err_and_market_unchanged(old_admin, &market, &rotated);
    let prior_admin = run_ix(Instruction::ResolveMarket, &mut [&mut admin_b, &mut market]);
    assert_err_and_market_unchanged(prior_admin, &market, &rotated);
    run_ix(Instruction::ResolveMarket, &mut [&mut admin_c, &mut market]).unwrap();
}

#[test]
fn v13_wrapper_close_portfolio_rejects_non_empty_and_closes_empty() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 1_000);

    let before = market.data.clone();
    let rejected = run_ix(
        Instruction::ClosePortfolio,
        &mut [&mut owner, &mut market, &mut portfolio],
    );
    assert_err_and_market_unchanged(rejected, &market, &before);
    assert!(state::read_portfolio(&portfolio.data).is_ok());

    withdraw(&mut owner, &mut market, &mut portfolio, 1_000);
    run_ix(
        Instruction::ClosePortfolio,
        &mut [&mut owner, &mut market, &mut portfolio],
    )
    .unwrap();

    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(group.materialized_portfolio_count, 0);
    assert!(
        portfolio.data.iter().all(|b| *b == 0),
        "closed portfolio account should be fully zeroed"
    );
}

#[test]
fn v13_wrapper_deposit_withdraw_roundtrip_preserves_accounting() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 1_000);

    withdraw(&mut owner, &mut market, &mut portfolio, 400);

    let (_, group) = state::read_market(&market.data).unwrap();
    let acct = state::read_portfolio(&portfolio.data).unwrap();
    assert_eq!(acct.capital, 600);
    assert_eq!(group.c_tot, 600);
    assert_eq!(group.vault, 600);
    assert_eq!(group.insurance, 0);
}

#[test]
fn v13_wrapper_multiple_portfolios_same_owner_stay_isolated_and_totals_match() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut counterparty_owner = signer();
    let mut portfolio_a = portfolio_account();
    let mut portfolio_b = portfolio_account();
    let mut counterparty = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio_a);
    init_portfolio(&mut owner, &mut market, &mut portfolio_b);
    init_portfolio(&mut counterparty_owner, &mut market, &mut counterparty);
    deposit(&mut owner, &mut market, &mut portfolio_a, 1_000);
    deposit(&mut owner, &mut market, &mut portfolio_a, 2_000);
    deposit(&mut owner, &mut market, &mut portfolio_b, 3_000);
    deposit(
        &mut counterparty_owner,
        &mut market,
        &mut counterparty,
        100_000,
    );

    let untouched_b = portfolio_b.data.clone();
    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: POS_SCALE as i128,
            exec_price: 100,
            fee_bps: 0,
        },
        &mut [
            &mut owner,
            &mut counterparty_owner,
            &mut market,
            &mut portfolio_a,
            &mut counterparty,
        ],
    )
    .unwrap();

    let (_, group) = state::read_market(&market.data).unwrap();
    let a = state::read_portfolio(&portfolio_a.data).unwrap();
    let b = state::read_portfolio(&portfolio_b.data).unwrap();
    let c = state::read_portfolio(&counterparty.data).unwrap();
    assert_eq!(b.capital, 3_000);
    assert_eq!(
        portfolio_b.data, untouched_b,
        "touching one portfolio must not mutate a sibling portfolio with the same owner"
    );
    assert_eq!(
        group.c_tot,
        a.capital + b.capital + c.capital,
        "market c_tot must equal the sum of materialized portfolio capital in this scenario"
    );
    assert_eq!(group.materialized_portfolio_count, 3);
}

#[test]
fn v13_wrapper_deposit_rejects_without_token_accounts() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);

    let before_market = market.data.clone();
    let before_portfolio = portfolio.data.clone();
    let rejected = run_ix(
        Instruction::Deposit { amount: 1_000 },
        &mut [&mut owner, &mut market, &mut portfolio],
    );
    assert_err_and_market_unchanged(rejected, &market, &before_market);
    assert_eq!(
        portfolio.data, before_portfolio,
        "ledger-only deposits must not be reachable through the public wrapper"
    );
}

#[test]
fn v13_wrapper_deposit_rejects_wrong_mint_and_insufficient_source_balance() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    let mint = init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    let mut wrong_source = user_token_account(owner.key, Pubkey::new_unique(), 1_000);
    let mut source_with_dust = user_token_account(owner.key, mint, 999);
    let mut vault = vault_token_account(&market, mint, 0);
    let mut token_program = token_program_account();

    let before_market = market.data.clone();
    let before_portfolio = portfolio.data.clone();
    let wrong_mint = run_ix(
        Instruction::Deposit { amount: 1_000 },
        &mut [
            &mut owner,
            &mut market,
            &mut portfolio,
            &mut wrong_source,
            &mut vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(wrong_mint, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);

    let short_balance = run_ix(
        Instruction::Deposit { amount: 1_000 },
        &mut [
            &mut owner,
            &mut market,
            &mut portfolio,
            &mut source_with_dust,
            &mut vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(short_balance, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);
}

#[test]
fn v13_wrapper_deposit_rejects_wrong_owner_and_bad_token_program() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut attacker = signer();
    let mut portfolio = portfolio_account();

    let mint = init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    let mut attacker_source = user_token_account(attacker.key, mint, 1_000);
    let mut owner_source = user_token_account(owner.key, mint, 1_000);
    let mut vault = vault_token_account(&market, mint, 0);
    let mut token_program = token_program_account();
    let mut bad_token_program = non_executable_token_program_account();

    let before_market = market.data.clone();
    let before_portfolio = portfolio.data.clone();
    let wrong_owner = run_ix(
        Instruction::Deposit { amount: 1_000 },
        &mut [
            &mut attacker,
            &mut market,
            &mut portfolio,
            &mut attacker_source,
            &mut vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(wrong_owner, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);

    let bad_program = run_ix(
        Instruction::Deposit { amount: 1_000 },
        &mut [
            &mut owner,
            &mut market,
            &mut portfolio,
            &mut owner_source,
            &mut vault,
            &mut bad_token_program,
        ],
    );
    assert_err_and_market_unchanged(bad_program, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);
}

#[test]
fn v13_wrapper_vault_accounts_reject_delegate_and_close_authority() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    let mint = init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 1_000);

    let mut source = user_token_account(owner.key, mint, 1_000);
    let mut delegated_vault = vault_token_account_with_controls(
        &market,
        mint,
        1_000,
        COption::Some(Pubkey::new_unique()),
        COption::None,
    );
    let mut closeable_vault = vault_token_account_with_controls(
        &market,
        mint,
        1_000,
        COption::None,
        COption::Some(Pubkey::new_unique()),
    );
    let mut dest = user_token_account(owner.key, mint, 0);
    let mut vault_auth = vault_authority_account(&market);
    let mut token_program = token_program_account();
    let before_market = market.data.clone();
    let before_portfolio = portfolio.data.clone();

    let deposit_bad_vault = run_ix(
        Instruction::Deposit { amount: 1_000 },
        &mut [
            &mut owner,
            &mut market,
            &mut portfolio,
            &mut source,
            &mut delegated_vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(deposit_bad_vault, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);

    let withdraw_bad_vault = run_ix(
        Instruction::Withdraw { amount: 1 },
        &mut [
            &mut owner,
            &mut market,
            &mut portfolio,
            &mut dest,
            &mut closeable_vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(withdraw_bad_vault, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);

    let mut admin_source = user_token_account(admin.key, mint, 1_000);
    let topup_bad_vault = run_ix(
        Instruction::TopUpInsurance { amount: 1_000 },
        &mut [
            &mut admin,
            &mut market,
            &mut admin_source,
            &mut delegated_vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(topup_bad_vault, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);
}

#[test]
fn v13_wrapper_token_accounts_must_be_initialized_for_custody_paths() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    let mint = init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 1_000);

    let mut frozen_source =
        user_token_account_with_state(owner.key, mint, 1_000, AccountState::Frozen);
    let mut good_source = user_token_account(owner.key, mint, 1_000);
    let mut frozen_dest = user_token_account_with_state(owner.key, mint, 0, AccountState::Frozen);
    let mut good_dest = user_token_account(owner.key, mint, 0);
    let mut frozen_vault =
        vault_token_account_with_state(&market, mint, 1_000, AccountState::Frozen);
    let mut good_vault = vault_token_account(&market, mint, 1_000);
    let mut vault_auth = vault_authority_account(&market);
    let mut token_program = token_program_account();
    let before_market = market.data.clone();
    let before_portfolio = portfolio.data.clone();

    let frozen_deposit_source = run_ix(
        Instruction::Deposit { amount: 1 },
        &mut [
            &mut owner,
            &mut market,
            &mut portfolio,
            &mut frozen_source,
            &mut good_vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(frozen_deposit_source, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);

    let frozen_deposit_vault = run_ix(
        Instruction::Deposit { amount: 1 },
        &mut [
            &mut owner,
            &mut market,
            &mut portfolio,
            &mut good_source,
            &mut frozen_vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(frozen_deposit_vault, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);

    let frozen_withdraw_dest = run_ix(
        Instruction::Withdraw { amount: 1 },
        &mut [
            &mut owner,
            &mut market,
            &mut portfolio,
            &mut frozen_dest,
            &mut good_vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(frozen_withdraw_dest, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);

    let frozen_withdraw_vault = run_ix(
        Instruction::Withdraw { amount: 1 },
        &mut [
            &mut owner,
            &mut market,
            &mut portfolio,
            &mut good_dest,
            &mut frozen_vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(frozen_withdraw_vault, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);

    let mut admin_source = user_token_account(admin.key, mint, 1_000);
    let frozen_topup_vault = run_ix(
        Instruction::TopUpInsurance { amount: 1 },
        &mut [
            &mut admin,
            &mut market,
            &mut admin_source,
            &mut frozen_vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(frozen_topup_vault, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);
}

#[test]
fn v13_wrapper_spl_u64_amount_limit_rejects_before_mutation() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    let mint = init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 1_000);

    let too_large = (u64::MAX as u128) + 1;
    let mut source = user_token_account(owner.key, mint, u64::MAX);
    let mut dest = user_token_account(owner.key, mint, 0);
    let mut vault = vault_token_account(&market, mint, u64::MAX);
    let mut vault_auth = vault_authority_account(&market);
    let mut token_program = token_program_account();
    let before_market = market.data.clone();
    let before_portfolio = portfolio.data.clone();

    let deposit_too_large = run_ix(
        Instruction::Deposit { amount: too_large },
        &mut [
            &mut owner,
            &mut market,
            &mut portfolio,
            &mut source,
            &mut vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(deposit_too_large, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);

    let withdraw_too_large = run_ix(
        Instruction::Withdraw { amount: too_large },
        &mut [
            &mut owner,
            &mut market,
            &mut portfolio,
            &mut dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(withdraw_too_large, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);

    let mut admin_source = user_token_account(admin.key, mint, u64::MAX);
    let topup_too_large = run_ix(
        Instruction::TopUpInsurance { amount: too_large },
        &mut [
            &mut admin,
            &mut market,
            &mut admin_source,
            &mut vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(topup_too_large, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);
}

#[test]
fn v13_wrapper_zero_amount_custody_paths_are_noop_without_state_drift() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    let mint = init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 1_000);

    let mut source = user_token_account(owner.key, mint, 0);
    let mut dest = user_token_account(owner.key, mint, 0);
    let mut vault = vault_token_account(&market, mint, 1_000);
    let mut vault_auth = vault_authority_account(&market);
    let mut token_program = token_program_account();
    let before_market = market.data.clone();
    let before_portfolio = portfolio.data.clone();

    run_ix(
        Instruction::Deposit { amount: 0 },
        &mut [
            &mut owner,
            &mut market,
            &mut portfolio,
            &mut source,
            &mut vault,
            &mut token_program,
        ],
    )
    .unwrap();
    assert_eq!(market.data, before_market);
    assert_eq!(portfolio.data, before_portfolio);

    run_ix(
        Instruction::Withdraw { amount: 0 },
        &mut [
            &mut owner,
            &mut market,
            &mut portfolio,
            &mut dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    )
    .unwrap();
    assert_eq!(market.data, before_market);
    assert_eq!(portfolio.data, before_portfolio);

    let mut admin_source = user_token_account(admin.key, mint, 0);
    run_ix(
        Instruction::TopUpInsurance { amount: 0 },
        &mut [
            &mut admin,
            &mut market,
            &mut admin_source,
            &mut vault,
            &mut token_program,
        ],
    )
    .unwrap();
    assert_eq!(market.data, before_market);
    assert_eq!(portfolio.data, before_portfolio);
}

#[test]
fn v13_wrapper_withdraw_rejects_wrong_vault_authority_and_wrong_destination_mint() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    let mint = init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 1_000);

    let mut wrong_dest = user_token_account(owner.key, Pubkey::new_unique(), 0);
    let mut vault = vault_token_account(&market, mint, 1_000);
    let mut vault_auth = vault_authority_account(&market);
    let mut token_program = token_program_account();
    let before_market = market.data.clone();
    let before_portfolio = portfolio.data.clone();
    let wrong_mint = run_ix(
        Instruction::Withdraw { amount: 1 },
        &mut [
            &mut owner,
            &mut market,
            &mut portfolio,
            &mut wrong_dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(wrong_mint, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);

    let mut dest = user_token_account(owner.key, mint, 0);
    let mut wrong_vault_auth = TestAccount::new(Pubkey::new_unique(), Pubkey::new_unique(), 0);
    let wrong_authority = run_ix(
        Instruction::Withdraw { amount: 1 },
        &mut [
            &mut owner,
            &mut market,
            &mut portfolio,
            &mut dest,
            &mut vault,
            &mut wrong_vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(wrong_authority, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);
}

#[test]
fn v13_wrapper_withdraw_rejects_wrong_owner_without_mutation() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut attacker = signer();
    let mut portfolio = portfolio_account();

    let mint = init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 1_000);

    let mut dest = user_token_account(attacker.key, mint, 0);
    let mut vault = vault_token_account(&market, mint, 1_000);
    let mut vault_auth = vault_authority_account(&market);
    let mut token_program = token_program_account();
    let before_market = market.data.clone();
    let before_portfolio = portfolio.data.clone();
    let wrong_owner = run_ix(
        Instruction::Withdraw { amount: 1 },
        &mut [
            &mut attacker,
            &mut market,
            &mut portfolio,
            &mut dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(wrong_owner, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);
}

#[test]
fn v13_wrapper_withdraw_rejects_over_capital_and_insufficient_vault_without_mutation() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    let mint = init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 500);

    let mut dest = user_token_account(owner.key, mint, 0);
    let mut vault = vault_token_account(&market, mint, 501);
    let mut vault_auth = vault_authority_account(&market);
    let mut token_program = token_program_account();
    let before_market = market.data.clone();
    let before_portfolio = portfolio.data.clone();
    let over_capital = run_ix(
        Instruction::Withdraw { amount: 501 },
        &mut [
            &mut owner,
            &mut market,
            &mut portfolio,
            &mut dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(over_capital, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);

    let mut underfunded_vault = vault_token_account(&market, mint, 399);
    let insufficient_vault = run_ix(
        Instruction::Withdraw { amount: 400 },
        &mut [
            &mut owner,
            &mut market,
            &mut portfolio,
            &mut dest,
            &mut underfunded_vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(insufficient_vault, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);
}

#[test]
fn v13_wrapper_close_portfolio_rejects_wrong_owner_without_mutation() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut attacker = signer();
    let mut portfolio = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);

    let before_market = market.data.clone();
    let before_portfolio = portfolio.data.clone();
    let rejected = run_ix(
        Instruction::ClosePortfolio,
        &mut [&mut attacker, &mut market, &mut portfolio],
    );
    assert_err_and_market_unchanged(rejected, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);
}

#[test]
fn v13_wrapper_cross_market_portfolio_provenance_is_fail_closed() {
    let mut admin_a = signer();
    let mut admin_b = signer();
    let mut market_a = market_account();
    let mut market_b = market_account();
    let mut owner_a = signer();
    let mut owner_b = signer();
    let mut account_a = portfolio_account();
    let mut account_b = portfolio_account();

    let _mint_a = init_market(&mut admin_a, &mut market_a);
    let mint_b = init_market(&mut admin_b, &mut market_b);
    init_portfolio(&mut owner_a, &mut market_a, &mut account_a);
    init_portfolio(&mut owner_b, &mut market_b, &mut account_b);
    deposit(&mut owner_a, &mut market_a, &mut account_a, 1_000);
    deposit(&mut owner_b, &mut market_b, &mut account_b, 1_000);

    let before_market_a = market_a.data.clone();
    let before_market_b = market_b.data.clone();
    let before_a = account_a.data.clone();
    let before_b = account_b.data.clone();

    let mut source_b = user_token_account(owner_a.key, mint_b, 1_000);
    let mut vault_b = vault_token_account(&market_b, mint_b, 1_000);
    let mut token_program = token_program_account();
    let cross_deposit = run_ix(
        Instruction::Deposit { amount: 1 },
        &mut [
            &mut owner_a,
            &mut market_b,
            &mut account_a,
            &mut source_b,
            &mut vault_b,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(cross_deposit, &market_b, &before_market_b);
    assert_eq!(account_a.data, before_a);

    let cross_crank = run_ix(
        Instruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 1,
            effective_price: 101,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut owner_a, &mut market_b, &mut account_a],
    );
    assert_err_and_market_unchanged(cross_crank, &market_b, &before_market_b);
    assert_eq!(account_a.data, before_a);

    let cross_close = run_ix(
        Instruction::ClosePortfolio,
        &mut [&mut owner_a, &mut market_b, &mut account_a],
    );
    assert_err_and_market_unchanged(cross_close, &market_b, &before_market_b);
    assert_eq!(account_a.data, before_a);

    let cross_trade = run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: POS_SCALE as i128,
            exec_price: 100,
            fee_bps: 0,
        },
        &mut [
            &mut owner_a,
            &mut owner_b,
            &mut market_a,
            &mut account_a,
            &mut account_b,
        ],
    );
    assert_err_and_market_unchanged(cross_trade, &market_a, &before_market_a);
    assert_eq!(account_a.data, before_a);
    assert_eq!(account_b.data, before_b);
}

#[test]
fn v13_wrapper_account_kind_confusion_is_rejected_before_mutation() {
    let mut admin = signer();
    let mut admin_b = signer();
    let mut market = market_account();
    let mut second_market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    let mint = init_market(&mut admin, &mut market);
    init_market(&mut admin_b, &mut second_market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 1_000);

    let before_market = market.data.clone();
    let before_second_market = second_market.data.clone();
    let before_portfolio = portfolio.data.clone();
    let mut source = user_token_account(owner.key, mint, 1_000);
    let mut vault = vault_token_account(&market, mint, 1_000);
    let mut token_program = token_program_account();

    let portfolio_as_market = run_ix(
        Instruction::Deposit { amount: 1 },
        &mut [
            &mut owner,
            &mut portfolio,
            &mut market,
            &mut source,
            &mut vault,
            &mut token_program,
        ],
    );
    assert!(
        portfolio_as_market.is_err(),
        "portfolio-as-market must reject"
    );
    assert_eq!(market.data, before_market);
    assert_eq!(portfolio.data, before_portfolio);

    let market_as_portfolio = run_ix(
        Instruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 1,
            effective_price: 100,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut owner, &mut market, &mut second_market],
    );
    assert!(
        market_as_portfolio.is_err(),
        "market-as-portfolio must reject"
    );
    assert_eq!(market.data, before_market);
    assert_eq!(second_market.data, before_second_market);
    assert_eq!(portfolio.data, before_portfolio);
}

#[test]
fn v13_wrapper_portfolio_key_mismatch_and_self_trade_are_rejected() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner_a = signer();
    let mut owner_b = signer();
    let mut account_a = portfolio_account();
    let mut account_b = portfolio_account();

    let mint = init_market(&mut admin, &mut market);
    init_portfolio(&mut owner_a, &mut market, &mut account_a);
    init_portfolio(&mut owner_b, &mut market, &mut account_b);
    deposit(&mut owner_a, &mut market, &mut account_a, 1_000);
    deposit(&mut owner_b, &mut market, &mut account_b, 1_000);

    let before_market = market.data.clone();
    let before_a = account_a.data.clone();
    let before_b = account_b.data.clone();

    account_a.key = Pubkey::new_unique();
    let mut source = user_token_account(owner_a.key, mint, 1_000);
    let mut vault = vault_token_account(&market, mint, 1_000);
    let mut token_program = token_program_account();
    let key_mismatch_deposit = run_ix(
        Instruction::Deposit { amount: 1 },
        &mut [
            &mut owner_a,
            &mut market,
            &mut account_a,
            &mut source,
            &mut vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(key_mismatch_deposit, &market, &before_market);
    assert_eq!(account_a.data, before_a);

    account_a.key = Pubkey::new_from_array(
        state::read_portfolio(&account_a.data)
            .unwrap()
            .provenance_header
            .portfolio_account_id,
    );
    account_b.key = account_a.key;
    let same_key_trade = run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: POS_SCALE as i128,
            exec_price: 100,
            fee_bps: 0,
        },
        &mut [
            &mut owner_a,
            &mut owner_b,
            &mut market,
            &mut account_a,
            &mut account_b,
        ],
    );
    assert_err_and_market_unchanged(same_key_trade, &market, &before_market);
    assert_eq!(account_a.data, before_a);
    assert_eq!(account_b.data, before_b);
}

#[test]
fn v13_wrapper_tradenocpi_negative_size_flips_long_short_roles() {
    let mut admin = signer();
    let mut market = market_account();
    let mut signer_a = signer();
    let mut signer_b = signer();
    let mut account_a = portfolio_account();
    let mut account_b = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut signer_a, &mut market, &mut account_a);
    init_portfolio(&mut signer_b, &mut market, &mut account_b);
    deposit(&mut signer_a, &mut market, &mut account_a, 1_000_000);
    deposit(&mut signer_b, &mut market, &mut account_b, 1_000_000);

    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: -(POS_SCALE as i128),
            exec_price: 100,
            fee_bps: 0,
        },
        &mut [
            &mut signer_a,
            &mut signer_b,
            &mut market,
            &mut account_a,
            &mut account_b,
        ],
    )
    .unwrap();

    let a = state::read_portfolio(&account_a.data).unwrap();
    let b = state::read_portfolio(&account_b.data).unwrap();
    assert_eq!(a.legs[0].basis_pos_q, -(POS_SCALE as i128));
    assert_eq!(b.legs[0].basis_pos_q, POS_SCALE as i128);
}

#[test]
fn v13_wrapper_tradenocpi_accepts_consented_wide_exec_price_without_moving_index() {
    let mut admin = signer();
    let mut market = market_account();
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 1_000_000);
    deposit(&mut short_owner, &mut market, &mut short_account, 1_000_000);

    let (_, before) = state::read_market(&market.data).unwrap();
    assert_eq!(before.assets[0].effective_price, 100);

    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: (10 * POS_SCALE) as i128,
            exec_price: 150,
            fee_bps: 100,
        },
        &mut [
            &mut long_owner,
            &mut short_owner,
            &mut market,
            &mut long_account,
            &mut short_account,
        ],
    )
    .unwrap();

    let (_, group) = state::read_market(&market.data).unwrap();
    let long = state::read_portfolio(&long_account.data).unwrap();
    let short = state::read_portfolio(&short_account.data).unwrap();
    assert_eq!(
        group.assets[0].effective_price, 100,
        "execution price must not move the oracle/index state"
    );
    assert_ne!(long.active_bitmap, 0);
    assert_ne!(short.active_bitmap, 0);
    assert_eq!(long.legs[0].basis_pos_q, (10 * POS_SCALE) as i128);
    assert_eq!(short.legs[0].basis_pos_q, -((10 * POS_SCALE) as i128));
    assert_eq!(
        group.insurance, 30,
        "notional=1500 and 100 bps charges 15 to each side"
    );
}

#[test]
fn v13_wrapper_tradenocpi_rejects_when_consented_price_would_break_margin() {
    let mut admin = signer();
    let mut market = market_account();
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 100);
    deposit(&mut short_owner, &mut market, &mut short_account, 100);

    let result = run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: (2 * POS_SCALE) as i128,
            exec_price: 100,
            fee_bps: 0,
        },
        &mut [
            &mut long_owner,
            &mut short_owner,
            &mut market,
            &mut long_account,
            &mut short_account,
        ],
    );

    assert!(
        result.is_err(),
        "the wrapper may accept any consented price, but the engine must still reject unhealthy accounts"
    );
    let long = state::read_portfolio(&long_account.data).unwrap();
    let short = state::read_portfolio(&short_account.data).unwrap();
    assert_eq!(long.active_bitmap, 0);
    assert_eq!(short.active_bitmap, 0);
}

#[test]
fn v13_wrapper_tradenocpi_rejects_bad_size_and_missing_signer_before_mutation() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner_a = signer();
    let mut owner_b = signer();
    let mut unsigned_b = TestAccount::new(owner_b.key, Pubkey::new_unique(), 0);
    let mut account_a = portfolio_account();
    let mut account_b = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner_a, &mut market, &mut account_a);
    init_portfolio(&mut owner_b, &mut market, &mut account_b);
    deposit(&mut owner_a, &mut market, &mut account_a, 1_000_000);
    deposit(&mut owner_b, &mut market, &mut account_b, 1_000_000);

    let before_market = market.data.clone();
    let before_a = account_a.data.clone();
    let before_b = account_b.data.clone();
    let missing_signature = run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: POS_SCALE as i128,
            exec_price: 100,
            fee_bps: 0,
        },
        &mut [
            &mut owner_a,
            &mut unsigned_b,
            &mut market,
            &mut account_a,
            &mut account_b,
        ],
    );
    assert_err_and_market_unchanged(missing_signature, &market, &before_market);
    assert_eq!(account_a.data, before_a);
    assert_eq!(account_b.data, before_b);

    let zero_size = run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: 0,
            exec_price: 100,
            fee_bps: 0,
        },
        &mut [
            &mut owner_a,
            &mut owner_b,
            &mut market,
            &mut account_a,
            &mut account_b,
        ],
    );
    assert_err_and_market_unchanged(zero_size, &market, &before_market);
    assert_eq!(account_a.data, before_a);
    assert_eq!(account_b.data, before_b);

    let min_size = run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: i128::MIN,
            exec_price: 100,
            fee_bps: 0,
        },
        &mut [
            &mut owner_a,
            &mut owner_b,
            &mut market,
            &mut account_a,
            &mut account_b,
        ],
    );
    assert_err_and_market_unchanged(min_size, &market, &before_market);
    assert_eq!(account_a.data, before_a);
    assert_eq!(account_b.data, before_b);
}

#[test]
fn v13_wrapper_tradenocpi_rejects_wrong_owner_fee_cap_and_invalid_asset() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner_a = signer();
    let mut owner_b = signer();
    let mut attacker = signer();
    let mut account_a = portfolio_account();
    let mut account_b = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner_a, &mut market, &mut account_a);
    init_portfolio(&mut owner_b, &mut market, &mut account_b);
    deposit(&mut owner_a, &mut market, &mut account_a, 1_000_000);
    deposit(&mut owner_b, &mut market, &mut account_b, 1_000_000);

    let before_market = market.data.clone();
    let before_a = account_a.data.clone();
    let before_b = account_b.data.clone();
    let wrong_owner = run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: POS_SCALE as i128,
            exec_price: 100,
            fee_bps: 0,
        },
        &mut [
            &mut attacker,
            &mut owner_b,
            &mut market,
            &mut account_a,
            &mut account_b,
        ],
    );
    assert_err_and_market_unchanged(wrong_owner, &market, &before_market);
    assert_eq!(account_a.data, before_a);
    assert_eq!(account_b.data, before_b);

    let fee_over_cap = run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: POS_SCALE as i128,
            exec_price: 100,
            fee_bps: 10_001,
        },
        &mut [
            &mut owner_a,
            &mut owner_b,
            &mut market,
            &mut account_a,
            &mut account_b,
        ],
    );
    assert_err_and_market_unchanged(fee_over_cap, &market, &before_market);
    assert_eq!(account_a.data, before_a);
    assert_eq!(account_b.data, before_b);

    let invalid_asset = run_ix(
        Instruction::TradeNoCpi {
            asset_index: 1,
            size_q: POS_SCALE as i128,
            exec_price: 100,
            fee_bps: 0,
        },
        &mut [
            &mut owner_a,
            &mut owner_b,
            &mut market,
            &mut account_a,
            &mut account_b,
        ],
    );
    assert_err_and_market_unchanged(invalid_asset, &market, &before_market);
    assert_eq!(account_a.data, before_a);
    assert_eq!(account_b.data, before_b);

    let zero_price = run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: POS_SCALE as i128,
            exec_price: 0,
            fee_bps: 0,
        },
        &mut [
            &mut owner_a,
            &mut owner_b,
            &mut market,
            &mut account_a,
            &mut account_b,
        ],
    );
    assert_err_and_market_unchanged(zero_price, &market, &before_market);
    assert_eq!(account_a.data, before_a);
    assert_eq!(account_b.data, before_b);

    let above_max_price = run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: POS_SCALE as i128,
            exec_price: percolator::MAX_ORACLE_PRICE + 1,
            fee_bps: 0,
        },
        &mut [
            &mut owner_a,
            &mut owner_b,
            &mut market,
            &mut account_a,
            &mut account_b,
        ],
    );
    assert_err_and_market_unchanged(above_max_price, &market, &before_market);
    assert_eq!(account_a.data, before_a);
    assert_eq!(account_b.data, before_b);
}

#[test]
fn v13_wrapper_permissionless_crank_advances_account_local_market_progress() {
    let mut admin = signer();
    let mut market = market_account();
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 1_000_000);
    deposit(&mut short_owner, &mut market, &mut short_account, 1_000_000);
    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: POS_SCALE as i128,
            exec_price: 100,
            fee_bps: 0,
        },
        &mut [
            &mut long_owner,
            &mut short_owner,
            &mut market,
            &mut long_account,
            &mut short_account,
        ],
    )
    .unwrap();

    run_ix(
        Instruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 1,
            effective_price: 101,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut admin, &mut market, &mut long_account],
    )
    .unwrap();

    let (_, group) = state::read_market(&market.data).unwrap();
    let long = state::read_portfolio(&long_account.data).unwrap();
    assert_eq!(group.slot_last, 1);
    assert_eq!(group.current_slot, 1);
    assert_eq!(group.assets[0].effective_price, 101);
    assert!(long.health_cert.valid);
}

#[test]
fn v13_wrapper_permissionless_crank_does_not_require_owner_signature() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut caller = TestAccount::new(Pubkey::new_unique(), Pubkey::new_unique(), 0);
    let mut portfolio = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 1_000);

    run_ix(
        Instruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 1,
            effective_price: 100,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut caller, &mut market, &mut portfolio],
    )
    .unwrap();

    let (_, group) = state::read_market(&market.data).unwrap();
    let account = state::read_portfolio(&portfolio.data).unwrap();
    assert_eq!(group.current_slot, 1);
    assert!(account.health_cert.valid);
}

#[test]
fn v13_wrapper_permissionless_crank_rejects_stale_now_without_mutation() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);

    run_ix(
        Instruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 1,
            effective_price: 100,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut owner, &mut market, &mut portfolio],
    )
    .unwrap();

    let market_before = market.data.clone();
    let portfolio_before = portfolio.data.clone();
    let rejected = run_ix(
        Instruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 0,
            effective_price: 100,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut owner, &mut market, &mut portfolio],
    );

    assert_err_and_market_unchanged(rejected, &market, &market_before);
    assert_eq!(
        portfolio.data, portfolio_before,
        "failed crank must not persist account-local mutation"
    );
}

#[test]
fn v13_wrapper_permissionless_crank_can_liquidate_unhealthy_candidate() {
    let mut admin = signer();
    let mut market = market_account();
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 1_000_000);
    deposit(&mut short_owner, &mut market, &mut short_account, 100);
    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: POS_SCALE as i128,
            exec_price: 100,
            fee_bps: 0,
        },
        &mut [
            &mut long_owner,
            &mut short_owner,
            &mut market,
            &mut long_account,
            &mut short_account,
        ],
    )
    .unwrap();

    run_ix(
        Instruction::PermissionlessCrank {
            action: 1,
            asset_index: 0,
            now_slot: 1,
            effective_price: 200,
            funding_rate_e9: 0,
            close_q: POS_SCALE,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut admin, &mut market, &mut short_account],
    )
    .unwrap();

    let (_, group) = state::read_market(&market.data).unwrap();
    let short = state::read_portfolio(&short_account.data).unwrap();
    assert_eq!(group.slot_last, 1);
    assert_eq!(group.assets[0].effective_price, 200);
    assert_eq!(
        short.active_bitmap, 0,
        "liquidation should close the unhealthy short through the public crank path"
    );
}

#[test]
fn v13_wrapper_permissionless_settle_b_without_b_state_is_fail_closed() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);

    let market_before = market.data.clone();
    let portfolio_before = portfolio.data.clone();
    let rejected = run_ix(
        Instruction::PermissionlessCrank {
            action: 2,
            asset_index: 0,
            now_slot: 0,
            effective_price: 100,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut owner, &mut market, &mut portfolio],
    );

    assert_err_and_market_unchanged(rejected, &market, &market_before);
    assert_eq!(portfolio.data, portfolio_before);
}

#[test]
fn v13_wrapper_permissionless_crank_rejects_invalid_asset_and_price_without_mutation() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);

    let before_market = market.data.clone();
    let before_portfolio = portfolio.data.clone();
    let invalid_asset = run_ix(
        Instruction::PermissionlessCrank {
            action: 0,
            asset_index: 1,
            now_slot: 0,
            effective_price: 100,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut owner, &mut market, &mut portfolio],
    );
    assert_err_and_market_unchanged(invalid_asset, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);

    let zero_price = run_ix(
        Instruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 0,
            effective_price: 0,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut owner, &mut market, &mut portfolio],
    );
    assert_err_and_market_unchanged(zero_price, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);
}

#[test]
fn v13_wrapper_permissionless_recovery_is_public_progress() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);

    run_ix(
        Instruction::PermissionlessCrank {
            action: 3,
            asset_index: 0,
            now_slot: 0,
            effective_price: 100,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut owner, &mut market, &mut portfolio],
    )
    .unwrap();

    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(
        group.recovery_reason,
        Some(PermissionlessRecoveryReasonV13::BelowProgressFloor)
    );
}

#[test]
fn v13_wrapper_permissionless_recovery_accepts_every_public_reason() {
    let expected = [
        PermissionlessRecoveryReasonV13::BelowProgressFloor,
        PermissionlessRecoveryReasonV13::BlockedSegmentHeadroomOrRepresentability,
        PermissionlessRecoveryReasonV13::AccountBSettlementCannotProgress,
        PermissionlessRecoveryReasonV13::BIndexHeadroomExhausted,
        PermissionlessRecoveryReasonV13::ActiveBankruptCloseCannotProgress,
        PermissionlessRecoveryReasonV13::ExplicitLossOrDustAuditOverflow,
        PermissionlessRecoveryReasonV13::OracleOrTargetUnavailableByAuthenticatedPolicy,
        PermissionlessRecoveryReasonV13::CounterOrEpochOverflowDeclaredRecovery,
    ];

    for (reason, expected_reason) in expected.iter().copied().enumerate() {
        let mut admin = signer();
        let mut market = market_account();
        let mut owner = signer();
        let mut portfolio = portfolio_account();
        init_market(&mut admin, &mut market);
        init_portfolio(&mut owner, &mut market, &mut portfolio);

        run_ix(
            Instruction::PermissionlessCrank {
                action: 3,
                asset_index: 0,
                now_slot: 0,
                effective_price: 100,
                funding_rate_e9: 0,
                close_q: 0,
                fee_bps: 0,
                recovery_reason: reason as u8,
            },
            &mut [&mut owner, &mut market, &mut portfolio],
        )
        .unwrap();
        let (_, group) = state::read_market(&market.data).unwrap();
        assert_eq!(group.recovery_reason, Some(expected_reason));
    }
}

#[test]
fn v13_wrapper_permissionless_crank_rejects_invalid_action_and_recovery_reason() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);

    let before_market = market.data.clone();
    let before_portfolio = portfolio.data.clone();
    let bad_action = run_ix(
        Instruction::PermissionlessCrank {
            action: 9,
            asset_index: 0,
            now_slot: 0,
            effective_price: 100,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut owner, &mut market, &mut portfolio],
    );
    assert_err_and_market_unchanged(bad_action, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);

    let bad_recovery_reason = run_ix(
        Instruction::PermissionlessCrank {
            action: 3,
            asset_index: 0,
            now_slot: 0,
            effective_price: 100,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 99,
        },
        &mut [&mut owner, &mut market, &mut portfolio],
    );
    assert_err_and_market_unchanged(bad_recovery_reason, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);
}

#[test]
fn v13_wrapper_resolve_market_is_admin_only_and_blocks_live_trade() {
    let mut admin = signer();
    let mut attacker = signer();
    let mut market = market_account();
    let mut owner_a = signer();
    let mut owner_b = signer();
    let mut portfolio_a = portfolio_account();
    let mut portfolio_b = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner_a, &mut market, &mut portfolio_a);
    init_portfolio(&mut owner_b, &mut market, &mut portfolio_b);
    deposit(&mut owner_a, &mut market, &mut portfolio_a, 1_000_000);
    deposit(&mut owner_b, &mut market, &mut portfolio_b, 1_000_000);

    let before = market.data.clone();
    let non_admin = run_ix(
        Instruction::ResolveMarket,
        &mut [&mut attacker, &mut market],
    );
    assert_err_and_market_unchanged(non_admin, &market, &before);

    run_ix(Instruction::ResolveMarket, &mut [&mut admin, &mut market]).unwrap();
    let resolved_market = market.data.clone();
    let before_a = portfolio_a.data.clone();
    let before_b = portfolio_b.data.clone();
    let trade_after_resolve = run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: POS_SCALE as i128,
            exec_price: 100,
            fee_bps: 0,
        },
        &mut [
            &mut owner_a,
            &mut owner_b,
            &mut market,
            &mut portfolio_a,
            &mut portfolio_b,
        ],
    );
    assert_err_and_market_unchanged(trade_after_resolve, &market, &resolved_market);
    assert_eq!(portfolio_a.data, before_a);
    assert_eq!(portfolio_b.data, before_b);
}

#[test]
fn v13_wrapper_resolved_market_blocks_new_activity_and_double_resolution() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    let mint = init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 1_000);

    run_ix(Instruction::ResolveMarket, &mut [&mut admin, &mut market]).unwrap();
    let resolved_market = market.data.clone();
    let resolved_portfolio = portfolio.data.clone();

    let double_resolve = run_ix(Instruction::ResolveMarket, &mut [&mut admin, &mut market]);
    assert_err_and_market_unchanged(double_resolve, &market, &resolved_market);
    assert_eq!(portfolio.data, resolved_portfolio);

    let mut new_owner = signer();
    let mut new_portfolio = portfolio_account();
    let new_portfolio_before = new_portfolio.data.clone();
    let init_after_resolve = run_ix(
        Instruction::InitPortfolio,
        &mut [&mut new_owner, &mut market, &mut new_portfolio],
    );
    assert_err_and_market_unchanged(init_after_resolve, &market, &resolved_market);
    assert_eq!(new_portfolio.data, new_portfolio_before);

    let mut source = user_token_account(owner.key, mint, 1_000);
    let mut vault = vault_token_account(&market, mint, 1_000);
    let mut token_program = token_program_account();
    let deposit_after_resolve = run_ix(
        Instruction::Deposit { amount: 1 },
        &mut [
            &mut owner,
            &mut market,
            &mut portfolio,
            &mut source,
            &mut vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(deposit_after_resolve, &market, &resolved_market);
    assert_eq!(portfolio.data, resolved_portfolio);

    let mut dest = user_token_account(owner.key, mint, 0);
    let mut vault_auth = vault_authority_account(&market);
    let withdraw_after_resolve = run_ix(
        Instruction::Withdraw { amount: 1 },
        &mut [
            &mut owner,
            &mut market,
            &mut portfolio,
            &mut dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(withdraw_after_resolve, &market, &resolved_market);
    assert_eq!(portfolio.data, resolved_portfolio);

    let mut admin_source = user_token_account(admin.key, mint, 1_000);
    let topup_after_resolve = run_ix(
        Instruction::TopUpInsurance { amount: 1 },
        &mut [
            &mut admin,
            &mut market,
            &mut admin_source,
            &mut vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(topup_after_resolve, &market, &resolved_market);
    assert_eq!(portfolio.data, resolved_portfolio);
}

#[test]
fn v13_wrapper_resolved_close_uses_engine_loss_and_fee_ordering_path() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 1_000);
    run_ix(Instruction::ResolveMarket, &mut [&mut admin, &mut market]).unwrap();
    close_resolved(&mut owner, &mut market, &mut portfolio, 0);

    let (_, group) = state::read_market(&market.data).unwrap();
    let acct = state::read_portfolio(&portfolio.data).unwrap();
    assert_eq!(group.mode, MarketModeV13::Resolved);
    assert_eq!(acct.capital, 0);
    assert_eq!(group.vault, 0);
}

#[test]
fn v13_wrapper_close_resolved_does_not_double_pay_after_closed_payout() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 1_000);
    run_ix(Instruction::ResolveMarket, &mut [&mut admin, &mut market]).unwrap();
    close_resolved(&mut owner, &mut market, &mut portfolio, 0);

    let after_first_market = market.data.clone();
    let after_first_portfolio = portfolio.data.clone();
    close_resolved(&mut owner, &mut market, &mut portfolio, 0);
    assert_eq!(
        market.data, after_first_market,
        "a second resolved close must not move market accounting"
    );
    assert_eq!(
        portfolio.data, after_first_portfolio,
        "a second resolved close must not recreate payout state"
    );
}

#[test]
fn v13_wrapper_close_resolved_is_permissionless_but_pays_only_owner_token_account() {
    let mut admin = signer();
    let mut market = market_account();
    let owner = signer();
    let mut owner_meta = TestAccount::new(owner.key, Pubkey::new_unique(), 0);
    let mut portfolio = portfolio_account();

    init_market(&mut admin, &mut market);
    let mut owner_for_init = TestAccount::new(owner.key, Pubkey::new_unique(), 0).signer();
    init_portfolio(&mut owner_for_init, &mut market, &mut portfolio);
    deposit(&mut owner_for_init, &mut market, &mut portfolio, 1_000);
    run_ix(Instruction::ResolveMarket, &mut [&mut admin, &mut market]).unwrap();

    let mint = Pubkey::new_from_array(state::read_market(&market.data).unwrap().0.collateral_mint);
    let mut attacker_dest = user_token_account(Pubkey::new_unique(), mint, 0);
    let mut vault = vault_token_account(&market, mint, 1_000);
    let mut vault_auth = vault_authority_account(&market);
    let mut token_program = token_program_account();
    let before_market = market.data.clone();
    let before_portfolio = portfolio.data.clone();
    let wrong_destination = run_ix(
        Instruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        &mut [
            &mut owner_meta,
            &mut market,
            &mut portfolio,
            &mut attacker_dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(wrong_destination, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);

    let mut owner_dest = user_token_account(owner.key, mint, 0);
    run_ix(
        Instruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        &mut [
            &mut owner_meta,
            &mut market,
            &mut portfolio,
            &mut owner_dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    )
    .unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    let account = state::read_portfolio(&portfolio.data).unwrap();
    assert_eq!(group.vault, 0);
    assert_eq!(account.capital, 0);
}

#[test]
fn v13_wrapper_close_resolved_rejects_before_resolution_without_mutation() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    let mint = init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 1_000);
    let mut dest = user_token_account(owner.key, mint, 0);
    let mut vault = vault_token_account(&market, mint, 1_000);
    let mut vault_auth = vault_authority_account(&market);
    let mut token_program = token_program_account();

    let before_market = market.data.clone();
    let before_portfolio = portfolio.data.clone();
    let rejected = run_ix(
        Instruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        &mut [
            &mut owner,
            &mut market,
            &mut portfolio,
            &mut dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(rejected, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);
}

#[test]
fn v13_wrapper_close_resolved_progress_only_does_not_pay_active_position() {
    let mut admin = signer();
    let mut market = market_account();
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();

    let mint = init_market(&mut admin, &mut market);
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 1_000_000);
    deposit(&mut short_owner, &mut market, &mut short_account, 1_000_000);
    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: POS_SCALE as i128,
            exec_price: 100,
            fee_bps: 0,
        },
        &mut [
            &mut long_owner,
            &mut short_owner,
            &mut market,
            &mut long_account,
            &mut short_account,
        ],
    )
    .unwrap();
    run_ix(Instruction::ResolveMarket, &mut [&mut admin, &mut market]).unwrap();

    let mut dest = user_token_account(long_owner.key, mint, 0);
    let mut vault = vault_token_account(&market, mint, 2_000_000);
    let mut vault_auth = vault_authority_account(&market);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        &mut [
            &mut long_owner,
            &mut market,
            &mut long_account,
            &mut dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    )
    .unwrap();

    let (_, group) = state::read_market(&market.data).unwrap();
    let long = state::read_portfolio(&long_account.data).unwrap();
    assert_eq!(
        group.vault, 2_000_000,
        "progress-only resolved close must not pay before account exposure is cleared"
    );
    assert_ne!(long.active_bitmap, 0);
    assert_eq!(long.capital, 1_000_000);
}

#[test]
fn v13_wrapper_close_resolved_requires_recipient_and_vault_accounts_for_payout() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 1_000);
    run_ix(Instruction::ResolveMarket, &mut [&mut admin, &mut market]).unwrap();

    let before_market = market.data.clone();
    let before_portfolio = portfolio.data.clone();
    let missing_token_accounts = run_ix(
        Instruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        &mut [&mut owner, &mut market, &mut portfolio],
    );
    assert_err_and_market_unchanged(missing_token_accounts, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);
}
