use litesvm::LiteSVM;
use percolator::{
    AssetLifecycleV16, BackingBucketStatusV16, CloseProgressLedgerV16, MarketGroupV16,
    MarketModeV16, PermissionlessRecoveryReasonV16, PortfolioAccountV16, ResolvedPayoutLedgerV16,
    ResolvedPayoutReceiptV16, SideModeV16, SideV16, TradeRequestV16, BOUND_SCALE, POS_SCALE,
};
use percolator_prog::{
    constants::{MATCHER_ABI_VERSION, ORACLE_LEG_FLAG_DIVIDE_LEG2, ORACLE_LEG_FLAG_DIVIDE_LEG3},
    ix::Instruction as ProgInstruction,
    oracle_v16, processor, state,
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
    let mut data = vec![0u8; 66];
    data[0] = 2;
    data[1] = 0;
    data[10..14].copy_from_slice(&100u32.to_le_bytes());
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
                    state::market_account_len_for_capacity(max_portfolio_assets as usize)
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
                max_portfolio_assets,
                h_min: 0,
                h_max: 10,
                initial_price: 100,
                min_nonzero_mm_req: 1,
                min_nonzero_im_req: 2,
                maintenance_margin_bps,
                initial_margin_bps,
                max_trading_fee_bps: 10_000,
                trade_fee_base_bps: 0,
                liquidation_fee_bps: 0,
                liquidation_fee_cap: 0,
                min_liquidation_abs: 0,
                max_price_move_bps_per_slot,
                max_accrual_dt_slots: 1,
                max_abs_funding_e9_per_slot: 0,
                min_funding_lifetime_slots: 1,
                max_account_b_settlement_chunks: 1,
                max_bankrupt_close_chunks: 1,
                max_bankrupt_close_lifetime_slots: 100,
                public_b_chunk_atoms: percolator::MAX_VAULT_TVL,
                maintenance_fee_per_slot,
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
                max_portfolio_assets as usize,
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
        .expect("trade")
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
        .expect("sync maintenance fee")
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

    fn init_matcher_context(
        &mut self,
        matcher_program: Pubkey,
        maker_account: Pubkey,
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
                data: encode_matcher_init_passive(u128::MAX),
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
        .expect("trade cpi")
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
        let key = Pubkey::new_unique();
        self.svm
            .set_account(
                key,
                Account {
                    lamports: 1_000_000_000,
                    data: make_pyth_data(feed, price, expo, 1, publish_time),
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
    assert!(
        liquidation_cu <= 1_350_000,
        "full-14-leg liquidation CU {} exceeded limit {}",
        liquidation_cu,
        1_350_000
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
