use litesvm::LiteSVM;
use percolator::{
    AssetLifecycleV16, BackingBucketStatusV16, CloseProgressLedgerV16, MarketModeV16,
    PermissionlessRecoveryReasonV16, ResolvedPayoutLedgerV16, ResolvedPayoutReceiptV16,
    SideModeV16, SideV16, TradeRequestV16, ADL_ONE, BOUND_SCALE, POS_SCALE,
};
use percolator_prog::{
    constants::{MATCHER_ABI_VERSION, ORACLE_LEG_FLAG_DIVIDE_LEG2, ORACLE_LEG_FLAG_DIVIDE_LEG3},
    ix::Instruction as ProgInstruction,
    oracle_v16, processor, state,
    state::{MarketGroupV16, PortfolioAccountV16},
};
use solana_sdk::{
    account::Account,
    clock::Clock,
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction},
    program_option::COption,
    program_pack::Pack,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};
use spl_token::state::{Account as TokenAccount, AccountState, Mint};
use std::path::PathBuf;

const CRANK_CU_LIMIT: u64 = 325_000;
const CUSTODY_CU_LIMIT: u64 = 300_000;
const TRADE_CU_LIMIT: u64 = 345_000;
const MULTI_ASSET_OPEN_TRADE_CU_LIMIT: u64 = 750_000;
const MATCHER_CONTEXT_LEN: usize = 320;

fn active_bitmap_with(indices: &[usize]) -> percolator::V16ActiveBitmap {
    let mut bitmap = percolator::active_bitmap_empty();
    for &idx in indices {
        percolator::active_bitmap_set(&mut bitmap, idx).unwrap();
    }
    bitmap
}

fn active_leg_for_asset(
    account: &PortfolioAccountV16,
    asset_index: usize,
) -> percolator::PortfolioLegV16 {
    account
        .legs
        .iter()
        .copied()
        .find(|leg| leg.active && leg.asset_index as usize == asset_index)
        .unwrap()
}

fn has_active_leg_for_asset(account: &PortfolioAccountV16, asset_index: usize) -> bool {
    account
        .legs
        .iter()
        .any(|leg| leg.active && leg.asset_index as usize == asset_index)
}

fn program_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("target/deploy/percolator_prog.so");
    assert!(
        path.exists(),
        "BPF not found at {:?}. Run `cargo build-sbf --no-default-features` first",
        path
    );
    path
}

fn matcher_program_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.push("percolator-match/target/deploy/percolator_match.so");
    assert!(
        path.exists(),
        "matcher BPF not found at {:?}. Run `cd ../percolator-match && cargo build-sbf` first",
        path
    );
    path
}

fn spl_token_program_path() -> PathBuf {
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let mut home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
            home.push(".cargo");
            home
        });
    let registry_src = cargo_home.join("registry/src");
    for registry in std::fs::read_dir(&registry_src).expect("registry/src") {
        let registry = registry.expect("registry entry").path();
        let candidate = registry.join("litesvm-0.1.0/src/spl/programs/spl_token-3.5.0.so");
        if candidate.exists() {
            return candidate;
        }
    }
    panic!("could not find LiteSVM SPL Token BPF under {registry_src:?}");
}

fn matcher_delegate_key(
    program_id: &Pubkey,
    market: &Pubkey,
    maker: &Pubkey,
    matcher_program: &Pubkey,
    matcher_context: &Pubkey,
) -> Pubkey {
    Pubkey::find_program_address(
        &[
            b"matcher",
            market.as_ref(),
            maker.as_ref(),
            matcher_program.as_ref(),
            matcher_context.as_ref(),
        ],
        program_id,
    )
    .0
}

fn encode_matcher_init_passive(max_fill_abs: u128) -> Vec<u8> {
    encode_matcher_init_passive_with_spread(max_fill_abs, 0, 100)
}

fn encode_matcher_init_passive_with_spread(
    max_fill_abs: u128,
    base_spread_bps: u32,
    max_total_bps: u32,
) -> Vec<u8> {
    let mut data = vec![0u8; 66];
    data[0] = 2;
    data[1] = 0;
    data[6..10].copy_from_slice(&base_spread_bps.to_le_bytes());
    data[10..14].copy_from_slice(&max_total_bps.to_le_bytes());
    data[34..50].copy_from_slice(&max_fill_abs.to_le_bytes());
    data
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
    let mut data = vec![0u8; TokenAccount::LEN];
    TokenAccount::pack(
        TokenAccount {
            mint,
            owner,
            amount,
            delegate: COption::None,
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 0,
            close_authority: COption::None,
        },
        &mut data,
    )
    .unwrap();
    data
}

fn make_pyth_data(
    feed_id: &[u8; 32],
    price: i64,
    expo: i32,
    conf: u64,
    publish_time: i64,
) -> Vec<u8> {
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

fn cu_ix() -> Instruction {
    ComputeBudgetInstruction::set_compute_unit_limit(1_400_000)
}

fn heap_ix() -> Instruction {
    ComputeBudgetInstruction::request_heap_frame(128 * 1024)
}

struct V16CuEnv {
    svm: LiteSVM,
    program_id: Pubkey,
    payer: Keypair,
    admin: Keypair,
    market: Pubkey,
    mint: Pubkey,
    vault: Pubkey,
    vault_authority: Pubkey,
    portfolio_account_len: usize,
}

#[derive(Clone, Copy)]
struct V16CuMarketParams {
    max_portfolio_assets: u16,
    h_min: u64,
    h_max: u64,
    initial_price: u64,
    min_nonzero_mm_req: u128,
    min_nonzero_im_req: u128,
    maintenance_margin_bps: u64,
    initial_margin_bps: u64,
    max_trading_fee_bps: u64,
    trade_fee_base_bps: u64,
    liquidation_fee_bps: u64,
    liquidation_fee_cap: u128,
    min_liquidation_abs: u128,
    max_price_move_bps_per_slot: u64,
    max_accrual_dt_slots: u64,
    max_abs_funding_e9_per_slot: u64,
    min_funding_lifetime_slots: u64,
    max_account_b_settlement_chunks: u64,
    max_bankrupt_close_chunks: u64,
    max_bankrupt_close_lifetime_slots: u64,
    public_b_chunk_atoms: u128,
    maintenance_fee_per_slot: u128,
}

impl Default for V16CuMarketParams {
    fn default() -> Self {
        Self {
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
}

impl V16CuEnv {
    fn new() -> Self {
        Self::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000)
    }

    fn new_with_market_params_and_price_move(
        max_portfolio_assets: u16,
        maintenance_margin_bps: u64,
        initial_margin_bps: u64,
        max_price_move_bps_per_slot: u64,
    ) -> Self {
        Self::new_with_market_params_price_move_and_maintenance_fee(
            max_portfolio_assets,
            maintenance_margin_bps,
            initial_margin_bps,
            max_price_move_bps_per_slot,
            0,
        )
    }

    fn new_with_market_params_price_move_and_maintenance_fee(
        max_portfolio_assets: u16,
        maintenance_margin_bps: u64,
        initial_margin_bps: u64,
        max_price_move_bps_per_slot: u64,
        maintenance_fee_per_slot: u128,
    ) -> Self {
        Self::new_with_init_params(V16CuMarketParams {
            max_portfolio_assets,
            maintenance_margin_bps,
            initial_margin_bps,
            max_price_move_bps_per_slot,
            maintenance_fee_per_slot,
            ..V16CuMarketParams::default()
        })
    }

    fn new_with_init_params(params: V16CuMarketParams) -> Self {
        let mut svm = LiteSVM::new();
        let program_id = percolator_prog::id();
        let program_bytes = std::fs::read(program_path()).expect("read BPF");
        svm.add_program(program_id, &program_bytes);
        let token_program_bytes = std::fs::read(spl_token_program_path()).expect("read token BPF");
        svm.add_program(spl_token::ID, &token_program_bytes);

        let payer = Keypair::new();
        let admin = Keypair::new();
        let market = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let vault = Pubkey::new_unique();
        let vault_authority =
            Pubkey::find_program_address(&[b"vault", market.as_ref()], &program_id).0;
        svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
        svm.airdrop(&admin.pubkey(), 1_000_000_000).unwrap();
        svm.set_account(
            mint,
            Account {
                lamports: 1_000_000_000,
                data: make_mint_data(),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
        svm.set_account(
            vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(mint, vault_authority, 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
        svm.set_account(
            market,
            Account {
                lamports: 1_000_000_000,
                data: vec![
                    0u8;
                    state::market_account_len_for_capacity(
                        params.max_portfolio_assets as usize
                    )
                    .unwrap()
                ],
                owner: program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

        send_tx(
            &mut svm,
            program_id,
            &payer,
            ProgInstruction::InitMarket {
                max_portfolio_assets: params.max_portfolio_assets,
                h_min: params.h_min,
                h_max: params.h_max,
                initial_price: params.initial_price,
                min_nonzero_mm_req: params.min_nonzero_mm_req,
                min_nonzero_im_req: params.min_nonzero_im_req,
                maintenance_margin_bps: params.maintenance_margin_bps,
                initial_margin_bps: params.initial_margin_bps,
                max_trading_fee_bps: params.max_trading_fee_bps,
                trade_fee_base_bps: params.trade_fee_base_bps,
                liquidation_fee_bps: params.liquidation_fee_bps,
                liquidation_fee_cap: params.liquidation_fee_cap,
                min_liquidation_abs: params.min_liquidation_abs,
                max_price_move_bps_per_slot: params.max_price_move_bps_per_slot,
                max_accrual_dt_slots: params.max_accrual_dt_slots,
                max_abs_funding_e9_per_slot: params.max_abs_funding_e9_per_slot,
                min_funding_lifetime_slots: params.min_funding_lifetime_slots,
                max_account_b_settlement_chunks: params.max_account_b_settlement_chunks,
                max_bankrupt_close_chunks: params.max_bankrupt_close_chunks,
                max_bankrupt_close_lifetime_slots: params.max_bankrupt_close_lifetime_slots,
                public_b_chunk_atoms: params.public_b_chunk_atoms,
                maintenance_fee_per_slot: params.maintenance_fee_per_slot,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(market, false),
                AccountMeta::new_readonly(mint, false),
            ],
            &[&admin],
        )
        .expect("init market");
        Self {
            svm,
            program_id,
            payer,
            admin,
            market,
            mint,
            vault,
            vault_authority,
            portfolio_account_len: state::portfolio_account_len_for_market_slots(
                params.max_portfolio_assets as usize,
            )
            .unwrap(),
        }
    }

    fn create_portfolio(&mut self, owner: &Keypair) -> Pubkey {
        self.create_portfolio_with_cu(owner).0
    }

    fn create_portfolio_with_cu(&mut self, owner: &Keypair) -> (Pubkey, u64) {
        let portfolio = Pubkey::new_unique();
        self.ensure_signer_account(owner.pubkey());
        self.svm
            .set_account(
                portfolio,
                Account {
                    lamports: 1_000_000_000,
                    data: vec![0u8; self.portfolio_account_len],
                    owner: self.program_id,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let cu = self
            .send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(self.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[owner],
            )
            .expect("init portfolio");
        (portfolio, cu)
    }

    fn deposit(&mut self, owner: &Keypair, portfolio: Pubkey, amount: u128) -> Pubkey {
        self.deposit_with_cu(owner, portfolio, amount).0
    }

    fn activate_asset(&mut self, asset_index: u16, now_slot: u64, initial_price: u64) -> u64 {
        self.activate_asset_with_authorities(
            asset_index,
            now_slot,
            initial_price,
            self.admin.pubkey(),
            self.admin.pubkey(),
            self.admin.pubkey(),
            self.admin.pubkey(),
        )
    }

    fn activate_asset_with_authorities(
        &mut self,
        asset_index: u16,
        now_slot: u64,
        initial_price: u64,
        insurance_authority: Pubkey,
        insurance_operator: Pubkey,
        backing_bucket_authority: Pubkey,
        oracle_authority: Pubkey,
    ) -> u64 {
        let clock = self.svm.get_sysvar::<Clock>();
        if clock.slot < now_slot {
            self.svm.warp_to_slot(now_slot);
        }
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::UpdateAssetLifecycle {
                action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
                asset_index,
                now_slot,
                initial_price,
                insurance_authority: insurance_authority.to_bytes(),
                insurance_operator: insurance_operator.to_bytes(),
                backing_bucket_authority: backing_bucket_authority.to_bytes(),
                oracle_authority: oracle_authority.to_bytes(),
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("activate asset")
    }

    fn update_market_init_fee_policy_with_cu(&mut self, min_init_fee: u128) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::UpdateMarketInitFeePolicy { min_init_fee },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("update market init fee policy")
    }

    fn update_asset_lifecycle_as_admin_with_cu(
        &mut self,
        action: u8,
        asset_index: u16,
        now_slot: u64,
        initial_price: u64,
    ) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::UpdateAssetLifecycle {
                action,
                asset_index,
                now_slot,
                initial_price,
                insurance_authority: self.admin.pubkey().to_bytes(),
                insurance_operator: self.admin.pubkey().to_bytes(),
                backing_bucket_authority: self.admin.pubkey().to_bytes(),
                oracle_authority: self.admin.pubkey().to_bytes(),
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("update asset lifecycle as admin")
    }

    fn update_liquidation_fee_policy_with_cu(&mut self, cranker_share_bps: u16) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::UpdateLiquidationFeePolicy { cranker_share_bps },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("update liquidation fee policy")
    }

    fn update_backing_fee_policy_with_cu(
        &mut self,
        domain: u8,
        fee_bps: u16,
        insurance_share_bps: u16,
    ) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::UpdateBackingFeePolicy {
                domain,
                fee_bps,
                insurance_share_bps,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("update backing fee policy")
    }

    fn update_trade_fee_policy_with_cu(&mut self, trade_fee_base_bps: u64) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::UpdateTradeFeePolicy { trade_fee_base_bps },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("update trade fee policy")
    }

    fn update_fee_redirect_policy_with_cu(&mut self, redirect_bps: u16) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::UpdateFeeRedirectPolicy { redirect_bps },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("update fee redirect policy")
    }

    fn update_asset_authority_with_cu(&mut self, new_authority: &Keypair) -> u64 {
        self.ensure_signer_account(new_authority.pubkey());
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::UpdateAuthority {
                kind: processor::AUTHORITY_ASSET,
                new_pubkey: new_authority.pubkey().to_bytes(),
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(new_authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin, new_authority],
        )
        .expect("update asset authority")
    }

    fn update_base_unit_mints_with_cu(
        &mut self,
        primary_mint: Pubkey,
        secondary_mint: Pubkey,
    ) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::UpdateBaseUnitMints {
                primary_mint: primary_mint.to_bytes(),
                secondary_mint: secondary_mint.to_bytes(),
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new_readonly(primary_mint, false),
                AccountMeta::new_readonly(secondary_mint, false),
            ],
            &[&self.admin],
        )
        .expect("update base unit mints")
    }

    fn swap_secondary_for_primary_with_cu(
        &mut self,
        primary_source: Pubkey,
        primary_vault: Pubkey,
        secondary_dest: Pubkey,
        secondary_vault: Pubkey,
        amount: u128,
    ) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::SwapSecondaryForPrimary { amount },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new_readonly(self.market, false),
                AccountMeta::new(primary_source, false),
                AccountMeta::new(primary_vault, false),
                AccountMeta::new(secondary_dest, false),
                AccountMeta::new(secondary_vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&self.admin],
        )
        .expect("swap secondary for primary")
    }

    fn token_account(&mut self, owner: Pubkey, amount: u64) -> Pubkey {
        let token = Pubkey::new_unique();
        self.svm
            .set_account(
                token,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, owner, amount),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        token
    }

    fn ensure_signer_account(&mut self, key: Pubkey) {
        if self.svm.get_account(&key).is_none() {
            self.svm.airdrop(&key, 1_000_000_000).unwrap();
        }
    }

    fn create_mint(&mut self) -> Pubkey {
        let mint = Pubkey::new_unique();
        self.svm
            .set_account(
                mint,
                Account {
                    lamports: 1_000_000_000,
                    data: make_mint_data(),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        mint
    }

    fn token_account_for_mint(&mut self, mint: Pubkey, owner: Pubkey, amount: u64) -> Pubkey {
        let token = Pubkey::new_unique();
        self.svm
            .set_account(
                token,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(mint, owner, amount),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        token
    }

    fn program_account(&mut self, data_len: usize) -> Pubkey {
        let key = Pubkey::new_unique();
        self.svm
            .set_account(
                key,
                Account {
                    lamports: 1_000_000_000,
                    data: vec![0u8; data_len],
                    owner: self.program_id,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        key
    }

    fn backing_domain_ledger_account(&mut self) -> Pubkey {
        self.program_account(state::backing_domain_ledger_account_len())
    }

    fn insurance_ledger_account(&mut self) -> Pubkey {
        self.program_account(state::insurance_ledger_account_len())
    }

    fn set_token_account_amount(
        &mut self,
        token: Pubkey,
        mint: Pubkey,
        owner: Pubkey,
        amount: u64,
    ) {
        let mut account = self.svm.get_account(&token).expect("token account");
        account.data = make_token_data(mint, owner, amount);
        account.owner = spl_token::ID;
        self.svm.set_account(token, account).unwrap();
    }

    fn market_state(&self) -> (state::WrapperConfigV16, MarketGroupV16) {
        let account = self.svm.get_account(&self.market).expect("market account");
        state::read_market(&account.data).unwrap()
    }

    fn portfolio_state(&self, portfolio: Pubkey) -> PortfolioAccountV16 {
        let account = self.svm.get_account(&portfolio).expect("portfolio account");
        state::read_portfolio(&account.data).unwrap()
    }

    fn mutate_market<F>(&mut self, f: F)
    where
        F: FnOnce(&mut state::WrapperConfigV16, &mut MarketGroupV16),
    {
        let mut account = self.svm.get_account(&self.market).expect("market account");
        let (mut cfg, mut group) = state::read_market(&account.data).unwrap();
        f(&mut cfg, &mut group);
        state::write_market(&mut account.data, &cfg, &group).unwrap();
        self.svm.set_account(self.market, account).unwrap();
    }

    fn add_source_positive_pnl(&mut self, portfolio: Pubkey, domain: usize, amount: u128) {
        let mut market_account = self.svm.get_account(&self.market).expect("market account");
        let mut portfolio_account = self.svm.get_account(&portfolio).expect("portfolio account");
        let (cfg, mut group) = state::read_market(&market_account.data).unwrap();
        let mut account = state::read_portfolio(&portfolio_account.data).unwrap();
        group
            .add_account_source_positive_pnl_not_atomic(&mut account, domain, amount)
            .unwrap();
        state::write_market(&mut market_account.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut portfolio_account.data, &account).unwrap();
        self.svm.set_account(self.market, market_account).unwrap();
        self.svm.set_account(portfolio, portfolio_account).unwrap();
    }

    fn seed_cancellable_close_progress(&mut self, portfolio: Pubkey) {
        let mut market_account = self.svm.get_account(&self.market).expect("market account");
        let mut portfolio_account = self.svm.get_account(&portfolio).expect("portfolio account");
        let (cfg, mut group) = state::read_market(&market_account.data).unwrap();
        let mut account = state::read_portfolio(&portfolio_account.data).unwrap();
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
        state::write_market(&mut market_account.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut portfolio_account.data, &account).unwrap();
        self.svm.set_account(self.market, market_account).unwrap();
        self.svm.set_account(portfolio, portfolio_account).unwrap();
    }

    fn activate_permissionless_asset_with_fee(
        &mut self,
        creator: &Keypair,
        asset_index: u16,
        now_slot: u64,
        initial_price: u64,
        insurance_authority: Pubkey,
        insurance_operator: Pubkey,
        backing_bucket_authority: Pubkey,
        oracle_authority: Pubkey,
        fee: u128,
    ) -> (Pubkey, u64) {
        self.ensure_signer_account(creator.pubkey());
        let source = self.token_account(creator.pubkey(), fee as u64);
        let cu = send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::UpdateAssetLifecycle {
                action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
                asset_index,
                now_slot,
                initial_price,
                insurance_authority: insurance_authority.to_bytes(),
                insurance_operator: insurance_operator.to_bytes(),
                backing_bucket_authority: backing_bucket_authority.to_bytes(),
                oracle_authority: oracle_authority.to_bytes(),
            },
            vec![
                AccountMeta::new(creator.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[creator],
        )
        .expect("permissionless asset activation with fee");
        (source, cu)
    }

    fn deposit_with_cu(
        &mut self,
        owner: &Keypair,
        portfolio: Pubkey,
        amount: u128,
    ) -> (Pubkey, u64) {
        let source = Pubkey::new_unique();
        self.svm
            .set_account(
                source,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, owner.pubkey(), amount as u64),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let cu = self
            .send(
                ProgInstruction::Deposit { amount },
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(self.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(source, false),
                    AccountMeta::new(self.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[owner],
            )
            .expect("deposit");
        (source, cu)
    }

    fn trade_with_cu(
        &mut self,
        owner_a: &Keypair,
        account_a: Pubkey,
        owner_b: &Keypair,
        account_b: Pubkey,
        size_q: i128,
        exec_price: u64,
        fee_bps: u64,
    ) -> u64 {
        self.trade_asset_with_cu(
            0, owner_a, account_a, owner_b, account_b, size_q, exec_price, fee_bps,
        )
    }

    fn trade_asset_with_cu(
        &mut self,
        asset_index: u16,
        owner_a: &Keypair,
        account_a: Pubkey,
        owner_b: &Keypair,
        account_b: Pubkey,
        size_q: i128,
        exec_price: u64,
        fee_bps: u64,
    ) -> u64 {
        self.try_trade_asset_with_cu(
            asset_index,
            owner_a,
            account_a,
            owner_b,
            account_b,
            size_q,
            exec_price,
            fee_bps,
        )
        .expect("trade")
    }

    #[allow(clippy::too_many_arguments)]
    fn try_trade_asset_with_cu(
        &mut self,
        asset_index: u16,
        owner_a: &Keypair,
        account_a: Pubkey,
        owner_b: &Keypair,
        account_b: Pubkey,
        size_q: i128,
        exec_price: u64,
        fee_bps: u64,
    ) -> Result<u64, String> {
        self.send(
            ProgInstruction::TradeNoCpi {
                asset_index,
                size_q,
                exec_price,
                fee_bps,
            },
            vec![
                AccountMeta::new(owner_a.pubkey(), true),
                AccountMeta::new(owner_b.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(account_a, false),
                AccountMeta::new(account_b, false),
            ],
            &[owner_a, owner_b],
        )
    }

    fn update_maintenance_fee_policy_with_cu(&mut self, cranker_share_bps: u16) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::UpdateMaintenanceFeePolicy { cranker_share_bps },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("update maintenance fee policy")
    }

    fn sync_maintenance_fee_with_cu(
        &mut self,
        portfolio: Pubkey,
        cranker_portfolio: Option<Pubkey>,
        now_slot: u64,
    ) -> u64 {
        self.try_sync_maintenance_fee_with_cu(portfolio, cranker_portfolio, now_slot)
            .expect("sync maintenance fee")
    }

    fn try_sync_maintenance_fee_with_cu(
        &mut self,
        portfolio: Pubkey,
        cranker_portfolio: Option<Pubkey>,
        now_slot: u64,
    ) -> Result<u64, String> {
        let mut accounts = vec![
            AccountMeta::new(self.market, false),
            AccountMeta::new(portfolio, false),
        ];
        if let Some(cranker_portfolio) = cranker_portfolio {
            accounts.push(AccountMeta::new(cranker_portfolio, false));
        }
        self.send(
            ProgInstruction::SyncMaintenanceFee { now_slot },
            accounts,
            &[],
        )
    }

    fn seed_n_leg_position_for_benchmark(
        &mut self,
        long_account: Pubkey,
        short_account: Pubkey,
        n: usize,
    ) {
        let mut market_account = self.svm.get_account(&self.market).expect("market account");
        let mut long_data = self.svm.get_account(&long_account).expect("long account");
        let mut short_data = self.svm.get_account(&short_account).expect("short account");
        let (_, _, max_market_slots, _) =
            state::read_market_config_mode_and_capacity(&market_account.data).unwrap();
        {
            let (_, mut group) = state::market_view_mut(&mut market_account.data).unwrap();
            let mut long =
                state::portfolio_view_mut_for_market_slots(&mut long_data.data, max_market_slots)
                    .unwrap();
            let mut short =
                state::portfolio_view_mut_for_market_slots(&mut short_data.data, max_market_slots)
                    .unwrap();
            for asset_index in 0..n {
                group
                    .execute_trade_with_fee_in_place_not_atomic(
                        &mut long,
                        &mut short,
                        TradeRequestV16 {
                            asset_index,
                            size_q: 10 * POS_SCALE,
                            exec_price: 100,
                            fee_bps: 0,
                        },
                    )
                    .unwrap();
            }
            for asset_index in 0..n {
                group
                    .accrue_asset_to_not_atomic(asset_index, 16, 95, 0, true)
                    .unwrap();
                group.markets[asset_index]
                    .engine
                    .asset
                    .raw_oracle_target_price = percolator::V16PodU64::new(95);
            }
        }
        self.svm.set_account(self.market, market_account).unwrap();
        self.svm.set_account(long_account, long_data).unwrap();
        self.svm.set_account(short_account, short_data).unwrap();
    }

    fn seed_current_n_leg_position_for_benchmark(
        &mut self,
        long_account: Pubkey,
        short_account: Pubkey,
        n: usize,
    ) {
        let mut market_account = self.svm.get_account(&self.market).expect("market account");
        let mut long_data = self.svm.get_account(&long_account).expect("long account");
        let mut short_data = self.svm.get_account(&short_account).expect("short account");
        let (_, _, max_market_slots, _) =
            state::read_market_config_mode_and_capacity(&market_account.data).unwrap();
        {
            let (_, mut group) = state::market_view_mut(&mut market_account.data).unwrap();
            let mut long =
                state::portfolio_view_mut_for_market_slots(&mut long_data.data, max_market_slots)
                    .unwrap();
            let mut short =
                state::portfolio_view_mut_for_market_slots(&mut short_data.data, max_market_slots)
                    .unwrap();
            for asset_index in 0..n {
                group
                    .execute_trade_with_fee_in_place_not_atomic(
                        &mut long,
                        &mut short,
                        TradeRequestV16 {
                            asset_index,
                            size_q: 10 * POS_SCALE,
                            exec_price: 100,
                            fee_bps: 0,
                        },
                    )
                    .unwrap();
            }
        }
        self.svm.set_account(self.market, market_account).unwrap();
        self.svm.set_account(long_account, long_data).unwrap();
        self.svm.set_account(short_account, short_data).unwrap();
    }

    fn force_portfolio_capital_for_benchmark(&mut self, portfolio_key: Pubkey, new_capital: u128) {
        let mut market_account = self.svm.get_account(&self.market).expect("market account");
        let mut portfolio_data = self
            .svm
            .get_account(&portfolio_key)
            .expect("portfolio account");
        let (cfg, mut group) = state::read_market(&market_account.data).unwrap();
        let mut portfolio = state::read_portfolio(&portfolio_data.data).unwrap();
        let old_capital = portfolio.capital;
        if new_capital < old_capital {
            let delta = old_capital - new_capital;
            group.c_tot -= delta;
            group.vault -= delta;
        } else {
            let delta = new_capital - old_capital;
            group.c_tot += delta;
            group.vault += delta;
        }
        portfolio.capital = new_capital;
        portfolio.health_cert.valid = false;
        state::write_market(&mut market_account.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut portfolio_data.data, &portfolio).unwrap();
        self.svm.set_account(self.market, market_account).unwrap();
        self.svm.set_account(portfolio_key, portfolio_data).unwrap();
    }

    fn force_portfolio_bankruptcy_for_security_test(&mut self, portfolio_key: Pubkey, loss: u128) {
        let mut market_account = self.svm.get_account(&self.market).expect("market account");
        let mut portfolio_data = self
            .svm
            .get_account(&portfolio_key)
            .expect("portfolio account");
        let (cfg, mut group) = state::read_market(&market_account.data).unwrap();
        let mut portfolio = state::read_portfolio(&portfolio_data.data).unwrap();
        if portfolio.capital != 0 {
            group.c_tot -= portfolio.capital;
            group.vault -= portfolio.capital;
            portfolio.capital = 0;
        }
        assert_eq!(
            portfolio.pnl, 0,
            "security seed expects neutral starting pnl"
        );
        let loss_i128 = i128::try_from(loss).unwrap();
        portfolio.pnl = -loss_i128;
        group.negative_pnl_account_count += 1;
        portfolio.health_cert.valid = false;
        state::write_market(&mut market_account.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut portfolio_data.data, &portfolio).unwrap();
        self.svm.set_account(self.market, market_account).unwrap();
        self.svm.set_account(portfolio_key, portfolio_data).unwrap();
    }

    fn force_portfolio_loss_for_security_test(&mut self, portfolio_key: Pubkey, loss: u128) {
        let mut market_account = self.svm.get_account(&self.market).expect("market account");
        let mut portfolio_data = self
            .svm
            .get_account(&portfolio_key)
            .expect("portfolio account");
        let (cfg, mut group) = state::read_market(&market_account.data).unwrap();
        let mut portfolio = state::read_portfolio(&portfolio_data.data).unwrap();
        assert!(
            portfolio.capital > loss,
            "loss must remain fully covered by capital"
        );
        assert_eq!(
            portfolio.pnl, 0,
            "security seed expects neutral starting pnl"
        );
        let loss_i128 = i128::try_from(loss).unwrap();
        portfolio.pnl = -loss_i128;
        group.negative_pnl_account_count += 1;
        portfolio.health_cert.valid = false;
        state::write_market(&mut market_account.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut portfolio_data.data, &portfolio).unwrap();
        self.svm.set_account(self.market, market_account).unwrap();
        self.svm.set_account(portfolio_key, portfolio_data).unwrap();
    }

    fn init_matcher_context(
        &mut self,
        matcher_program: Pubkey,
        maker_account: Pubkey,
    ) -> (Pubkey, Pubkey, u64) {
        self.init_matcher_context_with_data(
            matcher_program,
            maker_account,
            encode_matcher_init_passive(u128::MAX),
        )
    }

    fn init_matcher_context_with_passive_spread(
        &mut self,
        matcher_program: Pubkey,
        maker_account: Pubkey,
        base_spread_bps: u32,
        max_total_bps: u32,
    ) -> (Pubkey, Pubkey, u64) {
        self.init_matcher_context_with_data(
            matcher_program,
            maker_account,
            encode_matcher_init_passive_with_spread(u128::MAX, base_spread_bps, max_total_bps),
        )
    }

    fn init_matcher_context_with_data(
        &mut self,
        matcher_program: Pubkey,
        maker_account: Pubkey,
        init_data: Vec<u8>,
    ) -> (Pubkey, Pubkey, u64) {
        let ctx = Pubkey::new_unique();
        let delegate = matcher_delegate_key(
            &self.program_id,
            &self.market,
            &maker_account,
            &matcher_program,
            &ctx,
        );
        self.svm
            .set_account(
                delegate,
                Account {
                    lamports: 1_000_000_000,
                    data: vec![],
                    owner: Pubkey::default(),
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        self.svm
            .set_account(
                ctx,
                Account {
                    lamports: 1_000_000_000,
                    data: vec![0u8; MATCHER_CONTEXT_LEN],
                    owner: matcher_program,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let cu = send_raw_tx(
            &mut self.svm,
            &self.payer,
            Instruction {
                program_id: matcher_program,
                accounts: vec![
                    AccountMeta::new_readonly(delegate, false),
                    AccountMeta::new(ctx, false),
                ],
                data: init_data,
            },
            &[],
        )
        .expect("init matcher context");
        (ctx, delegate, cu)
    }

    fn trade_cpi_with_cu(
        &mut self,
        owner_a: &Keypair,
        account_a: Pubkey,
        owner_b: &Keypair,
        account_b: Pubkey,
        matcher_program: Pubkey,
        matcher_context: Pubkey,
        matcher_delegate: Pubkey,
        size_q: i128,
        fee_bps: u64,
    ) -> u64 {
        self.trade_cpi_with_cu_on_asset(
            owner_a,
            account_a,
            owner_b,
            account_b,
            matcher_program,
            matcher_context,
            matcher_delegate,
            0,
            size_q,
            fee_bps,
        )
    }

    fn trade_cpi_with_cu_on_asset(
        &mut self,
        owner_a: &Keypair,
        account_a: Pubkey,
        owner_b: &Keypair,
        account_b: Pubkey,
        matcher_program: Pubkey,
        matcher_context: Pubkey,
        matcher_delegate: Pubkey,
        asset_index: u16,
        size_q: i128,
        fee_bps: u64,
    ) -> u64 {
        self.try_trade_cpi_with_cu_on_asset(
            owner_a,
            account_a,
            owner_b,
            account_b,
            matcher_program,
            matcher_context,
            matcher_delegate,
            asset_index,
            size_q,
            fee_bps,
        )
        .expect("trade cpi")
    }

    #[allow(clippy::too_many_arguments)]
    fn try_trade_cpi_with_cu_on_asset(
        &mut self,
        owner_a: &Keypair,
        account_a: Pubkey,
        owner_b: &Keypair,
        account_b: Pubkey,
        matcher_program: Pubkey,
        matcher_context: Pubkey,
        matcher_delegate: Pubkey,
        asset_index: u16,
        size_q: i128,
        fee_bps: u64,
    ) -> Result<u64, String> {
        self.send(
            ProgInstruction::TradeCpi {
                asset_index,
                size_q,
                fee_bps,
                limit_price: 0,
            },
            vec![
                AccountMeta::new(owner_a.pubkey(), true),
                AccountMeta::new(owner_b.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(account_a, false),
                AccountMeta::new(account_b, false),
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new(matcher_context, false),
                AccountMeta::new_readonly(matcher_delegate, false),
            ],
            &[owner_a, owner_b],
        )
    }

    fn withdraw(&mut self, owner: &Keypair, portfolio: Pubkey, amount: u128) -> Pubkey {
        self.withdraw_with_cu(owner, portfolio, amount).0
    }

    fn close_portfolio_with_cu(&mut self, owner: &Keypair, portfolio: Pubkey) -> u64 {
        self.send(
            ProgInstruction::ClosePortfolio,
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[owner],
        )
        .expect("close portfolio")
    }

    fn withdraw_with_cu(
        &mut self,
        owner: &Keypair,
        portfolio: Pubkey,
        amount: u128,
    ) -> (Pubkey, u64) {
        let dest = Pubkey::new_unique();
        self.svm
            .set_account(
                dest,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, owner.pubkey(), 0),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let cu = self
            .send(
                ProgInstruction::Withdraw { amount },
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(self.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(dest, false),
                    AccountMeta::new(self.vault, false),
                    AccountMeta::new_readonly(self.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[owner],
            )
            .expect("withdraw");
        (dest, cu)
    }

    fn resolve(&mut self) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::ResolveMarket,
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("resolve market")
    }

    fn close_slab_with_cu(&mut self) -> u64 {
        let dest = Pubkey::new_unique();
        self.svm
            .set_account(
                dest,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, self.admin.pubkey(), 0),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::CloseSlab,
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new(dest, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&self.admin],
        )
        .expect("close slab")
    }

    fn configure_permissionless_resolve_with_cu(
        &mut self,
        stale_slots: u64,
        force_close_delay_slots: u64,
    ) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::ConfigurePermissionlessResolve {
                stale_slots,
                force_close_delay_slots,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("configure permissionless resolve")
    }

    fn enable_live_insurance_withdrawal(&mut self) {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::UpdateInsurancePolicy {
                max_bps: 5_000,
                deposits_only: 0,
                cooldown_slots: 1,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("enable live insurance withdrawal");
    }

    fn set_pyth_price(
        &mut self,
        feed: &[u8; 32],
        price: i64,
        expo: i32,
        publish_time: i64,
    ) -> Pubkey {
        self.set_pyth_price_with_conf(feed, price, expo, 1, publish_time)
    }

    fn set_pyth_price_with_conf(
        &mut self,
        feed: &[u8; 32],
        price: i64,
        expo: i32,
        conf: u64,
        publish_time: i64,
    ) -> Pubkey {
        let key = Pubkey::new_unique();
        self.svm
            .set_account(
                key,
                Account {
                    lamports: 1_000_000_000,
                    data: make_pyth_data(feed, price, expo, conf, publish_time),
                    owner: oracle_v16::PYTH_RECEIVER_PROGRAM_ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        key
    }

    fn configure_three_leg_hybrid_with_cu(
        &mut self,
        feeds: [[u8; 32]; 3],
        leg0: Pubkey,
        leg1: Pubkey,
        leg2: Pubkey,
        now_slot: u64,
        now_unix_ts: i64,
    ) -> u64 {
        self.try_configure_three_leg_hybrid(feeds, leg0, leg1, leg2, now_slot, now_unix_ts)
            .expect("configure hybrid oracle")
    }

    fn try_configure_three_leg_hybrid(
        &mut self,
        feeds: [[u8; 32]; 3],
        leg0: Pubkey,
        leg1: Pubkey,
        leg2: Pubkey,
        now_slot: u64,
        now_unix_ts: i64,
    ) -> Result<u64, String> {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::ConfigureHybridOracle {
                asset_index: 0,
                now_slot,
                now_unix_ts,
                oracle_leg_count: 3,
                oracle_leg_flags: ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3,
                max_staleness_secs: 60,
                hybrid_soft_stale_slots: 3,
                mark_ewma_halflife_slots: 1,
                mark_min_fee: 0,
                invert: 0,
                unit_scale: 0,
                conf_filter_bps: 500,
                oracle_leg_feeds: feeds,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new_readonly(leg0, false),
                AccountMeta::new_readonly(leg1, false),
                AccountMeta::new_readonly(leg2, false),
            ],
            &[&self.admin],
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn try_configure_hybrid_with_cu(
        &mut self,
        oracle_leg_count: u8,
        oracle_leg_flags: u8,
        feeds: [[u8; 32]; 3],
        oracle_accounts: &[Pubkey],
        now_slot: u64,
        now_unix_ts: i64,
        invert: u8,
        unit_scale: u32,
        hybrid_soft_stale_slots: u64,
    ) -> Result<u64, String> {
        self.try_configure_hybrid_asset_with_cu(
            0,
            oracle_leg_count,
            oracle_leg_flags,
            feeds,
            oracle_accounts,
            now_slot,
            now_unix_ts,
            invert,
            unit_scale,
            hybrid_soft_stale_slots,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn try_configure_hybrid_asset_with_cu(
        &mut self,
        asset_index: u16,
        oracle_leg_count: u8,
        oracle_leg_flags: u8,
        feeds: [[u8; 32]; 3],
        oracle_accounts: &[Pubkey],
        now_slot: u64,
        now_unix_ts: i64,
        invert: u8,
        unit_scale: u32,
        hybrid_soft_stale_slots: u64,
    ) -> Result<u64, String> {
        self.try_configure_hybrid_asset_with_conf_filter_cu(
            asset_index,
            oracle_leg_count,
            oracle_leg_flags,
            feeds,
            oracle_accounts,
            now_slot,
            now_unix_ts,
            invert,
            unit_scale,
            hybrid_soft_stale_slots,
            500,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn try_configure_hybrid_asset_with_conf_filter_cu(
        &mut self,
        asset_index: u16,
        oracle_leg_count: u8,
        oracle_leg_flags: u8,
        feeds: [[u8; 32]; 3],
        oracle_accounts: &[Pubkey],
        now_slot: u64,
        now_unix_ts: i64,
        invert: u8,
        unit_scale: u32,
        hybrid_soft_stale_slots: u64,
        conf_filter_bps: u16,
    ) -> Result<u64, String> {
        let mut accounts = vec![
            AccountMeta::new(self.admin.pubkey(), true),
            AccountMeta::new(self.market, false),
        ];
        accounts.extend(
            oracle_accounts
                .iter()
                .take(oracle_leg_count as usize)
                .copied()
                .map(|key| AccountMeta::new_readonly(key, false)),
        );
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::ConfigureHybridOracle {
                asset_index,
                now_slot,
                now_unix_ts,
                oracle_leg_count,
                oracle_leg_flags,
                max_staleness_secs: 60,
                hybrid_soft_stale_slots,
                mark_ewma_halflife_slots: 1,
                mark_min_fee: 0,
                invert,
                unit_scale,
                conf_filter_bps,
                oracle_leg_feeds: feeds,
            },
            accounts,
            &[&self.admin],
        )
    }

    fn configure_ewma_mark_with_cu(
        &mut self,
        now_slot: u64,
        initial_mark_e6: u64,
        halflife_slots: u64,
        mark_min_fee: u64,
    ) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::ConfigureEwmaMark {
                asset_index: 0,
                now_slot,
                initial_mark_e6,
                mark_ewma_halflife_slots: halflife_slots,
                mark_min_fee,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("configure ewma_mark mark")
    }

    fn push_ewma_mark_with_cu(&mut self, now_slot: u64, mark_e6: u64) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::PushEwmaMark {
                asset_index: 0,
                now_slot,
                mark_e6,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("push ewma_mark mark")
    }

    fn configure_auth_mark_with_cu(&mut self, now_slot: u64, initial_mark_e6: u64) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::ConfigureAuthMark {
                asset_index: 0,
                now_slot,
                initial_mark_e6,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("configure auth mark")
    }

    fn push_auth_mark_with_cu(&mut self, now_slot: u64, mark_e6: u64) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::PushAuthMark {
                asset_index: 0,
                now_slot,
                mark_e6,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("push auth mark")
    }

    fn configure_auth_mark_for_asset_as_admin(
        &mut self,
        asset_index: u16,
        now_slot: u64,
        initial_mark_e6: u64,
    ) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::ConfigureAuthMark {
                asset_index,
                now_slot,
                initial_mark_e6,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("configure auth mark for asset as admin")
    }

    fn push_auth_mark_for_asset_as_admin(
        &mut self,
        asset_index: u16,
        now_slot: u64,
        mark_e6: u64,
    ) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::PushAuthMark {
                asset_index,
                now_slot,
                mark_e6,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("push auth mark for asset as admin")
    }

    fn configure_auth_mark_for_asset_with_authority(
        &mut self,
        asset_index: u16,
        authority: &Keypair,
        now_slot: u64,
        initial_mark_e6: u64,
    ) -> u64 {
        self.ensure_signer_account(authority.pubkey());
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::ConfigureAuthMark {
                asset_index,
                now_slot,
                initial_mark_e6,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
        .expect("configure auth mark for asset")
    }

    fn push_auth_mark_for_asset_with_authority(
        &mut self,
        asset_index: u16,
        authority: &Keypair,
        now_slot: u64,
        mark_e6: u64,
    ) -> u64 {
        self.ensure_signer_account(authority.pubkey());
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::PushAuthMark {
                asset_index,
                now_slot,
                mark_e6,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
        .expect("push auth mark for asset")
    }

    fn resolve_stale_permissionless_with_cu(&mut self, now_slot: u64) -> u64 {
        self.svm.warp_to_slot(now_slot);
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::ResolveStalePermissionless { now_slot },
            vec![AccountMeta::new(self.market, false)],
            &[],
        )
        .expect("resolve stale permissionless")
    }

    fn close_resolved(&mut self, owner: &Keypair, portfolio: Pubkey) -> Pubkey {
        self.close_resolved_with_cu(owner, portfolio).0
    }

    fn close_resolved_with_cu(&mut self, owner: &Keypair, portfolio: Pubkey) -> (Pubkey, u64) {
        let dest = Pubkey::new_unique();
        self.svm
            .set_account(
                dest,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, owner.pubkey(), 0),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let cu = self
            .send(
                ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                },
                vec![
                    AccountMeta::new_readonly(owner.pubkey(), false),
                    AccountMeta::new(self.market, false),
                    AccountMeta::new(portfolio, false),
                    AccountMeta::new(dest, false),
                    AccountMeta::new(self.vault, false),
                    AccountMeta::new_readonly(self.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[],
            )
            .expect("close resolved");
        (dest, cu)
    }

    fn top_up_insurance(&mut self, amount: u128) -> Pubkey {
        self.top_up_insurance_with_cu(amount).0
    }

    fn top_up_insurance_domain_with_authority(
        &mut self,
        authority: &Keypair,
        domain: u8,
        amount: u128,
    ) -> Pubkey {
        self.top_up_insurance_domain_with_authority_and_cu(authority, domain, amount)
            .0
    }

    fn top_up_backing_bucket(&mut self, domain: u8, amount: u128, expiry_slot: u64) -> Pubkey {
        self.top_up_backing_bucket_with_cu(domain, amount, expiry_slot)
            .0
    }

    fn top_up_insurance_from_admin_token_with_cu(&mut self, source: Pubkey, amount: u128) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::TopUpInsurance { amount },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&self.admin],
        )
        .expect("top up insurance from admin token")
    }

    fn top_up_backing_bucket_from_admin_token_with_cu(
        &mut self,
        source: Pubkey,
        domain: u8,
        amount: u128,
        expiry_slot: u64,
    ) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::TopUpBackingBucket {
                domain,
                amount,
                expiry_slot,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&self.admin],
        )
        .expect("top up backing bucket from admin token")
    }

    fn top_up_insurance_with_cu(&mut self, amount: u128) -> (Pubkey, u64) {
        let source = Pubkey::new_unique();
        self.svm
            .set_account(
                source,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, self.admin.pubkey(), amount as u64),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let cu = send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::TopUpInsurance { amount },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&self.admin],
        )
        .expect("top up insurance");
        (source, cu)
    }

    fn top_up_insurance_with_ledger_with_cu(
        &mut self,
        ledger: Pubkey,
        amount: u128,
    ) -> (Pubkey, u64) {
        let source = Pubkey::new_unique();
        self.svm
            .set_account(
                source,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, self.admin.pubkey(), amount as u64),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let cu = send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::TopUpInsurance { amount },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(ledger, false),
            ],
            &[&self.admin],
        )
        .expect("top up insurance with ledger");
        (source, cu)
    }

    fn top_up_insurance_domain_with_authority_and_cu(
        &mut self,
        authority: &Keypair,
        domain: u8,
        amount: u128,
    ) -> (Pubkey, u64) {
        self.ensure_signer_account(authority.pubkey());
        let source = Pubkey::new_unique();
        self.svm
            .set_account(
                source,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, authority.pubkey(), amount as u64),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let cu = send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::TopUpInsuranceDomain { domain, amount },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[authority],
        )
        .expect("top up domain insurance");
        (source, cu)
    }

    fn top_up_backing_bucket_with_cu(
        &mut self,
        domain: u8,
        amount: u128,
        expiry_slot: u64,
    ) -> (Pubkey, u64) {
        let source = Pubkey::new_unique();
        self.svm
            .set_account(
                source,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, self.admin.pubkey(), amount as u64),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let cu = send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::TopUpBackingBucket {
                domain,
                amount,
                expiry_slot,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&self.admin],
        )
        .expect("top up backing bucket");
        (source, cu)
    }

    fn top_up_backing_bucket_with_ledger_with_cu(
        &mut self,
        ledger: Pubkey,
        domain: u8,
        amount: u128,
        expiry_slot: u64,
    ) -> (Pubkey, u64) {
        let source = Pubkey::new_unique();
        self.svm
            .set_account(
                source,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, self.admin.pubkey(), amount as u64),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let cu = send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::TopUpBackingBucket {
                domain,
                amount,
                expiry_slot,
            },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(ledger, false),
            ],
            &[&self.admin],
        )
        .expect("top up backing bucket with ledger");
        (source, cu)
    }

    fn top_up_backing_bucket_with_authority(
        &mut self,
        authority: &Keypair,
        domain: u8,
        amount: u128,
        expiry_slot: u64,
    ) -> Pubkey {
        self.ensure_signer_account(authority.pubkey());
        let source = self.token_account(authority.pubkey(), amount as u64);
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::TopUpBackingBucket {
                domain,
                amount,
                expiry_slot,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[authority],
        )
        .expect("top up backing bucket with authority");
        source
    }

    fn withdraw_insurance_with_cu(&mut self, amount: u128) -> (Pubkey, u64) {
        let dest = Pubkey::new_unique();
        self.svm
            .set_account(
                dest,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, self.admin.pubkey(), 0),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let cu = send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::WithdrawInsuranceLimited { amount },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&self.admin],
        )
        .expect("withdraw insurance");
        (dest, cu)
    }

    fn withdraw_insurance_domain_to_admin_token_with_cu(
        &mut self,
        dest: Pubkey,
        domain: u8,
        amount: u128,
    ) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::WithdrawInsuranceDomain { domain, amount },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&self.admin],
        )
        .expect("withdraw domain insurance to admin token")
    }

    fn withdraw_backing_bucket_to_admin_token_with_cu(
        &mut self,
        dest: Pubkey,
        domain: u8,
        amount: u128,
    ) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::WithdrawBackingBucket { domain, amount },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&self.admin],
        )
        .expect("withdraw backing bucket to admin token")
    }

    fn sync_backing_domain_ledger_with_cu(&mut self, ledger: Pubkey, domain: u8) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::SyncBackingDomainLedger { domain },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(ledger, false),
            ],
            &[&self.admin],
        )
        .expect("sync backing domain ledger")
    }

    fn withdraw_backing_bucket_earnings_to_admin_token_with_cu(
        &mut self,
        ledger: Pubkey,
        dest: Pubkey,
        domain: u8,
        amount: u128,
    ) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::WithdrawBackingBucketEarnings { domain, amount },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(ledger, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&self.admin],
        )
        .expect("withdraw backing bucket earnings")
    }

    fn sync_insurance_ledger_with_cu(&mut self, ledger: Pubkey) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::SyncInsuranceLedger,
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(ledger, false),
            ],
            &[&self.admin],
        )
        .expect("sync insurance ledger")
    }

    fn try_withdraw_insurance_domain_with_authority(
        &mut self,
        authority: &Keypair,
        domain: u8,
        amount: u128,
    ) -> Result<(Pubkey, u64), String> {
        let dest = Pubkey::new_unique();
        self.svm
            .set_account(
                dest,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, authority.pubkey(), 0),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let cu = send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::WithdrawInsuranceDomain { domain, amount },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[authority],
        )?;
        Ok((dest, cu))
    }

    fn withdraw_terminal_insurance_with_authority(
        &mut self,
        authority: &Keypair,
        amount: u128,
    ) -> (Pubkey, u64) {
        let dest = Pubkey::new_unique();
        self.svm
            .set_account(
                dest,
                Account {
                    lamports: 1_000_000_000,
                    data: make_token_data(self.mint, authority.pubkey(), 0),
                    owner: spl_token::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let cu = send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::WithdrawInsurance { amount },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[authority],
        )
        .expect("withdraw terminal insurance");
        (dest, cu)
    }

    fn token_amount(&self, key: Pubkey) -> u64 {
        let account = self.svm.get_account(&key).expect("token account");
        TokenAccount::unpack(&account.data).unwrap().amount
    }

    fn convert_released_pnl_with_cu(
        &mut self,
        owner: &Keypair,
        portfolio: Pubkey,
        amount: u128,
    ) -> u64 {
        self.send(
            ProgInstruction::ConvertReleasedPnl { amount },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[owner],
        )
        .expect("convert released pnl")
    }

    fn cure_and_cancel_close_with_cu(
        &mut self,
        owner: &Keypair,
        portfolio: Pubkey,
        source: Pubkey,
        amount: u128,
    ) -> u64 {
        self.send(
            ProgInstruction::CureAndCancelClose {
                optional_deposit: amount,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(source, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[owner],
        )
        .expect("cure and cancel close")
    }

    fn forfeit_recovery_leg_with_cu(
        &mut self,
        owner: &Keypair,
        portfolio: Pubkey,
        asset_index: u16,
        b_delta_budget: u128,
    ) -> u64 {
        self.send(
            ProgInstruction::ForfeitRecoveryLeg {
                asset_index,
                b_delta_budget,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[owner],
        )
        .expect("forfeit recovery leg")
    }

    fn rebalance_reduce_with_cu(
        &mut self,
        owner: &Keypair,
        portfolio: Pubkey,
        asset_index: u16,
        reduce_q: u128,
    ) -> u64 {
        self.send(
            ProgInstruction::RebalanceReduce {
                asset_index,
                reduce_q,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[owner],
        )
        .expect("rebalance reduce")
    }

    fn finalize_reset_side_with_cu(&mut self, asset_index: u16, side: u8) -> u64 {
        self.send(
            ProgInstruction::FinalizeResetSide { asset_index, side },
            vec![AccountMeta::new(self.market, false)],
            &[],
        )
        .expect("finalize reset side")
    }

    fn claim_resolved_payout_topup_with_cu(
        &mut self,
        owner: Pubkey,
        portfolio: Pubkey,
        dest: Pubkey,
    ) -> u64 {
        self.send(
            ProgInstruction::ClaimResolvedPayoutTopup,
            vec![
                AccountMeta::new_readonly(owner, false),
                AccountMeta::new(self.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[],
        )
        .expect("claim resolved payout topup")
    }

    fn refine_resolved_unreceipted_bound_with_cu(&mut self, decrease_num: u128) -> u64 {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::RefineResolvedUnreceiptedBound { decrease_num },
            vec![
                AccountMeta::new(self.admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[&self.admin],
        )
        .expect("refine resolved unreceipted bound")
    }

    fn crank(&mut self, portfolio: Pubkey, ix: ProgInstruction) -> u64 {
        self.send(
            ix,
            vec![
                AccountMeta::new(self.payer.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[],
        )
        .expect("crank")
    }

    fn crank_with_oracle_tail(
        &mut self,
        portfolio: Pubkey,
        ix: ProgInstruction,
        oracle_accounts: &[Pubkey],
    ) -> u64 {
        let mut accounts = vec![
            AccountMeta::new(self.payer.pubkey(), true),
            AccountMeta::new(self.market, false),
            AccountMeta::new(portfolio, false),
        ];
        accounts.extend(
            oracle_accounts
                .iter()
                .copied()
                .map(|key| AccountMeta::new_readonly(key, false)),
        );
        self.send(ix, accounts, &[])
            .expect("crank with oracle tail")
    }

    fn try_force_close_abandoned_asset_with_cu(
        &mut self,
        cranker: &Keypair,
        account_a: Pubkey,
        account_b: Pubkey,
        asset_index: u16,
        now_slot: u64,
        close_q: u128,
    ) -> Result<u64, String> {
        self.ensure_signer_account(cranker.pubkey());
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ProgInstruction::ForceCloseAbandonedAsset {
                asset_index,
                now_slot,
                close_q,
            },
            vec![
                AccountMeta::new(cranker.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(account_a, false),
                AccountMeta::new(account_b, false),
            ],
            &[cranker],
        )
    }

    fn force_close_abandoned_asset_with_cu(
        &mut self,
        cranker: &Keypair,
        account_a: Pubkey,
        account_b: Pubkey,
        asset_index: u16,
        now_slot: u64,
        close_q: u128,
    ) -> u64 {
        self.try_force_close_abandoned_asset_with_cu(
            cranker,
            account_a,
            account_b,
            asset_index,
            now_slot,
            close_q,
        )
        .expect("force close abandoned asset")
    }

    fn send(
        &mut self,
        ix: ProgInstruction,
        accounts: Vec<AccountMeta>,
        extra_signers: &[&Keypair],
    ) -> Result<u64, String> {
        send_tx(
            &mut self.svm,
            self.program_id,
            &self.payer,
            ix,
            accounts,
            extra_signers,
        )
    }
}

fn send_tx(
    svm: &mut LiteSVM,
    program_id: Pubkey,
    payer: &Keypair,
    ix: ProgInstruction,
    accounts: Vec<AccountMeta>,
    extra_signers: &[&Keypair],
) -> Result<u64, String> {
    let instruction = Instruction {
        program_id,
        accounts,
        data: ix.encode(),
    };
    let mut signer_refs = Vec::with_capacity(1 + extra_signers.len());
    signer_refs.push(payer);
    signer_refs.extend_from_slice(extra_signers);
    let tx = Transaction::new_signed_with_payer(
        &[heap_ix(), cu_ix(), instruction],
        Some(&payer.pubkey()),
        &signer_refs,
        svm.latest_blockhash(),
    );
    svm.send_transaction(tx)
        .map(|meta| meta.compute_units_consumed)
        .map_err(|e| format!("{e:?}"))
}

fn send_raw_tx(
    svm: &mut LiteSVM,
    payer: &Keypair,
    instruction: Instruction,
    extra_signers: &[&Keypair],
) -> Result<u64, String> {
    let mut signer_refs = Vec::with_capacity(1 + extra_signers.len());
    signer_refs.push(payer);
    signer_refs.extend_from_slice(extra_signers);
    let tx = Transaction::new_signed_with_payer(
        &[heap_ix(), cu_ix(), instruction],
        Some(&payer.pubkey()),
        &signer_refs,
        svm.latest_blockhash(),
    );
    svm.send_transaction(tx)
        .map(|meta| meta.compute_units_consumed)
        .map_err(|e| format!("{e:?}"))
}

fn assert_cu_within(label: &str, cu: u64, limit: u64) {
    assert!(
        cu <= limit,
        "{label} consumed {cu} CU, above the {limit} CU guardrail"
    );
}

#[test]
fn v16_bpf_deposit_and_withdraw_move_spl_tokens_with_ledger() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);

    let source = env.deposit(&owner, portfolio, 1_000);
    assert_eq!(env.token_amount(source), 0);
    assert_eq!(env.token_amount(env.vault), 1_000);
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let portfolio_data = env.svm.get_account(&portfolio).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    let account = state::read_portfolio(&portfolio_data).unwrap();
    assert_eq!(group.vault, 1_000);
    assert_eq!(group.c_tot, 1_000);
    assert_eq!(account.capital, 1_000);

    let dest = env.withdraw(&owner, portfolio, 400);
    assert_eq!(env.token_amount(dest), 400);
    assert_eq!(env.token_amount(env.vault), 600);
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let portfolio_data = env.svm.get_account(&portfolio).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    let account = state::read_portfolio(&portfolio_data).unwrap();
    assert_eq!(group.vault, 600);
    assert_eq!(group.c_tot, 600);
    assert_eq!(account.capital, 600);

    let insurance_source = env.top_up_insurance(250);
    assert_eq!(env.token_amount(insurance_source), 0);
    assert_eq!(env.token_amount(env.vault), 850);
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    assert_eq!(group.insurance, 250);
    assert_eq!(group.vault, 850);

    let backing_source = env.top_up_backing_bucket(1, 300, 10);
    assert_eq!(env.token_amount(backing_source), 0);
    assert_eq!(env.token_amount(env.vault), 1_150);
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    assert_eq!(group.insurance, 250);
    assert_eq!(group.vault, 1_150);
    assert_eq!(group.c_tot, 600);
    assert_eq!(
        group.source_backing_buckets[1].status,
        BackingBucketStatusV16::Fresh
    );
    assert_eq!(group.source_backing_buckets[1].expiry_slot, 10);
    assert_eq!(
        group.source_backing_buckets[1].fresh_unliened_backing_num,
        300 * BOUND_SCALE
    );
    assert_eq!(
        group.source_credit[1].fresh_reserved_backing_num,
        300 * BOUND_SCALE
    );

    env.enable_live_insurance_withdrawal();
    let (insurance_dest, _withdraw_insurance_cu) = env.withdraw_insurance_with_cu(100);
    assert_eq!(env.token_amount(insurance_dest), 100);
    assert_eq!(env.token_amount(env.vault), 1_050);
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    assert_eq!(group.insurance, 150);
    assert_eq!(group.vault, 1_050);
    assert_eq!(group.c_tot, 600);
}

#[test]
fn v16_bpf_failed_deposit_spl_transfer_rolls_back_engine_credit() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let source = Pubkey::new_unique();
    env.svm
        .set_account(
            source,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, owner.pubkey(), 100),
                owner: Pubkey::new_unique(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let source_before = env.svm.get_account(&source).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let result = env.send(
        ProgInstruction::Deposit { amount: 100 },
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );

    assert!(
        result.is_err(),
        "deposit must fail when the token CPI cannot debit the source account"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&portfolio).unwrap(), portfolio_before);
    assert_eq!(env.svm.get_account(&source).unwrap(), source_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    let (_, group) = env.market_state();
    let account = env.portfolio_state(portfolio);
    assert_eq!(group.vault, 0);
    assert_eq!(group.c_tot, 0);
    assert_eq!(account.capital, 0);
}

#[test]
fn v16_bpf_failed_withdraw_spl_transfer_rolls_back_engine_debit() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 100);
    let dest = env.token_account(owner.pubkey(), 0);
    let mut corrupted_vault = env.svm.get_account(&env.vault).unwrap();
    corrupted_vault.owner = Pubkey::new_unique();
    env.svm.set_account(env.vault, corrupted_vault).unwrap();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let dest_before = env.svm.get_account(&dest).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let result = env.send(
        ProgInstruction::Withdraw { amount: 40 },
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );

    assert!(
        result.is_err(),
        "withdraw must fail when the token CPI cannot debit the vault account"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&portfolio).unwrap(), portfolio_before);
    assert_eq!(env.svm.get_account(&dest).unwrap(), dest_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    let (_, group) = env.market_state();
    let account = env.portfolio_state(portfolio);
    assert_eq!(group.vault, 100);
    assert_eq!(group.c_tot, 100);
    assert_eq!(account.capital, 100);
    assert_eq!(env.token_amount(dest), 0);
}

#[test]
fn v16_bpf_resolved_terminal_insurance_drains_dynamic_domain_after_positions_close() {
    let mut env = V16CuEnv::new();
    let insurance_authority = Keypair::new();
    let insurance_operator = Keypair::new();
    env.svm
        .airdrop(&insurance_operator.pubkey(), 1_000_000_000)
        .unwrap();
    env.activate_asset_with_authorities(
        1,
        1,
        100,
        insurance_authority.pubkey(),
        insurance_operator.pubkey(),
        env.admin.pubkey(),
        env.admin.pubkey(),
    );

    let insurance_source = env.top_up_insurance_domain_with_authority(&insurance_authority, 2, 100);
    assert_eq!(env.token_amount(insurance_source), 0);
    assert_eq!(env.token_amount(env.vault), 100);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000);
    env.deposit(&short_owner, short_account, 1_000);
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

    env.svm.warp_to_slot(10);
    env.crank(
        long_account,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 1,
            now_slot: 10,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    assert!(
        group.loss_stale_active,
        "advancing SVM Clock must reproduce the live stale-loss gate"
    );
    assert_eq!(group.insurance_domain_budget[2], 100);

    assert!(
        env.try_withdraw_insurance_domain_with_authority(&insurance_operator, 2, 100)
            .is_err(),
        "live domain withdrawal remains blocked while loss-stale"
    );
    assert_eq!(env.token_amount(env.vault), 2_100);

    env.trade_asset_with_cu(
        1,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        -(POS_SCALE as i128),
        100,
        0,
    );
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    assert_eq!(group.assets[1].oi_eff_long_q, 0);
    assert_eq!(group.assets[1].oi_eff_short_q, 0);

    let long_dest = env.withdraw(&long_owner, long_account, 1_000);
    let short_dest = env.withdraw(&short_owner, short_account, 1_000);
    assert_eq!(env.token_amount(long_dest), 1_000);
    assert_eq!(env.token_amount(short_dest), 1_000);
    env.close_portfolio_with_cu(&long_owner, long_account);
    env.close_portfolio_with_cu(&short_owner, short_account);

    env.resolve();
    let (insurance_dest, _) =
        env.withdraw_terminal_insurance_with_authority(&insurance_authority, 100);
    assert_eq!(env.token_amount(insurance_dest), 100);
    assert_eq!(env.token_amount(env.vault), 0);
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    assert_eq!(group.vault, 0);
    assert_eq!(group.insurance, 0);
    assert_eq!(group.insurance_domain_budget[2], 0);

    env.close_slab_with_cu();
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    assert!(market_data.iter().all(|b| *b == 0));
}

#[test]
fn v16_bpf_permissionless_asset_cannot_withdraw_unrelated_domain_insurance() {
    let mut env = V16CuEnv::new();
    let victim_insurance = Keypair::new();

    env.activate_asset_with_authorities(
        1,
        1,
        100,
        victim_insurance.pubkey(),
        victim_insurance.pubkey(),
        victim_insurance.pubkey(),
        victim_insurance.pubkey(),
    );
    env.activate_asset_with_authorities(
        2,
        2,
        100,
        victim_insurance.pubkey(),
        victim_insurance.pubkey(),
        victim_insurance.pubkey(),
        victim_insurance.pubkey(),
    );
    env.top_up_insurance(500);
    env.top_up_insurance_domain_with_authority(&victim_insurance, 2, 500);
    env.top_up_insurance_domain_with_authority(&victim_insurance, 4, 500);

    let before_vault = env.token_amount(env.vault);
    let (_, before_group) = env.market_state();
    assert_eq!(before_vault, 1_500);
    assert_eq!(before_group.insurance, 1_500);
    assert_eq!(before_group.insurance_domain_budget[0], 250);
    assert_eq!(before_group.insurance_domain_budget[1], 250);
    assert_eq!(before_group.insurance_domain_budget[2], 500);
    assert_eq!(before_group.insurance_domain_budget[4], 500);

    let attacker = Keypair::new();
    env.update_market_init_fee_policy_with_cu(1);
    env.svm.warp_to_slot(3);
    let (_fee_source, _cu) = env.activate_permissionless_asset_with_fee(
        &attacker,
        3,
        3,
        100,
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        1,
    );

    let after_create_vault = env.token_amount(env.vault);
    let (_, after_create_group) = env.market_state();
    assert_eq!(
        after_create_group.assets[3].lifecycle,
        AssetLifecycleV16::Active
    );
    assert_eq!(
        after_create_group.insurance_domain_budget[6], 0,
        "new attacker-controlled domain must not inherit shared insurance"
    );
    assert_eq!(
        after_create_group.insurance_domain_budget[7], 0,
        "new attacker-controlled domain must not inherit shared insurance"
    );
    assert_eq!(
        after_create_vault, 1_501,
        "only the permissionless init fee should enter the shared vault"
    );

    assert!(
        env.try_withdraw_insurance_domain_with_authority(&attacker, 6, 840)
            .is_err(),
        "attacker must not withdraw victim-funded insurance through domain 6"
    );
    assert!(
        env.try_withdraw_insurance_domain_with_authority(&attacker, 7, 660)
            .is_err(),
        "attacker must not withdraw victim-funded insurance through domain 7"
    );

    let (_, final_group) = env.market_state();
    assert_eq!(env.token_amount(env.vault), after_create_vault);
    assert_eq!(final_group.insurance, 1_501);
    assert_eq!(final_group.vault, 1_501);
    assert_eq!(final_group.insurance_domain_budget[0], 250);
    assert_eq!(final_group.insurance_domain_budget[1], 251);
    assert_eq!(final_group.insurance_domain_budget[2], 500);
    assert_eq!(final_group.insurance_domain_budget[4], 500);
    assert_eq!(final_group.insurance_domain_budget[6], 0);
    assert_eq!(final_group.insurance_domain_budget[7], 0);
}

#[test]
fn v16_bpf_permissionless_append_activation_uses_authenticated_slot() {
    let mut env = V16CuEnv::new();
    let attacker = Keypair::new();
    env.update_market_init_fee_policy_with_cu(1);
    env.svm.warp_to_slot(100);

    let (_fee_source, _cu) = env.activate_permissionless_asset_with_fee(
        &attacker,
        1,
        u64::MAX,
        100,
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        1,
    );

    let (_, group) = env.market_state();
    assert_eq!(
        group.current_slot, 100,
        "permissionless append activation must authenticate now_slot against Clock"
    );
    assert_eq!(group.assets[1].slot_last, 100);

    let cranker = Keypair::new();
    let cranker_portfolio = env.create_portfolio(&cranker);
    env.crank(
        cranker_portfolio,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 100,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
}

#[test]
fn v16_bpf_permissionless_reuse_activation_uses_authenticated_slot() {
    let mut env = V16CuEnv::new();
    let attacker = Keypair::new();
    env.update_market_init_fee_policy_with_cu(1);

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

    env.svm.warp_to_slot(4);
    env.activate_permissionless_asset_with_fee(
        &attacker,
        1,
        u64::MAX,
        250,
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        1,
    );

    let (_, group) = env.market_state();
    assert_eq!(
        group.current_slot, 4,
        "permissionless reuse activation must authenticate now_slot against Clock"
    );
    assert_eq!(group.assets[1].slot_last, 4);

    let cranker = Keypair::new();
    let cranker_portfolio = env.create_portfolio(&cranker);
    env.crank(
        cranker_portfolio,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 4,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
}

#[test]
fn v16_bpf_privileged_retire_uses_authenticated_slot() {
    let mut env = V16CuEnv::new();
    env.activate_asset(1, 1, 100);
    env.svm.warp_to_slot(3);

    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_RETIRE,
        1,
        u64::MAX,
        0,
    );

    let (_, group) = env.market_state();
    assert_eq!(
        group.current_slot, 3,
        "privileged retire must authenticate now_slot against Clock"
    );
    assert_eq!(group.assets[1].retired_slot, 3);

    let cranker = Keypair::new();
    let cranker_portfolio = env.create_portfolio(&cranker);
    env.crank(
        cranker_portfolio,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 3,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
}

#[test]
fn v16_bpf_privileged_reactivate_uses_authenticated_slot() {
    let mut env = V16CuEnv::new();
    env.activate_asset(1, 1, 100);
    env.svm.warp_to_slot(3);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_RETIRE,
        1,
        3,
        0,
    );

    env.svm.warp_to_slot(4);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_ACTIVATE,
        1,
        u64::MAX,
        250,
    );

    let (_, group) = env.market_state();
    assert_eq!(
        group.current_slot, 4,
        "privileged reactivation must authenticate now_slot against Clock"
    );
    assert_eq!(group.assets[1].slot_last, 4);

    let cranker = Keypair::new();
    let cranker_portfolio = env.create_portfolio(&cranker);
    env.crank(
        cranker_portfolio,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 4,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
}

#[test]
fn v16_bpf_permissionless_oracle_liquidation_uses_only_its_own_domain_insurance() {
    let mut env = V16CuEnv::new();
    let victim_insurance = Keypair::new();
    let attacker = Keypair::new();

    env.activate_asset_with_authorities(
        1,
        1,
        100,
        victim_insurance.pubkey(),
        victim_insurance.pubkey(),
        victim_insurance.pubkey(),
        victim_insurance.pubkey(),
    );
    env.activate_asset_with_authorities(
        2,
        2,
        100,
        victim_insurance.pubkey(),
        victim_insurance.pubkey(),
        victim_insurance.pubkey(),
        victim_insurance.pubkey(),
    );
    env.top_up_insurance(500);
    env.top_up_insurance_domain_with_authority(&victim_insurance, 2, 500);
    env.top_up_insurance_domain_with_authority(&victim_insurance, 4, 500);

    env.update_market_init_fee_policy_with_cu(1);
    env.svm.warp_to_slot(3);
    env.activate_permissionless_asset_with_fee(
        &attacker,
        3,
        3,
        100,
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        1,
    );
    env.svm.warp_to_slot(4);
    env.configure_auth_mark_for_asset_with_authority(3, &attacker, 4, 100);
    env.top_up_insurance_domain_with_authority(&attacker, 6, 300);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 200);
    env.trade_asset_with_cu(
        3,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        (2 * POS_SCALE) as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(5);
    env.push_auth_mark_for_asset_with_authority(3, &attacker, 5, 1_000);
    for now_slot in [5u64, 6] {
        env.svm.warp_to_slot(now_slot);
        env.crank(
            long_account,
            ProgInstruction::PermissionlessCrank {
                action: 0,
                asset_index: 3,
                now_slot,
                funding_rate_e9: 0,
                close_q: 0,
                fee_bps: 0,
                recovery_reason: 0,
            },
        );
    }
    let (_, before_liq) = env.market_state();
    assert_eq!(before_liq.insurance_domain_budget[0], 250);
    assert_eq!(before_liq.insurance_domain_budget[1], 251);
    assert_eq!(before_liq.insurance_domain_budget[2], 500);
    assert_eq!(before_liq.insurance_domain_budget[4], 500);
    assert_eq!(before_liq.insurance_domain_budget[6], 300);
    assert_eq!(before_liq.insurance_domain_spent[6], 0);
    assert_eq!(before_liq.insurance, 1_801);

    env.svm.warp_to_slot(7);
    let liq_cu = env.crank(
        short_account,
        ProgInstruction::PermissionlessCrank {
            action: 1,
            asset_index: 3,
            now_slot: 7,
            funding_rate_e9: 0,
            close_q: 2 * POS_SCALE,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
    println!("v16 permissionless malicious-oracle liquidation CU: {liq_cu}");

    let (_, after_liq) = env.market_state();
    let own_domain_spent = after_liq.insurance_domain_spent[6];
    assert!(
        own_domain_spent > 0,
        "malicious asset liquidation should consume its own funded domain"
    );
    assert_eq!(
        before_liq.insurance - after_liq.insurance,
        own_domain_spent,
        "aggregate insurance decrease must be exactly the attacker-domain spend"
    );
    assert_eq!(after_liq.insurance_domain_budget[0], 250);
    assert_eq!(after_liq.insurance_domain_budget[1], 251);
    assert_eq!(after_liq.insurance_domain_budget[2], 500);
    assert_eq!(after_liq.insurance_domain_budget[4], 500);
    assert_eq!(after_liq.insurance_domain_spent[0], 0);
    assert_eq!(after_liq.insurance_domain_spent[1], 0);
    assert_eq!(after_liq.insurance_domain_spent[2], 0);
    assert_eq!(after_liq.insurance_domain_spent[4], 0);
}

#[test]
fn v16_bpf_permissionless_market_shutdown_force_closes_recovers_and_reuses_slot() {
    let mut env = V16CuEnv::new();
    let attacker = Keypair::new();
    let cranker = Keypair::new();
    let insurance_authority = Keypair::new();
    let insurance_operator = Keypair::new();
    let backing_authority = Keypair::new();
    env.svm
        .airdrop(&insurance_operator.pubkey(), 1_000_000_000)
        .unwrap();
    env.configure_permissionless_resolve_with_cu(100, 5);
    env.update_market_init_fee_policy_with_cu(25);

    env.svm.warp_to_slot(1);
    let (init_fee_source, init_cu) = env.activate_permissionless_asset_with_fee(
        &attacker,
        1,
        1,
        100,
        insurance_authority.pubkey(),
        insurance_operator.pubkey(),
        backing_authority.pubkey(),
        env.admin.pubkey(),
        25,
    );
    println!("v16 permissionless asset create BPF CU: {init_cu}");
    assert_eq!(env.token_amount(init_fee_source), 0);
    assert_eq!(env.token_amount(env.vault), 25);
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (cfg_after_create, group_after_create) = state::read_market(&market_data).unwrap();
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

    env.top_up_insurance_domain_with_authority(&insurance_authority, 2, 6);
    env.top_up_insurance_domain_with_authority(&insurance_authority, 3, 4);
    env.top_up_backing_bucket_with_authority(&backing_authority, 2, 20, 20);
    env.top_up_backing_bucket_with_authority(&backing_authority, 3, 25, 20);
    assert_eq!(env.token_amount(env.vault), 80);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 10_000);
    env.deposit(&short_owner, short_account, 10_000);
    env.trade_asset_with_cu(
        1,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        (2 * POS_SCALE) as i128,
        100,
        0,
    );
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, opened_group) = state::read_market(&market_data).unwrap();
    assert_eq!(opened_group.assets[1].oi_eff_long_q, 2 * POS_SCALE);
    assert_eq!(opened_group.assets[1].oi_eff_short_q, 2 * POS_SCALE);
    assert_eq!(env.token_amount(env.vault), 20_080);

    env.svm.warp_to_slot(2);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
        1,
        2,
        0,
    );
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, shutdown_group) = state::read_market(&market_data).unwrap();
    let shutdown_profile = state::read_asset_oracle_profile(&market_data, 1).unwrap();
    assert_eq!(
        shutdown_group.assets[1].lifecycle,
        AssetLifecycleV16::Recovery
    );
    assert_eq!(shutdown_profile.last_good_oracle_slot, 2);
    assert_eq!(shutdown_group.assets[1].effective_price, 100);

    env.trade_asset_with_cu(
        1,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        -(POS_SCALE as i128),
        100,
        0,
    );
    let long_data = env.svm.get_account(&long_account).unwrap().data;
    let short_data = env.svm.get_account(&short_account).unwrap().data;
    let long_after_exit_window = state::read_portfolio(&long_data).unwrap();
    let short_after_exit_window = state::read_portfolio(&short_data).unwrap();
    assert_eq!(
        active_leg_for_asset(&long_after_exit_window, 1)
            .basis_pos_q
            .unsigned_abs(),
        POS_SCALE
    );
    assert_eq!(
        active_leg_for_asset(&short_after_exit_window, 1)
            .basis_pos_q
            .unsigned_abs(),
        POS_SCALE
    );

    env.svm.warp_to_slot(6);
    let before_timeout_market = env.svm.get_account(&env.market).unwrap().data;
    let before_timeout_long = env.svm.get_account(&long_account).unwrap().data;
    let before_timeout_short = env.svm.get_account(&short_account).unwrap().data;
    let too_early = env.try_force_close_abandoned_asset_with_cu(
        &cranker,
        long_account,
        short_account,
        1,
        6,
        POS_SCALE,
    );
    assert!(
        too_early.is_err(),
        "force-close must be rejected before the shutdown timeout"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before_timeout_market
    );
    assert_eq!(
        env.svm.get_account(&long_account).unwrap().data,
        before_timeout_long
    );
    assert_eq!(
        env.svm.get_account(&short_account).unwrap().data,
        before_timeout_short
    );

    env.svm.warp_to_slot(7);
    let force_close_cu = env.force_close_abandoned_asset_with_cu(
        &cranker,
        long_account,
        short_account,
        1,
        7,
        POS_SCALE,
    );
    println!("v16 abandoned asset force close BPF CU: {force_close_cu}");
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let long_data = env.svm.get_account(&long_account).unwrap().data;
    let short_data = env.svm.get_account(&short_account).unwrap().data;
    let (_, liquidated_group) = state::read_market(&market_data).unwrap();
    let long_closed = state::read_portfolio(&long_data).unwrap();
    let short_closed = state::read_portfolio(&short_data).unwrap();
    assert_eq!(liquidated_group.assets[1].oi_eff_long_q, 0);
    assert_eq!(liquidated_group.assets[1].oi_eff_short_q, 0);
    assert!(!has_active_leg_for_asset(&long_closed, 1));
    assert!(!has_active_leg_for_asset(&short_closed, 1));

    let admin_key = env.admin.pubkey();
    let admin_recovery = env.token_account(admin_key, 0);
    for (domain, amount) in [(2u8, 6u128), (3u8, 4u128)] {
        env.withdraw_insurance_domain_to_admin_token_with_cu(admin_recovery, domain, amount);
    }
    for (domain, amount) in [(2u8, 20u128), (3u8, 25u128)] {
        env.withdraw_backing_bucket_to_admin_token_with_cu(admin_recovery, domain, amount);
    }
    assert_eq!(
        env.token_amount(admin_recovery),
        55,
        "admin must recover asset-domain insurance and backing funds"
    );
    assert_eq!(env.token_amount(env.vault), 20_025);

    env.top_up_insurance_from_admin_token_with_cu(admin_recovery, 10);
    env.top_up_backing_bucket_from_admin_token_with_cu(admin_recovery, 0, 45, 20);
    assert_eq!(
        env.token_amount(admin_recovery),
        0,
        "recovered funds should be re-deposited into market-0 buckets"
    );
    assert_eq!(env.token_amount(env.vault), 20_080);
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, recovered_group) = state::read_market(&market_data).unwrap();
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

    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_RETIRE,
        1,
        7,
        0,
    );
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (retired_cfg, retired_group) = state::read_market(&market_data).unwrap();
    assert_eq!(retired_cfg.free_market_slot_count, 1);
    assert_eq!(
        retired_group.assets[1].lifecycle,
        AssetLifecycleV16::Retired
    );
    let reuse_market_id = retired_group.next_market_id;
    assert!(reuse_market_id > old_market_id);

    env.svm.warp_to_slot(8);
    let (reuse_source, reuse_cu) = env.activate_permissionless_asset_with_fee(
        &attacker,
        1,
        8,
        250,
        insurance_authority.pubkey(),
        insurance_operator.pubkey(),
        backing_authority.pubkey(),
        env.admin.pubkey(),
        25,
    );
    println!("v16 permissionless asset reuse BPF CU: {reuse_cu}");
    assert_eq!(env.token_amount(reuse_source), 0);
    assert_eq!(env.token_amount(env.vault), 20_105);
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (reused_cfg, reused_group) = state::read_market(&market_data).unwrap();
    assert_eq!(reused_cfg.free_market_slot_count, 0);
    assert_eq!(reused_group.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(reused_group.assets[1].market_id, reuse_market_id);
    assert!(reused_group.assets[1].market_id > old_market_id);
    assert_eq!(reused_group.assets[1].effective_price, 250);
}

#[test]
fn v16_bpf_tradenocpi_executes_and_is_bounded() {
    let mut env = V16CuEnv::new();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 1_000_000);

    let trade_cu = env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        (10 * POS_SCALE) as i128,
        150,
        100,
    );
    println!("v16 TradeNoCpi BPF CU: {trade_cu}");
    assert!(
        trade_cu <= TRADE_CU_LIMIT,
        "TradeNoCpi CU {} exceeded limit {}",
        trade_cu,
        TRADE_CU_LIMIT
    );

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let long_data = env.svm.get_account(&long_account).unwrap().data;
    let short_data = env.svm.get_account(&short_account).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    let long = state::read_portfolio(&long_data).unwrap();
    let short = state::read_portfolio(&short_data).unwrap();

    assert_eq!(env.token_amount(env.vault), 2_000_000);
    println!(
        "TradeNoCpi BPF long_basis={}, short_basis={}, insurance={}",
        long.legs[0].basis_pos_q, short.legs[0].basis_pos_q, group.insurance
    );
    assert_eq!(long.legs[0].basis_pos_q, (10 * POS_SCALE) as i128);
    assert_eq!(short.legs[0].basis_pos_q, -((10 * POS_SCALE) as i128));
    assert_eq!(
        group.assets[0].effective_price, 100,
        "consented execution price must not move the effective oracle price"
    );
    assert_eq!(
        group.insurance, 30,
        "notional=1500 and 100 bps charges 15 to each side"
    );
    assert_eq!(group.vault, 2_000_000);
    assert_eq!(group.c_tot + group.insurance, group.vault);
}

#[test]
fn v16_bpf_tradenocpi_rejects_invalid_final_market_shape() {
    let mut env = V16CuEnv::new();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 1_000_000);

    env.mutate_market(|_, group| {
        group.insurance_domain_budget[0] = group.insurance.saturating_add(1);
    });
    let before_market = env.svm.get_account(&env.market).unwrap().data;

    let result = env.try_trade_asset_with_cu(
        0,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        100,
        0,
    );

    assert!(
        result.is_err(),
        "TradeNoCpi must reject instead of persisting an invalid market shape"
    );
    let after_market = env.svm.get_account(&env.market).unwrap().data;
    assert_eq!(
        after_market, before_market,
        "failed TradeNoCpi must roll back market data"
    );
}

#[test]
fn v16_bpf_tradenocpi_fresh_open_on_base_and_added_asset_is_bounded() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(4, 1_000, 1_000, 500);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000_000);
    env.deposit(&short_owner, short_account, 1_000_000_000);

    let asset0_cu = env.trade_asset_with_cu(
        0,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        (10 * POS_SCALE) as i128,
        100,
        0,
    );
    println!("v16 TradeNoCpi fresh open asset[0] CU: {asset0_cu}");
    assert!(
        asset0_cu <= MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
        "fresh asset[0] TradeNoCpi CU {} exceeded limit {}",
        asset0_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT
    );

    let asset3_cu = env.trade_asset_with_cu(
        3,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        (10 * POS_SCALE) as i128,
        100,
        0,
    );
    println!("v16 TradeNoCpi fresh open asset[3] CU: {asset3_cu}");
    assert!(
        asset3_cu <= MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
        "fresh asset[3] TradeNoCpi CU {} exceeded limit {}",
        asset3_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT
    );

    let long_data = env.svm.get_account(&long_account).unwrap().data;
    let short_data = env.svm.get_account(&short_account).unwrap().data;
    let long = state::read_portfolio(&long_data).unwrap();
    let short = state::read_portfolio(&short_data).unwrap();
    assert_eq!(
        active_leg_for_asset(&long, 0).basis_pos_q,
        (10 * POS_SCALE) as i128
    );
    assert_eq!(
        active_leg_for_asset(&short, 0).basis_pos_q,
        -((10 * POS_SCALE) as i128)
    );
    assert_eq!(
        active_leg_for_asset(&long, 3).basis_pos_q,
        (10 * POS_SCALE) as i128
    );
    assert_eq!(
        active_leg_for_asset(&short, 3).basis_pos_q,
        -((10 * POS_SCALE) as i128)
    );
}

#[test]
fn v16_bpf_perps_positive_smoke_cross_margin_pnl_convert_close_and_withdraw() {
    const INITIAL_PRICE: u64 = 100;
    const ASSET0_MARK: u64 = 105;
    const ASSET1_MARK: u64 = 100;
    const DEPOSIT: u128 = 2_000_000;
    const EXPECTED_PNL: i128 = 5;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(4, 1_000, 1_000, 500);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, INITIAL_PRICE);
    env.configure_auth_mark_for_asset_as_admin(1, 1, INITIAL_PRICE);

    let cross_owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let cross_account = env.create_portfolio(&cross_owner);
    let counterparty_account = env.create_portfolio(&counterparty_owner);
    env.deposit(&cross_owner, cross_account, DEPOSIT);
    env.deposit(&counterparty_owner, counterparty_account, DEPOSIT);

    let open_asset0_cu = env.trade_asset_with_cu(
        0,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        POS_SCALE as i128,
        INITIAL_PRICE,
        0,
    );
    assert_cu_within("perps smoke open asset[0]", open_asset0_cu, TRADE_CU_LIMIT);
    let open_asset1_cu = env.trade_asset_with_cu(
        1,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        -(POS_SCALE as i128),
        INITIAL_PRICE,
        0,
    );
    assert_cu_within("perps smoke open asset[1]", open_asset1_cu, TRADE_CU_LIMIT);

    let cross_open = env.portfolio_state(cross_account);
    assert_eq!(
        percolator::active_bitmap_count_ones(cross_open.active_bitmap),
        2
    );
    assert_eq!(
        active_leg_for_asset(&cross_open, 0).basis_pos_q,
        POS_SCALE as i128
    );
    assert_eq!(
        active_leg_for_asset(&cross_open, 1).basis_pos_q,
        -(POS_SCALE as i128)
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, ASSET0_MARK);
    env.push_auth_mark_for_asset_as_admin(1, 2, ASSET1_MARK);

    for (portfolio, asset_index, label) in [
        (
            counterparty_account,
            0,
            "counterparty asset[0] loss refresh",
        ),
        (cross_account, 0, "cross account asset[0] gain refresh"),
        (
            counterparty_account,
            1,
            "counterparty asset[1] loss refresh",
        ),
        (cross_account, 1, "cross account asset[1] gain refresh"),
    ] {
        let cu = env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                action: 0,
                asset_index,
                now_slot: 2,
                funding_rate_e9: 0,
                close_q: 0,
                fee_bps: 0,
                recovery_reason: 0,
            },
        );
        assert_cu_within(label, cu, CRANK_CU_LIMIT);
    }

    let cross_after_refresh = env.portfolio_state(cross_account);
    let counterparty_after_refresh = env.portfolio_state(counterparty_account);
    assert_eq!(
        cross_after_refresh.pnl, EXPECTED_PNL,
        "cross-margin account should realize +5 while carrying two active legs"
    );
    assert_eq!(counterparty_after_refresh.pnl, 0);
    assert_eq!(
        counterparty_after_refresh.capital,
        DEPOSIT - EXPECTED_PNL as u128
    );

    let close_asset0_cu = env.trade_asset_with_cu(
        0,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        -(POS_SCALE as i128),
        ASSET0_MARK,
        0,
    );
    assert_cu_within(
        "perps smoke close asset[0]",
        close_asset0_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );
    let close_asset1_cu = env.trade_asset_with_cu(
        1,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        POS_SCALE as i128,
        ASSET1_MARK,
        0,
    );
    assert_cu_within(
        "perps smoke close asset[1]",
        close_asset1_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );

    let cross_flat = env.portfolio_state(cross_account);
    let counterparty_flat = env.portfolio_state(counterparty_account);
    assert!(percolator::active_bitmap_is_empty(cross_flat.active_bitmap));
    assert!(percolator::active_bitmap_is_empty(
        counterparty_flat.active_bitmap
    ));
    assert_eq!(cross_flat.pnl, EXPECTED_PNL);
    assert_eq!(cross_flat.capital, DEPOSIT);
    assert_eq!(counterparty_flat.capital, DEPOSIT - EXPECTED_PNL as u128);

    let convert_cu =
        env.convert_released_pnl_with_cu(&cross_owner, cross_account, EXPECTED_PNL as u128);
    assert_cu_within(
        "perps smoke convert released pnl",
        convert_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );
    let cross_after_convert = env.portfolio_state(cross_account);
    assert_eq!(cross_after_convert.pnl, 0);
    assert_eq!(cross_after_convert.capital, DEPOSIT + EXPECTED_PNL as u128);

    let cross_dest = env.withdraw(&cross_owner, cross_account, cross_after_convert.capital);
    let counterparty_dest = env.withdraw(
        &counterparty_owner,
        counterparty_account,
        counterparty_flat.capital,
    );
    assert_eq!(
        env.token_amount(cross_dest) as u128,
        DEPOSIT + EXPECTED_PNL as u128
    );
    assert_eq!(
        env.token_amount(counterparty_dest) as u128,
        DEPOSIT - EXPECTED_PNL as u128
    );
    assert_eq!(env.token_amount(env.vault), 0);
    let (_, group) = env.market_state();
    assert_eq!(group.vault, 0);
    assert_eq!(group.c_tot, 0);
    assert_eq!(group.insurance, 0);
}

#[test]
fn v16_bpf_cross_margin_positive_pnl_allows_trading_negative_leg_before_convert() {
    const INITIAL_PRICE: u64 = 100;
    const ASSET0_MARK: u64 = 105;
    const ASSET1_MARK: u64 = 95;
    const ASSET0_SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const ASSET1_SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const DEPOSIT: u128 = 320;
    const EXPECTED_POSITIVE_PNL: i128 = 100;
    const EXPECTED_NET_PNL_AFTER_NEGATIVE_CLOSE: i128 = 50;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(4, 1_000, 1_000, 500);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, INITIAL_PRICE);
    env.configure_auth_mark_for_asset_as_admin(1, 1, INITIAL_PRICE);

    let cross_owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let cross_account = env.create_portfolio(&cross_owner);
    let counterparty_account = env.create_portfolio(&counterparty_owner);
    env.deposit(&cross_owner, cross_account, DEPOSIT);
    env.deposit(&counterparty_owner, counterparty_account, 1_000);

    env.trade_asset_with_cu(
        0,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        ASSET0_SIZE_Q,
        INITIAL_PRICE,
        0,
    );
    env.trade_asset_with_cu(
        1,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        ASSET1_SIZE_Q,
        INITIAL_PRICE,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, ASSET0_MARK);
    env.push_auth_mark_for_asset_as_admin(1, 2, ASSET1_MARK);

    for (portfolio, asset_index, label) in [
        (
            counterparty_account,
            0,
            "counterparty asset[0] loss refresh",
        ),
        (cross_account, 0, "cross account asset[0] gain refresh"),
        (
            counterparty_account,
            1,
            "counterparty asset[1] gain refresh",
        ),
    ] {
        let cu = env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                action: 0,
                asset_index,
                now_slot: 2,
                funding_rate_e9: 0,
                close_q: 0,
                fee_bps: 0,
                recovery_reason: 0,
            },
        );
        assert_cu_within(label, cu, CRANK_CU_LIMIT);
    }
    let (_, moved_group) = env.market_state();
    assert_eq!(moved_group.assets[0].effective_price, ASSET0_MARK);
    assert_eq!(moved_group.assets[1].effective_price, ASSET1_MARK);

    let cross_before_close = env.portfolio_state(cross_account);
    assert_eq!(cross_before_close.pnl, EXPECTED_POSITIVE_PNL);
    assert_eq!(cross_before_close.capital, DEPOSIT);
    assert_eq!(
        active_leg_for_asset(&cross_before_close, 1).basis_pos_q,
        ASSET1_SIZE_Q,
        "asset[1] is a long leg with negative mark-to-market at the moved price"
    );

    let close_negative_leg_cu = env.trade_asset_with_cu(
        1,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        -ASSET1_SIZE_Q,
        ASSET1_MARK,
        0,
    );
    assert_cu_within(
        "cross-margin close negative leg before pnl convert",
        close_negative_leg_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );

    let cross_after_close = env.portfolio_state(cross_account);
    assert!(
        has_active_leg_for_asset(&cross_after_close, 0),
        "positive-PnL leg should remain open"
    );
    assert!(
        !has_active_leg_for_asset(&cross_after_close, 1),
        "negative-PnL leg should close without converting positive PnL first"
    );
    assert_eq!(cross_after_close.capital, DEPOSIT);
    assert_eq!(
        cross_after_close.pnl, EXPECTED_NET_PNL_AFTER_NEGATIVE_CLOSE,
        "asset[1] loss should net against the existing source-backed positive PnL"
    );
}

#[derive(Clone, Copy, Debug)]
enum SourceCreditWatermarkTradePath {
    NoCpi,
    Cpi,
}

#[derive(Clone, Copy, Debug)]
enum SourceCreditWatermarkDirection {
    PositiveSize,
    NegativeSize,
}

#[allow(clippy::too_many_arguments)]
fn try_source_credit_watermark_trade(
    env: &mut V16CuEnv,
    path: SourceCreditWatermarkTradePath,
    matcher_program: Option<Pubkey>,
    owner_a: &Keypair,
    account_a: Pubkey,
    owner_b: &Keypair,
    account_b: Pubkey,
    asset_index: u16,
    size_q: i128,
    exec_price: u64,
    fee_bps: u64,
) -> Result<u64, String> {
    match path {
        SourceCreditWatermarkTradePath::NoCpi => env.try_trade_asset_with_cu(
            asset_index,
            owner_a,
            account_a,
            owner_b,
            account_b,
            size_q,
            exec_price,
            fee_bps,
        ),
        SourceCreditWatermarkTradePath::Cpi => {
            let matcher_program = matcher_program.expect("matcher program");
            let (matcher_ctx, matcher_delegate, _) =
                env.init_matcher_context(matcher_program, account_b);
            env.try_trade_cpi_with_cu_on_asset(
                owner_a,
                account_a,
                owner_b,
                account_b,
                matcher_program,
                matcher_ctx,
                matcher_delegate,
                asset_index,
                size_q,
                fee_bps,
            )
        }
    }
}

fn run_source_credit_watermark_trade_case(
    path: SourceCreditWatermarkTradePath,
    direction: SourceCreditWatermarkDirection,
) {
    const INITIAL_PRICE: u64 = 100;
    const ASSET0_SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const ASSET1_SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const SAFE_INCREASE_Q: i128 = POS_SCALE as i128;
    const DEPOSIT: u128 = 313;
    const EXPECTED_POSITIVE_PNL: i128 = 100;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(4, 1_000, 1_000, 500);
    let matcher_program = match path {
        SourceCreditWatermarkTradePath::NoCpi => None,
        SourceCreditWatermarkTradePath::Cpi => {
            let matcher_program = Pubkey::new_unique();
            let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
            env.svm.add_program(matcher_program, &matcher_bytes);
            Some(matcher_program)
        }
    };
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, INITIAL_PRICE);
    env.configure_auth_mark_for_asset_as_admin(1, 1, INITIAL_PRICE);

    let cross_owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let cross_account = env.create_portfolio(&cross_owner);
    let counterparty_account = env.create_portfolio(&counterparty_owner);
    env.deposit(&cross_owner, cross_account, DEPOSIT);
    env.deposit(&counterparty_owner, counterparty_account, 1_000);

    let (winning_domain, asset0_mark, asset1_mark, side_sign) = match direction {
        SourceCreditWatermarkDirection::PositiveSize => (1usize, 105, 95, 1i128),
        SourceCreditWatermarkDirection::NegativeSize => (0usize, 95, 105, -1i128),
    };
    env.top_up_backing_bucket(winning_domain as u8, 150, 10);

    env.trade_asset_with_cu(
        0,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        side_sign * ASSET0_SIZE_Q,
        INITIAL_PRICE,
        0,
    );
    env.trade_asset_with_cu(
        1,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        side_sign * ASSET1_SIZE_Q,
        INITIAL_PRICE,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, asset0_mark);
    env.push_auth_mark_for_asset_as_admin(1, 2, asset1_mark);
    for (portfolio, asset_index) in [
        (counterparty_account, 0),
        (cross_account, 0),
        (counterparty_account, 1),
    ] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                action: 0,
                asset_index,
                now_slot: 2,
                funding_rate_e9: 0,
                close_q: 0,
                fee_bps: 0,
                recovery_reason: 0,
            },
        );
    }
    let forced_capital = match direction {
        SourceCreditWatermarkDirection::PositiveSize => 260,
        SourceCreditWatermarkDirection::NegativeSize => 100,
    };
    env.force_portfolio_capital_for_benchmark(cross_account, forced_capital);

    let cross_before = env.portfolio_state(cross_account);
    assert_eq!(
        cross_before.pnl, EXPECTED_POSITIVE_PNL,
        "{path:?} {direction:?} setup must create source-backed positive PnL"
    );
    let (_, before_withdraw_group) = env.market_state();
    assert_eq!(
        before_withdraw_group.source_credit[winning_domain].positive_claim_bound_num,
        EXPECTED_POSITIVE_PNL as u128 * BOUND_SCALE
    );
    let surplus_backing = before_withdraw_group.source_credit[winning_domain]
        .fresh_reserved_backing_num
        .checked_sub(before_withdraw_group.source_credit[winning_domain].positive_claim_bound_num)
        .unwrap()
        / BOUND_SCALE;
    assert!(
        surplus_backing > 0,
        "{path:?} {direction:?} setup must leave withdrawable surplus backing"
    );

    let watermark_withdraw_dest = env.token_account(env.admin.pubkey(), 0);
    env.withdraw_backing_bucket_to_admin_token_with_cu(
        watermark_withdraw_dest,
        winning_domain as u8,
        surplus_backing,
    );
    let (_, exact_watermark_group) = env.market_state();
    assert_eq!(
        exact_watermark_group.source_credit[winning_domain].fresh_reserved_backing_num,
        exact_watermark_group.source_credit[winning_domain].positive_claim_bound_num,
        "{path:?} {direction:?} setup must leave no surplus source-credit backing"
    );

    let before_market = env.svm.get_account(&env.market).unwrap();
    let before_cross = env.svm.get_account(&cross_account).unwrap();
    let before_counterparty = env.svm.get_account(&counterparty_account).unwrap();
    let over_watermark = try_source_credit_watermark_trade(
        &mut env,
        path,
        matcher_program,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        1,
        side_sign * SAFE_INCREASE_Q,
        asset1_mark,
        0,
    );
    assert!(
        over_watermark.is_err(),
        "{path:?} {direction:?} risk increase must reject at the exact source-credit watermark"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before_market.data
    );
    assert_eq!(
        env.svm.get_account(&cross_account).unwrap().data,
        before_cross.data
    );
    assert_eq!(
        env.svm.get_account(&counterparty_account).unwrap().data,
        before_counterparty.data
    );

    env.top_up_backing_bucket(winning_domain as u8, 5_000, 10);
    let second_pass_deposit = match direction {
        SourceCreditWatermarkDirection::PositiveSize => 50,
        SourceCreditWatermarkDirection::NegativeSize => 200,
    };
    env.deposit(&cross_owner, cross_account, second_pass_deposit);
    env.deposit(
        &counterparty_owner,
        counterparty_account,
        second_pass_deposit,
    );
    env.svm.warp_to_slot(3);
    let inside_watermark = try_source_credit_watermark_trade(
        &mut env,
        path,
        matcher_program,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        1,
        side_sign * SAFE_INCREASE_Q,
        asset1_mark,
        1,
    );
    assert!(
        inside_watermark.is_ok(),
        "{path:?} {direction:?} risk increase inside the source-credit watermark failed: {inside_watermark:?}"
    );

    let (_, after_group) = env.market_state();
    let cross_after = env.portfolio_state(cross_account);
    assert_eq!(
        after_group.source_credit[winning_domain].credit_rate_num,
        percolator::CREDIT_RATE_SCALE,
        "{path:?} {direction:?} must not dilute live positive claims"
    );
    assert!(
        cross_after.source_lien_effective_reserved[winning_domain] > 0,
        "{path:?} {direction:?} must reserve source credit once surplus backing exists"
    );
}

#[test]
fn v16_bpf_trade_paths_respect_source_credit_watermark_permutations() {
    for path in [
        SourceCreditWatermarkTradePath::NoCpi,
        SourceCreditWatermarkTradePath::Cpi,
    ] {
        for direction in [
            SourceCreditWatermarkDirection::PositiveSize,
            SourceCreditWatermarkDirection::NegativeSize,
        ] {
            run_source_credit_watermark_trade_case(path, direction);
        }
    }
}

#[test]
fn v16_bpf_cross_margin_positive_pnl_allows_backed_risk_increase_on_negative_leg() {
    const INITIAL_PRICE: u64 = 100;
    const ASSET0_MARK: u64 = 105;
    const ASSET1_MARK: u64 = 95;
    const ASSET0_SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const ASSET1_SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const SAFE_INCREASE_Q: i128 = POS_SCALE as i128;
    const TOO_LARGE_INCREASE_Q: i128 = 30 * POS_SCALE as i128;
    const DEPOSIT: u128 = 313;
    const EXPECTED_POSITIVE_PNL: i128 = 100;
    const EXPECTED_NET_PNL_AFTER_REFRESH: i128 = 50;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(4, 1_000, 1_000, 500);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, INITIAL_PRICE);
    env.configure_auth_mark_for_asset_as_admin(1, 1, INITIAL_PRICE);

    let cross_owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let cross_account = env.create_portfolio(&cross_owner);
    let counterparty_account = env.create_portfolio(&counterparty_owner);
    env.deposit(&cross_owner, cross_account, DEPOSIT);
    env.deposit(&counterparty_owner, counterparty_account, 1_000);
    env.top_up_backing_bucket(1, 150, 10);

    env.trade_asset_with_cu(
        0,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        ASSET0_SIZE_Q,
        INITIAL_PRICE,
        0,
    );
    env.trade_asset_with_cu(
        1,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        ASSET1_SIZE_Q,
        INITIAL_PRICE,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, ASSET0_MARK);
    env.push_auth_mark_for_asset_as_admin(1, 2, ASSET1_MARK);
    for (portfolio, asset_index) in [
        (counterparty_account, 0),
        (cross_account, 0),
        (counterparty_account, 1),
    ] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                action: 0,
                asset_index,
                now_slot: 2,
                funding_rate_e9: 0,
                close_q: 0,
                fee_bps: 0,
                recovery_reason: 0,
            },
        );
    }

    let cross_before = env.portfolio_state(cross_account);
    assert_eq!(cross_before.pnl, EXPECTED_POSITIVE_PNL);
    assert_eq!(cross_before.capital, DEPOSIT);
    assert_eq!(
        active_leg_for_asset(&cross_before, 1).basis_pos_q,
        ASSET1_SIZE_Q,
        "asset[1] is a losing long leg before the risk-increasing trade"
    );
    let (_, before_watermark_group) = env.market_state();
    let fresh_reserved_before_withdraw =
        before_watermark_group.source_credit[1].fresh_reserved_backing_num;
    let positive_claim_before_withdraw =
        before_watermark_group.source_credit[1].positive_claim_bound_num;

    let watermark_withdraw_dest = env.token_account(env.admin.pubkey(), 0);
    let withdraw_cu =
        env.withdraw_backing_bucket_to_admin_token_with_cu(watermark_withdraw_dest, 1, 50);
    assert_cu_within(
        "WithdrawBackingBucket live watermark",
        withdraw_cu,
        CUSTODY_CU_LIMIT,
    );
    let (_, watermarked_group) = env.market_state();
    assert_eq!(
        watermarked_group.source_credit[1].fresh_reserved_backing_num,
        fresh_reserved_before_withdraw - 50 * BOUND_SCALE,
        "admin withdrawal lowers the future encumbrance watermark"
    );
    assert!(
        watermarked_group.source_credit[1].fresh_reserved_backing_num
            >= positive_claim_before_withdraw,
        "the lowered watermark must still cover live positive-claim demand"
    );
    assert_eq!(
        watermarked_group.source_credit[1].credit_rate_num,
        percolator::CREDIT_RATE_SCALE,
        "lowering the watermark must not dilute already-live positive claims"
    );

    let before_market = env.svm.get_account(&env.market).unwrap();
    let before_cross = env.svm.get_account(&cross_account).unwrap();
    let before_counterparty = env.svm.get_account(&counterparty_account).unwrap();
    let too_large = env.try_trade_asset_with_cu(
        1,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        TOO_LARGE_INCREASE_Q,
        ASSET1_MARK,
        0,
    );
    assert!(
        too_large.is_err(),
        "risk increase must stay capped by realizable source-backed positive PnL"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before_market.data
    );
    assert_eq!(
        env.svm.get_account(&cross_account).unwrap().data,
        before_cross.data
    );
    assert_eq!(
        env.svm.get_account(&counterparty_account).unwrap().data,
        before_counterparty.data
    );

    let increase_cu = env.trade_asset_with_cu(
        1,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        SAFE_INCREASE_Q,
        ASSET1_MARK,
        0,
    );
    assert_cu_within(
        "cross-margin increase negative leg with backed positive pnl",
        increase_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );

    let cross_after = env.portfolio_state(cross_account);
    assert_eq!(
        active_leg_for_asset(&cross_after, 1).basis_pos_q,
        ASSET1_SIZE_Q + SAFE_INCREASE_Q
    );
    assert_eq!(cross_after.capital, DEPOSIT);
    assert_eq!(cross_after.pnl, EXPECTED_NET_PNL_AFTER_REFRESH);
    assert!(
        cross_after.capital < cross_after.health_cert.certified_initial_req,
        "without positive PnL credit this risk increase would fail initial margin"
    );
    assert!(
        cross_after.health_cert.certified_equity as u128
            >= cross_after.health_cert.certified_initial_req
    );
    let source_lien_effective_reserved: u128 = cross_after
        .source_lien_effective_reserved
        .iter()
        .copied()
        .sum();
    assert!(
        source_lien_effective_reserved > 0,
        "risk-increasing trade must reserve backed source-credit support for IM"
    );
    assert!(
        cross_after
            .source_lien_counterparty_backing_num
            .iter()
            .any(|amount| *amount != 0)
            || cross_after
                .source_lien_insurance_backing_num
                .iter()
                .any(|amount| *amount != 0),
        "source-credit IM lien must be backed by counterparty backing or reserved insurance"
    );

    let (_, after_increase_group) = env.market_state();
    let after_increase_source = after_increase_group.source_credit[1];
    let after_increase_bucket = after_increase_group.source_backing_buckets[1];
    let insurance_encumbered_num = after_increase_source
        .valid_liened_insurance_num
        .checked_add(after_increase_source.impaired_liened_insurance_num)
        .unwrap();
    let available_backing_num = after_increase_source
        .fresh_reserved_backing_num
        .checked_sub(after_increase_source.valid_liened_backing_num)
        .unwrap()
        .checked_add(
            after_increase_source
                .insurance_credit_reserved_num
                .checked_sub(insurance_encumbered_num)
                .unwrap(),
        )
        .unwrap();
    let max_lossless_withdrawable_num = after_increase_bucket
        .fresh_unliened_backing_num
        .min(available_backing_num - after_increase_source.positive_claim_bound_num);
    let over_watermark_amount = max_lossless_withdrawable_num / BOUND_SCALE + 1;
    assert!(
        over_watermark_amount > 0,
        "test must attempt a withdrawal above the live backing watermark"
    );

    let backing_withdraw_dest = env.token_account(env.admin.pubkey(), 0);
    let market_before_withdraw = env.svm.get_account(&env.market).unwrap();
    let backing_withdraw = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawBackingBucket {
            domain: 1,
            amount: over_watermark_amount,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(backing_withdraw_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&env.admin],
    );
    assert!(
        backing_withdraw.is_err(),
        "withdrawal above the live backing watermark must not be allowed"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        market_before_withdraw.data
    );
}

#[test]
fn v16_bpf_permissionless_crank_computes_funding_from_internal_mark_premium() {
    const INITIAL_PRICE: u64 = 1_000_000;
    const DEPOSIT: u128 = 10_000_000;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: INITIAL_PRICE,
        max_price_move_bps_per_slot: 1_000,
        max_accrual_dt_slots: 1,
        max_abs_funding_e9_per_slot: 1_000,
        min_funding_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(0);
    env.configure_ewma_mark_with_cu(0, INITIAL_PRICE, 1, 0);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, DEPOSIT);
    env.deposit(&short_owner, short_account, DEPOSIT);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        INITIAL_PRICE,
        0,
    );

    env.svm.warp_to_slot(1);
    env.push_ewma_mark_with_cu(1, INITIAL_PRICE * 2);
    env.crank(
        long_account,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 1,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
    env.crank(
        short_account,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 1,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
    let (cfg_after_first, group_after_first) = env.market_state();
    assert_eq!(cfg_after_first.mark_ewma_e6, 1_500_000);
    assert_eq!(group_after_first.assets[0].effective_price, 1_100_000);
    assert_eq!(
        group_after_first.funding_epoch, 0,
        "a newly pushed mark must not retroactively charge funding before its slot"
    );
    assert_eq!(group_after_first.assets[0].f_long_num, 0);
    assert_eq!(group_after_first.assets[0].f_short_num, 0);

    env.svm.warp_to_slot(2);
    let funding_cu = env.crank(
        long_account,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 2,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
    assert_cu_within(
        "permissionless computed funding crank",
        funding_cu,
        CRANK_CU_LIMIT,
    );
    let (_, funded_group) = env.market_state();
    assert_eq!(funded_group.funding_epoch, 1);
    assert_eq!(funded_group.assets[0].effective_price, 1_210_000);
    assert_eq!(funded_group.assets[0].f_long_num, -(ADL_ONE as i128));
    assert_eq!(funded_group.assets[0].f_short_num, ADL_ONE as i128);
}

#[test]
fn v16_bpf_existing_funding_ledger_refreshes_and_converts_between_sides() {
    const INITIAL_PRICE: u64 = 1_000_000;
    const FUNDING_RATE_E9: i128 = 1_000;
    const DEPOSIT: u128 = 2_000_000;

    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, DEPOSIT);
    env.deposit(&short_owner, short_account, DEPOSIT);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        INITIAL_PRICE,
        0,
    );

    env.mutate_market(|_, group| {
        let out = group
            .accrue_asset_to_not_atomic(0, 1, INITIAL_PRICE, FUNDING_RATE_E9, true)
            .unwrap();
        assert!(out.funding_active);
        group.assets[0].raw_oracle_target_price = INITIAL_PRICE;
    });
    env.svm.warp_to_slot(1);
    let (_, funded_group) = env.market_state();
    assert_eq!(funded_group.funding_epoch, 1);
    assert_eq!(funded_group.assets[0].f_long_num, -(ADL_ONE as i128));
    assert_eq!(funded_group.assets[0].f_short_num, ADL_ONE as i128);

    let long_refresh_cu = env.crank(
        long_account,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 1,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
    assert_cu_within(
        "funding smoke long loss refresh",
        long_refresh_cu,
        CRANK_CU_LIMIT,
    );
    let long_after = env.portfolio_state(long_account);
    assert_eq!(long_after.pnl, 0);
    assert_eq!(long_after.capital, DEPOSIT - 1);

    let short_refresh_cu = env.crank(
        short_account,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 1,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
    assert_cu_within(
        "funding smoke short gain refresh",
        short_refresh_cu,
        CRANK_CU_LIMIT,
    );
    let short_after = env.portfolio_state(short_account);
    assert_eq!(short_after.pnl, 1);
    assert_eq!(short_after.capital, DEPOSIT);

    let close_cu = env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        -(POS_SCALE as i128),
        INITIAL_PRICE,
        0,
    );
    assert_cu_within(
        "funding smoke close funded position",
        close_cu,
        TRADE_CU_LIMIT,
    );
    let long_flat = env.portfolio_state(long_account);
    let short_flat = env.portfolio_state(short_account);
    assert!(percolator::active_bitmap_is_empty(long_flat.active_bitmap));
    assert!(percolator::active_bitmap_is_empty(short_flat.active_bitmap));
    assert_eq!(long_flat.capital, DEPOSIT - 1);
    assert_eq!(short_flat.pnl, 1);

    let convert_cu = env.convert_released_pnl_with_cu(&short_owner, short_account, 1);
    assert_cu_within(
        "funding smoke convert released pnl",
        convert_cu,
        CUSTODY_CU_LIMIT,
    );
    let short_after_convert = env.portfolio_state(short_account);
    assert_eq!(short_after_convert.pnl, 0);
    assert_eq!(short_after_convert.capital, DEPOSIT + 1);

    let (_, group) = env.market_state();
    assert_eq!(group.c_tot, DEPOSIT * 2);
    assert_eq!(group.vault, DEPOSIT * 2);
}

#[test]
fn v16_bpf_stale_asset_does_not_block_current_unrelated_trade() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(4, 1_000, 1_000, 500);

    let stale_long_owner = Keypair::new();
    let stale_short_owner = Keypair::new();
    let stale_long_account = env.create_portfolio(&stale_long_owner);
    let stale_short_account = env.create_portfolio(&stale_short_owner);
    env.deposit(&stale_long_owner, stale_long_account, 1_000_000_000);
    env.deposit(&stale_short_owner, stale_short_account, 1_000_000_000);
    env.trade_asset_with_cu(
        1,
        &stale_long_owner,
        stale_long_account,
        &stale_short_owner,
        stale_short_account,
        (10 * POS_SCALE) as i128,
        100,
        0,
    );

    let cranker_owner = Keypair::new();
    let cranker_portfolio = env.create_portfolio(&cranker_owner);
    env.svm.warp_to_slot(3);

    for nonce in 0..3 {
        env.crank(
            cranker_portfolio,
            ProgInstruction::PermissionlessCrank {
                action: 0,
                asset_index: 0,
                now_slot: 3 + nonce,
                funding_rate_e9: 0,
                close_q: 0,
                fee_bps: 0,
                recovery_reason: 0,
            },
        );
    }

    env.crank(
        cranker_portfolio,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 1,
            now_slot: 3,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );

    let (_, group) = env.market_state();
    assert_eq!(group.current_slot, 3);
    assert_eq!(group.assets[0].slot_last, 3);
    assert!(group.assets[1].slot_last < group.current_slot);
    assert!(
        group.loss_stale_active,
        "asset[1] partial catch-up must leave the market loss-stale bit set"
    );

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000_000);
    env.deposit(&short_owner, short_account, 1_000_000_000);

    let trade_cu = env.trade_asset_with_cu(
        0,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        (10 * POS_SCALE) as i128,
        100,
        0,
    );
    println!("v16 TradeNoCpi current asset[0] with stale asset[1] CU: {trade_cu}");
    assert_cu_within(
        "TradeNoCpi current asset[0] with unrelated stale asset[1]",
        trade_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );

    let (_, group_after) = env.market_state();
    assert_eq!(group_after.assets[0].slot_last, 3);
    assert!(group_after.assets[1].slot_last < group_after.current_slot);
    assert!(
        group_after.loss_stale_active,
        "unrelated trade must not hide the stale asset state"
    );

    let long = env.portfolio_state(long_account);
    let short = env.portfolio_state(short_account);
    assert!(has_active_leg_for_asset(&long, 0));
    assert!(has_active_leg_for_asset(&short, 0));
    assert!(!has_active_leg_for_asset(&long, 1));
    assert!(!has_active_leg_for_asset(&short, 1));
}

#[test]
fn v16_bpf_sync_maintenance_fee_with_cranker_share_is_bounded() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 10_000, 10_000, 10_000, 58,
    );
    let payer_owner = Keypair::new();
    let cranker_owner = Keypair::new();
    let payer_portfolio = env.create_portfolio(&payer_owner);
    let cranker_portfolio = env.create_portfolio(&cranker_owner);
    env.deposit(&payer_owner, payer_portfolio, 100_000_000);
    env.update_maintenance_fee_policy_with_cu(4_000);

    env.svm.warp_to_slot(10);
    let sync_cu = env.sync_maintenance_fee_with_cu(payer_portfolio, Some(cranker_portfolio), 10);
    println!("v16 SyncMaintenanceFee 3-account cranker-share CU: {sync_cu}");
    assert!(
        sync_cu <= CUSTODY_CU_LIMIT,
        "3-account SyncMaintenanceFee CU {} exceeded limit {}",
        sync_cu,
        CUSTODY_CU_LIMIT
    );

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let payer_data = env.svm.get_account(&payer_portfolio).unwrap().data;
    let cranker_data = env.svm.get_account(&cranker_portfolio).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    let payer = state::read_portfolio(&payer_data).unwrap();
    let cranker = state::read_portfolio(&cranker_data).unwrap();
    assert_eq!(payer.last_fee_slot, 10);
    assert_eq!(payer.capital, 100_000_000 - 580);
    assert_eq!(cranker.capital, 232);
    assert_eq!(group.insurance, 348);
}

#[test]
fn v16_bpf_underfunded_flat_sync_sweeps_remaining_capital_once() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 10_000, 10_000, 10_000, 40,
    );
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_portfolio = env.create_portfolio(&long_owner);
    let short_portfolio = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_portfolio, 1);
    env.deposit(&short_owner, short_portfolio, 10_000);

    env.svm.warp_to_slot(10);
    let market_lamports_before_close = env.svm.get_account(&env.market).unwrap().lamports;
    let long_lamports_before_close = env.svm.get_account(&long_portfolio).unwrap().lamports;
    env.sync_maintenance_fee_with_cu(long_portfolio, None, 10);
    let (_, group_after_flat_sync) = env.market_state();
    assert_eq!(
        group_after_flat_sync.insurance, 1,
        "underfunded flat sync sweeps the remaining capital into insurance"
    );
    assert_eq!(group_after_flat_sync.materialized_portfolio_count, 1);
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().lamports,
        market_lamports_before_close + long_lamports_before_close,
        "dust-closed portfolio rent should move into the market slab"
    );
    if let Some(closed_long_account) = env.svm.get_account(&long_portfolio) {
        assert_eq!(closed_long_account.lamports, 0);
        assert!(
            closed_long_account.data.is_empty()
                || !state::is_initialized(&closed_long_account.data)
        );
    }

    let fresh_long_portfolio = env.create_portfolio(&long_owner);
    env.deposit(&long_owner, fresh_long_portfolio, 1_000);
    env.trade_with_cu(
        &long_owner,
        fresh_long_portfolio,
        &short_owner,
        short_portfolio,
        POS_SCALE as i128,
        100,
        0,
    );
    env.crank(
        fresh_long_portfolio,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 10,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
    let (_, before_nonflat_sync) = env.market_state();
    assert_eq!(before_nonflat_sync.assets[0].slot_last, 1);
    let insurance_before_nonflat_sync = before_nonflat_sync.insurance;

    let fresh_long_lamports_before_sync =
        env.svm.get_account(&fresh_long_portfolio).unwrap().lamports;
    env.sync_maintenance_fee_with_cu(fresh_long_portfolio, None, 11);
    let (_, group_after_nonflat_sync) = env.market_state();
    let long_after_nonflat_sync = env.portfolio_state(fresh_long_portfolio);
    assert_eq!(long_after_nonflat_sync.capital, 1_000);
    assert_eq!(long_after_nonflat_sync.last_fee_slot, 10);
    assert_eq!(
        env.svm
            .get_account(&fresh_long_portfolio)
            .expect("non-flat portfolio should remain allocated")
            .lamports,
        fresh_long_lamports_before_sync
    );
    assert_eq!(
        group_after_nonflat_sync.insurance, insurance_before_nonflat_sync,
        "later deposits are not charged for an already-swept empty interval"
    );
}

#[test]
fn v16_bpf_nonflat_fee_sync_settles_hidden_loss_before_sweeping_fee() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 10_000, 10_000, 10_000, 100,
    );
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_portfolio = env.create_portfolio(&long_owner);
    let short_portfolio = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_portfolio, 100);
    env.deposit(&short_owner, short_portfolio, 1_000);
    env.trade_with_cu(
        &long_owner,
        long_portfolio,
        &short_owner,
        short_portfolio,
        POS_SCALE as i128,
        100,
        0,
    );

    let long_before_move = env.portfolio_state(long_portfolio);
    assert_eq!(long_before_move.capital, 100);
    assert_eq!(long_before_move.pnl, 0);

    env.mutate_market(|_, group| {
        group.accrue_asset_to_not_atomic(0, 1, 50, 0, true).unwrap();
        group.assets[0].raw_oracle_target_price = 50;
    });
    env.svm.warp_to_slot(1);

    let long_with_hidden_loss = env.portfolio_state(long_portfolio);
    assert_eq!(
        long_with_hidden_loss.capital, 100,
        "the price move should be hidden until the account is touched"
    );
    assert_eq!(long_with_hidden_loss.pnl, 0);
    let (_, group_with_hidden_loss) = env.market_state();
    assert_eq!(group_with_hidden_loss.insurance, 0);
    assert_eq!(group_with_hidden_loss.c_tot, 1_100);

    let sync_cu = env.sync_maintenance_fee_with_cu(long_portfolio, None, 1);
    println!("v16 SyncMaintenanceFee nonflat hidden-loss CU: {sync_cu}");
    assert_cu_within(
        "SyncMaintenanceFee nonflat hidden-loss regression",
        sync_cu,
        CUSTODY_CU_LIMIT,
    );

    let long_after_sync = env.portfolio_state(long_portfolio);
    let (_, group_after_sync) = env.market_state();
    assert_eq!(long_after_sync.capital, 0);
    assert_eq!(long_after_sync.pnl, 0);
    assert_eq!(long_after_sync.last_fee_slot, 1);
    assert_eq!(
        group_after_sync.insurance, 50,
        "only capital remaining after the hidden loss is settled can be swept as fee"
    );
    assert_eq!(group_after_sync.c_tot, 1_000);
}

#[test]
fn v16_bpf_fee_sync_rejects_reused_market_slot_stale_leg_without_mutation() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 10_000, 10_000, 10_000, 1,
    );
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_portfolio = env.create_portfolio(&long_owner);
    let short_portfolio = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_portfolio, 1_000);
    env.deposit(&short_owner, short_portfolio, 1_000);
    env.trade_with_cu(
        &long_owner,
        long_portfolio,
        &short_owner,
        short_portfolio,
        POS_SCALE as i128,
        100,
        0,
    );

    let old_market_id = env.market_state().1.assets[0].market_id;
    env.mutate_market(|_, group| {
        group
            .accrue_asset_to_not_atomic(0, 1, 100, 0, true)
            .unwrap();
        group.assets[0].market_id = old_market_id + 1;
        group.next_market_id = group.next_market_id.max(old_market_id + 2);
    });
    env.svm.warp_to_slot(1);

    let market_before = env.svm.get_account(&env.market).unwrap().data;
    let long_before = env.svm.get_account(&long_portfolio).unwrap().data;
    let err = env
        .try_sync_maintenance_fee_with_cu(long_portfolio, None, 1)
        .expect_err("stale market id leg must fail closed");
    println!("v16 SyncMaintenanceFee stale reused-market-id rejection: {err}");

    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        market_before,
        "failed sync must not mutate the reused market slot"
    );
    assert_eq!(
        env.svm.get_account(&long_portfolio).unwrap().data,
        long_before,
        "failed sync must not mutate the stale portfolio"
    );
}

#[test]
fn v16_bpf_close_portfolio_sweeps_rent_to_market_slab() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);
    env.withdraw(&owner, portfolio, 1_000);

    let market_lamports_before_close = env.svm.get_account(&env.market).unwrap().lamports;
    let portfolio_lamports_before_close = env.svm.get_account(&portfolio).unwrap().lamports;
    let close_cu = env.close_portfolio_with_cu(&owner, portfolio);
    assert_cu_within("close portfolio rent sweep", close_cu, CUSTODY_CU_LIMIT);

    let (_, group) = env.market_state();
    assert_eq!(group.materialized_portfolio_count, 0);
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().lamports,
        market_lamports_before_close + portfolio_lamports_before_close,
        "ClosePortfolio should move closed account rent into the market slab"
    );
    if let Some(closed_account) = env.svm.get_account(&portfolio) {
        assert_eq!(closed_account.lamports, 0);
        assert!(closed_account.data.is_empty() || !state::is_initialized(&closed_account.data));
    }
}

#[test]
fn v16_bpf_tradecpi_executes_through_external_matcher_and_is_bounded() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);

    let taker_owner = Keypair::new();
    let maker_owner = Keypair::new();
    let taker_account = env.create_portfolio(&taker_owner);
    let maker_account = env.create_portfolio(&maker_owner);
    env.deposit(&taker_owner, taker_account, 1_000_000);
    env.deposit(&maker_owner, maker_account, 1_000_000);

    let (matcher_ctx, matcher_delegate, init_matcher_cu) =
        env.init_matcher_context(matcher_program, maker_account);
    let trade_cpi_cu = env.trade_cpi_with_cu(
        &taker_owner,
        taker_account,
        &maker_owner,
        maker_account,
        matcher_program,
        matcher_ctx,
        matcher_delegate,
        (10 * POS_SCALE) as i128,
        100,
    );
    println!("v16 matcher init CU: {init_matcher_cu}, TradeCpi BPF CU: {trade_cpi_cu}");
    assert!(
        trade_cpi_cu <= TRADE_CU_LIMIT,
        "TradeCpi CU {} exceeded limit {}",
        trade_cpi_cu,
        TRADE_CU_LIMIT
    );

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let taker_data = env.svm.get_account(&taker_account).unwrap().data;
    let maker_data = env.svm.get_account(&maker_account).unwrap().data;
    let matcher_data = env.svm.get_account(&matcher_ctx).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    let taker = state::read_portfolio(&taker_data).unwrap();
    let maker = state::read_portfolio(&maker_data).unwrap();
    println!(
        "TradeCpi BPF taker_basis={}, maker_basis={}, insurance={}",
        taker.legs[0].basis_pos_q, maker.legs[0].basis_pos_q, group.insurance
    );
    assert_eq!(group.assets[0].effective_price, 100);
    assert_eq!(taker.legs[0].basis_pos_q, (10 * POS_SCALE) as i128);
    assert_eq!(maker.legs[0].basis_pos_q, -((10 * POS_SCALE) as i128));
    assert_eq!(
        group.insurance, 20,
        "passive matcher fills at oracle price; 100 bps charges 10 to each side"
    );
    assert_eq!(
        u32::from_le_bytes(matcher_data[0..4].try_into().unwrap()),
        MATCHER_ABI_VERSION,
        "LiteSVM matcher path must use the same ABI version as the wrapper"
    );
    assert_eq!(
        u64::from_le_bytes(matcher_data[56..64].try_into().unwrap()),
        0,
        "matcher must echo the requested asset index in the v3 return slot"
    );
    assert_eq!(group.c_tot + group.insurance, group.vault);
}

#[test]
fn v16_bpf_tradecpi_external_matcher_executes_on_added_asset() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    env.activate_asset(1, 1, 100);
    env.activate_asset(2, 2, 250);

    let taker_owner = Keypair::new();
    let maker_owner = Keypair::new();
    let taker_account = env.create_portfolio(&taker_owner);
    let maker_account = env.create_portfolio(&maker_owner);
    env.deposit(&taker_owner, taker_account, 1_000_000);
    env.deposit(&maker_owner, maker_account, 1_000_000);

    let (matcher_ctx, matcher_delegate, _) =
        env.init_matcher_context(matcher_program, maker_account);
    let trade_cpi_cu = env.trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker_account,
        &maker_owner,
        maker_account,
        matcher_program,
        matcher_ctx,
        matcher_delegate,
        2,
        (10 * POS_SCALE) as i128,
        100,
    );
    println!("v16 TradeCpi BPF nonzero-asset CU: {trade_cpi_cu}");
    assert!(
        trade_cpi_cu <= TRADE_CU_LIMIT,
        "TradeCpi nonzero-asset CU {} exceeded limit {}",
        trade_cpi_cu,
        TRADE_CU_LIMIT
    );

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let taker_data = env.svm.get_account(&taker_account).unwrap().data;
    let maker_data = env.svm.get_account(&maker_account).unwrap().data;
    let matcher_data = env.svm.get_account(&matcher_ctx).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    let taker = state::read_portfolio(&taker_data).unwrap();
    let maker = state::read_portfolio(&maker_data).unwrap();

    assert_eq!(group.assets[0].oi_eff_long_q, 0);
    assert_eq!(group.assets[0].oi_eff_short_q, 0);
    assert_eq!(group.assets[2].effective_price, 250);
    assert_eq!(group.assets[2].oi_eff_long_q, 10 * POS_SCALE);
    assert_eq!(group.assets[2].oi_eff_short_q, 10 * POS_SCALE);
    assert_eq!(taker.active_bitmap, active_bitmap_with(&[0]));
    assert_eq!(maker.active_bitmap, active_bitmap_with(&[0]));
    assert_eq!(
        active_leg_for_asset(&taker, 2).basis_pos_q,
        (10 * POS_SCALE) as i128
    );
    assert_eq!(
        active_leg_for_asset(&maker, 2).basis_pos_q,
        -((10 * POS_SCALE) as i128)
    );
    assert_eq!(
        group.insurance, 50,
        "passive matcher fills asset 2 at 250; notional=2500 and 100 bps charges 25 to each side"
    );
    assert_eq!(
        u64::from_le_bytes(matcher_data[56..64].try_into().unwrap()),
        2,
        "external matcher must echo the requested nonzero asset index"
    );
    assert_eq!(group.c_tot + group.insurance, group.vault);
}

#[test]
fn v16_bpf_permissionless_liquidation_is_bounded() {
    let mut env = V16CuEnv::new();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 250);
    env.configure_ewma_mark_with_cu(0, 100, 1, 0);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(1);
    env.push_ewma_mark_with_cu(1, 300);
    let liquidation_cu = env.crank(
        short_account,
        ProgInstruction::PermissionlessCrank {
            action: 1,
            asset_index: 0,
            now_slot: 1,
            funding_rate_e9: 0,
            close_q: POS_SCALE,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
    println!("v16 liquidation crank CU: {liquidation_cu}");
    assert!(
        liquidation_cu <= CRANK_CU_LIMIT,
        "liquidation CU {} exceeded limit {}",
        liquidation_cu,
        CRANK_CU_LIMIT
    );

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let short_data = env.svm.get_account(&short_account).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    let short = state::read_portfolio(&short_data).unwrap();
    assert_eq!(group.slot_last, 1);
    assert_eq!(group.assets[0].effective_price, 200);
    assert!(percolator::active_bitmap_is_empty(short.active_bitmap));
}

#[test]
fn v16_bpf_tradenocpi_rejects_off_mark_recycle_when_deficit_cannot_settle() {
    let mut env = V16CuEnv::new();
    env.top_up_insurance(1_000_000);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, 100);

    let extractor_owner = Keypair::new();
    let probe_owner = Keypair::new();
    let extractor = env.create_portfolio(&extractor_owner);
    let probe = env.create_portfolio(&probe_owner);
    env.deposit(&extractor_owner, extractor, 10_000);
    env.deposit(&probe_owner, probe, 1_000);
    env.trade_with_cu(
        &extractor_owner,
        extractor,
        &probe_owner,
        probe,
        (10 * POS_SCALE) as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, 300);
    env.crank(
        probe,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 2,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
    env.svm.warp_to_slot(3);
    env.crank(
        probe,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 3,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
    let before_market = env.svm.get_account(&env.market).unwrap();
    let before_extractor = env.svm.get_account(&extractor).unwrap();
    let before_probe = env.svm.get_account(&probe).unwrap();
    let (_, before_group) = state::read_market(&before_market.data).unwrap();
    assert_eq!(before_group.insurance, 1_000_000);
    let before_probe_state = state::read_portfolio(&before_probe.data).unwrap();
    assert!(
        before_probe_state.health_cert.certified_liq_deficit != 0,
        "probe must be liquidatable before the attempted recycling trade"
    );

    let close = env.try_trade_asset_with_cu(
        0,
        &extractor_owner,
        extractor,
        &probe_owner,
        probe,
        -((10 * POS_SCALE) as i128),
        500,
        0,
    );
    assert!(
        close.is_err(),
        "liquidatable probe must not recycle an unsettled deficit through an off-mark close"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before_market.data
    );
    assert_eq!(
        env.svm.get_account(&extractor).unwrap().data,
        before_extractor.data
    );
    assert_eq!(env.svm.get_account(&probe).unwrap().data, before_probe.data);
}

#[test]
fn v16_bpf_tradecpi_rejects_off_mark_recycle_when_deficit_cannot_settle() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    env.top_up_insurance(1_000_000);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, 100);

    let extractor_owner = Keypair::new();
    let probe_owner = Keypair::new();
    let extractor = env.create_portfolio(&extractor_owner);
    let probe = env.create_portfolio(&probe_owner);
    env.deposit(&extractor_owner, extractor, 10_000);
    env.deposit(&probe_owner, probe, 1_000);
    env.trade_with_cu(
        &extractor_owner,
        extractor,
        &probe_owner,
        probe,
        (10 * POS_SCALE) as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, 300);
    env.crank(
        probe,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 2,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
    env.svm.warp_to_slot(3);
    env.crank(
        probe,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 3,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
    let (matcher_ctx, matcher_delegate, _) =
        env.init_matcher_context_with_passive_spread(matcher_program, extractor, 9_000, 9_000);
    let before_market = env.svm.get_account(&env.market).unwrap();
    let before_extractor = env.svm.get_account(&extractor).unwrap();
    let before_probe = env.svm.get_account(&probe).unwrap();
    let before_matcher = env.svm.get_account(&matcher_ctx).unwrap();
    let before_probe_state = state::read_portfolio(&before_probe.data).unwrap();
    assert!(
        before_probe_state.health_cert.certified_liq_deficit != 0,
        "probe must be liquidatable before the attempted matcher recycling trade"
    );

    let close = env.try_trade_cpi_with_cu_on_asset(
        &probe_owner,
        probe,
        &extractor_owner,
        extractor,
        matcher_program,
        matcher_ctx,
        matcher_delegate,
        0,
        (10 * POS_SCALE) as i128,
        0,
    );
    assert!(
        close.is_err(),
        "liquidatable probe must not recycle an unsettled deficit through an off-mark matcher fill"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before_market.data
    );
    assert_eq!(
        env.svm.get_account(&extractor).unwrap().data,
        before_extractor.data
    );
    assert_eq!(env.svm.get_account(&probe).unwrap().data, before_probe.data);
    assert_eq!(
        env.svm.get_account(&matcher_ctx).unwrap().data,
        before_matcher.data
    );
}

#[test]
fn v16_bpf_tradenocpi_rejects_when_counterparty_starts_bankrupt() {
    let mut env = V16CuEnv::new();
    env.top_up_insurance(1_000_000);

    let extractor_owner = Keypair::new();
    let probe_owner = Keypair::new();
    let extractor = env.create_portfolio(&extractor_owner);
    let probe = env.create_portfolio(&probe_owner);
    env.deposit(&extractor_owner, extractor, 10_000);
    env.deposit(&probe_owner, probe, 2_000);
    env.trade_with_cu(
        &extractor_owner,
        extractor,
        &probe_owner,
        probe,
        (10 * POS_SCALE) as i128,
        100,
        0,
    );
    env.force_portfolio_bankruptcy_for_security_test(probe, 500);
    env.crank(
        probe,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 1,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );

    let before_market = env.svm.get_account(&env.market).unwrap();
    let before_extractor = env.svm.get_account(&extractor).unwrap();
    let before_probe = env.svm.get_account(&probe).unwrap();
    let before_probe_state = state::read_portfolio(&before_probe.data).unwrap();
    assert_eq!(before_probe_state.capital, 0);
    assert!(
        before_probe_state.pnl < 0,
        "probe must start bankrupt before the attempted recycling trade"
    );
    assert!(before_probe_state.health_cert.valid);
    assert!(
        before_probe_state.health_cert.certified_equity < 0,
        "refreshed probe certificate must confirm negative equity before the attempted trade"
    );

    let close = env.try_trade_asset_with_cu(
        0,
        &extractor_owner,
        extractor,
        &probe_owner,
        probe,
        -((10 * POS_SCALE) as i128),
        500,
        0,
    );
    assert!(
        close.is_err(),
        "bankrupt probe must not use an off-mark close as a normal bilateral trade"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before_market.data
    );
    assert_eq!(
        env.svm.get_account(&extractor).unwrap().data,
        before_extractor.data
    );
    assert_eq!(env.svm.get_account(&probe).unwrap().data, before_probe.data);
}

#[test]
fn v16_bpf_tradecpi_rejects_when_counterparty_starts_bankrupt() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    env.top_up_insurance(1_000_000);

    let extractor_owner = Keypair::new();
    let probe_owner = Keypair::new();
    let extractor = env.create_portfolio(&extractor_owner);
    let probe = env.create_portfolio(&probe_owner);
    env.deposit(&extractor_owner, extractor, 10_000);
    env.deposit(&probe_owner, probe, 2_000);
    env.trade_with_cu(
        &extractor_owner,
        extractor,
        &probe_owner,
        probe,
        (10 * POS_SCALE) as i128,
        100,
        0,
    );
    env.force_portfolio_bankruptcy_for_security_test(probe, 500);
    env.crank(
        probe,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 1,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
    let (matcher_ctx, matcher_delegate, _) =
        env.init_matcher_context_with_passive_spread(matcher_program, extractor, 9_000, 9_000);

    let before_market = env.svm.get_account(&env.market).unwrap();
    let before_extractor = env.svm.get_account(&extractor).unwrap();
    let before_probe = env.svm.get_account(&probe).unwrap();
    let before_matcher = env.svm.get_account(&matcher_ctx).unwrap();
    let before_probe_state = state::read_portfolio(&before_probe.data).unwrap();
    assert_eq!(before_probe_state.capital, 0);
    assert!(
        before_probe_state.pnl < 0,
        "probe must start bankrupt before the attempted matcher recycling trade"
    );
    assert!(before_probe_state.health_cert.valid);
    assert!(
        before_probe_state.health_cert.certified_equity < 0,
        "refreshed probe certificate must confirm negative equity before the attempted matcher trade"
    );

    let close = env.try_trade_cpi_with_cu_on_asset(
        &probe_owner,
        probe,
        &extractor_owner,
        extractor,
        matcher_program,
        matcher_ctx,
        matcher_delegate,
        0,
        (10 * POS_SCALE) as i128,
        0,
    );
    assert!(
        close.is_err(),
        "bankrupt probe must not use an off-mark matcher fill as a normal bilateral trade"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before_market.data
    );
    assert_eq!(
        env.svm.get_account(&extractor).unwrap().data,
        before_extractor.data
    );
    assert_eq!(env.svm.get_account(&probe).unwrap().data, before_probe.data);
    assert_eq!(
        env.svm.get_account(&matcher_ctx).unwrap().data,
        before_matcher.data
    );
}

#[test]
fn v16_bpf_tradenocpi_rejects_when_both_counterparties_start_bankrupt() {
    let mut env = V16CuEnv::new();
    env.top_up_insurance(1_000_000);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 2_000);
    env.deposit(&short_owner, short_account, 2_000);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        (10 * POS_SCALE) as i128,
        100,
        0,
    );
    env.force_portfolio_bankruptcy_for_security_test(long_account, 500);
    env.force_portfolio_bankruptcy_for_security_test(short_account, 500);
    env.crank(
        long_account,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 1,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
    env.crank(
        short_account,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 1,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );

    let before_market = env.svm.get_account(&env.market).unwrap();
    let before_long = env.svm.get_account(&long_account).unwrap();
    let before_short = env.svm.get_account(&short_account).unwrap();
    let before_long_state = state::read_portfolio(&before_long.data).unwrap();
    let before_short_state = state::read_portfolio(&before_short.data).unwrap();
    assert!(before_long_state.health_cert.valid);
    assert!(before_short_state.health_cert.valid);
    assert!(before_long_state.health_cert.certified_equity < 0);
    assert!(before_short_state.health_cert.certified_equity < 0);

    let close = env.try_trade_asset_with_cu(
        0,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        -((10 * POS_SCALE) as i128),
        100,
        0,
    );
    assert!(
        close.is_err(),
        "two bankrupt counterparties must not use TradeNoCpi as a bankruptcy close"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before_market.data
    );
    assert_eq!(
        env.svm.get_account(&long_account).unwrap().data,
        before_long.data
    );
    assert_eq!(
        env.svm.get_account(&short_account).unwrap().data,
        before_short.data
    );
}

#[test]
fn v16_bpf_tradenocpi_allows_both_counterparties_with_capitalized_losses_to_risk_reduce() {
    let mut env = V16CuEnv::new();
    env.top_up_insurance(1_000_000);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 10_000);
    env.deposit(&short_owner, short_account, 10_000);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        (10 * POS_SCALE) as i128,
        100,
        0,
    );
    env.force_portfolio_loss_for_security_test(long_account, 500);
    env.force_portfolio_loss_for_security_test(short_account, 500);

    let (_, before_group) = env.market_state();
    let before_long = env.portfolio_state(long_account);
    let before_short = env.portfolio_state(short_account);
    assert!(before_long.pnl < 0 && before_long.capital > before_long.pnl.unsigned_abs());
    assert!(before_short.pnl < 0 && before_short.capital > before_short.pnl.unsigned_abs());

    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        -((10 * POS_SCALE) as i128),
        100,
        0,
    );

    let (_, after_group) = env.market_state();
    let after_long = env.portfolio_state(long_account);
    let after_short = env.portfolio_state(short_account);
    assert_eq!(
        after_group.insurance, before_group.insurance,
        "capitalized losses must settle from account capital, not insurance"
    );
    assert_eq!(after_long.pnl, 0);
    assert_eq!(after_short.pnl, 0);
    assert!(!has_active_leg_for_asset(&after_long, 0));
    assert!(!has_active_leg_for_asset(&after_short, 0));
}

#[test]
fn v16_bpf_liquidatable_solvent_account_can_risk_reduce_without_insurance_drain() {
    let mut env = V16CuEnv::new();
    env.top_up_insurance(1_000_000);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, 100);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 10_000);
    env.deposit(&short_owner, short_account, 3_000);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        (10 * POS_SCALE) as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, 300);
    env.crank(
        short_account,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 2,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
    env.svm.warp_to_slot(3);
    env.crank(
        short_account,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 3,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );

    let (_, before_group) = env.market_state();
    let before_short = env.portfolio_state(short_account);
    assert_eq!(before_group.insurance, 1_000_000);
    assert!(
        before_short.health_cert.certified_liq_deficit != 0,
        "short account should be liquidatable before the safe risk reduction"
    );
    assert!(
        before_short.health_cert.certified_equity > 0,
        "short account should still be solvent before the safe risk reduction"
    );

    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        -((10 * POS_SCALE) as i128),
        500,
        0,
    );

    let (_, after_group) = env.market_state();
    let after_long = env.portfolio_state(long_account);
    let after_short = env.portfolio_state(short_account);
    assert_eq!(
        after_group.insurance, before_group.insurance,
        "safe risk reduction must not consume or credit insurance"
    );
    assert_eq!(
        after_group.c_tot + after_group.insurance + after_group.pnl_pos_tot,
        after_group.vault
    );
    assert!(!has_active_leg_for_asset(&after_long, 0));
    assert!(!has_active_leg_for_asset(&after_short, 0));
    assert!(
        after_long.health_cert.certified_liq_deficit == 0
            && after_short.health_cert.certified_liq_deficit == 0,
        "both accounts must be non-liquidatable after the risk reduction"
    );
}

#[test]
fn v16_bpf_no_cranker_liquidation_rejects_invalid_final_market_shape() {
    let mut env = V16CuEnv::new();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 250);
    env.configure_ewma_mark_with_cu(0, 100, 1, 0);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(1);
    env.push_ewma_mark_with_cu(1, 300);
    env.mutate_market(|_, group| {
        group.insurance_domain_budget[0] = group.insurance.saturating_add(1);
    });
    let before_market = env.svm.get_account(&env.market).unwrap().data;
    let before_short = env.svm.get_account(&short_account).unwrap().data;

    let result = env.send(
        ProgInstruction::PermissionlessCrank {
            action: 1,
            asset_index: 0,
            now_slot: 1,
            funding_rate_e9: 0,
            close_q: POS_SCALE,
            fee_bps: 0,
            recovery_reason: 0,
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short_account, false),
        ],
        &[],
    );

    assert!(
        result.is_err(),
        "no-cranker liquidation must reject instead of persisting an invalid market shape"
    );
    let after_market = env.svm.get_account(&env.market).unwrap().data;
    let after_short = env.svm.get_account(&short_account).unwrap().data;
    assert_eq!(
        after_market, before_market,
        "failed no-cranker liquidation must roll back market data"
    );
    assert_eq!(
        after_short, before_short,
        "failed no-cranker liquidation must roll back portfolio data"
    );
}

#[test]
fn v16_bpf_cranker_reward_liquidation_rejects_invalid_shape_without_paying_reward() {
    let mut env = V16CuEnv::new();
    env.update_liquidation_fee_policy_with_cu(10_000);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let cranker_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    let cranker_account = env.create_portfolio(&cranker_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 250);
    env.configure_ewma_mark_with_cu(0, 100, 1, 0);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(1);
    env.push_ewma_mark_with_cu(1, 300);
    env.mutate_market(|_, group| {
        group.config.liquidation_fee_bps = 10_000;
        group.config.liquidation_fee_cap = 1;
        group.insurance_domain_budget[0] = group.insurance.saturating_add(1_000_000);
    });
    let before_market = env.svm.get_account(&env.market).unwrap().data;
    let before_short = env.svm.get_account(&short_account).unwrap().data;
    let before_cranker = env.svm.get_account(&cranker_account).unwrap().data;

    let result = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::PermissionlessCrank {
            action: 1,
            asset_index: 0,
            now_slot: 1,
            funding_rate_e9: 0,
            close_q: POS_SCALE,
            fee_bps: 0,
            recovery_reason: 0,
        },
        vec![
            AccountMeta::new(cranker_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short_account, false),
            AccountMeta::new(cranker_account, false),
        ],
        &[&cranker_owner],
    );

    assert!(
        result.is_err(),
        "cranker-reward liquidation must reject instead of persisting an invalid market shape"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before_market,
        "failed cranker-reward liquidation must roll back market data"
    );
    assert_eq!(
        env.svm.get_account(&short_account).unwrap().data,
        before_short,
        "failed cranker-reward liquidation must roll back liquidated portfolio data"
    );
    assert_eq!(
        env.svm.get_account(&cranker_account).unwrap().data,
        before_cranker,
        "failed cranker-reward liquidation must not pay the cranker portfolio"
    );
}

#[test]
fn v16_bpf_full_14_leg_refresh_crank_is_under_tx_limit() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(14, 1_000, 1_000, 500);
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 2_000);
    env.deposit(&short_owner, short_account, 100_000);
    env.seed_n_leg_position_for_benchmark(long_account, short_account, 14);
    let before_slot_last = {
        let market_data = env.svm.get_account(&env.market).unwrap().data;
        let (_, group) = state::read_market(&market_data).unwrap();
        group.assets[0].slot_last
    };

    env.svm.warp_to_slot(16);
    let refresh_cu = env.crank(
        long_account,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 16,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
    println!("v16 full-14-leg refresh crank CU: {refresh_cu}");
    assert!(
        refresh_cu <= 900_000,
        "full-14-leg refresh CU {} exceeded limit {}",
        refresh_cu,
        900_000
    );

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let long_data = env.svm.get_account(&long_account).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    let long = state::read_portfolio(&long_data).unwrap();
    assert_eq!(group.config.max_portfolio_assets, 14);
    assert_eq!(percolator::active_bitmap_count_ones(long.active_bitmap), 14);
    assert!(
        group.assets[0].slot_last > before_slot_last,
        "full-14 refresh crank must commit bounded asset progress"
    );
    assert_eq!(group.assets[0].effective_price, 95);
}

#[test]
fn v16_bpf_full_14_leg_liquidation_crank_is_under_tx_limit() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(14, 1_000, 1_000, 500);
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 2_000);
    env.deposit(&short_owner, short_account, 100_000);
    env.seed_n_leg_position_for_benchmark(long_account, short_account, 14);
    env.force_portfolio_capital_for_benchmark(long_account, 1_000);

    env.svm.warp_to_slot(16);
    let liquidation_cu = env.crank(
        long_account,
        ProgInstruction::PermissionlessCrank {
            action: 1,
            asset_index: 0,
            now_slot: 16,
            funding_rate_e9: 0,
            close_q: 10 * POS_SCALE,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
    println!("v16 full-14-leg liquidation crank CU: {liquidation_cu}");
    const FULL_14_LEG_LIQUIDATION_CU_LIMIT: u64 = 1_375_000;
    assert!(
        liquidation_cu <= FULL_14_LEG_LIQUIDATION_CU_LIMIT,
        "full-14-leg liquidation CU {} exceeded limit {}",
        liquidation_cu,
        FULL_14_LEG_LIQUIDATION_CU_LIMIT
    );

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let long_data = env.svm.get_account(&long_account).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    let long = state::read_portfolio(&long_data).unwrap();
    assert_eq!(group.config.max_portfolio_assets, 14);
    assert_eq!(percolator::active_bitmap_count_ones(long.active_bitmap), 13);
    assert!(!long.legs[0].active);
    assert_eq!(group.assets[0].oi_eff_long_q, 0);
    assert_eq!(group.assets[0].oi_eff_short_q, 0);
}

#[test]
fn v16_bpf_current_full_14_leg_tradenocpi_is_under_tx_limit() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(14, 1_000, 1_000, 500);
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 20_000);
    env.deposit(&short_owner, short_account, 100_000);
    env.seed_current_n_leg_position_for_benchmark(long_account, short_account, 14);
    let trade_cu = env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        -(POS_SCALE as i128),
        100,
        0,
    );
    println!("v16 current full-14-leg TradeNoCpi CU: {trade_cu}");
    assert!(
        trade_cu <= 1_150_000,
        "current full-14-leg TradeNoCpi CU {} exceeded limit {}",
        trade_cu,
        1_150_000
    );

    let long_data = env.svm.get_account(&long_account).unwrap().data;
    let short_data = env.svm.get_account(&short_account).unwrap().data;
    let long = state::read_portfolio(&long_data).unwrap();
    let short = state::read_portfolio(&short_data).unwrap();
    assert_eq!(percolator::active_bitmap_count_ones(long.active_bitmap), 14);
    assert_eq!(
        percolator::active_bitmap_count_ones(short.active_bitmap),
        14
    );
    assert_eq!(long.legs[0].basis_pos_q, (9 * POS_SCALE) as i128);
    assert_eq!(short.legs[0].basis_pos_q, -((9 * POS_SCALE) as i128));
}

#[test]
fn v16_bpf_stale_full_14_leg_tradenocpi_is_under_tx_limit() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(14, 1_000, 1_000, 500);
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 20_000);
    env.deposit(&short_owner, short_account, 100_000);
    env.seed_n_leg_position_for_benchmark(long_account, short_account, 14);
    env.svm.warp_to_slot(16);
    let trade_cu = env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        -(POS_SCALE as i128),
        95,
        0,
    );
    println!("v16 stale full-14-leg TradeNoCpi CU: {trade_cu}");
    assert!(
        trade_cu <= 1_400_000,
        "stale full-14-leg TradeNoCpi CU {} exceeded limit {}",
        trade_cu,
        1_400_000
    );

    let long_data = env.svm.get_account(&long_account).unwrap().data;
    let short_data = env.svm.get_account(&short_account).unwrap().data;
    let long = state::read_portfolio(&long_data).unwrap();
    let short = state::read_portfolio(&short_data).unwrap();
    assert_eq!(percolator::active_bitmap_count_ones(long.active_bitmap), 14);
    assert_eq!(
        percolator::active_bitmap_count_ones(short.active_bitmap),
        14
    );
    assert_eq!(long.legs[0].basis_pos_q, (9 * POS_SCALE) as i128);
    assert_eq!(short.legs[0].basis_pos_q, -((9 * POS_SCALE) as i128));
}

#[test]
fn v16_bpf_close_resolved_moves_payout_tokens_with_ledger() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);

    env.resolve();
    let dest = env.close_resolved(&owner, portfolio);
    assert_eq!(env.token_amount(dest), 1_000);
    assert_eq!(env.token_amount(env.vault), 0);

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let portfolio_data = env.svm.get_account(&portfolio).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    let account = state::read_portfolio(&portfolio_data).unwrap();
    assert_eq!(group.vault, 0);
    assert_eq!(group.c_tot, 0);
    assert_eq!(account.capital, 0);
}

#[test]
fn v16_bpf_close_resolved_pays_positive_pnl_through_engine_ledger() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);
    env.top_up_backing_bucket(1, 250, 10);
    env.add_source_positive_pnl(portfolio, 1, 250);

    env.resolve();
    let dest = env.close_resolved(&owner, portfolio);
    assert_eq!(env.token_amount(dest), 1_250);
    assert_eq!(env.token_amount(env.vault), 0);

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let portfolio_data = env.svm.get_account(&portfolio).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    let account = state::read_portfolio(&portfolio_data).unwrap();
    assert_eq!(group.vault, 0);
    assert_eq!(group.c_tot, 0);
    assert_eq!(account.capital, 0);
    assert_eq!(account.pnl, 0);
    assert!(account.resolved_payout_receipt.present);
    assert!(account.resolved_payout_receipt.finalized);
}

#[test]
fn v16_bpf_permissionless_stale_resolve_is_bounded_and_oracle_free() {
    let mut env = V16CuEnv::new();
    let configure_cu = env.configure_permissionless_resolve_with_cu(5, 1);
    let stale_resolve_cu = env.resolve_stale_permissionless_with_cu(5);
    println!(
        "v16 permissionless stale resolve CU configure={configure_cu}, resolve={stale_resolve_cu}"
    );
    assert!(
        configure_cu <= CUSTODY_CU_LIMIT,
        "configure permissionless resolve CU {} exceeded limit {}",
        configure_cu,
        CUSTODY_CU_LIMIT
    );
    assert!(
        stale_resolve_cu <= CUSTODY_CU_LIMIT,
        "permissionless stale resolve CU {} exceeded limit {}",
        stale_resolve_cu,
        CUSTODY_CU_LIMIT
    );

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (cfg, group) = state::read_market(&market_data).unwrap();
    assert_eq!(cfg.permissionless_resolve_stale_slots, 5);
    assert_eq!(cfg.force_close_delay_slots, 1);
    assert_eq!(group.mode, percolator::MarketModeV16::Resolved);
    assert_eq!(group.resolved_slot, 5);
}

#[test]
fn v16_cu_custody_and_resolution_paths_are_bounded() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let (portfolio, init_portfolio_cu) = env.create_portfolio_with_cu(&owner);
    let (_source, deposit_cu) = env.deposit_with_cu(&owner, portfolio, 1_000);
    let (_dest, withdraw_cu) = env.withdraw_with_cu(&owner, portfolio, 400);
    let (_insurance_source, top_up_cu) = env.top_up_insurance_with_cu(250);
    env.enable_live_insurance_withdrawal();
    let (_insurance_dest, withdraw_insurance_cu) = env.withdraw_insurance_with_cu(100);
    let resolve_cu = env.resolve();
    let (_resolved_dest, close_resolved_cu) = env.close_resolved_with_cu(&owner, portfolio);

    println!(
        "v16 custody CU init_portfolio={init_portfolio_cu}, deposit={deposit_cu}, withdraw={withdraw_cu}, top_up={top_up_cu}, withdraw_insurance={withdraw_insurance_cu}, resolve={resolve_cu}, close_resolved={close_resolved_cu}"
    );
    for (name, cu) in [
        ("init_portfolio", init_portfolio_cu),
        ("deposit", deposit_cu),
        ("withdraw", withdraw_cu),
        ("top_up", top_up_cu),
        ("withdraw_insurance", withdraw_insurance_cu),
        ("resolve", resolve_cu),
        ("close_resolved", close_resolved_cu),
    ] {
        assert!(
            cu <= CUSTODY_CU_LIMIT,
            "{} CU {} exceeded limit {}",
            name,
            cu,
            CUSTODY_CU_LIMIT
        );
    }
}

#[test]
fn v16_cu_permissionless_crank_refresh_is_bounded() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000_000);

    let refresh_cu = env.crank(
        portfolio,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 1,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
    println!("v16 refresh crank CU: {refresh_cu}");
    assert!(refresh_cu <= CRANK_CU_LIMIT);
}

#[test]
fn v16_bpf_permissionless_crank_uses_authenticated_clock_slot_not_caller_slot() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000_000);

    let real_slot = 10;
    let spoofed_slot = 1_000_000;
    env.svm.warp_to_slot(real_slot);
    env.crank(
        portfolio,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: spoofed_slot,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );

    let clock = env.svm.get_sysvar::<Clock>();
    assert_eq!(clock.slot, real_slot);
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    assert_eq!(
        group.current_slot, clock.slot,
        "permissionless crank must authenticate engine time from SVM Clock, not the instruction body"
    );
    assert_ne!(
        group.current_slot, spoofed_slot,
        "caller-supplied crank now_slot must not be able to move engine time into the future"
    );
}

#[test]
fn v16_bpf_configure_hybrid_oracle_uses_authenticated_clock_slot_not_caller_slot() {
    let mut env = V16CuEnv::new();
    let real_slot = 10;
    let spoofed_slot = 1_000_000;
    env.svm.warp_to_slot(real_slot);
    let mut clock = env.svm.get_sysvar::<Clock>();
    clock.unix_timestamp = 1_000;
    env.svm.set_sysvar(&clock);
    let clock = env.svm.get_sysvar::<Clock>();

    let feeds = [[0x91u8; 32], [0x92u8; 32], [0x93u8; 32]];
    let leg0 = env.set_pyth_price(&feeds[0], 4_000_000_000, -6, clock.unix_timestamp);
    let leg1 = env.set_pyth_price(&feeds[1], 150_000_000, -6, clock.unix_timestamp);
    let leg2 = env.set_pyth_price(&feeds[2], 200_000_000, -6, clock.unix_timestamp);
    env.configure_three_leg_hybrid_with_cu(
        feeds,
        leg0,
        leg1,
        leg2,
        spoofed_slot,
        clock.unix_timestamp,
    );

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (cfg, group) = state::read_market(&market_data).unwrap();
    assert_eq!(
        group.current_slot, real_slot,
        "hybrid configuration must authenticate engine time from SVM Clock, not the instruction body"
    );
    assert_eq!(group.slot_last, real_slot);
    assert_eq!(cfg.last_good_oracle_slot, real_slot);
    assert_eq!(cfg.mark_ewma_last_slot, real_slot);
    assert_ne!(
        group.current_slot, spoofed_slot,
        "caller-supplied configure now_slot must not future-clock the market"
    );
}

#[test]
fn v16_bpf_configure_hybrid_oracle_uses_authenticated_unix_time_not_caller_time() {
    let mut env = V16CuEnv::new();
    env.svm.warp_to_slot(10);
    let mut clock = env.svm.get_sysvar::<Clock>();
    clock.unix_timestamp = 1_000;
    env.svm.set_sysvar(&clock);

    let feeds = [[0xa1u8; 32], [0xa2u8; 32], [0xa3u8; 32]];
    let stale_publish_time = 1;
    let leg0 = env.set_pyth_price(&feeds[0], 4_000_000_000, -6, stale_publish_time);
    let leg1 = env.set_pyth_price(&feeds[1], 150_000_000, -6, stale_publish_time);
    let leg2 = env.set_pyth_price(&feeds[2], 200_000_000, -6, stale_publish_time);
    let before = env.svm.get_account(&env.market).unwrap().data;

    let spoofed_fresh_unix = stale_publish_time;
    let result =
        env.try_configure_three_leg_hybrid(feeds, leg0, leg1, leg2, 10, spoofed_fresh_unix);

    assert!(
        result.is_err(),
        "hybrid configuration must not accept stale oracle accounts by trusting caller now_unix_ts"
    );
    let after = env.svm.get_account(&env.market).unwrap().data;
    assert_eq!(
        after, before,
        "rejected stale-oracle configuration must not mutate the market"
    );
}

fn set_test_clock(env: &mut V16CuEnv, slot: u64, unix_timestamp: i64) {
    env.svm.warp_to_slot(slot);
    let mut clock = env.svm.get_sysvar::<Clock>();
    clock.unix_timestamp = unix_timestamp;
    env.svm.set_sysvar(&clock);
}

fn run_hybrid_fresh_oracle_trade_case(dt: u64, oracle_leg_count: u8, invert: u8) {
    let mut env = V16CuEnv::new();
    set_test_clock(&mut env, 1, 100);

    let seed = 0xc0u8
        .wrapping_add((dt as u8) << 4)
        .wrapping_add(oracle_leg_count << 1)
        .wrapping_add(invert);
    let mut feeds = [[0u8; 32]; 3];
    feeds[0] = [seed; 32];
    if oracle_leg_count == 3 {
        feeds[1] = [seed.wrapping_add(1); 32];
        feeds[2] = [seed.wrapping_add(2); 32];
    }
    let oracle_leg_flags = if oracle_leg_count == 3 {
        ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3
    } else {
        0
    };

    let initial_oracles = if oracle_leg_count == 1 {
        vec![env.set_pyth_price(&feeds[0], 200_000, -6, 100)]
    } else {
        vec![
            env.set_pyth_price(&feeds[0], 4_000_000_000, -6, 100),
            env.set_pyth_price(&feeds[1], 150_000_000, -6, 100),
            env.set_pyth_price(&feeds[2], 200_000_000, -6, 100),
        ]
    };
    let configure_cu = env
        .try_configure_hybrid_with_cu(
            oracle_leg_count,
            oracle_leg_flags,
            feeds,
            &initial_oracles,
            1,
            100,
            invert,
            0,
            3,
        )
        .expect("configure hybrid oracle");
    assert_cu_within(
        "HybridMark fresh-trade configure",
        configure_cu,
        CUSTODY_CU_LIMIT,
    );

    let keeper = Keypair::new();
    let keeper_portfolio = env.create_portfolio(&keeper);
    set_test_clock(&mut env, 2, 101);
    let fresh_oracles = if oracle_leg_count == 1 {
        vec![env.set_pyth_price(&feeds[0], 210_000, -6, 101)]
    } else {
        vec![
            env.set_pyth_price(&feeds[0], 4_200_000_000, -6, 101),
            env.set_pyth_price(&feeds[1], 150_000_000, -6, 101),
            env.set_pyth_price(&feeds[2], 200_000_000, -6, 101),
        ]
    };
    let fresh_crank_cu = env.crank_with_oracle_tail(
        keeper_portfolio,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 2,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &fresh_oracles,
    );
    assert_cu_within(
        "HybridMark fresh-trade crank",
        fresh_crank_cu,
        CRANK_CU_LIMIT,
    );

    let (fresh_cfg, fresh_group) = env.market_state();
    let mark = fresh_group.assets[0].effective_price;
    assert!(mark > 0, "fresh HybridMark case produced a zero mark");
    assert_eq!(fresh_cfg.last_good_oracle_slot, 2);
    assert_eq!(fresh_cfg.hybrid_soft_stale_slots, 3);
    assert_eq!(fresh_cfg.mark_ewma_e6, mark);
    assert_eq!(fresh_group.assets[0].raw_oracle_target_price, mark);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 10_000_000);
    env.deposit(&short_owner, short_account, 10_000_000);

    if dt == 1 {
        set_test_clock(&mut env, 3, 102);
    }
    let (before_trade_cfg, before_trade_group) = env.market_state();
    let trade_slot = env.svm.get_sysvar::<Clock>().slot;
    assert_eq!(
        trade_slot - before_trade_cfg.last_good_oracle_slot,
        dt,
        "test case must trade while the hybrid oracle is still fresh"
    );
    assert!(
        dt <= before_trade_cfg.hybrid_soft_stale_slots,
        "test case must remain inside the live-oracle freshness window"
    );
    let insurance_before = before_trade_group.insurance;

    let size_q = POS_SCALE;
    let open_cu = env
        .try_trade_asset_with_cu(
            0,
            &long_owner,
            long_account,
            &short_owner,
            short_account,
            size_q as i128,
            mark,
            0,
        )
        .unwrap_or_else(|err| {
            panic!(
                "fresh HybridMark TradeNoCpi open failed for dt={dt}, legs={oracle_leg_count}, invert={invert}: {err}"
            )
        });
    assert_cu_within("HybridMark fresh open", open_cu, TRADE_CU_LIMIT);
    let (opened_cfg, opened_group) = env.market_state();
    assert_eq!(opened_group.assets[0].oi_eff_long_q, size_q);
    assert_eq!(opened_group.assets[0].oi_eff_short_q, size_q);
    assert_eq!(opened_group.assets[0].effective_price, mark);
    assert_eq!(opened_group.assets[0].raw_oracle_target_price, mark);
    assert_eq!(opened_cfg.mark_ewma_e6, mark);
    assert_eq!(
        opened_group.insurance, insurance_before,
        "fresh HybridMark trade at the live mark must not charge an after-hours movement premium"
    );

    let close_cu = env
        .try_trade_asset_with_cu(
            0,
            &long_owner,
            long_account,
            &short_owner,
            short_account,
            -(size_q as i128),
            mark,
            0,
        )
        .unwrap_or_else(|err| {
            panic!(
                "fresh HybridMark TradeNoCpi close failed for dt={dt}, legs={oracle_leg_count}, invert={invert}: {err}"
            )
        });
    assert_cu_within("HybridMark fresh close", close_cu, TRADE_CU_LIMIT);
    let (_, flat_group) = env.market_state();
    assert_eq!(flat_group.assets[0].oi_eff_long_q, 0);
    assert_eq!(flat_group.assets[0].oi_eff_short_q, 0);
    assert_eq!(flat_group.assets[0].effective_price, mark);
    assert_eq!(flat_group.insurance, insurance_before);
}

#[test]
fn v16_bpf_hybrid_fresh_oracle_trade_opens_and_closes() {
    for dt in [0, 1] {
        for oracle_leg_count in [1, 3] {
            for invert in [0, 1] {
                run_hybrid_fresh_oracle_trade_case(dt, oracle_leg_count, invert);
            }
        }
    }
}

fn production_risk_params() -> V16CuMarketParams {
    V16CuMarketParams {
        h_max: 6_480_000,
        initial_price: 1_000_000,
        min_nonzero_mm_req: 599,
        min_nonzero_im_req: 600,
        maintenance_margin_bps: 500,
        initial_margin_bps: 500,
        liquidation_fee_bps: 5,
        liquidation_fee_cap: percolator::MAX_PROTOCOL_FEE_ABS,
        max_price_move_bps_per_slot: 24,
        max_accrual_dt_slots: 20,
        max_abs_funding_e9_per_slot: 1_000,
        min_funding_lifetime_slots: 10_000_000,
        ..V16CuMarketParams::default()
    }
}

#[derive(Clone, Copy)]
struct ProductionRiskOraclePrices {
    leg0: i64,
    leg1: i64,
    leg2: i64,
}

impl ProductionRiskOraclePrices {
    fn default_inverted_composite() -> Self {
        Self {
            leg0: 4_200_000_000,
            leg1: 150_000_000,
            leg2: 200_000_000,
        }
    }

    fn sub_one_inverted_composite() -> Self {
        Self {
            leg0: 2_155_172_400,
            leg1: 5_000_000,
            leg2: 5_000_000,
        }
    }
}

#[derive(Clone, Copy)]
struct ProductionRiskTradeCase {
    name: &'static str,
    fixed_deposit: Option<u128>,
    same_owner: bool,
    oracle_prices: ProductionRiskOraclePrices,
    oracle_conf_bps: u16,
    conf_filter_bps: u16,
    size_q_abs: u128,
    assert_sub_one_mark: bool,
}

impl ProductionRiskTradeCase {
    fn baseline() -> Self {
        Self {
            name: "baseline",
            fixed_deposit: None,
            same_owner: false,
            oracle_prices: ProductionRiskOraclePrices::default_inverted_composite(),
            oracle_conf_bps: 0,
            conf_filter_bps: 500,
            size_q_abs: POS_SCALE,
            assert_sub_one_mark: false,
        }
    }

    fn fixed_deposit() -> Self {
        Self {
            name: "fixed-300m-deposit",
            fixed_deposit: Some(300_000_000),
            ..Self::baseline()
        }
    }

    fn same_owner() -> Self {
        Self {
            name: "same-owner-counterparties",
            same_owner: true,
            ..Self::baseline()
        }
    }

    fn sub_one_mark() -> Self {
        Self {
            name: "sub-one-inverted-mark",
            oracle_prices: ProductionRiskOraclePrices::sub_one_inverted_composite(),
            size_q_abs: 10 * POS_SCALE,
            assert_sub_one_mark: true,
            ..Self::baseline()
        }
    }

    fn real_conf_filter() -> Self {
        Self {
            name: "pyth-conf-150bps-filter-200bps",
            oracle_conf_bps: 150,
            conf_filter_bps: 200,
            ..Self::baseline()
        }
    }
}

fn pyth_conf_for_bps(price: i64, conf_bps: u16) -> u64 {
    if conf_bps == 0 {
        return 1;
    }
    ((price as u128) * conf_bps as u128 / 10_000)
        .max(1)
        .try_into()
        .unwrap()
}

fn set_production_risk_oracles(
    env: &mut V16CuEnv,
    feeds: &[[u8; 32]; 3],
    prices: ProductionRiskOraclePrices,
    conf_bps: u16,
    publish_time: i64,
) -> [Pubkey; 3] {
    [
        env.set_pyth_price_with_conf(
            &feeds[0],
            prices.leg0,
            -6,
            pyth_conf_for_bps(prices.leg0, conf_bps),
            publish_time,
        ),
        env.set_pyth_price_with_conf(
            &feeds[1],
            prices.leg1,
            -6,
            pyth_conf_for_bps(prices.leg1, conf_bps),
            publish_time,
        ),
        env.set_pyth_price_with_conf(
            &feeds[2],
            prices.leg2,
            -6,
            pyth_conf_for_bps(prices.leg2, conf_bps),
            publish_time,
        ),
    ]
}

fn run_hybrid_fresh_oracle_production_risk_trade_case(
    asset_index: u16,
    case: ProductionRiskTradeCase,
    direction_sign: i128,
) {
    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    set_test_clock(&mut env, 1, 100);
    if asset_index != 0 {
        env.activate_asset(asset_index, 1, production_risk_params().initial_price);
    }

    let feed_seed = 0xe0u8.wrapping_add(asset_index as u8 * 3);
    let feeds = [
        [feed_seed.wrapping_add(1); 32],
        [feed_seed.wrapping_add(2); 32],
        [feed_seed.wrapping_add(3); 32],
    ];
    let [initial_leg0, initial_leg1, initial_leg2] = set_production_risk_oracles(
        &mut env,
        &feeds,
        case.oracle_prices,
        case.oracle_conf_bps,
        100,
    );
    let configure_cu = env
        .try_configure_hybrid_asset_with_conf_filter_cu(
            asset_index,
            3,
            ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3,
            feeds,
            &[initial_leg0, initial_leg1, initial_leg2],
            1,
            100,
            1,
            0,
            3,
            case.conf_filter_bps,
        )
        .expect("configure inverted production-risk hybrid oracle");
    assert_cu_within(
        "HybridMark production-risk configure",
        configure_cu,
        CUSTODY_CU_LIMIT,
    );

    let keeper = Keypair::new();
    let keeper_portfolio = env.create_portfolio(&keeper);
    set_test_clock(&mut env, 2, 101);
    let [fresh_leg0, fresh_leg1, fresh_leg2] = set_production_risk_oracles(
        &mut env,
        &feeds,
        case.oracle_prices,
        case.oracle_conf_bps,
        101,
    );
    let fresh_crank_cu = env.crank_with_oracle_tail(
        keeper_portfolio,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index,
            now_slot: 2,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &[fresh_leg0, fresh_leg1, fresh_leg2],
    );
    assert_cu_within(
        "HybridMark production-risk fresh crank",
        fresh_crank_cu,
        CRANK_CU_LIMIT,
    );
    let (fresh_cfg, fresh_group) = env.market_state();
    let mark = fresh_group.assets[asset_index as usize].effective_price;
    if case.assert_sub_one_mark {
        assert!(
            mark < 1_000_000,
            "{} must exercise an inverted mark below 1.0, got {mark}",
            case.name
        );
    }
    if asset_index == 0 {
        assert_eq!(fresh_cfg.last_good_oracle_slot, 2);
        assert_eq!(fresh_cfg.hybrid_soft_stale_slots, 3);
        assert_eq!(fresh_cfg.mark_ewma_e6, mark);
    } else {
        let market_data = env.svm.get_account(&env.market).unwrap().data;
        let fresh_profile =
            state::read_asset_oracle_profile(&market_data, asset_index as usize).unwrap();
        assert_eq!(fresh_profile.last_good_oracle_slot, 2);
        assert_eq!(fresh_profile.hybrid_soft_stale_slots, 3);
        assert_eq!(fresh_profile.mark_ewma_e6, mark);
    }
    assert_eq!(
        fresh_group.assets[asset_index as usize].raw_oracle_target_price,
        mark
    );

    let long_owner = Keypair::new();
    let short_owner = if case.same_owner {
        None
    } else {
        Some(Keypair::new())
    };
    let short_owner_ref = short_owner.as_ref().unwrap_or(&long_owner);
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(short_owner_ref);
    let size_q = direction_sign
        .checked_mul(case.size_q_abs as i128)
        .expect("signed size");
    let notional = (mark as u128)
        .checked_mul(case.size_q_abs)
        .and_then(|v| v.checked_div(POS_SCALE))
        .expect("notional");
    let exact_im_deposit = notional
        .checked_mul(production_risk_params().initial_margin_bps as u128)
        .and_then(|v| v.checked_add(9_999))
        .and_then(|v| v.checked_div(10_000))
        .expect("deposit");
    let deposit_amount = case.fixed_deposit.unwrap_or(exact_im_deposit);
    env.deposit(&long_owner, long_account, deposit_amount);
    env.deposit(short_owner_ref, short_account, deposit_amount);

    let direction = if size_q > 0 { "long" } else { "short" };
    let open_cu = env
        .try_trade_asset_with_cu(
            asset_index,
            &long_owner,
            long_account,
            short_owner_ref,
            short_account,
            size_q,
            mark,
            0,
        )
        .unwrap_or_else(|err| {
            panic!(
                "production-risk fresh HybridMark {} asset[{asset_index}] {direction}-open failed at mark={mark}, deposit={deposit_amount}: {err}",
                case.name
            )
        });
    assert_cu_within(
        "HybridMark production-risk fresh open",
        open_cu,
        TRADE_CU_LIMIT,
    );
    let (_, opened_group) = env.market_state();
    assert_eq!(
        opened_group.assets[asset_index as usize].oi_eff_long_q,
        size_q.unsigned_abs()
    );
    assert_eq!(
        opened_group.assets[asset_index as usize].oi_eff_short_q,
        size_q.unsigned_abs()
    );

    let close_cu = env
        .try_trade_asset_with_cu(
            asset_index,
            &long_owner,
            long_account,
            short_owner_ref,
            short_account,
            -size_q,
            mark,
            0,
        )
        .unwrap_or_else(|err| {
            panic!(
                "production-risk fresh HybridMark {} asset[{asset_index}] {direction}-close failed at mark={mark}, deposit={deposit_amount}: {err}",
                case.name
            )
        });
    assert_cu_within(
        "HybridMark production-risk fresh close",
        close_cu,
        TRADE_CU_LIMIT,
    );
    let (_, flat_group) = env.market_state();
    assert_eq!(flat_group.assets[asset_index as usize].oi_eff_long_q, 0);
    assert_eq!(flat_group.assets[asset_index as usize].oi_eff_short_q, 0);
}

#[test]
fn v16_bpf_hybrid_fresh_oracle_trade_production_risk_params_opens_and_closes() {
    for asset_index in [0, 1] {
        for direction_sign in [1, -1] {
            run_hybrid_fresh_oracle_production_risk_trade_case(
                asset_index,
                ProductionRiskTradeCase::baseline(),
                direction_sign,
            );
        }
    }
}

#[test]
fn v16_bpf_hybrid_fresh_oracle_trade_devnet_difference_axes() {
    for case in [
        ProductionRiskTradeCase::fixed_deposit(),
        ProductionRiskTradeCase::same_owner(),
        ProductionRiskTradeCase::sub_one_mark(),
        ProductionRiskTradeCase::real_conf_filter(),
    ] {
        for asset_index in [0, 1] {
            for direction_sign in [1, -1] {
                run_hybrid_fresh_oracle_production_risk_trade_case(
                    asset_index,
                    case,
                    direction_sign,
                );
            }
        }
    }
}

#[test]
fn v16_bpf_hybrid_mark_uses_ewma_after_hours_then_oracle_when_fresh() {
    let mut env = V16CuEnv::new();
    env.svm.warp_to_slot(1);
    let mut clock = env.svm.get_sysvar::<Clock>();
    clock.unix_timestamp = 100;
    env.svm.set_sysvar(&clock);

    let feeds = [[0xb1u8; 32], [0xb2u8; 32], [0xb3u8; 32]];
    let leg0 = env.set_pyth_price(&feeds[0], 4_000_000_000, -6, 100);
    let leg1 = env.set_pyth_price(&feeds[1], 150_000_000, -6, 100);
    let leg2 = env.set_pyth_price(&feeds[2], 200_000_000, -6, 100);
    let configure_cu = env.configure_three_leg_hybrid_with_cu(feeds, leg0, leg1, leg2, 1, 100);
    assert_cu_within("ConfigureHybridOracle", configure_cu, CUSTODY_CU_LIMIT);

    let keeper = Keypair::new();
    let keeper_portfolio = env.create_portfolio(&keeper);
    env.svm.warp_to_slot(2);
    let mut clock = env.svm.get_sysvar::<Clock>();
    clock.unix_timestamp = 101;
    env.svm.set_sysvar(&clock);
    let fresh_leg0 = env.set_pyth_price(&feeds[0], 4_200_000_000, -6, 101);
    let fresh_leg1 = env.set_pyth_price(&feeds[1], 150_000_000, -6, 101);
    let fresh_leg2 = env.set_pyth_price(&feeds[2], 200_000_000, -6, 101);
    let fresh_crank_cu = env.crank_with_oracle_tail(
        keeper_portfolio,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 2,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &[fresh_leg0, fresh_leg1, fresh_leg2],
    );
    assert_cu_within("HybridMark fresh crank", fresh_crank_cu, CRANK_CU_LIMIT);
    let (fresh_cfg, fresh_group) = env.market_state();
    assert_eq!(fresh_group.assets[0].effective_price, 140_000);
    assert_eq!(fresh_cfg.mark_ewma_e6, 140_000);
    assert_eq!(fresh_cfg.last_good_oracle_slot, 2);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 10_000_000);
    env.deposit(&short_owner, short_account, 10_000_000);

    env.svm.warp_to_slot(10);
    let before_after_hours = env.market_state();
    let size_q = POS_SCALE;
    let after_hours_exec_price = before_after_hours.1.assets[0].effective_price * 150 / 100;
    let open_cu = env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        size_q as i128,
        after_hours_exec_price,
        0,
    );
    assert_cu_within("HybridMark after-hours open", open_cu, TRADE_CU_LIMIT);
    let (after_hours_cfg, after_hours_group) = env.market_state();
    assert!(
        after_hours_cfg.mark_ewma_e6 > before_after_hours.0.mark_ewma_e6,
        "after-hours hybrid trade must advance the fallback EWMA mark"
    );
    assert_eq!(
        after_hours_group.assets[0].effective_price, before_after_hours.1.assets[0].effective_price,
        "after-hours execution must not rewrite the last accepted oracle index"
    );
    assert!(
        after_hours_group.insurance > 0,
        "after-hours hybrid trade must charge a dynamic mark-movement fee"
    );

    let close_cu = env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        -(size_q as i128),
        after_hours_exec_price,
        0,
    );
    assert_cu_within("HybridMark after-hours close", close_cu, TRADE_CU_LIMIT);
    let (_, flat_group) = env.market_state();
    assert_eq!(flat_group.assets[0].oi_eff_long_q, 0);
    assert_eq!(flat_group.assets[0].oi_eff_short_q, 0);

    env.svm.warp_to_slot(11);
    let mut clock = env.svm.get_sysvar::<Clock>();
    clock.unix_timestamp = 102;
    env.svm.set_sysvar(&clock);
    let normal_leg0 = env.set_pyth_price(&feeds[0], 4_500_000_000, -6, 102);
    let normal_leg1 = env.set_pyth_price(&feeds[1], 150_000_000, -6, 102);
    let normal_leg2 = env.set_pyth_price(&feeds[2], 200_000_000, -6, 102);
    let normal_crank_cu = env.crank_with_oracle_tail(
        keeper_portfolio,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 11,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
        &[normal_leg0, normal_leg1, normal_leg2],
    );
    assert_cu_within(
        "HybridMark normal-hours crank",
        normal_crank_cu,
        CRANK_CU_LIMIT,
    );
    let (normal_cfg, normal_group) = env.market_state();
    assert_eq!(normal_cfg.last_good_oracle_slot, 11);
    assert_eq!(normal_cfg.mark_ewma_last_slot, 11);
    assert_eq!(normal_cfg.mark_ewma_e6, 150_000);
    assert_eq!(normal_group.assets[0].effective_price, 150_000);
    assert_eq!(normal_group.assets[0].raw_oracle_target_price, 150_000);
}

#[test]
fn v16_bpf_configure_and_push_ewma_mark_are_bounded_and_clock_authenticated() {
    let mut env = V16CuEnv::new();
    let configure_real_slot = 8;
    let push_real_slot = 9;
    let spoofed_slot = 1_000_000;
    env.svm.warp_to_slot(configure_real_slot);
    let configure_cu = env.configure_ewma_mark_with_cu(spoofed_slot, 100, 1, 0);
    env.svm.warp_to_slot(push_real_slot);
    let push_cu = env.push_ewma_mark_with_cu(spoofed_slot, 120);
    println!("v16 EwmaMark configure CU: {configure_cu}, push CU: {push_cu}");
    assert!(
        configure_cu <= CUSTODY_CU_LIMIT,
        "EwmaMark configure CU {} exceeded limit {}",
        configure_cu,
        CUSTODY_CU_LIMIT
    );
    assert!(
        push_cu <= CUSTODY_CU_LIMIT,
        "EwmaMark push CU {} exceeded limit {}",
        push_cu,
        CUSTODY_CU_LIMIT
    );

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (cfg, group) = state::read_market(&market_data).unwrap();
    assert_eq!(
        cfg.oracle_mode,
        percolator_prog::constants::ORACLE_MODE_EWMA_MARK
    );
    assert_eq!(group.current_slot, configure_real_slot);
    assert_eq!(group.slot_last, configure_real_slot);
    assert_eq!(cfg.mark_ewma_last_slot, push_real_slot);
    assert_eq!(
        cfg.mark_ewma_e6, 110,
        "authority mark push should update the EWMA using authenticated slot time"
    );
    assert_ne!(
        cfg.mark_ewma_last_slot, spoofed_slot,
        "caller-supplied PushEwmaMark now_slot must not authenticate mark liveness"
    );
}

#[test]
fn v16_bpf_configure_and_push_auth_mark_are_bounded_and_clock_authenticated() {
    let mut env = V16CuEnv::new();
    let configure_real_slot = 8;
    let push_real_slot = 9;
    let spoofed_slot = 1_000_000;
    env.svm.warp_to_slot(configure_real_slot);
    let configure_cu = env.configure_auth_mark_with_cu(spoofed_slot, 100);
    env.svm.warp_to_slot(push_real_slot);
    let push_cu = env.push_auth_mark_with_cu(spoofed_slot, 120);
    println!("v16 AuthMark configure CU: {configure_cu}, push CU: {push_cu}");
    assert!(
        configure_cu <= CUSTODY_CU_LIMIT,
        "AuthMark configure CU {} exceeded limit {}",
        configure_cu,
        CUSTODY_CU_LIMIT
    );
    assert!(
        push_cu <= CUSTODY_CU_LIMIT,
        "AuthMark push CU {} exceeded limit {}",
        push_cu,
        CUSTODY_CU_LIMIT
    );

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (cfg, group) = state::read_market(&market_data).unwrap();
    assert_eq!(
        cfg.oracle_mode,
        percolator_prog::constants::ORACLE_MODE_AUTH_MARK
    );
    assert_eq!(group.current_slot, configure_real_slot);
    assert_eq!(group.slot_last, configure_real_slot);
    assert_eq!(cfg.mark_ewma_last_slot, push_real_slot);
    assert_eq!(
        cfg.mark_ewma_e6, 120,
        "authority mark push should store the AuthMark value directly"
    );
    assert_eq!(cfg.oracle_target_price_e6, 120);
    assert_eq!(cfg.mark_ewma_halflife_slots, 0);
    assert_ne!(
        cfg.mark_ewma_last_slot, spoofed_slot,
        "caller-supplied PushAuthMark now_slot must not authenticate mark liveness"
    );
}

#[test]
fn v16_bpf_auth_mark_target_effective_lag_counts_toward_liquidation_health() {
    const INITIAL_MARK: u64 = 100_000_000;
    const TARGET_MARK: u64 = 90_000_000;
    const EXPECTED_EFFECTIVE_AFTER_ONE_SLOT: u64 = 99_760_000;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 24);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, INITIAL_MARK);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_portfolio = env.create_portfolio(&long_owner);
    let short_portfolio = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_portfolio, 100_000_000);
    env.deposit(&short_owner, short_portfolio, 200_000_000);
    env.trade_with_cu(
        &long_owner,
        long_portfolio,
        &short_owner,
        short_portfolio,
        POS_SCALE as i128,
        INITIAL_MARK,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, TARGET_MARK);
    env.crank(
        long_portfolio,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 2,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );

    let (_, lagged_group) = env.market_state();
    assert_eq!(
        lagged_group.assets[0].raw_oracle_target_price, TARGET_MARK,
        "AuthMark stores the un-clamped target for health certification"
    );
    assert_eq!(
        lagged_group.assets[0].effective_price, EXPECTED_EFFECTIVE_AFTER_ONE_SLOT,
        "effective price should be clamp-lagged by one 24 bps slot"
    );

    let lagged_long = env.portfolio_state(long_portfolio);
    assert!(
        lagged_long.health_cert.valid,
        "refresh must write a health certificate"
    );
    assert!(
        lagged_long.health_cert.certified_maintenance_req > INITIAL_MARK as u128,
        "maintenance must include the adverse target/effective lag penalty"
    );
    assert!(
        lagged_long.health_cert.certified_liq_deficit > 0,
        "lagged adverse AuthMark target must make the under-margined long liquidatable"
    );

    env.crank(
        long_portfolio,
        ProgInstruction::PermissionlessCrank {
            action: 1,
            asset_index: 0,
            now_slot: 2,
            funding_rate_e9: 0,
            close_q: POS_SCALE,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
    let liquidated_long = env.portfolio_state(long_portfolio);
    assert!(
        !has_active_leg_for_asset(&liquidated_long, 0),
        "positive lag-deficit certification must allow permissionless liquidation"
    );
}

#[test]
fn v16_cu_crank_cost_is_account_local_after_many_portfolios() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000_000);

    let before_extra = env.crank(
        portfolio,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 1,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
    for _ in 0..64 {
        let owner = Keypair::new();
        let p = env.create_portfolio(&owner);
        let acct = env.svm.get_account(&p).expect("portfolio account exists");
        let (_header, parsed_owner) = state::read_portfolio_owner_preflight(&acct.data).unwrap();
        assert_eq!(parsed_owner, owner.pubkey().to_bytes());
    }

    let after_extra = env.crank(
        portfolio,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 2,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
    println!(
        "v16 refresh crank CU before extra portfolios: {before_extra}, after 64 extras: {after_extra}"
    );

    assert!(after_extra <= CRANK_CU_LIMIT);
    assert!(
        after_extra.saturating_sub(before_extra) < 10_000,
        "v16 crank should stay account-local rather than scaling with materialized portfolio count"
    );
}

#[test]
fn v16_bpf_policy_authority_and_base_unit_tags_are_bounded_and_persist() {
    let mut env = V16CuEnv::new();

    let liquidation_cu = env.update_liquidation_fee_policy_with_cu(1_234);
    assert_cu_within(
        "UpdateLiquidationFeePolicy",
        liquidation_cu,
        CUSTODY_CU_LIMIT,
    );
    let (cfg, _) = env.market_state();
    assert_eq!(cfg.liquidation_cranker_fee_share_bps, 1_234);

    let backing_cu = env.update_backing_fee_policy_with_cu(0, 77, 5_000);
    assert_cu_within("UpdateBackingFeePolicy", backing_cu, CUSTODY_CU_LIMIT);
    let (cfg, _) = env.market_state();
    assert_eq!(cfg.backing_trade_fee_bps_long, 77);
    assert_eq!(cfg.backing_trade_fee_insurance_share_bps_long, 5_000);
    assert_eq!(cfg.backing_trade_fee_policy_count, 1);

    let trade_fee_cu = env.update_trade_fee_policy_with_cu(88);
    assert_cu_within("UpdateTradeFeePolicy", trade_fee_cu, CUSTODY_CU_LIMIT);
    let (cfg, _) = env.market_state();
    assert_eq!(cfg.trade_fee_base_bps, 88);

    let redirect_cu = env.update_fee_redirect_policy_with_cu(2_500);
    assert_cu_within("UpdateFeeRedirectPolicy", redirect_cu, CUSTODY_CU_LIMIT);
    let (cfg, _) = env.market_state();
    assert_eq!(cfg.fee_redirect_to_market_0_bps, 2_500);

    let secondary_mint = env.create_mint();
    let base_unit_cu = env.update_base_unit_mints_with_cu(env.mint, secondary_mint);
    assert_cu_within("UpdateBaseUnitMints", base_unit_cu, CUSTODY_CU_LIMIT);
    let (cfg, _) = env.market_state();
    assert_eq!(cfg.collateral_mint, env.mint.to_bytes());
    assert_eq!(cfg.secondary_collateral_mint, secondary_mint.to_bytes());

    let primary_source = env.token_account_for_mint(env.mint, env.admin.pubkey(), 50);
    let secondary_dest = env.token_account_for_mint(secondary_mint, env.admin.pubkey(), 0);
    let secondary_vault = env.token_account_for_mint(secondary_mint, env.vault_authority, 50);
    let before_swap_market = env.svm.get_account(&env.market).unwrap().data;
    let swap_cu = env.swap_secondary_for_primary_with_cu(
        primary_source,
        env.vault,
        secondary_dest,
        secondary_vault,
        50,
    );
    assert_cu_within("SwapSecondaryForPrimary", swap_cu, CUSTODY_CU_LIMIT);
    assert_eq!(env.token_amount(primary_source), 0);
    assert_eq!(env.token_amount(env.vault), 50);
    assert_eq!(env.token_amount(secondary_dest), 50);
    assert_eq!(env.token_amount(secondary_vault), 0);
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before_swap_market,
        "base-unit swap must only move SPL custody"
    );

    let new_asset_authority = Keypair::new();
    let authority_cu = env.update_asset_authority_with_cu(&new_asset_authority);
    assert_cu_within("UpdateAuthority", authority_cu, CUSTODY_CU_LIMIT);
    let (cfg, _) = env.market_state();
    assert_eq!(cfg.asset_authority, new_asset_authority.pubkey().to_bytes());
}

#[test]
fn v16_bpf_accounting_ledger_tags_are_bounded_and_update_state() {
    let mut env = V16CuEnv::new();
    let ledger = env.backing_domain_ledger_account();
    let (backing_source, top_up_cu) =
        env.top_up_backing_bucket_with_ledger_with_cu(ledger, 1, 100, 10);
    assert_cu_within(
        "TopUpBackingBucket ledger init",
        top_up_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(env.token_amount(backing_source), 0);

    env.mutate_market(|_, group| {
        group.source_backing_buckets[1].utilization_fee_earnings = 30;
        group.vault += 30;
    });
    env.set_token_account_amount(env.vault, env.mint, env.vault_authority, 130);

    let sync_cu = env.sync_backing_domain_ledger_with_cu(ledger, 1);
    assert_cu_within("SyncBackingDomainLedger", sync_cu, CUSTODY_CU_LIMIT);
    let ledger_data = env.svm.get_account(&ledger).unwrap().data;
    let ledger_state = state::read_backing_domain_ledger(&ledger_data).unwrap();
    assert_eq!(ledger_state.total_principal_atoms, 100);
    assert_eq!(ledger_state.last_observed_bucket_earnings_atoms, 30);
    assert_eq!(ledger_state.total_earnings_atoms, 30);

    let dest = env.token_account_for_mint(env.mint, env.admin.pubkey(), 0);
    let withdraw_earnings_cu =
        env.withdraw_backing_bucket_earnings_to_admin_token_with_cu(ledger, dest, 1, 20);
    assert_cu_within(
        "WithdrawBackingBucketEarnings",
        withdraw_earnings_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(env.token_amount(dest), 20);
    let ledger_data = env.svm.get_account(&ledger).unwrap().data;
    let ledger_state = state::read_backing_domain_ledger(&ledger_data).unwrap();
    let (_, group) = env.market_state();
    assert_eq!(ledger_state.total_earnings_withdrawn_atoms, 20);
    assert_eq!(ledger_state.last_observed_bucket_earnings_atoms, 10);
    assert_eq!(group.source_backing_buckets[1].utilization_fee_earnings, 10);
    assert_eq!(group.vault, 110);

    let mut pnl_env = V16CuEnv::new();
    let pnl_ledger = pnl_env.backing_domain_ledger_account();
    pnl_env.top_up_backing_bucket_with_ledger_with_cu(pnl_ledger, 1, 40, 10);
    let owner = Keypair::new();
    let portfolio = pnl_env.create_portfolio(&owner);
    pnl_env.add_source_positive_pnl(portfolio, 1, 40);
    pnl_env.crank(
        portfolio,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 0,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
    let convert_cu = pnl_env.convert_released_pnl_with_cu(&owner, portfolio, 40);
    assert_cu_within("ConvertReleasedPnl", convert_cu, CUSTODY_CU_LIMIT);
    let account = pnl_env.portfolio_state(portfolio);
    assert_eq!(account.capital, 40);
    pnl_env.sync_backing_domain_ledger_with_cu(pnl_ledger, 1);
    let ledger_data = pnl_env.svm.get_account(&pnl_ledger).unwrap().data;
    let ledger_state = state::read_backing_domain_ledger(&ledger_data).unwrap();
    assert_eq!(ledger_state.cumulative_loss_atoms, 40);
    assert_eq!(ledger_state.last_observed_unavailable_principal_atoms, 40);

    let mut insurance_env = V16CuEnv::new();
    let insurance_ledger = insurance_env.insurance_ledger_account();
    let (_, insurance_top_up_cu) =
        insurance_env.top_up_insurance_with_ledger_with_cu(insurance_ledger, 100);
    assert_cu_within(
        "TopUpInsurance ledger init",
        insurance_top_up_cu,
        CUSTODY_CU_LIMIT,
    );
    let init_cu = insurance_env.sync_insurance_ledger_with_cu(insurance_ledger);
    assert_cu_within("SyncInsuranceLedger init", init_cu, CUSTODY_CU_LIMIT);
    let ledger_data = insurance_env
        .svm
        .get_account(&insurance_ledger)
        .unwrap()
        .data;
    let ledger_state = state::read_insurance_ledger(&ledger_data).unwrap();
    assert_eq!(ledger_state.total_principal_atoms, 100);
    assert_eq!(ledger_state.last_observed_insurance_atoms, 100);

    insurance_env.mutate_market(|_, group| {
        group.insurance += 30;
        group.vault += 30;
        group.insurance_domain_budget[0] += 15;
        group.insurance_domain_budget[1] += 15;
    });
    insurance_env.svm.expire_blockhash();
    let profit_cu = insurance_env.sync_insurance_ledger_with_cu(insurance_ledger);
    assert_cu_within("SyncInsuranceLedger profit", profit_cu, CUSTODY_CU_LIMIT);
    let ledger_data = insurance_env
        .svm
        .get_account(&insurance_ledger)
        .unwrap()
        .data;
    let ledger_state = state::read_insurance_ledger(&ledger_data).unwrap();
    assert_eq!(ledger_state.cumulative_profit_atoms, 30);
    assert_eq!(ledger_state.last_observed_insurance_atoms, 130);

    insurance_env.mutate_market(|_, group| {
        group.insurance -= 20;
        group.vault -= 20;
        group.insurance_domain_budget[0] -= 10;
        group.insurance_domain_budget[1] -= 10;
    });
    insurance_env.svm.expire_blockhash();
    let loss_cu = insurance_env.sync_insurance_ledger_with_cu(insurance_ledger);
    assert_cu_within("SyncInsuranceLedger loss", loss_cu, CUSTODY_CU_LIMIT);
    let ledger_data = insurance_env
        .svm
        .get_account(&insurance_ledger)
        .unwrap()
        .data;
    let ledger_state = state::read_insurance_ledger(&ledger_data).unwrap();
    assert_eq!(ledger_state.cumulative_loss_atoms, 20);
    assert_eq!(ledger_state.last_observed_insurance_atoms, 110);
}

#[test]
fn v16_bpf_recovery_and_reset_tags_are_bounded_and_update_state() {
    let mut reduce_env = V16CuEnv::new();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = reduce_env.create_portfolio(&long_owner);
    let short_account = reduce_env.create_portfolio(&short_owner);
    reduce_env.deposit(&long_owner, long_account, 10_000);
    reduce_env.deposit(&short_owner, short_account, 10_000);
    reduce_env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        (2 * POS_SCALE) as i128,
        100,
        0,
    );

    let reduce_cu = reduce_env.rebalance_reduce_with_cu(&long_owner, long_account, 0, POS_SCALE);
    assert_cu_within("RebalanceReduce", reduce_cu, CUSTODY_CU_LIMIT);
    let (_, group) = reduce_env.market_state();
    let long = reduce_env.portfolio_state(long_account);
    assert_eq!(long.legs[0].basis_pos_q, POS_SCALE as i128);
    assert_eq!(group.assets[0].oi_eff_long_q, POS_SCALE);

    let mut forfeit_env = V16CuEnv::new();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = forfeit_env.create_portfolio(&long_owner);
    let short_account = forfeit_env.create_portfolio(&short_owner);
    forfeit_env.deposit(&long_owner, long_account, 10_000);
    forfeit_env.deposit(&short_owner, short_account, 10_000);
    forfeit_env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        100,
        0,
    );
    forfeit_env.mutate_market(|_, group| {
        group.mode = MarketModeV16::Recovery;
        group.recovery_reason = Some(PermissionlessRecoveryReasonV16::BelowProgressFloor);
    });
    let forfeit_cu = forfeit_env.forfeit_recovery_leg_with_cu(&long_owner, long_account, 0, 1);
    assert_cu_within("ForfeitRecoveryLeg", forfeit_cu, CUSTODY_CU_LIMIT);
    let (_, group) = forfeit_env.market_state();
    let long = forfeit_env.portfolio_state(long_account);
    assert!(percolator::active_bitmap_is_empty(long.active_bitmap));
    assert_eq!(long.legs[0].basis_pos_q, 0);
    assert_eq!(group.assets[0].oi_eff_long_q, 0);

    let mut cure_env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = cure_env.create_portfolio(&owner);
    cure_env.seed_cancellable_close_progress(portfolio);
    let source = cure_env.token_account_for_mint(cure_env.mint, owner.pubkey(), 20);
    let cure_cu = cure_env.cure_and_cancel_close_with_cu(&owner, portfolio, source, 20);
    assert_cu_within("CureAndCancelClose", cure_cu, CUSTODY_CU_LIMIT);
    let (_, group) = cure_env.market_state();
    let account = cure_env.portfolio_state(portfolio);
    assert!(account.close_progress.canceled);
    assert_eq!(account.capital, 20);
    assert_eq!(group.c_tot, 20);
    assert_eq!(group.vault, 20);
    assert_eq!(group.pending_domain_loss_barriers[0], 0);
    assert_eq!(cure_env.token_amount(source), 0);
    assert_eq!(cure_env.token_amount(cure_env.vault), 20);

    let mut reset_env = V16CuEnv::new();
    reset_env.mutate_market(|_, group| {
        group.assets[0].mode_long = SideModeV16::ResetPending;
    });
    let reset_cu = reset_env.finalize_reset_side_with_cu(0, 0);
    assert_cu_within("FinalizeResetSide", reset_cu, CUSTODY_CU_LIMIT);
    let (_, group) = reset_env.market_state();
    assert_eq!(group.assets[0].mode_long, SideModeV16::Normal);
}

#[test]
fn v16_bpf_resolved_payout_tags_are_bounded_and_update_state() {
    let mut claim_env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = claim_env.create_portfolio(&owner);
    {
        let mut market_account = claim_env
            .svm
            .get_account(&claim_env.market)
            .expect("market account");
        let mut portfolio_account = claim_env
            .svm
            .get_account(&portfolio)
            .expect("portfolio account");
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
        account.resolved_payout_receipt = ResolvedPayoutReceiptV16 {
            present: true,
            prior_bound_contribution_num: 100 * BOUND_SCALE,
            live_released_face_at_receipt: 0,
            terminal_positive_claim_face: 100,
            paid_effective: 40,
            finalized: false,
        };
        state::write_market(&mut market_account.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut portfolio_account.data, &account).unwrap();
        claim_env
            .svm
            .set_account(claim_env.market, market_account)
            .unwrap();
        claim_env
            .svm
            .set_account(portfolio, portfolio_account)
            .unwrap();
    }
    claim_env.set_token_account_amount(
        claim_env.vault,
        claim_env.mint,
        claim_env.vault_authority,
        60,
    );
    let dest = claim_env.token_account_for_mint(claim_env.mint, owner.pubkey(), 0);
    let claim_cu = claim_env.claim_resolved_payout_topup_with_cu(owner.pubkey(), portfolio, dest);
    assert_cu_within("ClaimResolvedPayoutTopup", claim_cu, CUSTODY_CU_LIMIT);
    assert_eq!(claim_env.token_amount(dest), 60);
    assert_eq!(claim_env.token_amount(claim_env.vault), 0);
    let (_, group) = claim_env.market_state();
    let account = claim_env.portfolio_state(portfolio);
    assert_eq!(group.vault, 0);
    assert_eq!(account.resolved_payout_receipt.paid_effective, 100);
    assert!(account.resolved_payout_receipt.finalized);

    let mut refine_env = V16CuEnv::new();
    refine_env.mutate_market(|_, group| {
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
    });
    let refine_cu = refine_env.refine_resolved_unreceipted_bound_with_cu(10 * BOUND_SCALE);
    assert_cu_within(
        "RefineResolvedUnreceiptedBound",
        refine_cu,
        CUSTODY_CU_LIMIT,
    );
    let (_, group) = refine_env.market_state();
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
