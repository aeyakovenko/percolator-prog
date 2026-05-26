use percolator::{
    AssetLifecycleV16, AssetStateV16Account, BackingBucketStatusV16, CloseProgressLedgerV16,
    EngineAssetSlotV16Account, MarketGroupV16, MarketGroupV16HeaderAccount, MarketModeV16,
    PermissionlessRecoveryReasonV16, PortfolioAccountV16Account, PortfolioLegV16,
    ResolvedPayoutLedgerV16, ResolvedPayoutReceiptV16, SideModeV16, SideV16, V16Config,
    BOUND_SCALE, POS_SCALE,
};
use percolator_prog::{
    constants::{
        ASSET_ORACLE_WRAPPER_LEN, DEFAULT_MARKET_SLOT_CAPACITY, HEADER_LEN, MARKET_ACCOUNT_LEN,
        MARKET_ASSET_SLOT_LEN, MARKET_GROUP_LEN, ORACLE_LEG_CAP, ORACLE_LEG_FLAG_DIVIDE_LEG2,
        ORACLE_LEG_FLAG_DIVIDE_LEG3, ORACLE_MODE_AUTH_MARK, ORACLE_MODE_EWMA_MARK,
        ORACLE_MODE_HYBRID_AFTER_HOURS, ORACLE_MODE_MANUAL, PORTFOLIO_ACCOUNT_LEN,
        PORTFOLIO_SOURCE_DOMAIN_LEN, PORTFOLIO_STATE_LEN, WRAPPER_CONFIG_LEN,
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

fn has_active_leg_for_asset(account: &percolator::PortfolioAccountV16, asset_index: usize) -> bool {
    account
        .legs
        .iter()
        .any(|leg| leg.active && leg.asset_index as usize == asset_index)
}

fn signer() -> TestAccount {
    TestAccount::new(Pubkey::new_unique(), Pubkey::new_unique(), 0).signer()
}

fn market_account() -> TestAccount {
    market_account_with_capacity(percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS as usize)
}

fn market_account_with_capacity(capacity: usize) -> TestAccount {
    TestAccount::new(
        Pubkey::new_unique(),
        program_id(),
        state::market_account_len_for_capacity(capacity).unwrap(),
    )
    .writable()
}

fn portfolio_account() -> TestAccount {
    portfolio_account_for_market_slots(
        percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS as usize,
    )
}

fn portfolio_account_for_market_slots(max_market_slots: usize) -> TestAccount {
    TestAccount::new(
        Pubkey::new_unique(),
        program_id(),
        state::portfolio_account_len_for_market_slots(max_market_slots).unwrap(),
    )
    .writable()
}

fn backing_domain_ledger_account() -> TestAccount {
    TestAccount::new(
        Pubkey::new_unique(),
        program_id(),
        state::backing_domain_ledger_account_len(),
    )
    .writable()
}

fn insurance_ledger_account() -> TestAccount {
    TestAccount::new(
        Pubkey::new_unique(),
        program_id(),
        state::insurance_ledger_account_len(),
    )
    .writable()
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

fn run_ix_data(data: &[u8], accounts: &mut [&mut TestAccount]) -> Result<(), ProgramError> {
    let snapshots: Vec<(u64, Vec<u8>)> = accounts
        .iter()
        .map(|a| (a.lamports, a.data.clone()))
        .collect();
    let result = {
        let infos: Vec<AccountInfo> = accounts.iter_mut().map(|a| a.to_info()).collect();
        processor::process_instruction(&program_id(), &infos, data)
    };
    if result.is_err() {
        for (account, (lamports, data)) in accounts.iter_mut().zip(snapshots) {
            account.lamports = lamports;
            account.data = data;
        }
    }
    result
}

fn run_ix(ix: Instruction, accounts: &mut [&mut TestAccount]) -> Result<(), ProgramError> {
    run_ix_data(&ix.encode(), accounts)
}

fn configure_base_ewma_mark(
    admin: &mut TestAccount,
    market: &mut TestAccount,
    now_slot: u64,
    mark_e6: u64,
) {
    run_ix(
        Instruction::ConfigureEwmaMark {
            asset_index: 0,
            now_slot,
            initial_mark_e6: mark_e6,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
        },
        &mut [admin, market],
    )
    .unwrap();
}

fn push_base_ewma_mark(
    admin: &mut TestAccount,
    market: &mut TestAccount,
    now_slot: u64,
    mark_e6: u64,
) {
    run_ix(
        Instruction::PushEwmaMark {
            asset_index: 0,
            now_slot,
            mark_e6,
        },
        &mut [admin, market],
    )
    .unwrap();
}

fn configure_base_auth_mark(
    admin: &mut TestAccount,
    market: &mut TestAccount,
    now_slot: u64,
    mark_e6: u64,
) {
    run_ix(
        Instruction::ConfigureAuthMark {
            asset_index: 0,
            now_slot,
            initial_mark_e6: mark_e6,
        },
        &mut [admin, market],
    )
    .unwrap();
}

fn push_base_auth_mark(
    admin: &mut TestAccount,
    market: &mut TestAccount,
    now_slot: u64,
    mark_e6: u64,
) {
    run_ix(
        Instruction::PushAuthMark {
            asset_index: 0,
            now_slot,
            mark_e6,
        },
        &mut [admin, market],
    )
    .unwrap();
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
        max_bankrupt_close_lifetime_slots: 100,
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

fn configure_base_unit_mints(
    authority: &mut TestAccount,
    market: &mut TestAccount,
    primary_mint: &mut TestAccount,
    secondary_mint: &mut TestAccount,
) -> Result<(), ProgramError> {
    run_ix(
        Instruction::UpdateBaseUnitMints {
            primary_mint: primary_mint.key.to_bytes(),
            secondary_mint: secondary_mint.key.to_bytes(),
        },
        &mut [authority, market, primary_mint, secondary_mint],
    )
}

fn update_asset_lifecycle(
    authority: &mut TestAccount,
    market: &mut TestAccount,
    action: u8,
    asset_index: u16,
    now_slot: u64,
    initial_price: u64,
) -> Result<(), ProgramError> {
    update_asset_lifecycle_with_authorities(
        authority,
        market,
        action,
        asset_index,
        now_slot,
        initial_price,
        authority.key.to_bytes(),
        authority.key.to_bytes(),
        authority.key.to_bytes(),
    )
}

fn update_asset_lifecycle_with_authorities(
    authority: &mut TestAccount,
    market: &mut TestAccount,
    action: u8,
    asset_index: u16,
    now_slot: u64,
    initial_price: u64,
    insurance_authority: [u8; 32],
    insurance_operator: [u8; 32],
    backing_bucket_authority: [u8; 32],
) -> Result<(), ProgramError> {
    run_ix(
        Instruction::UpdateAssetLifecycle {
            action,
            asset_index,
            now_slot,
            initial_price,
            insurance_authority,
            insurance_operator,
            backing_bucket_authority,
            oracle_authority: backing_bucket_authority,
        },
        &mut [authority, market],
    )
}

fn force_close_abandoned_asset(
    cranker: &mut TestAccount,
    market: &mut TestAccount,
    account_a: &mut TestAccount,
    account_b: &mut TestAccount,
    asset_index: u16,
    now_slot: u64,
    close_q: u128,
) -> Result<(), ProgramError> {
    run_ix(
        Instruction::ForceCloseAbandonedAsset {
            asset_index,
            now_slot,
            close_q,
        },
        &mut [cranker, market, account_a, account_b],
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

fn top_up_backing_bucket(
    authority: &mut TestAccount,
    market: &mut TestAccount,
    domain: u8,
    amount: u128,
    expiry_slot: u64,
) {
    let mint = Pubkey::new_from_array(state::read_market(&market.data).unwrap().0.collateral_mint);
    let amount_u64 = u64::try_from(amount).unwrap();
    let mut source = user_token_account(authority.key, mint, amount_u64);
    let mut vault = vault_token_account(market, mint, 0);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::TopUpBackingBucket {
            domain,
            amount,
            expiry_slot,
        },
        &mut [
            authority,
            market,
            &mut source,
            &mut vault,
            &mut token_program,
        ],
    )
    .unwrap();
}

fn add_source_positive_pnl(
    market: &mut TestAccount,
    portfolio: &mut TestAccount,
    domain: usize,
    amount: u128,
) {
    let (cfg, mut group) = state::read_market(&market.data).unwrap();
    let mut account = state::read_portfolio(&portfolio.data).unwrap();
    group
        .add_account_source_positive_pnl_not_atomic(&mut account, domain, amount)
        .unwrap();
    state::write_market(&mut market.data, &cfg, &group).unwrap();
    state::write_portfolio(&mut portfolio.data, &account).unwrap();
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

fn seed_cancellable_close_progress(market: &mut TestAccount, portfolio: &mut TestAccount) {
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
    cfg.max_bankrupt_close_lifetime_slots = 100;
    let mut expected = MarketGroupV16::new(market.key.to_bytes(), cfg).unwrap();
    expected.assets[0].raw_oracle_target_price = 100;
    expected.assets[0].effective_price = 100;
    expected.assets[0].fund_px_last = 100;
    expected.materialized_portfolio_count = 1;
    assert_eq!(group.config, expected.config);
    assert_eq!(group.materialized_portfolio_count, 1);
    assert_eq!(
        group.assets[0], expected.assets[0],
        "configured asset must match canonical engine init shape"
    );
    assert_eq!(
        &group.insurance_domain_budget[..2],
        &[0u128, 0u128],
        "configured domain budgets start empty and are funded explicitly"
    );
    assert_eq!(
        &group.source_backing_buckets[..2],
        &expected.source_backing_buckets[..],
        "configured backing buckets must match canonical engine init shape"
    );
    assert!(
        group.assets[1..]
            .iter()
            .all(|asset| asset.market_id == 0 && asset.lifecycle == AssetLifecycleV16::Disabled),
        "extra account capacity must decode as disabled, unreusable slots"
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
                max_bankrupt_close_lifetime_slots,
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
                *max_bankrupt_close_lifetime_slots = 50;
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
    assert_eq!(group.config.max_bankrupt_close_lifetime_slots, 50);
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
        Instruction::ConfigureEwmaMark {
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
fn v16_wrapper_trade_fee_policy_is_insurance_authority_gated_and_bounds_fee() {
    let mut admin = signer();
    let mut attacker = signer();
    let mut insurance_authority = signer();
    let mut market = market_account();
    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_trading_fee_bps,
                trade_fee_base_bps,
                ..
            } = ix
            {
                *max_trading_fee_bps = 100;
                *trade_fee_base_bps = 1;
            }
        }),
    );
    run_ix(
        Instruction::UpdateAuthority {
            kind: processor::AUTHORITY_INSURANCE,
            new_pubkey: insurance_authority.key.to_bytes(),
        },
        &mut [&mut admin, &mut insurance_authority, &mut market],
    )
    .unwrap();
    let before = market.data.clone();

    let rejected_attacker = run_ix(
        Instruction::UpdateTradeFeePolicy {
            trade_fee_base_bps: 2,
        },
        &mut [&mut attacker, &mut market],
    );
    assert_err_and_market_unchanged(rejected_attacker, &market, &before);

    let rejected_admin_after_rotation = run_ix(
        Instruction::UpdateTradeFeePolicy {
            trade_fee_base_bps: 2,
        },
        &mut [&mut admin, &mut market],
    );
    assert_err_and_market_unchanged(rejected_admin_after_rotation, &market, &before);

    let rejected_over_engine_cap = run_ix(
        Instruction::UpdateTradeFeePolicy {
            trade_fee_base_bps: 101,
        },
        &mut [&mut insurance_authority, &mut market],
    );
    assert_err_and_market_unchanged(rejected_over_engine_cap, &market, &before);

    run_ix(
        Instruction::UpdateTradeFeePolicy {
            trade_fee_base_bps: 25,
        },
        &mut [&mut insurance_authority, &mut market],
    )
    .unwrap();
    let (cfg, _) = state::read_market(&market.data).unwrap();
    assert_eq!(cfg.trade_fee_base_bps, 25);
}

#[test]
fn v16_wrapper_fee_redirect_policy_is_admin_gated_and_redirects_non_main_fees_to_market_zero() {
    let mut admin = signer();
    let mut attacker = signer();
    let mut market = market_account();
    let mint = init_market(&mut admin, &mut market);

    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        1,
        1,
        100,
    )
    .unwrap();

    let before = market.data.clone();
    let rejected_attacker = run_ix(
        Instruction::UpdateFeeRedirectPolicy {
            redirect_bps: 2_500,
        },
        &mut [&mut attacker, &mut market],
    );
    assert_err_and_market_unchanged(rejected_attacker, &market, &before);

    let rejected_over_cap = run_ix(
        Instruction::UpdateFeeRedirectPolicy {
            redirect_bps: 10_001,
        },
        &mut [&mut admin, &mut market],
    );
    assert_err_and_market_unchanged(rejected_over_cap, &market, &before);

    run_ix(
        Instruction::UpdateFeeRedirectPolicy {
            redirect_bps: 2_500,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    let (cfg, _) = state::read_market(&market.data).unwrap();
    assert_eq!(cfg.fee_redirect_to_market_0_bps, 2_500);

    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();
    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 20_000);
    deposit(&mut short_owner, &mut market, &mut short_account, 20_000);

    let size_q = 100 * POS_SCALE;
    let exec_price = 100;
    let fee_bps = 100;
    let expected_fee_total = two_sided_fee(size_q, exec_price, fee_bps);
    let expected_fee_per_side = expected_fee_total / 2;
    let expected_redirect_per_side = expected_fee_per_side * 2_500 / 10_000;
    let expected_domain_per_side = expected_fee_per_side - expected_redirect_per_side;
    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 1,
            size_q: size_q as i128,
            exec_price,
            fee_bps,
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
    assert_eq!(group.insurance, expected_fee_total);
    assert_eq!(group.insurance_domain_budget[2], expected_domain_per_side);
    assert_eq!(group.insurance_domain_budget[3], expected_domain_per_side);
    assert_eq!(
        group.insurance - group.insurance_domain_budget[2] - group.insurance_domain_budget[3],
        expected_redirect_per_side * 2
    );

    run_ix(
        Instruction::UpdateInsurancePolicy {
            max_bps: 9_999,
            deposits_only: 0,
            cooldown_slots: 1,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();

    let redirected_total = expected_redirect_per_side * 2;
    let withdraw_cap = redirected_total * 9_999 / 10_000;
    let before_withdraw = market.data.clone();
    let mut dest = user_token_account(admin.key, mint, 0);
    let mut vault = vault_token_account(&market, mint, 40_000);
    let mut vault_auth = vault_authority_account(&market);
    let mut token_program = token_program_account();
    let over_unreserved = run_ix(
        Instruction::WithdrawInsuranceLimited {
            amount: withdraw_cap + 1,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(over_unreserved, &market, &before_withdraw);

    run_ix(
        Instruction::WithdrawInsuranceLimited {
            amount: withdraw_cap,
        },
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
    assert_eq!(group.insurance, expected_fee_total - withdraw_cap);
    assert_eq!(group.insurance_domain_budget[2], expected_domain_per_side);
    assert_eq!(group.insurance_domain_budget[3], expected_domain_per_side);
    assert_eq!(
        group.insurance - group.insurance_domain_budget[2] - group.insurance_domain_budget[3],
        redirected_total - withdraw_cap
    );
}

#[test]
fn v16_wrapper_permissionless_market_init_fee_policy_gates_and_funds_base_market() {
    let mut admin = signer();
    let mut creator = signer();
    let mut market = market_account();
    let mint = init_market(&mut admin, &mut market);

    let insurance_authority = Pubkey::new_unique().to_bytes();
    let insurance_operator = Pubkey::new_unique().to_bytes();
    let backing_bucket_authority = Pubkey::new_unique().to_bytes();
    let oracle_authority = Pubkey::new_unique().to_bytes();

    let before_disabled = market.data.clone();
    let disabled = run_ix(
        Instruction::UpdateAssetLifecycle {
            action: processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
            now_slot: 1,
            initial_price: 100,
            insurance_authority,
            insurance_operator,
            backing_bucket_authority,
            oracle_authority,
        },
        &mut [&mut creator, &mut market],
    );
    assert_err_and_market_unchanged(disabled, &market, &before_disabled);

    let rejected_attacker = run_ix(
        Instruction::UpdateMarketInitFeePolicy { min_init_fee: 50 },
        &mut [&mut creator, &mut market],
    );
    assert_err_and_market_unchanged(rejected_attacker, &market, &before_disabled);

    let rejected_over_u64 = run_ix(
        Instruction::UpdateMarketInitFeePolicy {
            min_init_fee: u64::MAX as u128 + 1,
        },
        &mut [&mut admin, &mut market],
    );
    assert_err_and_market_unchanged(rejected_over_u64, &market, &before_disabled);

    run_ix(
        Instruction::UpdateMarketInitFeePolicy { min_init_fee: 50 },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    let (cfg, _) = state::read_market(&market.data).unwrap();
    assert_eq!(cfg.permissionless_market_init_fee, 50);

    let mut source = user_token_account(creator.key, mint, 50);
    let mut vault = vault_token_account(&market, mint, 0);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::UpdateAssetLifecycle {
            action: processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
            now_slot: 1,
            initial_price: 100,
            insurance_authority,
            insurance_operator,
            backing_bucket_authority,
            oracle_authority,
        },
        &mut [
            &mut creator,
            &mut market,
            &mut source,
            &mut vault,
            &mut token_program,
        ],
    )
    .unwrap();

    let profile = state::read_asset_oracle_profile(&market.data, 1).unwrap();
    assert_eq!(profile.insurance_authority, insurance_authority);
    assert_eq!(profile.insurance_operator, insurance_operator);
    assert_eq!(profile.backing_bucket_authority, backing_bucket_authority);
    assert_eq!(profile.oracle_authority, oracle_authority);
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(group.insurance, 50);
    assert_eq!(group.vault, 50);
    assert_eq!(group.insurance_domain_budget[0], 25);
    assert_eq!(group.insurance_domain_budget[1], 25);
    assert_eq!(group.insurance_domain_budget[2], 0);
    assert_eq!(group.insurance_domain_budget[3], 0);
}

#[test]
fn v16_wrapper_permissionless_market_init_fee_doubles_every_32_markets() {
    let mut admin = signer();
    let mut creator = signer();
    let mut market = market_account_with_capacity(33);
    let mint = init_market(&mut admin, &mut market);

    run_ix(
        Instruction::UpdateMarketInitFeePolicy { min_init_fee: 50 },
        &mut [&mut admin, &mut market],
    )
    .unwrap();

    for asset_index in 1..32 {
        update_asset_lifecycle(
            &mut admin,
            &mut market,
            processor::ASSET_ACTION_ACTIVATE,
            asset_index as u16,
            asset_index as u64,
            100 + asset_index as u64,
        )
        .unwrap();
    }

    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(group.config.max_market_slots, 32);
    assert_eq!(group.insurance, 0);
    assert_eq!(group.vault, 0);

    let insurance_authority = Pubkey::new_unique().to_bytes();
    let insurance_operator = Pubkey::new_unique().to_bytes();
    let backing_bucket_authority = Pubkey::new_unique().to_bytes();
    let oracle_authority = Pubkey::new_unique().to_bytes();

    let mut short_source = user_token_account(creator.key, mint, 50);
    let mut vault = vault_token_account(&market, mint, 0);
    let mut token_program = token_program_account();
    let before_boundary = market.data.clone();
    let underpaid = run_ix(
        Instruction::UpdateAssetLifecycle {
            action: processor::ASSET_ACTION_ACTIVATE,
            asset_index: 32,
            now_slot: 32,
            initial_price: 132,
            insurance_authority,
            insurance_operator,
            backing_bucket_authority,
            oracle_authority,
        },
        &mut [
            &mut creator,
            &mut market,
            &mut short_source,
            &mut vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(underpaid, &market, &before_boundary);

    let mut source = user_token_account(creator.key, mint, 100);
    let mut vault = vault_token_account(&market, mint, 0);
    run_ix(
        Instruction::UpdateAssetLifecycle {
            action: processor::ASSET_ACTION_ACTIVATE,
            asset_index: 32,
            now_slot: 32,
            initial_price: 132,
            insurance_authority,
            insurance_operator,
            backing_bucket_authority,
            oracle_authority,
        },
        &mut [
            &mut creator,
            &mut market,
            &mut source,
            &mut vault,
            &mut token_program,
        ],
    )
    .unwrap();

    let profile = state::read_asset_oracle_profile(&market.data, 32).unwrap();
    assert_eq!(profile.insurance_authority, insurance_authority);
    assert_eq!(profile.insurance_operator, insurance_operator);
    assert_eq!(profile.backing_bucket_authority, backing_bucket_authority);
    assert_eq!(profile.oracle_authority, oracle_authority);
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(group.config.max_market_slots, 33);
    assert_eq!(group.insurance, 100);
    assert_eq!(group.vault, 100);
    assert_eq!(group.insurance_domain_budget[0], 50);
    assert_eq!(group.insurance_domain_budget[1], 50);
}

#[test]
fn v16_wrapper_permissionless_market_creator_must_reuse_shutdown_slot_before_append() {
    let mut admin = signer();
    let mut creator = signer();
    let mut market = market_account_with_capacity(4);
    let mint = init_market(&mut admin, &mut market);

    run_ix(
        Instruction::UpdateMarketInitFeePolicy { min_init_fee: 50 },
        &mut [&mut admin, &mut market],
    )
    .unwrap();

    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        1,
        1,
        101,
    )
    .unwrap();
    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        2,
        2,
        102,
    )
    .unwrap();
    let (_, active_group) = state::read_market(&market.data).unwrap();
    assert_eq!(active_group.config.max_market_slots, 3);

    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_RETIRE,
        1,
        3,
        0,
    )
    .unwrap();
    let (retired_cfg, retired_group) = state::read_market(&market.data).unwrap();
    assert_eq!(retired_cfg.free_market_slot_count, 1);
    assert_eq!(
        retired_group.assets[1].lifecycle,
        AssetLifecycleV16::Retired
    );
    assert_eq!(retired_group.config.max_market_slots, 3);
    let old_market_id = retired_group.assets[1].market_id;
    let next_market_id = retired_group.next_market_id;

    let insurance_authority = Pubkey::new_unique().to_bytes();
    let insurance_operator = Pubkey::new_unique().to_bytes();
    let backing_bucket_authority = Pubkey::new_unique().to_bytes();
    let oracle_authority = Pubkey::new_unique().to_bytes();
    let mut append_source = user_token_account(creator.key, mint, 50);
    let mut append_vault = vault_token_account(&market, mint, 0);
    let mut token_program = token_program_account();
    let before_append = market.data.clone();
    let append = run_ix(
        Instruction::UpdateAssetLifecycle {
            action: processor::ASSET_ACTION_ACTIVATE,
            asset_index: 3,
            now_slot: 4,
            initial_price: 103,
            insurance_authority,
            insurance_operator,
            backing_bucket_authority,
            oracle_authority,
        },
        &mut [
            &mut creator,
            &mut market,
            &mut append_source,
            &mut append_vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(append, &market, &before_append);

    let mut reuse_source = user_token_account(creator.key, mint, 50);
    let mut reuse_vault = vault_token_account(&market, mint, 0);
    run_ix(
        Instruction::UpdateAssetLifecycle {
            action: processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
            now_slot: 4,
            initial_price: 201,
            insurance_authority,
            insurance_operator,
            backing_bucket_authority,
            oracle_authority,
        },
        &mut [
            &mut creator,
            &mut market,
            &mut reuse_source,
            &mut reuse_vault,
            &mut token_program,
        ],
    )
    .unwrap();

    let profile = state::read_asset_oracle_profile(&market.data, 1).unwrap();
    assert_eq!(profile.insurance_authority, insurance_authority);
    assert_eq!(profile.insurance_operator, insurance_operator);
    assert_eq!(profile.backing_bucket_authority, backing_bucket_authority);
    assert_eq!(profile.oracle_authority, oracle_authority);
    let (reused_cfg, reused_group) = state::read_market(&market.data).unwrap();
    assert_eq!(reused_cfg.free_market_slot_count, 0);
    assert_eq!(reused_group.config.max_market_slots, 3);
    assert_eq!(reused_group.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(reused_group.assets[1].market_id, next_market_id);
    assert!(reused_group.assets[1].market_id > old_market_id);
    assert_eq!(reused_group.insurance, 50);
    assert_eq!(reused_group.vault, 50);
    assert_eq!(reused_group.insurance_domain_budget[0], 25);
    assert_eq!(reused_group.insurance_domain_budget[1], 25);
    assert_eq!(reused_group.insurance_domain_budget[2], 0);
    assert_eq!(reused_group.insurance_domain_budget[3], 0);
}

#[test]
fn v16_wrapper_permissionless_dynamic_market_drains_after_positions_close() {
    let mut admin = signer();
    let mut creator = signer();
    let mut insurance_authority = signer();
    let mut insurance_operator = signer();
    let mut backing_authority = signer();
    let mut market = market_account_with_capacity(2);
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account_for_market_slots(2);
    let mut short_account = portfolio_account_for_market_slots(2);
    let mint = init_market(&mut admin, &mut market);

    run_ix(
        Instruction::UpdateMarketInitFeePolicy { min_init_fee: 50 },
        &mut [&mut admin, &mut market],
    )
    .unwrap();

    let mut init_fee_source = user_token_account(creator.key, mint, 50);
    let mut vault = vault_token_account(&market, mint, 0);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::UpdateAssetLifecycle {
            action: processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
            now_slot: 1,
            initial_price: 150,
            insurance_authority: insurance_authority.key.to_bytes(),
            insurance_operator: insurance_operator.key.to_bytes(),
            backing_bucket_authority: backing_authority.key.to_bytes(),
            oracle_authority: backing_authority.key.to_bytes(),
        },
        &mut [
            &mut creator,
            &mut market,
            &mut init_fee_source,
            &mut vault,
            &mut token_program,
        ],
    )
    .unwrap();

    let mut insurance_source = user_token_account(insurance_authority.key, mint, 10);
    let mut vault = vault_token_account(&market, mint, 0);
    run_ix(
        Instruction::TopUpInsuranceDomain {
            domain: 2,
            amount: 10,
        },
        &mut [
            &mut insurance_authority,
            &mut market,
            &mut insurance_source,
            &mut vault,
            &mut token_program,
        ],
    )
    .unwrap();
    let mut backing_source = user_token_account(backing_authority.key, mint, 25);
    let mut vault = vault_token_account(&market, mint, 0);
    run_ix(
        Instruction::TopUpBackingBucket {
            domain: 2,
            amount: 25,
            expiry_slot: 10,
        },
        &mut [
            &mut backing_authority,
            &mut market,
            &mut backing_source,
            &mut vault,
            &mut token_program,
        ],
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

    let before_open_retire = market.data.clone();
    let open_retire = update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_RETIRE,
        1,
        2,
        0,
    );
    assert_err_and_market_unchanged(open_retire, &market, &before_open_retire);

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
    let closed = state::read_market(&market.data).unwrap().1;
    assert_eq!(closed.assets[1].oi_eff_long_q, 0);
    assert_eq!(closed.assets[1].oi_eff_short_q, 0);

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

    let before_funded_retire = market.data.clone();
    let funded_retire = update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_RETIRE,
        1,
        3,
        0,
    );
    assert_err_and_market_unchanged(funded_retire, &market, &before_funded_retire);

    let mut insurance_dest = user_token_account(insurance_operator.key, mint, 0);
    let mut vault = vault_token_account(&market, mint, 85);
    let mut vault_auth = vault_authority_account(&market);
    run_ix(
        Instruction::WithdrawInsuranceDomain {
            domain: 2,
            amount: 10,
        },
        &mut [
            &mut insurance_operator,
            &mut market,
            &mut insurance_dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    )
    .unwrap();
    let mut backing_dest = user_token_account(backing_authority.key, mint, 0);
    run_ix(
        Instruction::WithdrawBackingBucket {
            domain: 2,
            amount: 25,
        },
        &mut [
            &mut backing_authority,
            &mut market,
            &mut backing_dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    )
    .unwrap();

    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_RETIRE,
        1,
        3,
        0,
    )
    .unwrap();
    let retired = state::read_market(&market.data).unwrap().1;
    assert_eq!(retired.assets[1].lifecycle, AssetLifecycleV16::Retired);
    assert_eq!(retired.assets[1].retired_slot, 3);
    assert_eq!(retired.insurance_domain_budget[2], 0);
    assert_eq!(
        retired.source_backing_buckets[2].fresh_unliened_backing_num,
        0
    );
    assert_eq!(retired.insurance, 50);
    assert_eq!(retired.vault, 20_050);
}

#[test]
fn v16_wrapper_shutdown_asset_force_closes_drains_retires_and_reuses_slot() {
    let mut admin = signer();
    let mut attacker = signer();
    let mut creator = signer();
    let mut asset_authority = signer();
    let mut insurance_authority = signer();
    let mut insurance_operator = signer();
    let mut backing_authority = signer();
    let mut cranker = signer();
    let mut market = market_account_with_capacity(2);
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account_for_market_slots(2);
    let mut short_account = portfolio_account_for_market_slots(2);
    let mint = init_market(&mut admin, &mut market);

    run_ix(
        Instruction::ConfigurePermissionlessResolve {
            stale_slots: 100,
            force_close_delay_slots: 5,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    run_ix(
        Instruction::UpdateMarketInitFeePolicy { min_init_fee: 10 },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    update_asset_lifecycle_with_authorities(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        1,
        1,
        150,
        insurance_authority.key.to_bytes(),
        insurance_operator.key.to_bytes(),
        backing_authority.key.to_bytes(),
    )
    .unwrap();
    run_ix(
        Instruction::UpdateAuthority {
            kind: processor::AUTHORITY_ASSET,
            new_pubkey: asset_authority.key.to_bytes(),
        },
        &mut [&mut admin, &mut asset_authority, &mut market],
    )
    .unwrap();

    let mut token_program = token_program_account();
    for (domain, amount) in [(2u8, 6u128), (3u8, 4u128)] {
        let mut source = user_token_account(insurance_authority.key, mint, amount as u64);
        let mut vault = vault_token_account(&market, mint, 0);
        run_ix(
            Instruction::TopUpInsuranceDomain { domain, amount },
            &mut [
                &mut insurance_authority,
                &mut market,
                &mut source,
                &mut vault,
                &mut token_program,
            ],
        )
        .unwrap();
    }
    top_up_backing_bucket(&mut backing_authority, &mut market, 2, 20, 20);
    top_up_backing_bucket(&mut backing_authority, &mut market, 3, 25, 20);

    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 10_000);
    deposit(&mut short_owner, &mut market, &mut short_account, 10_000);
    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 1,
            size_q: (POS_SCALE * 2) as i128,
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

    let before_unauthorized_shutdown = market.data.clone();
    let unauthorized_shutdown = update_asset_lifecycle(
        &mut attacker,
        &mut market,
        processor::ASSET_ACTION_SHUTDOWN,
        1,
        2,
        0,
    );
    assert_err_and_market_unchanged(
        unauthorized_shutdown,
        &market,
        &before_unauthorized_shutdown,
    );

    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_SHUTDOWN,
        1,
        2,
        0,
    )
    .unwrap();
    let shutdown_profile = state::read_asset_oracle_profile(&market.data, 1).unwrap();
    let (_, shutdown_group) = state::read_market(&market.data).unwrap();
    assert_eq!(
        shutdown_group.assets[1].lifecycle,
        AssetLifecycleV16::Recovery
    );
    assert_eq!(shutdown_profile.last_good_oracle_slot, 2);
    assert_eq!(shutdown_group.assets[1].effective_price, 150);

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
    let long_after_voluntary = state::read_portfolio(&long_account.data).unwrap();
    let short_after_voluntary = state::read_portfolio(&short_account.data).unwrap();
    assert_eq!(
        active_leg_for_asset(&long_after_voluntary, 1)
            .basis_pos_q
            .unsigned_abs(),
        POS_SCALE
    );
    assert_eq!(
        active_leg_for_asset(&short_after_voluntary, 1)
            .basis_pos_q
            .unsigned_abs(),
        POS_SCALE
    );

    let before_timeout_market = market.data.clone();
    let before_timeout_long = long_account.data.clone();
    let before_timeout_short = short_account.data.clone();
    let too_early = force_close_abandoned_asset(
        &mut cranker,
        &mut market,
        &mut long_account,
        &mut short_account,
        1,
        6,
        POS_SCALE,
    );
    assert_err_and_market_unchanged(too_early, &market, &before_timeout_market);
    assert_eq!(long_account.data, before_timeout_long);
    assert_eq!(short_account.data, before_timeout_short);

    force_close_abandoned_asset(
        &mut cranker,
        &mut market,
        &mut long_account,
        &mut short_account,
        1,
        7,
        POS_SCALE,
    )
    .unwrap();
    let long_closed = state::read_portfolio(&long_account.data).unwrap();
    let short_closed = state::read_portfolio(&short_account.data).unwrap();
    assert!(!has_active_leg_for_asset(&long_closed, 1));
    assert!(!has_active_leg_for_asset(&short_closed, 1));
    let (cfg, mut group) = state::read_market(&market.data).unwrap();
    assert_eq!(group.assets[1].oi_eff_long_q, 0);
    assert_eq!(group.assets[1].oi_eff_short_q, 0);
    group.current_slot = 7;
    group.loss_stale_active = true;
    state::write_market(&mut market.data, &cfg, &group).unwrap();

    let mut vault = vault_token_account(&market, mint, 100_000);
    let mut vault_auth = vault_authority_account(&market);
    let mut local_insurance_dest = user_token_account(insurance_operator.key, mint, 0);
    run_ix(
        Instruction::WithdrawInsuranceDomain {
            domain: 2,
            amount: 6,
        },
        &mut [
            &mut insurance_operator,
            &mut market,
            &mut local_insurance_dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    )
    .unwrap();
    let mut admin_insurance_dest = user_token_account(admin.key, mint, 0);
    run_ix(
        Instruction::WithdrawInsuranceDomain {
            domain: 3,
            amount: 4,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut admin_insurance_dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    )
    .unwrap();
    let mut local_backing_dest = user_token_account(backing_authority.key, mint, 0);
    run_ix(
        Instruction::WithdrawBackingBucket {
            domain: 2,
            amount: 20,
        },
        &mut [
            &mut backing_authority,
            &mut market,
            &mut local_backing_dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    )
    .unwrap();
    let mut admin_backing_dest = user_token_account(admin.key, mint, 0);
    run_ix(
        Instruction::WithdrawBackingBucket {
            domain: 3,
            amount: 25,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut admin_backing_dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    )
    .unwrap();

    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_RETIRE,
        1,
        7,
        0,
    )
    .unwrap();
    let (retired_cfg, retired_group) = state::read_market(&market.data).unwrap();
    assert_eq!(retired_cfg.free_market_slot_count, 1);
    assert_eq!(
        retired_group.assets[1].lifecycle,
        AssetLifecycleV16::Retired
    );
    let retired_market_id = retired_group.assets[1].market_id;
    let reuse_market_id = retired_group.next_market_id;
    assert_eq!(retired_group.insurance_domain_budget[2], 0);
    assert_eq!(retired_group.insurance_domain_budget[3], 0);
    assert_eq!(
        retired_group.source_backing_buckets[2].fresh_unliened_backing_num,
        0
    );
    assert_eq!(
        retired_group.source_backing_buckets[3].fresh_unliened_backing_num,
        0
    );

    let new_insurance_authority = Pubkey::new_unique().to_bytes();
    let new_insurance_operator = Pubkey::new_unique().to_bytes();
    let new_backing_authority = Pubkey::new_unique().to_bytes();
    let new_oracle_authority = Pubkey::new_unique().to_bytes();
    let mut append_source = user_token_account(creator.key, mint, 10);
    let mut append_vault = vault_token_account(&market, mint, 0);
    let before_append = market.data.clone();
    let append = run_ix(
        Instruction::UpdateAssetLifecycle {
            action: processor::ASSET_ACTION_ACTIVATE,
            asset_index: 2,
            now_slot: 8,
            initial_price: 250,
            insurance_authority: new_insurance_authority,
            insurance_operator: new_insurance_operator,
            backing_bucket_authority: new_backing_authority,
            oracle_authority: new_oracle_authority,
        },
        &mut [
            &mut creator,
            &mut market,
            &mut append_source,
            &mut append_vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(append, &market, &before_append);

    let mut reuse_source = user_token_account(creator.key, mint, 10);
    let mut reuse_vault = vault_token_account(&market, mint, 0);
    run_ix(
        Instruction::UpdateAssetLifecycle {
            action: processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
            now_slot: 8,
            initial_price: 250,
            insurance_authority: new_insurance_authority,
            insurance_operator: new_insurance_operator,
            backing_bucket_authority: new_backing_authority,
            oracle_authority: new_oracle_authority,
        },
        &mut [
            &mut creator,
            &mut market,
            &mut reuse_source,
            &mut reuse_vault,
            &mut token_program,
        ],
    )
    .unwrap();
    let profile = state::read_asset_oracle_profile(&market.data, 1).unwrap();
    assert_eq!(profile.insurance_authority, new_insurance_authority);
    assert_eq!(profile.insurance_operator, new_insurance_operator);
    assert_eq!(profile.backing_bucket_authority, new_backing_authority);
    assert_eq!(profile.oracle_authority, new_oracle_authority);
    let (reused_cfg, reused_group) = state::read_market(&market.data).unwrap();
    assert_eq!(reused_cfg.free_market_slot_count, 0);
    assert_eq!(reused_group.config.max_market_slots, 2);
    assert_eq!(reused_group.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(reused_group.assets[1].market_id, reuse_market_id);
    assert!(reused_group.assets[1].market_id > retired_market_id);
    assert_eq!(reused_group.assets[1].effective_price, 250);
    assert_eq!(reused_group.insurance_domain_budget[0], 5);
    assert_eq!(reused_group.insurance_domain_budget[1], 5);
    assert_eq!(reused_group.insurance_domain_budget[2], 0);
    assert_eq!(reused_group.insurance_domain_budget[3], 0);
    assert_eq!(reused_group.insurance, 10);
    assert_eq!(reused_group.vault, 20_010);
}

#[test]
fn v16_wrapper_permissionless_market_shutdown_force_closes_recovers_and_reuses_slot() {
    let mut admin = signer();
    let mut attacker = signer();
    let mut cranker = signer();
    let mut insurance_authority = signer();
    let mut backing_authority = signer();
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut market = market_account();
    let mut long_account = portfolio_account();
    let mut short_account = portfolio_account();
    let mint = init_market(&mut admin, &mut market);

    run_ix(
        Instruction::ConfigurePermissionlessResolve {
            stale_slots: 100,
            force_close_delay_slots: 5,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    run_ix(
        Instruction::UpdateMarketInitFeePolicy { min_init_fee: 25 },
        &mut [&mut admin, &mut market],
    )
    .unwrap();

    let insurance_operator = Pubkey::new_unique().to_bytes();
    let oracle_authority = admin.key.to_bytes();
    let mut init_fee_source = user_token_account(attacker.key, mint, 25);
    let mut init_fee_vault = vault_token_account(&market, mint, 0);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::UpdateAssetLifecycle {
            action: processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
            now_slot: 1,
            initial_price: 100,
            insurance_authority: insurance_authority.key.to_bytes(),
            insurance_operator,
            backing_bucket_authority: backing_authority.key.to_bytes(),
            oracle_authority,
        },
        &mut [
            &mut attacker,
            &mut market,
            &mut init_fee_source,
            &mut init_fee_vault,
            &mut token_program,
        ],
    )
    .unwrap();
    let (cfg_after_create, group_after_create) = state::read_market(&market.data).unwrap();
    assert_eq!(cfg_after_create.permissionless_market_init_fee, 25);
    assert_eq!(
        group_after_create.assets[1].lifecycle,
        AssetLifecycleV16::Active
    );
    assert_eq!(group_after_create.insurance, 25);
    assert_eq!(group_after_create.vault, 25);
    assert_eq!(group_after_create.insurance_domain_budget[0], 12);
    assert_eq!(group_after_create.insurance_domain_budget[1], 13);
    let old_market_id = group_after_create.assets[1].market_id;

    for (domain, amount) in [(2u8, 6u128), (3u8, 4u128)] {
        let mut source = user_token_account(insurance_authority.key, mint, amount as u64);
        let mut vault = vault_token_account(&market, mint, 0);
        run_ix(
            Instruction::TopUpInsuranceDomain { domain, amount },
            &mut [
                &mut insurance_authority,
                &mut market,
                &mut source,
                &mut vault,
                &mut token_program,
            ],
        )
        .unwrap();
    }
    top_up_backing_bucket(&mut backing_authority, &mut market, 2, 20, 20);
    top_up_backing_bucket(&mut backing_authority, &mut market, 3, 25, 20);

    init_portfolio(&mut long_owner, &mut market, &mut long_account);
    init_portfolio(&mut short_owner, &mut market, &mut short_account);
    deposit(&mut long_owner, &mut market, &mut long_account, 10_000);
    deposit(&mut short_owner, &mut market, &mut short_account, 10_000);
    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 1,
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

    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_SHUTDOWN,
        1,
        2,
        0,
    )
    .unwrap();
    let (_, shutdown_group) = state::read_market(&market.data).unwrap();
    assert_eq!(
        shutdown_group.assets[1].lifecycle,
        AssetLifecycleV16::Recovery
    );

    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 1,
            size_q: -(POS_SCALE as i128),
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
    let long_after_exit_window = state::read_portfolio(&long_account.data).unwrap();
    let short_after_exit_window = state::read_portfolio(&short_account.data).unwrap();
    assert_eq!(
        active_leg_for_asset(&long_after_exit_window, 1)
            .basis_pos_q
            .unsigned_abs(),
        POS_SCALE
    );
    assert!(
        active_leg_for_asset(&short_after_exit_window, 1)
            .basis_pos_q
            .unsigned_abs()
            == POS_SCALE
    );

    let before_timeout_market = market.data.clone();
    let before_timeout_long = long_account.data.clone();
    let before_timeout_short = short_account.data.clone();
    let too_early = force_close_abandoned_asset(
        &mut cranker,
        &mut market,
        &mut long_account,
        &mut short_account,
        1,
        6,
        POS_SCALE,
    );
    assert_err_and_market_unchanged(too_early, &market, &before_timeout_market);
    assert_eq!(long_account.data, before_timeout_long);
    assert_eq!(short_account.data, before_timeout_short);

    force_close_abandoned_asset(
        &mut cranker,
        &mut market,
        &mut long_account,
        &mut short_account,
        1,
        7,
        POS_SCALE,
    )
    .unwrap();
    assert!(
        !has_active_leg_for_asset(&state::read_portfolio(&long_account.data).unwrap(), 1),
        "long abandoned account should be closed after the shutdown timeout"
    );
    assert!(
        !has_active_leg_for_asset(&state::read_portfolio(&short_account.data).unwrap(), 1),
        "short abandoned account should be closed after the shutdown timeout"
    );

    let (_, liquidated_group) = state::read_market(&market.data).unwrap();
    assert_eq!(liquidated_group.assets[1].oi_eff_long_q, 0);
    assert_eq!(liquidated_group.assets[1].oi_eff_short_q, 0);

    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        group.current_slot = 7;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }

    let mut vault = vault_token_account(&market, mint, 100_000);
    let mut vault_auth = vault_authority_account(&market);
    let mut admin_dest = user_token_account(admin.key, mint, 0);
    for (domain, amount) in [(2u8, 6u128), (3u8, 4u128)] {
        run_ix(
            Instruction::WithdrawInsuranceDomain { domain, amount },
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
    }
    for (domain, amount) in [(2u8, 20u128), (3u8, 25u128)] {
        run_ix(
            Instruction::WithdrawBackingBucket { domain, amount },
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
    }

    let mut base_insurance_source = user_token_account(admin.key, mint, 10);
    let mut base_insurance_vault = vault_token_account(&market, mint, 0);
    run_ix(
        Instruction::TopUpInsurance { amount: 10 },
        &mut [
            &mut admin,
            &mut market,
            &mut base_insurance_source,
            &mut base_insurance_vault,
            &mut token_program,
        ],
    )
    .unwrap();
    top_up_backing_bucket(&mut admin, &mut market, 0, 45, 20);
    let (_, recovered_group) = state::read_market(&market.data).unwrap();
    assert_eq!(recovered_group.insurance_domain_budget[2], 0);
    assert_eq!(recovered_group.insurance_domain_budget[3], 0);
    assert_eq!(
        recovered_group.source_backing_buckets[2].fresh_unliened_backing_num,
        0
    );
    assert_eq!(
        recovered_group.source_backing_buckets[3].fresh_unliened_backing_num,
        0
    );
    assert_eq!(recovered_group.insurance_domain_budget[0], 17);
    assert_eq!(recovered_group.insurance_domain_budget[1], 18);
    assert_eq!(
        recovered_group.source_backing_buckets[0].fresh_unliened_backing_num,
        45 * BOUND_SCALE
    );

    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_RETIRE,
        1,
        7,
        0,
    )
    .unwrap();
    let (retired_cfg, retired_group) = state::read_market(&market.data).unwrap();
    assert_eq!(retired_cfg.free_market_slot_count, 1);
    assert_eq!(
        retired_group.assets[1].lifecycle,
        AssetLifecycleV16::Retired
    );
    let reuse_market_id = retired_group.next_market_id;
    assert!(reuse_market_id > old_market_id);

    let mut reuse_source = user_token_account(attacker.key, mint, 25);
    let mut reuse_vault = vault_token_account(&market, mint, 0);
    run_ix(
        Instruction::UpdateAssetLifecycle {
            action: processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
            now_slot: 8,
            initial_price: 250,
            insurance_authority: insurance_authority.key.to_bytes(),
            insurance_operator,
            backing_bucket_authority: backing_authority.key.to_bytes(),
            oracle_authority,
        },
        &mut [
            &mut attacker,
            &mut market,
            &mut reuse_source,
            &mut reuse_vault,
            &mut token_program,
        ],
    )
    .unwrap();
    let (reused_cfg, reused_group) = state::read_market(&market.data).unwrap();
    assert_eq!(reused_cfg.free_market_slot_count, 0);
    assert_eq!(reused_group.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(reused_group.assets[1].market_id, reuse_market_id);
    assert!(reused_group.assets[1].market_id > old_market_id);
    assert_eq!(reused_group.assets[1].effective_price, 250);
}

#[test]
fn v16_wrapper_shutdown_admin_drain_timeout_ledgers_and_backing_earnings() {
    let mut admin = signer();
    let mut insurance_authority = signer();
    let insurance_operator = signer();
    let mut backing_authority = signer();
    let mut market = market_account_with_capacity(2);
    let mint = init_market(&mut admin, &mut market);

    run_ix(
        Instruction::ConfigurePermissionlessResolve {
            stale_slots: 100,
            force_close_delay_slots: 5,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    update_asset_lifecycle_with_authorities(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        1,
        1,
        150,
        insurance_authority.key.to_bytes(),
        insurance_operator.key.to_bytes(),
        backing_authority.key.to_bytes(),
    )
    .unwrap();

    let mut token_program = token_program_account();
    let mut local_insurance_ledger = insurance_ledger_account();
    let mut insurance_source = user_token_account(insurance_authority.key, mint, 9);
    let mut vault = vault_token_account(&market, mint, 0);
    run_ix(
        Instruction::TopUpInsuranceDomain {
            domain: 2,
            amount: 9,
        },
        &mut [
            &mut insurance_authority,
            &mut market,
            &mut insurance_source,
            &mut vault,
            &mut token_program,
            &mut local_insurance_ledger,
        ],
    )
    .unwrap();

    let mut local_backing_ledger = backing_domain_ledger_account();
    let mut backing_source = user_token_account(backing_authority.key, mint, 20);
    run_ix(
        Instruction::TopUpBackingBucket {
            domain: 2,
            amount: 20,
            expiry_slot: 20,
        },
        &mut [
            &mut backing_authority,
            &mut market,
            &mut backing_source,
            &mut vault,
            &mut token_program,
            &mut local_backing_ledger,
        ],
    )
    .unwrap();
    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        group.source_backing_buckets[2].utilization_fee_earnings = 5;
        group.vault += 5;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }

    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_SHUTDOWN,
        1,
        2,
        0,
    )
    .unwrap();
    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        group.current_slot = 6;
        group.loss_stale_active = true;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }

    let mut vault = vault_token_account(&market, mint, 34);
    let mut vault_auth = vault_authority_account(&market);
    let mut admin_dest = user_token_account(admin.key, mint, 0);
    let before_timeout = market.data.clone();
    let too_early = run_ix(
        Instruction::WithdrawInsuranceDomain {
            domain: 2,
            amount: 1,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut admin_dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(too_early, &market, &before_timeout);

    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        group.current_slot = 7;
        group.loss_stale_active = true;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }

    let before_local_insurance_ledger = local_insurance_ledger.data.clone();
    let before_wrong_insurance_ledger = market.data.clone();
    let wrong_insurance_ledger = run_ix(
        Instruction::WithdrawInsuranceDomain {
            domain: 2,
            amount: 1,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut admin_dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
            &mut local_insurance_ledger,
        ],
    );
    assert_err_and_market_unchanged(
        wrong_insurance_ledger,
        &market,
        &before_wrong_insurance_ledger,
    );
    assert_eq!(local_insurance_ledger.data, before_local_insurance_ledger);

    let mut admin_insurance_ledger = insurance_ledger_account();
    run_ix(
        Instruction::WithdrawInsuranceDomain {
            domain: 2,
            amount: 9,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut admin_dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
            &mut admin_insurance_ledger,
        ],
    )
    .unwrap();
    let admin_insurance_ledger_state =
        state::read_insurance_ledger(&admin_insurance_ledger.data).unwrap();
    assert_eq!(admin_insurance_ledger_state.authority, admin.key.to_bytes());
    assert_eq!(admin_insurance_ledger_state.total_withdrawn_atoms, 9);

    let before_local_backing_ledger = local_backing_ledger.data.clone();
    let before_wrong_backing_ledger = market.data.clone();
    let wrong_backing_ledger = run_ix(
        Instruction::WithdrawBackingBucketEarnings {
            domain: 2,
            amount: 1,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut local_backing_ledger,
            &mut admin_dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(wrong_backing_ledger, &market, &before_wrong_backing_ledger);
    assert_eq!(local_backing_ledger.data, before_local_backing_ledger);

    let mut admin_backing_ledger = backing_domain_ledger_account();
    run_ix(
        Instruction::WithdrawBackingBucketEarnings {
            domain: 2,
            amount: 5,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut admin_backing_ledger,
            &mut admin_dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    )
    .unwrap();
    let admin_backing_ledger_state =
        state::read_backing_domain_ledger(&admin_backing_ledger.data).unwrap();
    assert_eq!(admin_backing_ledger_state.authority, admin.key.to_bytes());
    assert_eq!(admin_backing_ledger_state.total_earnings_withdrawn_atoms, 5);

    run_ix(
        Instruction::WithdrawBackingBucket {
            domain: 2,
            amount: 20,
        },
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
    assert_eq!(group.vault, 0);
    assert_eq!(group.insurance, 0);
    assert_eq!(group.insurance_domain_budget[2], 0);
    assert_eq!(
        group.source_backing_buckets[2].fresh_unliened_backing_num,
        0
    );
    assert_eq!(group.source_backing_buckets[2].utilization_fee_earnings, 0);
}

#[test]
fn v16_wrapper_backing_fee_policy_is_insurance_authority_gated_and_bounds_fee() {
    let mut admin = signer();
    let mut attacker = signer();
    let mut backing_authority = signer();
    let mut insurance_authority = signer();
    let mut market = market_account();
    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_trading_fee_bps,
                ..
            } = ix
            {
                *max_trading_fee_bps = 100;
            }
        }),
    );
    run_ix(
        Instruction::UpdateAuthority {
            kind: processor::AUTHORITY_BACKING_BUCKET,
            new_pubkey: backing_authority.key.to_bytes(),
        },
        &mut [&mut admin, &mut backing_authority, &mut market],
    )
    .unwrap();
    run_ix(
        Instruction::UpdateAuthority {
            kind: processor::AUTHORITY_INSURANCE,
            new_pubkey: insurance_authority.key.to_bytes(),
        },
        &mut [&mut admin, &mut insurance_authority, &mut market],
    )
    .unwrap();
    let before = market.data.clone();

    let rejected_attacker = run_ix(
        Instruction::UpdateBackingFeePolicy {
            domain: 1,
            fee_bps: 25,
            insurance_share_bps: 0,
        },
        &mut [&mut attacker, &mut market],
    );
    assert_err_and_market_unchanged(rejected_attacker, &market, &before);

    let rejected_admin_after_rotation = run_ix(
        Instruction::UpdateBackingFeePolicy {
            domain: 1,
            fee_bps: 25,
            insurance_share_bps: 0,
        },
        &mut [&mut admin, &mut market],
    );
    assert_err_and_market_unchanged(rejected_admin_after_rotation, &market, &before);

    let rejected_backing_authority = run_ix(
        Instruction::UpdateBackingFeePolicy {
            domain: 1,
            fee_bps: 25,
            insurance_share_bps: 0,
        },
        &mut [&mut backing_authority, &mut market],
    );
    assert_err_and_market_unchanged(rejected_backing_authority, &market, &before);

    let rejected_over_engine_cap = run_ix(
        Instruction::UpdateBackingFeePolicy {
            domain: 1,
            fee_bps: 101,
            insurance_share_bps: 0,
        },
        &mut [&mut insurance_authority, &mut market],
    );
    assert_err_and_market_unchanged(rejected_over_engine_cap, &market, &before);

    let rejected_inactive_domain = run_ix(
        Instruction::UpdateBackingFeePolicy {
            domain: 2,
            fee_bps: 25,
            insurance_share_bps: 0,
        },
        &mut [&mut insurance_authority, &mut market],
    );
    assert_err_and_market_unchanged(rejected_inactive_domain, &market, &before);

    let rejected_share_without_fee = run_ix(
        Instruction::UpdateBackingFeePolicy {
            domain: 1,
            fee_bps: 0,
            insurance_share_bps: 1,
        },
        &mut [&mut insurance_authority, &mut market],
    );
    assert_err_and_market_unchanged(rejected_share_without_fee, &market, &before);

    let rejected_share_over_bps = run_ix(
        Instruction::UpdateBackingFeePolicy {
            domain: 1,
            fee_bps: 25,
            insurance_share_bps: 10_001,
        },
        &mut [&mut insurance_authority, &mut market],
    );
    assert_err_and_market_unchanged(rejected_share_over_bps, &market, &before);

    run_ix(
        Instruction::UpdateBackingFeePolicy {
            domain: 1,
            fee_bps: 25,
            insurance_share_bps: 2_500,
        },
        &mut [&mut insurance_authority, &mut market],
    )
    .unwrap();
    let (cfg, _) = state::read_market(&market.data).unwrap();
    assert_eq!(cfg.backing_trade_fee_bps_long, 0);
    assert_eq!(cfg.backing_trade_fee_bps_short, 25);
    assert_eq!(cfg.backing_trade_fee_insurance_share_bps_short, 2_500);
    assert_eq!(cfg.backing_trade_fee_policy_count, 1);

    run_ix(
        Instruction::UpdateBackingFeePolicy {
            domain: 0,
            fee_bps: 33,
            insurance_share_bps: 4_000,
        },
        &mut [&mut insurance_authority, &mut market],
    )
    .unwrap();
    let (cfg, _) = state::read_market(&market.data).unwrap();
    assert_eq!(cfg.backing_trade_fee_bps_long, 33);
    assert_eq!(cfg.backing_trade_fee_bps_short, 25);
    assert_eq!(cfg.backing_trade_fee_insurance_share_bps_long, 4_000);
    assert_eq!(cfg.backing_trade_fee_policy_count, 2);

    run_ix(
        Instruction::UpdateBackingFeePolicy {
            domain: 1,
            fee_bps: 0,
            insurance_share_bps: 0,
        },
        &mut [&mut insurance_authority, &mut market],
    )
    .unwrap();
    let (cfg, _) = state::read_market(&market.data).unwrap();
    assert_eq!(cfg.backing_trade_fee_bps_long, 33);
    assert_eq!(cfg.backing_trade_fee_bps_short, 0);
    assert_eq!(cfg.backing_trade_fee_insurance_share_bps_short, 0);
    assert_eq!(cfg.backing_trade_fee_policy_count, 1);
}

#[test]
fn v16_wrapper_backing_fee_policy_does_not_floor_trades_without_new_backing_lien() {
    let mut admin = signer();
    let mut market = market_account();
    init_market(&mut admin, &mut market);
    run_ix(
        Instruction::UpdateBackingFeePolicy {
            domain: 1,
            fee_bps: 25,
            insurance_share_bps: 0,
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
    deposit(&mut owner_a, &mut market, &mut account_a, 20_000);
    deposit(&mut owner_b, &mut market, &mut account_b, 20_000);

    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: (100 * POS_SCALE) as i128,
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
    )
    .unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(
        group.insurance, 0,
        "domain backing fees are charged only when a trade locks new counterparty backing"
    );
}

#[test]
fn v16_wrapper_backing_fee_rejects_unsafe_charge_and_skips_without_new_lien_nocpi_and_cpi() {
    let mut admin = signer();
    let mut market = market_account();
    init_market(&mut admin, &mut market);
    top_up_backing_bucket(&mut admin, &mut market, 1, 1_000, 10);
    run_ix(
        Instruction::UpdateBackingFeePolicy {
            domain: 1,
            fee_bps: 1_000,
            insurance_share_bps: 0,
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
    deposit(&mut owner_a, &mut market, &mut account_a, 100);
    deposit(&mut owner_b, &mut market, &mut account_b, 2_000);
    add_source_positive_pnl(&mut market, &mut account_a, 1, 1_000);

    let before_market = market.data.clone();
    let before_a = account_a.data.clone();
    let before_b = account_b.data.clone();
    let too_expensive = run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: (10 * POS_SCALE) as i128,
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
    assert_eq!(
        too_expensive,
        Err(percolator_prog::error::PercolatorError::EngineLockActive.into()),
        "a domain backing fee cannot be charged if it would leave the borrower below initial margin"
    );
    assert_eq!(market.data, before_market);
    assert_eq!(account_a.data, before_a);
    assert_eq!(account_b.data, before_b);

    run_ix(
        Instruction::UpdateBackingFeePolicy {
            domain: 1,
            fee_bps: 100,
            insurance_share_bps: 0,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    deposit(&mut owner_a, &mut market, &mut account_a, 900);

    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: (10 * POS_SCALE) as i128,
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
    )
    .unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    let account = state::read_portfolio(&account_a.data).unwrap();
    assert_eq!(group.insurance, 0);
    assert_eq!(account.capital, 1_000);
    assert_eq!(
        account.source_lien_counterparty_backing_num[1], 0,
        "no fee is charged when the engine does not create a new source-backed lien"
    );
    assert_eq!(group.source_backing_buckets[1].valid_liened_backing_num, 0);
    assert_eq!(
        group.source_backing_buckets[1].fresh_unliened_backing_num,
        1_000 * BOUND_SCALE
    );

    let fresh_before_reduce = group.source_backing_buckets[1].fresh_unliened_backing_num;
    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 0,
            size_q: -(5 * POS_SCALE as i128),
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
    )
    .unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(
        group.source_backing_buckets[1].fresh_unliened_backing_num, fresh_before_reduce,
        "risk-reducing trades that do not lock new backing do not pay a backing reservation fee"
    );

    let mut cpi_market = market_account();
    init_market(&mut admin, &mut cpi_market);
    top_up_backing_bucket(&mut admin, &mut cpi_market, 1, 1_000, 10);
    run_ix(
        Instruction::UpdateBackingFeePolicy {
            domain: 1,
            fee_bps: 100,
            insurance_share_bps: 0,
        },
        &mut [&mut admin, &mut cpi_market],
    )
    .unwrap();
    let mut cpi_owner_a = signer();
    let mut cpi_owner_b = signer();
    let mut cpi_account_a = portfolio_account();
    let mut cpi_account_b = portfolio_account();
    init_portfolio(&mut cpi_owner_a, &mut cpi_market, &mut cpi_account_a);
    init_portfolio(&mut cpi_owner_b, &mut cpi_market, &mut cpi_account_b);
    deposit(&mut cpi_owner_a, &mut cpi_market, &mut cpi_account_a, 1_000);
    deposit(&mut cpi_owner_b, &mut cpi_market, &mut cpi_account_b, 2_000);
    add_source_positive_pnl(&mut cpi_market, &mut cpi_account_a, 1, 1_000);

    run_trade_cpi_with_matcher(
        &mut cpi_owner_a,
        &mut cpi_owner_b,
        &mut cpi_market,
        &mut cpi_account_a,
        &mut cpi_account_b,
        0,
        (10 * POS_SCALE) as i128,
        (10 * POS_SCALE) as i128,
        100,
        0,
        100,
    )
    .unwrap();
    let (_, group) = state::read_market(&cpi_market.data).unwrap();
    let account = state::read_portfolio(&cpi_account_a.data).unwrap();
    assert_eq!(
        account.source_lien_counterparty_backing_num[1], 0,
        "TradeCpi must match TradeNoCpi when no new source-backed lien is created"
    );
    assert_eq!(
        group.source_backing_buckets[1].fresh_unliened_backing_num,
        1_000 * BOUND_SCALE
    );
}

#[test]
fn v16_wrapper_backing_fee_policy_survives_non_base_oracle_reconfiguration() {
    let mut admin = signer();
    let mut market = market_account();
    init_market(&mut admin, &mut market);
    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        1,
        1,
        100,
    )
    .unwrap();

    run_ix(
        Instruction::UpdateBackingFeePolicy {
            domain: 3,
            fee_bps: 37,
            insurance_share_bps: 3_700,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    let before = state::read_asset_oracle_profile(&market.data, 1).unwrap();
    assert_eq!(before.backing_trade_fee_bps_long, 0);
    assert_eq!(before.backing_trade_fee_bps_short, 37);
    assert_eq!(before.backing_trade_fee_insurance_share_bps_short, 3_700);

    run_ix(
        Instruction::ConfigureEwmaMark {
            asset_index: 1,
            now_slot: 2,
            initial_mark_e6: 110,
            mark_ewma_halflife_slots: 10,
            mark_min_fee: 0,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    let after_ewma_mark = state::read_asset_oracle_profile(&market.data, 1).unwrap();
    assert_eq!(
        after_ewma_mark.backing_trade_fee_bps_short, 37,
        "oracle profile reconfiguration must not erase the backing authority's domain fee"
    );
    assert_eq!(
        after_ewma_mark.backing_trade_fee_insurance_share_bps_short, 3_700,
        "oracle profile reconfiguration must not erase the insurance fee split"
    );

    let feeds = [[0x71u8; 32], [0x72u8; 32], [0x73u8; 32]];
    let mut leg0 = pyth_account(&feeds[0], 4_000_000_000, -6, 1, 1_000);
    let mut leg1 = pyth_account(&feeds[1], 150_000_000, -6, 1, 1_000);
    let mut leg2 = pyth_account(&feeds[2], 200_000_000, -6, 1, 1_000);
    run_ix(
        Instruction::ConfigureHybridOracle {
            asset_index: 1,
            now_slot: 3,
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
    let after_hybrid = state::read_asset_oracle_profile(&market.data, 1).unwrap();
    assert_eq!(after_hybrid.backing_trade_fee_bps_short, 37);
    assert_eq!(
        after_hybrid.backing_trade_fee_insurance_share_bps_short,
        3_700
    );

    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_DRAIN_ONLY,
        1,
        0,
        0,
    )
    .unwrap();
    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_RETIRE,
        1,
        4,
        0,
    )
    .unwrap();
    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        1,
        5,
        120,
    )
    .unwrap();
    let after_reactivate = state::read_asset_oracle_profile(&market.data, 1).unwrap();
    assert_eq!(after_reactivate.oracle_mode, ORACLE_MODE_MANUAL);
    assert_eq!(after_reactivate.oracle_target_price_e6, 120);
    assert_eq!(
        after_reactivate.backing_trade_fee_bps_short, 37,
        "asset lifecycle resets oracle data but must not erase backing-authority fee policy"
    );
    assert_eq!(
        after_reactivate.backing_trade_fee_insurance_share_bps_short, 3_700,
        "asset lifecycle resets oracle data but must not erase backing fee split"
    );
    let (cfg, _) = state::read_market(&market.data).unwrap();
    assert_eq!(cfg.backing_trade_fee_policy_count, 1);
}

#[test]
fn v16_wrapper_maintenance_fee_sync_charges_recurring_fee_without_forcing_local_touch() {
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
    assert_eq!(account.pnl, 0);
    assert_eq!(
        account.capital, 990,
        "standalone maintenance fee sync charges only the recurring fee; local position settlement remains on lifecycle paths"
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
    let mut market = market_account_with_capacity(3);
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account_for_market_slots(3);
    let mut short_account = portfolio_account_for_market_slots(3);

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
fn v16_wrapper_trade_rejects_corrupt_unconfigured_tail_capacity_fail_closed() {
    let mut admin = signer();
    let mut market = market_account_with_capacity(2);
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut long_account = portfolio_account_for_market_slots(2);
    let mut short_account = portfolio_account_for_market_slots(2);

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

    let inactive_slot = 1usize;
    let slot_start = HEADER_LEN
        + WRAPPER_CONFIG_LEN
        + MarketGroupV16HeaderAccount::dynamic_asset_slot_offset::<[u8; ASSET_ORACLE_WRAPPER_LEN]>(
            inactive_slot,
        )
        .unwrap()
        + ASSET_ORACLE_WRAPPER_LEN;
    let market_id_offset = core::mem::offset_of!(EngineAssetSlotV16Account, asset)
        + core::mem::offset_of!(AssetStateV16Account, market_id);
    market.data[slot_start + market_id_offset] = 7;
    assert!(
        state::read_market(&market.data).is_err(),
        "full debug decode still rejects corrupt unconfigured tail slots"
    );

    let before_market = market.data.clone();
    let before_long = long_account.data.clone();
    let before_short = short_account.data.clone();
    let result = run_ix(
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
    );
    assert_eq!(
        result,
        Err(percolator_prog::error::PercolatorError::EngineInvalidConfig.into())
    );
    assert_eq!(market.data, before_market);
    assert_eq!(long_account.data, before_long);
    assert_eq!(short_account.data, before_short);
}

#[test]
fn v16_wrapper_dynamic_market_account_can_activate_beyond_fixed_runtime_window() {
    let mut admin = signer();
    let base_capacity = percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS as usize;
    let grown_capacity = base_capacity + 1;
    let mut market = market_account_with_capacity(grown_capacity);

    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_portfolio_assets,
                ..
            } = ix
            {
                *max_portfolio_assets = percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS;
            }
        }),
    );
    assert_eq!(
        state::market_slot_capacity(&market.data).unwrap(),
        grown_capacity
    );

    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        base_capacity as u16,
        grown_capacity as u64,
        1_000 + base_capacity as u64,
    )
    .unwrap();
    assert_eq!(
        state::market_slot_capacity(&market.data).unwrap(),
        grown_capacity,
        "append activation must consume the next dynamic market slot"
    );

    let (_, mode, current_slot, effective_price, _) =
        state::read_market_trade_preflight(&market.data, grown_capacity - 1).unwrap();
    assert_eq!(mode, MarketModeV16::Live);
    assert_eq!(current_slot, grown_capacity as u64);
    assert_eq!(effective_price, 1_000 + (grown_capacity - 1) as u64);
    let profile = state::read_asset_oracle_profile(&market.data, grown_capacity - 1).unwrap();
    assert_eq!(profile.oracle_target_price_e6, effective_price);
}

#[test]
fn v16_wrapper_existing_portfolio_with_growth_capacity_survives_market_append() {
    let mut admin = signer();
    let base_capacity = percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS as usize;
    let grown_capacity = base_capacity + 1;
    let mut market = market_account_with_capacity(grown_capacity);
    let mut owner = signer();
    let mut portfolio = portfolio_account_for_market_slots(grown_capacity);

    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_portfolio_assets,
                ..
            } = ix
            {
                *max_portfolio_assets = percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS;
            }
        }),
    );
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    let initial_portfolio_len = portfolio.data.len();
    let initial_domain_count = state::read_portfolio(&portfolio.data)
        .unwrap()
        .source_claim_market_id
        .len();

    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        base_capacity as u16,
        grown_capacity as u64,
        1_000 + base_capacity as u64,
    )
    .unwrap();
    assert_eq!(portfolio.data.len(), initial_portfolio_len);

    deposit(&mut owner, &mut market, &mut portfolio, 1);
    let required_len = state::portfolio_account_len_for_market_slots(grown_capacity).unwrap();
    let account = state::read_portfolio(&portfolio.data).unwrap();
    assert_eq!(
        portfolio.data.len(),
        required_len,
        "portfolio source-domain storage must match the appended market count"
    );
    assert_eq!(
        account.source_claim_market_id.len(),
        grown_capacity * 2,
        "portfolio source-domain capacity must track configured markets"
    );
    assert_eq!(initial_domain_count, grown_capacity * 2);
    assert_eq!(account.capital, 1);
}

#[test]
fn v16_wrapper_market_account_capacity_is_declared_by_account_length() {
    let mut admin = signer();
    let mut market = market_account_with_capacity(14);
    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_portfolio_assets,
                ..
            } = ix
            {
                *max_portfolio_assets = 14;
            }
        }),
    );

    assert_eq!(state::market_slot_capacity(&market.data).unwrap(), 14);
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(group.config.max_market_slots, 14);

    let mut too_small = market_account_with_capacity(13);
    let mut mint = mint_account();
    let res = run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_portfolio_assets,
                ..
            } = ix
            {
                *max_portfolio_assets = 14;
            }
        }),
        &mut [&mut admin, &mut too_small, &mut mint],
    );
    assert_eq!(res, Err(ProgramError::InvalidAccountData));
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
        Instruction::ConfigureEwmaMark {
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
        ORACLE_MODE_EWMA_MARK
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
        Instruction::ConfigureEwmaMark {
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
        Instruction::PushEwmaMark {
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

    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        let prediction_budget = group.insurance_domain_budget[2] + group.insurance_domain_budget[3];
        group.insurance -= prediction_budget;
        group.vault -= prediction_budget;
        group.insurance_domain_budget[2] = 0;
        group.insurance_domain_budget[3] = 0;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }
    let (_, cleared_group) = state::read_market(&market.data).unwrap();

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

    let invalid_index = 3u16;
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

    let mut pass_count = 1usize; // invalid gap append rejection above.
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
        core::mem::size_of::<MarketGroupV16HeaderAccount>()
    );
    assert_eq!(
        PORTFOLIO_STATE_LEN,
        core::mem::size_of::<PortfolioAccountV16Account>()
    );
    assert_eq!(
        WRAPPER_CONFIG_LEN,
        core::mem::size_of::<state::WrapperConfigV16>()
    );
    assert_eq!(core::mem::align_of::<MarketGroupV16HeaderAccount>(), 1);
    assert_eq!(core::mem::align_of::<PortfolioAccountV16Account>(), 1);
    assert_eq!(
        MARKET_ASSET_SLOT_LEN,
        core::mem::size_of::<percolator::Market<[u8; ASSET_ORACLE_WRAPPER_LEN]>>(),
        "one dynamic market slot stores wrapper oracle storage plus the engine asset slot"
    );
    assert_eq!(
        MARKET_ACCOUNT_LEN,
        HEADER_LEN
            + WRAPPER_CONFIG_LEN
            + MARKET_GROUP_LEN
            + DEFAULT_MARKET_SLOT_CAPACITY * MARKET_ASSET_SLOT_LEN,
        "default market account length must cover wrapper header/config + dynamic engine header + default slot table"
    );
    let grown_capacity = DEFAULT_MARKET_SLOT_CAPACITY + 1;
    assert_eq!(
        state::market_account_len_for_capacity(grown_capacity).unwrap(),
        HEADER_LEN + WRAPPER_CONFIG_LEN + MARKET_GROUP_LEN + grown_capacity * MARKET_ASSET_SLOT_LEN,
        "dynamic market account length must grow linearly with wrapper-supplied slot capacity"
    );
    assert_eq!(
        PORTFOLIO_ACCOUNT_LEN,
        HEADER_LEN
            + PORTFOLIO_STATE_LEN
            + DEFAULT_MARKET_SLOT_CAPACITY * 2 * PORTFOLIO_SOURCE_DOMAIN_LEN,
        "default portfolio account length must cover fixed state plus one source domain per side per default market"
    );
    assert_eq!(
        state::portfolio_account_len_for_market_slots(grown_capacity).unwrap(),
        HEADER_LEN + PORTFOLIO_STATE_LEN + grown_capacity * 2 * PORTFOLIO_SOURCE_DOMAIN_LEN,
        "portfolio account length must grow with market-domain storage"
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
    let market_bool_off = percolator_prog::constants::MARKET_GROUP_OFF
        + core::mem::offset_of!(MarketGroupV16HeaderAccount, bankruptcy_hlock_active);
    corrupt_market[market_bool_off] = 2;
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

    let mut bad_primary_mint = cfg;
    bad_primary_mint.collateral_mint = [0u8; 32];
    assert_bad_cfg(
        bad_primary_mint,
        "primary base-unit mint must not be empty in persisted config",
    );

    let mut bad_secondary_mint = cfg;
    bad_secondary_mint.secondary_collateral_mint = cfg.collateral_mint;
    assert_bad_cfg(
        bad_secondary_mint,
        "secondary base-unit mint must not alias the primary mint",
    );

    let mut bad_maintenance_reward = cfg;
    bad_maintenance_reward.maintenance_cranker_fee_share_bps = 10_001;
    assert_bad_cfg(
        bad_maintenance_reward,
        "corrupt maintenance reward share must not be trusted on read",
    );

    let mut bad_backing_trade_fee = cfg;
    bad_backing_trade_fee.backing_trade_fee_bps_long = 10_001;
    assert_bad_cfg(
        bad_backing_trade_fee,
        "corrupt long-domain backing trade fee must not be trusted on read",
    );

    let mut bad_backing_trade_fee = cfg;
    bad_backing_trade_fee.backing_trade_fee_bps_short = 10_001;
    assert_bad_cfg(
        bad_backing_trade_fee,
        "corrupt short-domain backing trade fee must not be trusted on read",
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

    let mut bad_ewma_mark_stale_hybrid_knob = cfg;
    bad_ewma_mark_stale_hybrid_knob.oracle_mode = ORACLE_MODE_EWMA_MARK;
    bad_ewma_mark_stale_hybrid_knob.mark_ewma_e6 = 100;
    bad_ewma_mark_stale_hybrid_knob.mark_ewma_halflife_slots = 1;
    bad_ewma_mark_stale_hybrid_knob.unit_scale = 6;
    assert_bad_cfg(
        bad_ewma_mark_stale_hybrid_knob,
        "EwmaMark oracle mode must not carry stale hybrid conversion knobs",
    );

    let mut bad_bool = cfg;
    bad_bool.insurance_withdraw_deposits_only = 2;
    assert_bad_cfg(
        bad_bool,
        "persistent wrapper booleans must reject invalid byte patterns",
    );

    let mut bad_live_insurance_policy = cfg;
    bad_live_insurance_policy.insurance_withdraw_max_bps = 10_000;
    bad_live_insurance_policy.insurance_withdraw_deposits_only = 0;
    bad_live_insurance_policy.insurance_withdraw_cooldown_slots = 1;
    assert_bad_cfg(
        bad_live_insurance_policy,
        "non-deposit-only live insurance policy must not persist a full-drain cap",
    );

    let mut bad_live_insurance_cooldown = cfg;
    bad_live_insurance_cooldown.insurance_withdraw_max_bps = 5_000;
    bad_live_insurance_cooldown.insurance_withdraw_deposits_only = 0;
    bad_live_insurance_cooldown.insurance_withdraw_cooldown_slots = 0;
    assert_bad_cfg(
        bad_live_insurance_cooldown,
        "non-deposit-only live insurance policy must not persist a zero cooldown",
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

    let invalid_bankrupt_close_lifetime = run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_bankrupt_close_lifetime_slots,
                ..
            } = ix
            {
                *max_bankrupt_close_lifetime_slots = 0;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    );
    assert_err_and_market_unchanged(invalid_bankrupt_close_lifetime, &market, &before);

    let invalid_h_max = run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket { h_max, .. } = ix {
                *h_max = (BOUND_SCALE as u64) + 1;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    );
    assert_err_and_market_unchanged(invalid_h_max, &market, &before);

    let invalid_maintenance_fee = run_ix(
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                maintenance_fee_per_slot,
                ..
            } = ix
            {
                *maintenance_fee_per_slot = percolator::MAX_PROTOCOL_FEE_ABS + 1;
            }
        }),
        &mut [&mut admin, &mut market, &mut mint],
    );
    assert_err_and_market_unchanged(invalid_maintenance_fee, &market, &before);
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
fn v16_wrapper_top_up_paths_reject_after_permissionless_resolve_maturity() {
    let mut admin = signer();
    let mut market = market_account();
    let mut bucket_authority = signer();

    let mint = init_market(&mut admin, &mut market);
    run_ix(
        Instruction::UpdateAuthority {
            kind: processor::AUTHORITY_BACKING_BUCKET,
            new_pubkey: bucket_authority.key.to_bytes(),
        },
        &mut [&mut admin, &mut bucket_authority, &mut market],
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
    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        group.current_slot = 5;
        group.slot_last = 5;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }

    let mut source = user_token_account(admin.key, mint, 100);
    let mut vault = vault_token_account(&market, mint, 0);
    let mut token_program = token_program_account();
    let before = market.data.clone();
    let top_up_insurance = run_ix(
        Instruction::TopUpInsurance { amount: 100 },
        &mut [
            &mut admin,
            &mut market,
            &mut source,
            &mut vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(top_up_insurance, &market, &before);

    let mut source = user_token_account(bucket_authority.key, mint, 100);
    let before = market.data.clone();
    let top_up_backing = run_ix(
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
    );
    assert_err_and_market_unchanged(top_up_backing, &market, &before);
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
    let mut vault = vault_token_account(&market, mint, 100);
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
    let mut vault = vault_token_account(&market, mint, 0);
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

    let mut dest = user_token_account(admin.key, mint, 0);
    let mut vault_auth = vault_authority_account(&market);
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
fn v16_wrapper_dynamic_asset_stores_domain_authorities_and_rejects_zero_authority() {
    let mut admin = signer();
    let mut market = market_account();
    init_market(&mut admin, &mut market);

    let insurance_authority = Pubkey::new_unique().to_bytes();
    let insurance_operator = Pubkey::new_unique().to_bytes();
    let backing_bucket_authority = Pubkey::new_unique().to_bytes();
    update_asset_lifecycle_with_authorities(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        1,
        1,
        100,
        insurance_authority,
        insurance_operator,
        backing_bucket_authority,
    )
    .unwrap();

    let profile = state::read_asset_oracle_profile(&market.data, 1).unwrap();
    assert_eq!(profile.insurance_authority, insurance_authority);
    assert_eq!(profile.insurance_operator, insurance_operator);
    assert_eq!(profile.backing_bucket_authority, backing_bucket_authority);
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(group.insurance_domain_budget[2], 0);
    assert_eq!(group.insurance_domain_budget[3], 0);
    assert_eq!(group.insurance_domain_spent[2], 0);
    assert_eq!(group.insurance_domain_spent[3], 0);

    let before = market.data.clone();
    let reject = update_asset_lifecycle_with_authorities(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        2,
        2,
        100,
        [0u8; 32],
        insurance_operator,
        backing_bucket_authority,
    );
    assert_err_and_market_unchanged(reject, &market, &before);
}

#[test]
fn v16_wrapper_non_main_domain_insurance_isolated_from_global_withdrawals() {
    let mut admin = signer();
    let mut market = market_account();
    let mut insurance_authority = signer();
    let mut insurance_operator = signer();
    let backing_bucket_authority = Pubkey::new_unique().to_bytes();
    let mint = init_market(&mut admin, &mut market);

    update_asset_lifecycle_with_authorities(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        1,
        1,
        100,
        insurance_authority.key.to_bytes(),
        insurance_operator.key.to_bytes(),
        backing_bucket_authority,
    )
    .unwrap();

    let mut source = user_token_account(insurance_authority.key, mint, 100);
    let mut vault = vault_token_account(&market, mint, 0);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::TopUpInsuranceDomain {
            domain: 2,
            amount: 100,
        },
        &mut [
            &mut insurance_authority,
            &mut market,
            &mut source,
            &mut vault,
            &mut token_program,
        ],
    )
    .unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(group.insurance, 100);
    assert_eq!(group.vault, 100);
    assert_eq!(group.insurance_domain_budget[2], 100);

    run_ix(
        Instruction::UpdateInsurancePolicy {
            max_bps: 5_000,
            deposits_only: 0,
            cooldown_slots: 1,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    let before_global_withdraw = market.data.clone();
    let mut global_dest = user_token_account(admin.key, mint, 0);
    let mut vault = vault_token_account(&market, mint, 100);
    let mut vault_auth = vault_authority_account(&market);
    let global_withdraw = run_ix(
        Instruction::WithdrawInsuranceLimited { amount: 1 },
        &mut [
            &mut admin,
            &mut market,
            &mut global_dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(global_withdraw, &market, &before_global_withdraw);

    let mut domain_dest = user_token_account(insurance_operator.key, mint, 0);
    run_ix(
        Instruction::WithdrawInsuranceDomain {
            domain: 2,
            amount: 100,
        },
        &mut [
            &mut insurance_operator,
            &mut market,
            &mut domain_dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    )
    .unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(group.insurance, 0);
    assert_eq!(group.vault, 0);
    assert_eq!(group.insurance_domain_budget[2], 0);
}

#[test]
fn v16_wrapper_domain_withdrawals_reject_admin_before_shutdown_and_accept_secondary_mint() {
    let mut admin = signer();
    let mut market = market_account();
    let mut primary_mint = mint_account();
    let primary_key = primary_mint.key;
    let mut secondary_mint = mint_account();
    let secondary_key = secondary_mint.key;
    let mut insurance_authority = signer();
    let mut insurance_operator = signer();
    let mut backing_authority = signer();

    run_ix(
        default_init_market_ix(),
        &mut [&mut admin, &mut market, &mut primary_mint],
    )
    .unwrap();
    configure_base_unit_mints(
        &mut admin,
        &mut market,
        &mut primary_mint,
        &mut secondary_mint,
    )
    .unwrap();
    update_asset_lifecycle_with_authorities(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        1,
        1,
        100,
        insurance_authority.key.to_bytes(),
        insurance_operator.key.to_bytes(),
        backing_authority.key.to_bytes(),
    )
    .unwrap();

    let mut token_program = token_program_account();
    let mut insurance_source = user_token_account(insurance_authority.key, primary_key, 11);
    let mut primary_vault = vault_token_account(&market, primary_key, 0);
    run_ix(
        Instruction::TopUpInsuranceDomain {
            domain: 2,
            amount: 11,
        },
        &mut [
            &mut insurance_authority,
            &mut market,
            &mut insurance_source,
            &mut primary_vault,
            &mut token_program,
        ],
    )
    .unwrap();
    let mut backing_source = user_token_account(backing_authority.key, primary_key, 30);
    run_ix(
        Instruction::TopUpBackingBucket {
            domain: 2,
            amount: 30,
            expiry_slot: 10,
        },
        &mut [
            &mut backing_authority,
            &mut market,
            &mut backing_source,
            &mut primary_vault,
            &mut token_program,
        ],
    )
    .unwrap();
    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        group.source_backing_buckets[2].utilization_fee_earnings = 7;
        group.vault += 7;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }

    let mut vault_auth = vault_authority_account(&market);
    let mut admin_dest = user_token_account(admin.key, secondary_key, 0);
    let mut secondary_vault = vault_token_account(&market, secondary_key, 48);
    let before_admin_insurance = market.data.clone();
    let admin_insurance = run_ix(
        Instruction::WithdrawInsuranceDomain {
            domain: 2,
            amount: 1,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut admin_dest,
            &mut secondary_vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(admin_insurance, &market, &before_admin_insurance);

    let before_admin_backing = market.data.clone();
    let admin_backing = run_ix(
        Instruction::WithdrawBackingBucket {
            domain: 2,
            amount: 1,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut admin_dest,
            &mut secondary_vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(admin_backing, &market, &before_admin_backing);

    let mut admin_backing_ledger = backing_domain_ledger_account();
    let before_admin_earnings = market.data.clone();
    let admin_earnings = run_ix(
        Instruction::WithdrawBackingBucketEarnings {
            domain: 2,
            amount: 1,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut admin_backing_ledger,
            &mut admin_dest,
            &mut secondary_vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(admin_earnings, &market, &before_admin_earnings);

    let mut mismatched_dest = user_token_account(insurance_operator.key, secondary_key, 0);
    let mut mismatched_primary_vault = vault_token_account(&market, primary_key, 48);
    let before_mismatched_mints = market.data.clone();
    let mismatched_mints = run_ix(
        Instruction::WithdrawInsuranceDomain {
            domain: 2,
            amount: 1,
        },
        &mut [
            &mut insurance_operator,
            &mut market,
            &mut mismatched_dest,
            &mut mismatched_primary_vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(mismatched_mints, &market, &before_mismatched_mints);

    let mut secondary_vault = vault_token_account(&market, secondary_key, 48);
    let mut insurance_dest = user_token_account(insurance_operator.key, secondary_key, 0);
    run_ix(
        Instruction::WithdrawInsuranceDomain {
            domain: 2,
            amount: 11,
        },
        &mut [
            &mut insurance_operator,
            &mut market,
            &mut insurance_dest,
            &mut secondary_vault,
            &mut vault_auth,
            &mut token_program,
        ],
    )
    .unwrap();

    let mut backing_ledger = backing_domain_ledger_account();
    let mut backing_dest = user_token_account(backing_authority.key, secondary_key, 0);
    run_ix(
        Instruction::WithdrawBackingBucketEarnings {
            domain: 2,
            amount: 7,
        },
        &mut [
            &mut backing_authority,
            &mut market,
            &mut backing_ledger,
            &mut backing_dest,
            &mut secondary_vault,
            &mut vault_auth,
            &mut token_program,
        ],
    )
    .unwrap();

    run_ix(
        Instruction::WithdrawBackingBucket {
            domain: 2,
            amount: 30,
        },
        &mut [
            &mut backing_authority,
            &mut market,
            &mut backing_dest,
            &mut secondary_vault,
            &mut vault_auth,
            &mut token_program,
        ],
    )
    .unwrap();

    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(group.vault, 0);
    assert_eq!(group.insurance, 0);
    assert_eq!(group.insurance_domain_budget[2], 0);
    assert_eq!(
        group.source_backing_buckets[2].fresh_unliened_backing_num,
        0
    );
    assert_eq!(group.source_backing_buckets[2].utilization_fee_earnings, 0);
}

#[test]
fn v16_wrapper_backing_bucket_authority_is_domain_scoped_for_dynamic_assets() {
    let mut admin = signer();
    let mut market = market_account();
    let mut backing_authority = signer();
    let mint = init_market(&mut admin, &mut market);

    update_asset_lifecycle_with_authorities(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        1,
        1,
        100,
        Pubkey::new_unique().to_bytes(),
        Pubkey::new_unique().to_bytes(),
        backing_authority.key.to_bytes(),
    )
    .unwrap();

    let before = market.data.clone();
    let mut admin_source = user_token_account(admin.key, mint, 10);
    let mut vault = vault_token_account(&market, mint, 0);
    let mut token_program = token_program_account();
    let unauthorized = run_ix(
        Instruction::TopUpBackingBucket {
            domain: 2,
            amount: 10,
            expiry_slot: 10,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut admin_source,
            &mut vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(unauthorized, &market, &before);

    let mut source = user_token_account(backing_authority.key, mint, 10);
    run_ix(
        Instruction::TopUpBackingBucket {
            domain: 2,
            amount: 10,
            expiry_slot: 10,
        },
        &mut [
            &mut backing_authority,
            &mut market,
            &mut source,
            &mut vault,
            &mut token_program,
        ],
    )
    .unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(
        group.source_backing_buckets[2].fresh_unliened_backing_num,
        10 * BOUND_SCALE
    );
}

#[test]
fn v16_wrapper_asset_zero_cannot_be_retired() {
    let mut admin = signer();
    let mut market = market_account();

    init_market(&mut admin, &mut market);
    let before = market.data.clone();
    let retire_base = update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_RETIRE,
        0,
        1,
        0,
    );
    assert_err_and_market_unchanged(retire_base, &market, &before);

    let (cfg, group) = state::read_market(&market.data).unwrap();
    assert_eq!(cfg.free_market_slot_count, 0);
    assert_eq!(group.assets[0].lifecycle, AssetLifecycleV16::Active);
}

#[test]
fn v16_wrapper_asset_retire_rejects_nonzero_domain_insurance_budget() {
    let mut admin = signer();
    let mut market = market_account();
    let mut insurance_authority = signer();
    let mint = init_market(&mut admin, &mut market);

    update_asset_lifecycle_with_authorities(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        1,
        1,
        100,
        insurance_authority.key.to_bytes(),
        Pubkey::new_unique().to_bytes(),
        Pubkey::new_unique().to_bytes(),
    )
    .unwrap();
    let mut source = user_token_account(insurance_authority.key, mint, 1);
    let mut vault = vault_token_account(&market, mint, 0);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::TopUpInsuranceDomain {
            domain: 2,
            amount: 1,
        },
        &mut [
            &mut insurance_authority,
            &mut market,
            &mut source,
            &mut vault,
            &mut token_program,
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
    let before_retire = market.data.clone();
    let retire = update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_RETIRE,
        1,
        10,
        0,
    );
    assert_err_and_market_unchanged(retire, &market, &before_retire);
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
fn v16_wrapper_withdraw_backing_bucket_returns_only_unencumbered_backing() {
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

    let mut source = user_token_account(bucket_authority.key, mint, 100);
    let mut vault = vault_token_account(&market, mint, 100);
    let mut token_program = token_program_account();
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

    let (_, after_topup_group) = state::read_market(&market.data).unwrap();
    let risk_epoch_after_topup = after_topup_group.risk_epoch;
    let source_epoch_after_topup = after_topup_group.source_credit[1].credit_epoch;

    let topped_up = market.data.clone();
    let mut attacker_dest = user_token_account(attacker.key, mint, 0);
    let mut vault_auth = vault_authority_account(&market);
    let unauthorized = run_ix(
        Instruction::WithdrawBackingBucket {
            domain: 1,
            amount: 1,
        },
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

    let mut dest = user_token_account(bucket_authority.key, mint, 0);
    run_ix(
        Instruction::WithdrawBackingBucket {
            domain: 1,
            amount: 40,
        },
        &mut [
            &mut bucket_authority,
            &mut market,
            &mut dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    )
    .unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(group.vault, 60);
    assert_eq!(group.insurance, 0);
    assert_eq!(group.c_tot, 0);
    assert_eq!(
        group.risk_epoch,
        risk_epoch_after_topup + 1,
        "withdrawing fresh backing changes risk inputs and must stale existing health certificates"
    );
    assert_eq!(
        group.source_credit[1].credit_epoch,
        source_epoch_after_topup + 1,
        "backing withdrawal must mirror engine source-credit mutation epoch semantics"
    );
    assert_eq!(
        group.source_backing_buckets[1].fresh_unliened_backing_num,
        60 * BOUND_SCALE
    );
    assert_eq!(
        group.source_credit[1].fresh_reserved_backing_num,
        60 * BOUND_SCALE
    );

    let before_overdraw = market.data.clone();
    let overdraw = run_ix(
        Instruction::WithdrawBackingBucket {
            domain: 1,
            amount: 61,
        },
        &mut [
            &mut bucket_authority,
            &mut market,
            &mut dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(overdraw, &market, &before_overdraw);

    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        group.source_credit[1].positive_claim_bound_num = 60 * BOUND_SCALE;
        group.source_credit[1].exact_positive_claim_num = 60 * BOUND_SCALE;
        group.source_credit[1].credit_rate_num = percolator::CREDIT_RATE_SCALE;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }
    let claim_backed = market.data.clone();
    let claim_dilution = run_ix(
        Instruction::WithdrawBackingBucket {
            domain: 1,
            amount: 1,
        },
        &mut [
            &mut bucket_authority,
            &mut market,
            &mut dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(claim_dilution, &market, &claim_backed);
}

#[test]
fn v16_wrapper_withdraw_backing_bucket_rejects_stress_and_allows_full_clean_drain() {
    let mut admin = signer();
    let mut market = market_account();

    let mint = init_market(&mut admin, &mut market);
    let mut source = user_token_account(admin.key, mint, 25);
    let mut vault = vault_token_account(&market, mint, 25);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::TopUpBackingBucket {
            domain: 1,
            amount: 25,
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

    let mut dest = user_token_account(admin.key, mint, 0);
    let mut vault_auth = vault_authority_account(&market);
    let zero = run_ix(
        Instruction::WithdrawBackingBucket {
            domain: 1,
            amount: 0,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert!(zero.is_err());

    let topped_up = market.data.clone();
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
            Instruction::WithdrawBackingBucket {
                domain: 1,
                amount: 1,
            },
            &mut [
                &mut admin,
                &mut market,
                &mut dest,
                &mut vault,
                &mut vault_auth,
                &mut token_program,
            ],
        );
        assert_err_and_market_unchanged(stressed_withdraw, &market, &stressed);
    }
    market.data.copy_from_slice(&topped_up);

    run_ix(
        Instruction::WithdrawBackingBucket {
            domain: 1,
            amount: 25,
        },
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
    assert_eq!(group.vault, 0);
    assert_eq!(group.source_credit[1].fresh_reserved_backing_num, 0);
    assert_eq!(
        group.source_backing_buckets[1].status,
        BackingBucketStatusV16::Empty
    );
    assert_eq!(group.source_backing_buckets[1].expiry_slot, 0);
}

#[test]
fn v16_wrapper_withdraw_backing_bucket_rejects_bad_custody_accounts() {
    let mut admin = signer();
    let mut market = market_account();
    let mint = init_market(&mut admin, &mut market);

    let mut source = user_token_account(admin.key, mint, 10);
    let mut vault = vault_token_account(&market, mint, 10);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::TopUpBackingBucket {
            domain: 1,
            amount: 10,
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

    let attacker = signer();
    let mut wrong_dest_owner = user_token_account(attacker.key, mint, 0);
    let mut vault_auth = vault_authority_account(&market);
    let before_wrong_dest = market.data.clone();
    let wrong_dest = run_ix(
        Instruction::WithdrawBackingBucket {
            domain: 1,
            amount: 1,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut wrong_dest_owner,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(wrong_dest, &market, &before_wrong_dest);

    let mut dest = user_token_account(admin.key, mint, 0);
    let mut wrong_vault_auth = signer();
    let before_wrong_auth = market.data.clone();
    let wrong_auth = run_ix(
        Instruction::WithdrawBackingBucket {
            domain: 1,
            amount: 1,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut dest,
            &mut vault,
            &mut wrong_vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(wrong_auth, &market, &before_wrong_auth);

    let wrong_mint = Pubkey::new_unique();
    let mut wrong_vault = vault_token_account(&market, wrong_mint, 10);
    let before_wrong_vault = market.data.clone();
    let wrong_vault_result = run_ix(
        Instruction::WithdrawBackingBucket {
            domain: 1,
            amount: 1,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut dest,
            &mut wrong_vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(wrong_vault_result, &market, &before_wrong_vault);
}

#[test]
fn v16_wrapper_backing_domain_ledger_tracks_authority_topup_earnings_and_withdraw() {
    let mut admin = signer();
    let mut market = market_account();
    let mut ledger = backing_domain_ledger_account();
    let mint = init_market(&mut admin, &mut market);

    let mut source = user_token_account(admin.key, mint, 100);
    let mut vault = vault_token_account(&market, mint, 0);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::TopUpBackingBucket {
            domain: 1,
            amount: 100,
            expiry_slot: 10,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut source,
            &mut vault,
            &mut token_program,
            &mut ledger,
        ],
    )
    .unwrap();

    let ledger_state = state::read_backing_domain_ledger(&ledger.data).unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(ledger_state.market_group, market.key.to_bytes());
    assert_eq!(ledger_state.authority, admin.key.to_bytes());
    assert_eq!(ledger_state.domain, 1);
    assert_eq!(ledger_state.total_principal_atoms, 100);
    assert_eq!(ledger_state.total_deposited_atoms, 100);
    assert_eq!(ledger_state.total_earnings_atoms, 0);
    assert_eq!(ledger_state.last_observed_bucket_earnings_atoms, 0);
    assert_eq!(group.vault, 100);
    assert_eq!(
        group.source_backing_buckets[1].fresh_unliened_backing_num,
        100 * BOUND_SCALE
    );

    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        group.source_backing_buckets[1].utilization_fee_earnings = 30;
        group.vault += 30;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }
    run_ix(
        Instruction::SyncBackingDomainLedger { domain: 1 },
        &mut [&mut admin, &mut market, &mut ledger],
    )
    .unwrap();
    let ledger_state = state::read_backing_domain_ledger(&ledger.data).unwrap();
    assert_eq!(ledger_state.last_observed_bucket_earnings_atoms, 30);
    assert_eq!(ledger_state.total_earnings_atoms, 30);

    vault = vault_token_account(&market, mint, 130);
    let mut dest = user_token_account(admin.key, mint, 0);
    let mut vault_auth = vault_authority_account(&market);
    run_ix(
        Instruction::WithdrawBackingBucketEarnings {
            domain: 1,
            amount: 20,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut ledger,
            &mut dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    )
    .unwrap();
    let ledger_state = state::read_backing_domain_ledger(&ledger.data).unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(ledger_state.total_earnings_withdrawn_atoms, 20);
    assert_eq!(ledger_state.total_earnings_atoms, 30);
    assert_eq!(ledger_state.last_observed_bucket_earnings_atoms, 10);
    assert_eq!(group.source_backing_buckets[1].utilization_fee_earnings, 10);
    assert_eq!(group.vault, 110);

    run_ix(
        Instruction::WithdrawBackingBucket {
            domain: 1,
            amount: 40,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
            &mut ledger,
        ],
    )
    .unwrap();
    let ledger_state = state::read_backing_domain_ledger(&ledger.data).unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(ledger_state.total_principal_atoms, 60);
    assert_eq!(ledger_state.total_principal_withdrawn_atoms, 40);
    assert_eq!(group.vault, 70);
    assert_eq!(
        group.source_backing_buckets[1].fresh_unliened_backing_num,
        60 * BOUND_SCALE
    );
}

#[test]
fn v16_wrapper_backing_domain_ledger_tracks_unavailable_principal_loss_and_recovery() {
    let mut admin = signer();
    let mut market = market_account();
    let mut ledger = backing_domain_ledger_account();
    let mint = init_market(&mut admin, &mut market);

    let mut source = user_token_account(admin.key, mint, 50);
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
            &mut ledger,
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
    run_ix(
        Instruction::SyncBackingDomainLedger { domain: 1 },
        &mut [&mut admin, &mut market, &mut ledger],
    )
    .unwrap();
    let ledger_state = state::read_backing_domain_ledger(&ledger.data).unwrap();
    assert_eq!(ledger_state.cumulative_loss_atoms, 40);
    assert_eq!(ledger_state.cumulative_recovery_atoms, 0);
    assert_eq!(ledger_state.last_observed_unavailable_principal_atoms, 40);

    run_ix(
        Instruction::TopUpBackingBucket {
            domain: 1,
            amount: 10,
            expiry_slot: 20,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut source,
            &mut vault,
            &mut token_program,
            &mut ledger,
        ],
    )
    .unwrap();
    run_ix(
        Instruction::SyncBackingDomainLedger { domain: 1 },
        &mut [&mut admin, &mut market, &mut ledger],
    )
    .unwrap();
    let ledger_state = state::read_backing_domain_ledger(&ledger.data).unwrap();
    assert_eq!(ledger_state.total_principal_atoms, 50);
    assert_eq!(ledger_state.cumulative_loss_atoms, 40);
    assert_eq!(ledger_state.cumulative_recovery_atoms, 10);
    assert_eq!(ledger_state.last_observed_unavailable_principal_atoms, 30);
}

#[test]
fn v16_wrapper_backing_domain_ledger_rejects_wrong_authority_and_domain() {
    let mut admin = signer();
    let mut attacker = signer();
    let mut market = market_account();
    let mut ledger = backing_domain_ledger_account();
    let mint = init_market(&mut admin, &mut market);

    let mut source = user_token_account(admin.key, mint, 10);
    let mut vault = vault_token_account(&market, mint, 0);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::TopUpBackingBucket {
            domain: 1,
            amount: 10,
            expiry_slot: 10,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut source,
            &mut vault,
            &mut token_program,
            &mut ledger,
        ],
    )
    .unwrap();

    let before_wrong_authority = market.data.clone();
    let wrong_authority = run_ix(
        Instruction::SyncBackingDomainLedger { domain: 1 },
        &mut [&mut attacker, &mut market, &mut ledger],
    );
    assert_err_and_market_unchanged(wrong_authority, &market, &before_wrong_authority);

    let before_wrong_domain = market.data.clone();
    let wrong_domain = run_ix(
        Instruction::SyncBackingDomainLedger { domain: 0 },
        &mut [&mut admin, &mut market, &mut ledger],
    );
    assert_err_and_market_unchanged(wrong_domain, &market, &before_wrong_domain);
}

#[test]
fn v16_wrapper_insurance_ledger_tracks_topup_profit_loss_and_withdrawal() {
    let mut admin = signer();
    let mut market = market_account();
    let mut ledger = insurance_ledger_account();
    let mint = init_market(&mut admin, &mut market);

    let mut source = user_token_account(admin.key, mint, 100);
    let mut vault = vault_token_account(&market, mint, 0);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::TopUpInsurance { amount: 100 },
        &mut [
            &mut admin,
            &mut market,
            &mut source,
            &mut vault,
            &mut token_program,
            &mut ledger,
        ],
    )
    .unwrap();
    let ledger_state = state::read_insurance_ledger(&ledger.data).unwrap();
    assert_eq!(ledger_state.market_group, market.key.to_bytes());
    assert_eq!(ledger_state.authority, admin.key.to_bytes());
    assert_eq!(ledger_state.total_principal_atoms, 100);
    assert_eq!(ledger_state.total_deposited_atoms, 100);
    assert_eq!(ledger_state.last_observed_insurance_atoms, 100);

    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        group.insurance += 30;
        group.vault += 30;
        group.insurance_domain_budget[0] += 15;
        group.insurance_domain_budget[1] += 15;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }
    run_ix(
        Instruction::SyncInsuranceLedger,
        &mut [&mut admin, &mut market, &mut ledger],
    )
    .unwrap();
    let ledger_state = state::read_insurance_ledger(&ledger.data).unwrap();
    assert_eq!(ledger_state.cumulative_profit_atoms, 30);
    assert_eq!(ledger_state.last_observed_insurance_atoms, 130);

    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        group.insurance -= 20;
        group.vault -= 20;
        group.insurance_domain_budget[0] -= 10;
        group.insurance_domain_budget[1] -= 10;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }
    run_ix(
        Instruction::SyncInsuranceLedger,
        &mut [&mut admin, &mut market, &mut ledger],
    )
    .unwrap();
    let ledger_state = state::read_insurance_ledger(&ledger.data).unwrap();
    assert_eq!(ledger_state.cumulative_loss_atoms, 20);
    assert_eq!(ledger_state.last_observed_insurance_atoms, 110);

    run_ix(
        Instruction::UpdateInsurancePolicy {
            max_bps: 5_000,
            deposits_only: 0,
            cooldown_slots: 1,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    vault = vault_token_account(&market, mint, 110);
    let mut dest = user_token_account(admin.key, mint, 0);
    let mut vault_auth = vault_authority_account(&market);
    run_ix(
        Instruction::WithdrawInsuranceLimited { amount: 10 },
        &mut [
            &mut admin,
            &mut market,
            &mut dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
            &mut ledger,
        ],
    )
    .unwrap();
    let ledger_state = state::read_insurance_ledger(&ledger.data).unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(ledger_state.total_withdrawn_atoms, 10);
    assert_eq!(ledger_state.total_principal_atoms, 90);
    assert_eq!(ledger_state.last_observed_insurance_atoms, 100);
    assert_eq!(group.insurance, 100);
    assert_eq!(group.vault, 100);
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
fn v16_wrapper_backing_top_up_refills_provider_receivable_in_engine() {
    let mut admin = signer();
    let mut market = market_account();
    let mint = init_market(&mut admin, &mut market);

    let mut source = user_token_account(admin.key, mint, 50);
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
    {
        let (_, group) = state::read_market(&market.data).unwrap();
        assert_eq!(
            group.source_credit[1].provider_receivable_num,
            40 * BOUND_SCALE
        );
        assert_eq!(
            group.source_backing_buckets[1].consumed_liened_backing_num,
            40 * BOUND_SCALE
        );
        assert_eq!(group.source_credit[1].fresh_reserved_backing_num, 0);
    }

    run_ix(
        Instruction::TopUpBackingBucket {
            domain: 1,
            amount: 10,
            expiry_slot: 20,
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
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(
        group.source_credit[1].provider_receivable_num,
        30 * BOUND_SCALE,
        "backing top-up must automatically repay provider receivable through the engine API"
    );
    assert_eq!(
        group.source_backing_buckets[1].consumed_liened_backing_num,
        30 * BOUND_SCALE
    );
    assert_eq!(
        group.source_credit[1].fresh_reserved_backing_num,
        10 * BOUND_SCALE
    );
    assert_eq!(
        group.source_backing_buckets[1].fresh_unliened_backing_num,
        10 * BOUND_SCALE
    );
}

#[test]
fn v16_wrapper_exploited_oracle_pnl_cannot_exit_against_unrelated_backing_or_insurance() {
    let mut admin = signer();
    let mut market = market_account();
    let mint = init_market(&mut admin, &mut market);
    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        1,
        1,
        100,
    )
    .unwrap();

    let mut insurance_source = user_token_account(admin.key, mint, 100);
    let mut vault = vault_token_account(&market, mint, 120);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::TopUpInsurance { amount: 100 },
        &mut [
            &mut admin,
            &mut market,
            &mut insurance_source,
            &mut vault,
            &mut token_program,
        ],
    )
    .unwrap();
    let mut unrelated_source = user_token_account(admin.key, mint, 100);
    run_ix(
        Instruction::TopUpBackingBucket {
            domain: 0,
            amount: 100,
            expiry_slot: 10,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut unrelated_source,
            &mut vault,
            &mut token_program,
        ],
    )
    .unwrap();

    let mut owner = signer();
    let mut portfolio = portfolio_account();
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 20);
    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        let mut account = state::read_portfolio(&portfolio.data).unwrap();
        group
            .add_account_source_positive_pnl_not_atomic(&mut account, 2, 50)
            .unwrap();
        state::write_market(&mut market.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut portfolio.data, &account).unwrap();
    }
    run_ix(
        Instruction::PermissionlessCrank {
            action: 0,
            asset_index: 1,
            now_slot: 1,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut owner, &mut market, &mut portfolio],
    )
    .unwrap();

    let before_convert_market = market.data.clone();
    let before_convert_portfolio = portfolio.data.clone();
    let convert = run_ix(
        Instruction::ConvertReleasedPnl { amount: 50 },
        &mut [&mut owner, &mut market, &mut portfolio],
    );
    assert_err_and_market_unchanged(convert, &market, &before_convert_market);
    assert_eq!(portfolio.data, before_convert_portfolio);

    let before_over_withdraw_market = market.data.clone();
    let before_over_withdraw_portfolio = portfolio.data.clone();
    let mut dest = user_token_account(owner.key, mint, 0);
    let mut vault_auth = vault_authority_account(&market);
    let over_withdraw = run_ix(
        Instruction::Withdraw { amount: 21 },
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
    assert_err_and_market_unchanged(over_withdraw, &market, &before_over_withdraw_market);
    assert_eq!(portfolio.data, before_over_withdraw_portfolio);

    withdraw(&mut owner, &mut market, &mut portfolio, 20);
    let (_, group) = state::read_market(&market.data).unwrap();
    let account = state::read_portfolio(&portfolio.data).unwrap();
    assert_eq!(account.capital, 0);
    assert_eq!(account.pnl, 50);
    assert_eq!(group.insurance, 100);
    assert_eq!(group.c_tot, 0);
    assert_eq!(group.source_credit[2].spent_backing_num, 0);
    assert_eq!(group.source_credit[0].spent_backing_num, 0);
    assert_eq!(
        group.source_backing_buckets[0].fresh_unliened_backing_num,
        100 * BOUND_SCALE,
        "unrelated backing must not support an exploited oracle claim"
    );
}

#[test]
fn v16_wrapper_exploited_added_asset_pnl_exit_caps_to_its_source_domain_backing() {
    let mut admin = signer();
    let mut market = market_account();
    let mint = init_market(&mut admin, &mut market);
    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        1,
        1,
        100,
    )
    .unwrap();

    let mut unrelated_source = user_token_account(admin.key, mint, 80);
    let mut corrupt_domain_source = user_token_account(admin.key, mint, 30);
    let mut vault = vault_token_account(&market, mint, 110);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::TopUpBackingBucket {
            domain: 0,
            amount: 80,
            expiry_slot: 10,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut unrelated_source,
            &mut vault,
            &mut token_program,
        ],
    )
    .unwrap();
    run_ix(
        Instruction::TopUpBackingBucket {
            domain: 2,
            amount: 30,
            expiry_slot: 10,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut corrupt_domain_source,
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
            .add_account_source_positive_pnl_not_atomic(&mut account, 2, 100)
            .unwrap();
        state::write_market(&mut market.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut portfolio.data, &account).unwrap();
    }
    run_ix(
        Instruction::PermissionlessCrank {
            action: 0,
            asset_index: 1,
            now_slot: 1,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut owner, &mut market, &mut portfolio],
    )
    .unwrap();

    run_ix(
        Instruction::ConvertReleasedPnl { amount: 100 },
        &mut [&mut owner, &mut market, &mut portfolio],
    )
    .unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    let account = state::read_portfolio(&portfolio.data).unwrap();
    assert_eq!(
        account.capital, 30,
        "manufactured PnL may exit only up to same-domain backing"
    );
    assert_eq!(account.pnl, 0);
    assert_eq!(group.c_tot, 30);
    assert_eq!(group.vault, 110);
    assert_eq!(group.insurance, 0);
    assert_eq!(
        group.source_backing_buckets[2].consumed_liened_backing_num,
        30 * BOUND_SCALE
    );
    assert_eq!(group.source_credit[2].spent_backing_num, 30 * BOUND_SCALE);
    assert_eq!(
        group.source_backing_buckets[0].fresh_unliened_backing_num,
        80 * BOUND_SCALE,
        "same-account cross-margin must not consume unrelated source backing"
    );
    assert_eq!(group.source_credit[0].spent_backing_num, 0);

    let before_over_withdraw_market = market.data.clone();
    let before_over_withdraw_portfolio = portfolio.data.clone();
    let mut dest = user_token_account(owner.key, mint, 0);
    let mut vault_auth = vault_authority_account(&market);
    let over_withdraw = run_ix(
        Instruction::Withdraw { amount: 31 },
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
    assert_err_and_market_unchanged(over_withdraw, &market, &before_over_withdraw_market);
    assert_eq!(portfolio.data, before_over_withdraw_portfolio);

    withdraw(&mut owner, &mut market, &mut portfolio, 30);
    let (_, group) = state::read_market(&market.data).unwrap();
    let account = state::read_portfolio(&portfolio.data).unwrap();
    assert_eq!(account.capital, 0);
    assert_eq!(group.c_tot, 0);
    assert_eq!(group.vault, 80);
    assert_eq!(
        group.source_backing_buckets[0].fresh_unliened_backing_num,
        80 * BOUND_SCALE
    );
}

#[test]
fn v16_wrapper_cross_margin_source_claims_leave_unbacked_corrupt_claim_unconverted() {
    let mut admin = signer();
    let mut market = market_account();
    let mint = init_market(&mut admin, &mut market);
    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        1,
        1,
        100,
    )
    .unwrap();

    let mut legit_source = user_token_account(admin.key, mint, 30);
    let mut vault = vault_token_account(&market, mint, 30);
    let mut token_program = token_program_account();
    run_ix(
        Instruction::TopUpBackingBucket {
            domain: 0,
            amount: 30,
            expiry_slot: 10,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut legit_source,
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
            .add_account_source_positive_pnl_not_atomic(&mut account, 0, 30)
            .unwrap();
        group
            .add_account_source_positive_pnl_not_atomic(&mut account, 2, 100)
            .unwrap();
        state::write_market(&mut market.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut portfolio.data, &account).unwrap();
    }
    run_ix(
        Instruction::PermissionlessCrank {
            action: 0,
            asset_index: 1,
            now_slot: 1,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut owner, &mut market, &mut portfolio],
    )
    .unwrap();

    run_ix(
        Instruction::ConvertReleasedPnl { amount: 130 },
        &mut [&mut owner, &mut market, &mut portfolio],
    )
    .unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    let account = state::read_portfolio(&portfolio.data).unwrap();
    assert_eq!(account.capital, 30);
    assert_eq!(account.pnl, 100);
    assert_eq!(account.source_claim_bound_num[0], 0);
    assert_eq!(account.source_claim_bound_num[2], 100 * BOUND_SCALE);
    assert_eq!(group.source_credit[0].spent_backing_num, 30 * BOUND_SCALE);
    assert_eq!(group.source_credit[2].spent_backing_num, 0);
    assert_eq!(
        group.source_credit[2].credit_rate_num, 0,
        "unbacked corrupt source claim must remain non-withdrawable"
    );

    let before_second_convert_market = market.data.clone();
    let before_second_convert_portfolio = portfolio.data.clone();
    let second_convert = run_ix(
        Instruction::ConvertReleasedPnl { amount: 100 },
        &mut [&mut owner, &mut market, &mut portfolio],
    );
    assert_err_and_market_unchanged(second_convert, &market, &before_second_convert_market);
    assert_eq!(portfolio.data, before_second_convert_portfolio);
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
fn v16_wrapper_insurance_withdraw_disabled_by_default() {
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

    let (cfg, _) = state::read_market(&market.data).unwrap();
    assert_eq!(
        cfg.insurance_withdraw_max_bps, 0,
        "live limited insurance withdrawal must be explicit opt-in"
    );

    let mut dest = user_token_account(admin.key, mint, 0);
    let mut vault_auth = vault_authority_account(&market);
    let before = market.data.clone();
    let default_withdraw = run_ix(
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
    assert_err_and_market_unchanged(default_withdraw, &market, &before);
}

#[test]
fn v16_wrapper_insurance_policy_rejects_live_unbounded_or_zero_cooldown() {
    let mut admin = signer();
    let mut market = market_account();
    init_market(&mut admin, &mut market);

    let cases = [
        (
            Instruction::UpdateInsurancePolicy {
                max_bps: 10_000,
                deposits_only: 0,
                cooldown_slots: 0,
            },
            "non-deposit-only full drain with no cooldown",
        ),
        (
            Instruction::UpdateInsurancePolicy {
                max_bps: 10_000,
                deposits_only: 0,
                cooldown_slots: 1,
            },
            "non-deposit-only full drain is unbounded even with cooldown",
        ),
        (
            Instruction::UpdateInsurancePolicy {
                max_bps: 5_000,
                deposits_only: 0,
                cooldown_slots: 0,
            },
            "non-deposit-only live withdrawal needs a real cooldown",
        ),
    ];
    for (ix, why) in cases {
        let before = market.data.clone();
        let err = run_ix(ix, &mut [&mut admin, &mut market]);
        assert!(
            err.is_err(),
            "{} must be rejected for live limited insurance withdrawal",
            why
        );
        assert_eq!(market.data, before);
    }

    run_ix(
        Instruction::UpdateInsurancePolicy {
            max_bps: 5_000,
            deposits_only: 0,
            cooldown_slots: 1,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
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
    run_ix(
        Instruction::UpdateInsurancePolicy {
            max_bps: 5_000,
            deposits_only: 0,
            cooldown_slots: 1,
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
fn v16_wrapper_withdraw_insurance_limited_is_live_only_and_terminal_uses_authority() {
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
    run_ix(
        Instruction::UpdateInsurancePolicy {
            max_bps: 10_000,
            deposits_only: 1,
            cooldown_slots: 0,
        },
        &mut [&mut admin, &mut market],
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
    let operator_terminal = run_ix(
        Instruction::WithdrawInsuranceLimited { amount: 50 },
        &mut [
            &mut operator,
            &mut market,
            &mut operator_dest,
            &mut vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(operator_terminal, &market, &resolved);

    run_ix(
        Instruction::WithdrawInsurance { amount: 50 },
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
    let mut attacker = signer();

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
    let mut zero_current = TestAccount::new(Pubkey::default(), Pubkey::new_unique(), 0).signer();
    let zero_key_cannot_revive = run_ix(
        Instruction::UpdateAuthority {
            kind: processor::AUTHORITY_INSURANCE,
            new_pubkey: attacker.key.to_bytes(),
        },
        &mut [&mut zero_current, &mut attacker, &mut market],
    );
    assert_err_and_market_unchanged(zero_key_cannot_revive, &market, &after_burn);

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
            kind: processor::AUTHORITY_MARK,
            new_pubkey: new_key.key.to_bytes(),
        },
        &mut [&mut admin, &mut new_key, &mut market],
    )
    .unwrap();
    let (cfg, _) = state::read_market(&market.data).unwrap();
    assert_eq!(
        cfg.mark_authority,
        new_key.key.to_bytes(),
        "the market-wide EwmaMark mark authority can be rotated before a specific asset enters EwmaMark mode"
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
fn v16_wrapper_configure_ewma_mark_pushes_and_cranks_from_internal_mark() {
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
        Instruction::ConfigureEwmaMark {
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
        percolator_prog::constants::ORACLE_MODE_EWMA_MARK
    );
    assert_eq!(cfg.mark_ewma_e6, 100);
    assert_eq!(cfg.mark_ewma_last_slot, 5);
    assert_eq!(cfg.last_good_oracle_slot, 5);
    assert_eq!(group.assets[0].effective_price, 100);

    let mut new_mark_authority = signer();
    run_ix(
        Instruction::UpdateAuthority {
            kind: processor::AUTHORITY_MARK,
            new_pubkey: new_mark_authority.key.to_bytes(),
        },
        &mut [&mut admin, &mut new_mark_authority, &mut market],
    )
    .unwrap();

    let before_bad_push = market.data.clone();
    let mut wrong_authority = signer();
    let bad_push = run_ix(
        Instruction::PushEwmaMark {
            asset_index: 0,
            now_slot: 10,
            mark_e6: 120,
        },
        &mut [&mut wrong_authority, &mut market],
    );
    assert_err_and_market_unchanged(bad_push, &market, &before_bad_push);

    run_ix(
        Instruction::PushEwmaMark {
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
        "full-weight EwmaMark push advances the configured EWMA mark at the authenticated slot"
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
        "EwmaMark crank ignores caller-selected price and walks to the internal mark"
    );
}

#[test]
fn v16_wrapper_configure_auth_mark_pushes_direct_mark_without_ewma_setup() {
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

    configure_base_auth_mark(&mut admin, &mut market, 5, 100);
    let (cfg, group) = state::read_market(&market.data).unwrap();
    assert_eq!(cfg.oracle_mode, ORACLE_MODE_AUTH_MARK);
    assert_eq!(cfg.mark_ewma_e6, 100);
    assert_eq!(cfg.oracle_target_price_e6, 100);
    assert_eq!(cfg.mark_ewma_halflife_slots, 0);
    assert_eq!(cfg.mark_min_fee, 0);
    assert_eq!(cfg.mark_ewma_last_slot, 5);
    assert_eq!(cfg.last_good_oracle_slot, 5);
    assert_eq!(group.assets[0].effective_price, 100);

    let mut new_mark_authority = signer();
    run_ix(
        Instruction::UpdateAuthority {
            kind: processor::AUTHORITY_MARK,
            new_pubkey: new_mark_authority.key.to_bytes(),
        },
        &mut [&mut admin, &mut new_mark_authority, &mut market],
    )
    .unwrap();

    let before_bad_push = market.data.clone();
    let mut wrong_authority = signer();
    let bad_push = run_ix(
        Instruction::PushAuthMark {
            asset_index: 0,
            now_slot: 10,
            mark_e6: 120,
        },
        &mut [&mut wrong_authority, &mut market],
    );
    assert_err_and_market_unchanged(bad_push, &market, &before_bad_push);

    push_base_auth_mark(&mut new_mark_authority, &mut market, 10, 120);
    let (cfg, _) = state::read_market(&market.data).unwrap();
    assert_eq!(
        cfg.mark_ewma_e6, 120,
        "AuthMark push stores the authority mark directly, without EWMA smoothing"
    );
    assert_eq!(cfg.oracle_target_price_e6, 120);
    assert_eq!(cfg.mark_ewma_last_slot, 10);
    assert_eq!(cfg.last_good_oracle_slot, 10);

    let mut caller = signer();
    let mut portfolio = portfolio_account();
    init_portfolio(&mut caller, &mut market, &mut portfolio);
    run_ix(
        Instruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 10,
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
        group.assets[0].effective_price, 120,
        "AuthMark crank consumes only the stored authority mark"
    );
}

#[test]
fn v16_wrapper_ewma_mark_profiles_reject_prices_above_engine_max() {
    let mut profile = state::AssetOracleProfileV16 {
        oracle_mode: ORACLE_MODE_EWMA_MARK,
        oracle_leg_count: 0,
        oracle_leg_flags: 0,
        invert: 0,
        unit_scale: 0,
        conf_filter_bps: 0,
        backing_trade_fee_bps_long: 0,
        backing_trade_fee_bps_short: 0,
        backing_trade_fee_insurance_share_bps_long: 0,
        backing_trade_fee_insurance_share_bps_short: 0,
        _padding0: [0u8; 6],
        insurance_authority: [1u8; 32],
        insurance_operator: [1u8; 32],
        backing_bucket_authority: [1u8; 32],
        oracle_authority: [1u8; 32],
        max_staleness_secs: 0,
        hybrid_soft_stale_slots: 0,
        mark_ewma_e6: percolator::MAX_ORACLE_PRICE + 1,
        mark_ewma_last_slot: 1,
        mark_ewma_halflife_slots: 1,
        mark_min_fee: 0,
        oracle_target_price_e6: percolator::MAX_ORACLE_PRICE,
        oracle_target_publish_time: 0,
        last_good_oracle_slot: 1,
        oracle_leg_feeds: [[0u8; 32]; ORACLE_LEG_CAP],
        oracle_leg_prices_e6: [0u64; ORACLE_LEG_CAP],
        oracle_leg_publish_times: [0i64; ORACLE_LEG_CAP],
    };
    assert!(
        state::validate_asset_oracle_profile(&profile).is_err(),
        "EwmaMark EWMA mark must stay within the engine oracle-price envelope"
    );

    profile.mark_ewma_e6 = percolator::MAX_ORACLE_PRICE;
    profile.oracle_target_price_e6 = percolator::MAX_ORACLE_PRICE + 1;
    assert!(
        state::validate_asset_oracle_profile(&profile).is_err(),
        "EwmaMark target mark must stay within the engine oracle-price envelope"
    );

    profile.oracle_mode = ORACLE_MODE_AUTH_MARK;
    profile.mark_ewma_e6 = percolator::MAX_ORACLE_PRICE;
    profile.oracle_target_price_e6 = percolator::MAX_ORACLE_PRICE;
    profile.mark_ewma_halflife_slots = 1;
    assert!(
        state::validate_asset_oracle_profile(&profile).is_err(),
        "AuthMark must not carry EWMA halflife configuration"
    );

    profile.mark_ewma_halflife_slots = 0;
    profile.oracle_target_price_e6 = percolator::MAX_ORACLE_PRICE - 1;
    assert!(
        state::validate_asset_oracle_profile(&profile).is_err(),
        "AuthMark target and stored mark must match"
    );

    profile.oracle_target_price_e6 = percolator::MAX_ORACLE_PRICE;
    assert!(
        state::validate_asset_oracle_profile(&profile).is_ok(),
        "AuthMark accepts a direct authority mark without EWMA or oracle-leg config"
    );

    profile.oracle_mode = ORACLE_MODE_HYBRID_AFTER_HOURS;
    profile.oracle_leg_count = 1;
    profile.oracle_leg_feeds[0] = [7u8; 32];
    profile.max_staleness_secs = 60;
    profile.hybrid_soft_stale_slots = 5;
    profile.oracle_target_price_e6 = percolator::MAX_ORACLE_PRICE;
    profile.mark_ewma_e6 = percolator::MAX_ORACLE_PRICE + 1;
    assert!(
        state::validate_asset_oracle_profile(&profile).is_err(),
        "hybrid fallback EWMA mark must stay within the engine oracle-price envelope"
    );
}

#[test]
fn v16_wrapper_push_ewma_mark_rejects_over_max_input_and_preserves_state() {
    let mut admin = signer();
    let mut market = market_account();
    let mut mint = mint_account();
    run_ix(
        default_init_market_ix(),
        &mut [&mut admin, &mut market, &mut mint],
    )
    .unwrap();

    run_ix(
        Instruction::ConfigureEwmaMark {
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: percolator::MAX_ORACLE_PRICE,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();

    let before = market.data.clone();
    let rejected = run_ix(
        Instruction::PushEwmaMark {
            asset_index: 0,
            now_slot: 2,
            mark_e6: percolator::MAX_ORACLE_PRICE + 1,
        },
        &mut [&mut admin, &mut market],
    );
    assert_err_and_market_unchanged(rejected, &market, &before);

    let (cfg, _) = state::read_market(&market.data).unwrap();
    assert_eq!(cfg.mark_ewma_e6, percolator::MAX_ORACLE_PRICE);
}

#[test]
fn v16_wrapper_configure_ewma_mark_clears_prior_hybrid_oracle_metadata() {
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
        Instruction::ConfigureEwmaMark {
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
    assert_eq!(cfg.oracle_mode, ORACLE_MODE_EWMA_MARK);
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
fn v16_wrapper_ewma_mark_trade_updates_mark_and_charges_dynamic_fee_without_oracle_tail() {
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
        Instruction::ConfigureEwmaMark {
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
        "direct EwmaMark trades pay dynamic mark-movement surcharge without oracle accounts"
    );
    assert!(
        after_cfg.mark_ewma_e6 > before_cfg.mark_ewma_e6,
        "direct EwmaMark trades move the fallback EWMA mark"
    );
    assert_eq!(
        after_group.assets[0].effective_price, before_group.assets[0].effective_price,
        "execution price flexibility must not rewrite the accepted EwmaMark index"
    );
}

#[test]
fn v16_wrapper_auth_mark_trade_cannot_update_authority_mark() {
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
    configure_base_auth_mark(&mut admin, &mut market, 1, 100_000);

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
    assert_eq!(
        after_group.insurance,
        before_group.insurance + base_only_fee,
        "AuthMark trades should not pay EWMA mark-movement surcharge"
    );
    assert_eq!(
        after_cfg.mark_ewma_e6, before_cfg.mark_ewma_e6,
        "AuthMark trades must not rewrite the authority mark"
    );
    assert_eq!(
        after_cfg.oracle_target_price_e6, before_cfg.oracle_target_price_e6,
        "AuthMark target only changes through PushAuthMark"
    );
    assert_eq!(
        after_group.assets[0].effective_price, before_group.assets[0].effective_price,
        "execution price flexibility must not rewrite the accepted AuthMark index"
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
fn v16_wrapper_base_unit_secondary_withdraws_but_primary_only_deposits() {
    let mut admin = signer();
    let mut market = market_account();
    let mut primary_mint = mint_account();
    let primary_key = primary_mint.key;
    let mut secondary_mint = mint_account();
    let secondary_key = secondary_mint.key;
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    run_ix(
        default_init_market_ix(),
        &mut [&mut admin, &mut market, &mut primary_mint],
    )
    .unwrap();
    configure_base_unit_mints(
        &mut admin,
        &mut market,
        &mut primary_mint,
        &mut secondary_mint,
    )
    .unwrap();
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 1_000);

    let before_market = market.data.clone();
    let before_portfolio = portfolio.data.clone();
    let mut secondary_source = user_token_account(owner.key, secondary_key, 1);
    let mut secondary_vault_for_deposit = vault_token_account(&market, secondary_key, 0);
    let mut token_program = token_program_account();
    let secondary_deposit = run_ix(
        Instruction::Deposit { amount: 1 },
        &mut [
            &mut owner,
            &mut market,
            &mut portfolio,
            &mut secondary_source,
            &mut secondary_vault_for_deposit,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(secondary_deposit, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);

    let mut secondary_dest = user_token_account(owner.key, secondary_key, 0);
    let mut primary_vault = vault_token_account(&market, primary_key, 250);
    let mut vault_auth = vault_authority_account(&market);
    let mismatched_vault = run_ix(
        Instruction::Withdraw { amount: 250 },
        &mut [
            &mut owner,
            &mut market,
            &mut portfolio,
            &mut secondary_dest,
            &mut primary_vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(mismatched_vault, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);

    let mut secondary_dest = user_token_account(owner.key, secondary_key, 0);
    let mut secondary_vault = vault_token_account(&market, secondary_key, 250);
    run_ix(
        Instruction::Withdraw { amount: 250 },
        &mut [
            &mut owner,
            &mut market,
            &mut portfolio,
            &mut secondary_dest,
            &mut secondary_vault,
            &mut vault_auth,
            &mut token_program,
        ],
    )
    .unwrap();

    let (_, group) = state::read_market(&market.data).unwrap();
    let account = state::read_portfolio(&portfolio.data).unwrap();
    assert_eq!(group.vault, 750);
    assert_eq!(account.capital, 750);
}

#[test]
fn v16_wrapper_base_unit_authority_changes_primary_and_rotates() {
    let mut admin = signer();
    let mut market = market_account();
    let mut old_primary_mint = mint_account();
    let old_primary_key = old_primary_mint.key;
    let mut new_primary_mint = mint_account();
    let new_primary_key = new_primary_mint.key;
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    run_ix(
        default_init_market_ix(),
        &mut [&mut admin, &mut market, &mut old_primary_mint],
    )
    .unwrap();
    configure_base_unit_mints(
        &mut admin,
        &mut market,
        &mut old_primary_mint,
        &mut new_primary_mint,
    )
    .unwrap();

    let mut base_unit_authority = signer();
    run_ix(
        Instruction::UpdateAuthority {
            kind: processor::AUTHORITY_BASE_UNIT,
            new_pubkey: base_unit_authority.key.to_bytes(),
        },
        &mut [&mut admin, &mut base_unit_authority, &mut market],
    )
    .unwrap();

    let before_rotation = market.data.clone();
    let stale_admin = configure_base_unit_mints(
        &mut admin,
        &mut market,
        &mut new_primary_mint,
        &mut old_primary_mint,
    );
    assert_err_and_market_unchanged(stale_admin, &market, &before_rotation);

    configure_base_unit_mints(
        &mut base_unit_authority,
        &mut market,
        &mut new_primary_mint,
        &mut old_primary_mint,
    )
    .unwrap();
    let (cfg, _) = state::read_market(&market.data).unwrap();
    assert_eq!(cfg.collateral_mint, new_primary_key.to_bytes());
    assert_eq!(cfg.secondary_collateral_mint, old_primary_key.to_bytes());
    assert_eq!(cfg.base_unit_authority, base_unit_authority.key.to_bytes());

    init_portfolio(&mut owner, &mut market, &mut portfolio);
    let before_deposit = market.data.clone();
    let before_portfolio = portfolio.data.clone();
    let mut old_primary_source = user_token_account(owner.key, old_primary_key, 1);
    let mut old_primary_vault = vault_token_account(&market, old_primary_key, 0);
    let mut token_program = token_program_account();
    let old_primary_deposit = run_ix(
        Instruction::Deposit { amount: 1 },
        &mut [
            &mut owner,
            &mut market,
            &mut portfolio,
            &mut old_primary_source,
            &mut old_primary_vault,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(old_primary_deposit, &market, &before_deposit);
    assert_eq!(portfolio.data, before_portfolio);

    let mut new_primary_source = user_token_account(owner.key, new_primary_key, 1);
    let mut new_primary_vault = vault_token_account(&market, new_primary_key, 0);
    run_ix(
        Instruction::Deposit { amount: 1 },
        &mut [
            &mut owner,
            &mut market,
            &mut portfolio,
            &mut new_primary_source,
            &mut new_primary_vault,
            &mut token_program,
        ],
    )
    .unwrap();
    let (_, group) = state::read_market(&market.data).unwrap();
    let account = state::read_portfolio(&portfolio.data).unwrap();
    assert_eq!(group.vault, 1);
    assert_eq!(account.capital, 1);
}

#[test]
fn v16_wrapper_base_unit_mint_rotation_rejects_nonempty_market_vault() {
    let mut admin = signer();
    let mut market = market_account();
    let mut old_primary_mint = mint_account();
    let mut new_primary_mint = mint_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    run_ix(
        default_init_market_ix(),
        &mut [&mut admin, &mut market, &mut old_primary_mint],
    )
    .unwrap();
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 1);

    let before = market.data.clone();
    let rotate = configure_base_unit_mints(
        &mut admin,
        &mut market,
        &mut old_primary_mint,
        &mut new_primary_mint,
    );
    assert_err_and_market_unchanged(rotate, &market, &before);

    let (cfg, group) = state::read_market(&market.data).unwrap();
    assert_eq!(cfg.collateral_mint, old_primary_mint.key.to_bytes());
    assert_eq!(group.vault, 1);
    assert_eq!(group.c_tot, 1);
}

#[test]
fn v16_wrapper_base_unit_atomic_swap_requires_primary_in_for_secondary_out() {
    let mut admin = signer();
    let mut market = market_account();
    let mut primary_mint = mint_account();
    let primary_key = primary_mint.key;
    let mut secondary_mint = mint_account();
    let secondary_key = secondary_mint.key;

    run_ix(
        default_init_market_ix(),
        &mut [&mut admin, &mut market, &mut primary_mint],
    )
    .unwrap();
    configure_base_unit_mints(
        &mut admin,
        &mut market,
        &mut primary_mint,
        &mut secondary_mint,
    )
    .unwrap();

    let mut attacker = signer();
    let mut primary_source = user_token_account(attacker.key, primary_key, 50);
    let mut primary_vault = vault_token_account(&market, primary_key, 0);
    let mut secondary_dest = user_token_account(attacker.key, secondary_key, 0);
    let mut secondary_vault = vault_token_account(&market, secondary_key, 50);
    let mut vault_auth = vault_authority_account(&market);
    let mut token_program = token_program_account();
    let before_market = market.data.clone();
    let zero_swap = run_ix(
        Instruction::SwapSecondaryForPrimary { amount: 0 },
        &mut [
            &mut admin,
            &mut market,
            &mut primary_source,
            &mut primary_vault,
            &mut secondary_dest,
            &mut secondary_vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(zero_swap, &market, &before_market);

    let unauthorized = run_ix(
        Instruction::SwapSecondaryForPrimary { amount: 50 },
        &mut [
            &mut attacker,
            &mut market,
            &mut primary_source,
            &mut primary_vault,
            &mut secondary_dest,
            &mut secondary_vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(unauthorized, &market, &before_market);

    let mut short_primary = user_token_account(admin.key, primary_key, 49);
    let mut primary_vault = vault_token_account(&market, primary_key, 0);
    let mut secondary_dest = user_token_account(admin.key, secondary_key, 0);
    let mut secondary_vault = vault_token_account(&market, secondary_key, 50);
    let short_primary_in = run_ix(
        Instruction::SwapSecondaryForPrimary { amount: 50 },
        &mut [
            &mut admin,
            &mut market,
            &mut short_primary,
            &mut primary_vault,
            &mut secondary_dest,
            &mut secondary_vault,
            &mut vault_auth,
            &mut token_program,
        ],
    );
    assert_err_and_market_unchanged(short_primary_in, &market, &before_market);

    let mut primary_source = user_token_account(admin.key, primary_key, 50);
    let mut primary_vault = vault_token_account(&market, primary_key, 0);
    let mut secondary_dest = user_token_account(admin.key, secondary_key, 0);
    let mut secondary_vault = vault_token_account(&market, secondary_key, 50);
    run_ix(
        Instruction::SwapSecondaryForPrimary { amount: 50 },
        &mut [
            &mut admin,
            &mut market,
            &mut primary_source,
            &mut primary_vault,
            &mut secondary_dest,
            &mut secondary_vault,
            &mut vault_auth,
            &mut token_program,
        ],
    )
    .unwrap();
    assert_eq!(
        market.data, before_market,
        "base-unit swaps only exchange SPL custody and must not change engine accounting"
    );
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
    let mut market = market_account_with_capacity(
        percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS as usize + 1,
    );
    let mut long_owner = signer();
    let mut short_owner = signer();
    let mut cranker = signer();
    let mut long_account = portfolio_account_for_market_slots(
        percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS as usize + 1,
    );
    let mut short_account = portfolio_account_for_market_slots(
        percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS as usize + 1,
    );
    let mut crank_account = portfolio_account_for_market_slots(
        percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS as usize + 1,
    );

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
        Instruction::ConfigureEwmaMark {
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
        Instruction::ConfigureEwmaMark {
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
    configure_base_ewma_mark(&mut admin, &mut market, 0, 100);

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
    push_base_ewma_mark(&mut admin, &mut market, 1, 102);
    run_ix(
        Instruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 1,
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
    let capital_before_convert = long.capital;

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
    assert_eq!(long.capital, capital_before_convert + 2);
    assert_eq!(group.pnl_pos_tot, 0);

    withdraw(&mut long_owner, &mut market, &mut long_account, 2);
    let (_, group) = state::read_market(&market.data).unwrap();
    let long = state::read_portfolio(&long_account.data).unwrap();
    assert_eq!(long.capital, capital_before_convert);
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
fn v16_wrapper_tradecpi_ewma_mark_trade_moves_mark_without_refreshing_liveness() {
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
        Instruction::ConfigureEwmaMark {
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
        "direct EwmaMark CPI trades must use the same dynamic mark path as TradeNoCpi"
    );
    assert_eq!(
        after_cfg.last_good_oracle_slot, before_cfg.last_good_oracle_slot,
        "trade-flow mark movement must not refresh the permissionless stale-resolution liveness stamp"
    );
    assert_eq!(
        after_group.assets[0].effective_price, before_group.assets[0].effective_price,
        "direct EwmaMark CPI trades must not rewrite the accepted effective index"
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
fn v16_wrapper_tradecpi_rejects_corrupt_backing_fee_policy_before_later_checks() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner_a = signer();
    let mut owner_b = signer();
    let mut attacker = signer();
    let mut account_a = portfolio_account();
    let mut account_b = portfolio_account();
    let mut matcher_program = matcher_program_account();
    let mut matcher_context = matcher_context_account(&matcher_program);
    let mut delegate =
        matcher_delegate_account(&market, &account_b, &matcher_program, &matcher_context);

    init_market_with_ix(
        &mut admin,
        &mut market,
        init_market_ix_with(|ix| {
            if let Instruction::InitMarket {
                max_trading_fee_bps,
                ..
            } = ix
            {
                *max_trading_fee_bps = 100;
            }
        }),
    );
    init_portfolio(&mut owner_a, &mut market, &mut account_a);
    init_portfolio(&mut owner_b, &mut market, &mut account_b);

    let (mut cfg, _group) = state::read_market(&market.data).unwrap();
    cfg.backing_trade_fee_bps_short = 10_001;
    market.data[HEADER_LEN..HEADER_LEN + WRAPPER_CONFIG_LEN]
        .copy_from_slice(bytemuck::bytes_of(&cfg));
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
            &mut attacker,
            &mut market,
            &mut account_a,
            &mut account_b,
            &mut matcher_program,
            &mut matcher_context,
            &mut delegate,
        ],
    );
    assert_eq!(
        rejected,
        Err(ProgramError::InvalidAccountData),
        "corrupt domain backing fee policy must be rejected before matcher CPI"
    );
    assert_eq!(market.data, before_market);
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
    configure_base_ewma_mark(&mut admin, &mut market, 0, 100);
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

    push_base_ewma_mark(&mut admin, &mut market, 1, 102);
    run_ix(
        Instruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 1,
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
    deposit(&mut short_owner, &mut market, &mut short_account, 250);
    configure_base_ewma_mark(&mut admin, &mut market, 0, 100);
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

    push_base_ewma_mark(&mut admin, &mut market, 1, 300);
    run_ix(
        Instruction::PermissionlessCrank {
            action: 1,
            asset_index: 0,
            now_slot: 1,
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
fn v16_wrapper_permissionless_crank_rejects_invalid_asset_and_legacy_price_payload_without_mutation(
) {
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
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut owner, &mut market, &mut portfolio],
    );
    assert_err_and_market_unchanged(invalid_asset, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);

    let mut legacy_price_payload = Instruction::PermissionlessCrank {
        action: 0,
        asset_index: 0,
        now_slot: 0,
        funding_rate_e9: 0,
        close_q: 0,
        fee_bps: 0,
        recovery_reason: 0,
    }
    .encode();
    legacy_price_payload.splice(12..12, 1_000_000u64.to_le_bytes().iter().copied());
    let legacy_price = run_ix_data(
        &legacy_price_payload,
        &mut [&mut owner, &mut market, &mut portfolio],
    );
    assert_err_and_market_unchanged(legacy_price, &market, &before_market);
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
fn v16_wrapper_permissionless_recovery_rejects_below_progress_floor_kill_switch() {
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
        Instruction::ConfigureEwmaMark {
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
        Instruction::PushEwmaMark {
            asset_index: 0,
            now_slot: 1,
            mark_e6: 3,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();

    let before_market = market.data.clone();
    let before_account = long_account.data.clone();
    let result = run_ix(
        Instruction::PermissionlessCrank {
            action: 3,
            asset_index: 0,
            now_slot: 1,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &mut [&mut long_owner, &mut market, &mut long_account],
    );
    assert_err_and_market_unchanged(result, &market, &before_market);
    assert_eq!(long_account.data, before_account);
    let (_, group) = state::read_market(&market.data).unwrap();
    assert_eq!(group.mode, MarketModeV16::Live);
    assert_eq!(
        group.recovery_reason,
        None,
        "even a proven below-progress-floor state must not let a public caller select terminal Recovery"
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
    seed_cancellable_close_progress(&mut market, &mut portfolio);
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
fn v16_wrapper_cure_and_cancel_close_rejects_resolved_market() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();
    let mint = init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    seed_cancellable_close_progress(&mut market, &mut portfolio);
    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        group.mode = MarketModeV16::Resolved;
        group.resolved_slot = group.current_slot;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }

    let mut source = user_token_account(owner.key, mint, 20);
    let mut vault = vault_token_account(&market, mint, 0);
    let mut token_program = token_program_account();
    let before_market = market.data.clone();
    let before_portfolio = portfolio.data.clone();
    let rejected = run_ix(
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
    );
    assert_err_and_market_unchanged(rejected, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);
}

#[test]
fn v16_wrapper_cure_and_cancel_close_rejects_after_permissionless_resolve_maturity() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();
    let mint = init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    seed_cancellable_close_progress(&mut market, &mut portfolio);
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

    let mut source = user_token_account(owner.key, mint, 20);
    let mut vault = vault_token_account(&market, mint, 0);
    let mut token_program = token_program_account();
    let before_market = market.data.clone();
    let before_portfolio = portfolio.data.clone();
    let rejected = run_ix(
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
    );
    assert_err_and_market_unchanged(rejected, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);
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
fn v16_wrapper_close_resolved_pays_positive_pnl_through_engine_ledger() {
    let mut admin = signer();
    let mut market = market_account();
    let mut owner = signer();
    let mut portfolio = portfolio_account();

    let mint = init_market(&mut admin, &mut market);
    init_portfolio(&mut owner, &mut market, &mut portfolio);
    deposit(&mut owner, &mut market, &mut portfolio, 1_000);
    top_up_backing_bucket(&mut admin, &mut market, 1, 250, 10);
    add_source_positive_pnl(&mut market, &mut portfolio, 1, 250);
    run_ix(Instruction::ResolveMarket, &mut [&mut admin, &mut market]).unwrap();

    let mut dest = user_token_account(owner.key, mint, 0);
    let mut vault = vault_token_account(&market, mint, 1_250);
    let mut vault_auth = vault_authority_account(&market);
    let mut token_program = token_program_account();
    run_ix(
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
    )
    .unwrap();

    let (_, group) = state::read_market(&market.data).unwrap();
    let account = state::read_portfolio(&portfolio.data).unwrap();
    assert_eq!(group.vault, 0);
    assert_eq!(group.c_tot, 0);
    assert_eq!(account.capital, 0);
    assert_eq!(account.pnl, 0);
    assert!(account.resolved_payout_receipt.present);
    assert!(account.resolved_payout_receipt.finalized);
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
fn v16_wrapper_close_resolved_enforces_configured_force_close_delay() {
    let mut admin = signer();
    let mut market = market_account();
    let owner_key = Pubkey::new_unique();
    let mut owner_for_init = TestAccount::new(owner_key, Pubkey::new_unique(), 0).signer();
    let mut owner_unsigned = TestAccount::new(owner_key, Pubkey::new_unique(), 0);
    let mut owner_signed = TestAccount::new(owner_key, Pubkey::new_unique(), 0).signer();
    let mut portfolio = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner_for_init, &mut market, &mut portfolio);
    run_ix(
        Instruction::ConfigurePermissionlessResolve {
            stale_slots: 10,
            force_close_delay_slots: 5,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    run_ix(Instruction::ResolveMarket, &mut [&mut admin, &mut market]).unwrap();

    let before_market = market.data.clone();
    let before_portfolio = portfolio.data.clone();
    let unsigned_before_delay = run_ix(
        Instruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        &mut [&mut owner_unsigned, &mut market, &mut portfolio],
    );
    assert_err_and_market_unchanged(unsigned_before_delay, &market, &before_market);
    assert_eq!(portfolio.data, before_portfolio);

    run_ix(
        Instruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        &mut [&mut owner_signed, &mut market, &mut portfolio],
    )
    .unwrap();
}

#[test]
fn v16_wrapper_close_resolved_becomes_permissionless_after_force_close_delay() {
    let mut admin = signer();
    let mut market = market_account();
    let owner_key = Pubkey::new_unique();
    let mut owner_for_init = TestAccount::new(owner_key, Pubkey::new_unique(), 0).signer();
    let mut owner_unsigned = TestAccount::new(owner_key, Pubkey::new_unique(), 0);
    let mut portfolio = portfolio_account();

    init_market(&mut admin, &mut market);
    init_portfolio(&mut owner_for_init, &mut market, &mut portfolio);
    run_ix(
        Instruction::ConfigurePermissionlessResolve {
            stale_slots: 10,
            force_close_delay_slots: 5,
        },
        &mut [&mut admin, &mut market],
    )
    .unwrap();
    run_ix(Instruction::ResolveMarket, &mut [&mut admin, &mut market]).unwrap();
    {
        let (cfg, mut group) = state::read_market(&market.data).unwrap();
        group.current_slot = group.resolved_slot + 5;
        state::write_market(&mut market.data, &cfg, &group).unwrap();
    }

    run_ix(
        Instruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        &mut [&mut owner_unsigned, &mut market, &mut portfolio],
    )
    .unwrap();
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
fn v16_wrapper_close_resolved_active_position_is_progress_only_without_payout() {
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
    let result = run_ix(
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
    );
    assert_eq!(result, Ok(()));
    assert_eq!(read_token_amount(&dest), 0);

    let (_, group) = state::read_market(&market.data).unwrap();
    let long = state::read_portfolio(&long_account.data).unwrap();
    assert_eq!(
        group.vault, 2_000_000,
        "resolved close must not pay before account exposure is cleared"
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

    let result = run_ix(
        Instruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        &mut [&mut long_owner, &mut market, &mut long_account],
    );
    assert_eq!(result, Ok(()));

    let (_, group) = state::read_market(&market.data).unwrap();
    let long = state::read_portfolio(&long_account.data).unwrap();
    assert_eq!(
        group.vault, 2_000_000,
        "active resolved close progress must not require or perform token payout"
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

// ============================================================================
// Stress test for the per-domain insurance over-withdrawal surface.
//
// Bug class under test (reported as a prior engine bug): a domain being able
// to withdraw MORE than was accounted to it - i.e., dipping into other
// domains' insurance / the global pool beyond its own allocation. The engine
// guards this in `validate_shape` via
//   sum_d (insurance_domain_budget[d] - insurance_domain_spent[d]) <= insurance
// and the wrapper calls validate_shape after every mutation. This test hammers
// the wrapper's per-domain top-up / withdraw surface with a long randomized
// sequence and independently re-derives the conservation invariants after
// every operation, plus asserts that deliberate over-withdrawals are rejected.
// ============================================================================
fn read_token_amount(acct: &TestAccount) -> u128 {
    u64::from_le_bytes(acct.data[64..72].try_into().unwrap()) as u128
}

#[test]
fn v16_wrapper_stress_per_domain_insurance_never_overdraws_cross_domain() {
    const N_DOMAINS: usize = 6; // asset 0 -> domains 0,1 ; asset 1 -> 2,3 ; asset 2 -> 4,5
    let mut admin = signer().writable();
    let mut market = market_account();
    let mint = init_market(&mut admin, &mut market);

    // Append assets 1 and 2 as the asset authority (admin) so each per-asset
    // domain carries admin as its insurance authority/operator.
    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        1,
        100,
        125,
    )
    .unwrap();
    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        2,
        200,
        125,
    )
    .unwrap();

    let mut token_program = token_program_account();
    // Persistent token accounts. The unit-test harness does not execute the
    // SPL token CPI (invoke is unavailable), so token balances do not move -
    // both accounts are pre-funded large enough that `require_token_balance`
    // always passes. The test focuses on the wrapper's internal per-domain
    // accounting, which is what enforces the no-cross-domain-overdraw property.
    let mut admin_tok = user_token_account(admin.key, mint, 1_000_000_000);
    let mut vault_tok = vault_token_account(&market, mint, 1_000_000_000);
    let mut vault_auth = vault_authority_account(&market);

    // Seed real user capital so all three fund classes (user capital +
    // insurance + backing) coexist in the vault accounting while the
    // per-domain insurance surface is stressed.
    let mut lp_owner = signer();
    let mut lp_account = portfolio_account();
    init_portfolio(&mut lp_owner, &mut market, &mut lp_account);
    deposit(&mut lp_owner, &mut market, &mut lp_account, 50_000);

    // Model of what has been credited / withdrawn per domain via the
    // per-domain insurance API (the only paths exercised here).
    let mut credited = [0u128; N_DOMAINS];
    let mut withdrawn = [0u128; N_DOMAINS];

    // Deterministic xorshift RNG (reproducible).
    let mut rng: u64 = 0x9E37_79B9_7F4A_7C15;
    let next = |rng: &mut u64| {
        *rng ^= *rng << 13;
        *rng ^= *rng >> 7;
        *rng ^= *rng << 17;
        *rng
    };

    let check_invariants = |market: &TestAccount,
                            vault_tok: &TestAccount,
                            credited: &[u128; N_DOMAINS],
                            withdrawn: &[u128; N_DOMAINS]| {
        let (_, group) = state::read_market(&market.data).unwrap();
        // (1) per-domain: no underflow, and accounted-remaining matches model.
        let mut sum_remaining = 0u128;
        for d in 0..N_DOMAINS {
            let budget = group.insurance_domain_budget[d];
            let spent = group.insurance_domain_spent[d];
            assert!(
                budget >= spent,
                "domain {d} spent {spent} > budget {budget}"
            );
            let remaining = budget - spent;
            // The on-chain remaining for a domain must never exceed what the
            // model says was credited minus withdrawn from THAT domain.
            assert!(
                remaining <= credited[d] - withdrawn[d],
                "domain {d} remaining {remaining} exceeds model credited-withdrawn {}",
                credited[d] - withdrawn[d]
            );
            sum_remaining += remaining;
        }
        // (2) the core engine invariant: domain budgets never over-allocate
        // the global insurance pool.
        assert!(
            sum_remaining <= group.insurance,
            "sum domain remaining {sum_remaining} > insurance {}",
            group.insurance
        );
        // (3) the vault must simultaneously cover insurance AND user capital
        // (c_tot): a domain over-withdraw would otherwise eat into user funds.
        assert!(
            group.vault >= group.insurance + group.c_tot,
            "vault {} < insurance {} + user capital {}",
            group.vault,
            group.insurance,
            group.c_tot
        );
        // (4) the wrapper's vault accounting is covered by the physical SPL
        // vault balance (a fixed pre-funded amount in this harness).
        assert!(
            group.vault <= read_token_amount(vault_tok),
            "wrapper vault accounting {} exceeds physical vault tokens {}",
            group.vault,
            read_token_amount(vault_tok)
        );
    };

    for _ in 0..600 {
        let r = next(&mut rng);
        let op = r % 3;
        let domain = (next(&mut rng) % N_DOMAINS as u64) as u8;
        let amount = ((next(&mut rng) % 250) + 1) as u128;

        match op {
            0 => {
                // Per-domain insurance top-up.
                let res = run_ix(
                    Instruction::TopUpInsuranceDomain { domain, amount },
                    &mut [
                        &mut admin,
                        &mut market,
                        &mut admin_tok,
                        &mut vault_tok,
                        &mut token_program,
                    ],
                );
                if res.is_ok() {
                    credited[domain as usize] += amount;
                }
            }
            1 => {
                // Per-domain insurance withdraw within the accounted budget.
                let res = run_ix(
                    Instruction::WithdrawInsuranceDomain { domain, amount },
                    &mut [
                        &mut admin,
                        &mut market,
                        &mut admin_tok,
                        &mut vault_tok,
                        &mut vault_auth,
                        &mut token_program,
                    ],
                );
                if res.is_ok() {
                    withdrawn[domain as usize] += amount;
                    // The headline bug-class assertion: a domain can never
                    // have withdrawn more than was credited to it.
                    assert!(
                        withdrawn[domain as usize] <= credited[domain as usize],
                        "domain {domain} over-withdrew: withdrawn {} > credited {}",
                        withdrawn[domain as usize],
                        credited[domain as usize]
                    );
                }
            }
            _ => {
                // Deliberate cross-domain over-withdraw: try to take strictly
                // more than this domain's accounted remaining. Must be rejected
                // and must not mutate state, even when OTHER domains hold enough
                // insurance to cover the amount globally.
                let remaining = credited[domain as usize] - withdrawn[domain as usize];
                let over = remaining + 1 + (next(&mut rng) % 50) as u128;
                let before = market.data.clone();
                let before_vault = vault_tok.data.clone();
                let res = run_ix(
                    Instruction::WithdrawInsuranceDomain {
                        domain,
                        amount: over,
                    },
                    &mut [
                        &mut admin,
                        &mut market,
                        &mut admin_tok,
                        &mut vault_tok,
                        &mut vault_auth,
                        &mut token_program,
                    ],
                );
                assert!(
                    res.is_err(),
                    "domain {domain} allowed over-withdraw of {over} (remaining {remaining})"
                );
                assert_eq!(market.data, before, "rejected over-withdraw mutated market");
                assert_eq!(
                    vault_tok.data, before_vault,
                    "rejected over-withdraw moved vault tokens"
                );
            }
        }

        check_invariants(&market, &vault_tok, &credited, &withdrawn);
    }

    // Final settlement check: total withdrawn across all domains never exceeds
    // total credited across all domains.
    let total_credited: u128 = credited.iter().sum();
    let total_withdrawn: u128 = withdrawn.iter().sum();
    assert!(
        total_withdrawn <= total_credited,
        "aggregate over-withdraw: {total_withdrawn} > {total_credited}"
    );
    // Non-vacuity: the run must actually exercise successful credits and
    // withdraws across multiple domains, and leave at least two domains
    // simultaneously funded at some point (so a cross-domain drain WOULD be
    // possible if the engine guard were missing).
    assert!(total_credited > 0, "stress run never credited any domain");
    assert!(
        total_withdrawn > 0,
        "stress run never withdrew from any domain"
    );
    let funded_domains = credited.iter().filter(|&&c| c > 0).count();
    assert!(
        funded_domains >= 2,
        "stress run only funded {funded_domains} domain(s); cross-domain drain not exercised"
    );
}

// ============================================================================
// Stress test for the per-domain BACKING over-withdrawal surface (the "+
// backing" half of the reported bug class: a domain pulling out more backing
// than was deposited to it). Hammers TopUpBackingBucket / WithdrawBackingBucket
// over domains 2..6 (the appended assets, where admin is the backing
// authority) and asserts that on-chain fresh backing always equals the model,
// that no domain ever withdraws more backing than it deposited, and that
// deliberate over-withdrawals are rejected without mutating state.
// ============================================================================
#[test]
fn v16_wrapper_stress_per_domain_backing_never_overdraws() {
    const N_DOMAINS: usize = 6;
    const FAR_EXPIRY: u64 = 1_000_000;
    let mut admin = signer().writable();
    let mut market = market_account();
    let mint = init_market(&mut admin, &mut market);
    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        1,
        100,
        125,
    )
    .unwrap();
    update_asset_lifecycle(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        2,
        200,
        125,
    )
    .unwrap();

    let mut token_program = token_program_account();
    let mut admin_tok = user_token_account(admin.key, mint, 1_000_000_000);
    let mut vault_tok = vault_token_account(&market, mint, 1_000_000_000);
    let mut vault_auth = vault_authority_account(&market);

    // Per-domain fresh backing the model believes is held, and gross
    // deposited / withdrawn for the no-overdraw assertion.
    let mut fresh = [0u128; N_DOMAINS];
    let mut deposited = [0u128; N_DOMAINS];
    let mut withdrawn = [0u128; N_DOMAINS];

    let mut rng: u64 = 0xD1B5_4A32_D192_ED03;
    let next = |rng: &mut u64| {
        *rng ^= *rng << 13;
        *rng ^= *rng >> 7;
        *rng ^= *rng << 17;
        *rng
    };

    let check = |market: &TestAccount, fresh: &[u128; N_DOMAINS]| {
        let (_, group) = state::read_market(&market.data).unwrap();
        let mut total_fresh_num = 0u128;
        // Only backing-capable domains (2..6) carry model-tracked backing.
        for d in 2..N_DOMAINS {
            let on_chain = group.source_backing_buckets[d].fresh_unliened_backing_num;
            assert_eq!(
                on_chain,
                fresh[d] * BOUND_SCALE,
                "domain {d} on-chain fresh backing diverged from model"
            );
            total_fresh_num += on_chain;
        }
        // Vault must physically cover insurance + all fresh backing.
        let backing_atoms = total_fresh_num / BOUND_SCALE;
        assert!(
            group.vault >= group.insurance + backing_atoms,
            "vault {} < insurance {} + backing {}",
            group.vault,
            group.insurance,
            backing_atoms
        );
    };

    for _ in 0..600 {
        let op = next(&mut rng) % 3;
        // Restrict to backing-capable domains 2..=5.
        let domain = (2 + (next(&mut rng) % 4)) as u8;
        let amount = ((next(&mut rng) % 250) + 1) as u128;
        let d = domain as usize;

        match op {
            0 => {
                let res = run_ix(
                    Instruction::TopUpBackingBucket {
                        domain,
                        amount,
                        expiry_slot: FAR_EXPIRY,
                    },
                    &mut [
                        &mut admin,
                        &mut market,
                        &mut admin_tok,
                        &mut vault_tok,
                        &mut token_program,
                    ],
                );
                if res.is_ok() {
                    fresh[d] += amount;
                    deposited[d] += amount;
                }
            }
            1 => {
                let res = run_ix(
                    Instruction::WithdrawBackingBucket { domain, amount },
                    &mut [
                        &mut admin,
                        &mut market,
                        &mut admin_tok,
                        &mut vault_tok,
                        &mut vault_auth,
                        &mut token_program,
                    ],
                );
                if res.is_ok() {
                    fresh[d] -= amount;
                    withdrawn[d] += amount;
                    assert!(
                        withdrawn[d] <= deposited[d],
                        "domain {domain} over-withdrew backing: {} > {}",
                        withdrawn[d],
                        deposited[d]
                    );
                }
            }
            _ => {
                // Deliberate over-withdraw beyond fresh backing must be rejected.
                let over = fresh[d] + 1 + (next(&mut rng) % 50) as u128;
                let before = market.data.clone();
                let res = run_ix(
                    Instruction::WithdrawBackingBucket {
                        domain,
                        amount: over,
                    },
                    &mut [
                        &mut admin,
                        &mut market,
                        &mut admin_tok,
                        &mut vault_tok,
                        &mut vault_auth,
                        &mut token_program,
                    ],
                );
                assert!(
                    res.is_err(),
                    "domain {domain} allowed backing over-withdraw {over} (fresh {})",
                    fresh[d]
                );
                assert_eq!(
                    market.data, before,
                    "rejected backing over-withdraw mutated market"
                );
            }
        }
        check(&market, &fresh);
    }
}

// ============================================================================
// Cross-domain isolation under oracle attack.
//
// Property: an attacker who fully controls one domain's oracle (asset 1 here)
// must NOT be able to extract value from OTHER domains - neither the honest
// domain's insurance/backing nor the capital of a bystander who only trades
// the honest domain - even though the wrapper is cross-margin and the victim
// holds positions. The engine isolates loss absorption per domain
// (consume_domain_insurance_for_negative_pnl is scoped to the bankrupt asset's
// own domain) and the wrapper keeps sum_d (budget - spent) <= insurance, so a
// loss in the attacker's domain can consume at most that domain's insurance +
// the victim's own capital + social loss among that asset's own participants.
//
// This test drives a randomized adversarial sequence on the attacker domain
// (wild oracle pushes, trades against a victim, liquidation cranks, and
// withdrawal attempts) and asserts after EVERY step that the honest domain's
// per-domain insurance budget, its backing bucket, and the bystander's capital
// are byte-for-byte unchanged from the pre-attack snapshot.
// ============================================================================
#[test]
fn v16_wrapper_oracle_attacker_cannot_drain_other_domains() {
    // asset 0 -> domains 0,1 ; asset 1 (ATTACKER) -> 2,3 ; asset 2 (HONEST) -> 4,5
    let mut admin = signer().writable();
    let mut market = market_account();
    let mint = init_market(&mut admin, &mut market);

    let mut attacker = signer().writable();
    let admin_key = admin.key.to_bytes();
    let attacker_key = attacker.key.to_bytes();
    // Asset 1 = attacker-controlled domain (including oracle authority). Asset
    // 2 = honest domain. Both are activated by admin (the asset authority).
    update_asset_lifecycle_with_authorities(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        1,
        100,
        125,
        attacker_key,
        attacker_key,
        attacker_key,
    )
    .unwrap();
    update_asset_lifecycle_with_authorities(
        &mut admin,
        &mut market,
        processor::ASSET_ACTION_ACTIVATE,
        2,
        200,
        125,
        admin_key,
        admin_key,
        admin_key,
    )
    .unwrap();

    let mut token_program = token_program_account();

    // ---- Fund the HONEST domain (asset 2: domains 4,5) ----
    let mut admin_src = user_token_account(admin.key, mint, 1_000_000_000);
    let mut vault_tok = vault_token_account(&market, mint, 1_000_000_000);
    let mut vault_auth = vault_authority_account(&market);
    let mut attacker_dest = user_token_account(attacker.key, mint, 0);
    for dom in [4u8, 5u8] {
        run_ix(
            Instruction::TopUpInsuranceDomain {
                domain: dom,
                amount: 5_000,
            },
            &mut [
                &mut admin,
                &mut market,
                &mut admin_src,
                &mut vault_tok,
                &mut token_program,
            ],
        )
        .unwrap();
    }
    run_ix(
        Instruction::TopUpBackingBucket {
            domain: 4,
            amount: 3_000,
            expiry_slot: 1_000_000,
        },
        &mut [
            &mut admin,
            &mut market,
            &mut admin_src,
            &mut vault_tok,
            &mut token_program,
        ],
    )
    .unwrap();

    // ---- Bystander who only ever touches the HONEST asset 2 ----
    let mut bystander = signer();
    let mut bystander_acct = portfolio_account();
    init_portfolio(&mut bystander, &mut market, &mut bystander_acct);
    deposit(&mut bystander, &mut market, &mut bystander_acct, 200_000);

    // ---- Attacker + victim on asset 1 ----
    let mut victim = signer();
    let mut atk_acct = portfolio_account();
    let mut vic_acct = portfolio_account();
    init_portfolio(&mut attacker, &mut market, &mut atk_acct);
    init_portfolio(&mut victim, &mut market, &mut vic_acct);
    deposit(&mut attacker, &mut market, &mut atk_acct, 1_000_000);
    deposit(&mut victim, &mut market, &mut vic_acct, 300);

    // Snapshot honest-domain state and bystander capital after setup.
    let snap = |market: &TestAccount| {
        let (_, g) = state::read_market(&market.data).unwrap();
        (
            g.insurance_domain_budget[4],
            g.insurance_domain_budget[5],
            g.insurance_domain_spent[4],
            g.insurance_domain_spent[5],
            g.source_backing_buckets[4].fresh_unliened_backing_num,
            g.source_backing_buckets[4].utilization_fee_earnings,
        )
    };
    let honest_before = snap(&market);
    let bystander_before = state::read_portfolio(&bystander_acct.data).unwrap().capital;

    let assert_honest_isolated = |market: &TestAccount, bystander_acct: &TestAccount, tag: &str| {
        assert_eq!(
            snap(market),
            honest_before,
            "honest domain state changed after {tag}"
        );
        let (_, group) = state::read_market(&market.data).unwrap();
        assert!(
            group.vault >= group.insurance + group.c_tot,
            "vault {} < insurance {} + user capital {} after {tag}",
            group.vault,
            group.insurance,
            group.c_tot
        );
        let cap = state::read_portfolio(&bystander_acct.data).unwrap().capital;
        assert_eq!(
            cap, bystander_before,
            "bystander capital changed after {tag}"
        );
    };

    // Configure asset 1 oracle as the attacker-controlled oracle authority.
    run_ix(
        Instruction::ConfigureEwmaMark {
            asset_index: 1,
            now_slot: 300,
            initial_mark_e6: 100,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
        },
        &mut [&mut attacker, &mut market],
    )
    .unwrap();
    assert_honest_isolated(&market, &bystander_acct, "configure asset1 oracle");

    // Open opposing positions on asset 1 between attacker (long) and victim
    // (short) at the honest price.
    run_ix(
        Instruction::TradeNoCpi {
            asset_index: 1,
            size_q: POS_SCALE as i128,
            exec_price: 100,
            fee_bps: 0,
        },
        &mut [
            &mut attacker,
            &mut victim,
            &mut market,
            &mut atk_acct,
            &mut vic_acct,
        ],
    )
    .unwrap();
    assert_honest_isolated(&market, &bystander_acct, "open asset1 position");

    // Randomized attack: push the asset-1 mark to wild values and liquidate.
    let mut rng: u64 = 0xA11C_E5_DEAD_BEEFu64;
    let next = |rng: &mut u64| {
        *rng ^= *rng << 13;
        *rng ^= *rng >> 7;
        *rng ^= *rng << 17;
        *rng
    };
    let mut liquidations = 0u32;
    let mut slot = 300u64;
    for _ in 0..200 {
        slot += 1;
        // Wild attacker-chosen price in [1, 1_000_000].
        let mark = 1 + (next(&mut rng) % 1_000_000);
        let _ = run_ix(
            Instruction::PushEwmaMark {
                asset_index: 1,
                now_slot: slot,
                mark_e6: mark,
            },
            &mut [&mut attacker, &mut market],
        );
        assert_honest_isolated(&market, &bystander_acct, "push asset1 mark");

        // Liquidation crank against the victim on asset 1.
        let res = run_ix(
            Instruction::PermissionlessCrank {
                action: 1,
                asset_index: 1,
                now_slot: slot,
                funding_rate_e9: 0,
                close_q: POS_SCALE,
                fee_bps: 0,
                recovery_reason: 0,
            },
            &mut [&mut admin, &mut market, &mut vic_acct],
        );
        if res.is_ok() {
            liquidations += 1;
        }
        assert_honest_isolated(&market, &bystander_acct, "liquidate victim on asset1");

        // Attacker tries to realize gains by withdrawing - capped by their real
        // equity; can never pull honest-domain value.
        let _ = run_ix(
            Instruction::Withdraw {
                amount: (next(&mut rng) % 2_000_000) as u128,
            },
            &mut [
                &mut attacker,
                &mut market,
                &mut atk_acct,
                &mut attacker_dest,
                &mut vault_tok,
                &mut vault_auth,
                &mut token_program,
            ],
        );
        assert_honest_isolated(&market, &bystander_acct, "attacker withdraw");
    }

    // Non-vacuity: the attack must have actually moved the asset-1 mark and
    // exercised the liquidation path at least once.
    assert!(
        liquidations > 0,
        "attack never triggered a liquidation; test is vacuous"
    );
    // Non-vacuity: the attack must have actually hurt the victim (drained their
    // capital and/or closed their position), proving the loss-absorption path
    // ran while the honest domain stayed isolated.
    let victim_after = state::read_portfolio(&vic_acct.data).unwrap();
    assert!(
        victim_after.capital < 300
            || percolator::active_bitmap_is_empty(victim_after.active_bitmap),
        "victim was never harmed (capital {}); attack vacuous",
        victim_after.capital
    );
    // Final isolation check.
    assert_honest_isolated(&market, &bystander_acct, "final");
}
