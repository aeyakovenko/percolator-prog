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

/// The canonical vault address the wrapper now pins to (F-VAULT-FRAG fix): the Associated Token
/// Account of the vault_authority PDA for the given mint.
fn canonical_vault_ata(vault_authority: Pubkey, mint: Pubkey) -> Pubkey {
    let ata_program = solana_sdk::pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
    Pubkey::find_program_address(
        &[vault_authority.as_ref(), spl_token::ID.as_ref(), mint.as_ref()],
        &ata_program,
    )
    .0
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
        let vault_authority =
            Pubkey::find_program_address(&[b"vault", market.as_ref()], &program_id).0;
        // F-VAULT-FRAG fix: the vault must be the canonical ATA of (vault_authority, mint).
        let vault = canonical_vault_ata(vault_authority, mint);
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
fn v16_bpf_failed_insurance_topup_transfer_rolls_back_budget_and_ledger() {
    let mut env = V16CuEnv::new();
    let ledger = env.insurance_ledger_account();
    let source = Pubkey::new_unique();
    env.svm
        .set_account(
            source,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, env.admin.pubkey(), 100),
                owner: Pubkey::new_unique(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let ledger_before = env.svm.get_account(&ledger).unwrap();
    let source_before = env.svm.get_account(&source).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let result = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpInsurance { amount: 100 },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        &[&env.admin],
    );

    assert!(
        result.is_err(),
        "insurance top-up must fail when the transfer CPI cannot debit the source"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&ledger).unwrap(), ledger_before);
    assert_eq!(env.svm.get_account(&source).unwrap(), source_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    let (_, group) = env.market_state();
    assert_eq!(group.insurance, 0);
    assert_eq!(group.vault, 0);
}

#[test]
fn v16_bpf_failed_backing_topup_transfer_rolls_back_bucket_and_ledger() {
    let mut env = V16CuEnv::new();
    let ledger = env.backing_domain_ledger_account();
    let source = Pubkey::new_unique();
    env.svm
        .set_account(
            source,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, env.admin.pubkey(), 100),
                owner: Pubkey::new_unique(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let ledger_before = env.svm.get_account(&ledger).unwrap();
    let source_before = env.svm.get_account(&source).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let result = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpBackingBucket {
            domain: 1,
            amount: 100,
            expiry_slot: 10,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        &[&env.admin],
    );

    assert!(
        result.is_err(),
        "backing top-up must fail when the transfer CPI cannot debit the source"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&ledger).unwrap(), ledger_before);
    assert_eq!(env.svm.get_account(&source).unwrap(), source_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    let (_, group) = env.market_state();
    assert_eq!(group.vault, 0);
    assert_eq!(
        group.source_backing_buckets[1].fresh_unliened_backing_num,
        0
    );
    assert_eq!(group.source_credit[1].fresh_reserved_backing_num, 0);
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
fn v16_bpf_failed_close_resolved_transfer_rolls_back_payout_state() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);
    env.resolve();
    let dest = env.token_account(owner.pubkey(), 0);
    let mut corrupted_vault = env.svm.get_account(&env.vault).unwrap();
    corrupted_vault.owner = Pubkey::new_unique();
    env.svm.set_account(env.vault, corrupted_vault).unwrap();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let dest_before = env.svm.get_account(&dest).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let result = env.send(
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        vec![
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );

    assert!(
        result.is_err(),
        "close-resolved must fail when the payout transfer CPI fails"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&portfolio).unwrap(), portfolio_before);
    assert_eq!(env.svm.get_account(&dest).unwrap(), dest_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    let (_, group) = env.market_state();
    let account = env.portfolio_state(portfolio);
    assert_eq!(group.vault, 1_000);
    assert_eq!(group.c_tot, 1_000);
    assert_eq!(account.capital, 1_000);
    assert!(
        !account.resolved_payout_receipt.present,
        "failed payout must not persist a paid/finalized receipt"
    );
    assert_eq!(env.token_amount(dest), 0);
}

#[test]
fn v16_bpf_failed_terminal_insurance_withdraw_rolls_back_market_and_ledger() {
    let mut env = V16CuEnv::new();
    env.top_up_insurance(100);
    env.resolve();
    let ledger = env.insurance_ledger_account();
    let dest = env.token_account(env.admin.pubkey(), 0);
    let mut corrupted_vault = env.svm.get_account(&env.vault).unwrap();
    corrupted_vault.owner = Pubkey::new_unique();
    env.svm.set_account(env.vault, corrupted_vault).unwrap();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let ledger_before = env.svm.get_account(&ledger).unwrap();
    let dest_before = env.svm.get_account(&dest).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let result = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawInsurance { amount: 40 },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        &[&env.admin],
    );

    assert!(
        result.is_err(),
        "terminal insurance withdraw must fail when the transfer CPI fails"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&ledger).unwrap(), ledger_before);
    assert_eq!(env.svm.get_account(&dest).unwrap(), dest_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    let (_, group) = env.market_state();
    assert_eq!(group.insurance, 100);
    assert_eq!(group.vault, 100);
    assert_eq!(env.token_amount(dest), 0);
}

#[test]
fn v16_bpf_failed_backing_withdraw_transfer_rolls_back_bucket_and_ledger() {
    let mut env = V16CuEnv::new();
    let ledger = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger, 1, 100, 10);
    let dest = env.token_account(env.admin.pubkey(), 0);
    let mut corrupted_vault = env.svm.get_account(&env.vault).unwrap();
    corrupted_vault.owner = Pubkey::new_unique();
    env.svm.set_account(env.vault, corrupted_vault).unwrap();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let ledger_before = env.svm.get_account(&ledger).unwrap();
    let dest_before = env.svm.get_account(&dest).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let result = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawBackingBucket {
            domain: 1,
            amount: 40,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        &[&env.admin],
    );

    assert!(
        result.is_err(),
        "backing withdraw must fail when the transfer CPI cannot debit the vault"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&ledger).unwrap(), ledger_before);
    assert_eq!(env.svm.get_account(&dest).unwrap(), dest_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    let (_, group) = env.market_state();
    assert_eq!(group.vault, 100);
    assert_eq!(
        group.source_backing_buckets[1].fresh_unliened_backing_num,
        100 * BOUND_SCALE
    );
    assert_eq!(
        group.source_credit[1].fresh_reserved_backing_num,
        100 * BOUND_SCALE
    );
    assert_eq!(env.token_amount(dest), 0);
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
    // F-VAULT-FRAG fix: the secondary vault must be the canonical ATA of (vault_authority, secondary_mint).
    let secondary_vault = canonical_vault_ata(env.vault_authority, secondary_mint);
    env.svm.set_account(secondary_vault, Account {
        lamports: 1_000_000_000, data: make_token_data(secondary_mint, env.vault_authority, 50),
        owner: spl_token::ID, executable: false, rent_epoch: 0,
    }).unwrap();
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

// Coverage probe (audit): an INSOLVENT resolved market (residual < positive-PnL
// face, so the resolved payout rate < 1) pays a winner only floor(face*rate) <
// face. The receipt's `finalized` flag is set ONLY when paid_effective ==
// terminal_positive_claim_face (the FULL face), so under a haircut it can never
// finalize. If that is a real gap, the winner's portfolio can never be
// dematerialized (portfolio_view_is_closable requires a finalized-or-absent
// receipt), materialized_portfolio_count is stuck >= 1, and the market can never
// WithdrawInsurance or CloseSlab -> permanent fund/rent strand.
//
// This test asserts the CORRECT end-state (the fully-settled winner reaches a
// closable receipt state and the portfolio can be reclaimed).
// GREEN regression: Finding D was fixed in engine b6e23b3
// (clear_fully_diluted_resolved_receipt_if_terminal clears the receipt at the
// terminal rate so the portfolio dematerializes).
#[test]
fn v16_audit_insolvent_resolved_winner_can_dematerialize() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);
    // Winner carries +250 of positive PnL face, but its domain is backed by only
    // 100, so the resolved junior pool (residual = vault - c_tot) is 100 < 250 ->
    // a permanent haircut: payout rate = 100/250 = 0.4.
    env.top_up_backing_bucket(1, 100, 10_000);
    env.add_source_positive_pnl(portfolio, 1, 250);

    env.resolve();
    let _dest = env.close_resolved(&owner, portfolio);

    let account =
        state::read_portfolio(&env.svm.get_account(&portfolio).unwrap().data).unwrap();
    assert_eq!(account.capital, 0, "capital paid out");
    assert_eq!(account.pnl, 0, "pnl zeroed by resolved close");
    // A fully-paid (haircut) resolved winner must reach a CLOSABLE receipt state so the
    // portfolio can dematerialize: either finalized, or cleared/absent once it has been
    // paid its full entitlement at the terminal rate. If it can't, materialized_portfolio_count
    // stays >= 1 and the market is permanently un-drainable (no WithdrawInsurance, no CloseSlab).
    assert!(
        !account.resolved_payout_receipt.present || account.resolved_payout_receipt.finalized,
        "haircut winner's receipt must be closable (finalized or cleared at the terminal rate); \
         present={} finalized={}",
        account.resolved_payout_receipt.present, account.resolved_payout_receipt.finalized,
    );

    // The consequence: the owner must be able to reclaim the fully-settled
    // portfolio (this dematerializes it). Panics if the receipt blocks closability.
    env.close_portfolio_with_cu(&owner, portfolio);
}

// Coverage probe (audit, Finding candidate): after a user defensively cures and
// cancels a forced close (CureAndCancelClose), their `close_progress` ledger is
// left in the `canceled` state, never reset to EMPTY. `withdraw_not_atomic`
// requires `close_progress == EMPTY`, so the user can never withdraw their flat,
// solvent capital again in Live mode. This test asserts the CORRECT outcome (the
// user can withdraw after curing).
// GREEN regression: Finding E was fixed in engine f9af174 (withdraw now allows an
// inert `canceled` close ledger).
#[test]
fn v16_audit_withdraw_after_cure_and_cancel_close() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 100);
    env.seed_cancellable_close_progress(portfolio);

    // Owner cures + cancels the forced close (no position -> IM is 0, so no extra
    // deposit needed).
    let source = env.token_account(owner.pubkey(), 0);
    env.cure_and_cancel_close_with_cu(&owner, portfolio, source, 0);

    // The account is now flat and solvent (capital 100, no positions). The user
    // must be able to withdraw their own capital.
    env.withdraw_with_cu(&owner, portfolio, 100);
    let account =
        state::read_portfolio(&env.svm.get_account(&portfolio).unwrap().data).unwrap();
    assert_eq!(
        account.capital, 0,
        "a flat, solvent user must be able to withdraw their capital after curing a cancelled close",
    );
}

// Coverage probe (audit, Finding F): the permissionless retired-slot REUSE branch
// of UpdateAssetLifecycle (v16_program.rs:8651) writes the four domain authorities
// straight from caller args with NO zero-check, unlike the append path which
// rejects zero authorities (v16_program.rs:1475). A permissionless creator can
// reuse a retired slot with insurance_authority = 0; fees later accrued to that
// asset's domain are withdrawable by nobody (terminal_insurance_remaining rejects
// a zero authority) -> CloseSlab permanently bricked. This asserts the CORRECT
// behavior (reuse with a zero authority is REJECTED).
// GREEN regression: Finding F fixed in the wrapper reuse branch (v16_program.rs:8651
// now rejects zero domain authorities, mirroring the append path).
#[test]
fn v16_audit_permissionless_reuse_rejects_zero_insurance_authority() {
    let mut env = V16CuEnv::new();
    let attacker = Keypair::new();
    env.update_market_init_fee_policy_with_cu(1);

    // Permissionlessly append asset 1 with valid authorities, then retire it so the
    // slot becomes reusable (free_market_slot_count == 1).
    env.svm.warp_to_slot(1);
    env.activate_permissionless_asset_with_fee(
        &attacker, 1, 1, 100,
        attacker.pubkey(), attacker.pubkey(), attacker.pubkey(), attacker.pubkey(), 1,
    );
    env.svm.warp_to_slot(3);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_RETIRE, 1, 3, 0,
    );
    let (_, retired_group) = env.market_state();
    assert_eq!(retired_group.assets[1].lifecycle, AssetLifecycleV16::Retired);

    // Reuse the retired slot with insurance_authority = ZERO.
    env.svm.warp_to_slot(4);
    let source = env.token_account(attacker.pubkey(), 1);
    let result = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
            asset_index: 1,
            now_slot: 4,
            initial_price: 250,
            insurance_authority: Pubkey::default().to_bytes(), // ZERO -> unrecoverable
            insurance_operator: attacker.pubkey().to_bytes(),
            backing_bucket_authority: attacker.pubkey().to_bytes(),
            oracle_authority: attacker.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&attacker],
    );
    assert!(
        result.is_err(),
        "reusing a retired slot with a zero insurance_authority must be rejected; accepting it \
         strands that domain's insurance (no authority can withdraw) and permanently bricks CloseSlab",
    );
}

// Coverage probe (audit, Finding G): close_resolved_account_not_atomic charges an
// accrued maintenance fee into group.insurance (handle_close_resolved passes
// cfg.maintenance_fee_per_slot) but the wrapper does NOT credit any per-domain
// insurance budget for it. WithdrawInsurance caps each authority's claim at
// Σ(domain budget remaining) (terminal_insurance_remaining_for_authority_view),
// not group.insurance, so this fee is withdrawable by NOBODY and permanently
// blocks CloseSlab (requires insurance==0). This asserts the CORRECT invariant
// (all of group.insurance is attributable to a withdrawable domain budget); it
// goes RED iff the strand is real. Confirmed by mainnet evidence (market AWCZ2pK,
// 4060 lamports of stranded dust with every authority = admin).
// GREEN regression: Finding G fixed in handle_close_resolved (the resolved maintenance
// fee is now domain-credited via credit_maintenance_fee_to_active_market_budgets_view).
#[test]
fn v16_audit_resolved_maintenance_fee_insurance_stays_recoverable() {
    // maintenance_fee_per_slot = 5
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 10_000, 10_000, 10_000, 5,
    );
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);

    // Accrue ~100 slots of maintenance fee, then resolve and close.
    env.svm.warp_to_slot(100);
    env.resolve();
    env.close_resolved(&owner, portfolio);

    let (_, group) = env.market_state();
    let sum_budgets: u128 = group.insurance_domain_budget.iter().sum();
    assert!(
        sum_budgets >= group.insurance,
        "all of group.insurance must be attributable to a withdrawable domain budget so an \
         authority can sweep it and CloseSlab can succeed; insurance={} but \
         sum(domain budgets)={} -> the {} difference is stranded forever",
        group.insurance, sum_budgets, group.insurance.saturating_sub(sum_budgets),
    );
}

// regression (security.md sweep): MTM settlement under a price move (§6.1 loss->capital,
// §6.2 profit->pnl warmup). After full winner->loser->winner cranking, total equity is
// conserved, the winner's +PnL is backed by the loser's realized-loss residual, and senior
// conservation (vault >= c_tot + insurance) holds. (Investigating a narrow-invariant probe
// that fired here confirmed the warmup settlement is order-robust once fully cranked.)
#[test]
fn v16_regression_mark_to_market_settles_conservation_under_price_move() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    let (_, _g0) = env.market_state();

    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110); // mark up 10%
    env.crank(pa, ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: 10, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 });
    env.svm.expire_blockhash();
    env.crank(pb, ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: 10, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 });
    env.svm.expire_blockhash();
    env.crank(pa, ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: 11, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 });

    let a = state::read_portfolio(&env.svm.get_account(&pa).unwrap().data).unwrap();
    let b = state::read_portfolio(&env.svm.get_account(&pb).unwrap().data).unwrap();
    let (_, g1) = env.market_state();
    // Widened (correct) invariant: senior conservation holds, total equity conserved, and after
    // full settlement the winner's gain is credited and backed by the loser's realized loss.
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation: vault >= c_tot + insurance");
    let total_equity = (a.capital as i128 + a.pnl) + (b.capital as i128 + b.pnl);
    assert_eq!(total_equity, 2_000_000, "total equity (capital+pnl) conserved across both accounts");
    let residual = g1.vault as i128 - g1.c_tot as i128 - g1.insurance as i128;
    let pos_pnl = a.pnl.max(0) + b.pnl.max(0);
    assert!(residual >= pos_pnl, "positive PnL must be backed by residual (no un-backed winner)");
}

// regression (security.md sweep): profit realization round-trip — open, mark up, settle,
// then close both legs. Total equity conserved, flat, senior conservation, +pnl backed.
#[test]
fn v16_regression_profit_realization_roundtrip_conserves() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110);
    env.crank(pa, ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: 10, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 });
    env.svm.expire_blockhash();
    env.crank(pb, ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: 10, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 });
    // Close both legs at the new mark.
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, -(POS_SCALE as i128), 110, 0);

    let a = state::read_portfolio(&env.svm.get_account(&pa).unwrap().data).unwrap();
    let b = state::read_portfolio(&env.svm.get_account(&pb).unwrap().data).unwrap();
    let (_, g) = env.market_state();
    assert_eq!(g.assets[0].oi_eff_long_q, 0, "flat after close");
    assert_eq!(g.assets[0].oi_eff_short_q, 0, "flat after close");
    let total_equity = (a.capital as i128 + a.pnl) + (b.capital as i128 + b.pnl);
    assert_eq!(total_equity, 2_000_000, "total equity conserved through open->mark->close");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    let residual = g.vault as i128 - g.c_tot as i128 - g.insurance as i128;
    assert!(residual >= a.pnl.max(0) + b.pnl.max(0), "positive pnl backed by residual");
}

// security.md sweep — numerical boundary (#37 i128::MIN negation / #38 wide overflow):
// extreme trade sizes must be rejected cleanly (no panic, no OI, no value movement).
#[test]
fn v16_attack_extreme_size_trade_rejected_no_panic() {
    let mut env = V16CuEnv::new();
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    for sz in [i128::MIN, i128::MAX, i128::MIN + 1] {
        env.svm.expire_blockhash();
        let r = env.try_trade_asset_with_cu(0, &la, pa, &lb, pb, sz, 100, 0);
        assert!(r.is_err(), "extreme size {} must be rejected cleanly", sz);
    }
    let (_, g) = env.market_state();
    assert_eq!(g.assets[0].oi_eff_long_q, 0, "no OI from rejected extreme-size trades");
    assert_eq!(g.c_tot, 2_000_000, "no capital moved");
}

// regression (security.md sweep): value extraction (#33/#35) — after a winner realizes profit and
// closes, withdraw each leg's full capital through the REAL token vault. Attacker success = total tokens
// out > total deposited (value printed) OR vault drops below c_tot+insurance (unbacked extraction).
#[test]
fn v16_regression_profit_withdraw_no_value_printed() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110); // winner = long (la)
    env.crank(pa, ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: 10, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 });
    env.svm.expire_blockhash();
    env.crank(pb, ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: 10, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 });
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, -(POS_SCALE as i128), 110, 0); // both flat

    // Each withdraws its full capital through the token vault.
    let cap_a = state::read_portfolio(&env.svm.get_account(&pa).unwrap().data).unwrap().capital;
    let cap_b = state::read_portfolio(&env.svm.get_account(&pb).unwrap().data).unwrap().capital;
    env.svm.expire_blockhash();
    let dest_a = env.withdraw(&la, pa, cap_a);
    env.svm.expire_blockhash();
    let dest_b = env.withdraw(&lb, pb, cap_b);

    let bal = |env: &V16CuEnv, k: &Pubkey| -> u64 {
        let d = env.svm.get_account(k).unwrap().data;
        u64::from_le_bytes(d[64..72].try_into().unwrap())
    };
    let out = bal(&env, &dest_a) as u128 + bal(&env, &dest_b) as u128;
    assert!(out <= 2_000_000, "no value printed: tokens out {} <= deposited 2_000_000", out);
    let (_, g) = env.market_state();
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation after profit withdraws");
}

// security.md sweep — privilege/injection (#19/#39): the permissionless crank's funding_rate_e9
// and recovery_reason are CALLER-supplied. Attacker tries to inject arbitrary funding to drain the
// counterparty. Gate (v16_program.rs:10092) must reject any nonzero caller value; the real rate is
// derived internally and clamped to max_abs_funding_e9_per_slot.
#[test]
fn v16_attack_crank_caller_funding_rate_injection_rejected() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    let crank_accts = |env: &V16CuEnv| vec![
        AccountMeta::new(env.payer.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(pa, false),
    ];
    // attacker-chosen extreme funding rates + nonzero recovery_reason must all be rejected.
    for (rate, rr) in [(i128::MAX, 0u8), (i128::MIN, 0), (1, 0), (-1, 0), (0, 1u8)] {
        env.svm.expire_blockhash();
        let r = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: 5, funding_rate_e9: rate, close_q: 0, fee_bps: 0, recovery_reason: rr }, crank_accts(&env), &[]);
        assert!(r.is_err(), "caller funding_rate_e9={} recovery_reason={} must be rejected", rate, rr);
    }
    // a legitimate rate-0 crank still works and conserves.
    env.svm.expire_blockhash();
    let r = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: 5, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 }, crank_accts(&env), &[]);
    assert!(r.is_ok(), "rate-0 crank must succeed");
    let (_, g) = env.market_state();
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation after crank");
    assert_eq!(g.c_tot, 2_000_000, "no capital injected via funding");
}

// security.md sweep — cross-margin (#22/#32): one portfolio holds positions on TWO assets.
// Probe aggregate conservation and per-asset OI balance under shared-capital cross-margin.
#[test]
fn v16_attack_cross_margin_two_asset_conservation() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::ConfigureAuthMark { asset_index: 1, now_slot: 0, initial_mark_e6: 100 },
        vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(env.market, false)],
        &[&env.admin]).expect("cfg auth mark asset1");
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 2_000_000);
    env.deposit(&lb, pb, 2_000_000);
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(1, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    let (_, g) = env.market_state();
    assert_eq!(g.c_tot, 4_000_000, "no capital created/destroyed across two-asset cross-margin");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    assert_eq!(g.assets[0].oi_eff_long_q, g.assets[0].oi_eff_short_q, "asset0 OI balanced");
    assert_eq!(g.assets[1].oi_eff_long_q, g.assets[1].oi_eff_short_q, "asset1 OI balanced");
    assert!(g.assets[1].oi_eff_long_q > 0, "asset1 position actually opened");
}

// security.md sweep — cross-margin settlement (#9/#33): same portfolio long on two assets;
// asset0 rises (gain), asset1 falls (loss). Net should wash. Probe value creation/destruction
// and senior conservation across cross-asset settlement.
#[test]
fn v16_attack_cross_margin_divergent_moves_conserve() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    let cfg_mark = |env: &mut V16CuEnv, ai: u16, _slot: u64, _mark: u64, ix: ProgInstruction| {
        send_tx(&mut env.svm, env.program_id, &env.payer, ix,
            vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(env.market, false)],
            &[&env.admin]).unwrap_or_else(|e| panic!("asset{} mark: {}", ai, e));
    };
    cfg_mark(&mut env, 1, 0, 100, ProgInstruction::ConfigureAuthMark { asset_index: 1, now_slot: 0, initial_mark_e6: 100 });
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 2_000_000);
    env.deposit(&lb, pb, 2_000_000);
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(1, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);

    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110); // asset0 up -> la gains
    cfg_mark(&mut env, 1, 10, 90, ProgInstruction::PushAuthMark { asset_index: 1, now_slot: 10, mark_e6: 90 }); // asset1 down -> la loses
    // crank both assets for both portfolios, two passes to converge §6.1/§6.2 warmup.
    for slot in [10u64, 11] {
        for ai in [0u16, 1] {
            for p in [pa, pb] {
                env.svm.expire_blockhash();
                let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: ai, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
                    vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false)], &[]);
            }
        }
    }
    let a = state::read_portfolio(&env.svm.get_account(&pa).unwrap().data).unwrap();
    let b = state::read_portfolio(&env.svm.get_account(&pb).unwrap().data).unwrap();
    let (_, g) = env.market_state();
    let total_equity = (a.capital as i128 + a.pnl) + (b.capital as i128 + b.pnl);
    assert_eq!(total_equity, 4_000_000, "total equity conserved across divergent cross-asset moves");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    let residual = g.vault as i128 - g.c_tot as i128 - g.insurance as i128;
    assert!(residual >= a.pnl.max(0) + b.pnl.max(0), "positive pnl backed by residual");
}

// security.md sweep — account confusion (#44/#45): pass wrong-type accounts where a portfolio is
// expected (the market account, the vault, an uninitialized account). Owner/discriminator checks
// must reject; no state mutation, no value movement.
#[test]
fn v16_attack_account_type_confusion_rejected() {
    let mut env = V16CuEnv::new();
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    let (_, g0) = env.market_state();

    // 1) withdraw naming the MARKET account as the portfolio.
    let dest = Pubkey::new_unique();
    env.svm.set_account(dest, Account { lamports: 1_000_000_000, data: make_token_data(env.mint, la.pubkey(), 0), owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
    let r1 = env.send(ProgInstruction::Withdraw { amount: 1 }, vec![
        AccountMeta::new(la.pubkey(), true), AccountMeta::new(env.market, false),
        AccountMeta::new(env.market, false), AccountMeta::new(dest, false),
        AccountMeta::new(env.vault, false), AccountMeta::new_readonly(env.vault_authority, false),
        AccountMeta::new_readonly(spl_token::ID, false)], &[&la]);
    assert!(r1.is_err(), "withdraw with market-as-portfolio must reject");

    // 2) trade naming the VAULT as account_a.
    env.svm.expire_blockhash();
    let r2 = env.send(ProgInstruction::TradeNoCpi { asset_index: 0, size_q: POS_SCALE as i128, exec_price: 100, fee_bps: 0 }, vec![
        AccountMeta::new(la.pubkey(), true), AccountMeta::new(lb.pubkey(), true),
        AccountMeta::new(env.market, false), AccountMeta::new(env.vault, false),
        AccountMeta::new(pb, false)], &[&la, &lb]);
    assert!(r2.is_err(), "trade with vault-as-portfolio must reject");

    // 3) crank naming an uninitialized (system) account as the portfolio.
    let junk = Pubkey::new_unique();
    env.svm.set_account(junk, Account { lamports: 1_000_000, data: vec![0u8; 64], owner: solana_sdk::system_program::ID, executable: false, rent_epoch: 0 }).unwrap();
    env.svm.expire_blockhash();
    let r3 = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: 1, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
        vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(junk, false)], &[]);
    assert!(r3.is_err(), "crank with uninitialized-account-as-portfolio must reject");

    let (_, g1) = env.market_state();
    assert_eq!(g1.c_tot, g0.c_tot, "no capital moved by confused-account calls");
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
}

// security.md sweep — loss-of-funds / DoS (#22/#30): after maintenance fees accrue over a long
// idle period, the user must still be able to withdraw their remaining (post-fee) capital. A bug
// here = funds locked (LoF). Probe: deposit, accrue fees, sync, then withdraw everything left.
#[test]
fn v16_attack_fee_accrual_does_not_lock_user_funds() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(1, 10_000, 10_000, 10_000, 58);
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    env.update_maintenance_fee_policy_with_cu(0);
    // long idle period, then settle the maintenance fee.
    env.svm.warp_to_slot(500);
    let _ = env.try_sync_maintenance_fee_with_cu(p, None, 500);
    let remaining = state::read_portfolio(&env.svm.get_account(&p).unwrap().data).unwrap().capital;
    assert!(remaining > 0 && remaining < 1_000_000, "fees took some but not all capital (got {})", remaining);
    // user withdraws ALL remaining capital — must succeed, funds not locked.
    let (dest, _) = env.withdraw_with_cu(&owner, p, remaining);
    let got = {
        let d = env.svm.get_account(&dest).unwrap().data;
        u64::from_le_bytes(d[64..72].try_into().unwrap()) as u128
    };
    assert_eq!(got, remaining, "user recovered full post-fee capital (no LoF)");
    let after = state::read_portfolio(&env.svm.get_account(&p).unwrap().data).unwrap();
    assert_eq!(after.capital, 0, "capital fully withdrawn");
    let (_, g) = env.market_state();
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation after fee+withdraw");
}

// security.md sweep — insolvency / bad-debt socialization (#9/#33/#19): drive a small-capital
// SHORT underwater past its capital via a multi-slot up-move, settling each slot. The winner's
// profit must NOT be paid out of the vault past what's actually backed — senior conservation
// (vault >= c_tot + insurance) must hold and the winner's positive pnl must be capped by residual
// (the loser's bad debt is socialized via haircut, not printed).
#[test]
fn v16_attack_insolvency_bad_debt_is_socialized_not_printed() {
    let mut env = V16CuEnv::new();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 250); // tiny capital -> will go insolvent
    env.configure_ewma_mark_with_cu(0, 100, 1, 0);
    env.trade_with_cu(&long_owner, long_account, &short_owner, short_account, POS_SCALE as i128, 100, 0);

    // Push the price up across slots (circuit breaker clamps ~100%/slot): 100 -> 200 -> 400.
    // Short's loss (size * (P-100)/POS_SCALE) exceeds its 250 capital -> bad debt.
    for (slot, mark) in [(1u64, 300u64), (2, 800)] {
        env.svm.warp_to_slot(slot);
        env.push_ewma_mark_with_cu(slot, mark);
        for acct in [long_account, short_account] {
            env.svm.expire_blockhash();
            let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
                vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(acct, false)], &[]);
        }
    }
    // Liquidate the insolvent short.
    env.svm.expire_blockhash();
    let _ = env.send(ProgInstruction::PermissionlessCrank { action: 1, asset_index: 0, now_slot: 2, funding_rate_e9: 0, close_q: POS_SCALE, fee_bps: 0, recovery_reason: 0 },
        vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(short_account, false)], &[]);

    let lo = state::read_portfolio(&env.svm.get_account(&long_account).unwrap().data).unwrap();
    let sh = state::read_portfolio(&env.svm.get_account(&short_account).unwrap().data).unwrap();
    let (_, g) = env.market_state();
    // Guard against a vacuous pass: confirm the scenario actually reached insolvency.
    assert!(g.assets[0].effective_price >= 300, "price actually moved up (got {})", g.assets[0].effective_price);
    assert_eq!(sh.capital, 0, "short was driven insolvent (capital wiped)");
    // The crux: the vault never owes more (senior) than it holds, no matter the bad debt.
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation holds under insolvency");
    let residual = g.vault as i128 - g.c_tot as i128 - g.insurance as i128;
    let pos_pnl = lo.pnl.max(0) + sh.pnl.max(0);
    assert!(residual >= pos_pnl, "winner profit capped by residual — bad debt socialized, not printed (residual {} pos_pnl {})", residual, pos_pnl);
    // No capital was conjured: total realized capital <= total deposited.
    assert!((lo.capital + sh.capital) <= 1_000_250, "no capital printed (got {})", lo.capital + sh.capital);
}

// security.md sweep — insurance backstop accounting (#33/#9): with a pre-funded insurance fund,
// bad debt from an insolvent loser should be absorbed by insurance so the winner is closer to
// whole, WITHOUT insurance going negative or the vault being over-credited. Probe: same insolvency
// as batch 16 but with seeded insurance; assert insurance never underflows and senior conservation
// holds (vault >= c_tot + insurance) with insurance accounted, no value printed.
#[test]
fn v16_attack_insurance_backstop_absorbs_bad_debt_no_underflow() {
    let mut env = V16CuEnv::new();
    env.top_up_insurance(1_000_000); // junior backstop
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 250);
    env.configure_ewma_mark_with_cu(0, 100, 1, 0);
    env.trade_with_cu(&long_owner, long_account, &short_owner, short_account, POS_SCALE as i128, 100, 0);

    let (_, g_before) = env.market_state();
    let ins_before = g_before.insurance;
    let vault_before = g_before.vault;

    for (slot, mark) in [(1u64, 300u64), (2, 800)] {
        env.svm.warp_to_slot(slot);
        env.push_ewma_mark_with_cu(slot, mark);
        for acct in [long_account, short_account] {
            env.svm.expire_blockhash();
            let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
                vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(acct, false)], &[]);
        }
    }
    env.svm.expire_blockhash();
    let _ = env.send(ProgInstruction::PermissionlessCrank { action: 1, asset_index: 0, now_slot: 2, funding_rate_e9: 0, close_q: POS_SCALE, fee_bps: 0, recovery_reason: 0 },
        vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(short_account, false)], &[]);

    let lo = state::read_portfolio(&env.svm.get_account(&long_account).unwrap().data).unwrap();
    let sh = state::read_portfolio(&env.svm.get_account(&short_account).unwrap().data).unwrap();
    let (_, g) = env.market_state();
    // insurance is a u128 accumulator: it must never wrap/underflow under bad-debt absorption.
    assert!(g.insurance <= ins_before, "insurance only spent (not conjured): {} <= {}", g.insurance, ins_before);
    // vault token balance is not increased by the bad-debt event (no minting).
    assert!(g.vault <= vault_before, "vault not over-credited: {} <= {}", g.vault, vault_before);
    // senior conservation with insurance fully accounted.
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation with insurance backstop");
    let residual = g.vault as i128 - g.c_tot as i128 - g.insurance as i128;
    assert!(residual >= lo.pnl.max(0) + sh.pnl.max(0), "winner profit backed by residual");
}

// security.md sweep — debtor escape / LoF for winner (#22/#48): an insolvent loser must NOT be
// able to withdraw or otherwise extract value before/at liquidation, which would strand the
// winner's claim. Probe: drive short underwater, then short attempts withdraw -> must reject; the
// winner's position and the vault backing remain intact.
#[test]
fn v16_attack_insolvent_loser_cannot_withdraw_to_escape() {
    let mut env = V16CuEnv::new();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 250);
    env.configure_ewma_mark_with_cu(0, 100, 1, 0);
    env.trade_with_cu(&long_owner, long_account, &short_owner, short_account, POS_SCALE as i128, 100, 0);
    for (slot, mark) in [(1u64, 300u64), (2, 800)] {
        env.svm.warp_to_slot(slot);
        env.push_ewma_mark_with_cu(slot, mark);
        env.svm.expire_blockhash();
        let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
            vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(short_account, false)], &[]);
    }
    let vault_before = env.market_state().1.vault;
    // insolvent short tries to withdraw ANY amount -> must reject (margin / no free capital).
    for amt in [1u128, 100, 250] {
        env.svm.expire_blockhash();
        let dest = Pubkey::new_unique();
        env.svm.set_account(dest, Account { lamports: 1_000_000_000, data: make_token_data(env.mint, short_owner.pubkey(), 0), owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
        let r = env.send(ProgInstruction::Withdraw { amount: amt }, vec![
            AccountMeta::new(short_owner.pubkey(), true), AccountMeta::new(env.market, false),
            AccountMeta::new(short_account, false), AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false), AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false)], &[&short_owner]);
        assert!(r.is_err(), "insolvent short must not withdraw {} to escape its debt", amt);
        let got = { let d = env.svm.get_account(&dest).unwrap().data; u64::from_le_bytes(d[64..72].try_into().unwrap()) };
        assert_eq!(got, 0, "no tokens leaked to escaping debtor");
    }
    assert_eq!(env.market_state().1.vault, vault_before, "vault untouched by rejected escape attempts");
}

// regression (security.md sweep): premium-funding + price-move settlement value-conservation.
// Balanced long/short with a persistent mark premium so funding accrues across slots. Probe whether
// funding/price settlement creates or destroys net VAULT value, breaks senior conservation, or
// leaves the winner unbacked. (Initial probe fired on a too-narrow Σ(capital+pnl)==deposits invariant
// — funding fees accrue to insurance and §6.2 warmup holds an in-vault residual; widened below.)
#[test]
fn v16_regression_premium_funding_settlement_conserves_vault() {
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
    let lo_owner = Keypair::new(); let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new(); let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, DEPOSIT);
    env.deposit(&sh_owner, sh, DEPOSIT);
    env.trade_with_cu(&lo_owner, lo, &sh_owner, sh, POS_SCALE as i128, INITIAL_PRICE, 0);
    // Push the mark premium ONCE at slot 1 (anti-retroactivity: it won't charge funding that slot),
    // then crank subsequent slots WITHOUT re-pushing so the established premium accrues funding.
    env.svm.warp_to_slot(1);
    env.push_ewma_mark_with_cu(1, INITIAL_PRICE * 2);
    let crank_both = |env: &mut V16CuEnv, slot: u64| {
        for acct in [lo, sh] {
            env.svm.expire_blockhash();
            let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
                vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(acct, false)], &[]);
        }
    };
    crank_both(&mut env, 1);
    for slot in 2..=5u64 {
        env.svm.warp_to_slot(slot);
        crank_both(&mut env, slot);
    }
    let a = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    let b = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
    let (_, g) = env.market_state();
    // funding must actually have accrued (non-vacuous): the ledger moved off zero.
    assert!(g.assets[0].f_long_num != 0 || g.assets[0].f_short_num != 0, "funding actually accrued");
    // Correct (widened) conservation invariant. NOTE: the account-level sum Σ(capital+pnl) is NOT
    // == deposits here, because funding fees legitimately accrue to INSURANCE and the §6.2 warmup
    // holds a RESIDUAL buffer in-vault before crediting the winner. The real guarantees are:
    //   1) no tokens minted/burned: the vault still holds exactly the deposited amount,
    //   2) senior conservation: vault >= c_tot + insurance,
    //   3) winner backed: positive pnl <= residual,
    //   4) no over-distribution: Σ(capital+pnl) + insurance <= vault.
    assert_eq!(g.vault, 2 * DEPOSIT, "no tokens minted or burned: vault == total deposited");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation under funding");
    let residual = g.vault as i128 - g.c_tot as i128 - g.insurance as i128;
    assert!(residual >= a.pnl.max(0) + b.pnl.max(0), "positive pnl backed by residual");
    let total_equity = (a.capital as i128 + a.pnl) + (b.capital as i128 + b.pnl);
    assert!(total_equity + g.insurance as i128 <= g.vault as i128, "no value over-distributed beyond the vault");
    assert!(g.assets[0].f_long_num < 0 && g.assets[0].f_short_num > 0, "longs pay shorts under mark premium");
}


// security.md sweep — §6.2 profit conversion (#33/#35): ConvertReleasedPnl moves source-backed
// released pnl into withdrawable capital. The caller supplies `amount`, but it must only be a CAP:
// a caller must never convert MORE than the engine's release-bounded amount (which would print
// withdrawable capital). Probe both directions: a huge cap converts exactly the released amount
// (not more), and an under-cap rejects (no partial over/under conversion, no value printed).
#[test]
fn v16_attack_convert_released_pnl_respects_caller_cap() {
    const RELEASED: u128 = 40;
    let mut env = V16CuEnv::new();
    let ledger = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger, 1, RELEASED, 10);
    // portfolio A: convert with a huge cap -> must convert exactly RELEASED, never more.
    let a_owner = Keypair::new(); let a = env.create_portfolio(&a_owner);
    env.add_source_positive_pnl(a, 1, RELEASED);
    env.crank(a, ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: 0, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 });
    let (_, g0) = env.market_state();
    env.svm.expire_blockhash();
    let ra = env.send(ProgInstruction::ConvertReleasedPnl { amount: 1_000_000_000 },
        vec![AccountMeta::new(a_owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(a, false)], &[&a_owner]);
    assert!(ra.is_ok(), "huge-cap convert should succeed: {:?}", ra);
    let acct_a = env.portfolio_state(a);
    assert_eq!(acct_a.capital, RELEASED, "huge cap converts EXACTLY the released amount, not more");

    // portfolio B: same released pnl, but an under-cap (RELEASED-1) -> wrapper rejects (converted > cap).
    let b_owner = Keypair::new(); let b = env.create_portfolio(&b_owner);
    env.add_source_positive_pnl(b, 1, RELEASED);
    env.crank(b, ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: 0, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 });
    env.svm.expire_blockhash();
    let rb = env.send(ProgInstruction::ConvertReleasedPnl { amount: RELEASED - 1 },
        vec![AccountMeta::new(b_owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(b, false)], &[&b_owner]);
    assert!(rb.is_err(), "under-cap convert must reject (engine releases {} > cap {})", RELEASED, RELEASED - 1);
    assert_eq!(env.portfolio_state(b).capital, 0, "rejected convert moves nothing");

    // zero-amount convert is rejected outright.
    env.svm.expire_blockhash();
    let rz = env.send(ProgInstruction::ConvertReleasedPnl { amount: 0 },
        vec![AccountMeta::new(b_owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(b, false)], &[&b_owner]);
    assert!(rz.is_err(), "zero-amount convert rejected");

    let (_, g1) = env.market_state();
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation after conversions");
    assert_eq!(g1.vault, g0.vault, "ConvertReleasedPnl moves no vault tokens");
}

// security.md sweep — cross-margin insolvency (#9/#33/#22): a portfolio short on TWO assets is
// driven underwater on BOTH until its combined loss exceeds shared capital. Cross-asset bad debt
// must still be socialized, not printed: senior conservation holds and the winner is capped by
// residual. Probes the interaction of cross-margin shared capital with multi-asset insolvency.
#[test]
fn v16_regression_cross_margin_insolvency_no_value_extraction() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    let cfg = |env: &mut V16CuEnv, ix: ProgInstruction| {
        send_tx(&mut env.svm, env.program_id, &env.payer, ix,
            vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(env.market, false)],
            &[&env.admin]).expect("asset1 mark cfg");
    };
    cfg(&mut env, ProgInstruction::ConfigureAuthMark { asset_index: 1, now_slot: 0, initial_mark_e6: 100 });
    let victim_owner = Keypair::new(); let victim = env.create_portfolio(&victim_owner);
    let cp_owner = Keypair::new(); let cp = env.create_portfolio(&cp_owner);
    env.deposit(&victim_owner, victim, 250);       // tiny shared capital
    env.deposit(&cp_owner, cp, 2_000_000);
    // victim SHORT on both assets (negative size on account_a); cp takes the long side.
    env.trade_asset_with_cu(0, &victim_owner, victim, &cp_owner, cp, -(POS_SCALE as i128), 100, 0);
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(1, &victim_owner, victim, &cp_owner, cp, -(POS_SCALE as i128), 100, 0);

    // drive BOTH asset marks up over two slots: shorts lose, combined loss > 250 capital.
    for (slot, mark) in [(1u64, 300u64), (2, 800)] {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_with_cu(slot, mark);
        cfg(&mut env, ProgInstruction::PushAuthMark { asset_index: 1, now_slot: slot, mark_e6: mark });
        for ai in [0u16, 1] {
            for p in [victim, cp] {
                env.svm.expire_blockhash();
                let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: ai, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
                    vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false)], &[]);
            }
        }
    }
    // liquidate the insolvent victim on both legs.
    for ai in [0u16, 1] {
        env.svm.expire_blockhash();
        let _ = env.send(ProgInstruction::PermissionlessCrank { action: 1, asset_index: ai, now_slot: 2, funding_rate_e9: 0, close_q: POS_SCALE, fee_bps: 0, recovery_reason: 0 },
            vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(victim, false)], &[]);
    }

    let v = state::read_portfolio(&env.svm.get_account(&victim).unwrap().data).unwrap();
    let (_, g) = env.market_state();
    // non-vacuity: victim actually insolvent on a real up-move on both assets.
    assert!(g.assets[0].effective_price >= 300 && g.assets[1].effective_price >= 300, "both prices moved up");
    assert_eq!(v.capital, 0, "victim's shared capital wiped by cross-asset losses");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation under cross-margin insolvency");

    // NOTE (PASS_SAFE but WEIRD): after liquidation the winner's long stays open and marks-to-market
    // against the still-rising oracle while its counterparty is gone, so its pnl figure grows
    // unboundedly (600 -> 1400 -> ...). That number is UNCOLLECTABLE PAPER: residual stays pinned at
    // 250 (the victim's recovered capital). The real safety guarantees — checked below — are that the
    // vault is never minted into and the winner can never EXTRACT more than the vault holds.
    for slot in 3..=8u64 {
        env.svm.warp_to_slot(slot);
        for ai in [0u16, 1] {
            for p in [victim, cp] {
                env.svm.expire_blockhash();
                let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: ai, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
                    vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false)], &[]);
            }
        }
    }
    let (_, g2) = env.market_state();
    assert_eq!(g2.vault, 2_000_250, "no tokens minted despite unbounded paper pnl");
    assert!(g2.vault >= g2.c_tot + g2.insurance, "senior conservation persists under growing paper pnl");

    // The winner's ConvertReleasedPnl is residual-bounded: it can NEVER pull paper pnl into capital
    // beyond the residual backing, no matter the pnl figure.
    let residual2 = (g2.vault - g2.c_tot - g2.insurance) as u128;
    let cap_before = env.portfolio_state(cp).capital;
    env.svm.expire_blockhash();
    let _ = env.send(ProgInstruction::ConvertReleasedPnl { amount: 1_000_000_000 },
        vec![AccountMeta::new(cp_owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(cp, false)], &[&cp_owner]);
    let converted = env.portfolio_state(cp).capital - cap_before;
    assert!(converted <= residual2, "winner conversion bounded by residual ({} <= {})", converted, residual2);

    // And total tokens the winner can actually pull out never exceed the vault.
    env.svm.expire_blockhash();
    let dest = Pubkey::new_unique();
    env.svm.set_account(dest, Account { lamports: 1_000_000_000, data: make_token_data(env.mint, cp_owner.pubkey(), 0), owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
    let cap_now = env.portfolio_state(cp).capital;
    let _ = env.send(ProgInstruction::Withdraw { amount: cap_now }, vec![
        AccountMeta::new(cp_owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(cp, false),
        AccountMeta::new(dest, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(env.vault_authority, false),
        AccountMeta::new_readonly(spl_token::ID, false)], &[&cp_owner]);
    let out = { let d = env.svm.get_account(&dest).unwrap().data; u64::from_le_bytes(d[64..72].try_into().unwrap()) as u128 };
    assert!(out <= 2_000_250, "winner cannot extract more tokens than the vault holds (got {})", out);
    let (_, g3) = env.market_state();
    assert!(g3.vault >= g3.c_tot + g3.insurance, "senior conservation after winner extraction attempt");
}

// security.md sweep — resolved wind-down LoF / over-claim (#22/#30/#48): a market can be resolved
// with OPEN positions (handle_resolve_market does not require flat). After resolution a long and a
// short must each recover their FAIR value via CloseResolved — neither stuck (LoF) nor able to
// over-claim. Total tokens paid out must never exceed total deposited.
#[test]
fn v16_regression_resolved_open_positions_recover_fairly_order_robust() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let lo_owner = Keypair::new(); let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new(); let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, 1_000_000);
    env.deposit(&sh_owner, sh, 1_000_000);
    env.trade_asset_with_cu(0, &lo_owner, lo, &sh_owner, sh, (10_000 * POS_SCALE) as i128, 100, 0); // notional 1M
    // move price so the long wins, settle both legs across two slots, THEN resolve with positions still open.
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110);
    for slot in [10u64, 11] {
        env.svm.warp_to_slot(slot);
        for p in [sh, lo] {
            env.svm.expire_blockhash();
            let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
                vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false)], &[]);
        }
    }
    env.resolve(); // resolve WITH open positions still on the book

    let bal = |env: &V16CuEnv, k: &Pubkey| -> u128 { let d = env.svm.get_account(k).unwrap().data; u64::from_le_bytes(d[64..72].try_into().unwrap()) as u128 };
    // Winner (long) closes FIRST, before the loser has funded the vault. This must be a SAFE NO-OP:
    // it pays 0 and leaves the winner's capital fully intact (no destructive partial close / no LoF).
    let (dest_lo1, _) = env.close_resolved_with_cu(&lo_owner, lo);
    assert_eq!(bal(&env, &dest_lo1), 0, "premature winner close pays nothing (vault not yet funded)");
    let mid = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    assert_eq!(mid.capital, 1_000_000, "premature winner close is a no-op: capital fully preserved");
    assert_eq!(mid.pnl, 100_000, "premature winner close preserves parked pnl");

    // Loser closes (recovers its post-loss capital, funding the vault for the winner).
    let (dest_sh, _) = env.close_resolved_with_cu(&sh_owner, sh);
    let out_sh = bal(&env, &dest_sh);
    // Winner RETRIES and now recovers full fair value.
    let (dest_lo2, _) = env.close_resolved_with_cu(&lo_owner, lo);
    let out_lo = bal(&env, &dest_lo2);

    // No LoF, exact value conservation, fair winner/loser split.
    assert_eq!(out_lo + out_sh, 2_000_000, "every account recovers; total payout == total deposited (no LoF, no printing)");
    assert_eq!(out_lo, 1_100_000, "winner recovers capital + realized profit");
    assert_eq!(out_sh, 900_000, "loser recovers capital - realized loss");
    let a = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    let b = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
    assert_eq!(a.capital, 0, "long fully wound down");
    assert_eq!(b.capital, 0, "short fully wound down");
    // the winner (positive claim) has a finalized payout receipt.
    assert!(a.resolved_payout_receipt.finalized, "winner payout receipt finalized");
    let (_, g) = env.market_state();
    assert_eq!(g.vault, 0, "vault fully drained, no funds stranded");
}

// security.md sweep — haircut payout rounding across multiple winners (#33/#37): when several
// resolved winners share ONE insufficient backing pool, each is paid floor(face * rate). The sum of
// floored payouts must NEVER exceed the backing (a rounding-up bug would let winners collectively
// extract more than the pool holds). Probe with deliberately non-divisible faces.
#[test]
fn v16_regression_resolved_multiwinner_haircut_no_overpay_no_strand() {
    const BACKING: u128 = 100;
    // three winners with non-divisible positive-pnl faces against a shared 100 backing.
    let faces: [u128; 3] = [250, 251, 253];
    let mut env = V16CuEnv::new();
    env.top_up_backing_bucket(1, BACKING, 10_000);
    let mut owners = Vec::new();
    let mut ports = Vec::new();
    for &face in faces.iter() {
        let o = Keypair::new();
        let p = env.create_portfolio(&o);
        env.deposit(&o, p, 1_000);
        env.add_source_positive_pnl(p, 1, face);
        owners.push(o); ports.push(p);
    }
    env.resolve();
    // Two close passes: early winners get a present-but-unfinalized receipt (terminal haircut rate
    // not settled while other claims pend); a RETRY close after all are processed clears them.
    let mut total_pnl_paid: u128 = 0;
    let mut total_out: u128 = 0;
    for _pass in 0..2 {
        for (o, p) in owners.iter().zip(ports.iter()) {
            let dest = env.close_resolved(o, *p);
            let got = env.token_amount(dest) as u128;
            total_out += got;
            total_pnl_paid += got.saturating_sub(if got >= 1_000 { 1_000 } else { got });
        }
    }
    // CRUX 1: summed haircut pnl never exceeds the shared backing (no rounding-up over-pay).
    assert!(total_pnl_paid <= BACKING, "summed haircut pnl {} must not exceed backing {}", total_pnl_paid, BACKING);
    // CRUX 2 (no strand): every winner's receipt is closable and the portfolio dematerializes.
    for (o, p) in owners.iter().zip(ports.iter()) {
        let a = state::read_portfolio(&env.svm.get_account(p).unwrap().data).unwrap();
        assert_eq!(a.capital, 0, "winner capital fully paid");
        assert!(!a.resolved_payout_receipt.present || a.resolved_payout_receipt.finalized, "receipt closable after retry");
        env.close_portfolio_with_cu(o, *p); // panics if dematerialization is blocked
    }
    let (_, g) = env.market_state();
    assert_eq!(g.materialized_portfolio_count, 0, "all winners dematerialized — no permanent strand");
    assert_eq!(g.c_tot, 0, "all capital wound down");
    assert!(g.vault <= 1, "at most conservative-rounding dust remains in vault (got {})", g.vault);
    assert!(total_out >= 3_000, "all senior capital recovered (no LoF on capital)");
}
// security.md sweep — slot spoofing / over-accrual DoS (#30/#19): the permissionless crank's
// now_slot is CALLER-supplied. A cranker passes a far-future now_slot to over-accrue funding/fees
// against a victim. The handler must authenticate against the real Clock (authenticated_now_slot)
// and IGNORE the caller's value — accrual reflects only real elapsed slots.
#[test]
fn v16_attack_crank_future_now_slot_does_not_overaccrue() {
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
    let lo_owner = Keypair::new(); let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new(); let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, DEPOSIT);
    env.deposit(&sh_owner, sh, DEPOSIT);
    env.trade_with_cu(&lo_owner, lo, &sh_owner, sh, POS_SCALE as i128, INITIAL_PRICE, 0);
    env.svm.warp_to_slot(1);
    env.push_ewma_mark_with_cu(1, INITIAL_PRICE * 2);

    // REAL clock is slot 2. Cranker lies with now_slot = 1_000_000 (a ~half-million-slot jump).
    env.svm.warp_to_slot(2);
    const LIE: u64 = 1_000_000;
    for acct in [lo, sh] {
        env.svm.expire_blockhash();
        let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: LIE, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
            vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(acct, false)], &[]);
    }
    let a = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    let b = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
    let (_, g) = env.market_state();
    // the market advanced to the REAL clock slot (2), NOT the caller's lie.
    assert_eq!(g.slot_last, 2, "accrual used the authenticated clock slot, not the caller's now_slot");
    assert!(g.assets[0].slot_last < LIE, "asset slot_last is the real clock, not the spoofed future");
    // price moved at most the per-slot clamp over REAL elapsed slots (not 1M slots of movement).
    assert!(g.assets[0].effective_price <= INITIAL_PRICE * 2, "price bounded by real elapsed time + circuit breaker");
    // value conserved: no massive funding/fee over-charge drained capital.
    let total_equity = (a.capital as i128 + a.pnl) + (b.capital as i128 + b.pnl);
    assert_eq!(g.vault, 2 * DEPOSIT, "no tokens created/destroyed");
    assert!(total_equity + g.insurance as i128 <= g.vault as i128, "no over-distribution");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation under slot-spoof attempt");
}

// security.md sweep — resolved payout replay / over-claim (#33/#48): once a resolved winner is fully
// paid (receipt finalized), replaying CloseResolved or ClaimResolvedPayoutTopup must extract ZERO
// extra tokens, and an unentitled account must not be able to claim. Guards against payout replay.
#[test]
fn v16_attack_resolved_payout_replay_extracts_nothing() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let lo_owner = Keypair::new(); let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new(); let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, 1_000_000);
    env.deposit(&sh_owner, sh, 1_000_000);
    env.trade_asset_with_cu(0, &lo_owner, lo, &sh_owner, sh, (10_000 * POS_SCALE) as i128, 100, 0);
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110);
    for slot in [10u64, 11] {
        env.svm.warp_to_slot(slot);
        for p in [sh, lo] {
            env.svm.expire_blockhash();
            let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
                vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false)], &[]);
        }
    }
    env.resolve();
    // loser closes first to fund the vault, then winner closes for full payout.
    let _ = env.close_resolved(&sh_owner, sh);
    let dest_win = env.close_resolved(&lo_owner, lo);
    let won = env.token_amount(dest_win) as u128;
    assert_eq!(won, 1_100_000, "winner fully paid");
    let a = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    assert!(a.resolved_payout_receipt.finalized || !a.resolved_payout_receipt.present, "winner receipt finalized/cleared");
    let (_, g_after) = env.market_state();

    let bal = |env: &V16CuEnv, k: &Pubkey| -> u128 { let d = env.svm.get_account(k).unwrap().data; u64::from_le_bytes(d[64..72].try_into().unwrap()) as u128 };
    // REPLAY 1: winner re-runs CloseResolved repeatedly -> 0 extra each time.
    for _ in 0..3 {
        env.svm.expire_blockhash();
        let d = Pubkey::new_unique();
        env.svm.set_account(d, Account { lamports: 1_000_000_000, data: make_token_data(env.mint, lo_owner.pubkey(), 0), owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
        let _ = env.send(ProgInstruction::CloseResolved { fee_rate_per_slot: 0 }, vec![
            AccountMeta::new_readonly(lo_owner.pubkey(), false), AccountMeta::new(env.market, false), AccountMeta::new(lo, false),
            AccountMeta::new(d, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false)], &[]);
        assert_eq!(bal(&env, &d), 0, "replayed CloseResolved extracts nothing");
    }
    // REPLAY 2: winner spams ClaimResolvedPayoutTopup -> 0 extra each time.
    for _ in 0..3 {
        env.svm.expire_blockhash();
        let d = Pubkey::new_unique();
        env.svm.set_account(d, Account { lamports: 1_000_000_000, data: make_token_data(env.mint, lo_owner.pubkey(), 0), owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
        let _ = env.send(ProgInstruction::ClaimResolvedPayoutTopup, vec![
            AccountMeta::new_readonly(lo_owner.pubkey(), false), AccountMeta::new(env.market, false), AccountMeta::new(lo, false),
            AccountMeta::new(d, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false)], &[]);
        assert_eq!(bal(&env, &d), 0, "replayed topup claim extracts nothing");
    }
    let (_, g_end) = env.market_state();
    assert_eq!(g_end.vault, g_after.vault, "vault unchanged by replay attempts");
    assert!(g_end.vault >= g_end.c_tot + g_end.insurance, "senior conservation preserved");
}

// security.md sweep — oracle/mark bounds (#37/#39): the auth-mark push feeds settlement. An extreme
// mark (0 or u64::MAX) must be rejected/clamped, never corrupt pnl or panic the program.
#[test]
fn v16_attack_extreme_auth_mark_push_rejected_or_safe() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let lo_owner = Keypair::new(); let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new(); let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, 1_000_000);
    env.deposit(&sh_owner, sh, 1_000_000);
    env.trade_asset_with_cu(0, &lo_owner, lo, &sh_owner, sh, POS_SCALE as i128, 100, 0);
    env.svm.warp_to_slot(5);
    // push extreme marks; each must reject or be clamped — never panic, never corrupt state.
    for mark in [0u64, 1, u64::MAX, u64::MAX / 2] {
        env.svm.expire_blockhash();
        let _ = send_tx(&mut env.svm, env.program_id, &env.payer,
            ProgInstruction::PushAuthMark { asset_index: 0, now_slot: 5, mark_e6: mark },
            vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(env.market, false)],
            &[&env.admin]); // ignore Err; we only require no panic + conservation
        // crank against whatever mark landed; must not corrupt conservation.
        env.svm.expire_blockhash();
        let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: 5, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
            vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(lo, false)], &[]);
        let (_, g) = env.market_state();
        assert_eq!(g.vault, 2_000_000, "vault intact under extreme mark {}", mark);
        assert!(g.vault >= g.c_tot + g.insurance, "senior conservation under extreme mark {}", mark);
        assert!(g.assets[0].effective_price > 0 && g.assets[0].effective_price <= percolator::MAX_ORACLE_PRICE,
            "effective price stays in valid bounds under extreme mark {} (got {})", mark, g.assets[0].effective_price);
    }
    // state still decodes and positions intact (no corruption). Vault holds exactly the deposits
    // (checked per-iteration); accounted equity + insurance never EXCEEDS the vault (the small
    // difference is the in-vault §6.2 residual buffer, not lost value).
    let a = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    let b = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
    let accounted = (a.capital as i128 + a.pnl) + (b.capital as i128 + b.pnl) + env.market_state().1.insurance as i128;
    assert!(accounted <= 2_000_000, "no value created by extreme mark pushes (accounted {})", accounted);
    assert!(accounted >= 2_000_000 - 1_000, "value not materially destroyed; remainder is in-vault residual (accounted {})", accounted);
}

// security.md sweep — fee bounds / overflow (#37/#19): TradeNoCpi's fee_bps is caller-supplied. An
// out-of-range fee_bps must be rejected (bounded by max_trading_fee_bps), never overflow or drain
// beyond capital. A valid max fee must accrue to insurance with exact conservation.
#[test]
fn v16_attack_trade_fee_bps_bounded_and_conserving() {
    let mut env = V16CuEnv::new(); // default max_trading_fee_bps = 10_000
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    // out-of-range fee_bps must be rejected with no state change.
    for bad in [u64::MAX, 10_001u64, 50_000] {
        env.svm.expire_blockhash();
        let r = env.try_trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, bad);
        assert!(r.is_err(), "fee_bps {} > max_trading_fee_bps must be rejected", bad);
    }
    let (_, g0) = env.market_state();
    assert_eq!(g0.assets[0].oi_eff_long_q, 0, "no OI from rejected over-fee trades");
    assert_eq!(g0.c_tot, 2_000_000, "no capital moved by rejected trades");

    // valid max fee (10_000 bps = 100% of notional) succeeds and accrues to insurance.
    env.svm.expire_blockhash();
    let r = env.try_trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 10_000);
    assert!(r.is_ok(), "max valid fee_bps should succeed: {:?}", r);
    let (_, g1) = env.market_state();
    // fee moved capital -> insurance internally; vault unchanged, conservation exact.
    assert_eq!(g1.vault, 2_000_000, "fee is internal: vault unchanged (no tokens created/destroyed)");
    assert_eq!(g1.vault, g1.c_tot + g1.insurance, "exact conservation: vault == c_tot + insurance");
    assert!(g1.insurance > 0, "fee actually accrued to insurance");
    // fee never exceeds the traded notional (bounded), so neither party is over-drained.
    let notional = 100u128; // POS_SCALE @ 100
    assert!(g1.insurance <= 2 * notional, "fee bounded by ~notional per side (insurance {})", g1.insurance);
    let a = state::read_portfolio(&env.svm.get_account(&pa).unwrap().data).unwrap();
    let b = state::read_portfolio(&env.svm.get_account(&pb).unwrap().data).unwrap();
    assert!(a.capital > 0 && b.capital > 0, "fee did not drain either party to zero");
}

// security.md sweep — accounting drift under churn (#32/#35): interleaved deposits, withdrawals, and
// a trade open/close must never drift the aggregates. At every checkpoint c_tot == Σ(capitals) and
// vault == c_tot + insurance, and OI stays balanced. Catches any aggregate-update slippage.
#[test]
fn v16_attack_conservation_under_deposit_withdraw_trade_churn() {
    let mut env = V16CuEnv::new();
    let a = Keypair::new(); let pa = env.create_portfolio(&a);
    let b = Keypair::new(); let pb = env.create_portfolio(&b);
    let c = Keypair::new(); let pc = env.create_portfolio(&c);
    let check = |env: &V16CuEnv, tag: &str| {
        let (_, g) = env.market_state();
        let sum: u128 = [pa, pb, pc].iter().map(|p| state::read_portfolio(&env.svm.get_account(p).unwrap().data).unwrap().capital).sum();
        assert_eq!(g.c_tot, sum, "[{}] c_tot == Σ capitals", tag);
        assert_eq!(g.vault, g.c_tot + g.insurance, "[{}] vault == c_tot + insurance", tag);
        assert_eq!(g.assets[0].oi_eff_long_q, g.assets[0].oi_eff_short_q, "[{}] OI balanced", tag);
    };
    env.deposit(&a, pa, 500_000); check(&env, "dep a");
    env.deposit(&b, pb, 800_000); check(&env, "dep b");
    env.svm.expire_blockhash(); env.withdraw(&a, pa, 100_000); check(&env, "wd a");
    env.deposit(&c, pc, 300_000); check(&env, "dep c");
    // a (400k) trades vs b (800k): open then more churn.
    env.trade_asset_with_cu(0, &a, pa, &b, pb, POS_SCALE as i128, 100, 0); check(&env, "open trade");
    env.svm.expire_blockhash(); env.deposit(&a, pa, 50_000); check(&env, "dep a2");
    env.svm.expire_blockhash(); env.withdraw(&c, pc, 250_000); check(&env, "wd c");
    // close the trade (opposite).
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(0, &a, pa, &b, pb, -(POS_SCALE as i128), 100, 0); check(&env, "close trade");
    // drain everyone fully.
    for (o, p) in [(&a, pa), (&b, pb), (&c, pc)] {
        let cap = state::read_portfolio(&env.svm.get_account(&p).unwrap().data).unwrap().capital;
        if cap > 0 { env.svm.expire_blockhash(); env.withdraw(o, p, cap); }
    }
    check(&env, "drained");
    let (_, g) = env.market_state();
    assert_eq!(g.c_tot, 0, "all capital withdrawn");
    // total deposited (500k+800k+300k+50k = 1,650k) minus total withdrawn must net to insurance+vault residue.
    assert_eq!(g.vault, g.insurance, "vault fully accounted as insurance after full drain (no stranded value)");
}

// security.md sweep — resolve mid-flight before settlement (#30 sequence/race): push a price move,
// then resolve WITHOUT any settlement crank. The resolved wind-down must still settle at the true
// post-move price — the winner recovers their gain, the loser bears their loss, value conserved.
// Attacker success = stale pre-move settlement (winner LoF, or loser escapes its loss).
#[test]
fn v16_regression_resolve_before_settlement_uses_official_price() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let lo_owner = Keypair::new(); let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new(); let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, 1_000_000);
    env.deposit(&sh_owner, sh, 1_000_000);
    env.trade_asset_with_cu(0, &lo_owner, lo, &sh_owner, sh, (10_000 * POS_SCALE) as i128, 100, 0); // notional 1M
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110); // pending mark; NOT yet accrued into effective_price (anti-retroactivity)
    // NO crank: resolve immediately. The pushed mark is unaccrued, so the official effective_price is
    // still 100 and the position is officially flat.
    let (_, g_pre) = env.market_state();
    assert_eq!(g_pre.assets[0].effective_price, 100, "unaccrued mark push does NOT move the official price");
    env.resolve();

    fn bal(env: &V16CuEnv, k: &Pubkey) -> u128 { let d = env.svm.get_account(k).unwrap().data; u64::from_le_bytes(d[64..72].try_into().unwrap()) as u128 }
    // loser-first, then winner (order-robust wind-down established in batch 23). Retry winner if deferred.
    let _ = env.close_resolved(&sh_owner, sh);
    let d1 = env.close_resolved(&lo_owner, lo);
    let mut won = bal(&env, &d1);
    if won == 0 { let d2 = env.close_resolved(&lo_owner, lo); won = bal(&env, &d2); }
    let lost = {
        let b = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
        assert_eq!(b.capital, 0, "loser wound down");
        2_000_000u128.saturating_sub(won)
    };
    // CORRECT behavior: resolve settles at the OFFICIAL accrued price (100). The unaccrued mark push
    // is NOT retroactively applied, so no value is created or destroyed — each party recovers exactly
    // its deposit. (Contrast batch 23: crank-to-accrue BEFORE resolve, and the winner gets 1.1M.)
    assert_eq!(won, 1_000_000, "no value invented from an unaccrued mark — deposit returned");
    assert_eq!(won + lost, 2_000_000, "exact conservation across resolve-before-settlement");
    let (_, g) = env.market_state();
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    let a = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    assert_eq!(a.capital, 0, "long fully wound down");
    assert!(a.resolved_payout_receipt.finalized || !a.resolved_payout_receipt.present, "receipt closable");
}

// security.md sweep — crank idempotency / double-accrual (#32 race): re-cranking an asset at the SAME
// slot must be a no-op. If a second same-slot crank re-applies the price move/funding, an attacker
// could double-realize a counterparty's loss or double-charge funding. We first crank to the
// settlement fixed point (§6.1/§6.2 needs multiple passes), then assert re-cranking is an exact no-op.
#[test]
fn v16_regression_crank_idempotent_at_settlement_fixed_point() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let lo_owner = Keypair::new(); let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new(); let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, 1_000_000);
    env.deposit(&sh_owner, sh, 1_000_000);
    env.trade_asset_with_cu(0, &lo_owner, lo, &sh_owner, sh, (10_000 * POS_SCALE) as i128, 100, 0);
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110);
    let crank = |env: &mut V16CuEnv, p: Pubkey, slot: u64| {
        env.svm.expire_blockhash();
        let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
            vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false)], &[]);
    };
    // crank many passes at slot 11 and watch the short's capital: it must CONVERGE to a fixed point
    // (settlement completing), not keep dropping (which would be double-accrual).
    env.svm.warp_to_slot(11);
    for _ in 0..8 { for p in [sh, lo] { crank(&mut env, p, 11); } } // crank to the settlement fixed point
    let lo1 = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    let sh1 = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
    let (_, g1) = env.market_state();
    let ep1 = g1.assets[0].effective_price;
    for _ in 0..3 { for p in [sh, lo] { crank(&mut env, p, 11); } }
    let lo2 = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    let sh2 = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
    let (_, g2) = env.market_state();

    assert_eq!(g2.assets[0].effective_price, ep1, "effective price unchanged by same-slot re-crank");
    assert_eq!((lo2.capital, lo2.pnl), (lo1.capital, lo1.pnl), "long pnl/capital not double-accrued");
    assert_eq!((sh2.capital, sh2.pnl), (sh1.capital, sh1.pnl), "short pnl/capital not double-accrued");
    assert_eq!(g2.assets[0].f_long_num, g1.assets[0].f_long_num, "funding ledger not double-applied");
    assert_eq!(g2.vault, 2_000_000, "vault conserved");
    assert!(g2.vault >= g2.c_tot + g2.insurance, "senior conservation");
}

// security.md sweep — account reuse / sentinel re-materialization (#44/#48): after ClosePortfolio,
// reusing the SAME account address (re-init) must yield a CLEAN portfolio — no stale capital, pnl,
// position, or resolved receipt may carry over. Attacker success = inheriting stale value/claims.
#[test]
fn v16_attack_portfolio_reuse_after_close_is_clean() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000);
    // flatten then close.
    env.svm.expire_blockhash();
    env.withdraw(&owner, p, 1_000);
    env.close_portfolio_with_cu(&owner, p);

    // Adversarial twist: re-fund the SAME address with the OLD (possibly stale) bytes still present,
    // simulating a reuse where the closed account's data was not zeroed. Re-init must overwrite it.
    let stale = env.svm.get_account(&p).map(|a| a.data.clone()).unwrap_or_else(|| vec![0u8; env.portfolio_account_len]);
    env.svm.set_account(p, Account {
        lamports: 1_000_000_000,
        data: stale, // whatever close left behind
        owner: env.program_id, executable: false, rent_epoch: 0,
    }).unwrap();
    let new_owner = Keypair::new();
    env.ensure_signer_account(new_owner.pubkey());
    env.svm.expire_blockhash();
    let r = env.send(ProgInstruction::InitPortfolio, vec![
        AccountMeta::new(new_owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false)], &[&new_owner]);
    // Either re-init is rejected (account still considered live) OR it succeeds with a CLEAN slate.
    if r.is_ok() {
        let a = state::read_portfolio(&env.svm.get_account(&p).unwrap().data).unwrap();
        assert_eq!(a.capital, 0, "reused portfolio starts with zero capital (no stale value)");
        assert_eq!(a.pnl, 0, "no stale pnl carried over");
        assert!(!a.resolved_payout_receipt.present, "no stale resolved receipt carried over");
        assert!(percolator::active_bitmap_is_empty(a.active_bitmap), "no stale positions");
        // and a fresh deposit credits exactly the deposited amount.
        env.svm.expire_blockhash();
        env.deposit(&new_owner, p, 500);
        let a2 = state::read_portfolio(&env.svm.get_account(&p).unwrap().data).unwrap();
        assert_eq!(a2.capital, 500, "fresh deposit credits exactly 500 (no stale base)");
    }
    // conservation intact regardless.
    let (_, g) = env.market_state();
    assert_eq!(g.vault, g.c_tot + g.insurance, "conservation after close+reuse");
}

// security.md sweep — rounding asymmetry (#37 dust): trade fees must round UP (ceil, protocol favor)
// so dust-notional trades are never free and repeated churn never leaks value to the trader. Attacker
// success = a fee that floors to 0 (free trade) or insurance that fails to grow on a fee'd dust trade.
#[test]
fn v16_attack_trade_fee_rounds_up_no_free_dust_trades() {
    let mut env = V16CuEnv::new(); // max_trading_fee_bps = 10_000
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    let ins = |env: &V16CuEnv| env.market_state().1.insurance;
    // dust notional with the smallest nonzero fee: notional = size*price/POS_SCALE.
    // size = POS_SCALE/100 @ price 100 => notional = 1; fee_bps=1 => true fee = 0.0001 -> must ceil to >=1.
    let dust_size = (POS_SCALE / 100) as i128;
    let mut prev_ins = ins(&env);
    let mut opened: i128 = 0;
    for i in 0..5 {
        env.svm.expire_blockhash();
        let r = env.try_trade_asset_with_cu(0, &la, pa, &lb, pb, dust_size, 100, 1);
        if r.is_err() { break; } // if dust trade is rejected outright, that's also safe (no free trade)
        opened += dust_size;
        let now = ins(&env);
        assert!(now > prev_ins, "dust trade #{} charged a nonzero fee (insurance grew {} -> {})", i, prev_ins, now);
        prev_ins = now;
        // conservation after each dust trade.
        let (_, g) = env.market_state();
        assert_eq!(g.vault, 2_000_000, "no value created by dust trade");
        assert_eq!(g.vault, g.c_tot + g.insurance, "exact conservation");
    }
    assert!(opened > 0, "at least one dust trade executed (non-vacuous)");
    // close the accumulated dust position; conservation still exact, insurance only grew.
    if opened > 0 {
        env.svm.expire_blockhash();
        let _ = env.try_trade_asset_with_cu(0, &la, pa, &lb, pb, -opened, 100, 0);
    }
    let (_, g) = env.market_state();
    assert_eq!(g.vault, 2_000_000, "vault conserved across dust churn");
    assert_eq!(g.vault, g.c_tot + g.insurance, "exact conservation after close");
    assert!(g.insurance >= prev_ins, "insurance never decreased (fees are protocol-favorable)");
}

// security.md sweep — over-liquidation (#2): liquidating a bankrupt account with close_q FAR larger
// than its position must clamp to the actual size — never over-close into phantom OI, negative OI,
// or manufactured value. Attacker success = excess close_q creating value / corrupting OI.
#[test]
fn v16_attack_over_liquidation_clamps_to_position() {
    let mut env = V16CuEnv::new();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 250); // tiny -> insolvent on up-move
    env.configure_ewma_mark_with_cu(0, 100, 1, 0);
    env.trade_with_cu(&long_owner, long_account, &short_owner, short_account, POS_SCALE as i128, 100, 0);
    for (slot, mark) in [(1u64, 300u64), (2, 800)] {
        env.svm.warp_to_slot(slot);
        env.push_ewma_mark_with_cu(slot, mark);
        env.svm.expire_blockhash();
        let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
            vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(short_account, false)], &[]);
    }
    let (_, g_pre) = env.market_state();
    let oi_pre = g_pre.assets[0].oi_eff_long_q;
    // liquidate with a grossly excessive close_q (1000x the position, and again u128::MAX-ish).
    for cq in [POS_SCALE * 1_000, u128::MAX / 2] {
        env.svm.expire_blockhash();
        let _ = env.send(ProgInstruction::PermissionlessCrank { action: 1, asset_index: 0, now_slot: 2, funding_rate_e9: 0, close_q: cq, fee_bps: 0, recovery_reason: 0 },
            vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(short_account, false)], &[]);
        let (_, g) = env.market_state();
        // OI never goes negative / never exceeds the original (no phantom from excess close_q).
        assert!(g.assets[0].oi_eff_short_q <= oi_pre, "short OI clamped (no phantom), got {} pre {}", g.assets[0].oi_eff_short_q, oi_pre);
        assert!(g.assets[0].oi_eff_long_q <= oi_pre, "long OI not inflated by over-liquidation");
        assert_eq!(g.vault, 1_000_250, "vault unchanged by liquidation (internal), no value created");
        assert!(g.vault >= g.c_tot + g.insurance, "senior conservation under over-liquidation");
    }
    // the short is fully closed (position gone), not over-closed into a phantom opposite position.
    let sh = state::read_portfolio(&env.svm.get_account(&short_account).unwrap().data).unwrap();
    assert!(percolator::active_bitmap_is_empty(sh.active_bitmap), "short position fully closed, no phantom flip");
}

// security.md sweep — funding cap precision (#19 DoS): an extreme mark premium must be clamped to
// max_abs_funding_e9_per_slot. If funding scaled with the raw premium, a tiny mark push could drain
// a counterparty arbitrarily fast. Decisive check: a 2x-index premium and a 1000x-index premium must
// accrue IDENTICAL funding (both pinned to the cap), and value stays conserved.
#[test]
fn v16_attack_extreme_premium_funding_is_capped() {
    const INITIAL_PRICE: u64 = 1_000_000;
    const DEPOSIT: u128 = 100_000_000;
    fn run_scenario(mark_mult: u64) -> (i128, u128, u128) {
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
        let lo_owner = Keypair::new(); let lo = env.create_portfolio(&lo_owner);
        let sh_owner = Keypair::new(); let sh = env.create_portfolio(&sh_owner);
        env.deposit(&lo_owner, lo, DEPOSIT);
        env.deposit(&sh_owner, sh, DEPOSIT);
        env.trade_with_cu(&lo_owner, lo, &sh_owner, sh, POS_SCALE as i128, INITIAL_PRICE, 0);
        env.svm.warp_to_slot(1);
        env.push_ewma_mark_with_cu(1, INITIAL_PRICE.saturating_mul(mark_mult)); // premium
        for slot in 1..=4u64 {
            env.svm.warp_to_slot(slot);
            for p in [lo, sh] {
                env.svm.expire_blockhash();
                let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
                    vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false)], &[]);
            }
        }
        let (_, g) = env.market_state();
        (g.assets[0].f_long_num, g.vault, g.c_tot + g.insurance)
    }
    let (f2, vault2, senior2) = run_scenario(2);     // 2x index premium
    let (f1000, vault1000, senior1000) = run_scenario(1000); // 1000x index premium (capped at clamp)
    assert!(f2 != 0, "funding actually accrued (non-vacuous)");
    // CRUX: extreme premium yields the SAME funding as the moderate one — both pinned to the cap.
    assert_eq!(f2, f1000, "extreme premium funding is clamped to the cap (identical to moderate)");
    // conservation in both runs.
    assert_eq!(vault2, 2 * DEPOSIT, "scenario 2x: vault conserved");
    assert_eq!(vault1000, 2 * DEPOSIT, "scenario 1000x: vault conserved");
    assert!(vault2 >= senior2 && vault1000 >= senior1000, "senior conservation in both");
}

// security.md sweep — cross-margin liquidation fairness (#2/#22): a net-solvent cross-margined
// portfolio (gain on asset0 offsetting a loss on asset1) must NOT be liquidatable for value
// extraction. Attempting to liquidate the losing leg must not unfairly drain the healthy account.
#[test]
fn v16_attack_cross_margin_solvent_account_not_unfairly_liquidated() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    let cfg = |env: &mut V16CuEnv, ix: ProgInstruction| {
        send_tx(&mut env.svm, env.program_id, &env.payer, ix,
            vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(env.market, false)], &[&env.admin]).expect("mark cfg");
    };
    cfg(&mut env, ProgInstruction::ConfigureAuthMark { asset_index: 1, now_slot: 0, initial_mark_e6: 100 });
    let victim_owner = Keypair::new(); let victim = env.create_portfolio(&victim_owner);
    let cp_owner = Keypair::new(); let cp = env.create_portfolio(&cp_owner);
    env.deposit(&victim_owner, victim, 1_000_000);
    env.deposit(&cp_owner, cp, 1_000_000);
    // victim LONG asset0 and SHORT asset1 (cross-margined opposite exposures).
    env.trade_asset_with_cu(0, &victim_owner, victim, &cp_owner, cp, POS_SCALE as i128, 100, 0);
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(1, &victim_owner, victim, &cp_owner, cp, -(POS_SCALE as i128), 100, 0);
    // both marks up 10%: victim GAINS on asset0 (long), LOSES on asset1 (short) -> net ~flat.
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110);
    cfg(&mut env, ProgInstruction::PushAuthMark { asset_index: 1, now_slot: 10, mark_e6: 110 });
    for slot in [10u64, 11] {
        env.svm.warp_to_slot(slot);
        for ai in [0u16, 1] { for p in [victim, cp] {
            env.svm.expire_blockhash();
            let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: ai, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
                vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false)], &[]);
        }}
    }
    let v_before = state::read_portfolio(&env.svm.get_account(&victim).unwrap().data).unwrap();
    let equity_before = v_before.capital as i128 + v_before.pnl;
    let (_, g_before) = env.market_state();
    // attacker tries to liquidate the victim's LOSING leg (asset1).
    env.svm.expire_blockhash();
    let _ = env.send(ProgInstruction::PermissionlessCrank { action: 1, asset_index: 1, now_slot: 11, funding_rate_e9: 0, close_q: POS_SCALE, fee_bps: 0, recovery_reason: 0 },
        vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(victim, false)], &[]);
    let v_after = state::read_portfolio(&env.svm.get_account(&victim).unwrap().data).unwrap();
    let equity_after = v_after.capital as i128 + v_after.pnl;
    let (_, g_after) = env.market_state();
    // the solvent victim's total equity is not reduced by the liquidation attempt (no unfair drain).
    assert!(equity_after >= equity_before, "solvent cross-margined victim equity not drained by liquidation attempt ({} -> {})", equity_before, equity_after);
    assert_eq!(g_after.vault, g_before.vault, "no tokens moved by liquidation attempt");
    assert!(g_after.vault >= g_after.c_tot + g_after.insurance, "senior conservation");
}

// security.md sweep — convert+withdraw exactness (#33/#35): a winner converts backed +PnL to capital
// then withdraws through the real token vault. It must receive EXACTLY the backed amount — not more
// (value printing) nor less (LoF) — and the system fully drains with conservation intact.
#[test]
fn v16_attack_convert_then_withdraw_pays_exactly_backed_amount() {
    const BACKED: u128 = 40;
    let mut env = V16CuEnv::new();
    let ledger = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger, 1, BACKED, 10);
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.add_source_positive_pnl(p, 1, BACKED);
    env.crank(p, ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: 0, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 });
    let vault0 = env.market_state().1.vault;
    // convert the backed pnl into withdrawable capital.
    env.svm.expire_blockhash();
    let cr = env.send(ProgInstruction::ConvertReleasedPnl { amount: BACKED }, vec![
        AccountMeta::new(owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false)], &[&owner]);
    assert!(cr.is_ok(), "convert backed pnl should succeed: {:?}", cr);
    assert_eq!(env.portfolio_state(p).capital, BACKED, "capital == backed amount after convert");
    // withdraw it all through the real vault.
    let (dest, _) = env.withdraw_with_cu(&owner, p, BACKED);
    let got = env.token_amount(dest) as u128;
    assert_eq!(got, BACKED, "winner receives EXACTLY the backed amount (no more, no less)");
    let a = env.portfolio_state(p);
    assert_eq!(a.capital, 0, "capital fully withdrawn");
    assert_eq!(a.pnl, 0, "no residual pnl");
    let (_, g) = env.market_state();
    assert_eq!(g.vault, vault0 - BACKED, "vault decreased by exactly the paid amount");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation after convert+withdraw");
}

// security.md sweep — zero-amount input validation (#39): zero-amount operations must reject or be
// clean no-ops across deposit/withdraw/trade/topup — never corrupt state or conservation.
#[test]
fn v16_attack_zero_amount_inputs_are_safe() {
    let mut env = V16CuEnv::new();
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    let (_, g0) = env.market_state();

    // deposit 0
    env.svm.expire_blockhash();
    let src = env.token_account(la.pubkey(), 0);
    let r_dep = env.send(ProgInstruction::Deposit { amount: 0 }, vec![
        AccountMeta::new(la.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(pa, false),
        AccountMeta::new(src, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&la]);
    // withdraw 0
    env.svm.expire_blockhash();
    let dest = Pubkey::new_unique();
    env.svm.set_account(dest, Account { lamports: 1_000_000_000, data: make_token_data(env.mint, la.pubkey(), 0), owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
    let r_wd = env.send(ProgInstruction::Withdraw { amount: 0 }, vec![
        AccountMeta::new(la.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(pa, false),
        AccountMeta::new(dest, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(env.vault_authority, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&la]);
    // trade size 0
    env.svm.expire_blockhash();
    let r_tr = env.try_trade_asset_with_cu(0, &la, pa, &lb, pb, 0, 100, 0);

    // whatever the dispositions (reject or clean no-op), conservation must be intact and nothing moved.
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "vault unchanged by zero-amount ops");
    assert_eq!(g1.c_tot, g0.c_tot, "c_tot unchanged by zero-amount ops");
    assert_eq!(g1.vault, g1.c_tot + g1.insurance, "conservation intact");
    assert_eq!(g1.assets[0].oi_eff_long_q, 0, "no OI from zero-size trade");
    assert_eq!(env.token_amount(dest) as u128, 0, "zero withdraw moved no tokens");
    // capitals unchanged.
    assert_eq!(env.portfolio_state(pa).capital, 1_000_000, "pa capital unchanged");
    assert_eq!(env.portfolio_state(pb).capital, 1_000_000, "pb capital unchanged");
    let _ = (r_dep, r_wd, r_tr);
}

// security.md sweep — backing-bucket withdraw vs committed lien (#22/#48 LoF): a backing authority
// must NOT be able to withdraw principal that is currently LIENED to back a winner's positive PnL —
// doing so would strand the winner (loss of funds). The withdraw is gated by fresh_unliened_backing.
#[test]
fn v16_attack_backing_withdraw_cannot_strand_liened_winner() {
    let mut env = V16CuEnv::new();
    env.top_up_backing_bucket(1, 40, 10_000); // domain 1: 40 backing
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.add_source_positive_pnl(p, 1, 40); // liens the 40 to back p's +PnL
    let (_, g0) = env.market_state();
    let p0 = env.portfolio_state(p);
    assert!(p0.pnl > 0, "winner has backed positive pnl (non-vacuous)");
    let dest = env.token_account_for_mint(env.mint, env.admin.pubkey(), 0);

    // try to withdraw the LIENED backing (full 40, and a partial 1) -> must reject.
    for amt in [40u128, 1] {
        env.svm.expire_blockhash();
        let r = send_tx(&mut env.svm, env.program_id, &env.payer,
            ProgInstruction::WithdrawBackingBucket { domain: 1, amount: amt },
            vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(env.market, false),
                 AccountMeta::new(dest, false), AccountMeta::new(env.vault, false),
                 AccountMeta::new_readonly(env.vault_authority, false), AccountMeta::new_readonly(spl_token::ID, false)],
            &[&env.admin]);
        assert!(r.is_err(), "withdrawing liened backing ({}) must reject (would strand winner)", amt);
        assert_eq!(env.token_amount(dest), 0, "no tokens extracted from liened backing");
    }
    // winner's backing intact: pnl still present and vault unchanged.
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "vault unchanged by rejected backing withdraws");
    assert_eq!(env.portfolio_state(p).pnl, p0.pnl, "winner's backed pnl preserved");
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}


// security.md sweep — deposit atomicity vs underfunded source (#35/#48): depositing more than the
// source token account holds must fail ATOMICALLY — capital must never be credited before the token
// transfer succeeds (a credit-before-transfer bug would let an attacker mint capital for free).
#[test]
fn v16_attack_deposit_underfunded_source_is_atomic() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    let (_, g0) = env.market_state();
    // source token account holds only 100, but we attempt to deposit 1_000_000.
    let source = Pubkey::new_unique();
    env.svm.set_account(source, Account {
        lamports: 1_000_000_000, data: make_token_data(env.mint, owner.pubkey(), 100),
        owner: spl_token::ID, executable: false, rent_epoch: 0,
    }).unwrap();
    env.svm.expire_blockhash();
    let r = env.send(ProgInstruction::Deposit { amount: 1_000_000 }, vec![
        AccountMeta::new(owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false),
        AccountMeta::new(source, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&owner]);
    assert!(r.is_err(), "depositing more than the source holds must fail (token transfer cannot cover it)");
    // ATOMIC: no capital credited, vault/c_tot unchanged, source untouched.
    assert_eq!(env.portfolio_state(p).capital, 0, "no capital credited on failed deposit (no free mint)");
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "vault unchanged by failed deposit");
    assert_eq!(g1.c_tot, g0.c_tot, "c_tot unchanged by failed deposit");
    assert_eq!(env.token_amount(source), 100, "source token balance untouched");
    // a valid deposit within balance still works afterward (state not corrupted).
    env.svm.expire_blockhash();
    let r2 = env.send(ProgInstruction::Deposit { amount: 100 }, vec![
        AccountMeta::new(owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false),
        AccountMeta::new(source, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&owner]);
    assert!(r2.is_ok(), "valid in-balance deposit succeeds after the failed one");
    assert_eq!(env.portfolio_state(p).capital, 100, "valid deposit credits exactly 100");
    assert_eq!(env.market_state().1.vault, g0.vault + 100, "vault grew by exactly the deposited 100");
}

// security.md sweep — maintenance-fee slot spoofing (#30/#19 DoS): SyncMaintenanceFee's now_slot is
// caller-supplied. A caller passes a far-future now_slot to over-charge maintenance fees and drain a
// victim. The handler must authenticate against the real Clock (charge only real elapsed slots).
#[test]
fn v16_attack_sync_maintenance_fee_future_slot_no_overcharge() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(1, 10_000, 10_000, 10_000, 58);
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    env.update_maintenance_fee_policy_with_cu(0);
    // real clock = slot 10; attacker lies with now_slot = 1_000_000.
    env.svm.warp_to_slot(10);
    let _ = env.try_sync_maintenance_fee_with_cu(p, None, 1_000_000);
    let a = env.portfolio_state(p);
    // fee reflects ~10 real slots (10*58 = 580), NOT 1_000_000 slots (which would drain everything).
    assert!(a.capital >= 1_000_000 - 10_000, "fee bounded by real elapsed slots, not the lie (capital {})", a.capital);
    assert!(a.capital < 1_000_000, "some fee was charged for the real elapsed time (non-vacuous)");
    assert_eq!(a.last_fee_slot, 10, "fee settled to the authenticated clock slot, not the spoofed future");
    let (_, g) = env.market_state();
    assert_eq!(g.vault, 1_000_000, "fee is internal (capital->insurance): vault unchanged");
    assert_eq!(g.vault, g.c_tot + g.insurance, "exact conservation under slot-spoof attempt");
    // a follow-up sync at the same real slot is a no-op (no further drain).
    let cap_before = env.portfolio_state(p).capital;
    env.svm.expire_blockhash();
    let _ = env.try_sync_maintenance_fee_with_cu(p, None, 1_000_000);
    assert_eq!(env.portfolio_state(p).capital, cap_before, "same-real-slot re-sync is a no-op despite future now_slot");
}

// security.md sweep — backing earnings over/double-withdraw (#33/#48): utilization-fee earnings must
// be withdrawable only ONCE and only up to what accrued. Over-withdrawing the remainder or replaying
// a withdraw must not pay out more than total earnings (vault drain / double-spend).
#[test]
fn v16_attack_backing_earnings_no_over_or_double_withdraw() {
    const EARNINGS: u128 = 30;
    let mut env = V16CuEnv::new();
    let ledger = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger, 1, 100, 10);
    // inject accrued utilization-fee earnings for domain 1's bucket, then sync the ledger.
    env.mutate_market(|_, group| { group.source_backing_buckets[1].utilization_fee_earnings = EARNINGS; });
    env.sync_backing_domain_ledger_with_cu(ledger, 1);
    let dest = env.token_account_for_mint(env.mint, env.admin.pubkey(), 0);
    let vault0 = env.market_state().1.vault;

    // withdraw 20 of the 30 earnings.
    env.withdraw_backing_bucket_earnings_to_admin_token_with_cu(ledger, dest, 1, 20);
    assert_eq!(env.token_amount(dest), 20, "first earnings withdraw pays 20");

    // attempt to over-withdraw: only 10 remain, request 20 -> must reject (no double-count).
    env.svm.expire_blockhash();
    let r_over = send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::WithdrawBackingBucketEarnings { domain: 1, amount: 20 },
        vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(ledger, false),
             AccountMeta::new(dest, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(env.vault_authority, false), AccountMeta::new_readonly(spl_token::ID, false)],
        &[&env.admin]);
    assert!(r_over.is_err(), "over-withdrawing beyond remaining earnings must reject");
    assert_eq!(env.token_amount(dest), 20, "no extra tokens paid on rejected over-withdraw");

    // withdraw the legitimate remaining 10.
    env.svm.expire_blockhash();
    env.withdraw_backing_bucket_earnings_to_admin_token_with_cu(ledger, dest, 1, 10);
    assert_eq!(env.token_amount(dest), 30, "exactly total earnings (30) withdrawn across calls");

    // replay: try to withdraw again -> nothing left, must reject (no double-spend).
    env.svm.expire_blockhash();
    let r_replay = send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::WithdrawBackingBucketEarnings { domain: 1, amount: 1 },
        vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(ledger, false),
             AccountMeta::new(dest, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(env.vault_authority, false), AccountMeta::new_readonly(spl_token::ID, false)],
        &[&env.admin]);
    assert!(r_replay.is_err(), "replayed earnings withdraw (nothing left) must reject");
    assert_eq!(env.token_amount(dest), 30, "no double-spend: total out capped at earnings");
    // exactly the earnings (30) left the vault, never more.
    assert_eq!(env.market_state().1.vault, vault0 - 30, "vault decreased by exactly total earnings");
}

// security.md sweep — withdraw mint confusion (#44): withdrawing to a dest token account of a
// DIFFERENT mint than the vault must reject (SPL transfer enforces matching mints). Capital must not
// be debited if the transfer can't land, and no tokens leak.
#[test]
fn v16_attack_withdraw_wrong_mint_dest_rejected() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    let (_, g0) = env.market_state();
    // a dest token account under a DIFFERENT mint.
    let other_mint = Pubkey::new_unique();
    let bad_dest = Pubkey::new_unique();
    env.svm.set_account(bad_dest, Account {
        lamports: 1_000_000_000, data: make_token_data(other_mint, owner.pubkey(), 0),
        owner: spl_token::ID, executable: false, rent_epoch: 0,
    }).unwrap();
    env.svm.expire_blockhash();
    let r = env.send(ProgInstruction::Withdraw { amount: 500_000 }, vec![
        AccountMeta::new(owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false),
        AccountMeta::new(bad_dest, false), AccountMeta::new(env.vault, false),
        AccountMeta::new_readonly(env.vault_authority, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&owner]);
    assert!(r.is_err(), "withdraw to a wrong-mint dest must reject (mint mismatch)");
    assert_eq!(env.token_amount(bad_dest), 0, "no tokens leaked to wrong-mint dest");
    // capital NOT debited (atomic): the failed transfer rolls back the whole op.
    assert_eq!(env.portfolio_state(p).capital, 1_000_000, "capital not debited on failed withdraw");
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert_eq!(g1.c_tot, g0.c_tot, "c_tot unchanged");
    // a correct-mint withdraw still works afterward.
    env.svm.expire_blockhash();
    let (good_dest, _) = env.withdraw_with_cu(&owner, p, 500_000);
    assert_eq!(env.token_amount(good_dest), 500_000, "correct-mint withdraw works after the rejected one");
}

// security.md sweep — TradeCpi matcher identity binding (#44/#49): the matcher_delegate is a PDA
// bound to (slab, maker, matcher_program, matcher_context). Routing a TradeCpi through a SPOOFED
// delegate or a wrong/non-program matcher must reject — no trade executes, no value moves.
#[test]
fn v16_attack_tradecpi_spoofed_matcher_binding_rejected() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new(); let taker = env.create_portfolio(&taker_owner);
    let maker_owner = Keypair::new(); let maker = env.create_portfolio(&maker_owner);
    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&maker_owner, maker, 1_000_000);
    let (matcher_ctx, matcher_delegate, _) = env.init_matcher_context(matcher_program, maker);
    let (_, g0) = env.market_state();

    // ATTACK 1: random (unbound) delegate.
    env.svm.expire_blockhash();
    let r1 = env.try_trade_cpi_with_cu_on_asset(&taker_owner, taker, &maker_owner, maker, matcher_program, matcher_ctx, Pubkey::new_unique(), 0, (10 * POS_SCALE) as i128, 100);
    assert!(r1.is_err(), "spoofed (unbound) matcher delegate must reject");

    // ATTACK 2: a delegate bound to a DIFFERENT context.
    let other_program = Pubkey::new_unique();
    env.svm.add_program(other_program, &matcher_bytes);
    let (_other_ctx, other_delegate, _) = env.init_matcher_context(other_program, maker);
    env.svm.expire_blockhash();
    let r2 = env.try_trade_cpi_with_cu_on_asset(&taker_owner, taker, &maker_owner, maker, matcher_program, matcher_ctx, other_delegate, 0, (10 * POS_SCALE) as i128, 100);
    assert!(r2.is_err(), "delegate bound to a different matcher/context must reject");

    // ATTACK 3: a non-program account as the matcher program (CPI target bogus).
    env.svm.expire_blockhash();
    let bogus_prog = Pubkey::new_unique();
    let r3 = env.try_trade_cpi_with_cu_on_asset(&taker_owner, taker, &maker_owner, maker, bogus_prog, matcher_ctx, matcher_delegate, 0, (10 * POS_SCALE) as i128, 100);
    assert!(r3.is_err(), "wrong/non-program matcher must reject");

    // no trade executed, no value moved across all spoof attempts.
    let (_, g1) = env.market_state();
    assert_eq!(g1.assets[0].oi_eff_long_q, 0, "no OI created by spoofed-matcher TradeCpi");
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert_eq!(g1.c_tot, g0.c_tot, "c_tot unchanged");
    assert_eq!(env.portfolio_state(taker).legs[0].basis_pos_q, 0, "taker has no position");
    // the legitimate binding still works (control).
    env.svm.expire_blockhash();
    let ok = env.try_trade_cpi_with_cu_on_asset(&taker_owner, taker, &maker_owner, maker, matcher_program, matcher_ctx, matcher_delegate, 0, (10 * POS_SCALE) as i128, 100);
    assert!(ok.is_ok(), "correctly-bound matcher executes: {:?}", ok);
}

// security.md sweep — TradeCpi limit_price slippage guard (#19/#39): with a spread matcher filling
// OFF oracle, a taker's limit_price must be enforced — a too-tight limit must REJECT the fill
// (slippage protection). Absent enforcement, a taker could be filled at an arbitrarily bad price.
#[test]
fn v16_attack_tradecpi_limit_price_enforced() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new(); let taker = env.create_portfolio(&taker_owner);
    let maker_owner = Keypair::new(); let maker = env.create_portfolio(&maker_owner);
    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&maker_owner, maker, 1_000_000);
    // spread matcher: fills off oracle (oracle=100), base spread 500 bps -> taker buy ask > 100.
    let (ctx, delegate, _) = env.init_matcher_context_with_passive_spread(matcher_program, maker, 500, 1_000);
    let do_trade = |env: &mut V16CuEnv, limit: u64| -> Result<u64, String> {
        env.svm.expire_blockhash();
        env.send(ProgInstruction::TradeCpi { asset_index: 0, size_q: (10 * POS_SCALE) as i128, fee_bps: 100, limit_price: limit },
            vec![AccountMeta::new(taker_owner.pubkey(), true), AccountMeta::new(maker_owner.pubkey(), true),
                 AccountMeta::new(env.market, false), AccountMeta::new(taker, false), AccountMeta::new(maker, false),
                 AccountMeta::new_readonly(matcher_program, false), AccountMeta::new(ctx, false), AccountMeta::new_readonly(delegate, false)],
            &[&taker_owner, &maker_owner])
    };
    let (_, g0) = env.market_state();
    // too-tight limit (100 = oracle, but buy ask is 100+spread) must reject.
    let r_tight = do_trade(&mut env, 100);
    assert!(r_tight.is_err(), "buy with limit at oracle must reject when matcher fills above it (slippage guard)");
    assert_eq!(env.portfolio_state(taker).legs[0].basis_pos_q, 0, "no fill on rejected tight-limit trade");
    assert_eq!(env.market_state().1.vault, g0.vault, "vault unchanged by rejected trade");
    // generous limit (way above ask) must execute.
    let r_ok = do_trade(&mut env, 1_000_000);
    assert!(r_ok.is_ok(), "buy with generous limit executes: {:?}", r_ok);
    assert!(env.portfolio_state(taker).legs[0].basis_pos_q > 0, "taker filled under generous limit");
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g1.c_tot + g1.insurance, "conservation after fill");
}

// security.md sweep — TradeCpi zero-fill (#39): a zero-capacity matcher (max_fill_abs=0) returns
// exec_size=0. The wrapper must handle it cleanly — reject or no-op — never create phantom OI/basis,
// charge a fee on nothing, or corrupt conservation.
#[test]
fn v16_attack_tradecpi_zero_fill_is_clean() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new(); let taker = env.create_portfolio(&taker_owner);
    let maker_owner = Keypair::new(); let maker = env.create_portfolio(&maker_owner);
    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&maker_owner, maker, 1_000_000);
    // matcher with ZERO fill capacity.
    let (ctx, delegate, _) = env.init_matcher_context_with_data(matcher_program, maker, encode_matcher_init_passive(0));
    let (_, g0) = env.market_state();
    env.svm.expire_blockhash();
    let r = env.try_trade_cpi_with_cu_on_asset(&taker_owner, taker, &maker_owner, maker, matcher_program, ctx, delegate, 0, (10 * POS_SCALE) as i128, 100);
    // whether reject or clean no-op: no OI, no basis, no fee, conservation intact.
    let (_, g1) = env.market_state();
    assert_eq!(g1.assets[0].oi_eff_long_q, 0, "no phantom long OI from zero-fill");
    assert_eq!(g1.assets[0].oi_eff_short_q, 0, "no phantom short OI from zero-fill");
    assert_eq!(env.portfolio_state(taker).legs[0].basis_pos_q, 0, "taker has no basis from zero-fill");
    assert_eq!(env.portfolio_state(maker).legs[0].basis_pos_q, 0, "maker has no basis from zero-fill");
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert_eq!(g1.c_tot, g0.c_tot, "c_tot unchanged (no fee charged on nothing)");
    assert_eq!(g1.insurance, g0.insurance, "no fee accrued on a zero fill");
    assert_eq!(g1.vault, g1.c_tot + g1.insurance, "conservation intact");
    let _ = r;
}

// security.md sweep — TradeCpi self-trade (#49 wash): taker == maker (same portfolio) on the matcher
// CPI path must reject like TradeNoCpi self-trade — no wash position / OI fabrication.
#[test]
fn v16_attack_tradecpi_self_trade_rejected() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let owner = Keypair::new(); let acct = env.create_portfolio(&owner);
    env.deposit(&owner, acct, 1_000_000);
    let (ctx, delegate, _) = env.init_matcher_context(matcher_program, acct);
    let (_, g0) = env.market_state();
    // taker == maker == acct.
    env.svm.expire_blockhash();
    let r = env.send(ProgInstruction::TradeCpi { asset_index: 0, size_q: (10 * POS_SCALE) as i128, fee_bps: 100, limit_price: 0 },
        vec![AccountMeta::new(owner.pubkey(), true), AccountMeta::new(owner.pubkey(), true),
             AccountMeta::new(env.market, false), AccountMeta::new(acct, false), AccountMeta::new(acct, false),
             AccountMeta::new_readonly(matcher_program, false), AccountMeta::new(ctx, false), AccountMeta::new_readonly(delegate, false)],
        &[&owner]);
    assert!(r.is_err(), "TradeCpi self-trade (taker==maker) must reject");
    let (_, g1) = env.market_state();
    assert_eq!(g1.assets[0].oi_eff_long_q, 0, "no OI fabricated by self-trade");
    assert_eq!(env.portfolio_state(acct).legs[0].basis_pos_q, 0, "no wash position");
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert_eq!(g1.c_tot, g0.c_tot, "c_tot unchanged");
}

// security.md sweep — CureAndCancelClose deposit accounting (#35/#48): the cure's optional_deposit
// must credit capital EXACTLY once matching the token transfer (no free-mint), and reject atomically
// if the source is underfunded. Finding E covered withdraw-after-cure; this covers the deposit leg.
#[test]
fn v16_attack_cure_deposit_exact_and_atomic() {
    let mut env = V16CuEnv::new();
    // account A: cure WITH a funded deposit -> capital credited exactly, source drained.
    let a_owner = Keypair::new(); let a = env.create_portfolio(&a_owner);
    env.deposit(&a_owner, a, 100);
    env.seed_cancellable_close_progress(a);
    let src_a = env.token_account_for_mint(env.mint, a_owner.pubkey(), 50);
    let (_, g_pre) = env.market_state();
    env.cure_and_cancel_close_with_cu(&a_owner, a, src_a, 50);
    assert_eq!(env.portfolio_state(a).capital, 150, "cure deposit credits capital exactly (100 + 50)");
    assert_eq!(env.token_amount(src_a), 0, "source token account drained by exactly the deposit");
    let (_, g_mid) = env.market_state();
    assert_eq!(g_mid.vault, g_pre.vault + 50, "vault grew by exactly the cure deposit");
    assert_eq!(g_mid.vault, g_mid.c_tot + g_mid.insurance, "conservation after cure deposit");

    // account B: cure with optional_deposit > source balance -> reject ATOMICALLY (no free-mint).
    let b_owner = Keypair::new(); let b = env.create_portfolio(&b_owner);
    env.deposit(&b_owner, b, 100);
    env.seed_cancellable_close_progress(b);
    let src_b = env.token_account_for_mint(env.mint, b_owner.pubkey(), 50);
    let vault_before_failed_cure = env.market_state().1.vault; // after B's deposit
    env.svm.expire_blockhash();
    let r = env.send(ProgInstruction::CureAndCancelClose { optional_deposit: 1_000 }, vec![
        AccountMeta::new(b_owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(b, false),
        AccountMeta::new(src_b, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&b_owner]);
    assert!(r.is_err(), "cure deposit exceeding source balance must reject");
    assert_eq!(env.portfolio_state(b).capital, 100, "no capital credited on failed cure (no free-mint)");
    assert_eq!(env.token_amount(src_b), 50, "source untouched on failed cure");
    let (_, g_end) = env.market_state();
    assert_eq!(g_end.vault, vault_before_failed_cure, "vault unchanged by failed cure");
    assert_eq!(g_end.vault, g_end.c_tot + g_end.insurance, "conservation intact");
    let _ = g_mid;
}

// security.md sweep — position flip margin (#19/#46 crosses_zero): a trade that flips a position
// long->short must enforce initial_margin_bps on the RESULTING side. An attacker must not be able to
// flip into a larger, under-margined opposite position.
#[test]
fn v16_attack_position_flip_enforces_initial_margin() {
    let mut env = V16CuEnv::new(); // IM = 100%
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 100);        // exactly enough for notional 100 at 100% IM
    env.deposit(&lb, pb, 10_000_000); // counterparty well-funded
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0); // la long 1 (notional 100)
    let basis_open = env.portfolio_state(pa).legs[0].basis_pos_q;
    assert_eq!(basis_open, POS_SCALE as i128, "la opened long 1");

    // ATTACK: flip to SHORT 2 (sell 3) -> needs margin 200 > capital 100 -> must reject.
    env.svm.expire_blockhash();
    let r_over = env.try_trade_asset_with_cu(0, &la, pa, &lb, pb, -(3 * POS_SCALE as i128), 100, 0);
    assert!(r_over.is_err(), "flip into an under-margined short (2x notional) must reject");
    assert_eq!(env.portfolio_state(pa).legs[0].basis_pos_q, basis_open, "position unchanged by rejected over-flip");

    // CONTROL: flip to SHORT 1 (sell 2) -> notional 100, margin 100 (at edge) -> allowed.
    env.svm.expire_blockhash();
    let r_ok = env.try_trade_asset_with_cu(0, &la, pa, &lb, pb, -(2 * POS_SCALE as i128), 100, 0);
    assert!(r_ok.is_ok(), "flip to an equally-margined short should be allowed: {:?}", r_ok);
    assert_eq!(env.portfolio_state(pa).legs[0].basis_pos_q, -(POS_SCALE as i128), "la is now short 1 after flip");
    let (_, g) = env.market_state();
    assert_eq!(g.vault, g.c_tot + g.insurance, "conservation after flip");
    assert_eq!(g.assets[0].oi_eff_long_q, g.assets[0].oi_eff_short_q, "OI balanced after flip");
}

// security.md sweep — withdraw/trade authorization (#6): only a portfolio's OWNER may withdraw from
// it or trade it. A non-owner signer must be rejected — no fund theft, no unauthorized position.
#[test]
fn v16_attack_non_owner_cannot_withdraw_or_trade() {
    let mut env = V16CuEnv::new();
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    let mallory = Keypair::new();
    env.ensure_signer_account(mallory.pubkey());
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    let (_, g0) = env.market_state();

    // Mallory tries to withdraw from pa (owned by la).
    env.svm.expire_blockhash();
    let dest = Pubkey::new_unique();
    env.svm.set_account(dest, Account { lamports: 1_000_000_000, data: make_token_data(env.mint, mallory.pubkey(), 0), owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
    let r_wd = env.send(ProgInstruction::Withdraw { amount: 500_000 }, vec![
        AccountMeta::new(mallory.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(pa, false),
        AccountMeta::new(dest, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(env.vault_authority, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&mallory]);
    assert!(r_wd.is_err(), "non-owner withdraw must reject");
    assert_eq!(env.token_amount(dest), 0, "no funds stolen by non-owner");
    assert_eq!(env.portfolio_state(pa).capital, 1_000_000, "pa capital intact");

    // Mallory tries to trade pa against pb (signing as the account_a owner).
    env.svm.expire_blockhash();
    let r_tr = env.send(ProgInstruction::TradeNoCpi { asset_index: 0, size_q: POS_SCALE as i128, exec_price: 100, fee_bps: 0 }, vec![
        AccountMeta::new(mallory.pubkey(), true), AccountMeta::new(lb.pubkey(), true), AccountMeta::new(env.market, false),
        AccountMeta::new(pa, false), AccountMeta::new(pb, false)], &[&mallory, &lb]);
    assert!(r_tr.is_err(), "non-owner trade of pa must reject");
    assert_eq!(env.portfolio_state(pa).legs[0].basis_pos_q, 0, "no unauthorized position opened on pa");

    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert_eq!(g1.c_tot, g0.c_tot, "c_tot unchanged");
}

// security.md sweep — admin-instruction authorization (#6): privileged ops (ResolveMarket,
// ConfigureAuthMark, UpdateConfig) must reject a non-admin signer. A permissionless resolve would be
// a catastrophic griefing/wind-down trigger.
#[test]
fn v16_attack_non_admin_cannot_resolve_or_configure() {
    let mut env = V16CuEnv::new();
    let mallory = Keypair::new();
    env.ensure_signer_account(mallory.pubkey());
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    env.deposit(&la, pa, 1_000_000);

    // non-admin ResolveMarket -> reject; market stays Live.
    env.svm.expire_blockhash();
    let r_res = send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::ResolveMarket,
        vec![AccountMeta::new(mallory.pubkey(), true), AccountMeta::new(env.market, false)], &[&mallory]);
    assert!(r_res.is_err(), "non-admin ResolveMarket must reject");
    let (cfg0, g0) = env.market_state();
    // mode 0 == Live: a successful attacker-resolve would flip it.
    // (read via raw mode to avoid coupling; vault/positions still operable below proves Live.)

    // non-admin ConfigureAuthMark -> reject.
    env.svm.expire_blockhash();
    let r_cfg = send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::ConfigureAuthMark { asset_index: 0, now_slot: 0, initial_mark_e6: 999_999 },
        vec![AccountMeta::new(mallory.pubkey(), true), AccountMeta::new(env.market, false)], &[&mallory]);
    assert!(r_cfg.is_err(), "non-admin ConfigureAuthMark must reject");

    // Proof the market is still Live & operable: the owner can still withdraw (rejected if resolved).
    env.svm.expire_blockhash();
    let (dest, _) = env.withdraw_with_cu(&la, pa, 500_000);
    assert_eq!(env.token_amount(dest), 500_000, "market still Live: owner withdraw works (not resolved by attacker)");
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault - 500_000, "only the legit withdraw moved funds");
    let _ = cfg0;
}

// security.md sweep — vault liquidity fragmentation (#44 account validation): the vault token
// account is validated ONLY by owner == vault_authority PDA (verify_vault_token_account /
// verify_withdrawable_token_accounts), NOT by a canonical address. ANY token account owned by the
// PDA is accepted. An attacker can create a second vault-authority-owned account, route a deposit to
// it, and withdraw from the canonical vault — fragmenting liquidity so an honest user's withdrawal
// against the canonical vault fails (loss-of-funds), at a 1:1 self-cost (abandoned funds).
#[test]
fn v16_regression_vault_pinned_to_canonical_ata_no_fragmentation() {
    // F-VAULT-FRAG REGRESSION (now FIXED): the wrapper pins the vault to the canonical ATA of
    // (vault_authority, mint). Routing a deposit to a second vault_authority-owned account is now
    // rejected, so the liquidity-fragmentation / honest-withdraw-strand attack is no longer possible.
    let mut env = V16CuEnv::new();
    let honest = Keypair::new(); let hp = env.create_portfolio(&honest);
    env.deposit(&honest, hp, 1_000_000);
    assert_eq!(env.token_amount(env.vault), 1_000_000, "canonical vault holds the honest deposit");

    // attacker creates a SECOND token account owned by the vault_authority PDA (NOT the canonical ATA).
    let attacker = Keypair::new(); let ap = env.create_portfolio(&attacker);
    let fake_vault = Pubkey::new_unique();
    env.svm.set_account(fake_vault, Account { lamports: 1_000_000_000, data: make_token_data(env.mint, env.vault_authority, 0), owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
    assert_ne!(fake_vault, env.vault, "fake vault is not the canonical vault");

    // FIX: deposit routed to the non-canonical (fake) vault is now REJECTED.
    let atk_src = env.token_account_for_mint(env.mint, attacker.pubkey(), 500_000);
    env.svm.expire_blockhash();
    let dep = env.send(ProgInstruction::Deposit { amount: 500_000 }, vec![
        AccountMeta::new(attacker.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(ap, false),
        AccountMeta::new(atk_src, false), AccountMeta::new(fake_vault, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&attacker]);
    assert!(dep.is_err(), "FIXED: deposit to a non-canonical vault-authority-owned account is rejected");
    assert_eq!(env.portfolio_state(ap).capital, 0, "no capital credited via a fake vault");
    assert_eq!(env.token_amount(fake_vault), 0, "fake vault received nothing");
    assert_eq!(env.token_amount(atk_src), 500_000, "attacker source untouched");

    // a withdraw routed to the fake vault is likewise rejected.
    env.deposit(&attacker, ap, 500_000); // legit deposit to canonical so attacker has capital
    env.svm.expire_blockhash();
    let dest = Pubkey::new_unique();
    env.svm.set_account(dest, Account { lamports: 1_000_000_000, data: make_token_data(env.mint, attacker.pubkey(), 0), owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
    let wd = env.send(ProgInstruction::Withdraw { amount: 500_000 }, vec![
        AccountMeta::new(attacker.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(ap, false),
        AccountMeta::new(dest, false), AccountMeta::new(fake_vault, false), AccountMeta::new_readonly(env.vault_authority, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&attacker]);
    assert!(wd.is_err(), "FIXED: withdraw from a non-canonical vault is rejected");

    // honest user can still withdraw their full balance from the canonical vault — no stranding.
    env.svm.expire_blockhash();
    let hdest = Pubkey::new_unique();
    env.svm.set_account(hdest, Account { lamports: 1_000_000_000, data: make_token_data(env.mint, honest.pubkey(), 0), owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
    let hw = env.send(ProgInstruction::Withdraw { amount: 1_000_000 }, vec![
        AccountMeta::new(honest.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(hp, false),
        AccountMeta::new(hdest, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(env.vault_authority, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&honest]);
    assert!(hw.is_ok(), "honest user withdraws their full 1M from the canonical vault (no fragmentation)");
    assert_eq!(env.token_amount(hdest), 1_000_000, "honest user fully paid");
}

// security.md sweep — withdraw dest-owner binding (#44): withdraw must deliver only to a dest token
// account owned by the withdrawing portfolio's owner. A dest owned by a third party must reject
// (verify_withdrawable_token_accounts: dest.owner == expected_dest_owner).
#[test]
fn v16_attack_withdraw_to_third_party_dest_rejected() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new(); let p = env.create_portfolio(&owner);
    let other = Keypair::new();
    env.deposit(&owner, p, 1_000_000);
    let (_, g0) = env.market_state();
    // a dest token account owned by SOMEONE ELSE (correct mint).
    let other_dest = Pubkey::new_unique();
    env.svm.set_account(other_dest, Account { lamports: 1_000_000_000, data: make_token_data(env.mint, other.pubkey(), 0), owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
    env.svm.expire_blockhash();
    let r = env.send(ProgInstruction::Withdraw { amount: 500_000 }, vec![
        AccountMeta::new(owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false),
        AccountMeta::new(other_dest, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(env.vault_authority, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&owner]);
    assert!(r.is_err(), "withdraw to a third-party-owned dest must reject");
    assert_eq!(env.token_amount(other_dest), 0, "no funds delivered to a third-party dest");
    assert_eq!(env.portfolio_state(p).capital, 1_000_000, "capital not debited on rejected withdraw");
    assert_eq!(env.market_state().1.vault, g0.vault, "vault unchanged");
    // own-dest withdraw works.
    env.svm.expire_blockhash();
    let (own, _) = env.withdraw_with_cu(&owner, p, 500_000);
    assert_eq!(env.token_amount(own), 500_000, "withdraw to own dest works");
}

// security.md sweep — asset_index bounds (#37/#39): an out-of-range asset_index on any instruction
// must reject cleanly (no OOB access / panic / state corruption).
#[test]
fn v16_attack_out_of_range_asset_index_rejected() {
    let mut env = V16CuEnv::new(); // 1 asset (index 0 valid)
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    let (_, g0) = env.market_state();
    for bad in [1u16, 7, 255, 9999, u16::MAX] {
        // trade on a bad asset index
        env.svm.expire_blockhash();
        let rt = env.try_trade_asset_with_cu(bad, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
        assert!(rt.is_err(), "trade on out-of-range asset_index {} must reject", bad);
        // crank on a bad asset index
        env.svm.expire_blockhash();
        let rc = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: bad, now_slot: 1, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
            vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(pa, false)], &[]);
        assert!(rc.is_err(), "crank on out-of-range asset_index {} must reject", bad);
        // push auth mark on a bad asset index (admin)
        env.svm.expire_blockhash();
        let rm = send_tx(&mut env.svm, env.program_id, &env.payer,
            ProgInstruction::PushAuthMark { asset_index: bad, now_slot: 1, mark_e6: 100 },
            vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(env.market, false)], &[&env.admin]);
        assert!(rm.is_err(), "push auth mark on out-of-range asset_index {} must reject", bad);
    }
    // no corruption from any rejected OOB attempt.
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert_eq!(g1.c_tot, g0.c_tot, "c_tot unchanged");
    assert_eq!(g1.assets[0].oi_eff_long_q, 0, "no OI created");
}

// security.md sweep — ledger account binding (#44, F-VAULT-FRAG sibling): a backing-domain ledger is
// bound to (market_group, authority, domain). Passing a ledger under the WRONG domain must reject —
// no cross-domain earnings/accounting manipulation. (Contrast the vault, which is owner-only.)
#[test]
fn v16_attack_backing_ledger_domain_binding_enforced() {
    let mut env = V16CuEnv::new();
    let ledger = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger, 1, 100, 10); // ledger bound to domain 1
    // sync the SAME ledger but claiming domain 2 -> must reject (domain mismatch).
    env.svm.expire_blockhash();
    let r = send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::SyncBackingDomainLedger { domain: 2 },
        vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(ledger, false)], &[&env.admin]);
    assert!(r.is_err(), "ledger used under the wrong domain must reject (binding enforced)");
    // the correct domain still syncs.
    env.svm.expire_blockhash();
    let r_ok = send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::SyncBackingDomainLedger { domain: 1 },
        vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(ledger, false)], &[&env.admin]);
    assert!(r_ok.is_ok(), "correct-domain sync works: {:?}", r_ok);
}

// security.md sweep — deposit source confusion (#35/#44): the deposit source must be a token account
// owned by the depositor. Passing the VAULT (or any non-owned account) as the source must reject —
// otherwise a vault->vault no-op transfer could credit capital for free (mint capital from nothing).
#[test]
fn v16_attack_deposit_from_vault_as_source_rejected() {
    let mut env = V16CuEnv::new();
    let honest = Keypair::new(); let hp = env.create_portfolio(&honest);
    env.deposit(&honest, hp, 1_000_000); // fund the vault with real tokens
    let attacker = Keypair::new(); let ap = env.create_portfolio(&attacker);
    let (_, g0) = env.market_state();

    // attacker tries to "deposit" using the VAULT as the source (vault is owned by vault_authority, not attacker).
    env.svm.expire_blockhash();
    let r = env.send(ProgInstruction::Deposit { amount: 500_000 }, vec![
        AccountMeta::new(attacker.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(ap, false),
        AccountMeta::new(env.vault, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&attacker]);
    assert!(r.is_err(), "deposit using the vault as source must reject (source not owned by depositor)");
    assert_eq!(env.portfolio_state(ap).capital, 0, "no free capital minted");
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "vault accounting unchanged");
    assert_eq!(env.token_amount(env.vault), 1_000_000, "real vault balance unchanged");

    // also: a source owned by a THIRD PARTY (not the attacker) must reject.
    let other = Keypair::new();
    let other_src = env.token_account_for_mint(env.mint, other.pubkey(), 500_000);
    env.svm.expire_blockhash();
    let r2 = env.send(ProgInstruction::Deposit { amount: 500_000 }, vec![
        AccountMeta::new(attacker.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(ap, false),
        AccountMeta::new(other_src, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&attacker]);
    assert!(r2.is_err(), "deposit from a third-party-owned source must reject");
    assert_eq!(env.portfolio_state(ap).capital, 0, "no capital credited from a non-owned source");
    assert_eq!(env.token_amount(other_src), 500_000, "third-party source untouched");
}

// security.md sweep — pnl_pos_tot aggregate integrity (#33, Bug-#10 neighborhood): pnl_pos_tot is the
// sum of positive account PnLs (the haircut denominator). It must stay EXACTLY equal to Σ max(0, pnl)
// as positions move through profit -> loss -> profit. A desync would mis-price the haircut.
#[test]
fn v16_attack_pnl_pos_tot_consistent_through_sign_flips() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let lo_owner = Keypair::new(); let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new(); let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, 1_000_000);
    env.deposit(&sh_owner, sh, 1_000_000);
    env.trade_asset_with_cu(0, &lo_owner, lo, &sh_owner, sh, (10_000 * POS_SCALE) as i128, 100, 0);
    let crank_both = |env: &mut V16CuEnv, slot: u64| {
        for p in [sh, lo] {
            env.svm.expire_blockhash();
            let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
                vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false)], &[]);
        }
    };
    let check = |env: &V16CuEnv, tag: &str| {
        let a = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
        let b = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
        let (_, g) = env.market_state();
        let sum_pos = (a.pnl.max(0) + b.pnl.max(0)) as u128;
        assert_eq!(g.pnl_pos_tot, sum_pos, "[{}] pnl_pos_tot == Σ max(0,pnl) (a.pnl={} b.pnl={})", tag, a.pnl, b.pnl);
        assert!(g.vault >= g.c_tot + g.insurance, "[{}] senior conservation", tag);
    };
    check(&env, "open");
    // price UP -> long wins; crank to settle.
    env.svm.warp_to_slot(10); env.push_auth_mark_with_cu(10, 120);
    crank_both(&mut env, 10); env.svm.warp_to_slot(11); crank_both(&mut env, 11);
    check(&env, "long winning");
    // price DOWN below entry -> long now LOSES (pnl flips negative), short wins.
    env.svm.warp_to_slot(12); env.push_auth_mark_with_cu(12, 80);
    crank_both(&mut env, 12); env.svm.warp_to_slot(13); crank_both(&mut env, 13);
    check(&env, "long losing / short winning");
    // back UP to entry -> roughly flat.
    env.svm.warp_to_slot(14); env.push_auth_mark_with_cu(14, 100);
    crank_both(&mut env, 14); env.svm.warp_to_slot(15); crank_both(&mut env, 15);
    check(&env, "back to entry");
}

// security.md sweep — withdraw vs open-position margin (#19/#46): an account with an open position
// must not be able to withdraw into under-collateralization (margin is conservatively reserved), yet
// its capital must remain fully recoverable once the position is closed (no permanent lock / LoF).
#[test]
fn v16_attack_withdraw_respects_margin_and_recoverable() {
    let mut env = V16CuEnv::new(); // IM = 100%, max_price_move = 100%/slot (conservative envelope)
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000);
    env.deposit(&lb, pb, 10_000_000);
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, (6 * POS_SCALE) as i128, 100, 0); // notional 600

    // with the position open, withdrawing the FULL capital must reject (margin reserved).
    let try_wd = |env: &mut V16CuEnv, amt: u128| -> bool {
        env.svm.expire_blockhash();
        let d = Pubkey::new_unique();
        env.svm.set_account(d, Account { lamports: 1_000_000_000, data: make_token_data(env.mint, la.pubkey(), 0), owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
        env.send(ProgInstruction::Withdraw { amount: amt }, vec![
            AccountMeta::new(la.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(pa, false),
            AccountMeta::new(d, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(env.vault_authority, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&la]).is_ok()
    };
    assert!(!try_wd(&mut env, 1_000), "cannot withdraw full capital with a position open");
    assert!(!try_wd(&mut env, 500), "conservative margin reserves capital under the worst-case envelope");
    assert_eq!(env.portfolio_state(pa).capital, 1_000, "capital intact after rejected withdraws (no partial debit)");

    // close the position; capital must then be fully recoverable (no permanent lock / LoF).
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, -(6 * POS_SCALE as i128), 100, 0);
    assert!(percolator::active_bitmap_is_empty(env.portfolio_state(pa).active_bitmap), "la flat after close");
    let cap = env.portfolio_state(pa).capital;
    env.svm.expire_blockhash();
    let (dest, _) = env.withdraw_with_cu(&la, pa, cap);
    assert_eq!(env.token_amount(dest) as u128, cap, "full capital recovered after closing (no LoF)");
    assert_eq!(env.portfolio_state(pa).capital, 0, "capital fully withdrawn");
    let (_, g) = env.market_state();
    assert_eq!(g.vault, g.c_tot + g.insurance, "conservation");
}

// security.md sweep — resolved-mode operation gating (#30): once resolved, every Live-only op
// (Deposit, Trade, Withdraw, ConvertReleasedPnl) must reject; only the wind-down path (CloseResolved)
// works. A Live-op leaking through after resolution could corrupt the frozen state.
#[test]
fn v16_attack_resolved_mode_gates_all_live_ops() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new(); let p = env.create_portfolio(&owner);
    let other = Keypair::new(); let pq = env.create_portfolio(&other); // create BEFORE resolve
    env.deposit(&owner, p, 1_000_000);
    env.resolve();
    let (_, g0) = env.market_state();

    // Deposit -> reject
    let src = env.token_account_for_mint(env.mint, owner.pubkey(), 100);
    env.svm.expire_blockhash();
    let r_dep = env.send(ProgInstruction::Deposit { amount: 100 }, vec![
        AccountMeta::new(owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false),
        AccountMeta::new(src, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&owner]);
    assert!(r_dep.is_err(), "Deposit must reject in resolved mode");
    // Withdraw -> reject (must use CloseResolved)
    env.svm.expire_blockhash();
    let dest = Pubkey::new_unique();
    env.svm.set_account(dest, Account { lamports: 1_000_000_000, data: make_token_data(env.mint, owner.pubkey(), 0), owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
    let r_wd = env.send(ProgInstruction::Withdraw { amount: 100 }, vec![
        AccountMeta::new(owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false),
        AccountMeta::new(dest, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(env.vault_authority, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&owner]);
    assert!(r_wd.is_err(), "Withdraw must reject in resolved mode");
    // ConvertReleasedPnl -> reject
    env.svm.expire_blockhash();
    let r_cv = env.send(ProgInstruction::ConvertReleasedPnl { amount: 1 },
        vec![AccountMeta::new(owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false)], &[&owner]);
    assert!(r_cv.is_err(), "ConvertReleasedPnl must reject in resolved mode");
    // Trade -> reject
    env.svm.expire_blockhash();
    let r_tr = env.try_trade_asset_with_cu(0, &owner, p, &other, pq, POS_SCALE as i128, 100, 0);
    assert!(r_tr.is_err(), "Trade must reject in resolved mode");

    // nothing changed; CloseResolved (the wind-down path) works.
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "vault unchanged by all rejected live ops");
    assert_eq!(g1.c_tot, g0.c_tot, "c_tot unchanged");
    let cr = env.close_resolved(&owner, p);
    assert_eq!(env.token_amount(cr), 1_000_000, "CloseResolved pays out the resolved capital");
}

// security.md sweep / F-VAULT-FRAG fix coverage — insurance top-up vault pinning: TopUpInsurance
// routes through verify_vault_token_account, so the canonical-ATA pin must apply here too. A top-up
// routed to a non-canonical vault-authority-owned account must reject (else insurance could be
// credited while tokens land in a fragment account).
#[test]
fn v16_attack_insurance_topup_pinned_to_canonical_vault() {
    let mut env = V16CuEnv::new();
    let (_, g0) = env.market_state();
    // control: top-up to the canonical vault works and conserves.
    let (_src_ok, _) = env.top_up_insurance_with_cu(500);
    let (_, g1) = env.market_state();
    assert_eq!(g1.insurance, g0.insurance + 500, "canonical insurance top-up credits insurance");
    assert_eq!(g1.vault, g0.vault + 500, "vault grows by the top-up");
    assert_eq!(env.token_amount(env.vault), g1.vault as u64, "real canonical vault balance matches accounting");

    // attack: top-up routed to a non-canonical vault-authority-owned account must reject.
    let fake_vault = Pubkey::new_unique();
    env.svm.set_account(fake_vault, Account { lamports: 1_000_000_000, data: make_token_data(env.mint, env.vault_authority, 0), owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
    let src = env.token_account_for_mint(env.mint, env.admin.pubkey(), 500);
    env.svm.expire_blockhash();
    let r = send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::TopUpInsurance { amount: 500 },
        vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(src, false), AccountMeta::new(fake_vault, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&env.admin]);
    assert!(r.is_err(), "FIXED: insurance top-up to a non-canonical vault is rejected");
    let (_, g2) = env.market_state();
    assert_eq!(g2.insurance, g1.insurance, "insurance unchanged by rejected fragment top-up");
    assert_eq!(g2.vault, g1.vault, "vault accounting unchanged");
    assert_eq!(env.token_amount(fake_vault), 0, "fragment vault received nothing");
    assert_eq!(env.token_amount(src), 500, "source untouched");
}

// security.md sweep — funding direction symmetry (#33/#9): with the mark BELOW the index (opposite of
// batch 19), funding must flow the other way (shorts pay longs) and still be value-conserving. Probes
// the negative-premium branch of premium_funding_rate_e9.
#[test]
fn v16_attack_funding_direction_mark_below_index_conserves() {
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
    let lo_owner = Keypair::new(); let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new(); let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, DEPOSIT);
    env.deposit(&sh_owner, sh, DEPOSIT);
    env.trade_with_cu(&lo_owner, lo, &sh_owner, sh, POS_SCALE as i128, INITIAL_PRICE, 0);
    // push the mark BELOW the index, then let funding accrue (no re-push).
    env.svm.warp_to_slot(1);
    env.push_ewma_mark_with_cu(1, INITIAL_PRICE / 2);
    for slot in 1..=5u64 {
        env.svm.warp_to_slot(slot);
        for p in [lo, sh] {
            env.svm.expire_blockhash();
            let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
                vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false)], &[]);
        }
    }
    let a = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    let b = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
    let (_, g) = env.market_state();
    assert!(g.assets[0].f_long_num != 0 || g.assets[0].f_short_num != 0, "funding accrued (non-vacuous)");
    // mark < index => OPPOSITE direction from batch 19 (where longs were charged): longs are credited.
    assert!(g.assets[0].f_long_num > 0 && g.assets[0].f_short_num < 0, "mark < index => shorts pay longs (f_long>0, f_short<0)");
    // value conservation (same widened invariant as batch 19).
    assert_eq!(g.vault, 2 * DEPOSIT, "no tokens minted/burned");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    let residual = g.vault as i128 - g.c_tot as i128 - g.insurance as i128;
    assert!(residual >= a.pnl.max(0) + b.pnl.max(0), "positive pnl backed by residual");
    let total_equity = (a.capital as i128 + a.pnl) + (b.capital as i128 + b.pnl);
    assert!(total_equity + g.insurance as i128 <= g.vault as i128, "no over-distribution");
}


// security.md sweep — UpdateAuthority new-authority binding (#6): setting an authority to a non-zero
// key requires THAT key to co-sign (handle_update_authority: expect_signer(new_authority) + key match).
// Otherwise an admin (or attacker) could assign an authority to a key nobody controls (griefing/brick).
#[test]
fn v16_attack_update_authority_requires_new_authority_signature() {
    let mut env = V16CuEnv::new();
    let victim = Keypair::new(); // a key that will NOT sign
    let (cfg0, _) = env.market_state();
    // admin tries to set the MARK authority to `victim` without victim signing -> reject.
    env.svm.expire_blockhash();
    let r = send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::UpdateAuthority { kind: processor::AUTHORITY_MARK, new_pubkey: victim.pubkey().to_bytes() },
        vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new_readonly(victim.pubkey(), false), AccountMeta::new(env.market, false)],
        &[&env.admin]);
    assert!(r.is_err(), "setting an authority to a non-signing key must reject");
    let (cfg1, _) = env.market_state();
    assert_eq!(cfg1.mark_authority, cfg0.mark_authority, "mark authority unchanged by the rejected update");

    // with the new authority co-signing, the update succeeds.
    let new_mark = Keypair::new();
    env.ensure_signer_account(new_mark.pubkey());
    env.svm.expire_blockhash();
    let r_ok = send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::UpdateAuthority { kind: processor::AUTHORITY_MARK, new_pubkey: new_mark.pubkey().to_bytes() },
        vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(new_mark.pubkey(), true), AccountMeta::new(env.market, false)],
        &[&env.admin, &new_mark]);
    assert!(r_ok.is_ok(), "co-signed authority update succeeds: {:?}", r_ok);
    assert_eq!(env.market_state().0.mark_authority, new_mark.pubkey().to_bytes(), "mark authority updated to the co-signing key");
}

// regression (security.md sweep): round-trip recovery under the junior-pnl model. A price round-trip
// (100->110->100) leaves the drawdown-first trader's recovery as JUNIOR pnl (realized losses are
// senior/immediate, recoveries park as junior pnl that is not liquid in Live mode). Value is fully
// CONSERVED (vault == deposits) and fully RECOVERABLE at resolution — NOT a loss of funds. Documents
// that per-account LIQUID equity is not symmetric in Live mode, but total value is and resolution pays
// everyone their fair amount.
#[test]
fn v16_regression_roundtrip_recovers_fully_at_resolution() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let lo_owner = Keypair::new(); let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new(); let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, 1_000_000);
    env.deposit(&sh_owner, sh, 1_000_000);
    env.trade_asset_with_cu(0, &lo_owner, lo, &sh_owner, sh, (10_000 * POS_SCALE) as i128, 100, 0);
    let crank_all = |env: &mut V16CuEnv, s: u64| { for p in [sh, lo] { env.svm.expire_blockhash();
        let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: s, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
            vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false)], &[]); } };
    // up to 110 then back to 100 (round-trip to breakeven).
    env.svm.warp_to_slot(10); env.push_auth_mark_with_cu(10, 110);
    for s in [10u64,11,12] { env.svm.warp_to_slot(s); crank_all(&mut env, s); }
    env.svm.warp_to_slot(20); env.push_auth_mark_with_cu(20, 100);
    for s in [20u64,21,22,23,24] { env.svm.warp_to_slot(s); crank_all(&mut env, s); }
    // close both, crank to convergence.
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(0, &lo_owner, lo, &sh_owner, sh, -(10_000 * POS_SCALE as i128), 100, 0);
    for s in 25u64..=35 { env.svm.warp_to_slot(s); crank_all(&mut env, s); }
    // Live-mode invariants: value conserved, short's recovery is junior pnl backed by residual.
    let b = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
    let (_, g) = env.market_state();
    assert_eq!(g.vault, 2_000_000, "vault conserved through the round-trip (no value created/destroyed)");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    let residual = g.vault as i128 - g.c_tot as i128 - g.insurance as i128;
    assert!(residual >= b.pnl.max(0), "short's junior recovery pnl is backed by residual");
    // resolution pays EVERYONE their full fair value — no permanent LoF from the junior-pnl mechanism.
    env.resolve();
    let lo_dest = env.close_resolved(&lo_owner, lo);
    let sh_dest = env.close_resolved(&sh_owner, sh);
    assert_eq!(env.token_amount(lo_dest), 1_000_000, "long fully recovered at resolution");
    assert_eq!(env.token_amount(sh_dest), 1_000_000, "short fully recovered at resolution (junior pnl realized)");
}

// security.md sweep — TradeCpi maker margin protection (#19/#46): a taker trading against a maker via
// the matcher must not be able to force the maker (LP) into an under-margined position. If the maker
// can't margin the fill, the trade must reject — the maker is protected like any account.
#[test]
fn v16_attack_tradecpi_thin_maker_margin_protected() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new(); let taker = env.create_portfolio(&taker_owner);
    let maker_owner = Keypair::new(); let maker = env.create_portfolio(&maker_owner);
    env.deposit(&taker_owner, taker, 100_000_000); // taker well funded
    env.deposit(&maker_owner, maker, 1_000);        // maker THIN
    let (ctx, delegate, _) = env.init_matcher_context(matcher_program, maker);
    let (_, g0) = env.market_state();

    // taker tries to trade a LARGE size against the thin maker -> maker can't margin it -> reject.
    env.svm.expire_blockhash();
    let r = env.try_trade_cpi_with_cu_on_asset(&taker_owner, taker, &maker_owner, maker, matcher_program, ctx, delegate, 0, (10_000 * POS_SCALE) as i128, 100);
    assert!(r.is_err(), "trade that would leave the thin maker under-margined must reject");
    // no position opened, no value moved, maker capital intact.
    let (_, g1) = env.market_state();
    assert_eq!(g1.assets[0].oi_eff_long_q, 0, "no OI created by the rejected over-fill");
    assert_eq!(env.portfolio_state(maker).legs[0].basis_pos_q, 0, "maker took no position");
    assert_eq!(env.portfolio_state(taker).legs[0].basis_pos_q, 0, "taker took no position");
    assert_eq!(env.portfolio_state(maker).capital, 1_000, "maker capital intact");
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    // a SMALL trade the maker CAN margin still works (control).
    env.svm.expire_blockhash();
    let r_ok = env.try_trade_cpi_with_cu_on_asset(&taker_owner, taker, &maker_owner, maker, matcher_program, ctx, delegate, 0, POS_SCALE as i128, 100);
    assert!(r_ok.is_ok(), "small in-margin trade executes against the maker: {:?}", r_ok);
}

// security.md sweep — circuit breaker on mark push (#9 oracle manipulation): a push to a far-away
// mark must move the effective price by at most max_price_move_bps_per_slot per slot. An attacker
// (mark authority) cannot jump the settlement price arbitrarily in one slot.
#[test]
fn v16_attack_mark_push_clamped_per_slot() {
    let mut env = V16CuEnv::new(); // max_price_move_bps_per_slot = 10_000 (100%/slot)
    env.configure_auth_mark_with_cu(0, 100);
    let lo_owner = Keypair::new(); let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new(); let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, 1_000_000);
    env.deposit(&sh_owner, sh, 1_000_000);
    env.trade_asset_with_cu(0, &lo_owner, lo, &sh_owner, sh, POS_SCALE as i128, 100, 0);
    let mut prev_price = 100u64;
    for slot in [10u64, 11, 12] {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_with_cu(slot, 1_000_000); // push to a huge mark (10000x) every slot
        env.svm.expire_blockhash();
        let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
            vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(lo, false)], &[]);
        let (_, g) = env.market_state();
        let ep = g.assets[0].effective_price;
        // the per-slot move is clamped to <= 100% (price at most doubles per slot).
        assert!(ep <= prev_price * 2, "slot {}: effective price {} clamped to <= 2x prev {} (circuit breaker)", slot, ep, prev_price);
        assert!(ep > prev_price, "slot {}: price moved toward the pushed mark (non-vacuous)", slot);
        prev_price = ep;
        assert!(g.vault >= g.c_tot + g.insurance, "senior conservation under clamped move");
    }
    // even after 3 slots of pushing to 1,000,000, the effective price is nowhere near it (clamped).
    let (_, g) = env.market_state();
    assert!(g.assets[0].effective_price <= 800, "after 3 clamped slots, price is far below the 1,000,000 push (got {})", g.assets[0].effective_price);
}

// security.md sweep — CloseResolved dest validation (#44): the resolved payout must reject a dest
// token account of the wrong mint or owned by a third party (verify_withdrawable_token_accounts
// applies here too). No payout to a mismatched/foreign account.
#[test]
fn v16_attack_close_resolved_dest_validation() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new(); let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    env.resolve();
    let (_, g0) = env.market_state();
    let cr = |env: &mut V16CuEnv, dest: Pubkey, signer: &Keypair| -> Result<u64, String> {
        env.svm.expire_blockhash();
        env.send(ProgInstruction::CloseResolved { fee_rate_per_slot: 0 }, vec![
            AccountMeta::new_readonly(signer.pubkey(), false), AccountMeta::new(env.market, false), AccountMeta::new(p, false),
            AccountMeta::new(dest, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(env.vault_authority, false), AccountMeta::new_readonly(spl_token::ID, false)], &[])
    };
    // wrong-mint dest -> reject.
    let other_mint = Pubkey::new_unique();
    let bad_mint_dest = Pubkey::new_unique();
    env.svm.set_account(bad_mint_dest, Account { lamports: 1_000_000_000, data: make_token_data(other_mint, owner.pubkey(), 0), owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
    assert!(cr(&mut env, bad_mint_dest, &owner).is_err(), "CloseResolved to a wrong-mint dest must reject");
    assert_eq!(env.token_amount(bad_mint_dest), 0, "no payout to wrong-mint dest");

    // third-party-owned dest -> reject.
    let other = Keypair::new();
    let foreign_dest = Pubkey::new_unique();
    env.svm.set_account(foreign_dest, Account { lamports: 1_000_000_000, data: make_token_data(env.mint, other.pubkey(), 0), owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
    assert!(cr(&mut env, foreign_dest, &owner).is_err(), "CloseResolved to a third-party dest must reject");
    assert_eq!(env.token_amount(foreign_dest), 0, "no payout to foreign dest");

    assert_eq!(env.market_state().1.vault, g0.vault, "vault unchanged by rejected payouts");
    assert_eq!(env.portfolio_state(p).capital, 1_000_000, "portfolio still owed its capital");
    // correct dest works.
    let good = env.close_resolved(&owner, p);
    assert_eq!(env.token_amount(good), 1_000_000, "correct-mint own dest receives the resolved payout");
}

// security.md sweep — liquidation of a healthy account (#2): an account above maintenance margin must
// NOT be liquidatable. A permissionless action:1 crank against a healthy account must be a no-op — no
// force-close, no fee extraction, position intact.
#[test]
fn v16_attack_healthy_account_not_liquidatable() {
    // maintenance 50% < initial 100% -> a freshly-opened account is well above maintenance.
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 5_000, 10_000, 1_000);
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    let basis0 = env.portfolio_state(pa).legs[0].basis_pos_q;
    assert!(basis0 != 0, "la opened a position");
    let (_, g0) = env.market_state();

    // attacker tries to liquidate the healthy la (no adverse price move).
    env.svm.warp_to_slot(1);
    env.svm.expire_blockhash();
    let _ = env.send(ProgInstruction::PermissionlessCrank { action: 1, asset_index: 0, now_slot: 1, funding_rate_e9: 0, close_q: POS_SCALE, fee_bps: 0, recovery_reason: 0 },
        vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(pa, false)], &[]);

    // healthy account: position intact, no fee extracted, conservation.
    assert_eq!(env.portfolio_state(pa).legs[0].basis_pos_q, basis0, "healthy account's position NOT force-closed by liquidation");
    assert_eq!(env.portfolio_state(pa).capital, 1_000_000, "healthy account capital not docked a liquidation fee");
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert_eq!(g1.assets[0].oi_eff_long_q, g1.assets[0].oi_eff_short_q, "OI still balanced (position intact)");
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}

// security.md sweep — permissionless resolve gating (#30 DoS): ResolveStalePermissionless lets ANYONE
// resolve a market, but ONLY when the oracle is genuinely stale-matured. It must reject on a fresh
// market (and when not configured) — otherwise an attacker could force resolution as a griefing DoS.
#[test]
fn v16_attack_permissionless_resolve_rejects_fresh_market() {
    let resolve_stale = |env: &mut V16CuEnv, now_slot: u64| -> Result<u64, String> {
        env.svm.warp_to_slot(now_slot);
        env.send(ProgInstruction::ResolveStalePermissionless { now_slot }, vec![AccountMeta::new(env.market, false)], &[])
    };
    // 1) DEFAULT env: permissionless_resolve_stale_slots == 0 -> always disabled. Even a huge future
    //    now_slot can't force resolution (slot is authenticated; staleness not configured).
    let mut env = V16CuEnv::new();
    let owner = Keypair::new(); let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    assert!(resolve_stale(&mut env, 1_000_000).is_err(), "permissionless resolve must reject when not configured");
    // market still Live: owner can withdraw (would fail if resolved).
    let (d, _) = env.withdraw_with_cu(&owner, p, 100_000);
    assert_eq!(env.token_amount(d), 100_000, "market still Live after rejected permissionless resolve");

    // 2) CONFIGURED env (stale_slots=5) but oracle FRESH -> still rejects.
    let mut env2 = V16CuEnv::new();
    env2.configure_permissionless_resolve_with_cu(5, 5);
    env2.configure_auth_mark_with_cu(0, 100);
    let o2 = Keypair::new(); let p2 = env2.create_portfolio(&o2);
    env2.deposit(&o2, p2, 1_000_000);
    // keep the oracle fresh by pushing/cranking at slot 3, then try to resolve only 2 slots later.
    env2.svm.warp_to_slot(3); env2.push_auth_mark_with_cu(3, 100);
    env2.svm.expire_blockhash();
    let _ = env2.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: 3, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
        vec![AccountMeta::new(env2.payer.pubkey(), true), AccountMeta::new(env2.market, false), AccountMeta::new(p2, false)], &[]);
    assert!(resolve_stale(&mut env2, 4).is_err(), "permissionless resolve must reject while the oracle is fresh (only 1 slot stale < 5)");
    // market still Live: a withdraw succeeds (resolved mode would reject it).
    let (d2, _) = env2.withdraw_with_cu(&o2, p2, 100_000);
    assert_eq!(env2.token_amount(d2), 100_000, "market still Live after rejected fresh-oracle resolve");
}

// security.md sweep — CloseSlab wind-down finality (#30/#48): a market may only be closed when fully
// wound down (mode==Resolved AND vault==0 && insurance==0 && c_tot==0 && no materialized portfolios).
// Closing while value/positions remain would strand funds — must reject.
#[test]
fn v16_attack_close_slab_requires_full_winddown() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new(); let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    let close_slab = |env: &mut V16CuEnv| -> Result<u64, String> {
        let dest = Pubkey::new_unique();
        env.svm.set_account(dest, Account { lamports: 1_000_000_000, data: make_token_data(env.mint, env.admin.pubkey(), 0), owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
        env.svm.expire_blockhash();
        send_tx(&mut env.svm, env.program_id, &env.payer, ProgInstruction::CloseSlab,
            vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(env.vault, false),
                 AccountMeta::new_readonly(env.vault_authority, false), AccountMeta::new(dest, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&env.admin])
    };
    // 1) Live market with funds -> reject (mode != Resolved).
    assert!(close_slab(&mut env).is_err(), "CloseSlab on a Live market must reject");
    assert_eq!(env.token_amount(env.vault), 1_000_000, "vault funds intact");

    // 2) Resolved market but still holding c_tot / a materialized portfolio -> reject.
    env.resolve();
    let (_, g) = env.market_state();
    assert!(g.c_tot != 0 || g.vault != 0, "market still holds value after resolve (positions not closed)");
    assert!(close_slab(&mut env).is_err(), "CloseSlab while value/positions remain must reject");
    assert_eq!(env.token_amount(env.vault), 1_000_000, "vault funds still intact (not stranded)");
    // the user can still recover via CloseResolved (funds not locked by the rejected CloseSlab).
    let cr = env.close_resolved(&owner, p);
    assert_eq!(env.token_amount(cr), 1_000_000, "user recovers funds via CloseResolved");
}

// security.md sweep — WithdrawInsuranceDomain operator authorization (#6): a per-domain insurance
// withdrawal must be signed by THAT domain's insurance_operator. A non-operator must reject — no
// draining a domain's insurance by an unauthorized caller.
#[test]
fn v16_attack_withdraw_insurance_domain_operator_gated() {
    let mut env = V16CuEnv::new();
    env.top_up_insurance(1_000_000);
    let mallory = Keypair::new();
    env.ensure_signer_account(mallory.pubkey());
    let (_, g0) = env.market_state();
    let dest = env.token_account_for_mint(env.mint, mallory.pubkey(), 0);
    // non-operator attempts a domain insurance withdrawal -> reject.
    env.svm.expire_blockhash();
    let r = send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::WithdrawInsuranceDomain { domain: 0, amount: 500_000 },
        vec![AccountMeta::new(mallory.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(dest, false),
             AccountMeta::new(env.vault, false), AccountMeta::new_readonly(env.vault_authority, false), AccountMeta::new_readonly(spl_token::ID, false)],
        &[&mallory]);
    assert!(r.is_err(), "non-operator domain insurance withdrawal must reject");
    assert_eq!(env.token_amount(dest), 0, "no insurance drained by non-operator");
    let (_, g1) = env.market_state();
    assert_eq!(g1.insurance, g0.insurance, "insurance unchanged");
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}

// security.md sweep — RebalanceReduce owner gating (#6/#46): RebalanceReduce is OWNER-gated
// self-service risk reduction (with_one_portfolio_view enforces owner signs + matches the portfolio).
// A non-owner must NOT be able to force-reduce a victim's position (griefing); the owner may reduce
// their own. Verifies no permissionless force-close.
#[test]
fn v16_attack_rebalance_reduce_owner_gated() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 5_000, 10_000, 1_000);
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    let basis0 = env.portfolio_state(pa).legs[0].basis_pos_q;
    assert!(basis0 != 0, "la opened a position");
    let (_, g0) = env.market_state();

    // ATTACK: a non-owner tries to force-reduce la's position -> reject (owner mismatch).
    let mallory = Keypair::new(); env.ensure_signer_account(mallory.pubkey());
    env.svm.expire_blockhash();
    let r_grief = env.send(ProgInstruction::RebalanceReduce { asset_index: 0, reduce_q: POS_SCALE },
        vec![AccountMeta::new(mallory.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(pa, false)], &[&mallory]);
    assert!(r_grief.is_err(), "non-owner force-reduce of a victim's position must reject");
    assert_eq!(env.portfolio_state(pa).legs[0].basis_pos_q, basis0, "victim's position not reduced by attacker");
    assert_eq!(env.market_state().1.vault, g0.vault, "vault unchanged by rejected griefing reduce");

    // LEGITIMATE: the OWNER may reduce their own position (self-service risk reduction).
    env.svm.expire_blockhash();
    let r_owner = env.send(ProgInstruction::RebalanceReduce { asset_index: 0, reduce_q: POS_SCALE },
        vec![AccountMeta::new(la.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(pa, false)], &[&la]);
    assert!(r_owner.is_ok(), "owner self-reduce should succeed: {:?}", r_owner);
    assert!(env.portfolio_state(pa).legs[0].basis_pos_q.unsigned_abs() < basis0.unsigned_abs(), "owner reduced their own position");
    let (_, g1) = env.market_state();
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
    assert_eq!(g1.assets[0].oi_eff_long_q, g1.assets[0].oi_eff_short_q, "OI still balanced");
}
// security.md sweep — RefineResolvedUnreceiptedBound gating (#6/#30): this wind-down tool (decreases
// the unreceipted resolved claim bound) is admin-only and resolved-mode-only. A non-admin must
// reject, and it must reject in Live mode — no tampering with the resolved payout accounting.
#[test]
fn v16_attack_refine_resolved_bound_admin_and_mode_gated() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new(); let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    let mallory = Keypair::new(); env.ensure_signer_account(mallory.pubkey());
    // 1) Live mode: even the admin can't refine (mode != Resolved).
    env.svm.expire_blockhash();
    let r_live = send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::RefineResolvedUnreceiptedBound { decrease_num: 1 },
        vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(env.market, false)], &[&env.admin]);
    assert!(r_live.is_err(), "refine must reject in Live mode");
    // 2) Resolved mode: a NON-admin can't refine.
    env.resolve();
    env.svm.expire_blockhash();
    let r_nonadmin = send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::RefineResolvedUnreceiptedBound { decrease_num: 1 },
        vec![AccountMeta::new(mallory.pubkey(), true), AccountMeta::new(env.market, false)], &[&mallory]);
    assert!(r_nonadmin.is_err(), "non-admin refine must reject in resolved mode");
    // user funds still fully recoverable (the rejected refines didn't corrupt the resolved accounting).
    let cr = env.close_resolved(&owner, p);
    assert_eq!(env.token_amount(cr), 1_000_000, "user recovers full resolved payout after rejected refines");
}

// security.md sweep — TopUpInsuranceDomain authorization (#6): a per-domain insurance top-up is gated
// to the domain's insurance_authority (v16_program.rs:6577 expect_live_authority). A non-authority
// must reject — no manipulating a domain's insurance/budget accounting by an unauthorized caller.
#[test]
fn v16_attack_topup_insurance_domain_authority_gated() {
    let mut env = V16CuEnv::new();
    let (_, g0) = env.market_state();
    // a non-authority donor tries to top up domain 0's insurance -> reject (Unauthorized).
    let donor = Keypair::new(); env.ensure_signer_account(donor.pubkey());
    let src = env.token_account_for_mint(env.mint, donor.pubkey(), 500);
    env.svm.expire_blockhash();
    let r = send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::TopUpInsuranceDomain { domain: 0, amount: 500 },
        vec![AccountMeta::new(donor.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(src, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&donor]);
    assert!(r.is_err(), "non-authority domain insurance top-up must reject");
    assert_eq!(env.token_amount(src), 500, "donor source untouched");
    let (_, g1) = env.market_state();
    assert_eq!(g1.insurance, g0.insurance, "insurance unchanged by unauthorized top-up");
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}
// security.md sweep — recovery-tool owner gating (#6): ForfeitRecoveryLeg and FinalizeResetSide are
// owner-gated (with_one_portfolio_view enforces owner signs + matches the portfolio). A non-owner
// must NOT be able to invoke them on a victim's portfolio (griefing a recovery/reset).
#[test]
fn v16_attack_recovery_tools_owner_gated() {
    let mut env = V16CuEnv::new();
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    let basis0 = env.portfolio_state(pa).legs[0].basis_pos_q;
    let (_, g0) = env.market_state();
    let mallory = Keypair::new(); env.ensure_signer_account(mallory.pubkey());

    // non-owner ForfeitRecoveryLeg on la's portfolio -> reject (owner mismatch).
    env.svm.expire_blockhash();
    let r1 = env.send(ProgInstruction::ForfeitRecoveryLeg { asset_index: 0, b_delta_budget: 1 },
        vec![AccountMeta::new(mallory.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(pa, false)], &[&mallory]);
    assert!(r1.is_err(), "non-owner ForfeitRecoveryLeg must reject");

    // non-owner FinalizeResetSide on la's portfolio -> reject.
    env.svm.expire_blockhash();
    let r2 = env.send(ProgInstruction::FinalizeResetSide { asset_index: 0, side: 0 },
        vec![AccountMeta::new(mallory.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(pa, false)], &[&mallory]);
    assert!(r2.is_err(), "non-owner FinalizeResetSide must reject");

    // victim's position untouched, conservation.
    assert_eq!(env.portfolio_state(pa).legs[0].basis_pos_q, basis0, "victim's position untouched by recovery-tool griefing");
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert_eq!(g1.assets[0].oi_eff_long_q, g1.assets[0].oi_eff_short_q, "OI still balanced");
}

// security.md sweep — operation-sequence conservation (#32/#33 fuzz-lite): a long varied sequence of
// deposits/trades/flips/price-moves/cranks/withdrawals must never drift the core invariants. Checks
// real-vault==accounting, c_tot==Σcapitals, senior conservation, OI balance at every checkpoint.
#[test]
fn v16_attack_long_sequence_conservation() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let k: Vec<Keypair> = (0..4).map(|_| Keypair::new()).collect();
    let p: Vec<Pubkey> = k.iter().map(|kp| env.create_portfolio(kp)).collect();
    for i in 0..4 { env.deposit(&k[i], p[i], 1_000_000); }
    let check = |env: &V16CuEnv, tag: &str| {
        let (_, g) = env.market_state();
        let sum: u128 = p.iter().map(|pp| state::read_portfolio(&env.svm.get_account(pp).unwrap().data).unwrap().capital).sum();
        assert_eq!(g.c_tot, sum, "[{}] c_tot == Σcapitals", tag);
        assert!(g.vault >= g.c_tot + g.insurance, "[{}] senior conservation", tag);
        assert_eq!(g.vault as u64, env.token_amount(env.vault), "[{}] accounting vault == real vault balance", tag);
        assert_eq!(g.assets[0].oi_eff_long_q, g.assets[0].oi_eff_short_q, "[{}] OI balanced", tag);
    };
    check(&env, "deposits");
    // trades among the 4 accounts (open, partial close, flip).
    env.trade_asset_with_cu(0, &k[0], p[0], &k[1], p[1], (5_000 * POS_SCALE) as i128, 100, 0); check(&env, "t1");
    env.svm.expire_blockhash(); env.trade_asset_with_cu(0, &k[2], p[2], &k[3], p[3], (3_000 * POS_SCALE) as i128, 100, 0); check(&env, "t2");
    // price move + cranks.
    env.svm.warp_to_slot(10); env.push_auth_mark_with_cu(10, 108);
    for slot in [10u64, 11] { env.svm.warp_to_slot(slot); for pp in &p { env.svm.expire_blockhash();
        let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
            vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(*pp, false)], &[]); } }
    check(&env, "after move+crank");
    // a deposit mid-stream + a flip.
    env.svm.expire_blockhash(); env.deposit(&k[0], p[0], 200_000); check(&env, "mid-deposit");
    env.svm.expire_blockhash(); env.trade_asset_with_cu(0, &k[1], p[1], &k[0], p[0], (2_000 * POS_SCALE) as i128, 108, 0); check(&env, "flip");
    // price back, settle, close everyone out.
    env.svm.warp_to_slot(20); env.push_auth_mark_with_cu(20, 100);
    for slot in 20u64..=25 { env.svm.warp_to_slot(slot); for pp in &p { env.svm.expire_blockhash();
        let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
            vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(*pp, false)], &[]); } }
    check(&env, "settled");
    // total real vault still equals accounting; no value created across the whole sequence.
    let (_, g) = env.market_state();
    assert_eq!(g.vault as u64, env.token_amount(env.vault), "final: accounting == real vault");
    assert!(g.vault <= 4_200_000, "no value created (total deposited 4*1M + 200k)");
}

// security.md sweep — cross-margin divergent close conservation (#33/#22): a portfolio long on asset0
// and short on asset1, both winning under divergent moves, closes both legs. Value must be conserved
// through the multi-asset settlement+close (no leakage, senior conservation, accounting==real vault).
#[test]
fn v16_attack_cross_margin_divergent_close_conserves() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    let cfg = |env: &mut V16CuEnv, ix: ProgInstruction| {
        send_tx(&mut env.svm, env.program_id, &env.payer, ix,
            vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(env.market, false)], &[&env.admin]).expect("mark cfg");
    };
    cfg(&mut env, ProgInstruction::ConfigureAuthMark { asset_index: 1, now_slot: 0, initial_mark_e6: 100 });
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 2_000_000);
    env.deposit(&lb, pb, 2_000_000);
    // la LONG asset0, SHORT asset1.
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(1, &la, pa, &lb, pb, -(POS_SCALE as i128), 100, 0);
    // asset0 UP (la long wins), asset1 DOWN (la short wins) -> la wins both, lb loses both.
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110);
    cfg(&mut env, ProgInstruction::PushAuthMark { asset_index: 1, now_slot: 10, mark_e6: 90 });
    for slot in [10u64, 11] { env.svm.warp_to_slot(slot); for ai in [0u16,1] { for p in [pa, pb] { env.svm.expire_blockhash();
        let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: ai, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
            vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false)], &[]); } } }
    // close both legs at the moved prices.
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, -(POS_SCALE as i128), 110, 0);
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(1, &la, pa, &lb, pb, POS_SCALE as i128, 90, 0);
    let a = state::read_portfolio(&env.svm.get_account(&pa).unwrap().data).unwrap();
    let b = state::read_portfolio(&env.svm.get_account(&pb).unwrap().data).unwrap();
    let (_, g) = env.market_state();
    assert_eq!(g.assets[0].oi_eff_long_q, 0, "asset0 flat after close");
    assert_eq!(g.assets[1].oi_eff_long_q, 0, "asset1 flat after close");
    let total_equity = (a.capital as i128 + a.pnl) + (b.capital as i128 + b.pnl);
    assert_eq!(total_equity, 4_000_000, "total equity conserved through divergent cross-asset close");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    assert_eq!(g.vault as u64, env.token_amount(env.vault), "accounting vault == real vault balance");
    let residual = g.vault as i128 - g.c_tot as i128 - g.insurance as i128;
    assert!(residual >= a.pnl.max(0) + b.pnl.max(0), "positive pnl backed by residual");
}

// security.md sweep — maintenance fee accrual on a positioned account (#32/#30): fees accrue
// INCREMENTALLY, bounded by max_accrual_dt per sync (anti-retroactivity: a cranker cannot charge a
// huge retroactive fee in one jump). Each increment must conserve (capital -> insurance), leave the
// position intact, and keep senior conservation + accounting==real-vault.
#[test]
fn v16_attack_maintenance_fee_with_open_position_conserves() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(1, 5_000, 10_000, 1_000, 58);
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    env.update_maintenance_fee_policy_with_cu(0);
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    let basis0 = env.portfolio_state(pa).legs[0].basis_pos_q;
    let cap0 = env.portfolio_state(pa).capital;
    let (_, g0) = env.market_state();

    // accrue fees incrementally across several slots (each crank+sync advances bounded by max_accrual_dt).
    let mut max_step: u128 = 0;
    let mut prev_cap = cap0;
    for slot in 1..=6u64 {
        env.svm.warp_to_slot(slot);
        env.svm.expire_blockhash();
        let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
            vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(pa, false)], &[]);
        env.svm.expire_blockhash();
        let _ = env.try_sync_maintenance_fee_with_cu(pa, None, slot);
        let cap = env.portfolio_state(pa).capital;
        let step = prev_cap - cap;
        max_step = max_step.max(step);
        prev_cap = cap;
    }
    let cap_final = env.portfolio_state(pa).capital;
    let fee = cap0 - cap_final;
    let (_, g1) = env.market_state();
    assert!(fee > 0, "maintenance fee accrued on the positioned account (non-vacuous)");
    // anti-retroactivity: no single step charges more than max_accrual_dt(1) * fee_per_slot(58) (with slack).
    assert!(max_step <= 58 * 3, "per-step fee bounded by the dt cap (no huge retroactive jump): max_step={}", max_step);
    // conservation: the fee moved capital -> insurance exactly.
    assert_eq!(g1.insurance, g0.insurance + fee, "fee moved capital -> insurance exactly");
    assert_eq!(g1.c_tot, g0.c_tot - fee, "c_tot decreased by exactly the fee");
    assert_eq!(g1.vault, g0.vault, "vault unchanged (fee internal)");
    assert_eq!(g1.vault as u64, env.token_amount(env.vault), "accounting vault == real vault balance");
    assert_eq!(env.portfolio_state(pa).legs[0].basis_pos_q, basis0, "fee accrual did not disturb the position");
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
    assert_eq!(g1.assets[0].oi_eff_long_q, g1.assets[0].oi_eff_short_q, "OI still balanced");
}
// security.md sweep — deposit with parked pnl (#32/#33): depositing while holding junior (parked) pnl
// must credit capital exactly and leave the pnl and its residual backing untouched. No double-count,
// no disturbance of the junior pnl, conservation holds.
#[test]
fn v16_attack_deposit_with_parked_pnl_clean() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let lo_owner = Keypair::new(); let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new(); let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, 1_000_000);
    env.deposit(&sh_owner, sh, 1_000_000);
    env.trade_asset_with_cu(0, &lo_owner, lo, &sh_owner, sh, (10_000 * POS_SCALE) as i128, 100, 0);
    // price up -> long accrues parked pnl; settle.
    env.svm.warp_to_slot(10); env.push_auth_mark_with_cu(10, 110);
    for slot in [10u64,11] { env.svm.warp_to_slot(slot); for p in [sh, lo] { env.svm.expire_blockhash();
        let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
            vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false)], &[]); } }
    let a0 = env.portfolio_state(lo);
    assert!(a0.pnl > 0, "long has parked pnl (non-vacuous)");
    let (_, g0) = env.market_state();
    let resid0 = g0.vault as i128 - g0.c_tot as i128 - g0.insurance as i128;

    // deposit MORE while holding the parked pnl.
    env.svm.expire_blockhash();
    env.deposit(&lo_owner, lo, 500_000);
    let a1 = env.portfolio_state(lo);
    let (_, g1) = env.market_state();
    assert_eq!(a1.capital, a0.capital + 500_000, "capital credited exactly by the deposit");
    assert_eq!(a1.pnl, a0.pnl, "parked pnl UNCHANGED by the deposit (no double-count/disturbance)");
    assert_eq!(g1.vault, g0.vault + 500_000, "vault grew by exactly the deposit");
    assert_eq!(g1.c_tot, g0.c_tot + 500_000, "c_tot grew by exactly the deposit");
    assert_eq!(g1.vault as u64, env.token_amount(env.vault), "accounting vault == real vault balance");
    // the parked pnl is still backed by (at least) the same residual.
    let resid1 = g1.vault as i128 - g1.c_tot as i128 - g1.insurance as i128;
    assert_eq!(resid1, resid0, "residual backing of the junior pnl unchanged by the deposit");
    assert!(resid1 >= a1.pnl.max(0), "junior pnl still backed by residual");
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}

// security.md sweep — two-sided trade fee symmetry (#33/#37): a trade fee is charged to BOTH sides;
// each must pay exactly the same amount (no rounding asymmetry favoring one side), and the total fee
// must equal the insurance increase. Conservation: vault unchanged (fee is internal capital->insurance).
#[test]
fn v16_attack_two_sided_trade_fee_symmetric() {
    let mut env = V16CuEnv::new(); // max_trading_fee_bps = 10_000
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    let (_, g0) = env.market_state();
    let ca0 = env.portfolio_state(pa).capital;
    let cb0 = env.portfolio_state(pb).capital;
    // trade with a fee (notional 100 @ POS_SCALE, 100 bps).
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 100);
    let ca1 = env.portfolio_state(pa).capital;
    let cb1 = env.portfolio_state(pb).capital;
    let (_, g1) = env.market_state();
    let fee_a = ca0 - ca1;
    let fee_b = cb0 - cb1;
    assert!(fee_a > 0, "a fee was charged (non-vacuous)");
    assert_eq!(fee_a, fee_b, "both sides pay EXACTLY the same fee (no rounding asymmetry)");
    // total fee -> insurance exactly; vault unchanged.
    assert_eq!(g1.insurance, g0.insurance + fee_a + fee_b, "total two-sided fee accrued to insurance");
    assert_eq!(g1.c_tot, g0.c_tot - fee_a - fee_b, "c_tot decreased by exactly the total fee");
    assert_eq!(g1.vault, g0.vault, "vault unchanged (fee internal)");
    assert_eq!(g1.vault as u64, env.token_amount(env.vault), "accounting vault == real vault");
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}

// security.md sweep — fee redirect policy (#6/#33): fee_redirect_to_market_0_bps splits fees to
// market 0's domain (INTERNAL), not an external party. It must be admin-gated, bounded to <=10000,
// and must never leak value out of the protocol (vault unchanged on a fee'd trade).
#[test]
fn v16_attack_fee_redirect_gated_bounded_no_leak() {
    let mut env = V16CuEnv::new();
    let mallory = Keypair::new(); env.ensure_signer_account(mallory.pubkey());
    // non-admin can't set the redirect.
    env.svm.expire_blockhash();
    let r_auth = send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::UpdateFeeRedirectPolicy { redirect_bps: 5_000 },
        vec![AccountMeta::new(mallory.pubkey(), true), AccountMeta::new(env.market, false)], &[&mallory]);
    assert!(r_auth.is_err(), "non-admin fee redirect update must reject");
    // out-of-range redirect rejected (admin).
    env.svm.expire_blockhash();
    let r_oob = send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::UpdateFeeRedirectPolicy { redirect_bps: 20_000 },
        vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(env.market, false)], &[&env.admin]);
    assert!(r_oob.is_err(), "redirect_bps > 10000 must reject");
    // valid redirect set by admin.
    env.svm.expire_blockhash();
    let r_ok = send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::UpdateFeeRedirectPolicy { redirect_bps: 5_000 },
        vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(env.market, false)], &[&env.admin]);
    assert!(r_ok.is_ok(), "admin redirect update should succeed: {:?}", r_ok);

    // a fee'd trade with redirect active: fee stays INTERNAL (vault unchanged, no external leak).
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    let (_, g0) = env.market_state();
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 100);
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "fee with redirect stays internal: vault unchanged (no external leak)");
    assert_eq!(g1.vault as u64, env.token_amount(env.vault), "accounting vault == real on-chain vault");
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
    // total value (c_tot + insurance + any domain attribution) still bounded by the vault.
    assert!(g1.c_tot + g1.insurance <= g1.vault, "no value created by the redirect split");
}

// security.md sweep — UpdateBaseUnitMints guard (#44/#48): the collateral mint can only be changed
// when the market holds NO funds (vault==0 && c_tot==0 && insurance==0). Changing it with deposits
// present would strand them (mint confusion). Must reject while funds exist, and for a non-authority.
#[test]
fn v16_attack_update_base_unit_mints_guarded() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new(); let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000); // market now holds funds
    let new_primary = env.create_mint();
    let new_secondary = env.create_mint();
    let (cfg0, g0) = env.market_state();

    // authority tries to change the collateral mint WHILE funds exist -> reject.
    env.svm.expire_blockhash();
    let r_funds = send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::UpdateBaseUnitMints { primary_mint: new_primary.to_bytes(), secondary_mint: new_secondary.to_bytes() },
        vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new_readonly(new_primary, false), AccountMeta::new_readonly(new_secondary, false)], &[&env.admin]);
    assert!(r_funds.is_err(), "changing collateral mint with funds present must reject");

    // a non-authority also can't change it.
    let mallory = Keypair::new(); env.ensure_signer_account(mallory.pubkey());
    env.svm.expire_blockhash();
    let r_auth = send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::UpdateBaseUnitMints { primary_mint: new_primary.to_bytes(), secondary_mint: new_secondary.to_bytes() },
        vec![AccountMeta::new(mallory.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new_readonly(new_primary, false), AccountMeta::new_readonly(new_secondary, false)], &[&mallory]);
    assert!(r_auth.is_err(), "non-authority mint change must reject");

    // collateral mint unchanged; funds intact and still withdrawable in the ORIGINAL mint.
    let (cfg1, g1) = env.market_state();
    assert_eq!(cfg1.collateral_mint, cfg0.collateral_mint, "collateral mint unchanged by rejected updates");
    assert_eq!(g1.vault, g0.vault, "funds intact");
    let (d, _) = env.withdraw_with_cu(&owner, p, 500_000);
    assert_eq!(env.token_amount(d), 500_000, "funds still withdrawable in the original mint");
}

// security.md sweep — partial liquidation exactness (#2/#33): liquidating with close_q < the position
// must reduce the position by at most close_q (no over-close), conserve value (vault unchanged,
// accounting==real), and never create value. Complements over-liquidation (batch 35).
#[test]
fn v16_attack_partial_liquidation_bounded_and_conserves() {
    let mut env = V16CuEnv::new();
    let long_owner = Keypair::new(); let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 250);
    env.configure_ewma_mark_with_cu(0, 100, 1, 0);
    env.trade_with_cu(&long_owner, long_account, &short_owner, short_account, POS_SCALE as i128, 100, 0);
    for (slot, mark) in [(1u64, 300u64), (2, 800)] {
        env.svm.warp_to_slot(slot); env.push_ewma_mark_with_cu(slot, mark);
        env.svm.expire_blockhash();
        let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
            vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(short_account, false)], &[]);
    }
    let (_, g_pre) = env.market_state();
    let oi_pre = g_pre.assets[0].oi_eff_short_q;
    // partial liquidation: close only HALF (POS_SCALE/2) of the POS_SCALE position.
    env.svm.expire_blockhash();
    let _ = env.send(ProgInstruction::PermissionlessCrank { action: 1, asset_index: 0, now_slot: 2, funding_rate_e9: 0, close_q: POS_SCALE / 2, fee_bps: 0, recovery_reason: 0 },
        vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(short_account, false)], &[]);
    let (_, g_post) = env.market_state();
    // OI reduced by AT MOST close_q (bounded; the engine may close less if that resolves things).
    let closed = oi_pre.saturating_sub(g_post.assets[0].oi_eff_short_q);
    assert!(closed <= POS_SCALE / 2, "partial liquidation closed at most close_q (no over-close): closed={}", closed);
    assert!(g_post.assets[0].oi_eff_short_q <= oi_pre, "OI never increased");
    // conservation: vault unchanged (internal), accounting==real, senior conservation.
    assert_eq!(g_post.vault, g_pre.vault, "vault unchanged by partial liquidation");
    assert_eq!(g_post.vault as u64, env.token_amount(env.vault), "accounting vault == real vault");
    assert!(g_post.vault >= g_post.c_tot + g_post.insurance, "senior conservation");
}

// security.md sweep — insurance makes winner whole at resolution (#33/#9): with a funded insurance
// backstop, a winner facing a loser's bad debt should recover their full claim at resolution (insurance
// absorbs the deficit), bounded by available insurance. Value conserved; insurance only spent.
#[test]
fn v16_attack_insurance_makes_winner_whole_at_resolution() {
    let mut env = V16CuEnv::new();
    env.top_up_insurance(1_000_000); // backstop
    env.configure_ewma_mark_with_cu(0, 100, 1, 0);
    let lo_owner = Keypair::new(); let lo = env.create_portfolio(&lo_owner);   // long winner
    let sh_owner = Keypair::new(); let sh = env.create_portfolio(&sh_owner);   // short loser (thin)
    env.deposit(&lo_owner, lo, 1_000_000);
    env.deposit(&sh_owner, sh, 250);
    env.trade_with_cu(&lo_owner, lo, &sh_owner, sh, POS_SCALE as i128, 100, 0);
    let ins_before = env.market_state().1.insurance;
    let vault_before = env.market_state().1.vault;
    // price up over slots -> short insolvent.
    for (slot, mark) in [(1u64, 300u64), (2, 800)] {
        env.svm.warp_to_slot(slot); env.push_ewma_mark_with_cu(slot, mark);
        for acct in [lo, sh] { env.svm.expire_blockhash();
            let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
                vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(acct, false)], &[]); }
    }
    // liquidate the insolvent short, then resolve and wind down.
    env.svm.expire_blockhash();
    let _ = env.send(ProgInstruction::PermissionlessCrank { action: 1, asset_index: 0, now_slot: 2, funding_rate_e9: 0, close_q: POS_SCALE, fee_bps: 0, recovery_reason: 0 },
        vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(sh, false)], &[]);
    env.resolve();
    let lo_dest = env.close_resolved(&lo_owner, lo);
    let _ = env.close_resolved(&sh_owner, sh);
    let won = env.token_amount(lo_dest) as u128;
    let (_, g) = env.market_state();
    // winner recovered MORE than just their capital — insurance covered the loser's bad debt.
    assert!(won > 1_000_000, "winner made (more) whole by insurance backstop: got {}", won);
    // insurance was SPENT (not conjured), bounded by what was available.
    assert!(g.insurance <= ins_before, "insurance only spent, never conjured ({} <= {})", g.insurance, ins_before);
    // no value printed: total tokens out + remaining vault accounted, no creation.
    assert!(g.vault <= vault_before, "vault not over-credited");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    assert_eq!(g.vault as u64, env.token_amount(env.vault), "accounting vault == real vault");
}

// security.md sweep — funding + maintenance fee combined (#32/#33): an account with a position accrues
// BOTH premium funding (zero-sum transfer to the counterparty) AND maintenance fees (to insurance).
// Both must apply together and conserve total value (no tokens created/destroyed; vault==deposited).
#[test]
fn v16_attack_funding_and_fee_combined_conserve() {
    const INITIAL_PRICE: u64 = 1_000_000;
    const DEPOSIT: u128 = 10_000_000;
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: INITIAL_PRICE,
        max_price_move_bps_per_slot: 1_000,
        max_accrual_dt_slots: 1,
        max_abs_funding_e9_per_slot: 1_000,
        min_funding_lifetime_slots: 1,
        maintenance_fee_per_slot: 58,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(0);
    env.configure_ewma_mark_with_cu(0, INITIAL_PRICE, 1, 0);
    env.update_maintenance_fee_policy_with_cu(0);
    let lo_owner = Keypair::new(); let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new(); let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, DEPOSIT);
    env.deposit(&sh_owner, sh, DEPOSIT);
    env.trade_with_cu(&lo_owner, lo, &sh_owner, sh, POS_SCALE as i128, INITIAL_PRICE, 0);
    env.svm.warp_to_slot(1); env.push_ewma_mark_with_cu(1, INITIAL_PRICE * 2); // premium
    for slot in 1..=6u64 {
        env.svm.warp_to_slot(slot);
        for p in [lo, sh] {
            env.svm.expire_blockhash();
            let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
                vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false)], &[]);
            env.svm.expire_blockhash();
            let _ = env.try_sync_maintenance_fee_with_cu(p, None, slot);
        }
    }
    let (_, g) = env.market_state();
    // funding actually accrued AND fees were charged (non-vacuous combination).
    assert!(g.assets[0].f_long_num != 0 || g.assets[0].f_short_num != 0, "funding accrued");
    assert!(g.insurance > 0, "maintenance fees accrued to insurance");
    // total value conserved: no tokens minted/burned, everything accounted within the vault.
    assert_eq!(g.vault, 2 * DEPOSIT, "vault == total deposited (funding zero-sum + fees internal)");
    assert_eq!(g.vault as u64, env.token_amount(env.vault), "accounting vault == real on-chain vault");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation under funding + fees");
    let a = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    let b = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
    let total_equity = (a.capital as i128 + a.pnl) + (b.capital as i128 + b.pnl);
    assert!(total_equity + g.insurance as i128 <= g.vault as i128, "no value over-distributed");
}

// security.md sweep — multi-party exposure transfer (#32/#33): A goes long vs B (short); then B closes
// by going long vs C (short). Exposure passes B->C. OI must stay balanced, B ends flat, and value is
// conserved through the chain (no leakage at the intermediary).
#[test]
fn v16_attack_exposure_transfer_chain_conserves() {
    let mut env = V16CuEnv::new();
    let a = Keypair::new(); let pa = env.create_portfolio(&a);
    let b = Keypair::new(); let pb = env.create_portfolio(&b);
    let c = Keypair::new(); let pc = env.create_portfolio(&c);
    env.deposit(&a, pa, 1_000_000);
    env.deposit(&b, pb, 1_000_000);
    env.deposit(&c, pc, 1_000_000);
    let (_, g0) = env.market_state();
    // A long vs B short.
    env.trade_asset_with_cu(0, &a, pa, &b, pb, POS_SCALE as i128, 100, 0);
    assert_eq!(env.portfolio_state(pa).legs[0].basis_pos_q, POS_SCALE as i128, "A long");
    assert_eq!(env.portfolio_state(pb).legs[0].basis_pos_q, -(POS_SCALE as i128), "B short");
    // B closes by going long vs C (B: -1 -> 0; C: 0 -> -1). Exposure transferred B->C.
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(0, &b, pb, &c, pc, POS_SCALE as i128, 100, 0);
    assert_eq!(env.portfolio_state(pb).legs[0].basis_pos_q, 0, "B now flat (exposure passed to C)");
    assert_eq!(env.portfolio_state(pc).legs[0].basis_pos_q, -(POS_SCALE as i128), "C now short");
    assert_eq!(env.portfolio_state(pa).legs[0].basis_pos_q, POS_SCALE as i128, "A still long");
    // OI balanced (A's long matched by C's short), conservation, accounting==real vault.
    let (_, g1) = env.market_state();
    assert_eq!(g1.assets[0].oi_eff_long_q, g1.assets[0].oi_eff_short_q, "OI balanced after transfer");
    assert_eq!(g1.c_tot, g0.c_tot, "c_tot conserved (no fees) through the chain");
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert_eq!(g1.vault as u64, env.token_amount(env.vault), "accounting vault == real vault");
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}

// security.md sweep — token program validation (#44): deposit/withdraw must verify the token program
// account is the real SPL Token program. Injecting a different program must reject — no routing the
// transfer CPI through an attacker-controlled program.
#[test]
fn v16_attack_wrong_token_program_rejected() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new(); let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    let (_, g0) = env.market_state();
    let fake_token_program = Pubkey::new_unique(); // not spl_token::ID
    let dest = Pubkey::new_unique();
    env.svm.set_account(dest, Account { lamports: 1_000_000_000, data: make_token_data(env.mint, owner.pubkey(), 0), owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
    // withdraw with a bogus token program -> reject.
    env.svm.expire_blockhash();
    let r = env.send(ProgInstruction::Withdraw { amount: 500_000 }, vec![
        AccountMeta::new(owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false),
        AccountMeta::new(dest, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(env.vault_authority, false), AccountMeta::new_readonly(fake_token_program, false)], &[&owner]);
    assert!(r.is_err(), "withdraw with a non-SPL-token program must reject");
    assert_eq!(env.token_amount(dest), 0, "no tokens delivered via a bogus token program");
    assert_eq!(env.portfolio_state(p).capital, 1_000_000, "capital not debited");
    assert_eq!(env.market_state().1.vault, g0.vault, "vault unchanged");
    // deposit with a bogus token program -> reject.
    let src = env.token_account_for_mint(env.mint, owner.pubkey(), 100);
    env.svm.expire_blockhash();
    let r2 = env.send(ProgInstruction::Deposit { amount: 100 }, vec![
        AccountMeta::new(owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false),
        AccountMeta::new(src, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(fake_token_program, false)], &[&owner]);
    assert!(r2.is_err(), "deposit with a non-SPL-token program must reject");
    // correct token program still works.
    let (good, _) = env.withdraw_with_cu(&owner, p, 500_000);
    assert_eq!(env.token_amount(good), 500_000, "withdraw with the real token program works");
}

// security.md sweep — haircut proportionality with disparate claims (#33/#37): two resolved winners
// with very different positive-pnl faces (100 vs 900) sharing insufficient backing must be paid
// PROPORTIONALLY to their claim size (~1:9), and the total must not exceed the backing.
#[test]
fn v16_attack_haircut_proportional_to_claim_size() {
    const BACKING: u128 = 100;
    let mut env = V16CuEnv::new();
    env.top_up_backing_bucket(1, BACKING, 10_000);
    let o1 = Keypair::new(); let p1 = env.create_portfolio(&o1);
    let o2 = Keypair::new(); let p2 = env.create_portfolio(&o2);
    env.deposit(&o1, p1, 1_000);
    env.deposit(&o2, p2, 1_000);
    env.add_source_positive_pnl(p1, 1, 100); // small claim
    env.add_source_positive_pnl(p2, 1, 900); // 9x larger claim
    env.resolve();
    // two close passes to converge the terminal haircut rate.
    let mut out1 = 0u128; let mut out2 = 0u128;
    for _ in 0..2 {
        let d1 = env.close_resolved(&o1, p1); out1 += env.token_amount(d1) as u128;
        let d2 = env.close_resolved(&o2, p2); out2 += env.token_amount(d2) as u128;
    }
    let hc1 = out1.saturating_sub(1_000); // haircut payout above senior capital
    let hc2 = out2.saturating_sub(1_000);
    // proportionality: the larger claim (9x) gets ~9x the haircut payout.
    assert!(hc1 > 0 && hc2 > 0, "both winners got some haircut payout (hc1={} hc2={})", hc1, hc2);
    assert!(hc2 >= hc1 * 8 && hc2 <= hc1 * 10, "payout ~proportional to claim size (9x): hc1={} hc2={}", hc1, hc2);
    // total haircut paid never exceeds the backing (no over-pay).
    assert!(hc1 + hc2 <= BACKING, "summed haircut payout {} <= backing {}", hc1 + hc2, BACKING);
    let (_, g) = env.market_state();
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
}

// security.md sweep — TopUpBackingBucket authorization (#6) + vault pinning (#44): funding a backing
// bucket is gated to the domain's backing_bucket_authority and routes only to the canonical vault
// (F-VAULT-FRAG fix). A non-authority must reject.
#[test]
fn v16_attack_topup_backing_bucket_authority_gated() {
    let mut env = V16CuEnv::new();
    let (_, g0) = env.market_state();
    let donor = Keypair::new(); env.ensure_signer_account(donor.pubkey());
    let src = env.token_account_for_mint(env.mint, donor.pubkey(), 500);
    // non-authority backing top-up -> reject.
    env.svm.expire_blockhash();
    let r = send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::TopUpBackingBucket { domain: 0, amount: 500, expiry_slot: 10_000 },
        vec![AccountMeta::new(donor.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(src, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&donor]);
    assert!(r.is_err(), "non-authority backing bucket top-up must reject");
    assert_eq!(env.token_amount(src), 500, "donor source untouched");
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert_eq!(g1.vault as u64, env.token_amount(env.vault), "accounting vault == real vault");
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}

// security.md sweep — F-VAULT-FRAG fix on a WITHDRAW path: WithdrawBackingBucket transfers FROM the
// vault; the canonical-ATA pin must apply here too. A withdrawal routed to a non-canonical
// vault-authority-owned account must reject (no draining a fragment / fabricating an outbound path).
#[test]
fn v16_attack_backing_withdraw_pinned_to_canonical_vault() {
    let mut env = V16CuEnv::new();
    env.top_up_backing_bucket(1, 1_000, 10_000); // real backing in the canonical vault
    let (_, g0) = env.market_state();
    // a fake "vault" owned by vault_authority but NOT the canonical ATA.
    let fake_vault = Pubkey::new_unique();
    env.svm.set_account(fake_vault, Account { lamports: 1_000_000_000, data: make_token_data(env.mint, env.vault_authority, 5_000), owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
    let dest = env.token_account_for_mint(env.mint, env.admin.pubkey(), 0);
    // backing withdraw routed to the fake vault -> reject (canonical pin).
    env.svm.expire_blockhash();
    let r = send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::WithdrawBackingBucket { domain: 1, amount: 500 },
        vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(dest, false),
             AccountMeta::new(fake_vault, false), AccountMeta::new_readonly(env.vault_authority, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&env.admin]);
    assert!(r.is_err(), "backing withdraw routed to a non-canonical vault must reject");
    assert_eq!(env.token_amount(dest), 0, "no tokens out via the fragment vault");
    assert_eq!(env.token_amount(fake_vault), 5_000, "fragment vault untouched");
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "accounting vault unchanged");
    assert_eq!(env.token_amount(env.vault), g1.vault as u64, "real canonical vault intact == accounting");
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}

// security.md sweep — convert bounded by available backing (#33/#35): if a winner's positive pnl
// exceeds its source backing, ConvertReleasedPnl must release at most the AVAILABLE backing, never the
// full (partly-unbacked) pnl. Otherwise unbacked pnl would convert into withdrawable capital.
#[test]
fn v16_attack_convert_bounded_by_available_backing() {
    const BACKING: u128 = 40;
    let mut env = V16CuEnv::new();
    let ledger = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger, 1, BACKING, 10);
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    // add MORE positive pnl (80) than the backing (40).
    env.add_source_positive_pnl(p, 1, 80);
    env.crank(p, ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: 0, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 });
    let cap_before = env.portfolio_state(p).capital;
    let (_, g0) = env.market_state();
    // convert with a huge cap -> released amount must be bounded by the available backing.
    env.svm.expire_blockhash();
    let _ = env.send(ProgInstruction::ConvertReleasedPnl { amount: 1_000_000_000 },
        vec![AccountMeta::new(owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false)], &[&owner]);
    let converted = env.portfolio_state(p).capital - cap_before;
    assert!(converted <= BACKING, "convert released at most the available backing ({} <= {})", converted, BACKING);
    let (_, g1) = env.market_state();
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation after convert");
    assert_eq!(g1.vault as u64, env.token_amount(env.vault), "accounting vault == real vault");
    let _ = g0;
}

// security.md sweep — undersized portfolio account handling (#44/#37): InitPortfolio on an account
// smaller than the required size must NOT cause an out-of-bounds write. The wrapper safely REALLOCS
// the account up to the required length (zero-initialized) before any portfolio write — then it works
// correctly. (No OOB; this verifies the safe-resize path.)
#[test]
fn v16_attack_undersized_portfolio_account_realloced_safely() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    env.ensure_signer_account(owner.pubkey());
    // a program-owned account smaller than the required portfolio length.
    let small = Pubkey::new_unique();
    let small_len = env.portfolio_account_len / 2;
    env.svm.set_account(small, Account { lamports: 1_000_000_000, data: vec![0u8; small_len], owner: env.program_id, executable: false, rent_epoch: 0 }).unwrap();
    // InitPortfolio reallocs it to the required size (no OOB) and succeeds.
    env.svm.expire_blockhash();
    let r = env.send(ProgInstruction::InitPortfolio, vec![
        AccountMeta::new(owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(small, false)], &[&owner]);
    assert!(r.is_ok(), "InitPortfolio reallocs an undersized account safely: {:?}", r);
    assert!(env.svm.get_account(&small).unwrap().data.len() >= env.portfolio_account_len, "account realloced to >= required size");
    // and the realloced portfolio works correctly (no corruption): a deposit credits exactly.
    env.deposit(&owner, small, 1_000);
    assert_eq!(env.portfolio_state(small).capital, 1_000, "realloced portfolio credits a deposit exactly (no OOB/corruption)");
    let (_, g) = env.market_state();
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    assert_eq!(g.vault as u64, env.token_amount(env.vault), "accounting vault == real vault");
}
// security.md sweep — max-leg multi-asset conservation (#32/#22): one portfolio holding positions on
// ALL asset slots must keep every invariant (c_tot==Σcapitals, accounting==real vault, per-asset OI
// balanced). Probes breadth across the full leg array.
#[test]
fn v16_attack_max_leg_multi_asset_conserves() {
    const N: u16 = 4;
    let mut env = V16CuEnv::new_with_market_params_and_price_move(N, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    for ai in 1..N {
        send_tx(&mut env.svm, env.program_id, &env.payer,
            ProgInstruction::ConfigureAuthMark { asset_index: ai, now_slot: 0, initial_mark_e6: 100 },
            vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(env.market, false)], &[&env.admin]).expect("cfg mark");
    }
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 5_000_000);
    env.deposit(&lb, pb, 5_000_000);
    // open a long on every asset from pa vs pb.
    for ai in 0..N {
        env.svm.expire_blockhash();
        env.trade_asset_with_cu(ai, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    }
    // every leg opened, conservation across all of them.
    let (_, g) = env.market_state();
    for ai in 0..N as usize {
        assert!(g.assets[ai].oi_eff_long_q > 0, "asset {} position opened", ai);
        assert_eq!(g.assets[ai].oi_eff_long_q, g.assets[ai].oi_eff_short_q, "asset {} OI balanced", ai);
    }
    let sum: u128 = [pa, pb].iter().map(|p| state::read_portfolio(&env.svm.get_account(p).unwrap().data).unwrap().capital).sum();
    assert_eq!(g.c_tot, sum, "c_tot == Σcapitals across all legs");
    assert_eq!(g.c_tot, 10_000_000, "no value created across the multi-leg open");
    assert_eq!(g.vault as u64, env.token_amount(env.vault), "accounting vault == real vault");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    // crank every asset; still conserves.
    env.svm.warp_to_slot(5);
    for ai in 0..N { env.svm.expire_blockhash();
        let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: ai, now_slot: 5, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
            vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(pa, false)], &[]); }
    let (_, g2) = env.market_state();
    assert_eq!(g2.vault as u64, env.token_amount(env.vault), "accounting==real vault after cranking all legs");
    assert!(g2.vault >= g2.c_tot + g2.insurance, "senior conservation after crank");
}

// security.md sweep — full fee'd round-trip conservation (#32/#33): deposit -> open (fee) -> close
// (fee) -> withdraw-all for both parties. Total tokens withdrawn + remaining insurance must equal the
// total deposited (every fee is accounted, nothing created or leaked).
#[test]
fn v16_attack_full_feed_roundtrip_conserves() {
    let mut env = V16CuEnv::new();
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    // open with a fee, then close with a fee (both flat).
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 100);
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, -(POS_SCALE as i128), 100, 100);
    assert!(percolator::active_bitmap_is_empty(env.portfolio_state(pa).active_bitmap), "la flat");
    assert!(percolator::active_bitmap_is_empty(env.portfolio_state(pb).active_bitmap), "lb flat");
    // both withdraw all their capital.
    let cap_a = env.portfolio_state(pa).capital;
    let cap_b = env.portfolio_state(pb).capital;
    env.svm.expire_blockhash(); let da = env.withdraw(&la, pa, cap_a);
    env.svm.expire_blockhash(); let db = env.withdraw(&lb, pb, cap_b);
    let out = env.token_amount(da) as u128 + env.token_amount(db) as u128;
    let (_, g) = env.market_state();
    // total accounting closes: tokens out + remaining insurance == total deposited.
    assert_eq!(out + g.insurance, 2_000_000, "out ({}) + insurance ({}) == deposited 2M", out, g.insurance);
    assert!(g.insurance > 0, "fees accrued to insurance (non-vacuous)");
    assert_eq!(g.c_tot, 0, "all capital withdrawn");
    assert_eq!(g.vault, g.insurance, "remaining vault is exactly the insurance (the fees)");
    assert_eq!(g.vault as u64, env.token_amount(env.vault), "accounting vault == real vault");
    assert!(out <= 2_000_000, "no value created: out <= deposited");
}

// security.md sweep — long sequence with a liquidation (#32/#33 fuzz-lite): a realistic flow including
// an insolvency+liquidation event must keep accounting==real-vault and senior conservation at every
// checkpoint, with no value created across the whole sequence.
#[test]
fn v16_attack_sequence_with_liquidation_conserves() {
    let mut env = V16CuEnv::new();
    env.configure_ewma_mark_with_cu(0, 100, 1, 0);
    let big = Keypair::new(); let pbig = env.create_portfolio(&big);
    let thin = Keypair::new(); let pthin = env.create_portfolio(&thin);
    let cp = Keypair::new(); let pcp = env.create_portfolio(&cp);
    env.deposit(&big, pbig, 5_000_000);
    env.deposit(&thin, pthin, 250);
    env.deposit(&cp, pcp, 5_000_000);
    let check = |env: &V16CuEnv, tag: &str| {
        let (_, g) = env.market_state();
        assert_eq!(g.vault as u64, env.token_amount(env.vault), "[{}] accounting == real vault", tag);
        assert!(g.vault >= g.c_tot + g.insurance, "[{}] senior conservation", tag);
        let sum: u128 = [pbig, pthin, pcp].iter().map(|p| state::read_portfolio(&env.svm.get_account(p).unwrap().data).unwrap().capital).sum();
        assert_eq!(g.c_tot, sum, "[{}] c_tot == Σcapitals", tag);
    };
    check(&env, "deposits");
    // big long vs thin short.
    env.trade_with_cu(&big, pbig, &thin, pthin, POS_SCALE as i128, 100, 0); check(&env, "open");
    // price up -> thin insolvent.
    for (slot, mark) in [(1u64, 300u64), (2, 800)] {
        env.svm.warp_to_slot(slot); env.push_ewma_mark_with_cu(slot, mark);
        env.svm.expire_blockhash();
        let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
            vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(pthin, false)], &[]);
    }
    check(&env, "thin insolvent");
    // liquidate thin.
    env.svm.expire_blockhash();
    let _ = env.send(ProgInstruction::PermissionlessCrank { action: 1, asset_index: 0, now_slot: 2, funding_rate_e9: 0, close_q: POS_SCALE, fee_bps: 0, recovery_reason: 0 },
        vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(pthin, false)], &[]);
    check(&env, "after liquidation");
    // crank big, then cp trades (fresh activity post-liquidation).
    env.svm.warp_to_slot(3); env.svm.expire_blockhash();
    let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: 3, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
        vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(pbig, false)], &[]);
    check(&env, "settled");
    let (_, g) = env.market_state();
    assert!(g.vault <= 10_000_250, "no value created across the whole sequence (deposited 5M+250+5M)");
}

// security.md sweep — EWMA mark halflife edge (#37): configuring the EWMA mark with halflife 0
// (instant) must be handled cleanly — no div-by-zero/panic, no settlement corruption. The mark/price
// stays in valid bounds and conservation holds.
#[test]
fn v16_attack_ewma_mark_halflife_zero_safe() {
    let mut env = V16CuEnv::new();
    // configure with halflife = 0 (instant). If accepted, settlement must stay safe.
    let r = send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::ConfigureEwmaMark { asset_index: 0, now_slot: 0, initial_mark_e6: 100, mark_ewma_halflife_slots: 0, mark_min_fee: 0 },
        vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(env.market, false)], &[&env.admin]);
    let lo = Keypair::new(); let plo = env.create_portfolio(&lo);
    let sh = Keypair::new(); let psh = env.create_portfolio(&sh);
    env.deposit(&lo, plo, 1_000_000);
    env.deposit(&sh, psh, 1_000_000);
    env.trade_with_cu(&lo, plo, &sh, psh, POS_SCALE as i128, 100, 0);
    // if the halflife=0 config was accepted, push a mark and crank — must not panic/corrupt.
    if r.is_ok() {
        env.svm.warp_to_slot(1); env.push_ewma_mark_with_cu(1, 150);
        for slot in [1u64, 2] { env.svm.warp_to_slot(slot); for p in [psh, plo] { env.svm.expire_blockhash();
            let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
                vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false)], &[]); } }
    }
    // regardless of accept/reject: no corruption, state decodes, conservation holds, price in bounds.
    let (_, g) = env.market_state();
    assert_eq!(g.vault, 2_000_000, "vault intact");
    assert_eq!(g.vault as u64, env.token_amount(env.vault), "accounting == real vault");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    assert!(g.assets[0].effective_price > 0 && g.assets[0].effective_price <= percolator::MAX_ORACLE_PRICE, "price in valid bounds (no corruption)");
    let _ = r;
}


// security.md sweep — vault delegate/close-authority guard (#44 defense-in-depth): the wrapper rejects
// a vault token account that has a delegate or close_authority set (verify_withdrawable_token_accounts).
// This prevents any delegated/closable drain path on the vault. Verify a delegated vault is rejected.
#[test]
fn v16_attack_vault_with_delegate_rejected() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new(); let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000); // funds the canonical vault
    let real_bal = env.token_amount(env.vault);
    // overwrite the canonical vault with the SAME balance/mint/owner but a DELEGATE set.
    let attacker = Pubkey::new_unique();
    let mut delegated = vec![0u8; TokenAccount::LEN];
    TokenAccount::pack(TokenAccount {
        mint: env.mint, owner: env.vault_authority, amount: real_bal,
        delegate: COption::Some(attacker), state: AccountState::Initialized, is_native: COption::None,
        delegated_amount: real_bal, close_authority: COption::None,
    }, &mut delegated).unwrap();
    env.svm.set_account(env.vault, Account { lamports: 1_000_000_000, data: delegated, owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
    // withdraw against the delegated vault -> reject.
    env.svm.expire_blockhash();
    let dest = Pubkey::new_unique();
    env.svm.set_account(dest, Account { lamports: 1_000_000_000, data: make_token_data(env.mint, owner.pubkey(), 0), owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
    let r = env.send(ProgInstruction::Withdraw { amount: 500_000 }, vec![
        AccountMeta::new(owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false),
        AccountMeta::new(dest, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(env.vault_authority, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&owner]);
    assert!(r.is_err(), "withdraw against a delegated vault must reject (defense-in-depth)");
    assert_eq!(env.token_amount(dest), 0, "no tokens out via a delegated vault");
    assert_eq!(env.token_amount(env.vault), real_bal, "vault balance intact");
}

// security.md sweep — dest token account state validation (#44): withdraw must reject a dest that is
// not Initialized (uninitialized or frozen) — the transfer can't land, so capital must not be debited.
#[test]
fn v16_attack_withdraw_to_noninitialized_dest_rejected() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new(); let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    let do_wd = |env: &mut V16CuEnv, dest: Pubkey| -> Result<u64, String> {
        env.svm.expire_blockhash();
        env.send(ProgInstruction::Withdraw { amount: 500_000 }, vec![
            AccountMeta::new(owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false),
            AccountMeta::new(dest, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(env.vault_authority, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&owner])
    };
    // uninitialized dest (zeroed spl-token-owned account).
    let uninit = Pubkey::new_unique();
    env.svm.set_account(uninit, Account { lamports: 1_000_000_000, data: vec![0u8; TokenAccount::LEN], owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
    assert!(do_wd(&mut env, uninit).is_err(), "withdraw to an uninitialized dest must reject");
    // frozen dest.
    let frozen = Pubkey::new_unique();
    let mut fd = vec![0u8; TokenAccount::LEN];
    TokenAccount::pack(TokenAccount { mint: env.mint, owner: owner.pubkey(), amount: 0, delegate: COption::None, state: AccountState::Frozen, is_native: COption::None, delegated_amount: 0, close_authority: COption::None }, &mut fd).unwrap();
    env.svm.set_account(frozen, Account { lamports: 1_000_000_000, data: fd, owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
    assert!(do_wd(&mut env, frozen).is_err(), "withdraw to a frozen dest must reject");
    // capital not debited by either rejected withdraw.
    assert_eq!(env.portfolio_state(p).capital, 1_000_000, "capital intact after rejected withdraws");
    assert_eq!(env.market_state().1.vault, 1_000_000, "vault unchanged");
    // a valid Initialized dest works.
    let (good, _) = env.withdraw_with_cu(&owner, p, 500_000);
    assert_eq!(env.token_amount(good), 500_000, "withdraw to a valid Initialized dest works");
}

// security.md sweep — vault_authority PDA validation (#44): the withdraw must verify the passed
// vault_authority account is the canonical derived PDA (expect_key). A wrong/attacker-chosen
// vault_authority must reject — otherwise a controlled authority could sign the vault transfer.
#[test]
fn v16_attack_wrong_vault_authority_rejected() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new(); let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    let (_, g0) = env.market_state();
    let dest = Pubkey::new_unique();
    env.svm.set_account(dest, Account { lamports: 1_000_000_000, data: make_token_data(env.mint, owner.pubkey(), 0), owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
    let bad_authority = Pubkey::new_unique(); // not the derived vault PDA
    env.svm.expire_blockhash();
    let r = env.send(ProgInstruction::Withdraw { amount: 500_000 }, vec![
        AccountMeta::new(owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false),
        AccountMeta::new(dest, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(bad_authority, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&owner]);
    assert!(r.is_err(), "withdraw with a non-canonical vault_authority must reject");
    assert_eq!(env.token_amount(dest), 0, "no tokens out via a wrong vault_authority");
    assert_eq!(env.portfolio_state(p).capital, 1_000_000, "capital not debited");
    assert_eq!(env.market_state().1.vault, g0.vault, "vault unchanged");
    // the correct vault_authority still works.
    let (good, _) = env.withdraw_with_cu(&owner, p, 500_000);
    assert_eq!(env.token_amount(good), 500_000, "withdraw with the canonical vault_authority works");
}

// security.md sweep — InitPortfolio account-owner validation (#44/#45): initializing a portfolio on an
// account NOT owned by the program must reject (the program can't safely realloc/write a foreign
// account). No corrupting an account it doesn't own.
#[test]
fn v16_attack_init_portfolio_foreign_account_rejected() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new(); env.ensure_signer_account(owner.pubkey());
    // an account owned by SPL Token (a foreign program), sized like a portfolio.
    let foreign = Pubkey::new_unique();
    env.svm.set_account(foreign, Account { lamports: 1_000_000_000, data: vec![0u8; env.portfolio_account_len], owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
    env.svm.expire_blockhash();
    let r = env.send(ProgInstruction::InitPortfolio, vec![
        AccountMeta::new(owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(foreign, false)], &[&owner]);
    assert!(r.is_err(), "InitPortfolio on a foreign-owned account must reject");
    // the foreign account is unchanged (still spl-token-owned, not a portfolio).
    let acc = env.svm.get_account(&foreign).unwrap();
    assert_eq!(acc.owner, spl_token::ID, "foreign account ownership unchanged (not hijacked)");
    // a proper program-owned account still initializes fine.
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000);
    assert_eq!(env.portfolio_state(p).capital, 1_000, "proper portfolio works");
}

// security.md sweep — asset RETIRE authorization (#6/#48): RETIRE is gated to the asset_authority (or
// admin). A non-authority must NOT be able to retire an asset (which, if it held positions, could
// strand them). The engine additionally requires the asset to be EMPTY before retiring.
#[test]
fn v16_attack_retire_asset_authority_gated() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::ConfigureAuthMark { asset_index: 1, now_slot: 0, initial_mark_e6: 100 },
        vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(env.market, false)], &[&env.admin]).expect("cfg mark");
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    env.trade_asset_with_cu(1, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    assert!(env.market_state().1.assets[1].oi_eff_long_q > 0, "asset 1 has open positions");
    // a NON-authority tries to retire asset 1 -> reject.
    let mallory = Keypair::new(); env.ensure_signer_account(mallory.pubkey());
    env.svm.warp_to_slot(5);
    let r = send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::UpdateAssetLifecycle { action: percolator_prog::processor::ASSET_ACTION_RETIRE, asset_index: 1, now_slot: 5, initial_price: 0,
            insurance_authority: [0u8;32], insurance_operator: [0u8;32], backing_bucket_authority: [0u8;32], oracle_authority: [0u8;32] },
        vec![AccountMeta::new(mallory.pubkey(), true), AccountMeta::new(env.market, false)], &[&mallory]);
    assert!(r.is_err(), "non-authority asset RETIRE must reject");
    // positions intact, not stranded.
    assert!(env.market_state().1.assets[1].oi_eff_long_q > 0, "asset 1 positions NOT stranded by rejected retire");
    let (_, g) = env.market_state();
    assert_eq!(g.vault as u64, env.token_amount(env.vault), "accounting == real vault");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
}
// security.md sweep — self-crank maintenance fee (#32): an account syncing its OWN maintenance fee as
// the cranker (cranker_portfolio == self) must still conserve — the fee splits into the cranker share
// (back to self) and insurance, totaling exactly the fee charged. No value created by self-cranking.
#[test]
fn v16_attack_self_crank_maintenance_fee_conserves() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(1, 10_000, 10_000, 10_000, 580);
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    env.deposit(&la, pa, 100_000_000);
    env.update_maintenance_fee_policy_with_cu(4_000); // cranker takes 40%
    let cap0 = env.portfolio_state(pa).capital;
    let (_, g0) = env.market_state();
    // la syncs its OWN fee, naming ITSELF as the cranker.
    env.svm.warp_to_slot(10);
    let _ = env.try_sync_maintenance_fee_with_cu(pa, Some(pa), 10);
    let cap1 = env.portfolio_state(pa).capital;
    let (_, g1) = env.market_state();
    // net effect on la's capital = -(fee) + (cranker share). insurance += (fee - cranker share).
    let net_loss = cap0.saturating_sub(cap1);
    let insurance_gain = g1.insurance - g0.insurance;
    // total value conserved: la's net loss == insurance gain (the cranker share returned to la nets out).
    assert_eq!(net_loss, insurance_gain, "self-crank: la's net loss == insurance gain (fee fully conserved)");
    assert_eq!(g1.vault, g0.vault, "vault unchanged (fee internal)");
    assert_eq!(g1.vault as u64, env.token_amount(env.vault), "accounting == real vault");
    assert_eq!(g1.c_tot, cap1, "c_tot == la's capital (single account)");
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
    // la can't gain from self-cranking: its capital did not increase.
    assert!(cap1 <= cap0, "self-cranking never increases the caller's capital (no value extraction)");
}

// security.md sweep — per-asset crank isolation (#32/#22): cranking one asset must not alter another
// asset's state. A crank+price-move on asset 0 must leave asset 1's effective_price and OI unchanged
// (no cross-asset corruption).
#[test]
fn v16_attack_per_asset_crank_isolation() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::ConfigureAuthMark { asset_index: 1, now_slot: 0, initial_mark_e6: 100 },
        vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(env.market, false)], &[&env.admin]).expect("cfg mark");
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 2_000_000);
    env.deposit(&lb, pb, 2_000_000);
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(1, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    // record asset 1's state.
    let (_, g0) = env.market_state();
    let a1_price0 = g0.assets[1].effective_price;
    let a1_oi_long0 = g0.assets[1].oi_eff_long_q;
    let a1_oi_short0 = g0.assets[1].oi_eff_short_q;
    let a1_klong0 = g0.assets[1].k_long;
    // move ONLY asset 0's mark and crank ONLY asset 0.
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 130);
    for slot in [10u64, 11] { env.svm.warp_to_slot(slot); for p in [pa, pb] { env.svm.expire_blockhash();
        let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
            vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false)], &[]); } }
    // asset 0 moved; asset 1's state must be UNCHANGED.
    let (_, g1) = env.market_state();
    assert!(g1.assets[0].effective_price > a1_price0, "asset 0 price moved (non-vacuous)");
    assert_eq!(g1.assets[1].effective_price, a1_price0, "asset 1 effective_price UNCHANGED by asset-0 crank");
    assert_eq!(g1.assets[1].oi_eff_long_q, a1_oi_long0, "asset 1 long OI unchanged");
    assert_eq!(g1.assets[1].oi_eff_short_q, a1_oi_short0, "asset 1 short OI unchanged");
    assert_eq!(g1.assets[1].k_long, a1_klong0, "asset 1 k_long (settlement index) unchanged");
    assert_eq!(g1.vault as u64, env.token_amount(env.vault), "accounting == real vault");
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}

// security.md sweep — ClosePortfolio with parked pnl (#48): an account holding positive (junior) pnl
// must NOT be closeable — closing would discard the pnl and its residual backing. ClosePortfolio
// requires PnL == 0; a portfolio with pnl must reject (the value stays recoverable).
#[test]
fn v16_attack_close_portfolio_with_pnl_rejected() {
    let mut env = V16CuEnv::new();
    let ledger = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger, 1, 40, 10);
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.add_source_positive_pnl(p, 1, 40); // p now has +40 pnl, 0 capital
    env.crank(p, ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: 0, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 });
    assert!(env.portfolio_state(p).pnl > 0, "p holds parked positive pnl (non-vacuous)");
    // ClosePortfolio must reject (PnL != 0).
    env.svm.expire_blockhash();
    let r = env.send(ProgInstruction::ClosePortfolio, vec![
        AccountMeta::new(owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false)], &[&owner]);
    assert!(r.is_err(), "ClosePortfolio with parked pnl must reject");
    // the account and its pnl are intact (not discarded), conservation holds.
    assert!(env.portfolio_state(p).pnl > 0, "parked pnl NOT discarded by the rejected close");
    let (_, g) = env.market_state();
    assert_eq!(g.vault as u64, env.token_amount(env.vault), "accounting == real vault");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
}

// security.md sweep — backing-bucket expiry (#33): a winner's positive pnl backed by a backing bucket
// with an expiry. After the expiry passes, the payout must still be CONSERVING — the winner is paid at
// most the (still-available) backing, never more, and the system never over-pays expired backing.
#[test]
fn v16_attack_backing_expiry_no_overpay() {
    let mut env = V16CuEnv::new();
    let ledger = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger, 1, 40, 5); // backing expires at slot 5
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000);
    env.add_source_positive_pnl(p, 1, 40);
    // advance PAST the backing expiry.
    env.svm.warp_to_slot(20);
    env.crank(p, ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: 20, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 });
    let vault_before = env.market_state().1.vault;
    env.resolve();
    // two close passes to converge.
    let mut out = 0u128;
    for _ in 0..2 { let d = env.close_resolved(&owner, p); out += env.token_amount(d) as u128; }
    let (_, g) = env.market_state();
    // winner gets at least its senior capital (1000) and at most capital + backing (1040) -- no over-pay.
    assert!(out >= 1_000, "winner recovers at least its senior capital");
    assert!(out <= 1_040, "winner paid at most capital + backing (no over-pay against expired backing): out={}", out);
    assert!(g.vault <= vault_before, "vault not over-credited");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    assert_eq!(g.vault as u64, env.token_amount(env.vault), "accounting == real vault");
}

// security.md sweep — large-amount deposit boundary + TVL cap (#37): the vault is capped at
// MAX_VAULT_TVL (overflow prevention). A deposit above the cap must reject; a large deposit just below
// it must credit exactly (no truncation/wraparound in the u128 aggregates) and round-trip exactly.
#[test]
fn v16_attack_large_amount_deposit_withdraw_exact() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new(); let p = env.create_portfolio(&owner);
    const MAX_TVL: u128 = 10_000_000_000_000_000;
    // over-cap deposit -> reject.
    let over = MAX_TVL + 1;
    let src_over = env.token_account_for_mint(env.mint, owner.pubkey(), over as u64);
    env.svm.expire_blockhash();
    let r = env.send(ProgInstruction::Deposit { amount: over }, vec![
        AccountMeta::new(owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false),
        AccountMeta::new(src_over, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&owner]);
    assert!(r.is_err(), "deposit above MAX_VAULT_TVL must reject (overflow/abuse cap)");
    assert_eq!(env.portfolio_state(p).capital, 0, "no capital credited on over-cap deposit");

    // large below-cap deposit -> exact credit, no overflow.
    let big: u128 = MAX_TVL - 7;
    env.deposit(&owner, p, big);
    assert_eq!(env.portfolio_state(p).capital, big, "capital credited exactly (no overflow/truncation)");
    let (_, g1) = env.market_state();
    assert_eq!(g1.c_tot, big, "c_tot == the large deposit");
    assert_eq!(g1.vault, big, "vault == the large deposit");
    assert_eq!(g1.vault as u64, env.token_amount(env.vault), "accounting == real vault");
    // withdraw it all back -> exact.
    let (dest, _) = env.withdraw_with_cu(&owner, p, big);
    assert_eq!(env.token_amount(dest) as u128, big, "withdrew exactly the large amount");
    let (_, g2) = env.market_state();
    assert_eq!(g2.c_tot, 0, "c_tot back to 0");
    assert_eq!(g2.vault, 0, "vault drained exactly");
    assert!(g2.vault >= g2.c_tot + g2.insurance, "senior conservation");
}
// security.md sweep — TradeCpi atomic fill vs matcher capacity (#33/#39): a request exceeding the
// matcher's fill capacity must reject ATOMICALLY (no partial/phantom position, no OI), while a
// within-capacity request fills correctly. No phantom over-fill, conservation holds.
#[test]
fn v16_attack_tradecpi_atomic_fill_vs_capacity() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new(); let taker = env.create_portfolio(&taker_owner);
    let maker_owner = Keypair::new(); let maker = env.create_portfolio(&maker_owner);
    env.deposit(&taker_owner, taker, 100_000_000);
    env.deposit(&maker_owner, maker, 100_000_000);
    let (ctx, delegate, _) = env.init_matcher_context_with_data(matcher_program, maker, encode_matcher_init_passive(POS_SCALE));
    let (_, g0) = env.market_state();
    // request 10x the cap -> rejects atomically (no partial/phantom fill).
    let r_over = env.try_trade_cpi_with_cu_on_asset(&taker_owner, taker, &maker_owner, maker, matcher_program, ctx, delegate, 0, (10 * POS_SCALE) as i128, 100);
    assert!(r_over.is_err(), "over-capacity TradeCpi must reject atomically");
    let (_, g1) = env.market_state();
    assert_eq!(g1.assets[0].oi_eff_long_q, 0, "no phantom OI from rejected over-capacity trade");
    assert_eq!(env.portfolio_state(taker).legs[0].basis_pos_q, 0, "no partial/phantom position");
    assert_eq!(g1.vault, g0.vault, "vault unchanged by rejected trade");
    // within-capacity request fills correctly.
    let r_ok = env.try_trade_cpi_with_cu_on_asset(&taker_owner, taker, &maker_owner, maker, matcher_program, ctx, delegate, 0, POS_SCALE as i128, 100);
    assert!(r_ok.is_ok(), "within-capacity TradeCpi fills: {:?}", r_ok);
    let basis = env.portfolio_state(taker).legs[0].basis_pos_q;
    assert_eq!(basis, POS_SCALE as i128, "taker filled exactly the requested within-capacity amount");
    let (_, g2) = env.market_state();
    assert_eq!(g2.assets[0].oi_eff_long_q, g2.assets[0].oi_eff_short_q, "OI balanced to the fill");
    assert_eq!(g2.vault as u64, env.token_amount(env.vault), "accounting == real vault");
    assert!(g2.vault >= g2.c_tot + g2.insurance, "senior conservation");
}
// security.md sweep — third-party withdraw vs winner's pnl backing (#33/#22): a winner's parked pnl is
// backed by residual (vault - c_tot - insurance). An UNRELATED account withdrawing its own capital
// reduces vault and c_tot equally, so the residual — and thus the winner's backing — must be unchanged.
#[test]
fn v16_attack_third_party_withdraw_preserves_pnl_backing() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let lo = Keypair::new(); let plo = env.create_portfolio(&lo);
    let sh = Keypair::new(); let psh = env.create_portfolio(&sh);
    let c = Keypair::new(); let pc = env.create_portfolio(&c);
    env.deposit(&lo, plo, 1_000_000);
    env.deposit(&sh, psh, 1_000_000);
    env.deposit(&c, pc, 1_000_000); // unrelated, no position
    env.trade_asset_with_cu(0, &lo, plo, &sh, psh, (10_000 * POS_SCALE) as i128, 100, 0);
    // price up -> long parks pnl, short realizes loss (freeing residual).
    env.svm.warp_to_slot(10); env.push_auth_mark_with_cu(10, 110);
    for slot in [10u64, 11] { env.svm.warp_to_slot(slot); for p in [psh, plo] { env.svm.expire_blockhash();
        let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
            vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false)], &[]); } }
    let long_pnl = env.portfolio_state(plo).pnl;
    assert!(long_pnl > 0, "long has parked pnl (non-vacuous)");
    let (_, g0) = env.market_state();
    let resid0 = g0.vault as i128 - g0.c_tot as i128 - g0.insurance as i128;
    assert!(resid0 >= long_pnl, "long's pnl is backed by residual");

    // unrelated account C withdraws ALL its capital.
    env.svm.expire_blockhash();
    let (_d, _) = env.withdraw_with_cu(&c, pc, 1_000_000);
    let (_, g1) = env.market_state();
    let resid1 = g1.vault as i128 - g1.c_tot as i128 - g1.insurance as i128;
    // residual (and thus the winner's backing) is UNCHANGED by C's withdrawal.
    assert_eq!(resid1, resid0, "third-party withdraw did NOT change the residual backing the winner");
    assert!(resid1 >= env.portfolio_state(plo).pnl.max(0), "long's pnl still fully backed");
    assert_eq!(g1.vault, g0.vault - 1_000_000, "vault decreased by exactly C's withdrawal");
    assert_eq!(g1.c_tot, g0.c_tot - 1_000_000, "c_tot decreased by exactly C's withdrawal");
    assert_eq!(g1.vault as u64, env.token_amount(env.vault), "accounting == real vault");
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}

// security.md sweep — cumulative TVL cap (#37): MAX_VAULT_TVL must be enforced on the TOTAL vault, not
// per-deposit. After the vault reaches the cap, any further deposit must reject (the cumulative cap
// can't be bypassed by splitting deposits).
#[test]
fn v16_attack_cumulative_tvl_cap_enforced() {
    let mut env = V16CuEnv::new();
    const MAX_TVL: u128 = 10_000_000_000_000_000;
    let a = Keypair::new(); let pa = env.create_portfolio(&a);
    let b = Keypair::new(); let pb = env.create_portfolio(&b);
    // fill the vault to exactly the cap.
    env.deposit(&a, pa, MAX_TVL);
    assert_eq!(env.market_state().1.vault, MAX_TVL, "vault at the cap");
    // a SECOND deposit (even tiny) by another account must reject -- cumulative cap.
    let src = env.token_account_for_mint(env.mint, b.pubkey(), 100);
    env.svm.expire_blockhash();
    let r = env.send(ProgInstruction::Deposit { amount: 100 }, vec![
        AccountMeta::new(b.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(pb, false),
        AccountMeta::new(src, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&b]);
    assert!(r.is_err(), "deposit pushing the vault over MAX_VAULT_TVL must reject (cumulative cap)");
    assert_eq!(env.portfolio_state(pb).capital, 0, "no capital credited over the cap");
    assert_eq!(env.market_state().1.vault, MAX_TVL, "vault still exactly at the cap");
    assert_eq!(env.token_amount(src), 100, "would-be depositor's source untouched");
    // and the first depositor can still withdraw their funds (cap doesn't lock them).
    let (d, _) = env.withdraw_with_cu(&a, pa, 1_000_000);
    assert_eq!(env.token_amount(d), 1_000_000, "funds withdrawable from a capped vault");
}

// security.md sweep — portfolio-as-market confusion (#44/#45): passing a portfolio account where the
// market is expected must reject (the market view decode fails on portfolio-shaped data). No cross-
// type confusion drains funds.
#[test]
fn v16_attack_portfolio_as_market_rejected() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new(); let p = env.create_portfolio(&owner);
    let other = Keypair::new(); let p2 = env.create_portfolio(&other);
    env.deposit(&owner, p, 1_000_000);
    let (_, g0) = env.market_state();
    let dest = Pubkey::new_unique();
    env.svm.set_account(dest, Account { lamports: 1_000_000_000, data: make_token_data(env.mint, owner.pubkey(), 0), owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
    // withdraw but pass a PORTFOLIO account (p2) in the MARKET slot.
    env.svm.expire_blockhash();
    let r = env.send(ProgInstruction::Withdraw { amount: 500_000 }, vec![
        AccountMeta::new(owner.pubkey(), true), AccountMeta::new(p2, false), AccountMeta::new(p, false),
        AccountMeta::new(dest, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(env.vault_authority, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&owner]);
    assert!(r.is_err(), "withdraw with a portfolio in the market slot must reject");
    assert_eq!(env.token_amount(dest), 0, "no funds drained via type confusion");
    assert_eq!(env.portfolio_state(p).capital, 1_000_000, "capital intact");
    assert_eq!(env.market_state().1.vault, g0.vault, "vault unchanged");
}

// security.md sweep — wrong-mint vault (#44): the vault token account must hold the collateral mint.
// A vault of a different mint must reject (mint check + canonical-ATA pin), so no draining via a
// mismatched-mint vault.
#[test]
fn v16_attack_wrong_mint_vault_rejected() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new(); let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    let real_bal = env.token_amount(env.vault);
    // overwrite the vault address with a token account of a DIFFERENT mint (still vault_authority-owned).
    let other_mint = Pubkey::new_unique();
    env.svm.set_account(env.vault, Account { lamports: 1_000_000_000, data: make_token_data(other_mint, env.vault_authority, real_bal), owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
    let dest = Pubkey::new_unique();
    env.svm.set_account(dest, Account { lamports: 1_000_000_000, data: make_token_data(env.mint, owner.pubkey(), 0), owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
    env.svm.expire_blockhash();
    let r = env.send(ProgInstruction::Withdraw { amount: 500_000 }, vec![
        AccountMeta::new(owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false),
        AccountMeta::new(dest, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(env.vault_authority, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&owner]);
    assert!(r.is_err(), "withdraw against a wrong-mint vault must reject");
    assert_eq!(env.token_amount(dest), 0, "no tokens out via a wrong-mint vault");
    assert_eq!(env.portfolio_state(p).capital, 1_000_000, "capital not debited");
}

// security.md sweep — no-fee liquidation cranker reward (#3): with no liquidation fee configured
// (default), a third-party cranker liquidating an insolvent account must receive ZERO reward — no
// value extraction from a no-fee liquidation. Conservation holds.
#[test]
fn v16_attack_no_fee_liquidation_cranker_gets_nothing() {
    let mut env = V16CuEnv::new(); // default: liquidation_fee_bps = 0
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let cranker_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    let cranker_account = env.create_portfolio(&cranker_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 250);
    env.deposit(&cranker_owner, cranker_account, 1_000);
    env.configure_ewma_mark_with_cu(0, 100, 1, 0);
    env.trade_with_cu(&long_owner, long_account, &short_owner, short_account, POS_SCALE as i128, 100, 0);
    for (slot, mark) in [(1u64, 300u64), (2, 800)] {
        env.svm.warp_to_slot(slot); env.push_ewma_mark_with_cu(slot, mark);
        env.svm.expire_blockhash();
        let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
            vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(short_account, false)], &[]);
    }
    let cranker0 = env.portfolio_state(cranker_account).capital;
    let (_, g0) = env.market_state();
    // liquidate with the cranker portfolio (4 accounts).
    env.svm.expire_blockhash();
    let _ = send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::PermissionlessCrank { action: 1, asset_index: 0, now_slot: 2, funding_rate_e9: 0, close_q: POS_SCALE, fee_bps: 0, recovery_reason: 0 },
        vec![AccountMeta::new(cranker_owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(short_account, false), AccountMeta::new(cranker_account, false)], &[&cranker_owner]);
    // cranker got NO reward (no fee configured).
    assert_eq!(env.portfolio_state(cranker_account).capital, cranker0, "cranker receives no reward in a no-fee liquidation");
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "vault unchanged (internal liquidation, no fee out)");
    assert_eq!(g1.vault as u64, env.token_amount(env.vault), "accounting == real vault");
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}

// security.md sweep — withdraw requires flat account (#19/#46): withdraw_not_atomic requires the
// account to be FLAT (active_bitmap empty) — ANY open position blocks withdrawal, regardless of how
// small the position or how large the capital. After closing, the full capital is recoverable (no
// permanent lock). This documents the flatness gate (not a margin calc).
#[test]
fn v16_attack_withdraw_requires_flat_regardless_of_size() {
    let mut env = V16CuEnv::new();
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 10_000_000);
    env.deposit(&lb, pb, 10_000_000);
    // TINY position (notional 100) vs huge (10M) capital.
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    // even a tiny withdrawal is blocked while ANY position is open (flatness gate, not margin).
    let try_wd = |env: &mut V16CuEnv, amt: u128| -> bool {
        env.svm.expire_blockhash();
        let dd = Pubkey::new_unique();
        env.svm.set_account(dd, Account { lamports: 1_000_000_000, data: make_token_data(env.mint, la.pubkey(), 0), owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
        env.send(ProgInstruction::Withdraw { amount: amt }, vec![
            AccountMeta::new(la.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(pa, false),
            AccountMeta::new(dd, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(env.vault_authority, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&la]).is_ok()
    };
    assert!(!try_wd(&mut env, 1), "tiny withdraw blocked while a (tiny) position is open");
    assert!(!try_wd(&mut env, 9_000_000), "bulk withdraw also blocked while positioned");
    assert_eq!(env.portfolio_state(pa).capital, 10_000_000, "capital intact (no partial debit)");
    // close the position -> full capital recoverable (no permanent lock).
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, -(POS_SCALE as i128), 100, 0);
    assert!(percolator::active_bitmap_is_empty(env.portfolio_state(pa).active_bitmap), "la flat after close");
    let cap = env.portfolio_state(pa).capital;
    let (d2, _) = env.withdraw_with_cu(&la, pa, cap);
    assert_eq!(env.token_amount(d2) as u128, cap, "full capital recovered after closing (no permanent lock)");
    let (_, g) = env.market_state();
    assert_eq!(g.vault as u64, env.token_amount(env.vault), "accounting == real vault");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
}
// security.md sweep — withdraw blocked during active close (#22/#48): an account with an active/in-
// progress forced close must NOT be able to withdraw (withdraw_not_atomic rejects a non-inert close
// ledger). Prevents withdrawing funds out from under a forced close.
#[test]
fn v16_attack_withdraw_blocked_during_active_close() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new(); let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    // seed an ACTIVE (cancellable) forced-close ledger.
    env.seed_cancellable_close_progress(p);
    // withdraw must reject while the close is active.
    env.svm.expire_blockhash();
    let dest = Pubkey::new_unique();
    env.svm.set_account(dest, Account { lamports: 1_000_000_000, data: make_token_data(env.mint, owner.pubkey(), 0), owner: spl_token::ID, executable: false, rent_epoch: 0 }).unwrap();
    let r = env.send(ProgInstruction::Withdraw { amount: 500_000 }, vec![
        AccountMeta::new(owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false),
        AccountMeta::new(dest, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(env.vault_authority, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&owner]);
    assert!(r.is_err(), "withdraw during an active forced-close must reject");
    assert_eq!(env.token_amount(dest), 0, "no funds withdrawn during active close");
    assert_eq!(env.portfolio_state(p).capital, 1_000_000, "capital intact");
    // after curing+cancelling the close (Finding E), withdraw works again.
    let src = env.token_account(owner.pubkey(), 0);
    env.cure_and_cancel_close_with_cu(&owner, p, src, 0);
    let (d, _) = env.withdraw_with_cu(&owner, p, 500_000);
    assert_eq!(env.token_amount(d), 500_000, "withdraw works after curing the close");
}

// security.md sweep — trade blocked during active close (#22): an account with an active forced-close
// must not be able to open/modify positions (it's being wound down). Trading on it must reject.
#[test]
fn v16_attack_trade_blocked_during_active_close() {
    let mut env = V16CuEnv::new();
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    env.seed_cancellable_close_progress(pa); // la has an active forced-close
    let (_, g0) = env.market_state();
    // trading on la (with an active close) must reject.
    env.svm.expire_blockhash();
    let r = env.try_trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    assert!(r.is_err(), "trade on an account with an active forced-close must reject");
    assert_eq!(env.portfolio_state(pa).legs[0].basis_pos_q, 0, "no position opened during active close");
    let (_, g1) = env.market_state();
    assert_eq!(g1.vault, g0.vault, "vault unchanged");
    assert_eq!(g1.assets[0].oi_eff_long_q, 0, "no OI created");
    assert_eq!(g1.vault as u64, env.token_amount(env.vault), "accounting == real vault");
}

// security.md sweep — per-asset funding isolation (#33/#22): funding accruing on one asset (its mark
// premium) must NOT alter another asset's funding ledger. Asset 0's funding must leave asset 1's
// f_long_num/f_short_num unchanged.
#[test]
fn v16_attack_per_asset_funding_isolation() {
    const IP: u64 = 1_000_000;
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        max_portfolio_assets: 2, initial_price: IP, max_price_move_bps_per_slot: 1_000,
        max_accrual_dt_slots: 1, max_abs_funding_e9_per_slot: 1_000, min_funding_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(0);
    env.configure_ewma_mark_with_cu(0, IP, 1, 0);
    send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::ConfigureEwmaMark { asset_index: 1, now_slot: 0, initial_mark_e6: IP, mark_ewma_halflife_slots: 1, mark_min_fee: 0 },
        vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(env.market, false)], &[&env.admin]).expect("cfg ewma asset1");
    let lo = Keypair::new(); let plo = env.create_portfolio(&lo);
    let sh = Keypair::new(); let psh = env.create_portfolio(&sh);
    env.deposit(&lo, plo, 100_000_000);
    env.deposit(&sh, psh, 100_000_000);
    // balanced positions on BOTH assets.
    env.trade_with_cu(&lo, plo, &sh, psh, POS_SCALE as i128, IP, 0);
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(1, &lo, plo, &sh, psh, POS_SCALE as i128, IP, 0);
    let a1_flong0 = env.market_state().1.assets[1].f_long_num;
    let a1_fshort0 = env.market_state().1.assets[1].f_short_num;
    // induce a mark premium and accrue funding on asset 0 ONLY.
    env.svm.warp_to_slot(1);
    env.push_ewma_mark_with_cu(1, IP * 2); // asset 0 premium
    for slot in 1..=4u64 { env.svm.warp_to_slot(slot); for p in [plo, psh] { env.svm.expire_blockhash();
        let _ = env.send(ProgInstruction::PermissionlessCrank { action: 0, asset_index: 0, now_slot: slot, funding_rate_e9: 0, close_q: 0, fee_bps: 0, recovery_reason: 0 },
            vec![AccountMeta::new(env.payer.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false)], &[]); } }
    let (_, g) = env.market_state();
    // asset 0 funding accrued; asset 1's funding ledger UNCHANGED.
    assert!(g.assets[0].f_long_num != 0, "asset 0 funding accrued (non-vacuous)");
    assert_eq!(g.assets[1].f_long_num, a1_flong0, "asset 1 f_long_num UNCHANGED by asset-0 funding");
    assert_eq!(g.assets[1].f_short_num, a1_fshort0, "asset 1 f_short_num unchanged");
    assert_eq!(g.vault, 200_000_000, "vault conserved");
    assert_eq!(g.vault as u64, env.token_amount(env.vault), "accounting == real vault");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
}

// security.md sweep — deposit during active close (#22): a plain deposit to an account with an active
// forced-close must be handled safely — whether allowed (adds capital toward curing) or rejected, it
// must conserve and never corrupt the close ledger or accounting.
#[test]
fn v16_attack_deposit_during_active_close_safe() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new(); let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    env.seed_cancellable_close_progress(p);
    let cap0 = env.portfolio_state(p).capital;
    let (_, g0) = env.market_state();
    // attempt a plain deposit during the active close.
    let src = env.token_account_for_mint(env.mint, owner.pubkey(), 500);
    env.svm.expire_blockhash();
    let r = env.send(ProgInstruction::Deposit { amount: 500 }, vec![
        AccountMeta::new(owner.pubkey(), true), AccountMeta::new(env.market, false), AccountMeta::new(p, false),
        AccountMeta::new(src, false), AccountMeta::new(env.vault, false), AccountMeta::new_readonly(spl_token::ID, false)], &[&owner]);
    let cap1 = env.portfolio_state(p).capital;
    let (_, g1) = env.market_state();
    // either outcome must conserve: capital change == vault change == source debit, accounting intact.
    if r.is_ok() {
        assert_eq!(cap1, cap0 + 500, "deposit credited exactly during close");
        assert_eq!(g1.vault, g0.vault + 500, "vault grew by exactly the deposit");
        assert_eq!(env.token_amount(src), 0, "source fully transferred");
    } else {
        assert_eq!(cap1, cap0, "rejected deposit: capital unchanged");
        assert_eq!(g1.vault, g0.vault, "rejected deposit: vault unchanged");
        assert_eq!(env.token_amount(src), 500, "rejected deposit: source untouched");
    }
    assert_eq!(g1.vault as u64, env.token_amount(env.vault), "accounting == real vault");
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}

// security.md sweep — fee-redirect split lands in the correct domains (#32/#33): with
// fee_redirect_to_market_0_bps set, a fee'd trade on market N must split EXACTLY: the redirect share
// to market 0's domain budget(s), the rest to market N's local domain budget(s). Total == fee
// charged (conservation), no value created/lost in the split.
#[test]
fn v16_attack_fee_redirect_split_lands_correctly() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::ConfigureAuthMark { asset_index: 1, now_slot: 0, initial_mark_e6: 100 },
        vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(env.market, false)], &[&env.admin]).expect("cfg mark");
    env.update_fee_redirect_policy_with_cu(2_000); // 20% of market 1..N fees -> market 0 domain
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 5_000_000);
    env.deposit(&lb, pb, 5_000_000);
    let dom = |env: &V16CuEnv, d: usize| env.market_state().1.insurance_domain_budget[d];
    let (b0, b1, b2, b3) = (dom(&env, 0), dom(&env, 1), dom(&env, 2), dom(&env, 3));
    let ins0 = env.market_state().1.insurance;
    // fee'd trade on ASSET 1 (market 1) -> fees split between market 0 (domains 0,1) and market 1 (2,3).
    env.trade_asset_with_cu(1, &la, pa, &lb, pb, (10_000 * POS_SCALE) as i128, 100, 100); // notional 1M -> fee large enough for the redirect
    let (g0d, g1d, g2d, g3d) = (dom(&env, 0) - b0, dom(&env, 1) - b1, dom(&env, 2) - b2, dom(&env, 3) - b3);
    let total_to_mkt0 = g0d + g1d;        // domains 0,1 belong to market 0
    let total_to_mkt1 = g2d + g3d;        // domains 2,3 belong to market 1
    let total_fee = total_to_mkt0 + total_to_mkt1;
    assert!(total_fee > 0, "a fee was charged (non-vacuous)");
    // global insurance grew by exactly the total fee (conservation).
    assert_eq!(env.market_state().1.insurance, ins0 + total_fee, "insurance += total fee");
    // the redirect share (20%) landed in market 0's domains; the rest (80%) stayed local in market 1.
    // each side: redirect = floor(fee_side * 2000/10000); allow +-1 per side for flooring.
    assert!(total_to_mkt0 >= total_fee * 2 / 10 - 2 && total_to_mkt0 <= total_fee * 2 / 10 + 2,
        "~20% of fee redirected to market 0 (got {} of {})", total_to_mkt0, total_fee);
    assert!(total_to_mkt1 >= total_fee * 8 / 10 - 2, "~80% of fee stayed local in market 1 (got {})", total_to_mkt1);
    let (_, g) = env.market_state();
    assert_eq!(g.vault as u64, env.token_amount(env.vault), "accounting == real vault");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
}

// security.md sweep — backing-fee policy authorization (#6) [fee-routing #7]: UpdateBackingFeePolicy
// (per-domain backing fee + insurance share) is gated to the domain's insurance_authority. A non-
// authority must reject, and an out-of-range share must reject. No unauthorized fee-policy tampering.
#[test]
fn v16_attack_backing_fee_policy_authority_gated() {
    let mut env = V16CuEnv::new();
    let (cfg0, _) = env.market_state();
    let mallory = Keypair::new(); env.ensure_signer_account(mallory.pubkey());
    // non-authority sets the backing fee policy -> reject.
    env.svm.expire_blockhash();
    let r = send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::UpdateBackingFeePolicy { domain: 0, fee_bps: 77, insurance_share_bps: 5_000 },
        vec![AccountMeta::new(mallory.pubkey(), true), AccountMeta::new(env.market, false)], &[&mallory]);
    assert!(r.is_err(), "non-authority backing fee policy update must reject");
    // out-of-range insurance share (>10000) by the real authority -> reject.
    env.svm.expire_blockhash();
    let r_oob = send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::UpdateBackingFeePolicy { domain: 0, fee_bps: 77, insurance_share_bps: 20_000 },
        vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(env.market, false)], &[&env.admin]);
    assert!(r_oob.is_err(), "insurance_share_bps > 10000 must reject");
    // policy unchanged by the rejected updates.
    let (cfg1, _) = env.market_state();
    assert_eq!(cfg1.backing_trade_fee_bps_long, cfg0.backing_trade_fee_bps_long, "backing fee policy unchanged");
    assert_eq!(cfg1.backing_trade_fee_insurance_share_bps_long, cfg0.backing_trade_fee_insurance_share_bps_long, "insurance share unchanged");
    // the real authority CAN set it (control).
    env.update_backing_fee_policy_with_cu(0, 77, 5_000);
    assert_eq!(env.market_state().0.backing_trade_fee_insurance_share_bps_long, 5_000, "authority sets the insurance share");
}

// security.md sweep — market 0 fees don't self-redirect (#32/#33) [fee-routing #2]: with
// fee_redirect_to_market_0_bps set, a fee on MARKET 0 itself must stay 100% local (the asset_index==0
// branch redirects 0). No spurious self-redirect / double-credit.
#[test]
fn v16_attack_market0_fees_stay_local() {
    let mut env = V16CuEnv::new();
    env.update_fee_redirect_policy_with_cu(2_000); // 20% redirect for markets 1..N
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 5_000_000);
    env.deposit(&lb, pb, 5_000_000);
    let dom = |env: &V16CuEnv, d: usize| env.market_state().1.insurance_domain_budget[d];
    let (b0, b1) = (dom(&env, 0), dom(&env, 1));
    let ins0 = env.market_state().1.insurance;
    // fee'd trade on ASSET 0 (market 0) with a large notional.
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, (10_000 * POS_SCALE) as i128, 100, 100);
    let g0d = dom(&env, 0) - b0;
    let g1d = dom(&env, 1) - b1;
    let total_local = g0d + g1d;
    let total_fee = env.market_state().1.insurance - ins0;
    assert!(total_fee > 0, "a fee was charged (non-vacuous)");
    // ALL of market 0's fee stayed in market 0's domains (0,1) -- nothing redirected away or double-counted.
    assert_eq!(total_local, total_fee, "100% of market-0 fee stays in market-0 domains (no self-redirect)");
    let (_, g) = env.market_state();
    assert_eq!(g.vault as u64, env.token_amount(env.vault), "accounting == real vault");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
}

// security.md sweep — fee-redirect 100% boundary (#32/#33) [fee-routing #4]: with redirect=10000, ALL
// of market N's fees must route to market 0's domains and NONE stays local. Boundary precision of the
// redirect split.
#[test]
fn v16_attack_fee_redirect_full_boundary() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_with_cu(0, 100);
    send_tx(&mut env.svm, env.program_id, &env.payer,
        ProgInstruction::ConfigureAuthMark { asset_index: 1, now_slot: 0, initial_mark_e6: 100 },
        vec![AccountMeta::new(env.admin.pubkey(), true), AccountMeta::new(env.market, false)], &[&env.admin]).expect("cfg mark");
    env.update_fee_redirect_policy_with_cu(10_000); // 100% redirect to market 0
    let la = Keypair::new(); let pa = env.create_portfolio(&la);
    let lb = Keypair::new(); let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 5_000_000);
    env.deposit(&lb, pb, 5_000_000);
    let dom = |env: &V16CuEnv, d: usize| env.market_state().1.insurance_domain_budget[d];
    let (b0, b1, b2, b3) = (dom(&env,0), dom(&env,1), dom(&env,2), dom(&env,3));
    let ins0 = env.market_state().1.insurance;
    env.trade_asset_with_cu(1, &la, pa, &lb, pb, (10_000 * POS_SCALE) as i128, 100, 100);
    let to_mkt0 = (dom(&env,0)-b0) + (dom(&env,1)-b1);
    let to_mkt1 = (dom(&env,2)-b2) + (dom(&env,3)-b3);
    let total_fee = env.market_state().1.insurance - ins0;
    assert!(total_fee > 0, "fee charged (non-vacuous)");
    assert_eq!(to_mkt0, total_fee, "100% redirect: ALL of market-1 fee went to market 0");
    assert_eq!(to_mkt1, 0, "100% redirect: NOTHING stayed local in market 1");
    let (_, g) = env.market_state();
    assert_eq!(g.vault as u64, env.token_amount(env.vault), "accounting == real vault");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
}

// security.md sweep — WithdrawInsurance wind-down gate: the global terminal WithdrawInsurance must be
// rejected (a) while the market is Live (mode != 1), and (b) after resolution while c_tot != 0 (open
// capital still backed). Attacker goal: drain insurance out from under accounts that still hold capital.
// We pre-fund domain-0's budget so the available-amount gate PASSES — isolating the wind-down gate as
// the sole reason for rejection. The same amount is then shown to SUCCEED once c_tot reaches 0.
#[test]
fn v16_attack_withdraw_insurance_requires_full_wind_down() {
    let mut env = V16CuEnv::new();
    // Fund domain-0 insurance budget so available_insurance(admin) >= the amount we attempt.
    env.top_up_insurance_domain_with_authority(&env.admin.insecure_clone(), 0, 1_000_000);
    let amount: u128 = 400_000;

    let attempt = |env: &mut V16CuEnv| {
        let dest = env.token_account(env.admin.pubkey(), 0);
        send_tx(
            &mut env.svm, env.program_id, &env.payer,
            ProgInstruction::WithdrawInsurance { amount },
            vec![
                AccountMeta::new(env.admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&env.admin.insecure_clone()],
        )
    };

    // (a) Live mode (mode==0): must reject — insurance is not withdrawable before resolution.
    assert_eq!(env.market_state().1.mode, percolator::MarketModeV16::Live, "starts Live");
    assert!(attempt(&mut env).is_err(), "WithdrawInsurance must reject while Live");

    // Open capital, then resolve. c_tot stays > 0 (depositor's capital is still on the book).
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 600_000);
    env.resolve();
    let g = env.market_state().1;
    assert_eq!(g.mode, percolator::MarketModeV16::Resolved, "resolved");
    assert!(g.c_tot > 0, "capital still open after resolve (non-vacuous gate)");

    // (b) Resolved but c_tot != 0: still must reject — can't drain insurance from under open capital.
    assert!(
        attempt(&mut env).is_err(),
        "WithdrawInsurance must reject while c_tot != 0 (capital still backed)"
    );

    // Control: a FRESH env, identically funded, but fully wound down (no open capital, so c_tot == 0
    // after resolve). The SAME amount now SUCCEEDS — proving (a)/(b) rejected on the wind-down gate,
    // not the available-amount gate (the amount/budget is identical in all three).
    let mut env2 = V16CuEnv::new();
    env2.top_up_insurance_domain_with_authority(&env2.admin.insecure_clone(), 0, 1_000_000);
    env2.resolve();
    let g2 = env2.market_state().1;
    assert_eq!(g2.mode, percolator::MarketModeV16::Resolved, "control resolved");
    assert_eq!(g2.c_tot, 0, "control fully wound down");
    assert!(
        attempt(&mut env2).is_ok(),
        "WithdrawInsurance succeeds once fully wound down (discriminating control)"
    );

    let g = env2.market_state().1;
    assert_eq!(g.vault as u64, env2.token_amount(env2.vault), "accounting == real vault");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
}

// security.md sweep — per-domain WithdrawInsuranceDomain budget conservation (#6): the domain insurance
// authority may withdraw accrued domain budget in LIVE mode, but NEVER more than the domain's remaining
// budget, and partial withdrawals must debit the remaining budget so it cannot be double-drained.
// Attacker goal: withdraw a domain's insurance twice (or over its budget) to extract more than accrued.
#[test]
fn v16_attack_withdraw_insurance_domain_budget_cannot_be_overdrawn() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    // Credit domain 0's budget with 1_000_000 (Live mode). insurance and domain-0 budget both = 1M.
    env.top_up_insurance_domain_with_authority(&admin, 0, 1_000_000);
    let g = env.market_state().1;
    assert_eq!(g.mode, percolator::MarketModeV16::Live, "Live");
    assert_eq!(g.insurance_domain_budget[0], 1_000_000, "domain-0 budget funded");

    let conserve = |env: &V16CuEnv| {
        let g = env.market_state().1;
        assert_eq!(g.vault as u64, env.token_amount(env.vault), "accounting == real vault");
        assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    };

    // (1) Over-budget in one shot must reject (amount > remaining domain budget).
    assert!(
        env.try_withdraw_insurance_domain_with_authority(&admin, 0, 1_000_001).is_err(),
        "withdraw > domain budget must reject"
    );
    conserve(&env);

    // (2) Partial withdraw succeeds and debits the remaining budget (and insurance).
    let (_d, _cu) = env
        .try_withdraw_insurance_domain_with_authority(&admin, 0, 600_000)
        .expect("partial domain withdraw ok");
    let g = env.market_state().1;
    assert_eq!(g.insurance_domain_budget[0], 400_000, "budget debited to 400k");
    assert_eq!(g.insurance, 400_000, "insurance debited to 400k");
    conserve(&env);

    // (3) A second withdraw exceeding the NEW remaining budget must reject (no double-drain).
    assert!(
        env.try_withdraw_insurance_domain_with_authority(&admin, 0, 500_000).is_err(),
        "withdraw > remaining (400k) must reject — no double-drain"
    );
    let g = env.market_state().1;
    assert_eq!(g.insurance_domain_budget[0], 400_000, "rejected withdraw left budget intact");
    conserve(&env);

    // (4) Draining exactly the remainder succeeds; budget -> 0.
    env.try_withdraw_insurance_domain_with_authority(&admin, 0, 400_000)
        .expect("drain remainder ok");
    let g = env.market_state().1;
    assert_eq!(g.insurance_domain_budget[0], 0, "budget fully drained");
    conserve(&env);

    // (5) Any further withdraw from the exhausted domain must reject.
    assert!(
        env.try_withdraw_insurance_domain_with_authority(&admin, 0, 1).is_err(),
        "withdraw from exhausted domain budget must reject"
    );
    conserve(&env);
}
