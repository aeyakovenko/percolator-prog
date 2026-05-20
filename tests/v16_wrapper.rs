use percolator::{
    AssetLifecycleV16, BackingBucketStatusV16, CloseProgressLedgerV16, MarketGroupV16,
    MarketGroupV16Account, MarketModeV16, PermissionlessRecoveryReasonV16,
    PortfolioAccountV16Account, PortfolioLegV16, ResolvedPayoutLedgerV16, ResolvedPayoutReceiptV16,
    SideModeV16, SideV16, V16Config, BOUND_SCALE, POS_SCALE,
};
use percolator_prog::{
    constants::{
        ASSET_ORACLE_PROFILES_LEN, HEADER_LEN, MARKET_ACCOUNT_LEN, MARKET_GROUP_LEN,
        ORACLE_LEG_CAP, ORACLE_LEG_FLAG_DIVIDE_LEG2, ORACLE_LEG_FLAG_DIVIDE_LEG3,
        ORACLE_MODE_HYBRID_AFTER_HOURS, ORACLE_MODE_HYPERP, ORACLE_MODE_MANUAL,
        PORTFOLIO_ACCOUNT_LEN, PORTFOLIO_STATE_LEN, WRAPPER_CONFIG_LEN,
    },
    ix::Instruction,
    oracle_v16, policy_v16, processor, state,
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

fn active_bitmap_with(indices: &[usize]) -> percolator::V16ActiveBitmap {
    let mut bitmap = percolator::active_bitmap_empty();
    for &idx in indices {
        percolator::active_bitmap_set(&mut bitmap, idx).unwrap();
    }
    bitmap
}

fn active_leg_for_asset(
    account: &percolator::PortfolioAccountV16,
    asset_index: usize,
) -> percolator::PortfolioLegV16 {
    account
        .legs
        .iter()
        .copied()
        .find(|leg| leg.active && leg.asset_index as usize == asset_index)
        .unwrap()
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

fn matcher_delegate(
    market: &TestAccount,
    maker: &TestAccount,
    matcher_program: &TestAccount,
    matcher_context: &TestAccount,
) -> Pubkey {
    Pubkey::find_program_address(
        &[
            b"matcher",
            market.key.as_ref(),
            maker.key.as_ref(),
            matcher_program.key.as_ref(),
            matcher_context.key.as_ref(),
        ],
        &program_id(),
    )
    .0
}

fn matcher_program_account() -> TestAccount {
    TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0).executable()
}

fn matcher_context_account(matcher_program: &TestAccount) -> TestAccount {
    TestAccount::new(Pubkey::new_unique(), matcher_program.key, 320).writable()
}

fn matcher_delegate_account(
    market: &TestAccount,
    maker: &TestAccount,
    matcher_program: &TestAccount,
    matcher_context: &TestAccount,
) -> TestAccount {
    TestAccount::new(
        matcher_delegate(market, maker, matcher_program, matcher_context),
        Pubkey::default(),
        0,
    )
}

fn write_matcher_return(
    matcher_context: &mut TestAccount,
    exec_price_e6: u64,
    exec_size: i128,
    req_id: u64,
    lp_account_id: u64,
    asset_index: u16,
    oracle_price_e6: u64,
) {
    matcher_context.data[0..4].copy_from_slice(&3u32.to_le_bytes());
    matcher_context.data[4..8].copy_from_slice(&3u32.to_le_bytes());
    matcher_context.data[8..16].copy_from_slice(&exec_price_e6.to_le_bytes());
    matcher_context.data[16..32].copy_from_slice(&exec_size.to_le_bytes());
    matcher_context.data[32..40].copy_from_slice(&req_id.to_le_bytes());
    matcher_context.data[40..48].copy_from_slice(&lp_account_id.to_le_bytes());
    matcher_context.data[48..56].copy_from_slice(&oracle_price_e6.to_le_bytes());
    matcher_context.data[56..64].copy_from_slice(&(asset_index as u64).to_le_bytes());
}

fn run_trade_cpi_with_matcher(
    owner_a: &mut TestAccount,
    owner_b: &mut TestAccount,
    market: &mut TestAccount,
    account_a: &mut TestAccount,
    account_b: &mut TestAccount,
    asset_index: u16,
    req_size: i128,
    exec_size: i128,
    exec_price: u64,
    fee_bps: u64,
    limit_price: u64,
) -> Result<(), ProgramError> {
    let mut matcher_program = matcher_program_account();
    let mut matcher_context = matcher_context_account(&matcher_program);
    let mut delegate =
        matcher_delegate_account(market, account_b, &matcher_program, &matcher_context);
    let (_, group) = state::read_market(&market.data).unwrap();
    let req_id = group.current_slot.wrapping_add(1);
    let lp_account_id = {
        let bytes = delegate.key.to_bytes();
        u64::from_le_bytes(bytes[0..8].try_into().unwrap())
    };
    write_matcher_return(
        &mut matcher_context,
        exec_price,
        exec_size,
        req_id,
        lp_account_id,
        asset_index,
        group.assets[asset_index as usize].effective_price,
    );
    run_ix(
        Instruction::TradeCpi {
            asset_index,
            size_q: req_size,
            fee_bps,
            limit_price,
        },
        &mut [
            owner_a,
            owner_b,
            market,
            account_a,
            account_b,
            &mut matcher_program,
            &mut matcher_context,
            &mut delegate,
        ],
    )
}

fn token_program_account() -> TestAccount {
    TestAccount::new(spl_token::ID, Pubkey::default(), 0).executable()
}

fn non_executable_token_program_account() -> TestAccount {
    TestAccount::new(spl_token::ID, Pubkey::default(), 0)
}

fn make_pyth(feed_id: &[u8; 32], price: i64, expo: i32, conf: u64, publish_time: i64) -> Vec<u8> {
    let mut data = vec![0u8; 134];
    data[0..8].copy_from_slice(&[0x22, 0xf1, 0x23, 0x63, 0x9d, 0x7e, 0xf4, 0xcd]);
    data[40] = 1;
    data[41..73].copy_from_slice(feed_id);
    data[73..81].copy_from_slice(&price.to_le_bytes());
    data[81..89].copy_from_slice(&conf.to_le_bytes());
    data[89..93].copy_from_slice(&expo.to_le_bytes());
    data[93..101].copy_from_slice(&publish_time.to_le_bytes());
    data
}

fn pyth_account(
    feed_id: &[u8; 32],
    price: i64,
    expo: i32,
    conf: u64,
    publish_time: i64,
) -> TestAccount {
    TestAccount::new_with_data(
        Pubkey::new_unique(),
        oracle_v16::PYTH_RECEIVER_PROGRAM_ID,
        make_pyth(feed_id, price, expo, conf, publish_time),
    )
}

fn switchboard_account(
    key: Pubkey,
    price_e6: u64,
    std_dev_e6: u64,
    publish_time: i64,
) -> TestAccount {
    let mut data = vec![0u8; 3_208];
    data[0..8].copy_from_slice(&[196, 27, 108, 196, 10, 215, 219, 40]);
    data[2_120..2_152].copy_from_slice(&[0x5au8; 32]);
    data[2_215] = 1;
    data[2_216..2_224].copy_from_slice(&publish_time.to_le_bytes());
    let value = (price_e6 as i128) * 1_000_000_000_000i128;
    let std_dev = (std_dev_e6 as i128) * 1_000_000_000_000i128;
    data[2_264..2_280].copy_from_slice(&value.to_le_bytes());
    data[2_280..2_296].copy_from_slice(&std_dev.to_le_bytes());
    data[2_360] = 1;
    data[2_368..2_376].copy_from_slice(&1u64.to_le_bytes());
    TestAccount::new_with_data(
        key,
        oracle_v16::SWITCHBOARD_ON_DEMAND_MAINNET_PROGRAM_ID,
        data,
    )
}

fn chainlink_account(key: Pubkey, answer: i128, decimals: u8, publish_time: u32) -> TestAccount {
    let mut data = vec![0u8; 248];
    data[0..8].copy_from_slice(&[96, 179, 69, 66, 128, 129, 73, 117]);
    data[8] = 1;
    data[138] = decimals;
    data[143..147].copy_from_slice(&1u32.to_le_bytes());
    data[148..152].copy_from_slice(&1u32.to_le_bytes());
    data[200..208].copy_from_slice(&1u64.to_le_bytes());
    data[208..212].copy_from_slice(&publish_time.to_le_bytes());
    data[216..232].copy_from_slice(&answer.to_le_bytes());
    TestAccount::new_with_data(key, oracle_v16::CHAINLINK_STORE_PROGRAM_ID, data)
}

fn two_sided_fee(size_q: u128, price: u64, fee_bps: u64) -> u128 {
    let notional = size_q * price as u128 / POS_SCALE;
    let one_side = (notional * fee_bps as u128 + 9_999) / 10_000;
    one_side * 2
}

fn run_ix(ix: Instruction, accounts: &mut [&mut TestAccount]) -> Result<(), ProgramError> {
    let infos: Vec<AccountInfo> = accounts.iter_mut().map(|a| a.to_info()).collect();
    processor::process_instruction(&program_id(), &infos, &ix.encode())
}

fn default_init_market_ix() -> Instruction {
    Instruction::InitMarket {
        max_portfolio_assets: 1,
        h_min: 0,
        h_max: 10,
        initial_price: 100,
        min_nonzero_mm_req: 1,
        min_nonzero_im_req: 2,
        maintenance_margin_bps: 10_000,
        initial_margin_bps: 10_000,
        max_trading_fee_bps: 10_000,
        trade_fee_base_bps: 0,
        liquidation_fee_bps: 0,
        liquidation_fee_cap: 0,
        min_liquidation_abs: 0,
        max_price_move_bps_per_slot: 10_000,
        max_accrual_dt_slots: 1,
        max_abs_funding_e9_per_slot: 0,
        min_funding_lifetime_slots: 1,
        max_account_b_settlement_chunks: 1,
        max_bankrupt_close_chunks: 1,
        public_b_chunk_atoms: percolator::MAX_VAULT_TVL,
        maintenance_fee_per_slot: 0,
    }
}

fn init_market_ix_with(f: impl FnOnce(&mut Instruction)) -> Instruction {
    let mut ix = default_init_market_ix();
    f(&mut ix);
    ix
}

fn init_market(admin: &mut TestAccount, market: &mut TestAccount) -> Pubkey {
    let mut mint = mint_account();
    let mint_key = mint.key;
    run_ix(default_init_market_ix(), &mut [admin, market, &mut mint]).unwrap();
    mint_key
}

fn init_market_with_ix(
    admin: &mut TestAccount,
    market: &mut TestAccount,
    ix: Instruction,
) -> Pubkey {
    let mut mint = mint_account();
    let mint_key = mint.key;
    run_ix(ix, &mut [admin, market, &mut mint]).unwrap();
    mint_key
}

fn update_asset_lifecycle(
    authority: &mut TestAccount,
    market: &mut TestAccount,
    action: u8,
    asset_index: u16,
    now_slot: u64,
    initial_price: u64,
) -> Result<(), ProgramError> {
    run_ix(
        Instruction::UpdateAssetLifecycle {
            action,
            asset_index,
            now_slot,
            initial_price,
        },
        &mut [authority, market],
    )
}

fn init_portfolio(owner: &mut TestAccount, market: &mut TestAccount, portfolio: &mut TestAccount) {
    run_ix(Instruction::InitPortfolio, &mut [owner, market, portfolio]).unwrap();
}

fn sync_maintenance_fee(
    market: &mut TestAccount,
    portfolio: &mut TestAccount,
    now_slot: u64,
) -> Result<(), ProgramError> {
    run_ix(
        Instruction::SyncMaintenanceFee { now_slot },
        &mut [market, portfolio],
    )
}

fn sync_maintenance_fee_with_cranker(
    market: &mut TestAccount,
    portfolio: &mut TestAccount,
    cranker: &mut TestAccount,
    now_slot: u64,
) -> Result<(), ProgramError> {
    run_ix(
        Instruction::SyncMaintenanceFee { now_slot },
        &mut [market, portfolio, cranker],
    )
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

fn configure_three_leg_hybrid(
    admin: &mut TestAccount,
    market: &mut TestAccount,
    feeds: [[u8; 32]; 3],
    leg0: &mut TestAccount,
    leg1: &mut TestAccount,
    leg2: &mut TestAccount,
    now_slot: u64,
    now_unix_ts: i64,
    soft_stale_slots: u64,
    mark_min_fee: u64,
) {
    run_ix(
        Instruction::ConfigureHybridOracle {
            asset_index: 0,
            now_slot,
            now_unix_ts,
            oracle_leg_count: 3,
            oracle_leg_flags: ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3,
            max_staleness_secs: 60,
            hybrid_soft_stale_slots: soft_stale_slots,
            mark_ewma_halflife_slots: 1,
            mark_min_fee,
            invert: 0,
            unit_scale: 0,
            conf_filter_bps: 500,
            oracle_leg_feeds: feeds,
        },
        &mut [admin, market, leg0, leg1, leg2],
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
fn v16_wrapper_init_binds_market_and_portfolio_provenance() {
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

    let mut cfg = V16Config::public_user_fund(1, 0, 10);
    cfg.maintenance_margin_bps = 10_000;
    cfg.initial_margin_bps = 10_000;
    cfg.max_trading_fee_bps = 10_000;
    cfg.max_price_move_bps_per_slot = 10_000;
    cfg.max_accrual_dt_slots = 1;
    cfg.min_funding_lifetime_slots = 1;
    let mut expected = MarketGroupV16::new(market.key.to_bytes(), cfg).unwrap();
    expected.assets[0].raw_oracle_target_price = 100;
    expected.assets[0].effective_price = 100;
    expected.assets[0].fund_px_last = 100;
    for asset in &mut expected.assets[1..] {
        asset.lifecycle = AssetLifecycleV16::Disabled;
    }
    expected.materialized_portfolio_count = 1;
    assert_eq!(
        group, expected,
        "wrapper heap init must match canonical engine init shape"
    );
}

#[test]
fn v16_wrapper_init_market_ports_full_engine_config_fields() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();

    run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                h_max,
                min_nonzero_mm_req,
                min_nonzero_im_req,
                maintenance_margin_bps,
                initial_margin_bps,
                max_trading_fee_bps,
                trade_fee_base_bps,
                liquidation_fee_bps,
                liquidation_fee_cap,
                min_liquidation_abs,
                max_price_move_bps_per_slot,
                max_accrual_dt_slots,
                max_abs_funding_e9_per_slot,
                min_funding_lifetime_slots,
                max_account_b_settlement_chunks,
                max_bankrupt_close_chunks,
                public_b_chunk_atoms,
                maintenance_fee_per_slot,
                ..
            } = ix
            {
                *h_max = 30;
                *min_nonzero_mm_req = 5;
                *min_nonzero_im_req = 8;
                *maintenance_margin_bps = 10_000;
                *initial_margin_bps = 10_000;
                *max_trading_fee_bps = 1_000;
                *trade_fee_base_bps = 7;
                *liquidation_fee_bps = 1;
                *liquidation_fee_cap = 1;
                *min_liquidation_abs = 0;
                *max_price_move_bps_per_slot = 1;
                *max_accrual_dt_slots = 1;
                *max_abs_funding_e9_per_slot = 0;
                *min_funding_lifetime_slots = 30;
                *max_account_b_settlement_chunks = 3;
                *max_bankrupt_close_chunks = 4;
                *public_b_chunk_atoms = 12_345;
                *maintenance_fee_per_slot = 7;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();

    let (wrapper, group) = state::read_market(&market.data).unwrap();
    assert_eq!(wrapper.maintenance_fee_per_slot, 7);
    assert_eq!(wrapper.trade_fee_base_bps, 7);
    assert_eq!(group.config.h_max, 30);
    assert_eq!(group.config.min_nonzero_mm_req, 5);
    assert_eq!(group.config.min_nonzero_im_req, 8);
    assert_eq!(group.config.maintenance_margin_bps, 10_000);
    assert_eq!(group.config.initial_margin_bps, 10_000);
    assert_eq!(group.config.max_trading_fee_bps, 1_000);
    assert_eq!(group.config.liquidation_fee_bps, 1);
    assert_eq!(group.config.liquidation_fee_cap, 1);
    assert_eq!(group.config.min_liquidation_abs, 0);
    assert_eq!(group.config.max_price_move_bps_per_slot, 1);
    assert_eq!(group.config.max_accrual_dt_slots, 1);
    assert_eq!(group.config.max_abs_funding_e9_per_slot, 0);
    assert_eq!(group.config.min_funding_lifetime_slots, 30);
    assert_eq!(group.config.max_account_b_settlement_chunks, 3);
    assert_eq!(group.config.max_bankrupt_close_chunks, 4);
    assert_eq!(group.config.public_b_chunk_atoms, 12_345);
}

#[test]
fn v16_wrapper_permissionless_maintenance_fee_charges_flat_portfolio_to_insurance() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();
    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                maintenance_fee_per_slot,
                ..
            } = ix
            {
                *maintenance_fee_per_slot = 5;
            }
        }),
    );
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 100);

    sync_maintenance_fee(&mut market, &mut portfolio, 10).unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    let account = state::read_portfolio(&portfolio.data).unwrap();
    assert_eq!(account.capital, 50);
    assert_eq!(account.last_fee_slot, 10);
    assert_eq!(group.insurance, 50);
    assert_eq!(group.c_tot, 50);

    let market_after_first = market.data.clone();
    let portfolio_after_first = portfolio.data.clone();
    sync_maintenance_fee(&mut market, &mut portfolio, 10).unwrap();
    assert_eq!(
        market.data, market_after_first,
        "same-slot fee sync must be idempotent"
    );
    assert_eq!(portfolio.data, portfolio_after_first);
}

#[test]
fn v16_wrapper_init_portfolio_anchors_fee_slot_at_market_current_slot() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();
    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                maintenance_fee_per_slot,
                ..
            } = ix
            {
                *maintenance_fee_per_slot = 5;
            }
        }),
    );
    run_ix(
        Instruction::ConfigureHyperpMark {
            asset_index: 0,
            now_slot: 100,
            initial_mark_e6: 100,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    assert_eq!(
        state::read_market(&market.data).unwrap().1.current_slot,
        100
    );

    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 1_000);

    sync_maintenance_fee(&mut market, &mut portfolio, 110).unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    let account = state::read_portfolio(&portfolio.data).unwrap();
    assert_eq!(
        account.capital, 950,
        "new portfolios owe maintenance from creation time, not from slot zero"
    );
    assert_eq!(account.last_fee_slot, 110);
    assert_eq!(group.insurance, 50);
    assert_eq!(group.c_tot, 950);
}

#[test]
fn v16_wrapper_init_portfolio_fee_anchor_tracks_crank_and_asset_lifecycle_time() {
    let assert_new_portfolio_pays_from_creation_slot =
        |market: &mut TestAccount, owner: &mut TestAccount, portfolio: &mut TestAccount| {
            init_portfolio(owner, market, portfolio);
            deposit(owner, market, portfolio, 1_000);
            sync_maintenance_fee(market, portfolio, 110).unwrap();
            let (_, group) = state::read_market(&market.data).unwrap();
            let account = state::read_portfolio(&portfolio.data).unwrap();
            assert_eq!(group.current_slot, 100);
            assert_eq!(account.last_fee_slot, 110);
            assert_eq!(account.capital, 950);
            assert_eq!(group.insurance, 50);
        };

    let mut admin = signer();
    let mut market = market_account();
    let mut old_owner = signer();
    let mut new_owner = signer();
    let mut old_portfolio = portfolio_account();
    let mut new_portfolio = portfolio_account();
    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                maintenance_fee_per_slot,
                ..
            } = ix
            {
                *maintenance_fee_per_slot = 5;
            }
        }),
    );
    init_portfolio(&mut old_owner, &mut market, &mut old_portfolio);
    run_ix(
        Instruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 100,
            effective_price: 100,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut old_owner, &mut market, &mut old_portfolio],
    )
    .unwrap();
    assert_new_portfolio_pays_from_creation_slot(&mut market, &mut new_owner, &mut new_portfolio);

    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();
    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                maintenance_fee_per_slot,
                ..
            } = ix
            {
                *maintenance_fee_per_slot = 5;
            }
        }),
    );
    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        1,
        100,
        125,
    )
    .unwrap();
    assert_new_portfolio_pays_from_creation_slot(&mut market, &mut owner, &mut portfolio);
}

#[test]
fn v16_wrapper_maintenance_fee_is_permissionless_and_capital_capped() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();
    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                maintenance_fee_per_slot,
                ..
            } = ix
            {
                *maintenance_fee_per_slot = 40;
            }
        }),
    );
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 75);

    owner.is_signer = false;
    sync_maintenance_fee(&mut market, &mut portfolio, 2).unwrap();
    owner.is_signer = true;

    let (_, group) = state::read_market(&market.data).unwrap();
    let account = state::read_portfolio(&portfolio.data).unwrap();
    assert_eq!(account.capital, 0);
    assert_eq!(account.last_fee_slot, 2);
    assert_eq!(group.insurance, 75);
    assert_eq!(group.c_tot, 0);
}

#[test]
fn v16_wrapper_maintenance_fee_policy_splits_optional_cranker_share() {
    let mut admin = signer();
    let mut market = market_account();
    let mut payer_owner = signer();
    let mut cranker_owner = signer();
    let mut payer = portfolio_account();
    let mut cranker = portfolio_account();
    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                maintenance_fee_per_slot,
                ..
            } = ix
            {
                *maintenance_fee_per_slot = 5;
            }
        }),
    );
    init_portfolio(&mut payer_owner, &mut market, &mut payer);
    init_portfolio(&mut cranker_owner, &mut market, &mut cranker);
    deposit(&mut payer_owner, &mut market, &mut payer, 100);
    run_ix(
        Instruction::UpdateMaintenanceFeePolicy {
            cranker_share_bps: 4_000,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();

    sync_maintenance_fee_with_cranker(&mut market, &mut payer, &mut cranker, 10).unwrap();

    let (_, group) = state::read_market(&market.data).unwrap();
    let payer = state::read_portfolio(&payer.data).unwrap();
    let cranker = state::read_portfolio(&cranker.data).unwrap();
    assert_eq!(payer.capital, 50);
    assert_eq!(payer.last_fee_slot, 10);
    assert_eq!(cranker.capital, 20);
    assert_eq!(
        group.insurance, 30,
        "the configured cranker share is split out and the remaining maintenance fee stays in insurance"
    );
    assert_eq!(group.c_tot, 70);
    assert_eq!(group.vault, 100);
}

#[test]
fn v16_wrapper_maintenance_fee_reward_account_absent_keeps_full_fee_in_insurance() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();
    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                maintenance_fee_per_slot,
                ..
            } = ix
            {
                *maintenance_fee_per_slot = 5;
            }
        }),
    );
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 100);
    run_ix(
        Instruction::UpdateMaintenanceFeePolicy {
            cranker_share_bps: 4_000,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();

    sync_maintenance_fee(&mut market, &mut portfolio, 10).unwrap();

    let (_, group) = state::read_market(&market.data).unwrap();
    let account = state::read_portfolio(&portfolio.data).unwrap();
    assert_eq!(account.capital, 50);
    assert_eq!(group.insurance, 50);
    assert_eq!(
        group.c_tot, 50,
        "no optional cranker account means no internal cranker reward is minted"
    );
}

#[test]
fn v16_wrapper_maintenance_fee_same_cranker_key_still_leaves_insurance_share() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();
    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                maintenance_fee_per_slot,
                ..
            } = ix
            {
                *maintenance_fee_per_slot = 5;
            }
        }),
    );
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 100);
    run_ix(
        Instruction::UpdateMaintenanceFeePolicy {
            cranker_share_bps: 4_000,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();

    let mut duplicate_same_key = portfolio_account();
    duplicate_same_key.key = portfolio.key;
    sync_maintenance_fee_with_cranker(&mut market, &mut portfolio, &mut duplicate_same_key, 10)
        .unwrap();

    let (_, group) = state::read_market(&market.data).unwrap();
    let account = state::read_portfolio(&portfolio.data).unwrap();
    assert_eq!(
        account.capital, 70,
        "same-key cranker receives only the configured reward share after paying the full fee"
    );
    assert_eq!(
        group.insurance, 30,
        "same-key reward is not a noop because the unsplit insurance share remains collected"
    );
    assert_eq!(group.c_tot, 70);
    assert_eq!(group.vault, 100);
}

#[test]
fn v16_wrapper_maintenance_fee_policy_is_admin_gated_and_bounds_share() {
    let mut admin = signer();
    let mut attacker = signer();
    let mut market = market_account();
    init_market(&mut admin, &mut market);
    let before = market.data.clone();

    let rejected = run_ix(
        Instruction::UpdateMaintenanceFeePolicy {
            cranker_share_bps: 10_001,
        },
        &mut [&mut admin, &mut market],
    );
    assert_err_and_market_unchanged(rejected, &market, &before);

    let rejected = run_ix(
        Instruction::UpdateMaintenanceFeePolicy {
            cranker_share_bps: 4_000,
        },
        &mut [&mut attacker, &mut market],
    );
    assert_err_and_market_unchanged(rejected, &market, &before);

    run_ix(
        Instruction::UpdateMaintenanceFeePolicy {
            cranker_share_bps: 4_000,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    let (cfg, _) = state::read_market(&market.data).unwrap();
    assert_eq!(cfg.maintenance_cranker_fee_share_bps, 4_000);
}

#[test]
fn v16_wrapper_maintenance_fee_settles_losses_before_charging_nonflat_account() {
    let mut admin = signer();
    let mut market = market_account();
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();
    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                maintenance_fee_per_slot,
                ..
            } = ix
            {
                *maintenance_fee_per_slot = 10;
            }
        }),
    );
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 1_000);
    deposit(&mut short_owner, &mut market, &mut short_account, 1_000);
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

    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        group.accrue_asset_to_not_atomic(0, 1, 50, 0, true).unwrap();
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }

    sync_maintenance_fee(&mut market, &mut long_account, 1).unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    let account = state::read_portfolio(&long_account.data).unwrap();
    assert_eq!(account.pnl, 0, "lazy mark loss must be settled first");
    assert_eq!(
        account.capital, 940,
        "capital should lose 50 to mark loss, then 10 to maintenance"
    );
    assert_eq!(account.last_fee_slot, 1);
    assert_eq!(group.insurance, 10);
}

#[test]
fn v16_wrapper_maintenance_fee_rejects_recovery_mode_without_mutation() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();
    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                maintenance_fee_per_slot,
                ..
            } = ix
            {
                *maintenance_fee_per_slot = 5;
            }
        }),
    );
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 100);
    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        group.mode = MarketModeV16::Recovery;
        group.recovery_reason = Some(PermissionlessRecoveryReasonV16::BelowProgressFloor);
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }

    let before_market = market.data.clone();
    let before_portfolio = portfolio.data.clone();
    let result = sync_maintenance_fee(&mut market, &mut portfolio, 10);
    assert_err_and_market_unchanged(result, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);
}

#[test]
fn v16_wrapper_asset_authority_can_append_activate_and_trade_assets() {
    let mut admin = signer();
    let mut market = market_account();
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();

    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_portfolio_assets,
                ..
            } = ix
            {
                *max_portfolio_assets = 1;
            }
        }),
    );
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 10_000);
    deposit(&mut short_owner, &mut market, &mut short_account, 10_000);

    let before = market.data.clone();
    let rejected_before_add = run_ix(
        Instruction::TradeNoCpi {
            asset_index: 2,
            size_q: POS_SCALE as i128,
            exec_price: 250,
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
    assert_err_and_market_unchanged(rejected_before_add, &market, &before);

    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        1,
        1,
        150,
    )
    .unwrap();
    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        2,
        2,
        250,
    )
    .unwrap();
    let (cfg, group) = state::read_market(&market.data).unwrap();
    assert_eq!(cfg.asset_authority, admin.key.to_bytes());
    assert_eq!(group.config.max_market_slots, 3);
    assert_eq!(group.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(group.assets[2].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(group.assets[2].effective_price, 250);

    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 2,
            size_q: POS_SCALE as i128,
            exec_price: 250,
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
    let long = state::read_portfolio(&long_account.data).unwrap();
    let short = state::read_portfolio(&short_account.data).unwrap();
    assert_eq!(long.active_bitmap, active_bitmap_with(&[0]));
    assert_eq!(short.active_bitmap, active_bitmap_with(&[0]));
    assert_eq!(
        active_leg_for_asset(&long, 2).basis_pos_q,
        POS_SCALE as i128
    );
    assert_eq!(
        active_leg_for_asset(&short, 2).basis_pos_q,
        -(POS_SCALE as i128)
    );
}

#[test]
fn v16_wrapper_one_portfolio_can_hold_multiple_asset_positions_independently() {
    let mut admin = signer();
    let mut market = market_account();
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();

    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_portfolio_assets,
                ..
            } = ix
            {
                *max_portfolio_assets = 3;
            }
        }),
    );
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
        Instruction::TradeNoCpi {
            asset_index: 2,
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
    )
    .unwrap();

    let (_, group) = state::read_market(&market.data).unwrap();
    let long = state::read_portfolio(&long_account.data).unwrap();
    let short = state::read_portfolio(&short_account.data).unwrap();
    assert_eq!(long.active_bitmap, active_bitmap_with(&[0, 1]));
    assert_eq!(short.active_bitmap, active_bitmap_with(&[0, 1]));
    assert_eq!(
        active_leg_for_asset(&long, 0).basis_pos_q,
        POS_SCALE as i128
    );
    assert_eq!(
        active_leg_for_asset(&short, 0).basis_pos_q,
        -(POS_SCALE as i128)
    );
    assert_eq!(
        active_leg_for_asset(&long, 2).basis_pos_q,
        (2 * POS_SCALE) as i128
    );
    assert_eq!(
        active_leg_for_asset(&short, 2).basis_pos_q,
        -((2 * POS_SCALE) as i128)
    );
    assert_eq!(group.assets[0].oi_eff_long_q, POS_SCALE);
    assert_eq!(group.assets[0].oi_eff_short_q, POS_SCALE);
    assert_eq!(group.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(group.assets[1].oi_eff_long_q, 0);
    assert_eq!(group.assets[1].oi_eff_short_q, 0);
    assert_eq!(group.assets[2].oi_eff_long_q, 2 * POS_SCALE);
    assert_eq!(group.assets[2].oi_eff_short_q, 2 * POS_SCALE);
}

#[test]
fn v16_wrapper_asset_authority_rotation_and_burn_gate_lifecycle_updates() {
    let mut admin = signer();
    let mut new_asset_authority = signer();
    let mut attacker = signer();
    let mut market = market_account();

    init_market(&mut admin, &mut market);
    let initialized = market.data.clone();
    assert_err_and_market_unchanged(
        update_asset_lifecycle(
            &mut attacker,
            &mut market,
            processor::ASSET_ACTION_ACTIVATE,
            1,
            1,
            101,
        ),
        &market,
        &initialized,
    );

    run_ix(
        Instruction::UpdateAuthority {
            kind: processor::AUTHORITY_ASSET,
            new_pubkey: new_asset_authority.key.to_bytes(),
        },
        &mut [&mut admin, &mut new_asset_authority, &mut market],
    )
    .unwrap();
    let rotated = market.data.clone();
    assert_err_and_market_unchanged(
        update_asset_lifecycle(
            &mut admin,
            &mut market,
            processor::ASSET_ACTION_ACTIVATE,
            1,
            1,
            101,
        ),
        &market,
        &rotated,
    );
    update_asset_lifecycle(
        &mut new_asset_authority,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        1,
        1,
        101,
    )
    .unwrap();

    run_ix(
        Instruction::UpdateAuthority {
            kind: processor::AUTHORITY_ASSET,
            new_pubkey: [0u8; 32],
        },
        &mut [&mut new_asset_authority, &mut attacker, &mut market],
    )
    .unwrap();
    let burned = market.data.clone();
    assert_err_and_market_unchanged(
        update_asset_lifecycle(
            &mut new_asset_authority,
            &mut market,
            processor::ASSET_ACTION_ACTIVATE,
            2,
            2,
            102,
        ),
        &market,
        &burned,
    );
}

#[test]
fn v16_wrapper_asset_drain_only_and_retire_enforce_engine_lifecycle() {
    let mut admin = signer();
    let mut market = market_account();
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();

    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_portfolio_assets,
                ..
            } = ix
            {
                *max_portfolio_assets = 1;
            }
        }),
    );
    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        1,
        1,
        150,
    )
    .unwrap();

    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 10_000);
    deposit(&mut short_owner, &mut market, &mut short_account, 10_000);
    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 1,
            size_q: POS_SCALE as i128,
            exec_price: 150,
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

    let before_retire_nonempty = market.data.clone();
    assert_err_and_market_unchanged(
        update_asset_lifecycle(
            &mut admin,
            &mut market,
            processor::ASSET_ACTION_RETIRE,
            1,
            2,
            0,
        ),
        &market,
        &before_retire_nonempty,
    );

    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 1,
            size_q: -(POS_SCALE as i128),
            exec_price: 150,
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
    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_DRAIN_ONLY,
        1,
        0,
        0,
    )
    .unwrap();
    let drained = state::read_market(&market.data).unwrap().1;
    assert_eq!(drained.assets[1].lifecycle, AssetLifecycleV16::DrainOnly);

    let before_new_position = market.data.clone();
    let new_position = run_ix(
        Instruction::TradeNoCpi {
            asset_index: 1,
            size_q: POS_SCALE as i128,
            exec_price: 150,
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
    assert_err_and_market_unchanged(new_position, &market, &before_new_position);

    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_RETIRE,
        1,
        2,
        0,
    )
    .unwrap();
    let retired = state::read_market(&market.data).unwrap().1;
    assert_eq!(retired.assets[1].lifecycle, AssetLifecycleV16::Retired);
}

#[test]
fn v16_wrapper_prediction_asset_can_drain_retire_and_reactivate_without_closing_other_legs() {
    let mut admin = signer();
    let mut market = market_account();
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();

    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_portfolio_assets,
                ..
            } = ix
            {
                *max_portfolio_assets = 2;
            }
        }),
    );
    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        2,
        1,
        1_000_000,
    )
    .unwrap();
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 10_000_000);
    deposit(
        &mut short_owner,
        &mut market,
        &mut short_account,
        10_000_000,
    );
    let prediction_q = POS_SCALE / 100;

    // Keep another market leg live while the prediction-style asset is cycled.
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
        Instruction::TradeNoCpi {
            asset_index: 2,
            size_q: prediction_q as i128,
            exec_price: 1_000_000,
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

    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_DRAIN_ONLY,
        2,
        0,
        0,
    )
    .unwrap();
    let before_blocked_risk_increase = market.data.clone();
    let blocked_risk_increase = run_ix(
        Instruction::TradeNoCpi {
            asset_index: 2,
            size_q: prediction_q as i128,
            exec_price: 1_000_000,
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
    assert_err_and_market_unchanged(
        blocked_risk_increase,
        &market,
        &before_blocked_risk_increase,
    );

    let before_retire_with_open_legs = market.data.clone();
    let retire_with_open_legs = update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_RETIRE,
        2,
        2,
        0,
    );
    assert_err_and_market_unchanged(
        retire_with_open_legs,
        &market,
        &before_retire_with_open_legs,
    );

    run_ix(
        Instruction::RebalanceReduce {
            asset_index: 2,
            reduce_q: prediction_q,
        },
        &mut [&mut long_owner, &mut market, &mut long_account],
    )
    .unwrap();
    let after_unilateral_reduce = state::read_portfolio(&short_account.data).unwrap();
    assert!(
        active_leg_for_asset(&after_unilateral_reduce, 2).active,
        "counterparty leg is stale/dead after unilateral reduction and must be explicitly cleared"
    );
    let second_unilateral_reduce = run_ix(
        Instruction::RebalanceReduce {
            asset_index: 2,
            reduce_q: prediction_q,
        },
        &mut [&mut short_owner, &mut market, &mut short_account],
    );
    assert!(
        second_unilateral_reduce.is_err(),
        "once the opposite side is reset-pending, cleanup uses the dead-leg forfeit path"
    );
    run_ix(
        Instruction::ForfeitRecoveryLeg {
            asset_index: 2,
            b_delta_budget: 1,
        },
        &mut [&mut short_owner, &mut market, &mut short_account],
    )
    .unwrap();
    run_ix(
        Instruction::FinalizeResetSide {
            asset_index: 2,
            side: 0,
        },
        &mut [&mut market],
    )
    .unwrap();
    run_ix(
        Instruction::FinalizeResetSide {
            asset_index: 2,
            side: 1,
        },
        &mut [&mut market],
    )
    .unwrap();

    let (cfg, group) = state::read_market(&market.data).unwrap();
    let long = state::read_portfolio(&long_account.data).unwrap();
    let short = state::read_portfolio(&short_account.data).unwrap();
    assert_eq!(long.active_bitmap, active_bitmap_with(&[0]));
    assert_eq!(short.active_bitmap, active_bitmap_with(&[0]));
    assert_eq!(group.assets[0].oi_eff_long_q, POS_SCALE);
    assert_eq!(group.assets[0].oi_eff_short_q, POS_SCALE);
    assert_eq!(group.assets[2].oi_eff_long_q, 0);
    assert_eq!(group.assets[2].oi_eff_short_q, 0);

    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_RETIRE,
        2,
        group.current_slot + 1,
        0,
    )
    .unwrap();
    let retired = state::read_market(&market.data).unwrap().1;
    assert_eq!(retired.assets[2].lifecycle, AssetLifecycleV16::Retired);
    assert_eq!(retired.assets[2].retired_slot, group.current_slot + 1);
    let old_market_id = retired.assets[2].market_id;
    let next_market_id_before = retired.next_market_id;

    let before_early_reuse = market.data.clone();
    assert_err_and_market_unchanged(
        update_asset_lifecycle(
            &mut admin,
            &mut market,
            processor::ASSET_ACTION_ACTIVATE,
            2,
            retired.current_slot,
            500_000,
        ),
        &market,
        &before_early_reuse,
    );

    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        2,
        retired.current_slot + 1,
        500_000,
    )
    .unwrap();
    let reactivated = state::read_market(&market.data).unwrap().1;
    assert_eq!(reactivated.assets[2].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(reactivated.assets[2].market_id, next_market_id_before);
    assert!(reactivated.assets[2].market_id > old_market_id);
    assert_eq!(reactivated.next_market_id, next_market_id_before + 1);
    assert_eq!(reactivated.assets[2].effective_price, 500_000);

    let mut stale_long = state::read_portfolio(&long_account.data).unwrap();
    stale_long.legs[1] = PortfolioLegV16 {
        active: true,
        asset_index: 2,
        market_id: old_market_id,
        side: SideV16::Long,
        basis_pos_q: prediction_q as i128,
        a_basis: percolator::ADL_ONE,
        k_snap: 0,
        f_snap: 0,
        epoch_snap: 0,
        loss_weight: prediction_q,
        b_snap: 0,
        b_rem: 0,
        b_epoch_snap: 0,
        b_stale: false,
        stale: false,
    };
    stale_long.active_bitmap = active_bitmap_with(&[0, 1]);
    state::write_portfolio(&mut long_account.data, &stale_long).unwrap();
    let before_stale_market_id = market.data.clone();
    let stale_market_id_deposit = {
        let mint =
            Pubkey::new_from_array(state::read_market(&market.data).unwrap().0.collateral_mint);
        let mut source_token = user_token_account(long_owner.key, mint, 1);
        let mut vault_token = vault_token_account(&market, mint, 0);
        let mut token_program = token_program_account();
        run_ix(
            Instruction::Deposit { amount: 1 },
            &mut [
                &mut long_owner,
                &mut market,
                &mut long_account,
                &mut source_token,
                &mut vault_token,
                &mut token_program,
            ],
        )
    };
    assert_err_and_market_unchanged(stale_market_id_deposit, &market, &before_stale_market_id);
    let mut clean_long = state::read_portfolio(&long_account.data).unwrap();
    clean_long.legs[1] = percolator::PortfolioLegV16::EMPTY;
    clean_long.active_bitmap = active_bitmap_with(&[0]);
    state::write_portfolio(&mut long_account.data, &clean_long).unwrap();

    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 2,
            size_q: prediction_q as i128,
            exec_price: 500_000,
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
    let (_, final_group) = state::read_market(&market.data).unwrap();
    let long = state::read_portfolio(&long_account.data).unwrap();
    let short = state::read_portfolio(&short_account.data).unwrap();
    assert_eq!(long.active_bitmap, active_bitmap_with(&[0, 1]));
    assert_eq!(short.active_bitmap, active_bitmap_with(&[0, 1]));
    assert_eq!(final_group.assets[0].oi_eff_long_q, POS_SCALE);
    assert_eq!(final_group.assets[0].oi_eff_short_q, POS_SCALE);
    assert_eq!(final_group.assets[2].oi_eff_long_q, prediction_q);
    assert_eq!(final_group.assets[2].oi_eff_short_q, prediction_q);
    assert_eq!(cfg.admin, state::read_market(&market.data).unwrap().0.admin);
}

#[test]
fn v16_wrapper_reactivated_asset_resets_prior_oracle_profile() {
    let mut admin = signer();
    let mut market = market_account();

    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_portfolio_assets,
                ..
            } = ix
            {
                *max_portfolio_assets = 2;
            }
        }),
    );
    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        2,
        1,
        1_000_000,
    )
    .unwrap();
    run_ix(
        Instruction::ConfigureHyperpMark {
            asset_index: 2,
            now_slot: 2,
            initial_mark_e6: 123_000,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    assert_eq!(
        state::read_asset_oracle_profile(&market.data, 2)
            .unwrap()
            .oracle_mode,
        ORACLE_MODE_HYPERP
    );

    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_DRAIN_ONLY,
        2,
        0,
        0,
    )
    .unwrap();
    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_RETIRE,
        2,
        3,
        0,
    )
    .unwrap();
    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        2,
        4,
        500_000,
    )
    .unwrap();

    let (_, group) = state::read_market(&market.data).unwrap();
    let profile = state::read_asset_oracle_profile(&market.data, 2).unwrap();
    assert_eq!(group.assets[2].effective_price, 500_000);
    assert_eq!(
        profile.oracle_mode, ORACLE_MODE_MANUAL,
        "reactivating a retired asset slot must not inherit its previous market's price-managed profile"
    );
    assert_eq!(profile.oracle_target_price_e6, 500_000);
    assert_eq!(profile.mark_ewma_e6, 500_000);
    assert_eq!(profile.last_good_oracle_slot, 4);
}

#[test]
fn v16_wrapper_retired_asset_profile_cannot_refresh_market_liveness() {
    let mut admin = signer();
    let mut market = market_account();

    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_portfolio_assets,
                ..
            } = ix
            {
                *max_portfolio_assets = 2;
            }
        }),
    );
    run_ix(
        Instruction::ConfigurePermissionlessResolve {
            stale_slots: 5,
            force_close_delay_slots: 1,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        2,
        1,
        1_000_000,
    )
    .unwrap();
    run_ix(
        Instruction::ConfigureHyperpMark {
            asset_index: 2,
            now_slot: 1,
            initial_mark_e6: 123_000,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_DRAIN_ONLY,
        2,
        0,
        0,
    )
    .unwrap();
    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_RETIRE,
        2,
        2,
        0,
    )
    .unwrap();

    let retired_profile = state::read_asset_oracle_profile(&market.data, 2).unwrap();
    assert_eq!(
        retired_profile.oracle_mode, ORACLE_MODE_MANUAL,
        "retired slots must not keep a price-managed profile that can refresh global liveness"
    );
    let before_push = market.data.clone();
    let stale_refresh = run_ix(
        Instruction::PushHyperpMark {
            asset_index: 2,
            now_slot: 4,
            mark_e6: 222_000,
        },
        &mut [&mut admin, &mut market],
    );
    assert_err_and_market_unchanged(stale_refresh, &market, &before_push);

    run_ix(
        Instruction::ResolveStalePermissionless { now_slot: 6 },
        &mut [&mut market],
    )
    .unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(group.mode, MarketModeV16::Resolved);
}

#[test]
fn v16_wrapper_three_asset_hybrid_prediction_shutdown_reuses_only_prediction_slot() {
    let mut admin = signer();
    let mut market = market_account();
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();

    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_portfolio_assets,
                max_trading_fee_bps,
                trade_fee_base_bps,
                max_price_move_bps_per_slot,
                maintenance_fee_per_slot,
                ..
            } = ix
            {
                *max_portfolio_assets = 3;
                *max_trading_fee_bps = 10_000;
                *trade_fee_base_bps = 1;
                *max_price_move_bps_per_slot = 500;
                *maintenance_fee_per_slot = 3;
            }
        }),
    );

    let feeds = [[0xe1u8; 32], [0xe2u8; 32], [0xe3u8; 32]];
    let mut stoxx_jpy = pyth_account(&feeds[0], 4_000_000_000, -6, 1, 100);
    let mut usd_jpy = pyth_account(&feeds[1], 150_000_000, -6, 1, 100);
    let mut sol_usd = pyth_account(&feeds[2], 200_000_000, -6, 1, 100);
    configure_three_leg_hybrid(
        &mut admin,
        &mut market,
        feeds,
        &mut stoxx_jpy,
        &mut usd_jpy,
        &mut sol_usd,
        1,
        100,
        2,
        0,
    );
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 10_000_000);
    deposit(
        &mut short_owner,
        &mut market,
        &mut short_account,
        10_000_000,
    );
    let prediction_q = POS_SCALE / 100;
    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: POS_SCALE as i128,
            exec_price: 133_333,
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
        Instruction::TradeNoCpi {
            asset_index: 1,
            size_q: prediction_q as i128,
            exec_price: 1_000_000,
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
        Instruction::TradeNoCpi {
            asset_index: 2,
            size_q: (2 * POS_SCALE) as i128,
            exec_price: 250,
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

    let (before_cfg, before_group) = state::read_market(&market.data).unwrap();
    assert_eq!(before_cfg.oracle_mode, ORACLE_MODE_HYBRID_AFTER_HOURS);
    assert_eq!(before_group.assets[0].effective_price, 133_333);
    assert_eq!(before_group.assets[1].effective_price, 100);
    assert_eq!(before_group.assets[2].effective_price, 100);

    // Once the external asset-0 oracle is soft-stale, a prediction-asset
    // crank must still use the prediction asset's own supplied price. The
    // asset-0 hybrid EWMA/composite is not a valid price for asset 1.
    run_ix(
        Instruction::PermissionlessCrank {
            action: 0,
            asset_index: 1,
            now_slot: before_group.current_slot + 3,
            effective_price: 100,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut admin, &mut market, &mut long_account],
    )
    .unwrap();
    let (after_prediction_crank_cfg, after_prediction_crank) =
        state::read_market(&market.data).unwrap();
    assert_eq!(
        after_prediction_crank.assets[1].effective_price, 100,
        "asset-1 prediction crank must not inherit the asset-0 hybrid fallback mark"
    );
    assert_eq!(
        after_prediction_crank_cfg.mark_ewma_e6, before_cfg.mark_ewma_e6,
        "nonzero-asset crank must not mutate the asset-0 hybrid mark"
    );
    assert_eq!(
        after_prediction_crank.assets[0].effective_price,
        before_group.assets[0].effective_price
    );
    assert_eq!(
        after_prediction_crank.assets[2].effective_price,
        before_group.assets[2].effective_price
    );

    // Drain and clear the prediction slot while the other two assets remain
    // open. This models a resolved prediction market being removed so the slot
    // can host the next prediction market.
    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_DRAIN_ONLY,
        1,
        0,
        0,
    )
    .unwrap();
    let blocked_new_prediction_risk = run_ix(
        Instruction::TradeNoCpi {
            asset_index: 1,
            size_q: prediction_q as i128,
            exec_price: 1_000_000,
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
    assert!(blocked_new_prediction_risk.is_err());

    run_ix(
        Instruction::RebalanceReduce {
            asset_index: 1,
            reduce_q: prediction_q,
        },
        &mut [&mut long_owner, &mut market, &mut long_account],
    )
    .unwrap();
    run_ix(
        Instruction::ForfeitRecoveryLeg {
            asset_index: 1,
            b_delta_budget: 1,
        },
        &mut [&mut short_owner, &mut market, &mut short_account],
    )
    .unwrap();
    run_ix(
        Instruction::FinalizeResetSide {
            asset_index: 1,
            side: 0,
        },
        &mut [&mut market],
    )
    .unwrap();
    run_ix(
        Instruction::FinalizeResetSide {
            asset_index: 1,
            side: 1,
        },
        &mut [&mut market],
    )
    .unwrap();

    let (_, cleared_group) = state::read_market(&market.data).unwrap();
    let long_after_clear = state::read_portfolio(&long_account.data).unwrap();
    let short_after_clear = state::read_portfolio(&short_account.data).unwrap();
    assert_eq!(long_after_clear.active_bitmap, active_bitmap_with(&[0, 2]));
    assert_eq!(short_after_clear.active_bitmap, active_bitmap_with(&[0, 2]));
    assert_eq!(cleared_group.assets[1].oi_eff_long_q, 0);
    assert_eq!(cleared_group.assets[1].oi_eff_short_q, 0);
    assert_eq!(cleared_group.assets[0].oi_eff_long_q, POS_SCALE);
    assert_eq!(cleared_group.assets[2].oi_eff_long_q, 2 * POS_SCALE);

    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_RETIRE,
        1,
        cleared_group.current_slot + 1,
        0,
    )
    .unwrap();
    let retired = state::read_market(&market.data).unwrap().1;
    let old_prediction_market_id = retired.assets[1].market_id;
    let next_market_id = retired.next_market_id;

    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        1,
        retired.current_slot + 1,
        750_000,
    )
    .unwrap();
    let (_, reused_group) = state::read_market(&market.data).unwrap();
    assert_eq!(reused_group.assets[1].market_id, next_market_id);
    assert!(reused_group.assets[1].market_id > old_prediction_market_id);
    assert_eq!(reused_group.assets[1].effective_price, 750_000);
    assert_eq!(
        reused_group.assets[0].effective_price,
        before_group.assets[0].effective_price
    );
    assert_eq!(reused_group.assets[2].effective_price, 100);

    let mut stale_prediction_leg = state::read_portfolio(&long_account.data).unwrap();
    stale_prediction_leg.legs[1] = PortfolioLegV16 {
        active: true,
        asset_index: 1,
        market_id: old_prediction_market_id,
        side: SideV16::Long,
        basis_pos_q: prediction_q as i128,
        a_basis: percolator::ADL_ONE,
        k_snap: 0,
        f_snap: 0,
        epoch_snap: 0,
        loss_weight: prediction_q,
        b_snap: 0,
        b_rem: 0,
        b_epoch_snap: 0,
        b_stale: false,
        stale: false,
    };
    stale_prediction_leg.active_bitmap = active_bitmap_with(&[0, 1, 2]);
    state::write_portfolio(&mut long_account.data, &stale_prediction_leg).unwrap();
    let before_stale_trade = market.data.clone();
    let stale_reuse_trade = run_ix(
        Instruction::TradeNoCpi {
            asset_index: 1,
            size_q: prediction_q as i128,
            exec_price: 750_000,
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
    assert_err_and_market_unchanged(stale_reuse_trade, &market, &before_stale_trade);
}

#[test]
fn v16_wrapper_security_sweep_reused_asset_market_ids_fail_closed() {
    let mut admin = signer();
    let mut market = market_account();
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();

    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_portfolio_assets,
                ..
            } = ix
            {
                *max_portfolio_assets = 1;
            }
        }),
    );
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 10_000);
    deposit(&mut short_owner, &mut market, &mut short_account, 10_000);

    let invalid_index = percolator::V16_MAX_MARKET_SLOTS_N as u16;
    let last_asset = 1u16;
    let before_invalid = market.data.clone();
    assert_err_and_market_unchanged(
        update_asset_lifecycle(
            &mut admin,
            &mut market,
            processor::ASSET_ACTION_ACTIVATE,
            invalid_index,
            1,
            200,
        ),
        &market,
        &before_invalid,
    );

    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        last_asset,
        1,
        200,
    )
    .unwrap();
    let activated = state::read_market(&market.data).unwrap().1;
    assert_eq!(
        activated.config.max_market_slots as usize, 2,
        "activating the next append slot grows the configured market-slot set"
    );
    let old_market_id = activated.assets[last_asset as usize].market_id;
    let next_market_id_before = activated.next_market_id;

    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_RETIRE,
        last_asset,
        2,
        0,
    )
    .unwrap();
    let retired = state::read_market(&market.data).unwrap().1;
    assert_eq!(
        retired.assets[last_asset as usize].lifecycle,
        AssetLifecycleV16::Retired
    );

    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        last_asset,
        retired.current_slot + 1,
        250,
    )
    .unwrap();
    let reactivated = state::read_market(&market.data).unwrap().1;
    let new_market_id = reactivated.assets[last_asset as usize].market_id;
    assert_eq!(new_market_id, next_market_id_before);
    assert!(new_market_id > old_market_id);
    assert_eq!(reactivated.next_market_id, next_market_id_before + 1);

    let mut pass_count = 1usize; // invalid fixed-capacity grow rejection above.
    let mut stale = state::read_portfolio(&long_account.data).unwrap();
    stale.legs[0] = PortfolioLegV16 {
        active: true,
        asset_index: last_asset as u32,
        market_id: old_market_id,
        side: SideV16::Long,
        basis_pos_q: POS_SCALE as i128,
        a_basis: percolator::ADL_ONE,
        k_snap: 0,
        f_snap: 0,
        epoch_snap: 0,
        loss_weight: POS_SCALE,
        b_snap: 0,
        b_rem: 0,
        b_epoch_snap: 0,
        b_stale: false,
        stale: false,
    };
    stale.active_bitmap = active_bitmap_with(&[0]);
    state::write_portfolio(&mut long_account.data, &stale).unwrap();
    let stale_market = market.data.clone();
    let stale_portfolio = long_account.data.clone();

    let mint = Pubkey::new_from_array(state::read_market(&market.data).unwrap().0.collateral_mint);
    let mut source = user_token_account(long_owner.key, mint, 1);
    let mut vault = vault_token_account(&market, mint, 1);
    let mut token_program = token_program_account();
    let deposit_stale = run_ix(
        Instruction::Deposit { amount: 1 },
        &mut [
            &mut long_owner,
            &mut market,
            &mut long_account,
            &mut source,
            &mut vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(deposit_stale, &market, &stale_market);
    assert_eq!(long_account.data, stale_portfolio);
    pass_count += 1;

    let mut dest = user_token_account(long_owner.key, mint, 0);
    let mut vault = vault_token_account(&market, mint, 1);
    let mut vault_auth = vault_authority_account(&market);
    let mut token_program = token_program_account();
    let withdraw_stale = run_ix(
        Instruction::Withdraw { amount: 1 },
        &mut [
            &mut long_owner,
            &mut market,
            &mut long_account,
            &mut dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(withdraw_stale, &market, &stale_market);
    assert_eq!(long_account.data, stale_portfolio);
    pass_count += 1;

    let sync_stale = sync_maintenance_fee(&mut market, &mut long_account, 10);
    assert_err_and_market_unchanged(sync_stale, &market, &stale_market);
    assert_eq!(long_account.data, stale_portfolio);
    pass_count += 1;

    let refresh_stale = run_ix(
        Instruction::PermissionlessCrank {
            action: 0,
            asset_index: last_asset,
            now_slot: 10,
            effective_price: 250,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut admin, &mut market, &mut long_account],
    );
    assert_err_and_market_unchanged(refresh_stale, &market, &stale_market);
    assert_eq!(long_account.data, stale_portfolio);
    pass_count += 1;

    let trade_stale = run_ix(
        Instruction::TradeNoCpi {
            asset_index: last_asset,
            size_q: POS_SCALE as i128,
            exec_price: 250,
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
    assert_err_and_market_unchanged(trade_stale, &market, &stale_market);
    assert_eq!(long_account.data, stale_portfolio);
    pass_count += 1;

    let reduce_stale = run_ix(
        Instruction::RebalanceReduce {
            asset_index: last_asset,
            reduce_q: 1,
        },
        &mut [&mut long_owner, &mut market, &mut long_account],
    );
    assert_err_and_market_unchanged(reduce_stale, &market, &stale_market);
    assert_eq!(long_account.data, stale_portfolio);
    pass_count += 1;

    let forfeit_stale = run_ix(
        Instruction::ForfeitRecoveryLeg {
            asset_index: last_asset,
            b_delta_budget: 1,
        },
        &mut [&mut long_owner, &mut market, &mut long_account],
    );
    assert_err_and_market_unchanged(forfeit_stale, &market, &stale_market);
    assert_eq!(long_account.data, stale_portfolio);
    pass_count += 1;

    let close_stale = run_ix(
        Instruction::ClosePortfolio,
        &mut [&mut long_owner, &mut market, &mut long_account],
    );
    assert_err_and_market_unchanged(close_stale, &market, &stale_market);
    assert_eq!(long_account.data, stale_portfolio);
    pass_count += 1;

    let mut source_claim_stale = state::read_portfolio(&long_account.data).unwrap();
    source_claim_stale.legs[0] = PortfolioLegV16::EMPTY;
    source_claim_stale.active_bitmap = active_bitmap_with(&[]);
    let domain = last_asset as usize * 2;
    source_claim_stale.source_claim_market_id[domain] = old_market_id;
    source_claim_stale.source_claim_bound_num[domain] = BOUND_SCALE;
    state::write_portfolio(&mut long_account.data, &source_claim_stale).unwrap();
    let source_claim_portfolio = long_account.data.clone();
    let source_claim_sync = sync_maintenance_fee(&mut market, &mut long_account, 10);
    assert_err_and_market_unchanged(source_claim_sync, &market, &stale_market);
    assert_eq!(long_account.data, source_claim_portfolio);
    pass_count += 1;

    let mut close_progress_stale = state::read_portfolio(&long_account.data).unwrap();
    close_progress_stale.source_claim_market_id[domain] = 0;
    close_progress_stale.source_claim_bound_num[domain] = 0;
    close_progress_stale.close_progress = CloseProgressLedgerV16 {
        active: true,
        finalized: false,
        canceled: false,
        close_id: 1,
        asset_index: last_asset as u32,
        market_id: old_market_id,
        domain_side: SideV16::Long,
        gross_loss_at_close_start: 1,
        drift_reference_slot: reactivated.current_slot,
        max_close_slot: reactivated.current_slot + 10,
        residual_remaining: 1,
        ..CloseProgressLedgerV16::EMPTY
    };
    state::write_portfolio(&mut long_account.data, &close_progress_stale).unwrap();
    let close_progress_portfolio = long_account.data.clone();
    let close_progress_sync = sync_maintenance_fee(&mut market, &mut long_account, 10);
    assert_err_and_market_unchanged(close_progress_sync, &market, &stale_market);
    assert_eq!(long_account.data, close_progress_portfolio);
    pass_count += 1;

    let mut clean = state::read_portfolio(&long_account.data).unwrap();
    clean.close_progress = CloseProgressLedgerV16::EMPTY;
    state::write_portfolio(&mut long_account.data, &clean).unwrap();
    run_ix(
        Instruction::TradeNoCpi {
            asset_index: last_asset,
            size_q: POS_SCALE as i128,
            exec_price: 250,
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
    pass_count += 1;

    assert!(
        pass_count >= 10,
        "security sweep must retain at least ten fail-closed/pass-safe probes for reused market ids"
    );
}

#[test]
fn v16_wrapper_security_sweep_resolved_market_and_fee_branches() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_portfolio_assets,
                maintenance_fee_per_slot,
                ..
            } = ix
            {
                *max_portfolio_assets = 1;
                *maintenance_fee_per_slot = 5;
            }
        }),
    );
    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        1,
        1,
        125,
    )
    .unwrap();
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 100);
    run_ix(
        Instruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 10,
            effective_price: 100,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut owner, &mut market, &mut portfolio],
    )
    .unwrap();

    run_ix(Instruction::ResolveMarket, &mut [&mut admin, &mut market]).unwrap();
    let resolved = state::read_market(&market.data).unwrap().1;
    assert_eq!(resolved.mode, MarketModeV16::Resolved);
    assert_eq!(resolved.resolved_slot, 10);

    let before_asset_mutation = market.data.clone();
    let lifecycle_after_resolve = update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_RETIRE,
        1,
        11,
        0,
    );
    assert_err_and_market_unchanged(lifecycle_after_resolve, &market, &before_asset_mutation);

    sync_maintenance_fee(&mut market, &mut portfolio, 100).unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    let account = state::read_portfolio(&portfolio.data).unwrap();
    assert_eq!(
        account.last_fee_slot, 10,
        "resolved fee sync must anchor at resolved_slot, not caller-supplied now_slot"
    );
    assert_eq!(
        account.capital, 55,
        "resolved fees start at the portfolio creation fee slot"
    );
    assert_eq!(group.insurance, 45);
    assert_eq!(group.c_tot, 55);

    let after_fee = market.data.clone();
    let after_fee_portfolio = portfolio.data.clone();
    sync_maintenance_fee(&mut market, &mut portfolio, 200).unwrap();
    assert_eq!(
        market.data, after_fee,
        "resolved fee sync must be idempotent after resolved_slot is reached"
    );
    assert_eq!(portfolio.data, after_fee_portfolio);
}

#[test]
fn v16_wrapper_account_layout_constants_match_serialized_state() {
    assert_eq!(
        MARKET_GROUP_LEN,
        core::mem::size_of::<MarketGroupV16Account>()
    );
    assert_eq!(
        PORTFOLIO_STATE_LEN,
        core::mem::size_of::<PortfolioAccountV16Account>()
    );
    assert_eq!(
        WRAPPER_CONFIG_LEN,
        core::mem::size_of::<state::WrapperConfigV16>()
    );
    assert_eq!(core::mem::align_of::<MarketGroupV16Account>(), 1);
    assert_eq!(core::mem::align_of::<PortfolioAccountV16Account>(), 1);
    assert_eq!(
        MARKET_ACCOUNT_LEN,
        16 + WRAPPER_CONFIG_LEN + ASSET_ORACLE_PROFILES_LEN + MARKET_GROUP_LEN,
        "market account length must exactly cover header + wrapper config + per-asset oracle profiles + serialized engine state"
    );
    assert_eq!(
        PORTFOLIO_ACCOUNT_LEN,
        16 + PORTFOLIO_STATE_LEN,
        "portfolio account length must exactly cover header + serialized portfolio state"
    );
    assert_eq!(state::alignment_note(), 1);

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

    let mut corrupt_market = market.data.clone();
    corrupt_market[MARKET_ACCOUNT_LEN - 1] = 2;
    assert_eq!(
        state::read_market(&corrupt_market),
        Err(ProgramError::InvalidAccountData),
        "serialized bool fields must be validated before engine state is materialized"
    );
    let mut corrupt_portfolio = portfolio.data.clone();
    let close_progress_active_off = HEADER_LEN
        + core::mem::offset_of!(PortfolioAccountV16Account, close_progress)
        + core::mem::offset_of!(percolator::CloseProgressLedgerV16Account, active);
    corrupt_portfolio[close_progress_active_off] = 2;
    assert_eq!(
        state::read_portfolio(&corrupt_portfolio),
        Err(ProgramError::InvalidAccountData),
        "serialized portfolio bool fields must be validated before engine state is materialized"
    );
}

#[test]
fn v16_wrapper_raw_wrapper_config_invalid_values_fail_closed() {
    let mut admin = signer();
    let mut market = market_account();
    init_market(&mut admin, &mut market);

    let (cfg, _group) = state::read_market(&market.data).unwrap();
    let assert_bad_cfg = |bad_cfg: state::WrapperConfigV16, label: &str| {
        let mut corrupt = market.data.clone();
        corrupt[HEADER_LEN..HEADER_LEN + WRAPPER_CONFIG_LEN]
            .copy_from_slice(bytemuck::bytes_of(&bad_cfg));
        assert_eq!(
            state::read_market(&corrupt),
            Err(ProgramError::InvalidAccountData),
            "{}",
            label
        );
    };

    let mut bad_reward = cfg;
    bad_reward.liquidation_cranker_fee_share_bps = 10_001;
    assert_bad_cfg(
        bad_reward,
        "corrupt liquidation reward share must not be trusted on read",
    );

    let mut bad_maintenance_reward = cfg;
    bad_maintenance_reward.maintenance_cranker_fee_share_bps = 10_001;
    assert_bad_cfg(
        bad_maintenance_reward,
        "corrupt maintenance reward share must not be trusted on read",
    );

    let mut bad_oracle_mode = cfg;
    bad_oracle_mode.oracle_mode = 9;
    assert_bad_cfg(
        bad_oracle_mode,
        "retired or unknown oracle modes must fail closed",
    );

    let mut bad_manual_legs = cfg;
    bad_manual_legs.oracle_leg_count = 1;
    assert_bad_cfg(
        bad_manual_legs,
        "manual oracle mode must not carry stale hybrid leg metadata",
    );

    let mut bad_hybrid_leg = cfg;
    bad_hybrid_leg.oracle_mode = ORACLE_MODE_HYBRID_AFTER_HOURS;
    bad_hybrid_leg.oracle_leg_count = 1;
    bad_hybrid_leg.max_staleness_secs = 60;
    bad_hybrid_leg.hybrid_soft_stale_slots = 1;
    bad_hybrid_leg.mark_ewma_e6 = 100;
    assert_bad_cfg(
        bad_hybrid_leg,
        "hybrid oracle mode must reject malformed persisted leg metadata",
    );

    let mut bad_hyperp_stale_hybrid_knob = cfg;
    bad_hyperp_stale_hybrid_knob.oracle_mode = ORACLE_MODE_HYPERP;
    bad_hyperp_stale_hybrid_knob.mark_ewma_e6 = 100;
    bad_hyperp_stale_hybrid_knob.mark_ewma_halflife_slots = 1;
    bad_hyperp_stale_hybrid_knob.unit_scale = 6;
    assert_bad_cfg(
        bad_hyperp_stale_hybrid_knob,
        "Hyperp oracle mode must not carry stale hybrid conversion knobs",
    );

    let mut bad_bool = cfg;
    bad_bool.insurance_withdraw_deposits_only = 2;
    assert_bad_cfg(
        bad_bool,
        "persistent wrapper booleans must reject invalid byte patterns",
    );

    let mut bad_padding = cfg;
    bad_padding._padding0 = 1;
    assert_bad_cfg(
        bad_padding,
        "wrapper config padding must stay canonical zero bytes",
    );
}

#[test]
fn v16_wrapper_init_market_rejects_invalid_mint_and_double_init() {
    let mut admin = signer();
    let mut market = market_account();
    let mut bad_mint = invalid_mint_account();

    let before = market.data.clone();
    let invalid_mint = run_ix(
        default_init_market_ix(),
        &mut [&mut admin, &mut market, &mut bad_mint],
    );
    assert_err_and_market_unchanged(invalid_mint, &market, &before);

    let mut good_mint = mint_account();
    run_ix(
        default_init_market_ix(),
        &mut [&mut admin, &mut market, &mut good_mint],
    )
    .unwrap();
    let initialized = market.data.clone();
    let double_init = run_ix(
        default_init_market_ix(),
        &mut [&mut admin, &mut market, &mut good_mint],
    );
    assert_err_and_market_unchanged(double_init, &market, &initialized);
}

#[test]
fn v16_wrapper_init_and_account_meta_guards_fail_before_mutation() {
    let mut admin = signer();
    let mut unsigned_admin = TestAccount::new(admin.key, Pubkey::new_unique(), 0);
    let mut market = market_account();
    let mut mint = mint_account();

    let before_market = market.data.clone();
    let missing_admin_signature = run_ix(
        default_init_market_ix(),
        &mut [&mut unsigned_admin, &mut market, &mut mint],
    );
    assert_err_and_market_unchanged(missing_admin_signature, &market, &before_market);

    market.is_writable = false;
    let nonwritable_market = run_ix(
        default_init_market_ix(),
        &mut [&mut admin, &mut market, &mut mint],
    );
    assert_err_and_market_unchanged(nonwritable_market, &market, &before_market);
    market.is_writable = true;

    market.owner = Pubkey::new_unique();
    let wrong_market_owner = run_ix(
        default_init_market_ix(),
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
fn v16_wrapper_init_market_rejects_invalid_engine_params_without_mutation() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();

    let before = market.data.clone();
    let zero_price = run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket { initial_price, .. } = ix {
                *initial_price = 0;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    );
    assert_err_and_market_unchanged(zero_price, &market, &before);

    let zero_dt = run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_accrual_dt_slots,
                ..
            } = ix
            {
                *max_accrual_dt_slots = 0;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    );
    assert_err_and_market_unchanged(zero_dt, &market, &before);

    let zero_price_move = run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_price_move_bps_per_slot,
                ..
            } = ix
            {
                *max_price_move_bps_per_slot = 0;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    );
    assert_err_and_market_unchanged(zero_price_move, &market, &before);

    let invalid_min_margin_floor = run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                min_nonzero_mm_req, ..
            } = ix
            {
                *min_nonzero_mm_req = 0;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    );
    assert_err_and_market_unchanged(invalid_min_margin_floor, &market, &before);

    let invalid_liq_floor = run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                liquidation_fee_cap,
                min_liquidation_abs,
                ..
            } = ix
            {
                *liquidation_fee_cap = 1;
                *min_liquidation_abs = 2;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    );
    assert_err_and_market_unchanged(invalid_liq_floor, &market, &before);

    let invalid_base_fee = run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_trading_fee_bps,
                trade_fee_base_bps,
                ..
            } = ix
            {
                *max_trading_fee_bps = 99;
                *trade_fee_base_bps = 100;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    );
    assert_err_and_market_unchanged(invalid_base_fee, &market, &before);

    let invalid_portfolio_leg_cap = run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_portfolio_assets,
                ..
            } = ix
            {
                *max_portfolio_assets =
                    percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS + 1;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    );
    assert_err_and_market_unchanged(invalid_portfolio_leg_cap, &market, &before);

    let invalid_funding_cap = run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_abs_funding_e9_per_slot,
                ..
            } = ix
            {
                *max_abs_funding_e9_per_slot = 10_001;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    );
    assert_err_and_market_unchanged(invalid_funding_cap, &market, &before);

    let invalid_b_budget = run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_account_b_settlement_chunks,
                ..
            } = ix
            {
                *max_account_b_settlement_chunks = 0;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    );
    assert_err_and_market_unchanged(invalid_b_budget, &market, &before);
}

#[test]
fn v16_wrapper_init_portfolio_requires_signer_and_rejects_double_init_without_mutation() {
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
fn v16_wrapper_top_up_insurance_requires_authority_and_updates_vault() {
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
fn v16_wrapper_top_up_insurance_rejects_wrong_mint_and_insufficient_source_balance() {
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
fn v16_wrapper_resolved_insurance_authority_can_withdraw_all_remaining_insurance() {
    let mut admin = signer();
    let mut market = market_account();
    let mint = init_market(&mut admin, &mut market);
    let mut source = user_token_account(admin.key, mint, 100);
    let mut vault = vault_token_account(&market, mint, 100);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::TopUpInsurance { amount: 100 },
        &mut [
            &mut admin,
            &mut market,
            &mut source,
            &mut vault,
            &mut token_program,
        ],
    )
    .unwrap();
    run_ix(Instruction::ResolveMarket, &mut [&mut admin, &mut market]).unwrap();
    run_ix(
        Instruction::UpdateInsurancePolicy {
            max_bps: 1,
            deposits_only: 0,
            cooldown_slots: 100,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();

    let mut dest = user_token_account(admin.key, mint, 0);
    let mut vault_auth = vault_authority_account(&market);
    run_ix(
        Instruction::WithdrawInsurance { amount: 100 },
        &mut [
            &mut admin,
            &mut market,
            &mut dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    )
    .unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(group.insurance, 0);
    assert_eq!(group.vault, 0);
}

#[test]
fn v16_wrapper_resolved_insurance_withdraw_rejects_live_wrong_authority_and_open_accounts() {
    let mut admin = signer();
    let mut attacker = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();
    let mint = init_market(&mut admin, &mut market);
    let mut source = user_token_account(admin.key, mint, 100);
    let mut vault = vault_token_account(&market, mint, 100);
    let mut dest = user_token_account(admin.key, mint, 0);
    let mut vault_auth = vault_authority_account(&market);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::TopUpInsurance { amount: 100 },
        &mut [
            &mut admin,
            &mut market,
            &mut source,
            &mut vault,
            &mut token_program,
        ],
    )
    .unwrap();

    let live = market.data.clone();
    let live_reject = run_ix(
        Instruction::WithdrawInsurance { amount: 1 },
        &mut [
            &mut admin,
            &mut market,
            &mut dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(live_reject, &market, &live);

    init_portfolio(&mut owner, &mut market, &mut portfolio);
    run_ix(Instruction::ResolveMarket, &mut [&mut admin, &mut market]).unwrap();
    let resolved_with_open = market.data.clone();
    let open_reject = run_ix(
        Instruction::WithdrawInsurance { amount: 1 },
        &mut [
            &mut admin,
            &mut market,
            &mut dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(open_reject, &market, &resolved_with_open);

    let wrong_auth = run_ix(
        Instruction::WithdrawInsurance { amount: 1 },
        &mut [
            &mut attacker,
            &mut market,
            &mut dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert!(wrong_auth.is_err());
}

#[test]
fn v16_wrapper_top_up_backing_bucket_uses_separate_authority_and_domain_ledger() {
    let mut admin = signer();
    let mut market = market_account();
    let mut bucket_authority = signer();
    let mut attacker = signer();

    let mint = init_market(&mut admin, &mut market);
    run_ix(
        Instruction::UpdateAuthority {
            kind: processor::AUTHORITY_BACKING_BUCKET,
            new_pubkey: bucket_authority.key.to_bytes(),
        },
        &mut [&mut admin, &mut bucket_authority, &mut market],
    )
    .unwrap();

    let before = market.data.clone();
    let mut attacker_src = user_token_account(attacker.key, mint, 100);
    let mut vault = vault_token_account(&market, mint, 0);
    let mut token_program = token_program_account();
    let unauthorized = run_ix(
        Instruction::TopUpBackingBucket {
            domain: 1,
            amount: 100,
            expiry_slot: 10,
        },
        &mut [
            &mut attacker,
            &mut market,
            &mut attacker_src,
            &mut vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(unauthorized, &market, &before);

    let mut source = user_token_account(bucket_authority.key, mint, 100);
    run_ix(
        Instruction::TopUpBackingBucket {
            domain: 1,
            amount: 100,
            expiry_slot: 10,
        },
        &mut [
            &mut bucket_authority,
            &mut market,
            &mut source,
            &mut vault,
            &mut token_program,
        ],
    )
    .unwrap();

    let (cfg, group) = state::read_market(&market.data).unwrap();
    assert_eq!(
        cfg.backing_bucket_authority,
        bucket_authority.key.to_bytes()
    );
    assert_eq!(group.insurance, 0);
    assert_eq!(group.c_tot, 0);
    assert_eq!(group.vault, 100);
    assert_eq!(
        group.source_backing_buckets[1].status,
        BackingBucketStatusV16::Fresh
    );
    assert_eq!(group.source_backing_buckets[1].expiry_slot, 10);
    assert_eq!(
        group.source_backing_buckets[1].fresh_unliened_backing_num,
        100 * BOUND_SCALE
    );
    assert_eq!(
        group.source_credit[1].fresh_reserved_backing_num,
        100 * BOUND_SCALE
    );

    let before_bad_domain = market.data.clone();
    let mut source = user_token_account(bucket_authority.key, mint, 1);
    let bad_domain = run_ix(
        Instruction::TopUpBackingBucket {
            domain: 32,
            amount: 1,
            expiry_slot: 10,
        },
        &mut [
            &mut bucket_authority,
            &mut market,
            &mut source,
            &mut vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(bad_domain, &market, &before_bad_domain);

    let before_inactive_domain = market.data.clone();
    let mut source = user_token_account(bucket_authority.key, mint, 1);
    let inactive_domain = run_ix(
        Instruction::TopUpBackingBucket {
            domain: 2,
            amount: 1,
            expiry_slot: 10,
        },
        &mut [
            &mut bucket_authority,
            &mut market,
            &mut source,
            &mut vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(inactive_domain, &market, &before_inactive_domain);

    let before_bad_expiry = market.data.clone();
    let mut source = user_token_account(bucket_authority.key, mint, 1);
    let bad_expiry = run_ix(
        Instruction::TopUpBackingBucket {
            domain: 1,
            amount: 1,
            expiry_slot: 0,
        },
        &mut [
            &mut bucket_authority,
            &mut market,
            &mut source,
            &mut vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(bad_expiry, &market, &before_bad_expiry);

    let mut zero_new = TestAccount::new(Pubkey::default(), Pubkey::new_unique(), 0);
    run_ix(
        Instruction::UpdateAuthority {
            kind: processor::AUTHORITY_BACKING_BUCKET,
            new_pubkey: [0u8; 32],
        },
        &mut [&mut bucket_authority, &mut zero_new, &mut market],
    )
    .unwrap();
    let burned = market.data.clone();
    let mut source = user_token_account(bucket_authority.key, mint, 1);
    let after_burn = run_ix(
        Instruction::TopUpBackingBucket {
            domain: 1,
            amount: 1,
            expiry_slot: 10,
        },
        &mut [
            &mut bucket_authority,
            &mut market,
            &mut source,
            &mut vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(after_burn, &market, &burned);
}

#[test]
fn v16_wrapper_source_backed_positive_pnl_converts_from_backing_not_insurance() {
    let mut admin = signer();
    let mut market = market_account();
    let mint = init_market(&mut admin, &mut market);

    let mut source = user_token_account(admin.key, mint, 40);
    let mut vault = vault_token_account(&market, mint, 0);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::TopUpBackingBucket {
            domain: 1,
            amount: 40,
            expiry_slot: 10,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut source,
            &mut vault,
            &mut token_program,
        ],
    )
    .unwrap();

    let mut owner = signer();
    let mut portfolio = portfolio_account();
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        let mut account = state::read_portfolio(&portfolio.data).unwrap();
        group
            .add_account_source_positive_pnl_not_atomic(&mut account, 1, 40)
            .unwrap();
        state::write_market(&mut market.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut portfolio.data, &account).unwrap();
    }
    run_ix(
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
    )
    .unwrap();

    run_ix(
        Instruction::ConvertReleasedPnl { amount: 40 },
        &mut [&mut owner, &mut market, &mut portfolio],
    )
    .unwrap();

    let (_, group) = state::read_market(&market.data).unwrap();
    let account = state::read_portfolio(&portfolio.data).unwrap();
    assert_eq!(account.capital, 40);
    assert_eq!(account.pnl, 0);
    assert_eq!(group.c_tot, 40);
    assert_eq!(group.vault, 40);
    assert_eq!(
        group.insurance, 0,
        "counterparty backing must support the source claim without spending insurance"
    );
    assert_eq!(
        group.source_backing_buckets[1].consumed_liened_backing_num,
        40 * BOUND_SCALE
    );
    assert_eq!(group.source_credit[1].spent_backing_num, 40 * BOUND_SCALE);

    withdraw(&mut owner, &mut market, &mut portfolio, 40);
    let (_, group) = state::read_market(&market.data).unwrap();
    let account = state::read_portfolio(&portfolio.data).unwrap();
    assert_eq!(account.capital, 0);
    assert_eq!(group.c_tot, 0);
    assert_eq!(group.vault, 0);
}

#[test]
fn v16_wrapper_insurance_policy_deposit_only_leaves_fee_growth_behind() {
    let mut admin = signer();
    let mut market = market_account();

    let mint = init_market(&mut admin, &mut market);
    run_ix(
        Instruction::UpdateInsurancePolicy {
            max_bps: 10_000,
            deposits_only: 1,
            cooldown_slots: 0,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();

    let mut source = user_token_account(admin.key, mint, 100);
    let mut vault = vault_token_account(&market, mint, 150);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::TopUpInsurance { amount: 100 },
        &mut [
            &mut admin,
            &mut market,
            &mut source,
            &mut vault,
            &mut token_program,
        ],
    )
    .unwrap();

    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        assert_eq!(cfg.insurance_withdraw_deposit_remaining, 100);
        group.insurance += 50;
        group.vault += 50;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }

    let mut dest = user_token_account(admin.key, mint, 0);
    let mut vault_auth = vault_authority_account(&market);
    run_ix(
        Instruction::WithdrawInsuranceLimited { amount: 100 },
        &mut [
            &mut admin,
            &mut market,
            &mut dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    )
    .unwrap();

    let (cfg, group) = state::read_market(&market.data).unwrap();
    assert_eq!(cfg.insurance_withdraw_deposit_remaining, 0);
    assert_eq!(group.insurance, 50);
    assert_eq!(group.vault, 50);

    let before = market.data.clone();
    let fee_growth_withdraw = run_ix(
        Instruction::WithdrawInsuranceLimited { amount: 1 },
        &mut [
            &mut admin,
            &mut market,
            &mut dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(fee_growth_withdraw, &market, &before);
}

#[test]
fn v16_wrapper_insurance_policy_enforces_bps_cap_and_cooldown() {
    let mut admin = signer();
    let mut market = market_account();

    let mint = init_market(&mut admin, &mut market);
    run_ix(
        Instruction::UpdateInsurancePolicy {
            max_bps: 5_000,
            deposits_only: 0,
            cooldown_slots: 5,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    let mut source = user_token_account(admin.key, mint, 100);
    let mut vault = vault_token_account(&market, mint, 100);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::TopUpInsurance { amount: 100 },
        &mut [
            &mut admin,
            &mut market,
            &mut source,
            &mut vault,
            &mut token_program,
        ],
    )
    .unwrap();
    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        group.current_slot = 10;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }

    let mut dest = user_token_account(admin.key, mint, 0);
    let mut vault_auth = vault_authority_account(&market);
    let before = market.data.clone();
    let over_cap = run_ix(
        Instruction::WithdrawInsuranceLimited { amount: 51 },
        &mut [
            &mut admin,
            &mut market,
            &mut dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(over_cap, &market, &before);

    run_ix(
        Instruction::WithdrawInsuranceLimited { amount: 50 },
        &mut [
            &mut admin,
            &mut market,
            &mut dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    )
    .unwrap();
    let cooldown = run_ix(
        Instruction::WithdrawInsuranceLimited { amount: 1 },
        &mut [
            &mut admin,
            &mut market,
            &mut dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert!(
        cooldown.is_err(),
        "same-slot second withdrawal must hit cooldown"
    );
}

#[test]
fn v16_wrapper_withdraw_insurance_requires_operator_and_healthy_market() {
    let mut admin = signer();
    let mut market = market_account();
    let mut attacker = signer();

    let mint = init_market(&mut admin, &mut market);
    let mut source = user_token_account(admin.key, mint, 100);
    let mut vault = vault_token_account(&market, mint, 100);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::TopUpInsurance { amount: 100 },
        &mut [
            &mut admin,
            &mut market,
            &mut source,
            &mut vault,
            &mut token_program,
        ],
    )
    .unwrap();

    let mut attacker_dest = user_token_account(attacker.key, mint, 0);
    let mut vault_auth = vault_authority_account(&market);
    let topped_up = market.data.clone();
    let unauthorized = run_ix(
        Instruction::WithdrawInsuranceLimited { amount: 1 },
        &mut [
            &mut attacker,
            &mut market,
            &mut attacker_dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(unauthorized, &market, &topped_up);

    let zero = run_ix(
        Instruction::WithdrawInsuranceLimited { amount: 0 },
        &mut [
            &mut admin,
            &mut market,
            &mut attacker_dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(zero, &market, &topped_up);

    let stress_cases: &[fn(&mut MarketGroupV16)] = &[
        |group| group.bankruptcy_hlock_active = true,
        |group| group.threshold_stress_active = true,
        |group| group.loss_stale_active = true,
        |group| group.recovery_reason = Some(PermissionlessRecoveryReasonV16::BelowProgressFloor),
    ];
    for set_stress in stress_cases {
        let (cfg, mut group) = state::read_market(&topped_up).unwrap();
        set_stress(&mut group);
        state::write_market(&mut market.data, &cfg, &group).unwrap();
        let stressed = market.data.clone();
        let stressed_withdraw = run_ix(
            Instruction::WithdrawInsuranceLimited { amount: 1 },
            &mut [
                &mut admin,
                &mut market,
                &mut attacker_dest,
                &mut vault,
                &mut vault_auth,
                &mut token_program,
            ],
        );
        assert_err_and_market_unchanged(stressed_withdraw, &market, &stressed);
    }
    market.data.copy_from_slice(&topped_up);
    let mut admin_dest = user_token_account(admin.key, mint, 0);
    run_ix(
        Instruction::WithdrawInsuranceLimited { amount: 40 },
        &mut [
            &mut admin,
            &mut market,
            &mut admin_dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    )
    .unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(group.insurance, 60);
    assert_eq!(group.vault, 60);

    let overdraw = run_ix(
        Instruction::WithdrawInsuranceLimited { amount: 61 },
        &mut [
            &mut admin,
            &mut market,
            &mut admin_dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert!(overdraw.is_err());

    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        group.c_tot = 60;
        group.vault = 100;
        group.insurance = 60;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }
    let corrupt = market.data.clone();
    let senior_invariant_reject = run_ix(
        Instruction::WithdrawInsuranceLimited { amount: 1 },
        &mut [
            &mut admin,
            &mut market,
            &mut admin_dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(senior_invariant_reject, &market, &corrupt);
}

#[test]
fn v16_wrapper_withdraw_insurance_uses_rotated_operator_and_terminal_drain() {
    let mut admin = signer().writable();
    let mut market = market_account();
    let mut operator = signer();

    let mint = init_market(&mut admin, &mut market);
    run_ix(
        Instruction::UpdateAuthority {
            kind: processor::AUTHORITY_INSURANCE_OPERATOR,
            new_pubkey: operator.key.to_bytes(),
        },
        &mut [&mut admin, &mut operator, &mut market],
    )
    .unwrap();
    let mut source = user_token_account(admin.key, mint, 50);
    let mut vault = vault_token_account(&market, mint, 50);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::TopUpInsurance { amount: 50 },
        &mut [
            &mut admin,
            &mut market,
            &mut source,
            &mut vault,
            &mut token_program,
        ],
    )
    .unwrap();
    run_ix(Instruction::ResolveMarket, &mut [&mut admin, &mut market]).unwrap();

    let mut vault_auth = vault_authority_account(&market);
    let mut admin_dest = user_token_account(admin.key, mint, 0);
    let resolved = market.data.clone();
    let old_operator = run_ix(
        Instruction::WithdrawInsuranceLimited { amount: 1 },
        &mut [
            &mut admin,
            &mut market,
            &mut admin_dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(old_operator, &market, &resolved);

    let mut operator_dest = user_token_account(operator.key, mint, 0);
    run_ix(
        Instruction::WithdrawInsuranceLimited { amount: 50 },
        &mut [
            &mut operator,
            &mut market,
            &mut operator_dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    )
    .unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(group.insurance, 0);
    assert_eq!(group.vault, 0);

    let mut close_dest = user_token_account(admin.key, mint, 0);
    run_ix(
        Instruction::CloseSlab,
        &mut [
            &mut admin,
            &mut market,
            &mut vault,
            &mut vault_auth,
            &mut close_dest,
            &mut token_program,
        ],
    )
    .unwrap();
    assert!(market.data.iter().all(|b| *b == 0));
}

#[test]
fn v16_wrapper_withdraw_insurance_resolved_requires_all_portfolios_closed() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    let mint = init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 10);
    let mut source = user_token_account(admin.key, mint, 10);
    let mut vault = vault_token_account(&market, mint, 20);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::TopUpInsurance { amount: 10 },
        &mut [
            &mut admin,
            &mut market,
            &mut source,
            &mut vault,
            &mut token_program,
        ],
    )
    .unwrap();
    run_ix(Instruction::ResolveMarket, &mut [&mut admin, &mut market]).unwrap();

    let mut dest = user_token_account(admin.key, mint, 0);
    let mut vault_auth = vault_authority_account(&market);
    let resolved_with_claims = market.data.clone();
    let rejected = run_ix(
        Instruction::WithdrawInsuranceLimited { amount: 1 },
        &mut [
            &mut admin,
            &mut market,
            &mut dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(rejected, &market, &resolved_with_claims);
}

#[test]
fn v16_wrapper_update_authority_rotates_admin_with_dual_signature() {
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
    assert_eq!(group.mode, MarketModeV16::Resolved);
}

#[test]
fn v16_wrapper_update_authority_rotates_insurance_keys_and_supports_operator_burn() {
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
fn v16_wrapper_update_authority_rejects_unsupported_kind_and_live_admin_burn() {
    let mut admin = signer();
    let mut market = market_account();
    let mut new_key = signer();

    init_market(&mut admin, &mut market);

    run_ix(
        Instruction::UpdateAuthority {
            kind: processor::AUTHORITY_HYPERP_MARK,
            new_pubkey: new_key.key.to_bytes(),
        },
        &mut [&mut admin, &mut new_key, &mut market],
    )
    .unwrap();
    let (cfg, _) = state::read_market(&market.data).unwrap();
    assert_eq!(
        cfg.hyperp_mark_authority,
        new_key.key.to_bytes(),
        "the market-wide Hyperp mark authority can be rotated before a specific asset enters Hyperp mode"
    );
    let initialized = market.data.clone();

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
fn v16_wrapper_configure_hyperp_mark_pushes_and_cranks_from_internal_mark() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_price_move_bps_per_slot,
                ..
            } = ix
            {
                *max_price_move_bps_per_slot = 1_000;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();

    run_ix(
        Instruction::ConfigureHyperpMark {
            asset_index: 0,
            now_slot: 5,
            initial_mark_e6: 100,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    let (cfg, group) = state::read_market(&market.data).unwrap();
    assert_eq!(
        cfg.oracle_mode,
        percolator_prog::constants::ORACLE_MODE_HYPERP
    );
    assert_eq!(cfg.mark_ewma_e6, 100);
    assert_eq!(cfg.mark_ewma_last_slot, 5);
    assert_eq!(cfg.last_good_oracle_slot, 5);
    assert_eq!(group.assets[0].effective_price, 100);

    let mut new_mark_authority = signer();
    run_ix(
        Instruction::UpdateAuthority {
            kind: processor::AUTHORITY_HYPERP_MARK,
            new_pubkey: new_mark_authority.key.to_bytes(),
        },
        &mut [&mut admin, &mut new_mark_authority, &mut market],
    )
    .unwrap();

    let before_bad_push = market.data.clone();
    let mut wrong_authority = signer();
    let bad_push = run_ix(
        Instruction::PushHyperpMark {
            asset_index: 0,
            now_slot: 10,
            mark_e6: 120,
        },
        &mut [&mut wrong_authority, &mut market],
    );
    assert_err_and_market_unchanged(bad_push, &market, &before_bad_push);

    run_ix(
        Instruction::PushHyperpMark {
            asset_index: 0,
            now_slot: 10,
            mark_e6: 120,
        },
        &mut [&mut new_mark_authority, &mut market],
    )
    .unwrap();
    let (cfg, _) = state::read_market(&market.data).unwrap();
    assert_eq!(
        cfg.mark_ewma_e6, 116,
        "full-weight Hyperp push advances the configured EWMA mark at the authenticated slot"
    );
    assert_eq!(cfg.mark_ewma_last_slot, 10);
    assert_eq!(cfg.last_good_oracle_slot, 10);

    let mut caller = signer();
    let mut portfolio = portfolio_account();
    init_portfolio(&mut caller, &mut market, &mut portfolio);
    run_ix(
        Instruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 6,
            effective_price: 999,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut caller, &mut market, &mut portfolio],
    )
    .unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(
        group.assets[0].effective_price, 116,
        "Hyperp crank ignores caller-selected price and walks to the internal mark"
    );
}

#[test]
fn v16_wrapper_configure_hyperp_mark_clears_prior_hybrid_oracle_metadata() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    run_ix(
        default_init_market_ix(),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();

    let feeds = [[1u8; 32], [2u8; 32], [3u8; 32]];
    let mut leg0 = pyth_account(&feeds[0], 200, 0, 0, 1_000);
    let mut leg1 = pyth_account(&feeds[1], 2, 0, 0, 1_000);
    let mut leg2 = pyth_account(&feeds[2], 4, 0, 0, 1_000);
    run_ix(
        Instruction::ConfigureHybridOracle {
            asset_index: 0,
            now_slot: 10,
            now_unix_ts: 1_000,
            oracle_leg_count: 3,
            oracle_leg_flags: ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3,
            max_staleness_secs: 60,
            hybrid_soft_stale_slots: 3,
            mark_ewma_halflife_slots: 11,
            mark_min_fee: 7,
            invert: 1,
            unit_scale: 6,
            conf_filter_bps: 500,
            oracle_leg_feeds: feeds,
        },
        &mut [&mut admin, &mut market, &mut leg0, &mut leg1, &mut leg2],
    )
    .unwrap();

    let (hybrid_cfg, _) = state::read_market(&market.data).unwrap();
    assert_eq!(hybrid_cfg.oracle_mode, ORACLE_MODE_HYBRID_AFTER_HOURS);
    assert_eq!(hybrid_cfg.invert, 1);
    assert_eq!(hybrid_cfg.unit_scale, 6);
    assert_eq!(hybrid_cfg.conf_filter_bps, 500);
    assert_eq!(hybrid_cfg.oracle_leg_count, 3);

    run_ix(
        Instruction::ConfigureHyperpMark {
            asset_index: 0,
            now_slot: 11,
            initial_mark_e6: 123,
            mark_ewma_halflife_slots: 5,
            mark_min_fee: 9,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();

    let (cfg, group) = state::read_market(&market.data).unwrap();
    assert_eq!(cfg.oracle_mode, ORACLE_MODE_HYPERP);
    assert_eq!(cfg.oracle_leg_count, 0);
    assert_eq!(cfg.oracle_leg_flags, 0);
    assert_eq!(cfg.oracle_leg_feeds, [[0u8; 32]; ORACLE_LEG_CAP]);
    assert_eq!(cfg.oracle_leg_prices_e6, [0u64; ORACLE_LEG_CAP]);
    assert_eq!(cfg.oracle_leg_publish_times, [0i64; ORACLE_LEG_CAP]);
    assert_eq!(cfg.max_staleness_secs, 0);
    assert_eq!(cfg.hybrid_soft_stale_slots, 0);
    assert_eq!(cfg.invert, 0);
    assert_eq!(cfg.unit_scale, 0);
    assert_eq!(cfg.conf_filter_bps, 0);
    assert_eq!(cfg.mark_ewma_e6, 123);
    assert_eq!(cfg.mark_ewma_halflife_slots, 5);
    assert_eq!(cfg.mark_min_fee, 9);
    assert_eq!(group.assets[0].effective_price, 123);
}

#[test]
fn v16_wrapper_hyperp_trade_updates_mark_and_charges_dynamic_fee_without_oracle_tail() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_trading_fee_bps,
                trade_fee_base_bps,
                max_price_move_bps_per_slot,
                ..
            } = ix
            {
                *max_trading_fee_bps = 10_000;
                *trade_fee_base_bps = 1;
                *max_price_move_bps_per_slot = 500;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();
    run_ix(
        Instruction::ConfigureHyperpMark {
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: 100_000,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();

    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 10_000_000);
    deposit(
        &mut short_owner,
        &mut market,
        &mut short_account,
        10_000_000,
    );
    {
        let (mut cfg, mut group) = state::read_market(&market.data).unwrap();
        cfg.mark_ewma_last_slot = 0;
        group.current_slot = 10;
        group.slot_last = 10;
        group.assets[0].slot_last = 10;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }

    let (before_cfg, before_group) = state::read_market(&market.data).unwrap();
    let size_q = 10 * POS_SCALE;
    let exec_price = before_group.assets[0].effective_price * 150 / 100;
    let base_only_fee = two_sided_fee(size_q, exec_price, 1);
    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: size_q as i128,
            exec_price,
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

    let (after_cfg, after_group) = state::read_market(&market.data).unwrap();
    assert!(
        after_group.insurance > before_group.insurance + base_only_fee,
        "direct Hyperp trades pay dynamic mark-movement surcharge without oracle accounts"
    );
    assert!(
        after_cfg.mark_ewma_e6 > before_cfg.mark_ewma_e6,
        "direct Hyperp trades move the fallback EWMA mark"
    );
    assert_eq!(
        after_group.assets[0].effective_price, before_group.assets[0].effective_price,
        "execution price flexibility must not rewrite the accepted Hyperp index"
    );
}

#[test]
fn v16_wrapper_permissionless_resolve_policy_is_admin_gated_and_enables_admin_burn() {
    let mut admin = signer();
    let mut attacker = signer();
    let mut market = market_account();
    let mut new_key = signer();

    init_market(&mut admin, &mut market);
    let initialized = market.data.clone();

    let attacker_update = run_ix(
        Instruction::ConfigurePermissionlessResolve {
            stale_slots: 5,
            force_close_delay_slots: 1,
        },
        &mut [&mut attacker, &mut market],
    );
    assert_err_and_market_unchanged(attacker_update, &market, &initialized);

    for (stale_slots, force_close_delay_slots) in [(0, 1), (1, 0)] {
        let before = market.data.clone();
        let rejected = run_ix(
            Instruction::ConfigurePermissionlessResolve {
                stale_slots,
                force_close_delay_slots,
            },
            &mut [&mut admin, &mut market],
        );
        assert_err_and_market_unchanged(rejected, &market, &before);
    }

    run_ix(
        Instruction::ConfigurePermissionlessResolve {
            stale_slots: 5,
            force_close_delay_slots: 1,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    let (cfg, group) = state::read_market(&market.data).unwrap();
    assert_eq!(group.mode, MarketModeV16::Live);
    assert_eq!(cfg.permissionless_resolve_stale_slots, 5);
    assert_eq!(cfg.force_close_delay_slots, 1);

    run_ix(
        Instruction::UpdateAuthority {
            kind: processor::AUTHORITY_ADMIN,
            new_pubkey: [0u8; 32],
        },
        &mut [&mut admin, &mut new_key, &mut market],
    )
    .unwrap();
    let (cfg, group) = state::read_market(&market.data).unwrap();
    assert_eq!(group.mode, MarketModeV16::Live);
    assert_eq!(cfg.admin, [0u8; 32]);
}

#[test]
fn v16_wrapper_update_authority_allows_chained_admin_rotation_without_old_key_reuse() {
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
fn v16_wrapper_close_portfolio_rejects_non_empty_and_closes_empty() {
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
fn v16_wrapper_close_slab_requires_admin_resolved_empty_market() {
    let mut admin = signer().writable();
    let mut market = market_account();
    let mut attacker = signer().writable();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    let mint = init_market(&mut admin, &mut market);
    let mut vault = vault_token_account(&market, mint, 0);
    let mut vault_auth = vault_authority_account(&market);
    let mut dest_token = user_token_account(admin.key, mint, 0);
    let mut token_program = token_program_account();

    let live_before = market.data.clone();
    let live_close = run_ix(
        Instruction::CloseSlab,
        &mut [
            &mut admin,
            &mut market,
            &mut vault,
            &mut vault_auth,
            &mut dest_token,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(live_close, &market, &live_before);

    init_portfolio(&mut owner, &mut market, &mut portfolio);
    run_ix(Instruction::ResolveMarket, &mut [&mut admin, &mut market]).unwrap();
    let resolved_before = market.data.clone();
    let non_admin = run_ix(
        Instruction::CloseSlab,
        &mut [
            &mut attacker,
            &mut market,
            &mut vault,
            &mut vault_auth,
            &mut dest_token,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(non_admin, &market, &resolved_before);

    let with_portfolio = market.data.clone();
    let nonempty_count = run_ix(
        Instruction::CloseSlab,
        &mut [
            &mut admin,
            &mut market,
            &mut vault,
            &mut vault_auth,
            &mut dest_token,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(nonempty_count, &market, &with_portfolio);

    run_ix(
        Instruction::ClosePortfolio,
        &mut [&mut owner, &mut market, &mut portfolio],
    )
    .unwrap();
    let market_lamports = market.lamports;
    let admin_lamports = admin.lamports;
    run_ix(
        Instruction::CloseSlab,
        &mut [
            &mut admin,
            &mut market,
            &mut vault,
            &mut vault_auth,
            &mut dest_token,
            &mut token_program,
        ],
    )
    .unwrap();
    assert_eq!(market.lamports, 0);
    assert_eq!(admin.lamports, admin_lamports + market_lamports);
    assert!(
        market.data.iter().all(|b| *b == 0),
        "closed market account should be zeroed"
    );
}

#[test]
fn v16_wrapper_close_slab_rejects_burned_admin_zero_key() {
    let mut admin = signer().writable();
    let mut market = market_account();

    let mint = init_market(&mut admin, &mut market);
    run_ix(Instruction::ResolveMarket, &mut [&mut admin, &mut market]).unwrap();
    {
        let (mut cfg, group) = state::read_market(&market.data).unwrap();
        cfg.admin = [0u8; 32];
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }

    let mut zero_admin = TestAccount::new(Pubkey::default(), Pubkey::new_unique(), 0)
        .signer()
        .writable();
    let mut vault = vault_token_account(&market, mint, 0);
    let mut vault_auth = vault_authority_account(&market);
    let mut dest_token = user_token_account(zero_admin.key, mint, 0);
    let mut token_program = token_program_account();

    let burned_admin_market = market.data.clone();
    let rejected = run_ix(
        Instruction::CloseSlab,
        &mut [
            &mut zero_admin,
            &mut market,
            &mut vault,
            &mut vault_auth,
            &mut dest_token,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(rejected, &market, &burned_admin_market);
}

#[test]
fn v16_wrapper_close_slab_rejects_nonzero_engine_vault_or_insurance() {
    let mut admin = signer().writable();
    let mut market = market_account();

    let mint = init_market(&mut admin, &mut market);
    let mut source = user_token_account(admin.key, mint, 10);
    let mut vault = vault_token_account(&market, mint, 10);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::TopUpInsurance { amount: 10 },
        &mut [
            &mut admin,
            &mut market,
            &mut source,
            &mut vault,
            &mut token_program,
        ],
    )
    .unwrap();
    run_ix(Instruction::ResolveMarket, &mut [&mut admin, &mut market]).unwrap();

    let mut vault_auth = vault_authority_account(&market);
    let mut dest_token = user_token_account(admin.key, mint, 0);
    let before = market.data.clone();
    let rejected = run_ix(
        Instruction::CloseSlab,
        &mut [
            &mut admin,
            &mut market,
            &mut vault,
            &mut vault_auth,
            &mut dest_token,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(rejected, &market, &before);
}

#[test]
fn v16_wrapper_close_slab_rejects_uninitialized_market_without_rent_drain() {
    let mut admin = signer().writable();
    let mut market = market_account();
    let mint = Pubkey::new_unique();
    let mut vault = vault_token_account(&market, mint, 0);
    let mut vault_auth = vault_authority_account(&market);
    let mut dest_token = user_token_account(admin.key, mint, 0);
    let mut token_program = token_program_account();

    let before_market = market.data.clone();
    let before_lamports = (admin.lamports, market.lamports);
    let rejected = run_ix(
        Instruction::CloseSlab,
        &mut [
            &mut admin,
            &mut market,
            &mut vault,
            &mut vault_auth,
            &mut dest_token,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(rejected, &market, &before_market);
    assert_eq!((admin.lamports, market.lamports), before_lamports);
}

#[test]
fn v16_wrapper_deposit_withdraw_roundtrip_preserves_accounting() {
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
fn v16_wrapper_multiple_portfolios_same_owner_stay_isolated_and_totals_match() {
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
fn v16_wrapper_deposit_rejects_without_token_accounts() {
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
fn v16_wrapper_deposit_rejects_wrong_mint_and_insufficient_source_balance() {
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
fn v16_wrapper_deposit_rejects_wrong_owner_and_bad_token_program() {
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
fn v16_wrapper_vault_accounts_reject_delegate_and_close_authority() {
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
fn v16_wrapper_token_accounts_must_be_initialized_for_custody_paths() {
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

    let mut admin_source = user_token_account(admin.key, mint, 1_000);
    let frozen_backing_vault = run_ix(
        Instruction::TopUpBackingBucket {
            domain: 1,
            amount: 1,
            expiry_slot: 1,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut admin_source,
            &mut frozen_vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(frozen_backing_vault, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);
}

#[test]
fn v16_wrapper_spl_u64_amount_limit_rejects_before_mutation() {
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

    let mut admin_source = user_token_account(admin.key, mint, u64::MAX);
    let backing_too_large = run_ix(
        Instruction::TopUpBackingBucket {
            domain: 1,
            amount: too_large,
            expiry_slot: 1,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut admin_source,
            &mut vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(backing_too_large, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);
}

#[test]
fn v16_wrapper_zero_amount_custody_paths_are_noop_without_state_drift() {
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

    let mut admin_source = user_token_account(admin.key, mint, 0);
    run_ix(
        Instruction::TopUpBackingBucket {
            domain: 1,
            amount: 0,
            expiry_slot: 1,
        },
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
fn v16_wrapper_withdraw_rejects_wrong_vault_authority_and_wrong_destination_mint() {
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
fn v16_wrapper_withdraw_rejects_wrong_owner_without_mutation() {
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
fn v16_wrapper_withdraw_rejects_over_capital_and_insufficient_vault_without_mutation() {
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
fn v16_wrapper_close_portfolio_rejects_wrong_owner_without_mutation() {
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
fn v16_wrapper_cross_market_portfolio_provenance_is_fail_closed() {
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
fn v16_wrapper_same_owner_can_trade_independent_positions_across_markets() {
    let mut admin_a = signer();
    let mut admin_b = signer();
    let mut market_a = market_account();
    let mut market_b = market_account();
    let mut owner = signer();
    let mut counterparty_owner_a = signer();
    let mut counterparty_owner_b = signer();
    let mut owner_account_a = portfolio_account();
    let mut owner_account_b = portfolio_account();
    let mut counterparty_a = portfolio_account();
    let mut counterparty_b = portfolio_account();

    init_market(&mut admin_a, &mut market_a);
    init_market(&mut admin_b, &mut market_b);
    init_portfolio(&mut owner, &mut market_a, &mut owner_account_a);
    init_portfolio(&mut owner, &mut market_b, &mut owner_account_b);
    init_portfolio(
        &mut counterparty_owner_a,
        &mut market_a,
        &mut counterparty_a,
    );
    init_portfolio(
        &mut counterparty_owner_b,
        &mut market_b,
        &mut counterparty_b,
    );
    deposit(&mut owner, &mut market_a, &mut owner_account_a, 1_000_000);
    deposit(&mut owner, &mut market_b, &mut owner_account_b, 2_000_000);
    deposit(
        &mut counterparty_owner_a,
        &mut market_a,
        &mut counterparty_a,
        1_000_000,
    );
    deposit(
        &mut counterparty_owner_b,
        &mut market_b,
        &mut counterparty_b,
        2_000_000,
    );

    let market_b_before_a_trade = market_b.data.clone();
    let owner_b_before_a_trade = owner_account_b.data.clone();
    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: POS_SCALE as i128,
            exec_price: 100,
            fee_bps: 0,
        },
        &mut [
            &mut owner,
            &mut counterparty_owner_a,
            &mut market_a,
            &mut owner_account_a,
            &mut counterparty_a,
        ],
    )
    .unwrap();
    assert_eq!(
        market_b.data, market_b_before_a_trade,
        "valid trade in market A must not mutate market B"
    );
    assert_eq!(
        owner_account_b.data, owner_b_before_a_trade,
        "valid trade in market A must not mutate owner's market-B portfolio"
    );

    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: -(2 * POS_SCALE as i128),
            exec_price: 100,
            fee_bps: 0,
        },
        &mut [
            &mut owner,
            &mut counterparty_owner_b,
            &mut market_b,
            &mut owner_account_b,
            &mut counterparty_b,
        ],
    )
    .unwrap();

    let (_, group_a) = state::read_market(&market_a.data).unwrap();
    let (_, group_b) = state::read_market(&market_b.data).unwrap();
    let owner_a = state::read_portfolio(&owner_account_a.data).unwrap();
    let owner_b = state::read_portfolio(&owner_account_b.data).unwrap();
    assert_eq!(
        owner_a.provenance_header.market_group_id,
        market_a.key.to_bytes()
    );
    assert_eq!(
        owner_b.provenance_header.market_group_id,
        market_b.key.to_bytes()
    );
    assert_eq!(owner_a.owner, owner.key.to_bytes());
    assert_eq!(owner_b.owner, owner.key.to_bytes());
    assert_eq!(owner_a.legs[0].basis_pos_q, POS_SCALE as i128);
    assert_eq!(owner_b.legs[0].basis_pos_q, -(2 * POS_SCALE as i128));
    assert_eq!(group_a.assets[0].oi_eff_long_q, POS_SCALE);
    assert_eq!(group_a.assets[0].oi_eff_short_q, POS_SCALE);
    assert_eq!(group_b.assets[0].oi_eff_long_q, 2 * POS_SCALE);
    assert_eq!(group_b.assets[0].oi_eff_short_q, 2 * POS_SCALE);
}

#[test]
fn v16_wrapper_account_kind_confusion_is_rejected_before_mutation() {
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
fn v16_wrapper_portfolio_key_mismatch_and_self_trade_are_rejected() {
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
fn v16_wrapper_tradenocpi_negative_size_flips_long_short_roles() {
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
fn v16_wrapper_tradenocpi_accepts_consented_wide_exec_price_without_moving_index() {
    let mut admin = signer();
    let mut market = market_account();
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 10_000_000);
    deposit(
        &mut short_owner,
        &mut market,
        &mut short_account,
        10_000_000,
    );

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
    assert!(!percolator::active_bitmap_is_empty(long.active_bitmap));
    assert!(!percolator::active_bitmap_is_empty(short.active_bitmap));
    assert_eq!(long.legs[0].basis_pos_q, (10 * POS_SCALE) as i128);
    assert_eq!(short.legs[0].basis_pos_q, -((10 * POS_SCALE) as i128));
    assert_eq!(
        group.insurance, 30,
        "notional=1500 and 100 bps charges 15 to each side"
    );
}

#[test]
fn v16_wrapper_configure_hybrid_oracle_composes_toto_sol_cross_and_rejects_rollbacks() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_price_move_bps_per_slot,
                ..
            } = ix
            {
                *max_price_move_bps_per_slot = 500;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();

    let feeds = [[0x11u8; 32], [0x22u8; 32], [0x33u8; 32]];
    let mut toto_jpy = pyth_account(&feeds[0], 4_000_000_000, -6, 1, 100);
    let mut usd_jpy = pyth_account(&feeds[1], 150_000_000, -6, 1, 101);
    let mut sol_usd = pyth_account(&feeds[2], 200_000_000, -6, 1, 102);
    configure_three_leg_hybrid(
        &mut admin,
        &mut market,
        feeds,
        &mut toto_jpy,
        &mut usd_jpy,
        &mut sol_usd,
        5,
        102,
        10,
        0,
    );

    let (cfg, group) = state::read_market(&market.data).unwrap();
    assert_eq!(cfg.oracle_leg_count, 3);
    assert_eq!(
        cfg.oracle_leg_prices_e6,
        [4_000_000_000, 150_000_000, 200_000_000]
    );
    assert_eq!(cfg.mark_ewma_e6, 133_333);
    assert_eq!(cfg.oracle_target_price_e6, 133_333);
    assert_eq!(group.assets[0].effective_price, 133_333);

    let mut keeper = signer();
    let mut portfolio = portfolio_account();
    init_portfolio(&mut keeper, &mut market, &mut portfolio);
    usd_jpy.data = make_pyth(&feeds[1], 150_000_000, -6, 1, 99);
    let before = market.data.clone();
    let rollback = run_ix(
        Instruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 6,
            effective_price: 133_333,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [
            &mut keeper,
            &mut market,
            &mut portfolio,
            &mut toto_jpy,
            &mut usd_jpy,
            &mut sol_usd,
        ],
    );
    assert_err_and_market_unchanged(rollback, &market, &before);
}

#[test]
fn v16_wrapper_non_base_asset_profile_converts_stoxx_eur_to_base_sol() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_price_move_bps_per_slot,
                initial_price,
                ..
            } = ix
            {
                *initial_price = 1_000_000;
                *max_price_move_bps_per_slot = 500;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();
    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        1,
        1,
        1_000_000,
    )
    .unwrap();

    let feeds = [[0x44u8; 32], [0x45u8; 32], [0x46u8; 32]];
    let mut stoxx_eur = pyth_account(&feeds[0], 4_500_000_000, -6, 1, 100);
    let mut eur_usd = pyth_account(&feeds[1], 1_100_000, -6, 1, 101);
    let mut sol_usd = pyth_account(&feeds[2], 200_000_000, -6, 1, 102);
    run_ix(
        Instruction::ConfigureHybridOracle {
            asset_index: 1,
            now_slot: 5,
            now_unix_ts: 102,
            oracle_leg_count: 3,
            oracle_leg_flags: ORACLE_LEG_FLAG_DIVIDE_LEG3,
            max_staleness_secs: 60,
            hybrid_soft_stale_slots: 10,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            invert: 0,
            unit_scale: 0,
            conf_filter_bps: 500,
            oracle_leg_feeds: feeds,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut stoxx_eur,
            &mut eur_usd,
            &mut sol_usd,
        ],
    )
    .unwrap();

    let (cfg, group) = state::read_market(&market.data).unwrap();
    let base_profile = state::read_asset_oracle_profile(&market.data, 0).unwrap();
    let stoxx_profile = state::read_asset_oracle_profile(&market.data, 1).unwrap();
    assert_eq!(
        cfg.oracle_mode, ORACLE_MODE_MANUAL,
        "config-level oracle fields are only the asset-0 mirror; non-base profiles are per-asset"
    );
    assert_eq!(base_profile.oracle_mode, ORACLE_MODE_MANUAL);
    assert_eq!(group.assets[0].effective_price, 1_000_000);
    assert_eq!(stoxx_profile.oracle_leg_count, 3);
    assert_eq!(stoxx_profile.oracle_leg_flags, ORACLE_LEG_FLAG_DIVIDE_LEG3);
    assert_eq!(
        stoxx_profile.oracle_leg_prices_e6,
        [4_500_000_000, 1_100_000, 200_000_000]
    );
    assert_eq!(stoxx_profile.oracle_target_price_e6, 24_750_000);
    assert_eq!(stoxx_profile.mark_ewma_e6, 24_750_000);
    assert_eq!(group.assets[1].effective_price, 24_750_000);

    let mut keeper = signer();
    let mut portfolio = portfolio_account();
    init_portfolio(&mut keeper, &mut market, &mut portfolio);
    stoxx_eur.data = make_pyth(&feeds[0], 4_600_000_000, -6, 1, 103);
    eur_usd.data = make_pyth(&feeds[1], 1_200_000, -6, 1, 104);
    sol_usd.data = make_pyth(&feeds[2], 200_000_000, -6, 1, 105);
    run_ix(
        Instruction::PermissionlessCrank {
            action: 0,
            asset_index: 1,
            now_slot: 10,
            effective_price: 1,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [
            &mut keeper,
            &mut market,
            &mut portfolio,
            &mut stoxx_eur,
            &mut eur_usd,
            &mut sol_usd,
        ],
    )
    .unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    let stoxx_profile = state::read_asset_oracle_profile(&market.data, 1).unwrap();
    assert_eq!(group.assets[0].effective_price, 1_000_000);
    assert_eq!(stoxx_profile.oracle_target_price_e6, 27_600_000);
    assert_eq!(stoxx_profile.mark_ewma_e6, 27_600_000);
    assert_eq!(group.assets[1].effective_price, 27_600_000);
}

#[test]
fn v16_wrapper_price_managed_asset_above_portfolio_limit_still_updates_mark_after_trade() {
    let mut admin = signer();
    let mut market = market_account();
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut cranker = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();
    let mut crank_account = portfolio_account();

    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_portfolio_assets,
                max_trading_fee_bps,
                max_price_move_bps_per_slot,
                ..
            } = ix
            {
                *max_portfolio_assets = 14;
                *max_trading_fee_bps = 10_000;
                *max_price_move_bps_per_slot = 10_000;
            }
        }),
    );
    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        14,
        1,
        100,
    )
    .unwrap();
    run_ix(
        Instruction::ConfigureHyperpMark {
            asset_index: 14,
            now_slot: 1,
            initial_mark_e6: 100,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    init_portfolio(&mut cranker, &mut market, &mut crank_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 1_000_000);
    deposit(&mut short_owner, &mut market, &mut short_account, 1_000_000);

    run_ix(
        Instruction::PermissionlessCrank {
            action: 0,
            asset_index: 14,
            now_slot: 2,
            effective_price: 999,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut cranker, &mut market, &mut crank_account],
    )
    .unwrap();
    let before_profile = state::read_asset_oracle_profile(&market.data, 14).unwrap();
    assert_eq!(before_profile.mark_ewma_e6, 100);
    assert_eq!(before_profile.mark_ewma_last_slot, 1);

    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 14,
            size_q: POS_SCALE as i128,
            exec_price: 200,
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

    let base_profile = state::read_asset_oracle_profile(&market.data, 0).unwrap();
    let after_profile = state::read_asset_oracle_profile(&market.data, 14).unwrap();
    assert_eq!(base_profile.oracle_mode, ORACLE_MODE_MANUAL);
    assert_eq!(
        after_profile.mark_ewma_e6, 150,
        "valid market slots above the per-portfolio active-position cap still have their own mark"
    );
    assert_eq!(after_profile.mark_ewma_last_slot, 2);
}

#[test]
fn v16_wrapper_hybrid_oracle_accepts_switchboard_and_chainlink_legs() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    run_ix(
        default_init_market_ix(),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();

    let switchboard_key = Pubkey::new_unique();
    let chainlink_key = Pubkey::new_unique();
    let mut switchboard = switchboard_account(switchboard_key, 4_000_000_000, 10_000, 100);
    let mut chainlink = chainlink_account(chainlink_key, 150_000_000_000, 8, 100);
    run_ix(
        Instruction::ConfigureHybridOracle {
            asset_index: 0,
            now_slot: 1,
            now_unix_ts: 100,
            oracle_leg_count: 2,
            oracle_leg_flags: ORACLE_LEG_FLAG_DIVIDE_LEG2,
            max_staleness_secs: 60,
            hybrid_soft_stale_slots: 10,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            invert: 0,
            unit_scale: 0,
            conf_filter_bps: 500,
            oracle_leg_feeds: [
                switchboard_key.to_bytes(),
                chainlink_key.to_bytes(),
                [0u8; 32],
            ],
        },
        &mut [&mut admin, &mut market, &mut switchboard, &mut chainlink],
    )
    .unwrap();

    let (cfg, group) = state::read_market(&market.data).unwrap();
    assert_eq!(cfg.oracle_leg_count, 2);
    assert_eq!(cfg.oracle_leg_prices_e6, [4_000_000_000, 1_500_000_000, 0]);
    assert_eq!(cfg.oracle_target_price_e6, 2_666_666);
    assert_eq!(group.assets[0].effective_price, 2_666_666);
}

#[test]
fn v16_wrapper_switchboard_oracle_rejects_wrong_key_stale_and_conf_wide() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    run_ix(
        default_init_market_ix(),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();

    let feed_key = Pubkey::new_unique();
    let before = market.data.clone();
    let mut wrong_key = switchboard_account(Pubkey::new_unique(), 100_000_000, 0, 100);
    let wrong_key_result = run_ix(
        Instruction::ConfigureHybridOracle {
            asset_index: 0,
            now_slot: 1,
            now_unix_ts: 100,
            oracle_leg_count: 1,
            oracle_leg_flags: 0,
            max_staleness_secs: 60,
            hybrid_soft_stale_slots: 10,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            invert: 0,
            unit_scale: 0,
            conf_filter_bps: 500,
            oracle_leg_feeds: [feed_key.to_bytes(), [0u8; 32], [0u8; 32]],
        },
        &mut [&mut admin, &mut market, &mut wrong_key],
    );
    assert_err_and_market_unchanged(wrong_key_result, &market, &before);

    let mut stale = switchboard_account(feed_key, 100_000_000, 0, 39);
    let stale_result = run_ix(
        Instruction::ConfigureHybridOracle {
            asset_index: 0,
            now_slot: 1,
            now_unix_ts: 100,
            oracle_leg_count: 1,
            oracle_leg_flags: 0,
            max_staleness_secs: 60,
            hybrid_soft_stale_slots: 10,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            invert: 0,
            unit_scale: 0,
            conf_filter_bps: 500,
            oracle_leg_feeds: [feed_key.to_bytes(), [0u8; 32], [0u8; 32]],
        },
        &mut [&mut admin, &mut market, &mut stale],
    );
    assert_err_and_market_unchanged(stale_result, &market, &before);

    let mut wide = switchboard_account(feed_key, 100_000_000, 6_000_000, 100);
    let conf_result = run_ix(
        Instruction::ConfigureHybridOracle {
            asset_index: 0,
            now_slot: 1,
            now_unix_ts: 100,
            oracle_leg_count: 1,
            oracle_leg_flags: 0,
            max_staleness_secs: 60,
            hybrid_soft_stale_slots: 10,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            invert: 0,
            unit_scale: 0,
            conf_filter_bps: 500,
            oracle_leg_feeds: [feed_key.to_bytes(), [0u8; 32], [0u8; 32]],
        },
        &mut [&mut admin, &mut market, &mut wide],
    );
    assert_err_and_market_unchanged(conf_result, &market, &before);
}

#[test]
fn v16_wrapper_chainlink_oracle_rejects_wrong_key_stale_and_bad_answer() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    run_ix(
        default_init_market_ix(),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();

    let feed_key = Pubkey::new_unique();
    let before = market.data.clone();
    let mut wrong_key = chainlink_account(Pubkey::new_unique(), 100_000_000, 6, 100);
    let wrong_key_result = run_ix(
        Instruction::ConfigureHybridOracle {
            asset_index: 0,
            now_slot: 1,
            now_unix_ts: 100,
            oracle_leg_count: 1,
            oracle_leg_flags: 0,
            max_staleness_secs: 60,
            hybrid_soft_stale_slots: 10,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            invert: 0,
            unit_scale: 0,
            conf_filter_bps: 500,
            oracle_leg_feeds: [feed_key.to_bytes(), [0u8; 32], [0u8; 32]],
        },
        &mut [&mut admin, &mut market, &mut wrong_key],
    );
    assert_err_and_market_unchanged(wrong_key_result, &market, &before);

    let mut stale = chainlink_account(feed_key, 100_000_000, 6, 39);
    let stale_result = run_ix(
        Instruction::ConfigureHybridOracle {
            asset_index: 0,
            now_slot: 1,
            now_unix_ts: 100,
            oracle_leg_count: 1,
            oracle_leg_flags: 0,
            max_staleness_secs: 60,
            hybrid_soft_stale_slots: 10,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            invert: 0,
            unit_scale: 0,
            conf_filter_bps: 500,
            oracle_leg_feeds: [feed_key.to_bytes(), [0u8; 32], [0u8; 32]],
        },
        &mut [&mut admin, &mut market, &mut stale],
    );
    assert_err_and_market_unchanged(stale_result, &market, &before);

    let mut bad_answer = chainlink_account(feed_key, 0, 6, 100);
    let bad_result = run_ix(
        Instruction::ConfigureHybridOracle {
            asset_index: 0,
            now_slot: 1,
            now_unix_ts: 100,
            oracle_leg_count: 1,
            oracle_leg_flags: 0,
            max_staleness_secs: 60,
            hybrid_soft_stale_slots: 10,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            invert: 0,
            unit_scale: 0,
            conf_filter_bps: 500,
            oracle_leg_feeds: [feed_key.to_bytes(), [0u8; 32], [0u8; 32]],
        },
        &mut [&mut admin, &mut market, &mut bad_answer],
    );
    assert_err_and_market_unchanged(bad_result, &market, &before);
}

#[test]
fn v16_wrapper_configure_hybrid_oracle_allows_empty_portfolio_grief_without_blocking_setup() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    let mut griefer = signer();
    let mut empty_portfolio = portfolio_account();
    run_ix(
        default_init_market_ix(),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();
    init_portfolio(&mut griefer, &mut market, &mut empty_portfolio);
    assert_eq!(
        state::read_market(&market.data)
            .unwrap()
            .1
            .materialized_portfolio_count,
        1,
        "test must model a public empty portfolio created before oracle setup"
    );

    let feeds = [[0xb1u8; 32], [0xb2u8; 32], [0xb3u8; 32]];
    let mut toto_jpy = pyth_account(&feeds[0], 4_000_000_000, -6, 1, 100);
    let mut usd_jpy = pyth_account(&feeds[1], 150_000_000, -6, 1, 100);
    let mut sol_usd = pyth_account(&feeds[2], 200_000_000, -6, 1, 100);
    configure_three_leg_hybrid(
        &mut admin,
        &mut market,
        feeds,
        &mut toto_jpy,
        &mut usd_jpy,
        &mut sol_usd,
        1,
        100,
        10,
        0,
    );

    let (cfg, group) = state::read_market(&market.data).unwrap();
    assert_eq!(cfg.oracle_leg_count, 3);
    assert_eq!(cfg.mark_ewma_e6, 133_333);
    assert_eq!(group.materialized_portfolio_count, 1);
    assert_eq!(group.c_tot, 0);
}

#[test]
fn v16_wrapper_configure_hybrid_oracle_allows_prefunded_flat_portfolio_without_blocking_setup() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();
    run_ix(
        default_init_market_ix(),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 1);

    let feeds = [[0xc1u8; 32], [0xc2u8; 32], [0xc3u8; 32]];
    let mut toto_jpy = pyth_account(&feeds[0], 4_000_000_000, -6, 1, 100);
    let mut usd_jpy = pyth_account(&feeds[1], 150_000_000, -6, 1, 100);
    let mut sol_usd = pyth_account(&feeds[2], 200_000_000, -6, 1, 100);
    configure_three_leg_hybrid(
        &mut admin,
        &mut market,
        feeds,
        &mut toto_jpy,
        &mut usd_jpy,
        &mut sol_usd,
        1,
        100,
        10,
        0,
    );

    let (cfg, group) = state::read_market(&market.data).unwrap();
    assert_eq!(cfg.oracle_leg_count, 3);
    assert_eq!(group.c_tot, 1);
    assert_eq!(
        group.assets[0].oi_eff_long_q, 0,
        "test must keep the prefunded account flat"
    );
}

#[test]
fn v16_wrapper_configure_hybrid_oracle_rejects_after_positions_enter_market() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();
    run_ix(
        default_init_market_ix(),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();
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

    let feeds = [[0xd4u8; 32], [0xd5u8; 32], [0xd6u8; 32]];
    let mut toto_jpy = pyth_account(&feeds[0], 4_000_000_000, -6, 1, 100);
    let mut usd_jpy = pyth_account(&feeds[1], 150_000_000, -6, 1, 100);
    let mut sol_usd = pyth_account(&feeds[2], 200_000_000, -6, 1, 100);
    let before = market.data.clone();
    let result = run_ix(
        Instruction::ConfigureHybridOracle {
            asset_index: 0,
            now_slot: 1,
            now_unix_ts: 100,
            oracle_leg_count: 3,
            oracle_leg_flags: ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3,
            max_staleness_secs: 60,
            hybrid_soft_stale_slots: 10,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            invert: 0,
            unit_scale: 0,
            conf_filter_bps: 500,
            oracle_leg_feeds: feeds,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut toto_jpy,
            &mut usd_jpy,
            &mut sol_usd,
        ],
    );
    assert_err_and_market_unchanged(result, &market, &before);
}

#[test]
fn v16_wrapper_configuring_empty_asset_does_not_advance_other_asset_fee_anchor() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();
    run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_portfolio_assets,
                maintenance_fee_per_slot,
                ..
            } = ix
            {
                *max_portfolio_assets = 2;
                *maintenance_fee_per_slot = 1;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 1_000);
    deposit(&mut short_owner, &mut market, &mut short_account, 10_000);
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

    let (_, before_configure) = state::read_market(&market.data).unwrap();
    assert_eq!(before_configure.slot_last, 0);
    assert_ne!(
        before_configure.assets[0].oi_eff_long_q, 0,
        "asset 0 must be exposed for this regression"
    );
    assert_eq!(
        before_configure.assets[1].oi_eff_long_q, 0,
        "asset 1 must be empty so oracle reconfiguration remains allowed"
    );

    run_ix(
        Instruction::ConfigureHyperpMark {
            asset_index: 1,
            now_slot: 100,
            initial_mark_e6: 250,
            mark_ewma_halflife_slots: 10,
            mark_min_fee: 0,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    let (_, after_configure) = state::read_market(&market.data).unwrap();
    assert_eq!(after_configure.current_slot, 100);
    assert_eq!(after_configure.assets[1].effective_price, 250);
    assert_eq!(
        after_configure.slot_last, 0,
        "configuring an empty asset must not declare other exposed assets loss-current"
    );

    let feeds = [[0xe1u8; 32], [0xe2u8; 32], [0xe3u8; 32]];
    let mut leg0 = pyth_account(&feeds[0], 4_000_000_000, -6, 1, 1_000);
    let mut leg1 = pyth_account(&feeds[1], 150_000_000, -6, 1, 1_000);
    let mut leg2 = pyth_account(&feeds[2], 200_000_000, -6, 1, 1_000);
    run_ix(
        Instruction::ConfigureHybridOracle {
            asset_index: 1,
            now_slot: 101,
            now_unix_ts: 1_000,
            oracle_leg_count: 3,
            oracle_leg_flags: ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3,
            max_staleness_secs: 60,
            hybrid_soft_stale_slots: 10,
            mark_ewma_halflife_slots: 10,
            mark_min_fee: 0,
            invert: 0,
            unit_scale: 0,
            conf_filter_bps: 500,
            oracle_leg_feeds: feeds,
        },
        &mut [&mut admin, &mut market, &mut leg0, &mut leg1, &mut leg2],
    )
    .unwrap();
    let (_, after_hybrid_configure) = state::read_market(&market.data).unwrap();
    assert_eq!(after_hybrid_configure.current_slot, 101);
    assert_eq!(
        after_hybrid_configure.slot_last, 0,
        "hybrid configuration of an empty asset must preserve the same group loss anchor"
    );

    let insurance_before_fee = after_hybrid_configure.insurance;
    sync_maintenance_fee(&mut market, &mut long_account, 101).unwrap();
    let (_, after_fee) = state::read_market(&market.data).unwrap();
    let long_after_fee = state::read_portfolio(&long_account.data).unwrap();
    assert_eq!(
        after_fee.insurance, insurance_before_fee,
        "maintenance fees on a nonflat account are loss-safe only up to the unchanged global anchor"
    );
    assert_eq!(long_after_fee.last_fee_slot, 0);
}

#[test]
fn v16_wrapper_hybrid_fresh_crank_tracks_external_composite_then_after_hours_trade_moves_mark() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_trading_fee_bps,
                trade_fee_base_bps,
                max_price_move_bps_per_slot,
                ..
            } = ix
            {
                *max_trading_fee_bps = 10_000;
                *trade_fee_base_bps = 1;
                *max_price_move_bps_per_slot = 500;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();

    let feeds = [[0x44u8; 32], [0x55u8; 32], [0x66u8; 32]];
    let mut toto_jpy = pyth_account(&feeds[0], 4_000_000_000, -6, 1, 100);
    let mut usd_jpy = pyth_account(&feeds[1], 150_000_000, -6, 1, 100);
    let mut sol_usd = pyth_account(&feeds[2], 200_000_000, -6, 1, 100);
    configure_three_leg_hybrid(
        &mut admin,
        &mut market,
        feeds,
        &mut toto_jpy,
        &mut usd_jpy,
        &mut sol_usd,
        1,
        100,
        3,
        0,
    );

    let mut keeper = signer();
    let mut keeper_portfolio = portfolio_account();
    init_portfolio(&mut keeper, &mut market, &mut keeper_portfolio);
    toto_jpy.data = make_pyth(&feeds[0], 4_200_000_000, -6, 1, 101);
    run_ix(
        Instruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 2,
            effective_price: 133_333,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [
            &mut keeper,
            &mut market,
            &mut keeper_portfolio,
            &mut toto_jpy,
            &mut usd_jpy,
            &mut sol_usd,
        ],
    )
    .unwrap();
    let (cfg_after_fresh, group_after_fresh) = state::read_market(&market.data).unwrap();
    assert_eq!(
        group_after_fresh.assets[0].effective_price, 140_000,
        "flat markets accept the fresh composite target directly"
    );
    assert_eq!(cfg_after_fresh.mark_ewma_e6, 140_000);
    assert_eq!(cfg_after_fresh.last_good_oracle_slot, 2);

    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 10_000_000);
    deposit(
        &mut short_owner,
        &mut market,
        &mut short_account,
        10_000_000,
    );
    {
        let (mut cfg, mut group) = state::read_market(&market.data).unwrap();
        cfg.last_good_oracle_slot = 0;
        cfg.mark_ewma_last_slot = 0;
        group.current_slot = 10;
        group.slot_last = 10;
        group.assets[0].slot_last = 10;
        group.assets[0].raw_oracle_target_price = group.assets[0].effective_price;
        group.loss_stale_active = false;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }
    let (before_cfg, before_group) = state::read_market(&market.data).unwrap();
    let size_q = 10 * POS_SCALE;
    let exec_price = before_group.assets[0].effective_price * 150 / 100;
    let base_only_fee = two_sided_fee(size_q, exec_price, 1);
    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: size_q as i128,
            exec_price,
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
    let (after_cfg, after_group) = state::read_market(&market.data).unwrap();
    assert!(
        after_group.insurance > base_only_fee,
        "after-hours hybrid trade must pay dynamic mark-movement surcharge"
    );
    assert!(
        after_cfg.mark_ewma_e6 > before_cfg.mark_ewma_e6,
        "after-hours execution must move the fallback EWMA mark"
    );
    assert_eq!(
        after_group.assets[0].effective_price, before_group.assets[0].effective_price,
        "execution price flexibility must not rewrite the accepted oracle index"
    );
}

#[test]
fn v16_wrapper_hybrid_regular_hours_wide_trade_keeps_mark_pinned_to_external_oracle() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_trading_fee_bps,
                trade_fee_base_bps,
                max_price_move_bps_per_slot,
                ..
            } = ix
            {
                *max_trading_fee_bps = 10_000;
                *trade_fee_base_bps = 1;
                *max_price_move_bps_per_slot = 500;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();

    let feeds = [[0x77u8; 32], [0x88u8; 32], [0x99u8; 32]];
    let mut toto_jpy = pyth_account(&feeds[0], 4_000_000_000, -6, 1, 100);
    let mut usd_jpy = pyth_account(&feeds[1], 150_000_000, -6, 1, 100);
    let mut sol_usd = pyth_account(&feeds[2], 200_000_000, -6, 1, 100);
    configure_three_leg_hybrid(
        &mut admin,
        &mut market,
        feeds,
        &mut toto_jpy,
        &mut usd_jpy,
        &mut sol_usd,
        1,
        100,
        100,
        0,
    );
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 10_000_000);
    deposit(
        &mut short_owner,
        &mut market,
        &mut short_account,
        10_000_000,
    );
    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        group.current_slot = 10;
        group.slot_last = 10;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }
    let (before_cfg, before_group) = state::read_market(&market.data).unwrap();
    let size_q = 10 * POS_SCALE;
    let exec_price = before_group.assets[0].effective_price * 150 / 100;
    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: size_q as i128,
            exec_price,
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
    let (after_cfg, after_group) = state::read_market(&market.data).unwrap();
    assert_eq!(after_cfg.mark_ewma_e6, before_cfg.mark_ewma_e6);
    assert_eq!(
        after_cfg.mark_ewma_last_slot,
        before_cfg.mark_ewma_last_slot
    );
    assert_eq!(
        after_group.insurance,
        two_sided_fee(size_q, exec_price, 1),
        "regular-hours hybrid pays only the static 1 bp two-sided fee"
    );
}

#[test]
fn v16_wrapper_hybrid_after_hours_downward_mark_moves_effective_price() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_trading_fee_bps,
                trade_fee_base_bps,
                max_price_move_bps_per_slot,
                ..
            } = ix
            {
                *max_trading_fee_bps = 10_000;
                *trade_fee_base_bps = 1;
                *max_price_move_bps_per_slot = 500;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();

    let feeds = [[0xd1u8; 32], [0xd2u8; 32], [0xd3u8; 32]];
    let mut toto_jpy = pyth_account(&feeds[0], 4_000_000_000, -6, 1, 100);
    let mut usd_jpy = pyth_account(&feeds[1], 150_000_000, -6, 1, 100);
    let mut sol_usd = pyth_account(&feeds[2], 200_000_000, -6, 1, 100);
    configure_three_leg_hybrid(
        &mut admin,
        &mut market,
        feeds,
        &mut toto_jpy,
        &mut usd_jpy,
        &mut sol_usd,
        1,
        100,
        3,
        0,
    );

    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(
        &mut long_owner,
        &mut market,
        &mut long_account,
        1_000_000_000,
    );
    deposit(
        &mut short_owner,
        &mut market,
        &mut short_account,
        1_000_000_000,
    );
    {
        let (mut cfg, mut group) = state::read_market(&market.data).unwrap();
        cfg.last_good_oracle_slot = 0;
        cfg.mark_ewma_last_slot = 0;
        group.current_slot = 10;
        group.slot_last = 10;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }

    let (before_cfg, before_group) = state::read_market(&market.data).unwrap();
    let size_q = 10 * POS_SCALE;
    let exec_price = before_group.assets[0].effective_price / 2;
    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: size_q as i128,
            exec_price,
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
    let (after_trade_cfg, after_trade_group) = state::read_market(&market.data).unwrap();
    assert!(
        after_trade_cfg.mark_ewma_e6 < before_cfg.mark_ewma_e6,
        "after-hours below-mark trade should move fallback EWMA down"
    );
    assert_eq!(
        after_trade_group.assets[0].effective_price, before_group.assets[0].effective_price,
        "trades update fallback mark, not the accepted engine price"
    );

    run_ix(
        Instruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 11,
            effective_price: after_trade_group.assets[0].effective_price,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut admin, &mut market, &mut long_account],
    )
    .unwrap();
    let (_, after_crank_group) = state::read_market(&market.data).unwrap();
    let expected = oracle_v16::effective_price_from_target(
        after_trade_group.assets[0].effective_price,
        after_trade_cfg.mark_ewma_e6,
        after_trade_group.config.max_price_move_bps_per_slot,
        1,
        true,
    );
    assert!(
        expected < after_trade_group.assets[0].effective_price,
        "test must select a fallback mark below the accepted engine price"
    );
    assert_eq!(after_crank_group.assets[0].effective_price, expected);
}

#[test]
fn v16_wrapper_hybrid_after_hours_fee_floor_scales_with_next_crank_segment_budget() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_trading_fee_bps,
                trade_fee_base_bps,
                max_price_move_bps_per_slot,
                max_accrual_dt_slots,
                min_funding_lifetime_slots,
                ..
            } = ix
            {
                *max_trading_fee_bps = 10_000;
                *trade_fee_base_bps = 1;
                *max_price_move_bps_per_slot = 500;
                *max_accrual_dt_slots = 10;
                *min_funding_lifetime_slots = 10;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();

    let feeds = [[0xf1u8; 32], [0xf2u8; 32], [0xf3u8; 32]];
    let mut toto_jpy = pyth_account(&feeds[0], 4_000_000_000, -6, 1, 100);
    let mut usd_jpy = pyth_account(&feeds[1], 150_000_000, -6, 1, 100);
    let mut sol_usd = pyth_account(&feeds[2], 200_000_000, -6, 1, 100);
    configure_three_leg_hybrid(
        &mut admin,
        &mut market,
        feeds,
        &mut toto_jpy,
        &mut usd_jpy,
        &mut sol_usd,
        1,
        100,
        3,
        0,
    );

    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 10_000_000);
    deposit(
        &mut short_owner,
        &mut market,
        &mut short_account,
        10_000_000,
    );
    {
        let (mut cfg, mut group) = state::read_market(&market.data).unwrap();
        cfg.last_good_oracle_slot = 0;
        cfg.mark_ewma_last_slot = 0;
        group.current_slot = 11;
        group.slot_last = 1;
        group.loss_stale_active = false;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }

    let (_, before_group) = state::read_market(&market.data).unwrap();
    let size_q = 10 * POS_SCALE;
    let exec_price = before_group.assets[0].effective_price;
    let before_insurance = before_group.insurance;
    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: size_q as i128,
            exec_price,
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
    let (_, after_group) = state::read_market(&market.data).unwrap();
    let fee_delta = after_group.insurance - before_insurance;
    let one_slot_floor = two_sided_fee(size_q, exec_price, 1 + 500);
    let full_segment_floor = two_sided_fee(size_q, exec_price, 1 + 500 * 10);
    assert!(
        fee_delta >= full_segment_floor,
        "stale fallback fee must cover the full price movement budget the next honest crank can consume"
    );
    assert!(
        full_segment_floor > one_slot_floor,
        "test must distinguish a one-slot fee floor from the full bounded segment floor"
    );
}

#[test]
fn v16_wrapper_hybrid_after_hours_max_caller_fee_does_not_bypass_dynamic_fee_rejection() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_trading_fee_bps,
                trade_fee_base_bps,
                max_price_move_bps_per_slot,
                ..
            } = ix
            {
                *max_trading_fee_bps = 10_000;
                *trade_fee_base_bps = 1;
                *max_price_move_bps_per_slot = 500;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();

    let feeds = [[0xe1u8; 32], [0xe2u8; 32], [0xe3u8; 32]];
    let mut toto_jpy = pyth_account(&feeds[0], 4_000_000_000, -6, 1, 100);
    let mut usd_jpy = pyth_account(&feeds[1], 150_000_000, -6, 1, 100);
    let mut sol_usd = pyth_account(&feeds[2], 200_000_000, -6, 1, 100);
    configure_three_leg_hybrid(
        &mut admin,
        &mut market,
        feeds,
        &mut toto_jpy,
        &mut usd_jpy,
        &mut sol_usd,
        1,
        100,
        3,
        0,
    );

    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(
        &mut long_owner,
        &mut market,
        &mut long_account,
        1_000_000_000,
    );
    deposit(
        &mut short_owner,
        &mut market,
        &mut short_account,
        1_000_000_000,
    );

    let initial_price = state::read_market(&market.data).unwrap().1.assets[0].effective_price;
    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: (100 * POS_SCALE) as i128,
            exec_price: initial_price,
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
    {
        let (mut cfg, mut group) = state::read_market(&market.data).unwrap();
        cfg.last_good_oracle_slot = 0;
        cfg.mark_ewma_last_slot = 0;
        group.current_slot = 10;
        group.slot_last = 10;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }

    let before_market = market.data.clone();
    let before_long = long_account.data.clone();
    let before_short = short_account.data.clone();
    let (_, before_group) = state::read_market(&market.data).unwrap();
    let probe_size = POS_SCALE / 100;
    let probe_price = before_group.assets[0].effective_price * 2;
    let max_side_q = core::cmp::max(
        before_group.assets[0].oi_eff_long_q,
        before_group.assets[0].oi_eff_short_q,
    );
    let max_side_notional =
        (max_side_q * before_group.assets[0].effective_price as u128 + POS_SCALE - 1) / POS_SCALE;
    let required = policy_v16::dynamic_fee_bps_with_externality_floor(
        10_000,
        state::read_market(&market.data).unwrap().0.mark_ewma_e6,
        oracle_v16::clamp_toward_engine_dt(
            before_group.assets[0].effective_price,
            probe_price,
            before_group.config.max_price_move_bps_per_slot,
            1,
        ),
        1,
        0,
        before_group.current_slot,
        probe_size * probe_price as u128 / POS_SCALE,
        max_side_notional.checked_mul(2).unwrap(),
        0,
        before_group.config.max_price_move_bps_per_slot,
    );
    assert!(
        required.is_none(),
        "test must select a trade whose dynamic externality fee exceeds the market cap"
    );

    let result = run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: probe_size as i128,
            exec_price: probe_price,
            fee_bps: 10_000,
        },
        &mut [
            &mut long_owner,
            &mut short_owner,
            &mut market,
            &mut long_account,
            &mut short_account,
        ],
    );
    assert_err_and_market_unchanged(result, &market, &before_market);
    assert_eq!(long_account.data, before_long);
    assert_eq!(short_account.data, before_short);
}

#[test]
fn v16_wrapper_tradenocpi_applies_static_base_fee_floor() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();

    run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_trading_fee_bps,
                trade_fee_base_bps,
                ..
            } = ix
            {
                *max_trading_fee_bps = 1_000;
                *trade_fee_base_bps = 100;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 1_000_000);
    deposit(&mut short_owner, &mut market, &mut short_account, 1_000_000);

    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: (10 * POS_SCALE) as i128,
            exec_price: 150,
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

    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(
        group.insurance, 30,
        "zero caller fee still pays the configured 100 bps base fee on both sides"
    );
}

#[test]
fn v16_wrapper_tradenocpi_rejects_when_consented_price_would_break_margin() {
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
    assert!(percolator::active_bitmap_is_empty(long.active_bitmap));
    assert!(percolator::active_bitmap_is_empty(short.active_bitmap));
}

#[test]
fn v16_wrapper_convert_released_pnl_respects_cap_and_unlocks_withdrawal() {
    let mut admin = signer();
    let mut market = market_account();
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 10_000);
    deposit(&mut short_owner, &mut market, &mut short_account, 10_000);

    run_ix(
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
    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: (2 * POS_SCALE) as i128,
            exec_price: 101,
            fee_bps: 0,
        },
        &mut [
            &mut short_owner,
            &mut long_owner,
            &mut market,
            &mut short_account,
            &mut long_account,
        ],
    )
    .unwrap();

    let before_market = market.data.clone();
    let before_long = long_account.data.clone();
    let before_short = short_account.data.clone();
    let long = state::read_portfolio(&long_account.data).unwrap();
    assert!(percolator::active_bitmap_is_empty(long.active_bitmap));
    assert_eq!(long.pnl, 2);
    assert_eq!(long.capital, 10_000);

    let too_low_cap = run_ix(
        Instruction::ConvertReleasedPnl { amount: 1 },
        &mut [&mut long_owner, &mut market, &mut long_account],
    );
    assert_err_and_market_unchanged(too_low_cap, &market, &before_market);
    assert_eq!(long_account.data, before_long);
    assert_eq!(short_account.data, before_short);

    run_ix(
        Instruction::ConvertReleasedPnl { amount: 2 },
        &mut [&mut long_owner, &mut market, &mut long_account],
    )
    .unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    let long = state::read_portfolio(&long_account.data).unwrap();
    assert_eq!(long.pnl, 0);
    assert_eq!(long.capital, 10_002);
    assert_eq!(group.pnl_pos_tot, 0);

    withdraw(&mut long_owner, &mut market, &mut long_account, 2);
    let (_, group) = state::read_market(&market.data).unwrap();
    let long = state::read_portfolio(&long_account.data).unwrap();
    assert_eq!(long.capital, 10_000);
    assert_eq!(group.vault, 19_998);
}

#[test]
fn v16_wrapper_convert_released_pnl_rejects_resolved_market_without_mutation() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 10);
    run_ix(Instruction::ResolveMarket, &mut [&mut admin, &mut market]).unwrap();

    let market_before = market.data.clone();
    let portfolio_before = portfolio.data.clone();
    let rejected = run_ix(
        Instruction::ConvertReleasedPnl { amount: 1 },
        &mut [&mut owner, &mut market, &mut portfolio],
    );
    assert_err_and_market_unchanged(rejected, &market, &market_before);
    assert_eq!(portfolio.data, portfolio_before);
}

#[test]
fn v16_wrapper_tradenocpi_rejects_bad_size_and_missing_signer_before_mutation() {
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
fn v16_wrapper_tradenocpi_rejects_wrong_owner_fee_cap_and_invalid_asset() {
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
fn v16_wrapper_tradecpi_executes_manual_consented_wide_price() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner_a = signer();
    let mut owner_b = signer();
    let mut account_a = portfolio_account();
    let mut account_b = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner_a, &mut market, &mut account_a);
    init_portfolio(&mut owner_b, &mut market, &mut account_b);
    deposit(&mut owner_a, &mut market, &mut account_a, 1_000_000);
    deposit(&mut owner_b, &mut market, &mut account_b, 1_000_000);

    let (_, before_group) = state::read_market(&market.data).unwrap();
    let size_q = POS_SCALE;
    let exec_price = before_group.assets[0].effective_price * 150 / 100;
    run_trade_cpi_with_matcher(
        &mut owner_a,
        &mut owner_b,
        &mut market,
        &mut account_a,
        &mut account_b,
        0,
        size_q as i128,
        size_q as i128,
        exec_price,
        0,
        0,
    )
    .unwrap();

    let (_, after_group) = state::read_market(&market.data).unwrap();
    let after_a = state::read_portfolio(&account_a.data).unwrap();
    let after_b = state::read_portfolio(&account_b.data).unwrap();
    assert_eq!(
        after_group.assets[0].effective_price, before_group.assets[0].effective_price,
        "consented execution price must not rewrite the accepted engine price"
    );
    assert_eq!(after_a.legs[0].basis_pos_q, size_q as i128);
    assert_eq!(after_b.legs[0].basis_pos_q, -(size_q as i128));
}

#[test]
fn v16_wrapper_tradecpi_executes_on_added_asset_and_binds_matcher_asset_echo() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner_a = signer();
    let mut owner_b = signer();
    let mut account_a = portfolio_account();
    let mut account_b = portfolio_account();

    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_portfolio_assets,
                ..
            } = ix
            {
                *max_portfolio_assets = 2;
            }
        }),
    );
    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        2,
        1,
        250,
    )
    .unwrap();
    init_portfolio(&mut owner_a, &mut market, &mut account_a);
    init_portfolio(&mut owner_b, &mut market, &mut account_b);
    deposit(&mut owner_a, &mut market, &mut account_a, 1_000_000);
    deposit(&mut owner_b, &mut market, &mut account_b, 1_000_000);

    let (_, before_group) = state::read_market(&market.data).unwrap();
    run_trade_cpi_with_matcher(
        &mut owner_a,
        &mut owner_b,
        &mut market,
        &mut account_a,
        &mut account_b,
        2,
        POS_SCALE as i128,
        POS_SCALE as i128,
        275,
        0,
        0,
    )
    .unwrap();

    let (_, after_group) = state::read_market(&market.data).unwrap();
    let after_a = state::read_portfolio(&account_a.data).unwrap();
    let after_b = state::read_portfolio(&account_b.data).unwrap();
    assert_eq!(
        after_group.assets[2].effective_price, before_group.assets[2].effective_price,
        "CPI execution price for an added asset must not rewrite the accepted index"
    );
    assert_eq!(after_group.assets[0].oi_eff_long_q, 0);
    assert_eq!(after_group.assets[0].oi_eff_short_q, 0);
    assert_eq!(after_group.assets[2].oi_eff_long_q, POS_SCALE);
    assert_eq!(after_group.assets[2].oi_eff_short_q, POS_SCALE);
    assert_eq!(after_a.active_bitmap, active_bitmap_with(&[0]));
    assert_eq!(after_b.active_bitmap, active_bitmap_with(&[0]));
    assert_eq!(
        active_leg_for_asset(&after_a, 2).basis_pos_q,
        POS_SCALE as i128
    );
    assert_eq!(
        active_leg_for_asset(&after_b, 2).basis_pos_q,
        -(POS_SCALE as i128)
    );
}

#[test]
fn v16_wrapper_tradecpi_hybrid_regular_and_after_hours_follow_mark_policy() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_trading_fee_bps,
                trade_fee_base_bps,
                max_price_move_bps_per_slot,
                ..
            } = ix
            {
                *max_trading_fee_bps = 10_000;
                *trade_fee_base_bps = 1;
                *max_price_move_bps_per_slot = 500;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();

    let feeds = [[0xc1u8; 32], [0xc2u8; 32], [0xc3u8; 32]];
    let mut toto_jpy = pyth_account(&feeds[0], 4_000_000_000, -6, 1, 100);
    let mut usd_jpy = pyth_account(&feeds[1], 150_000_000, -6, 1, 100);
    let mut sol_usd = pyth_account(&feeds[2], 200_000_000, -6, 1, 100);
    configure_three_leg_hybrid(
        &mut admin,
        &mut market,
        feeds,
        &mut toto_jpy,
        &mut usd_jpy,
        &mut sol_usd,
        1,
        100,
        100,
        0,
    );

    let mut owner_a = signer();
    let mut owner_b = signer();
    let mut account_a = portfolio_account();
    let mut account_b = portfolio_account();
    init_portfolio(&mut owner_a, &mut market, &mut account_a);
    init_portfolio(&mut owner_b, &mut market, &mut account_b);
    deposit(&mut owner_a, &mut market, &mut account_a, 100_000_000);
    deposit(&mut owner_b, &mut market, &mut account_b, 100_000_000);
    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        group.current_slot = 10;
        group.slot_last = 10;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }

    let (regular_cfg_before, regular_group_before) = state::read_market(&market.data).unwrap();
    let size_q = 10 * POS_SCALE;
    let regular_exec_price = regular_group_before.assets[0].effective_price * 150 / 100;
    run_trade_cpi_with_matcher(
        &mut owner_a,
        &mut owner_b,
        &mut market,
        &mut account_a,
        &mut account_b,
        0,
        size_q as i128,
        size_q as i128,
        regular_exec_price,
        0,
        0,
    )
    .unwrap();
    let (regular_cfg_after, regular_group_after) = state::read_market(&market.data).unwrap();
    assert_eq!(
        regular_cfg_after.mark_ewma_e6,
        regular_cfg_before.mark_ewma_e6
    );
    assert_eq!(
        regular_cfg_after.mark_ewma_last_slot,
        regular_cfg_before.mark_ewma_last_slot
    );
    assert_eq!(
        regular_group_after.insurance,
        two_sided_fee(size_q, regular_exec_price, 1),
        "regular-hours hybrid CPI trades pay only the static fee floor"
    );

    {
        let (mut cfg, mut group) = state::read_market(&market.data).unwrap();
        cfg.last_good_oracle_slot = 0;
        cfg.mark_ewma_last_slot = 0;
        cfg.hybrid_soft_stale_slots = 3;
        group.current_slot = 10;
        group.slot_last = 10;
        group.loss_stale_active = false;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }
    let (stale_cfg_before, stale_group_before) = state::read_market(&market.data).unwrap();
    let stale_exec_price = stale_group_before.assets[0].effective_price * 150 / 100;
    let insurance_before = stale_group_before.insurance;
    run_trade_cpi_with_matcher(
        &mut owner_a,
        &mut owner_b,
        &mut market,
        &mut account_a,
        &mut account_b,
        0,
        size_q as i128,
        size_q as i128,
        stale_exec_price,
        0,
        0,
    )
    .unwrap();
    let (stale_cfg_after, stale_group_after) = state::read_market(&market.data).unwrap();
    assert!(
        stale_cfg_after.mark_ewma_e6 > stale_cfg_before.mark_ewma_e6,
        "after-hours CPI execution must move the fallback EWMA mark"
    );
    assert!(
        stale_group_after.insurance - insurance_before > two_sided_fee(size_q, stale_exec_price, 1),
        "after-hours CPI execution must pay dynamic mark-movement surcharge"
    );
    assert_eq!(
        stale_group_after.assets[0].effective_price, stale_group_before.assets[0].effective_price,
        "CPI execution price flexibility must not rewrite the accepted index"
    );
}

#[test]
fn v16_wrapper_tradecpi_hyperp_trade_moves_mark_without_refreshing_liveness() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_trading_fee_bps,
                trade_fee_base_bps,
                max_price_move_bps_per_slot,
                ..
            } = ix
            {
                *max_trading_fee_bps = 10_000;
                *trade_fee_base_bps = 1;
                *max_price_move_bps_per_slot = 500;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();
    run_ix(
        Instruction::ConfigureHyperpMark {
            asset_index: 0,
            now_slot: 5,
            initial_mark_e6: 100,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();

    let mut owner_a = signer();
    let mut owner_b = signer();
    let mut account_a = portfolio_account();
    let mut account_b = portfolio_account();
    init_portfolio(&mut owner_a, &mut market, &mut account_a);
    init_portfolio(&mut owner_b, &mut market, &mut account_b);
    deposit(&mut owner_a, &mut market, &mut account_a, 10_000_000);
    deposit(&mut owner_b, &mut market, &mut account_b, 10_000_000);
    {
        let (mut cfg, mut group) = state::read_market(&market.data).unwrap();
        cfg.mark_ewma_last_slot = 0;
        group.current_slot = 10;
        group.slot_last = 10;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }

    let (before_cfg, before_group) = state::read_market(&market.data).unwrap();
    let size_q = 10 * POS_SCALE;
    let exec_price = before_group.assets[0].effective_price * 150 / 100;
    run_trade_cpi_with_matcher(
        &mut owner_a,
        &mut owner_b,
        &mut market,
        &mut account_a,
        &mut account_b,
        0,
        size_q as i128,
        size_q as i128,
        exec_price,
        0,
        0,
    )
    .unwrap();
    let (after_cfg, after_group) = state::read_market(&market.data).unwrap();
    assert!(
        after_cfg.mark_ewma_e6 > before_cfg.mark_ewma_e6,
        "direct Hyperp CPI trades must use the same dynamic mark path as TradeNoCpi"
    );
    assert_eq!(
        after_cfg.last_good_oracle_slot, before_cfg.last_good_oracle_slot,
        "trade-flow mark movement must not refresh the permissionless stale-resolution liveness stamp"
    );
    assert_eq!(
        after_group.assets[0].effective_price, before_group.assets[0].effective_price,
        "direct Hyperp CPI trades must not rewrite the accepted effective index"
    );
}

#[test]
fn v16_wrapper_tradecpi_requires_bilateral_signatures_before_matcher_cpi() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner_a = signer();
    let mut owner_b = signer();
    let mut unsigned_b = TestAccount::new(owner_b.key, Pubkey::new_unique(), 0);
    let mut account_a = portfolio_account();
    let mut account_b = portfolio_account();
    let mut matcher_program = matcher_program_account();
    let mut matcher_context = matcher_context_account(&matcher_program);
    let mut delegate =
        matcher_delegate_account(&market, &account_b, &matcher_program, &matcher_context);

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner_a, &mut market, &mut account_a);
    init_portfolio(&mut owner_b, &mut market, &mut account_b);
    deposit(&mut owner_a, &mut market, &mut account_a, 1_000_000);
    deposit(&mut owner_b, &mut market, &mut account_b, 1_000_000);

    let before_market = market.data.clone();
    let before_a = account_a.data.clone();
    let before_b = account_b.data.clone();
    let rejected = run_ix(
        Instruction::TradeCpi {
            asset_index: 0,
            size_q: POS_SCALE as i128,
            fee_bps: 0,
            limit_price: 0,
        },
        &mut [
            &mut owner_a,
            &mut unsigned_b,
            &mut market,
            &mut account_a,
            &mut account_b,
            &mut matcher_program,
            &mut matcher_context,
            &mut delegate,
        ],
    );
    assert_err_and_market_unchanged(rejected, &market, &before_market);
    assert_eq!(account_a.data, before_a);
    assert_eq!(account_b.data, before_b);
    assert_eq!(
        matcher_context.data,
        vec![0u8; 320],
        "signature failure must happen before matcher context mutation"
    );
}

#[test]
fn v16_wrapper_tradecpi_rejects_wrong_delegate_and_unsafe_tail_before_cpi() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner_a = signer();
    let mut owner_b = signer();
    let mut account_a = portfolio_account();
    let mut account_b = portfolio_account();
    let mut matcher_program = matcher_program_account();
    let mut matcher_context = matcher_context_account(&matcher_program);
    let mut wrong_delegate = TestAccount::new(Pubkey::new_unique(), Pubkey::default(), 0);

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner_a, &mut market, &mut account_a);
    init_portfolio(&mut owner_b, &mut market, &mut account_b);
    deposit(&mut owner_a, &mut market, &mut account_a, 1_000_000);
    deposit(&mut owner_b, &mut market, &mut account_b, 1_000_000);

    let before_market = market.data.clone();
    let before_a = account_a.data.clone();
    let before_b = account_b.data.clone();
    let wrong_delegate_result = run_ix(
        Instruction::TradeCpi {
            asset_index: 0,
            size_q: POS_SCALE as i128,
            fee_bps: 0,
            limit_price: 0,
        },
        &mut [
            &mut owner_a,
            &mut owner_b,
            &mut market,
            &mut account_a,
            &mut account_b,
            &mut matcher_program,
            &mut matcher_context,
            &mut wrong_delegate,
        ],
    );
    assert_err_and_market_unchanged(wrong_delegate_result, &market, &before_market);
    assert_eq!(account_a.data, before_a);
    assert_eq!(account_b.data, before_b);

    let mut delegate =
        matcher_delegate_account(&market, &account_b, &matcher_program, &matcher_context);
    let mut program_owned_tail = TestAccount::new(Pubkey::new_unique(), program_id(), 0).writable();
    let unsafe_tail = run_ix(
        Instruction::TradeCpi {
            asset_index: 0,
            size_q: POS_SCALE as i128,
            fee_bps: 0,
            limit_price: 0,
        },
        &mut [
            &mut owner_a,
            &mut owner_b,
            &mut market,
            &mut account_a,
            &mut account_b,
            &mut matcher_program,
            &mut matcher_context,
            &mut delegate,
            &mut program_owned_tail,
        ],
    );
    assert_err_and_market_unchanged(unsafe_tail, &market, &before_market);
    assert_eq!(account_a.data, before_a);
    assert_eq!(account_b.data, before_b);

    let mut too_many_tail: Vec<TestAccount> = (0..33)
        .map(|_| TestAccount::new(Pubkey::new_unique(), Pubkey::new_unique(), 0))
        .collect();
    let oversized_tail = {
        let mut infos = vec![
            owner_a.to_info(),
            owner_b.to_info(),
            market.to_info(),
            account_a.to_info(),
            account_b.to_info(),
            matcher_program.to_info(),
            matcher_context.to_info(),
            delegate.to_info(),
        ];
        infos.extend(too_many_tail.iter_mut().map(TestAccount::to_info));
        processor::process_instruction(
            &program_id(),
            &infos,
            &Instruction::TradeCpi {
                asset_index: 0,
                size_q: POS_SCALE as i128,
                fee_bps: 0,
                limit_price: 0,
            }
            .encode(),
        )
    };
    assert_err_and_market_unchanged(oversized_tail, &market, &before_market);
    assert_eq!(account_a.data, before_a);
    assert_eq!(account_b.data, before_b);
}

#[test]
fn v16_wrapper_tradecpi_rejects_wrong_asset_echo_from_matcher() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner_a = signer();
    let mut owner_b = signer();
    let mut account_a = portfolio_account();
    let mut account_b = portfolio_account();
    let mut matcher_program = matcher_program_account();
    let mut matcher_context = matcher_context_account(&matcher_program);
    let mut delegate =
        matcher_delegate_account(&market, &account_b, &matcher_program, &matcher_context);

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner_a, &mut market, &mut account_a);
    init_portfolio(&mut owner_b, &mut market, &mut account_b);
    deposit(&mut owner_a, &mut market, &mut account_a, 1_000_000);
    deposit(&mut owner_b, &mut market, &mut account_b, 1_000_000);

    let (_, group) = state::read_market(&market.data).unwrap();
    let req_id = group.current_slot.wrapping_add(1);
    let lp_account_id = {
        let bytes = delegate.key.to_bytes();
        u64::from_le_bytes(bytes[0..8].try_into().unwrap())
    };
    write_matcher_return(
        &mut matcher_context,
        100,
        POS_SCALE as i128,
        req_id,
        lp_account_id,
        1,
        100,
    );

    let before_market = market.data.clone();
    let before_a = account_a.data.clone();
    let before_b = account_b.data.clone();
    let rejected = run_ix(
        Instruction::TradeCpi {
            asset_index: 0,
            size_q: POS_SCALE as i128,
            fee_bps: 0,
            limit_price: 0,
        },
        &mut [
            &mut owner_a,
            &mut owner_b,
            &mut market,
            &mut account_a,
            &mut account_b,
            &mut matcher_program,
            &mut matcher_context,
            &mut delegate,
        ],
    );
    assert_err_and_market_unchanged(rejected, &market, &before_market);
    assert_eq!(account_a.data, before_a);
    assert_eq!(account_b.data, before_b);
}

#[test]
fn v16_wrapper_tradecpi_zero_fill_rejects_resolved_market_before_success() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner_a = signer();
    let mut owner_b = signer();
    let mut account_a = portfolio_account();
    let mut account_b = portfolio_account();
    let mut matcher_program = matcher_program_account();
    let mut matcher_context = matcher_context_account(&matcher_program);
    let mut delegate =
        matcher_delegate_account(&market, &account_b, &matcher_program, &matcher_context);

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner_a, &mut market, &mut account_a);
    init_portfolio(&mut owner_b, &mut market, &mut account_b);
    run_ix(Instruction::ResolveMarket, &mut [&mut admin, &mut market]).unwrap();

    let (_, group) = state::read_market(&market.data).unwrap();
    let req_id = group.current_slot.wrapping_add(1);
    let lp_account_id = {
        let bytes = delegate.key.to_bytes();
        u64::from_le_bytes(bytes[0..8].try_into().unwrap())
    };
    write_matcher_return(&mut matcher_context, 100, 0, req_id, lp_account_id, 0, 100);

    let before_market = market.data.clone();
    let before_a = account_a.data.clone();
    let before_b = account_b.data.clone();
    let rejected = run_ix(
        Instruction::TradeCpi {
            asset_index: 0,
            size_q: POS_SCALE as i128,
            fee_bps: 0,
            limit_price: 0,
        },
        &mut [
            &mut owner_a,
            &mut owner_b,
            &mut market,
            &mut account_a,
            &mut account_b,
            &mut matcher_program,
            &mut matcher_context,
            &mut delegate,
        ],
    );
    assert_err_and_market_unchanged(rejected, &market, &before_market);
    assert_eq!(account_a.data, before_a);
    assert_eq!(account_b.data, before_b);
}

#[test]
fn v16_wrapper_tradecpi_zero_fill_rejects_fee_above_cap_before_success() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner_a = signer();
    let mut owner_b = signer();
    let mut account_a = portfolio_account();
    let mut account_b = portfolio_account();
    let mut matcher_program = matcher_program_account();
    let mut matcher_context = matcher_context_account(&matcher_program);
    let mut delegate =
        matcher_delegate_account(&market, &account_b, &matcher_program, &matcher_context);

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner_a, &mut market, &mut account_a);
    init_portfolio(&mut owner_b, &mut market, &mut account_b);
    let (_, group) = state::read_market(&market.data).unwrap();
    let req_id = group.current_slot.wrapping_add(1);
    let lp_account_id = {
        let bytes = delegate.key.to_bytes();
        u64::from_le_bytes(bytes[0..8].try_into().unwrap())
    };
    write_matcher_return(&mut matcher_context, 100, 0, req_id, lp_account_id, 0, 100);

    let before_market = market.data.clone();
    let before_a = account_a.data.clone();
    let before_b = account_b.data.clone();
    let rejected = run_ix(
        Instruction::TradeCpi {
            asset_index: 0,
            size_q: POS_SCALE as i128,
            fee_bps: 10_001,
            limit_price: 0,
        },
        &mut [
            &mut owner_a,
            &mut owner_b,
            &mut market,
            &mut account_a,
            &mut account_b,
            &mut matcher_program,
            &mut matcher_context,
            &mut delegate,
        ],
    );
    assert_err_and_market_unchanged(rejected, &market, &before_market);
    assert_eq!(account_a.data, before_a);
    assert_eq!(account_b.data, before_b);
}

#[test]
fn v16_wrapper_permissionless_crank_advances_account_local_market_progress() {
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
fn v16_wrapper_permissionless_crank_does_not_require_owner_signature() {
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
fn v16_wrapper_permissionless_crank_rejects_stale_now_without_mutation() {
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
fn v16_wrapper_permissionless_crank_can_liquidate_unhealthy_candidate() {
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
    assert!(
        percolator::active_bitmap_is_empty(short.active_bitmap),
        "liquidation should close the unhealthy short through the public crank path"
    );
}

#[test]
fn v16_wrapper_liquidation_uses_configured_fee_not_permissionless_caller_fee() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();

    run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                maintenance_margin_bps,
                initial_margin_bps,
                min_nonzero_mm_req,
                min_nonzero_im_req,
                liquidation_fee_bps,
                liquidation_fee_cap,
                max_price_move_bps_per_slot,
                max_accrual_dt_slots,
                max_abs_funding_e9_per_slot,
                min_funding_lifetime_slots,
                ..
            } = ix
            {
                *maintenance_margin_bps = 500;
                *initial_margin_bps = 600;
                *min_nonzero_mm_req = 100;
                *min_nonzero_im_req = 101;
                *liquidation_fee_bps = 100;
                *liquidation_fee_cap = percolator::MAX_PROTOCOL_FEE_ABS;
                *max_price_move_bps_per_slot = 3;
                *max_accrual_dt_slots = 100;
                *max_abs_funding_e9_per_slot = 10_000;
                *min_funding_lifetime_slots = 100;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 1_000_000);
    deposit(&mut short_owner, &mut market, &mut short_account, 101);
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
    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        let mut short = state::read_portfolio(&short_account.data).unwrap();
        short.capital = 99;
        group.c_tot -= 2;
        group.vault -= 2;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut short_account.data, &short).unwrap();
    }

    run_ix(
        Instruction::PermissionlessCrank {
            action: 1,
            asset_index: 0,
            now_slot: 1,
            effective_price: 100,
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
    assert!(percolator::active_bitmap_is_empty(short.active_bitmap));
    assert_eq!(
        group.insurance, 1,
        "1% configured liquidation fee on 100 notional must be charged even if caller supplies zero"
    );
}

#[test]
fn v16_wrapper_liquidation_fee_policy_splits_retained_penalty_to_cranker() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut cranker_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();
    let mut cranker_account = portfolio_account();

    run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                maintenance_margin_bps,
                initial_margin_bps,
                min_nonzero_mm_req,
                min_nonzero_im_req,
                liquidation_fee_bps,
                liquidation_fee_cap,
                max_price_move_bps_per_slot,
                max_accrual_dt_slots,
                max_abs_funding_e9_per_slot,
                min_funding_lifetime_slots,
                ..
            } = ix
            {
                *maintenance_margin_bps = 500;
                *initial_margin_bps = 600;
                *min_nonzero_mm_req = 100;
                *min_nonzero_im_req = 101;
                *liquidation_fee_bps = 100;
                *liquidation_fee_cap = percolator::MAX_PROTOCOL_FEE_ABS;
                *max_price_move_bps_per_slot = 3;
                *max_accrual_dt_slots = 100;
                *max_abs_funding_e9_per_slot = 10_000;
                *min_funding_lifetime_slots = 100;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    init_portfolio(&mut cranker_owner, &mut market, &mut cranker_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 1_000_000);
    deposit(&mut short_owner, &mut market, &mut short_account, 601);
    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: (100 * POS_SCALE) as i128,
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
    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        let mut short = state::read_portfolio(&short_account.data).unwrap();
        short.capital = 499;
        group.c_tot -= 102;
        group.vault -= 102;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut short_account.data, &short).unwrap();
    }
    run_ix(
        Instruction::UpdateLiquidationFeePolicy {
            cranker_share_bps: 4_000,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();

    let vault_before = state::read_market(&market.data).unwrap().1.vault;
    run_ix(
        Instruction::PermissionlessCrank {
            action: 1,
            asset_index: 0,
            now_slot: 1,
            effective_price: 100,
            funding_rate_e9: 0,
            close_q: 100 * POS_SCALE,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [
            &mut cranker_owner,
            &mut market,
            &mut short_account,
            &mut cranker_account,
        ],
    )
    .unwrap();

    let (_, group) = state::read_market(&market.data).unwrap();
    let short = state::read_portfolio(&short_account.data).unwrap();
    let cranker = state::read_portfolio(&cranker_account.data).unwrap();
    assert!(percolator::active_bitmap_is_empty(short.active_bitmap));
    assert_eq!(
        group.insurance, 60,
        "1% liquidation fee on 10,000 notional is 100; 40% retained-fee share pays 40 to the cranker"
    );
    assert_eq!(
        group.vault, vault_before,
        "internal cranker reward must not withdraw SPL custody from the vault"
    );
    assert_eq!(
        cranker.capital, 40,
        "cranker reward is credited as Percolator account capital"
    );
}

#[test]
fn v16_wrapper_liquidation_reward_account_is_optional_and_absent_keeps_fee_in_insurance() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();

    run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                maintenance_margin_bps,
                initial_margin_bps,
                min_nonzero_mm_req,
                min_nonzero_im_req,
                liquidation_fee_bps,
                liquidation_fee_cap,
                max_price_move_bps_per_slot,
                max_accrual_dt_slots,
                max_abs_funding_e9_per_slot,
                min_funding_lifetime_slots,
                ..
            } = ix
            {
                *maintenance_margin_bps = 500;
                *initial_margin_bps = 600;
                *min_nonzero_mm_req = 100;
                *min_nonzero_im_req = 101;
                *liquidation_fee_bps = 100;
                *liquidation_fee_cap = percolator::MAX_PROTOCOL_FEE_ABS;
                *max_price_move_bps_per_slot = 3;
                *max_accrual_dt_slots = 100;
                *max_abs_funding_e9_per_slot = 10_000;
                *min_funding_lifetime_slots = 100;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 1_000_000);
    deposit(&mut short_owner, &mut market, &mut short_account, 601);
    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: (100 * POS_SCALE) as i128,
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
    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        let mut short = state::read_portfolio(&short_account.data).unwrap();
        short.capital = 499;
        group.c_tot -= 102;
        group.vault -= 102;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut short_account.data, &short).unwrap();
    }
    run_ix(
        Instruction::UpdateLiquidationFeePolicy {
            cranker_share_bps: 4_000,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();

    let vault_before = state::read_market(&market.data).unwrap().1.vault;
    run_ix(
        Instruction::PermissionlessCrank {
            action: 1,
            asset_index: 0,
            now_slot: 1,
            effective_price: 100,
            funding_rate_e9: 0,
            close_q: 100 * POS_SCALE,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut admin, &mut market, &mut short_account],
    )
    .unwrap();

    let (_, group) = state::read_market(&market.data).unwrap();
    let short = state::read_portfolio(&short_account.data).unwrap();
    assert!(percolator::active_bitmap_is_empty(short.active_bitmap));
    assert_eq!(
        group.insurance, 100,
        "without an optional cranker portfolio, the full liquidation penalty remains in insurance"
    );
    assert_eq!(group.vault, vault_before);
}

#[test]
fn v16_wrapper_liquidation_reward_never_spends_insurance_needed_for_losses() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut cranker_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();
    let mut cranker_account = portfolio_account();

    run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                liquidation_fee_bps,
                liquidation_fee_cap,
                ..
            } = ix
            {
                *liquidation_fee_bps = 0;
                *liquidation_fee_cap = 10_000;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    init_portfolio(&mut cranker_owner, &mut market, &mut cranker_account);
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
    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        let mut short = state::read_portfolio(&short_account.data).unwrap();
        short.capital = 0;
        group.c_tot -= 100;
        group.vault -= 100;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut short_account.data, &short).unwrap();
    }
    run_ix(
        Instruction::UpdateLiquidationFeePolicy {
            cranker_share_bps: 4_000,
        },
        &mut [&mut admin, &mut market],
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
        &mut [
            &mut cranker_owner,
            &mut market,
            &mut short_account,
            &mut cranker_account,
        ],
    )
    .unwrap();

    let (_, group) = state::read_market(&market.data).unwrap();
    let short = state::read_portfolio(&short_account.data).unwrap();
    let cranker = state::read_portfolio(&cranker_account.data).unwrap();
    assert!(percolator::active_bitmap_is_empty(short.active_bitmap));
    assert_eq!(
        group.insurance, 0,
        "the liquidation did not increase retained insurance, so no cranker reward is paid"
    );
    assert_eq!(
        group.vault, 1_000_000,
        "loss paths must not leak pre-existing custody out as rewards"
    );
    assert_eq!(
        cranker.capital, 0,
        "no retained-fee growth means no internal cranker reward"
    );
}

#[test]
fn v16_wrapper_liquidation_fee_policy_is_admin_gated_and_bounds_share() {
    let mut admin = signer();
    let mut attacker = signer();
    let mut market = market_account();

    init_market(&mut admin, &mut market);
    let before = market.data.clone();
    let rejected = run_ix(
        Instruction::UpdateLiquidationFeePolicy {
            cranker_share_bps: 10_001,
        },
        &mut [&mut admin, &mut market],
    );
    assert_err_and_market_unchanged(rejected, &market, &before);

    let rejected = run_ix(
        Instruction::UpdateLiquidationFeePolicy {
            cranker_share_bps: 4_000,
        },
        &mut [&mut attacker, &mut market],
    );
    assert_err_and_market_unchanged(rejected, &market, &before);

    run_ix(
        Instruction::UpdateLiquidationFeePolicy {
            cranker_share_bps: 4_000,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    let (cfg, _) = state::read_market(&market.data).unwrap();
    assert_eq!(cfg.liquidation_cranker_fee_share_bps, 4_000);
}

#[test]
fn v16_wrapper_permissionless_settle_b_without_b_state_is_fail_closed() {
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
fn v16_wrapper_permissionless_crank_rejects_invalid_asset_and_price_without_mutation() {
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
fn v16_wrapper_permissionless_recovery_action_is_not_public_kill_switch() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);

    let before_market = market.data.clone();
    let before_portfolio = portfolio.data.clone();
    let result = run_ix(
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
    );

    assert_err_and_market_unchanged(result, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(group.mode, MarketModeV16::Live);
    assert_eq!(
        group.recovery_reason, None,
        "a caller-selected recovery reason must not terminal-lock a healthy market"
    );
}

#[test]
fn v16_wrapper_permissionless_recovery_rejects_every_caller_selected_reason() {
    let reasons = [
        PermissionlessRecoveryReasonV16::BelowProgressFloor,
        PermissionlessRecoveryReasonV16::BlockedSegmentHeadroomOrRepresentability,
        PermissionlessRecoveryReasonV16::AccountBSettlementCannotProgress,
        PermissionlessRecoveryReasonV16::BIndexHeadroomExhausted,
        PermissionlessRecoveryReasonV16::ActiveBankruptCloseCannotProgress,
        PermissionlessRecoveryReasonV16::ExplicitLossOrDustAuditOverflow,
        PermissionlessRecoveryReasonV16::OracleOrTargetUnavailableByAuthenticatedPolicy,
        PermissionlessRecoveryReasonV16::CounterOrEpochOverflowDeclaredRecovery,
    ];

    for (reason, _) in reasons.iter().copied().enumerate() {
        let mut admin = signer();
        let mut market = market_account();
        let mut owner = signer();
        let mut portfolio = portfolio_account();
        init_market(&mut admin, &mut market);
        init_portfolio(&mut owner, &mut market, &mut portfolio);

        let before_market = market.data.clone();
        let before_portfolio = portfolio.data.clone();
        let result = run_ix(
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
        );
        assert_err_and_market_unchanged(result, &market, &before_market);
        assert_eq!(portfolio.data, before_portfolio);
        let (_, group) = state::read_market(&market.data).unwrap();
        assert_eq!(group.mode, MarketModeV16::Live);
        assert_eq!(group.recovery_reason, None);
    }
}

#[test]
fn v16_wrapper_permissionless_crank_rejects_invalid_action_and_recovery_reason() {
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
fn v16_wrapper_permissionless_crank_rejects_caller_supplied_funding_rate() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_abs_funding_e9_per_slot,
                max_price_move_bps_per_slot,
                ..
            } = ix
            {
                *max_abs_funding_e9_per_slot = 1;
                *max_price_move_bps_per_slot = 4_999;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();
    init_portfolio(&mut owner, &mut market, &mut portfolio);

    let before_market = market.data.clone();
    let before_portfolio = portfolio.data.clone();
    let result = run_ix(
        Instruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 1,
            effective_price: 100,
            funding_rate_e9: 1,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut owner, &mut market, &mut portfolio],
    );
    assert_err_and_market_unchanged(result, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);
}

#[test]
fn v16_wrapper_permissionless_recovery_declares_below_progress_floor_when_proven() {
    let mut admin = signer();
    let mut market = market_account();
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();

    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                initial_price,
                max_price_move_bps_per_slot,
                max_trading_fee_bps,
                ..
            } = ix
            {
                *initial_price = 1;
                *max_price_move_bps_per_slot = 1;
                *max_trading_fee_bps = 10_000;
            }
        }),
    );
    run_ix(
        Instruction::ConfigureHyperpMark {
            asset_index: 0,
            now_slot: 0,
            initial_mark_e6: 1,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 1_000);
    deposit(&mut short_owner, &mut market, &mut short_account, 1_000);
    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: POS_SCALE as i128,
            exec_price: 1,
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
        Instruction::PushHyperpMark {
            asset_index: 0,
            now_slot: 1,
            mark_e6: 3,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();

    run_ix(
        Instruction::PermissionlessCrank {
            action: 3,
            asset_index: 0,
            now_slot: 1,
            effective_price: 1,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut long_owner, &mut market, &mut long_account],
    )
    .unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(group.mode, MarketModeV16::Recovery);
    assert_eq!(
        group.recovery_reason,
        Some(PermissionlessRecoveryReasonV16::BelowProgressFloor)
    );
}

#[test]
fn v16_wrapper_rebalance_reduce_is_owner_signed_and_strictly_reduces_risk() {
    let mut admin = signer();
    let mut market = market_account();
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut attacker = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 10_000);
    deposit(&mut short_owner, &mut market, &mut short_account, 10_000);
    run_ix(
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
    )
    .unwrap();

    let before_market = market.data.clone();
    let before_account = long_account.data.clone();
    let unauthorized = run_ix(
        Instruction::RebalanceReduce {
            asset_index: 0,
            reduce_q: POS_SCALE,
        },
        &mut [&mut attacker, &mut market, &mut long_account],
    );
    assert_err_and_market_unchanged(unauthorized, &market, &before_market);
    assert_eq!(long_account.data, before_account);

    run_ix(
        Instruction::RebalanceReduce {
            asset_index: 0,
            reduce_q: POS_SCALE,
        },
        &mut [&mut long_owner, &mut market, &mut long_account],
    )
    .unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    let long = state::read_portfolio(&long_account.data).unwrap();
    assert_eq!(long.legs[0].basis_pos_q, POS_SCALE as i128);
    assert_eq!(group.assets[0].oi_eff_long_q, POS_SCALE);
}

#[test]
fn v16_wrapper_dead_leg_forfeit_is_owner_signed_and_detaches_recovery_leg() {
    let mut admin = signer();
    let mut market = market_account();
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut attacker = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 10_000);
    deposit(&mut short_owner, &mut market, &mut short_account, 10_000);
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
    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        group.mode = MarketModeV16::Recovery;
        group.recovery_reason = Some(PermissionlessRecoveryReasonV16::BelowProgressFloor);
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }

    let before_market = market.data.clone();
    let before_account = long_account.data.clone();
    let unauthorized = run_ix(
        Instruction::ForfeitRecoveryLeg {
            asset_index: 0,
            b_delta_budget: 1,
        },
        &mut [&mut attacker, &mut market, &mut long_account],
    );
    assert_err_and_market_unchanged(unauthorized, &market, &before_market);
    assert_eq!(long_account.data, before_account);

    run_ix(
        Instruction::ForfeitRecoveryLeg {
            asset_index: 0,
            b_delta_budget: 1,
        },
        &mut [&mut long_owner, &mut market, &mut long_account],
    )
    .unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    let long = state::read_portfolio(&long_account.data).unwrap();
    assert!(percolator::active_bitmap_is_empty(long.active_bitmap));
    assert_eq!(long.legs[0].basis_pos_q, 0);
    assert_eq!(group.assets[0].oi_eff_long_q, 0);
}

#[test]
fn v16_wrapper_cure_and_cancel_close_uses_owner_deposit_before_support_is_consumed() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();
    let mint = init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        let mut account = state::read_portfolio(&portfolio.data).unwrap();
        account.close_progress = CloseProgressLedgerV16 {
            active: true,
            finalized: false,
            canceled: false,
            close_id: 1,
            asset_index: 0,
            market_id: group.assets[0].market_id,
            domain_side: SideV16::Long,
            gross_loss_at_close_start: 10,
            drift_reference_slot: 0,
            max_close_slot: 10,
            residual_remaining: 10,
            ..CloseProgressLedgerV16::EMPTY
        };
        group.pending_domain_loss_barriers[0] = 1;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut portfolio.data, &account).unwrap();
    }
    let mut source = user_token_account(owner.key, mint, 20);
    let mut vault = vault_token_account(&market, mint, 20);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::CureAndCancelClose {
            optional_deposit: 20,
        },
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
    let (_, group) = state::read_market(&market.data).unwrap();
    let account = state::read_portfolio(&portfolio.data).unwrap();
    assert!(account.close_progress.canceled);
    assert_eq!(account.capital, 20);
    assert_eq!(group.c_tot, 20);
    assert_eq!(group.vault, 20);
    assert_eq!(group.pending_domain_loss_barriers[0], 0);
}

#[test]
fn v16_wrapper_finalize_reset_side_is_permissionless_but_validates_side_and_readiness() {
    let mut admin = signer();
    let mut market = market_account();
    init_market(&mut admin, &mut market);
    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        group.assets[0].mode_long = SideModeV16::ResetPending;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }

    let bad_side = run_ix(
        Instruction::FinalizeResetSide {
            asset_index: 0,
            side: 2,
        },
        &mut [&mut market],
    );
    assert!(bad_side.is_err());

    run_ix(
        Instruction::FinalizeResetSide {
            asset_index: 0,
            side: 0,
        },
        &mut [&mut market],
    )
    .unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(group.assets[0].mode_long, SideModeV16::Normal);
}

#[test]
fn v16_wrapper_claim_resolved_payout_topup_pays_only_stored_owner_receipt() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut attacker_owner = signer();
    let mut portfolio = portfolio_account();
    let mint = init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        let mut account = state::read_portfolio(&portfolio.data).unwrap();
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
        account.resolved_payout_receipt = ResolvedPayoutReceiptV16 {
            present: true,
            prior_bound_contribution_num: 100 * BOUND_SCALE,
            live_released_face_at_receipt: 0,
            terminal_positive_claim_face: 100,
            paid_effective: 40,
            finalized: false,
        };
        state::write_market(&mut market.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut portfolio.data, &account).unwrap();
    }
    let mut dest = user_token_account(owner.key, mint, 0);
    let mut vault = vault_token_account(&market, mint, 60);
    let mut vault_auth = vault_authority_account(&market);
    let mut token_program = token_program_account();

    let before_market = market.data.clone();
    let before_portfolio = portfolio.data.clone();
    let wrong_owner = run_ix(
        Instruction::ClaimResolvedPayoutTopup,
        &mut [
            &mut attacker_owner,
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

    run_ix(
        Instruction::ClaimResolvedPayoutTopup,
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
    let (_, group) = state::read_market(&market.data).unwrap();
    let account = state::read_portfolio(&portfolio.data).unwrap();
    assert_eq!(group.vault, 0);
    assert_eq!(account.resolved_payout_receipt.paid_effective, 100);
    assert!(account.resolved_payout_receipt.finalized);
}

#[test]
fn v16_wrapper_refine_resolved_unreceipted_bound_is_admin_only_and_monotonic() {
    let mut admin = signer();
    let mut attacker = signer();
    let mut market = market_account();
    init_market(&mut admin, &mut market);
    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        group.mode = MarketModeV16::Resolved;
        group.resolved_slot = 1;
        group.current_slot = 1;
        group.payout_snapshot_captured = true;
        group.payout_snapshot = 100;
        group.resolved_payout_ledger = ResolvedPayoutLedgerV16 {
            snapshot_residual: 100,
            terminal_claim_exact_receipts_num: 0,
            terminal_claim_bound_unreceipted_num: 100 * BOUND_SCALE,
            current_payout_rate_num: 100 * BOUND_SCALE,
            current_payout_rate_den: 100 * BOUND_SCALE,
            snapshot_slot: 1,
            payout_halted: false,
            finalized: false,
        };
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }

    let before = market.data.clone();
    let unauthorized = run_ix(
        Instruction::RefineResolvedUnreceiptedBound {
            decrease_num: 10 * BOUND_SCALE,
        },
        &mut [&mut attacker, &mut market],
    );
    assert_err_and_market_unchanged(unauthorized, &market, &before);

    run_ix(
        Instruction::RefineResolvedUnreceiptedBound {
            decrease_num: 10 * BOUND_SCALE,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(
        group
            .resolved_payout_ledger
            .terminal_claim_bound_unreceipted_num,
        90 * BOUND_SCALE
    );
    assert_eq!(
        group.resolved_payout_ledger.current_payout_rate_num,
        90 * BOUND_SCALE
    );
    assert_eq!(
        group.resolved_payout_ledger.current_payout_rate_den,
        90 * BOUND_SCALE
    );
}

#[test]
fn v16_wrapper_resolve_market_is_admin_only_and_blocks_live_trade() {
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
fn v16_wrapper_permissionless_stale_resolve_requires_hard_stale_maturity() {
    let mut admin = signer();
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
    run_ix(
        Instruction::ConfigurePermissionlessResolve {
            stale_slots: 5,
            force_close_delay_slots: 1,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();

    let before = market.data.clone();
    let early = run_ix(
        Instruction::ResolveStalePermissionless { now_slot: 4 },
        &mut [&mut market],
    );
    assert_err_and_market_unchanged(early, &market, &before);

    run_ix(
        Instruction::ResolveStalePermissionless { now_slot: 5 },
        &mut [&mut market],
    )
    .unwrap();
    let (cfg, group) = state::read_market(&market.data).unwrap();
    assert_eq!(cfg.last_good_oracle_slot, 0);
    assert_eq!(group.mode, MarketModeV16::Resolved);

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
fn v16_wrapper_permissionless_stale_resolve_uses_stamped_liveness_not_oracle_tail() {
    let feed = [11u8; 32];
    let mut admin = signer();
    let mut market = market_account();
    let mut fresh_oracle = pyth_account(&feed, 100, 0, 0, 1_000);
    let mut stale_oracle = pyth_account(&feed, 100, 0, 0, 1);

    init_market(&mut admin, &mut market);
    run_ix(
        Instruction::ConfigureHybridOracle {
            asset_index: 0,
            now_slot: 10,
            now_unix_ts: 1_000,
            oracle_leg_count: 1,
            oracle_leg_flags: 0,
            max_staleness_secs: 60,
            hybrid_soft_stale_slots: 2,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            invert: 0,
            unit_scale: 0,
            conf_filter_bps: 500,
            oracle_leg_feeds: [feed, [0u8; 32], [0u8; 32]],
        },
        &mut [&mut admin, &mut market, &mut fresh_oracle],
    )
    .unwrap();
    run_ix(
        Instruction::ConfigurePermissionlessResolve {
            stale_slots: 5,
            force_close_delay_slots: 1,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();

    let before = market.data.clone();
    let caller_chosen_stale_tail = run_ix(
        Instruction::ResolveStalePermissionless { now_slot: 14 },
        &mut [&mut market, &mut stale_oracle],
    );
    assert_err_and_market_unchanged(caller_chosen_stale_tail, &market, &before);

    run_ix(
        Instruction::ResolveStalePermissionless { now_slot: 15 },
        &mut [&mut market, &mut stale_oracle],
    )
    .unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(group.mode, MarketModeV16::Resolved);
    assert_eq!(group.resolved_slot, 15);
}

#[test]
fn v16_wrapper_permissionless_resolve_maturity_blocks_manual_live_trade_race() {
    let mut admin = signer();
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
    run_ix(
        Instruction::ConfigurePermissionlessResolve {
            stale_slots: 5,
            force_close_delay_slots: 1,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        group.current_slot = 5;
        group.slot_last = 5;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }

    let before_market = market.data.clone();
    let before_a = portfolio_a.data.clone();
    let before_b = portfolio_b.data.clone();
    let trade_after_resolve_maturity = run_ix(
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
    assert_err_and_market_unchanged(trade_after_resolve_maturity, &market, &before_market);
    assert_eq!(portfolio_a.data, before_a);
    assert_eq!(portfolio_b.data, before_b);

    let mut late_owner = signer();
    let mut late_portfolio = portfolio_account();
    let late_init = run_ix(
        Instruction::InitPortfolio,
        &mut [&mut late_owner, &mut market, &mut late_portfolio],
    );
    assert_err_and_market_unchanged(late_init, &market, &before_market);
    assert!(
        late_portfolio.data.iter().all(|b| *b == 0),
        "terminal-resolvable live market must not admit new empty accounts that can grief final cleanup"
    );

    let crank_after_resolve_maturity = run_ix(
        Instruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 5,
            effective_price: 100,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut owner_a, &mut market, &mut portfolio_a],
    );
    assert_err_and_market_unchanged(crank_after_resolve_maturity, &market, &before_market);
    assert_eq!(portfolio_a.data, before_a);

    run_ix(
        Instruction::ResolveStalePermissionless { now_slot: 5 },
        &mut [&mut market],
    )
    .unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(group.mode, MarketModeV16::Resolved);
}

#[test]
fn v16_wrapper_resolved_market_blocks_new_activity_and_double_resolution() {
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
fn v16_wrapper_resolved_close_uses_engine_loss_and_fee_ordering_path() {
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
    assert_eq!(group.mode, MarketModeV16::Resolved);
    assert_eq!(acct.capital, 0);
    assert_eq!(group.vault, 0);
}

#[test]
fn v16_wrapper_close_resolved_uses_configured_fee_not_permissionless_caller_fee() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 1_000);
    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        group.current_slot = 10;
        group.slot_last = 10;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }
    run_ix(Instruction::ResolveMarket, &mut [&mut admin, &mut market]).unwrap();

    close_resolved(&mut owner, &mut market, &mut portfolio, 100);

    let (_, group) = state::read_market(&market.data).unwrap();
    let acct = state::read_portfolio(&portfolio.data).unwrap();
    assert_eq!(
        group.insurance, 0,
        "permissionless resolved close must not let caller redirect payout into fees"
    );
    assert_eq!(group.vault, 0);
    assert_eq!(acct.capital, 0);
}

#[test]
fn v16_wrapper_close_resolved_does_not_double_pay_after_closed_payout() {
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
fn v16_wrapper_close_resolved_is_permissionless_but_pays_only_owner_token_account() {
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
fn v16_wrapper_close_resolved_rejects_before_resolution_without_mutation() {
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
fn v16_wrapper_close_resolved_progress_only_does_not_pay_active_position() {
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
    assert!(!percolator::active_bitmap_is_empty(long.active_bitmap));
    assert_eq!(long.capital, 1_000_000);
}

#[test]
fn v16_wrapper_close_resolved_progress_only_does_not_require_token_accounts() {
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
    run_ix(Instruction::ResolveMarket, &mut [&mut admin, &mut market]).unwrap();

    run_ix(
        Instruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        &mut [&mut long_owner, &mut market, &mut long_account],
    )
    .unwrap();

    let (_, group) = state::read_market(&market.data).unwrap();
    let long = state::read_portfolio(&long_account.data).unwrap();
    assert_eq!(
        group.vault, 2_000_000,
        "progress-only resolved close must not require or perform token payout"
    );
    assert!(!percolator::active_bitmap_is_empty(long.active_bitmap));
    assert_eq!(long.capital, 1_000_000);
}

#[test]
fn v16_wrapper_close_resolved_requires_recipient_and_vault_accounts_for_payout() {
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

#[test]
fn v16_wrapper_hybrid_hard_stale_uses_permissionless_resolve_not_recovery_kill_switch() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_trading_fee_bps,
                trade_fee_base_bps,
                max_price_move_bps_per_slot,
                ..
            } = ix
            {
                *max_trading_fee_bps = 10_000;
                *trade_fee_base_bps = 1;
                *max_price_move_bps_per_slot = 500;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();
    let feeds = [[0xc1u8; 32], [0xc2u8; 32], [0xc3u8; 32]];
    let mut toto_jpy = pyth_account(&feeds[0], 4_000_000_000, -6, 1, 100);
    let mut usd_jpy = pyth_account(&feeds[1], 150_000_000, -6, 1, 100);
    let mut sol_usd = pyth_account(&feeds[2], 200_000_000, -6, 1, 100);
    configure_three_leg_hybrid(
        &mut admin,
        &mut market,
        feeds,
        &mut toto_jpy,
        &mut usd_jpy,
        &mut sol_usd,
        1,
        100,
        3,
        0,
    );
    run_ix(
        Instruction::ConfigurePermissionlessResolve {
            stale_slots: 4,
            force_close_delay_slots: 1,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    let mut owner = signer();
    let mut portfolio = portfolio_account();
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    {
        let (mut cfg, mut group) = state::read_market(&market.data).unwrap();
        cfg.last_good_oracle_slot = 1;
        cfg.oracle_target_publish_time = 100;
        group.current_slot = 1;
        group.slot_last = 1;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }

    let before_recovery = market.data.clone();
    let rejected_recovery = run_ix(
        Instruction::PermissionlessCrank {
            action: 3,
            asset_index: 0,
            now_slot: 5,
            effective_price: 133_333,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 6,
        },
        &mut [&mut owner, &mut market, &mut portfolio],
    );
    assert_err_and_market_unchanged(rejected_recovery, &market, &before_recovery);

    run_ix(
        Instruction::ResolveStalePermissionless { now_slot: 5 },
        &mut [&mut market],
    )
    .unwrap();

    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(
        group.mode,
        MarketModeV16::Resolved,
        "hard-stale hybrid markets exit through the proof-free stamped stale resolver"
    );
}

#[test]
fn v16_wrapper_hybrid_hard_stale_blocks_live_value_movement_until_resolved() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    let mint_key = mint.key;
    run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_trading_fee_bps,
                trade_fee_base_bps,
                max_price_move_bps_per_slot,
                ..
            } = ix
            {
                *max_trading_fee_bps = 10_000;
                *trade_fee_base_bps = 1;
                *max_price_move_bps_per_slot = 500;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();

    let feeds = [[0xd1u8; 32], [0xd2u8; 32], [0xd3u8; 32]];
    let mut toto_jpy = pyth_account(&feeds[0], 4_000_000_000, -6, 1, 100);
    let mut usd_jpy = pyth_account(&feeds[1], 150_000_000, -6, 1, 100);
    let mut sol_usd = pyth_account(&feeds[2], 200_000_000, -6, 1, 100);
    configure_three_leg_hybrid(
        &mut admin,
        &mut market,
        feeds,
        &mut toto_jpy,
        &mut usd_jpy,
        &mut sol_usd,
        1,
        100,
        3,
        0,
    );
    run_ix(
        Instruction::ConfigurePermissionlessResolve {
            stale_slots: 4,
            force_close_delay_slots: 1,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();

    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 1_000_000);
    deposit(&mut short_owner, &mut market, &mut short_account, 1_000_000);
    {
        let (mut cfg, mut group) = state::read_market(&market.data).unwrap();
        cfg.last_good_oracle_slot = 1;
        cfg.oracle_target_publish_time = 100;
        group.current_slot = 5;
        group.slot_last = 5;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }

    let before_market = market.data.clone();
    let before_long = long_account.data.clone();
    let before_short = short_account.data.clone();
    let trade_after_hard_stale = run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: POS_SCALE as i128,
            exec_price: 133_333,
            fee_bps: 10_000,
        },
        &mut [
            &mut long_owner,
            &mut short_owner,
            &mut market,
            &mut long_account,
            &mut short_account,
        ],
    );
    assert_err_and_market_unchanged(trade_after_hard_stale, &market, &before_market);
    assert_eq!(long_account.data, before_long);
    assert_eq!(short_account.data, before_short);

    let mut dest = user_token_account(long_owner.key, mint_key, 0);
    let mut vault = vault_token_account(&market, mint_key, 2_000_000);
    let mut vault_auth = vault_authority_account(&market);
    let mut token_program = token_program_account();
    let withdraw_after_hard_stale = run_ix(
        Instruction::Withdraw { amount: 1 },
        &mut [
            &mut long_owner,
            &mut market,
            &mut long_account,
            &mut dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(withdraw_after_hard_stale, &market, &before_market);
    assert_eq!(long_account.data, before_long);

    run_ix(
        Instruction::ResolveStalePermissionless { now_slot: 5 },
        &mut [&mut market],
    )
    .unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(group.mode, MarketModeV16::Resolved);
}
