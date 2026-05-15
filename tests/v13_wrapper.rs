use percolator::{
    MarketGroupV13, MarketModeV13, PermissionlessRecoveryReasonV13, V13Config, POS_SCALE,
};
use percolator_prog::{
    constants::{MARKET_ACCOUNT_LEN, PORTFOLIO_ACCOUNT_LEN},
    ix::Instruction,
    processor, state,
};
use solana_program::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey};

struct TestAccount {
    key: Pubkey,
    owner: Pubkey,
    lamports: u64,
    data: Vec<u8>,
    is_signer: bool,
    is_writable: bool,
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

    fn to_info<'a>(&'a mut self) -> AccountInfo<'a> {
        AccountInfo::new(
            &self.key,
            self.is_signer,
            self.is_writable,
            &mut self.lamports,
            &mut self.data,
            &self.owner,
            false,
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

fn run_ix(ix: Instruction, accounts: &mut [&mut TestAccount]) -> Result<(), ProgramError> {
    let infos: Vec<AccountInfo> = accounts.iter_mut().map(|a| a.to_info()).collect();
    processor::process_instruction(&program_id(), &infos, &ix.encode())
}

fn init_market(admin: &mut TestAccount, market: &mut TestAccount) {
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
        &mut [admin, market],
    )
    .unwrap();
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
    run_ix(
        Instruction::Deposit { amount },
        &mut [owner, market, portfolio],
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
fn v13_wrapper_top_up_insurance_requires_authority_and_updates_vault() {
    let mut admin = signer();
    let mut market = market_account();
    let mut attacker = signer();

    init_market(&mut admin, &mut market);

    let before = market.data.clone();
    let unauthorized = run_ix(
        Instruction::TopUpInsurance { amount: 777 },
        &mut [&mut attacker, &mut market],
    );
    assert_err_and_market_unchanged(unauthorized, &market, &before);

    run_ix(
        Instruction::TopUpInsurance { amount: 777 },
        &mut [&mut admin, &mut market],
    )
    .unwrap();

    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(group.insurance, 777);
    assert_eq!(group.vault, 777);
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

    run_ix(
        Instruction::Withdraw { amount: 1_000 },
        &mut [&mut owner, &mut market, &mut portfolio],
    )
    .unwrap();
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

    run_ix(
        Instruction::Withdraw { amount: 400 },
        &mut [&mut owner, &mut market, &mut portfolio],
    )
    .unwrap();

    let (_, group) = state::read_market(&market.data).unwrap();
    let acct = state::read_portfolio(&portfolio.data).unwrap();
    assert_eq!(acct.capital, 600);
    assert_eq!(group.c_tot, 600);
    assert_eq!(group.vault, 600);
    assert_eq!(group.insurance, 0);
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
fn v13_wrapper_resolved_close_uses_engine_loss_and_fee_ordering_path() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 1_000);
    run_ix(Instruction::ResolveMarket, &mut [&mut admin, &mut market]).unwrap();
    run_ix(
        Instruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        &mut [&mut owner, &mut market, &mut portfolio],
    )
    .unwrap();

    let (_, group) = state::read_market(&market.data).unwrap();
    let acct = state::read_portfolio(&portfolio.data).unwrap();
    assert_eq!(group.mode, MarketModeV13::Resolved);
    assert_eq!(acct.capital, 0);
    assert_eq!(group.vault, 0);
}
