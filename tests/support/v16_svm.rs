use litesvm::LiteSVM;
use percolator::POS_SCALE;
use percolator_prog::{
    ix::{BatchTradeCpiLeg, BatchTradeLeg, CrankObservationHint, Instruction as ProgInstruction},
    state,
    state::{AssetOracleProfileV16, MarketGroupV16, PortfolioAccountV16},
};
use solana_sdk::{
    account::Account,
    clock::Clock,
    compute_budget::ComputeBudgetInstruction,
    hash::{hash, hashv, Hash},
    instruction::{AccountMeta, Instruction},
    program_option::COption,
    program_pack::Pack,
    pubkey::Pubkey,
    signature::{keypair_from_seed, Keypair, Signer},
    system_instruction,
    transaction::Transaction,
};
use spl_token::state::{Account as TokenAccount, AccountState, Mint};
use std::path::{Path, PathBuf};

pub const ASSET_COUNT: usize = 3;
pub const USER_COUNT: usize = 4;
pub const EXIT_MAKER_INDEX: usize = USER_COUNT;
pub const PRIMARY_ACTOR_COUNT: usize = USER_COUNT + 1;
pub const INITIAL_PRICE: u64 = 1_000_000;
pub const USER_DEPOSIT: u128 = 100_000_000;
pub const EXIT_MAKER_DEPOSIT: u128 = 2_000_000_000;
pub const TX_CU_LIMIT: u64 = 1_400_000;
const TOKEN_BALANCE_PER_USER: u64 = 200_000_000;
pub const EXIT_MAKER_TOKEN_BALANCE: u64 = 2_500_000_000;
const FOREIGN_TOKEN_BALANCE: u64 = 200_000_000;
const MATCHER_CONTEXT_LEN: usize = 320;

fn next_control_sequence(current: u64) -> u64 {
    current.checked_add(1).expect("control sequence exhausted")
}

#[derive(Clone, Copy, Debug)]
pub struct MarketConfig {
    pub initial_price: u64,
    pub h_max: u64,
    pub min_nonzero_mm_req: u128,
    pub min_nonzero_im_req: u128,
    pub maintenance_margin_bps: u64,
    pub initial_margin_bps: u64,
    pub max_trading_fee_bps: u64,
    pub liquidation_fee_bps: u64,
    pub liquidation_fee_cap: u128,
    pub min_liquidation_abs: u128,
    pub max_price_move_bps_per_slot: u64,
    pub max_accrual_dt_slots: u64,
    pub max_abs_funding_e9_per_slot: u64,
    pub min_funding_lifetime_slots: u64,
    pub max_bankrupt_close_lifetime_slots: u64,
    pub public_b_chunk_atoms: u128,
    pub maintenance_fee_per_slot: u128,
    pub actor_deposits: [u128; PRIMARY_ACTOR_COUNT],
    pub actor_token_balances: [u64; PRIMARY_ACTOR_COUNT],
}

impl Default for MarketConfig {
    fn default() -> Self {
        Self {
            initial_price: INITIAL_PRICE,
            h_max: 10,
            min_nonzero_mm_req: 1,
            min_nonzero_im_req: 2,
            maintenance_margin_bps: 10_000,
            initial_margin_bps: 10_000,
            max_trading_fee_bps: 10_000,
            liquidation_fee_bps: 0,
            liquidation_fee_cap: 0,
            min_liquidation_abs: 0,
            max_price_move_bps_per_slot: 1_000,
            max_accrual_dt_slots: 4,
            max_abs_funding_e9_per_slot: 0,
            min_funding_lifetime_slots: 4,
            max_bankrupt_close_lifetime_slots: 100,
            public_b_chunk_atoms: percolator::MAX_VAULT_TVL,
            maintenance_fee_per_slot: 0,
            actor_deposits: [
                USER_DEPOSIT,
                USER_DEPOSIT,
                USER_DEPOSIT,
                USER_DEPOSIT,
                EXIT_MAKER_DEPOSIT,
            ],
            actor_token_balances: [
                TOKEN_BALANCE_PER_USER,
                TOKEN_BALANCE_PER_USER,
                TOKEN_BALANCE_PER_USER,
                TOKEN_BALANCE_PER_USER,
                EXIT_MAKER_TOKEN_BALANCE,
            ],
        }
    }
}

pub struct Actor {
    pub signer: Keypair,
    pub portfolio: Pubkey,
    pub source_token: Pubkey,
    pub destination_token: Pubkey,
    pub matcher_context: Pubkey,
    pub matcher_delegate: Pubkey,
}

pub struct ForeignActor {
    pub signer: Keypair,
    pub portfolio: Pubkey,
    pub source_token: Pubkey,
    pub destination_token: Pubkey,
}

#[derive(Clone, Debug)]
pub struct TxSuccess {
    pub compute_units: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicTraceAccountMeta {
    pub key: Pubkey,
    pub is_signer: bool,
    pub is_writable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicTraceStep {
    pub program_id: Pubkey,
    pub instruction_data: Vec<u8>,
    pub fee_payer: Pubkey,
    pub transaction_signers: Vec<Pubkey>,
    pub accounts: Vec<PublicTraceAccountMeta>,
    pub succeeded: bool,
    pub compute_units: Option<u64>,
    pub rejected_exact_writable_rollback: Option<bool>,
    pub rejected_no_program_lamport_delta: Option<bool>,
    pub token_deltas: Vec<(Pubkey, i128)>,
    pub lamport_deltas: Vec<(Pubkey, i128)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicTraceEvidence {
    pub steps: Vec<PublicTraceStep>,
    pub out_of_band_economic_mutations: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TraceAccountState {
    lamports: u64,
    data: Vec<u8>,
    owner: Pubkey,
    executable: bool,
    rent_epoch: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TraceStateSnapshot(Vec<(Pubkey, Option<TraceAccountState>)>);

struct PublicTraceCapture {
    expected_state: TraceStateSnapshot,
    steps: Vec<PublicTraceStep>,
    out_of_band_economic_mutations: usize,
}

struct PendingPublicTraceStep {
    program_id: Pubkey,
    instruction_data: Vec<u8>,
    fee_payer: Pubkey,
    transaction_signers: Vec<Pubkey>,
    accounts: Vec<PublicTraceAccountMeta>,
    writable_before: Vec<(Pubkey, Option<TraceAccountState>)>,
    token_balances_before: Vec<(Pubkey, u64)>,
    lamports_before: Vec<(Pubkey, u64)>,
}

pub struct V16Svm {
    pub svm: LiteSVM,
    pub program_id: Pubkey,
    pub matcher_program: Pubkey,
    pub market: Pubkey,
    pub foreign_market: Pubkey,
    pub mint: Pubkey,
    pub vault: Pubkey,
    pub foreign_vault: Pubkey,
    pub vault_authority: Pubkey,
    pub foreign_vault_authority: Pubkey,
    pub provider_source_token: Pubkey,
    pub provider_destination_token: Pubkey,
    pub backing_domain_ledger: Pubkey,
    pub market_admin_destination_token: Pubkey,
    pub actors: Vec<Actor>,
    pub foreign_actor: ForeignActor,
    pub initial_token_supply: u128,
    pub loaded_program_hash: Hash,
    payer: Keypair,
    admin: Keypair,
    foreign_admin: Keypair,
    token_accounts: Vec<Pubkey>,
    tx_sequence: u64,
    public_trace: Option<PublicTraceCapture>,
}

impl V16Svm {
    pub fn new(seed: [u8; 32], config: MarketConfig) -> Self {
        let mut svm = LiteSVM::new();
        let program_id = percolator_prog::id();
        let program_bytes =
            std::fs::read(program_path()).expect("read production percolator SBF artifact");
        let loaded_program_hash = hash(&program_bytes);
        svm.add_program(program_id, &program_bytes);
        let token_program =
            std::fs::read(spl_token_program_path()).expect("read LiteSVM SPL Token artifact");
        svm.add_program(spl_token::ID, &token_program);
        let associated_token_program = std::fs::read(associated_token_program_path())
            .expect("read LiteSVM Associated Token artifact");
        svm.add_program(associated_token_program_id(), &associated_token_program);

        let matcher_program = deterministic_keypair(&seed, 6).pubkey();
        let matcher_bytes =
            std::fs::read(auth_matcher_program_path()).expect("read authenticated matcher SBF");
        svm.add_program(matcher_program, &matcher_bytes);

        let payer = deterministic_keypair(&seed, 1);
        let admin = deterministic_keypair(&seed, 2);
        let foreign_admin = deterministic_keypair(&seed, 3);
        let market = deterministic_keypair(&seed, 4).pubkey();
        let foreign_market = deterministic_keypair(&seed, 5).pubkey();
        let mint = deterministic_keypair(&seed, 7).pubkey();
        let vault_authority =
            Pubkey::find_program_address(&[b"vault", market.as_ref()], &program_id).0;
        let foreign_vault_authority =
            Pubkey::find_program_address(&[b"vault", foreign_market.as_ref()], &program_id).0;
        let vault = canonical_vault_ata(vault_authority, mint);
        let foreign_vault = canonical_vault_ata(foreign_vault_authority, mint);

        for signer in [&payer, &admin, &foreign_admin] {
            svm.airdrop(&signer.pubkey(), 100_000_000_000)
                .expect("airdrop fixture signer");
        }

        let portfolio_len =
            state::portfolio_account_len_for_market_slots(ASSET_COUNT).expect("portfolio len");
        let mut actors = Vec::with_capacity(PRIMARY_ACTOR_COUNT);
        let mut token_accounts = vec![vault, foreign_vault];
        let mut token_supply = 0u128;
        for i in 0..PRIMARY_ACTOR_COUNT {
            let signer = deterministic_keypair(&seed, 20 + i as u8);
            let portfolio = deterministic_keypair(&seed, 40 + i as u8).pubkey();
            let source_token = deterministic_keypair(&seed, 60 + i as u8).pubkey();
            let destination_token = deterministic_keypair(&seed, 80 + i as u8).pubkey();
            let matcher_context = deterministic_keypair(&seed, 100 + i as u8).pubkey();
            let matcher_delegate = matcher_delegate_key(
                &program_id,
                &market,
                &portfolio,
                &signer.pubkey(),
                &matcher_program,
                &matcher_context,
            );
            let source_balance = config.actor_token_balances[i];
            token_supply += source_balance as u128;
            token_accounts.extend([source_token, destination_token]);
            svm.airdrop(&signer.pubkey(), 10_000_000_000)
                .expect("airdrop primary actor");
            set_program_account(&mut svm, portfolio, program_id, portfolio_len);
            set_token_account(
                &mut svm,
                source_token,
                mint,
                signer.pubkey(),
                source_balance,
            );
            set_token_account(&mut svm, destination_token, mint, signer.pubkey(), 0);
            set_program_account(
                &mut svm,
                matcher_context,
                matcher_program,
                MATCHER_CONTEXT_LEN,
            );
            svm.set_account(
                matcher_delegate,
                Account {
                    lamports: 1_000_000_000,
                    data: vec![],
                    owner: Pubkey::default(),
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .expect("set matcher delegate fixture");
            actors.push(Actor {
                signer,
                portfolio,
                source_token,
                destination_token,
                matcher_context,
                matcher_delegate,
            });
        }

        let foreign_signer = deterministic_keypair(&seed, 120);
        let foreign_portfolio = deterministic_keypair(&seed, 121).pubkey();
        let foreign_source = deterministic_keypair(&seed, 122).pubkey();
        let foreign_destination = deterministic_keypair(&seed, 123).pubkey();
        let provider_source_token = deterministic_keypair(&seed, 124).pubkey();
        let provider_destination_token = deterministic_keypair(&seed, 125).pubkey();
        let backing_domain_ledger = deterministic_keypair(&seed, 126).pubkey();
        let market_admin_destination_token = deterministic_keypair(&seed, 127).pubkey();
        const PROVIDER_TOKEN_BALANCE: u64 = 1_000_000_000;
        token_supply += FOREIGN_TOKEN_BALANCE as u128;
        token_supply += PROVIDER_TOKEN_BALANCE as u128;
        token_accounts.extend([
            foreign_source,
            foreign_destination,
            provider_source_token,
            provider_destination_token,
            market_admin_destination_token,
        ]);
        svm.airdrop(&foreign_signer.pubkey(), 10_000_000_000)
            .expect("airdrop foreign actor");
        set_program_account(&mut svm, foreign_portfolio, program_id, portfolio_len);
        set_token_account(
            &mut svm,
            foreign_source,
            mint,
            foreign_signer.pubkey(),
            FOREIGN_TOKEN_BALANCE,
        );
        set_token_account(
            &mut svm,
            foreign_destination,
            mint,
            foreign_signer.pubkey(),
            0,
        );
        set_token_account(
            &mut svm,
            provider_source_token,
            mint,
            admin.pubkey(),
            PROVIDER_TOKEN_BALANCE,
        );
        set_token_account(
            &mut svm,
            provider_destination_token,
            mint,
            admin.pubkey(),
            0,
        );
        set_token_account(
            &mut svm,
            market_admin_destination_token,
            mint,
            admin.pubkey(),
            0,
        );
        set_program_account(
            &mut svm,
            backing_domain_ledger,
            program_id,
            state::backing_domain_ledger_account_len(),
        );
        let foreign_actor = ForeignActor {
            signer: foreign_signer,
            portfolio: foreign_portfolio,
            source_token: foreign_source,
            destination_token: foreign_destination,
        };

        set_mint_account(&mut svm, mint, token_supply as u64);
        set_token_account(&mut svm, vault, mint, vault_authority, 0);
        set_token_account(&mut svm, foreign_vault, mint, foreign_vault_authority, 0);
        set_program_account(
            &mut svm,
            market,
            program_id,
            state::market_account_len_for_capacity(ASSET_COUNT).expect("market len"),
        );
        set_program_account(
            &mut svm,
            foreign_market,
            program_id,
            state::market_account_len_for_capacity(ASSET_COUNT).expect("foreign market len"),
        );

        let mut out = Self {
            svm,
            program_id,
            matcher_program,
            market,
            foreign_market,
            mint,
            vault,
            foreign_vault,
            vault_authority,
            foreign_vault_authority,
            provider_source_token,
            provider_destination_token,
            backing_domain_ledger,
            market_admin_destination_token,
            actors,
            foreign_actor,
            initial_token_supply: token_supply,
            loaded_program_hash,
            payer,
            admin,
            foreign_admin,
            token_accounts,
            tx_sequence: 0,
            public_trace: None,
        };
        out.initialize_world(config);
        out
    }

    pub fn add_primary_actor(
        &mut self,
        seed: [u8; 32],
        actor_label: u8,
        source_balance: u64,
        deposit: u128,
    ) -> usize {
        let base = 130u8
            .checked_add(
                actor_label
                    .checked_mul(5)
                    .expect("extra actor label multiplication"),
            )
            .expect("extra actor label range");
        let signer = deterministic_keypair(&seed, base);
        let portfolio =
            deterministic_keypair(&seed, base.checked_add(1).expect("portfolio label")).pubkey();
        let source_token =
            deterministic_keypair(&seed, base.checked_add(2).expect("source label")).pubkey();
        let destination_token =
            deterministic_keypair(&seed, base.checked_add(3).expect("destination label")).pubkey();
        let matcher_context =
            deterministic_keypair(&seed, base.checked_add(4).expect("matcher label")).pubkey();
        let matcher_delegate = matcher_delegate_key(
            &self.program_id,
            &self.market,
            &portfolio,
            &signer.pubkey(),
            &self.matcher_program,
            &matcher_context,
        );
        self.svm
            .airdrop(&signer.pubkey(), 10_000_000_000)
            .expect("airdrop extra primary actor");
        set_program_account(
            &mut self.svm,
            portfolio,
            self.program_id,
            state::portfolio_account_len_for_market_slots(ASSET_COUNT).expect("portfolio len"),
        );
        set_token_account(
            &mut self.svm,
            source_token,
            self.mint,
            signer.pubkey(),
            source_balance,
        );
        set_token_account(
            &mut self.svm,
            destination_token,
            self.mint,
            signer.pubkey(),
            0,
        );
        set_program_account(
            &mut self.svm,
            matcher_context,
            self.matcher_program,
            MATCHER_CONTEXT_LEN,
        );
        self.svm
            .set_account(
                matcher_delegate,
                Account {
                    lamports: 1_000_000_000,
                    data: vec![],
                    owner: Pubkey::default(),
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .expect("set extra matcher delegate fixture");
        self.token_accounts
            .extend([source_token, destination_token]);
        self.initial_token_supply = self
            .initial_token_supply
            .checked_add(u128::from(source_balance))
            .expect("extra actor token supply");
        let actor_index = self.actors.len();
        self.actors.push(Actor {
            signer,
            portfolio,
            source_token,
            destination_token,
            matcher_context,
            matcher_delegate,
        });
        self.init_primary_portfolio(actor_index);
        self.init_matcher(actor_index);
        self.deposit_primary(actor_index, deposit)
            .expect("deposit extra primary actor");
        actor_index
    }

    fn initialize_world(&mut self, config: MarketConfig) {
        self.init_market(false, config);
        self.init_market(true, config);
        self.warp_to_slot(1);
        for asset_index in 0..ASSET_COUNT as u16 {
            self.configure_auth_mark(false, asset_index, 1, config.initial_price)
                .expect("configure primary AuthMark");
            self.configure_auth_mark(true, asset_index, 1, config.initial_price)
                .expect("configure foreign AuthMark");
        }

        for actor_index in 0..PRIMARY_ACTOR_COUNT {
            self.init_primary_portfolio(actor_index);
            self.init_matcher(actor_index);
            let deposit = config.actor_deposits[actor_index];
            self.deposit_primary(actor_index, deposit)
                .expect("initial primary deposit");
        }
        self.init_foreign_portfolio();
        self.deposit_foreign(USER_DEPOSIT)
            .expect("initial foreign deposit");
    }

    fn init_market(&mut self, foreign: bool, config: MarketConfig) {
        let (admin, market) = if foreign {
            (copy_keypair(&self.foreign_admin), self.foreign_market)
        } else {
            (copy_keypair(&self.admin), self.market)
        };
        self.send_program(
            ProgInstruction::InitMarket {
                max_portfolio_assets: ASSET_COUNT as u16,
                h_min: 0,
                h_max: config.h_max,
                initial_price: config.initial_price,
                min_nonzero_mm_req: config.min_nonzero_mm_req,
                min_nonzero_im_req: config.min_nonzero_im_req,
                maintenance_margin_bps: config.maintenance_margin_bps,
                initial_margin_bps: config.initial_margin_bps,
                max_trading_fee_bps: config.max_trading_fee_bps,
                trade_fee_base_bps: 0,
                liquidation_fee_bps: config.liquidation_fee_bps,
                liquidation_fee_cap: config.liquidation_fee_cap,
                min_liquidation_abs: config.min_liquidation_abs,
                max_price_move_bps_per_slot: config.max_price_move_bps_per_slot,
                max_accrual_dt_slots: config.max_accrual_dt_slots,
                max_abs_funding_e9_per_slot: config.max_abs_funding_e9_per_slot,
                min_funding_lifetime_slots: config.min_funding_lifetime_slots,
                max_account_b_settlement_chunks: 1,
                max_bankrupt_close_chunks: 1,
                max_bankrupt_close_lifetime_slots: config.max_bankrupt_close_lifetime_slots,
                public_b_chunk_atoms: config.public_b_chunk_atoms,
                maintenance_fee_per_slot: config.maintenance_fee_per_slot,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(market, false),
                AccountMeta::new_readonly(self.mint, false),
            ],
            &[admin],
        )
        .expect("initialize public market");
    }

    fn init_primary_portfolio(&mut self, actor_index: usize) {
        let owner = copy_keypair(&self.actors[actor_index].signer);
        let portfolio = self.actors[actor_index].portfolio;
        self.send_program(
            ProgInstruction::InitPortfolio,
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[owner],
        )
        .expect("initialize primary portfolio");
    }

    fn init_foreign_portfolio(&mut self) {
        let owner = copy_keypair(&self.foreign_actor.signer);
        self.send_program(
            ProgInstruction::InitPortfolio,
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.foreign_market, false),
                AccountMeta::new(self.foreign_actor.portfolio, false),
            ],
            &[owner],
        )
        .expect("initialize foreign portfolio");
    }

    fn init_matcher(&mut self, actor_index: usize) {
        let owner = copy_keypair(&self.actors[actor_index].signer);
        let context = self.actors[actor_index].matcher_context;
        let delegate = self.actors[actor_index].matcher_delegate;
        let portfolio = self.actors[actor_index].portfolio;
        self.send_raw_instruction(
            Instruction {
                program_id: self.matcher_program,
                accounts: vec![
                    AccountMeta::new_readonly(owner.pubkey(), true),
                    AccountMeta::new_readonly(delegate, false),
                    AccountMeta::new(context, false),
                    AccountMeta::new_readonly(self.program_id, false),
                    AccountMeta::new_readonly(self.market, false),
                    AccountMeta::new_readonly(portfolio, false),
                ],
                data: vec![2],
            },
            &[copy_keypair(&owner)],
        )
        .expect("initialize authenticated matcher context");
        let portfolio_id = self.primary_portfolio_id(actor_index);
        let expected_sequence = self.primary_portfolio_matcher_sequence(actor_index);
        self.send_program(
            ProgInstruction::SetMatcherConfig {
                portfolio_id,
                expected_sequence,
                enabled: 1,
                trade_fee_cap_bps: 10_000,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new_readonly(self.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new_readonly(self.matcher_program, false),
                AccountMeta::new_readonly(context, false),
                AccountMeta::new_readonly(delegate, false),
            ],
            &[owner],
        )
        .expect("bind portfolio to authenticated matcher");
    }

    pub fn set_matcher_config(
        &mut self,
        actor_index: usize,
        enabled: u8,
    ) -> Result<TxSuccess, String> {
        self.set_matcher_config_with_trade_fee_cap(
            actor_index,
            enabled,
            if enabled == 0 { 0 } else { 10_000 },
        )
    }

    pub fn set_matcher_config_with_trade_fee_cap(
        &mut self,
        actor_index: usize,
        enabled: u8,
        trade_fee_cap_bps: u16,
    ) -> Result<TxSuccess, String> {
        assert!(enabled <= 1, "matcher enabled flag must be boolean");
        let portfolio_id = self.primary_portfolio_id(actor_index);
        let expected_sequence = self.primary_portfolio_matcher_sequence(actor_index);
        let owner = copy_keypair(&self.actors[actor_index].signer);
        let portfolio = self.actors[actor_index].portfolio;
        let matcher_context = self.actors[actor_index].matcher_context;
        let matcher_delegate = self.actors[actor_index].matcher_delegate;
        let mut accounts = vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new_readonly(self.market, false),
            AccountMeta::new(portfolio, false),
        ];
        if enabled == 1 {
            accounts.extend([
                AccountMeta::new_readonly(self.matcher_program, false),
                AccountMeta::new_readonly(matcher_context, false),
                AccountMeta::new_readonly(matcher_delegate, false),
            ]);
        }
        self.send_program(
            ProgInstruction::SetMatcherConfig {
                portfolio_id,
                expected_sequence,
                enabled,
                trade_fee_cap_bps,
            },
            accounts,
            &[owner],
        )
    }

    pub fn build_retained_matcher_config(
        &mut self,
        actor_index: usize,
        enabled: u8,
    ) -> Transaction {
        self.build_retained_matcher_config_with_trade_fee_cap(
            actor_index,
            enabled,
            if enabled == 0 { 0 } else { 10_000 },
        )
    }

    pub fn build_retained_matcher_config_with_trade_fee_cap(
        &mut self,
        actor_index: usize,
        enabled: u8,
        trade_fee_cap_bps: u16,
    ) -> Transaction {
        assert!(enabled <= 1, "matcher enabled flag must be boolean");
        let portfolio_id = self.primary_portfolio_id(actor_index);
        let expected_sequence = self.primary_portfolio_matcher_sequence(actor_index);
        let owner = copy_keypair(&self.actors[actor_index].signer);
        let portfolio = self.actors[actor_index].portfolio;
        let matcher_context = self.actors[actor_index].matcher_context;
        let matcher_delegate = self.actors[actor_index].matcher_delegate;
        let mut accounts = vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new_readonly(self.market, false),
            AccountMeta::new(portfolio, false),
        ];
        if enabled == 1 {
            accounts.extend([
                AccountMeta::new_readonly(self.matcher_program, false),
                AccountMeta::new_readonly(matcher_context, false),
                AccountMeta::new_readonly(matcher_delegate, false),
            ]);
        }
        self.build_program_transaction(
            ProgInstruction::SetMatcherConfig {
                portfolio_id,
                expected_sequence,
                enabled,
                trade_fee_cap_bps,
            },
            accounts,
            &[owner],
        )
    }

    pub fn set_matcher_spreads(
        &mut self,
        actor_index: usize,
        bid_spread_bps: u64,
        ask_spread_bps: u64,
    ) -> Result<TxSuccess, String> {
        let actor = &self.actors[actor_index];
        let owner = copy_keypair(&actor.signer);
        let mut data = vec![4];
        data.extend_from_slice(&bid_spread_bps.to_le_bytes());
        data.extend_from_slice(&ask_spread_bps.to_le_bytes());
        self.send_raw_instruction(
            Instruction {
                program_id: self.matcher_program,
                accounts: vec![
                    AccountMeta::new_readonly(owner.pubkey(), true),
                    AccountMeta::new(actor.matcher_context, false),
                ],
                data,
            },
            &[owner],
        )
    }

    pub fn set_matcher_backing_fee_cap(
        &mut self,
        actor_index: usize,
        backing_fee_cap_bps: u16,
    ) -> Result<TxSuccess, String> {
        let actor = &self.actors[actor_index];
        let owner = copy_keypair(&actor.signer);
        let mut data = vec![5];
        data.extend_from_slice(&backing_fee_cap_bps.to_le_bytes());
        self.send_raw_instruction(
            Instruction {
                program_id: self.matcher_program,
                accounts: vec![
                    AccountMeta::new_readonly(owner.pubkey(), true),
                    AccountMeta::new(actor.matcher_context, false),
                ],
                data,
            },
            &[owner],
        )
    }

    pub fn deposit_primary(
        &mut self,
        actor_index: usize,
        amount: u128,
    ) -> Result<TxSuccess, String> {
        let actor = &self.actors[actor_index];
        let owner = copy_keypair(&actor.signer);
        let portfolio = actor.portfolio;
        let source = actor.source_token;
        let portfolio_id = self.primary_portfolio_id(actor_index);
        let expected_sequence = self.primary_portfolio_matcher_sequence(actor_index);
        self.send_program(
            ProgInstruction::Deposit {
                portfolio_id,
                expected_sequence,
                amount,
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
    }

    fn deposit_foreign(&mut self, amount: u128) -> Result<TxSuccess, String> {
        let owner = copy_keypair(&self.foreign_actor.signer);
        let portfolio_id = self.foreign_portfolio_id();
        let expected_sequence = self.foreign_portfolio_matcher_sequence();
        self.send_program(
            ProgInstruction::Deposit {
                portfolio_id,
                expected_sequence,
                amount,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.foreign_market, false),
                AccountMeta::new(self.foreign_actor.portfolio, false),
                AccountMeta::new(self.foreign_actor.source_token, false),
                AccountMeta::new(self.foreign_vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[owner],
        )
    }

    pub fn withdraw_foreign(&mut self, amount: u128) -> Result<TxSuccess, String> {
        let owner = copy_keypair(&self.foreign_actor.signer);
        let portfolio_id = self.foreign_portfolio_id();
        let expected_sequence = self.foreign_portfolio_matcher_sequence();
        self.send_program(
            ProgInstruction::Withdraw {
                portfolio_id,
                expected_sequence,
                amount,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.foreign_market, false),
                AccountMeta::new(self.foreign_actor.portfolio, false),
                AccountMeta::new(self.foreign_actor.destination_token, false),
                AccountMeta::new(self.foreign_vault, false),
                AccountMeta::new_readonly(self.foreign_vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[owner],
        )
    }

    pub fn withdraw_primary(
        &mut self,
        actor_index: usize,
        amount: u128,
    ) -> Result<TxSuccess, String> {
        let actor = &self.actors[actor_index];
        let owner = copy_keypair(&actor.signer);
        let portfolio_id = self.primary_portfolio_id(actor_index);
        let expected_sequence = self.primary_portfolio_matcher_sequence(actor_index);
        self.send_program(
            ProgInstruction::Withdraw {
                portfolio_id,
                expected_sequence,
                amount,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(actor.portfolio, false),
                AccountMeta::new(actor.destination_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[owner],
        )
    }

    pub fn close_primary_portfolio(&mut self, actor_index: usize) -> Result<TxSuccess, String> {
        let actor = &self.actors[actor_index];
        let owner = copy_keypair(&actor.signer);
        let portfolio_id = self.primary_portfolio_id(actor_index);
        let expected_sequence = self.primary_portfolio_matcher_sequence(actor_index);
        let position_epoch = self.primary_portfolio_position_epoch(actor_index);
        self.send_program(
            ProgInstruction::ClosePortfolio {
                portfolio_id,
                expected_sequence,
                position_epoch,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(actor.portfolio, false),
            ],
            &[owner],
        )
    }

    pub fn build_retained_close_primary_portfolio(&mut self, actor_index: usize) -> Transaction {
        let actor = &self.actors[actor_index];
        let owner = copy_keypair(&actor.signer);
        let portfolio_id = self.primary_portfolio_id(actor_index);
        let expected_sequence = self.primary_portfolio_matcher_sequence(actor_index);
        let position_epoch = self.primary_portfolio_position_epoch(actor_index);
        self.build_program_transaction(
            ProgInstruction::ClosePortfolio {
                portfolio_id,
                expected_sequence,
                position_epoch,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(actor.portfolio, false),
            ],
            &[owner],
        )
    }

    pub fn cure_and_cancel_primary_close(
        &mut self,
        actor_index: usize,
        optional_deposit: u128,
    ) -> Result<TxSuccess, String> {
        let portfolio_id = self.primary_portfolio_id(actor_index);
        let position_epoch = self.primary_portfolio_position_epoch(actor_index);
        let actor = &self.actors[actor_index];
        let owner = copy_keypair(&actor.signer);
        self.send_program(
            ProgInstruction::CureAndCancelClose {
                portfolio_id,
                position_epoch,
                optional_deposit,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(actor.portfolio, false),
                AccountMeta::new(actor.source_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[owner],
        )
    }

    pub fn build_retained_cure_and_cancel_primary_close(
        &mut self,
        actor_index: usize,
        optional_deposit: u128,
    ) -> Transaction {
        let portfolio_id = self.primary_portfolio_id(actor_index);
        let position_epoch = self.primary_portfolio_position_epoch(actor_index);
        let actor = &self.actors[actor_index];
        let owner = copy_keypair(&actor.signer);
        self.build_program_transaction(
            ProgInstruction::CureAndCancelClose {
                portfolio_id,
                position_epoch,
                optional_deposit,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(actor.portfolio, false),
                AccountMeta::new(actor.source_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[owner],
        )
    }

    pub fn fund_closed_primary_portfolio(
        &mut self,
        actor_index: usize,
        lamports: u64,
    ) -> Result<TxSuccess, String> {
        let portfolio = self.actors[actor_index].portfolio;
        let payer = self.payer.pubkey();
        self.send_raw_instruction(
            system_instruction::transfer(&payer, &portfolio, lamports),
            &[],
        )
    }

    pub fn reinitialize_primary_portfolio(
        &mut self,
        actor_index: usize,
    ) -> Result<TxSuccess, String> {
        let actor = &self.actors[actor_index];
        let owner = copy_keypair(&actor.signer);
        self.send_program(
            ProgInstruction::InitPortfolio,
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(actor.portfolio, false),
            ],
            &[owner],
        )
    }

    pub fn cycle_closed_primary_portfolio_through_owner(
        &mut self,
        actor_index: usize,
        intermediate_owner_index: usize,
    ) -> Result<(u64, u64), String> {
        assert_ne!(
            actor_index, intermediate_owner_index,
            "portfolio owner cycle requires a distinct intermediate owner"
        );
        let portfolio = self.actors[actor_index].portfolio;
        let intermediate_owner_init = copy_keypair(&self.actors[intermediate_owner_index].signer);
        let intermediate_owner_close = copy_keypair(&self.actors[intermediate_owner_index].signer);
        let intermediate_owner_pubkey = intermediate_owner_init.pubkey();

        self.fund_closed_primary_portfolio(actor_index, 1_000_000_000)?;
        self.send_program(
            ProgInstruction::InitPortfolio,
            vec![
                AccountMeta::new(intermediate_owner_pubkey, true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[intermediate_owner_init],
        )?;
        let intermediate_portfolio_id = self.primary_portfolio_id(actor_index);
        let expected_sequence = self.primary_portfolio_matcher_sequence(actor_index);
        let position_epoch = self.primary_portfolio_position_epoch(actor_index);
        self.send_program(
            ProgInstruction::ClosePortfolio {
                portfolio_id: intermediate_portfolio_id,
                expected_sequence,
                position_epoch,
            },
            vec![
                AccountMeta::new(intermediate_owner_pubkey, true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[intermediate_owner_close],
        )?;

        self.fund_closed_primary_portfolio(actor_index, 1_000_000_000)?;
        self.reinitialize_primary_portfolio(actor_index)?;
        Ok((
            intermediate_portfolio_id,
            self.primary_portfolio_id(actor_index),
        ))
    }

    pub fn primary_portfolio_id(&self, actor_index: usize) -> u64 {
        let data = self.primary_portfolio_data(actor_index);
        state::read_portfolio_id(&data).expect("decode primary portfolio id")
    }

    pub fn foreign_portfolio_id(&self) -> u64 {
        let account = self
            .svm
            .get_account(&self.foreign_actor.portfolio)
            .expect("foreign portfolio account");
        state::read_portfolio_id(&account.data).expect("decode foreign portfolio id")
    }

    pub fn foreign_portfolio_position_epoch(&self) -> u64 {
        let account = self
            .svm
            .get_account(&self.foreign_actor.portfolio)
            .expect("foreign portfolio account");
        state::read_portfolio_position_epoch(&account.data).expect("decode foreign position epoch")
    }

    pub fn foreign_portfolio_matcher_sequence(&self) -> u64 {
        let account = self
            .svm
            .get_account(&self.foreign_actor.portfolio)
            .expect("foreign portfolio account");
        state::read_portfolio_matcher_sequence(&account.data)
            .expect("decode foreign matcher sequence")
    }

    pub fn primary_portfolio_position_epoch(&self, actor_index: usize) -> u64 {
        let data = self.primary_portfolio_data(actor_index);
        state::read_portfolio_position_epoch(&data).expect("decode primary position epoch")
    }

    pub fn primary_portfolio_matcher_sequence(&self, actor_index: usize) -> u64 {
        let data = self.primary_portfolio_data(actor_index);
        state::read_portfolio_matcher_sequence(&data).expect("decode primary matcher sequence")
    }

    pub fn ensure_primary_matcher_enabled(
        &mut self,
        actor_index: usize,
    ) -> Result<Option<TxSuccess>, String> {
        let data = self.primary_portfolio_data(actor_index);
        let config = state::read_portfolio_matcher_config(&data)
            .map_err(|error| format!("decode primary matcher config: {error:?}"))?;
        if config.enabled() == 1 {
            return Ok(None);
        }
        self.set_matcher_config_with_trade_fee_cap(actor_index, 1, config.trade_fee_cap_bps())
            .map(Some)
    }

    pub fn resolve_market(&mut self) -> Result<TxSuccess, String> {
        let admin = copy_keypair(&self.admin);
        let asset_generation_frontier = self.primary_market_state().1.next_market_id;
        self.send_program(
            ProgInstruction::ResolveMarket {
                asset_generation_frontier,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[admin],
        )
    }

    pub fn close_primary_slab(&mut self) -> Result<TxSuccess, String> {
        let admin = copy_keypair(&self.admin);
        self.send_program(
            ProgInstruction::CloseSlab,
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new(self.market_admin_destination_token, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[admin],
        )
    }

    pub fn fund_closed_primary_market(&mut self) -> Result<TxSuccess, String> {
        let payer = self.payer.pubkey();
        self.send_raw_instruction(
            system_instruction::transfer(&payer, &self.market, 1_000_000_000),
            &[],
        )
    }

    pub fn recreate_primary_vault(&mut self) -> Result<TxSuccess, String> {
        self.send_raw_instruction(
            Instruction {
                program_id: associated_token_program_id(),
                accounts: vec![
                    AccountMeta::new(self.payer.pubkey(), true),
                    AccountMeta::new(self.vault, false),
                    AccountMeta::new_readonly(self.vault_authority, false),
                    AccountMeta::new_readonly(self.mint, false),
                    AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new_readonly(solana_sdk::sysvar::rent::ID, false),
                ],
                data: vec![],
            },
            &[],
        )
    }

    pub fn reinitialize_primary_market(
        &mut self,
        config: MarketConfig,
    ) -> Result<TxSuccess, String> {
        let admin = copy_keypair(&self.admin);
        self.send_program(
            ProgInstruction::InitMarket {
                max_portfolio_assets: ASSET_COUNT as u16,
                h_min: 0,
                h_max: config.h_max,
                initial_price: config.initial_price,
                min_nonzero_mm_req: config.min_nonzero_mm_req,
                min_nonzero_im_req: config.min_nonzero_im_req,
                maintenance_margin_bps: config.maintenance_margin_bps,
                initial_margin_bps: config.initial_margin_bps,
                max_trading_fee_bps: config.max_trading_fee_bps,
                trade_fee_base_bps: 0,
                liquidation_fee_bps: config.liquidation_fee_bps,
                liquidation_fee_cap: config.liquidation_fee_cap,
                min_liquidation_abs: config.min_liquidation_abs,
                max_price_move_bps_per_slot: config.max_price_move_bps_per_slot,
                max_accrual_dt_slots: config.max_accrual_dt_slots,
                max_abs_funding_e9_per_slot: config.max_abs_funding_e9_per_slot,
                min_funding_lifetime_slots: config.min_funding_lifetime_slots,
                max_account_b_settlement_chunks: 1,
                max_bankrupt_close_chunks: 1,
                max_bankrupt_close_lifetime_slots: config.max_bankrupt_close_lifetime_slots,
                public_b_chunk_atoms: config.public_b_chunk_atoms,
                maintenance_fee_per_slot: config.maintenance_fee_per_slot,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new_readonly(self.mint, false),
            ],
            &[admin],
        )
    }

    pub fn configure_permissionless_resolve(
        &mut self,
        stale_slots: u64,
        force_close_delay_slots: u64,
    ) -> Result<TxSuccess, String> {
        let policy_sequence =
            next_control_sequence(self.primary_control_sequences(0).permissionless_resolve);
        let asset_generation_frontier = self.primary_market_state().1.next_market_id;
        let admin = copy_keypair(&self.admin);
        self.send_program(
            ProgInstruction::ConfigurePermissionlessResolve {
                asset_generation_frontier,
                stale_slots,
                force_close_delay_slots,
                policy_sequence,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[admin],
        )
    }

    pub fn resolve_stale_permissionless(&mut self, now_slot: u64) -> Result<TxSuccess, String> {
        self.warp_to_slot(now_slot);
        self.send_program(
            ProgInstruction::ResolveStalePermissionless { now_slot },
            vec![AccountMeta::new(self.market, false)],
            &[],
        )
    }

    pub fn close_resolved_primary(&mut self, actor_index: usize) -> Result<TxSuccess, String> {
        let actor = &self.actors[actor_index];
        self.send_program(
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            },
            vec![
                AccountMeta::new_readonly(actor.signer.pubkey(), false),
                AccountMeta::new(self.market, false),
                AccountMeta::new(actor.portfolio, false),
                AccountMeta::new(actor.destination_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[],
        )
    }

    pub fn close_resolved_primary_signed(
        &mut self,
        actor_index: usize,
    ) -> Result<TxSuccess, String> {
        let actor = &self.actors[actor_index];
        let owner = copy_keypair(&actor.signer);
        self.send_program(
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            },
            vec![
                AccountMeta::new_readonly(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(actor.portfolio, false),
                AccountMeta::new(actor.destination_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[owner],
        )
    }

    pub fn claim_resolved_payout_topup_primary(
        &mut self,
        actor_index: usize,
    ) -> Result<TxSuccess, String> {
        let actor = &self.actors[actor_index];
        self.send_program(
            ProgInstruction::ClaimResolvedPayoutTopup,
            vec![
                AccountMeta::new_readonly(actor.signer.pubkey(), false),
                AccountMeta::new(self.market, false),
                AccountMeta::new(actor.portfolio, false),
                AccountMeta::new(actor.destination_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[],
        )
    }

    pub fn convert_released_pnl(
        &mut self,
        actor_index: usize,
        amount: u128,
    ) -> Result<TxSuccess, String> {
        let actor = &self.actors[actor_index];
        let owner = copy_keypair(&actor.signer);
        let portfolio_id = self.primary_portfolio_id(actor_index);
        let position_epoch = self.primary_portfolio_position_epoch(actor_index);
        self.send_program(
            ProgInstruction::ConvertReleasedPnl {
                portfolio_id,
                position_epoch,
                amount,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(actor.portfolio, false),
            ],
            &[owner],
        )
    }

    pub fn rebalance_reduce(
        &mut self,
        actor_index: usize,
        asset_index: u16,
        reduce_q: u128,
    ) -> Result<TxSuccess, String> {
        let portfolio_id = self.primary_portfolio_id(actor_index);
        let position_epoch = self.primary_portfolio_position_epoch(actor_index);
        let actor = &self.actors[actor_index];
        let owner = copy_keypair(&actor.signer);
        self.send_program(
            ProgInstruction::RebalanceReduce {
                portfolio_id,
                position_epoch,
                asset_index,
                reduce_q,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(actor.portfolio, false),
            ],
            &[owner],
        )
    }

    pub fn forfeit_recovery_leg(
        &mut self,
        actor_index: usize,
        asset_index: u16,
        b_delta_budget: u128,
    ) -> Result<TxSuccess, String> {
        let portfolio_id = self.primary_portfolio_id(actor_index);
        let position_epoch = self.primary_portfolio_position_epoch(actor_index);
        let actor = &self.actors[actor_index];
        let owner = copy_keypair(&actor.signer);
        self.send_program(
            ProgInstruction::ForfeitRecoveryLeg {
                portfolio_id,
                position_epoch,
                asset_index,
                b_delta_budget,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(actor.portfolio, false),
            ],
            &[owner],
        )
    }

    pub fn finalize_reset_side(&mut self, asset_index: u16, side: u8) -> Result<TxSuccess, String> {
        self.send_program(
            ProgInstruction::FinalizeResetSide { asset_index, side },
            vec![AccountMeta::new(self.market, false)],
            &[],
        )
    }

    pub fn trade_no_cpi(
        &mut self,
        taker: usize,
        maker: usize,
        asset_index: u16,
        size_q: i128,
        exec_price: u64,
        fee_bps: u64,
    ) -> Result<TxSuccess, String> {
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let account_a_portfolio_id = self.primary_portfolio_id(taker);
        let account_a_position_epoch = self.primary_portfolio_position_epoch(taker);
        let account_b_portfolio_id = self.primary_portfolio_id(maker);
        let account_b_position_epoch = self.primary_portfolio_position_epoch(maker);
        let taker_owner = copy_keypair(&self.actors[taker].signer);
        let maker_owner = copy_keypair(&self.actors[maker].signer);
        self.send_program(
            ProgInstruction::TradeNoCpi {
                account_a_portfolio_id,
                account_a_position_epoch,
                account_b_portfolio_id,
                account_b_position_epoch,
                asset_index,
                market_id,
                size_q,
                exec_price,
                fee_bps,
            },
            vec![
                AccountMeta::new(taker_owner.pubkey(), true),
                AccountMeta::new(maker_owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.actors[taker].portfolio, false),
                AccountMeta::new(self.actors[maker].portfolio, false),
            ],
            &[taker_owner, maker_owner],
        )
    }

    pub fn batch_trade_no_cpi(
        &mut self,
        taker: usize,
        maker: usize,
        legs: Vec<BatchTradeLeg>,
    ) -> Result<TxSuccess, String> {
        let account_a_portfolio_id = self.primary_portfolio_id(taker);
        let account_a_position_epoch = self.primary_portfolio_position_epoch(taker);
        let account_b_portfolio_id = self.primary_portfolio_id(maker);
        let account_b_position_epoch = self.primary_portfolio_position_epoch(maker);
        let taker_owner = copy_keypair(&self.actors[taker].signer);
        let maker_owner = copy_keypair(&self.actors[maker].signer);
        self.send_program(
            ProgInstruction::BatchTradeNoCpi {
                account_a_portfolio_id,
                account_a_position_epoch,
                account_b_portfolio_id,
                account_b_position_epoch,
                legs,
            },
            vec![
                AccountMeta::new(taker_owner.pubkey(), true),
                AccountMeta::new(maker_owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.actors[taker].portfolio, false),
                AccountMeta::new(self.actors[maker].portfolio, false),
            ],
            &[taker_owner, maker_owner],
        )
    }

    pub fn trade_cpi(
        &mut self,
        taker: usize,
        maker: usize,
        asset_index: u16,
        size_q: i128,
        fee_bps: u64,
        limit_price: u64,
    ) -> Result<TxSuccess, String> {
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let account_a_portfolio_id = self.primary_portfolio_id(taker);
        let account_a_position_epoch = self.primary_portfolio_position_epoch(taker);
        let account_b_portfolio_id = self.primary_portfolio_id(maker);
        let account_b_position_epoch = self.primary_portfolio_position_epoch(maker);
        let taker_owner = copy_keypair(&self.actors[taker].signer);
        let binding = &self.actors[maker];
        self.send_program(
            ProgInstruction::TradeCpi {
                account_a_portfolio_id,
                account_a_position_epoch,
                account_b_portfolio_id,
                account_b_position_epoch,
                asset_index,
                market_id,
                size_q,
                fee_bps,
                limit_price,
            },
            vec![
                AccountMeta::new(taker_owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.actors[taker].portfolio, false),
                AccountMeta::new(binding.portfolio, false),
                AccountMeta::new_readonly(self.matcher_program, false),
                AccountMeta::new(binding.matcher_context, false),
                AccountMeta::new_readonly(binding.matcher_delegate, false),
            ],
            &[taker_owner],
        )
    }

    pub fn batch_trade_cpi(
        &mut self,
        taker: usize,
        maker: usize,
        legs: Vec<BatchTradeCpiLeg>,
    ) -> Result<TxSuccess, String> {
        let account_a_portfolio_id = self.primary_portfolio_id(taker);
        let account_a_position_epoch = self.primary_portfolio_position_epoch(taker);
        let account_b_portfolio_id = self.primary_portfolio_id(maker);
        let account_b_position_epoch = self.primary_portfolio_position_epoch(maker);
        let taker_owner = copy_keypair(&self.actors[taker].signer);
        let binding = &self.actors[maker];
        self.send_program(
            ProgInstruction::BatchTradeCpi {
                account_a_portfolio_id,
                account_a_position_epoch,
                account_b_portfolio_id,
                account_b_position_epoch,
                legs,
            },
            vec![
                AccountMeta::new(taker_owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.actors[taker].portfolio, false),
                AccountMeta::new(binding.portfolio, false),
                AccountMeta::new_readonly(self.matcher_program, false),
                AccountMeta::new(binding.matcher_context, false),
                AccountMeta::new_readonly(binding.matcher_delegate, false),
            ],
            &[taker_owner],
        )
    }

    pub fn configure_auth_mark(
        &mut self,
        foreign: bool,
        asset_index: u16,
        now_slot: u64,
        mark: u64,
    ) -> Result<TxSuccess, String> {
        let observation_sequence = if foreign {
            next_control_sequence(
                self.foreign_control_sequences(asset_index as usize)
                    .oracle_observation,
            )
        } else {
            next_control_sequence(
                self.primary_control_sequences(asset_index as usize)
                    .oracle_observation,
            )
        };
        let (authority, market) = if foreign {
            (copy_keypair(&self.foreign_admin), self.foreign_market)
        } else {
            (copy_keypair(&self.admin), self.market)
        };
        let market_id = if foreign {
            self.foreign_market_state().1.assets[asset_index as usize].market_id
        } else {
            self.primary_market_state().1.assets[asset_index as usize].market_id
        };
        self.send_program(
            ProgInstruction::ConfigureAuthMark {
                asset_index,
                market_id,
                now_slot,
                initial_mark_e6: mark,
                observation_sequence,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(market, false),
            ],
            &[authority],
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn configure_hybrid_oracle(
        &mut self,
        asset_index: u16,
        now_slot: u64,
        now_unix_ts: i64,
        oracle_leg_flags: u8,
        feeds: [[u8; 32]; 3],
        oracle_accounts: &[Pubkey],
        hybrid_soft_stale_slots: u64,
        conf_filter_bps: u16,
    ) -> Result<TxSuccess, String> {
        let observation_sequence = next_control_sequence(
            self.primary_control_sequences(asset_index as usize)
                .oracle_observation,
        );
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let authority = copy_keypair(&self.admin);
        let mut accounts = vec![
            AccountMeta::new(authority.pubkey(), true),
            AccountMeta::new(self.market, false),
        ];
        accounts.extend(
            oracle_accounts
                .iter()
                .copied()
                .map(|key| AccountMeta::new_readonly(key, false)),
        );
        self.send_program(
            ProgInstruction::ConfigureHybridOracle {
                asset_index,
                market_id,
                now_slot,
                now_unix_ts,
                oracle_leg_count: oracle_accounts.len() as u8,
                oracle_leg_flags,
                max_staleness_secs: 60,
                hybrid_soft_stale_slots,
                mark_ewma_halflife_slots: 1,
                mark_min_fee: 0,
                invert: 0,
                unit_scale: 0,
                conf_filter_bps,
                oracle_leg_feeds: feeds,
                observation_sequence,
            },
            accounts,
            &[authority],
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn build_retained_hybrid_oracle_config(
        &mut self,
        asset_index: u16,
        now_slot: u64,
        now_unix_ts: i64,
        oracle_leg_flags: u8,
        feeds: [[u8; 32]; 3],
        oracle_accounts: &[Pubkey],
        hybrid_soft_stale_slots: u64,
        conf_filter_bps: u16,
    ) -> Transaction {
        let observation_sequence = next_control_sequence(
            self.primary_control_sequences(asset_index as usize)
                .oracle_observation,
        );
        self.build_retained_hybrid_oracle_config_with_sequence(
            asset_index,
            now_slot,
            now_unix_ts,
            oracle_leg_flags,
            feeds,
            oracle_accounts,
            hybrid_soft_stale_slots,
            conf_filter_bps,
            observation_sequence,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn build_retained_hybrid_oracle_config_with_sequence(
        &mut self,
        asset_index: u16,
        now_slot: u64,
        now_unix_ts: i64,
        oracle_leg_flags: u8,
        feeds: [[u8; 32]; 3],
        oracle_accounts: &[Pubkey],
        hybrid_soft_stale_slots: u64,
        conf_filter_bps: u16,
        observation_sequence: u64,
    ) -> Transaction {
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let authority = copy_keypair(&self.admin);
        let mut accounts = vec![
            AccountMeta::new(authority.pubkey(), true),
            AccountMeta::new(self.market, false),
        ];
        accounts.extend(
            oracle_accounts
                .iter()
                .copied()
                .map(|key| AccountMeta::new_readonly(key, false)),
        );
        self.build_program_transaction(
            ProgInstruction::ConfigureHybridOracle {
                asset_index,
                market_id,
                now_slot,
                now_unix_ts,
                oracle_leg_count: oracle_accounts.len() as u8,
                oracle_leg_flags,
                max_staleness_secs: 60,
                hybrid_soft_stale_slots,
                mark_ewma_halflife_slots: 1,
                mark_min_fee: 0,
                invert: 0,
                unit_scale: 0,
                conf_filter_bps,
                oracle_leg_feeds: feeds,
                observation_sequence,
            },
            accounts,
            &[authority],
        )
    }

    pub fn configure_auth_mark_for_actor(
        &mut self,
        actor_index: usize,
        asset_index: u16,
        now_slot: u64,
        mark: u64,
    ) -> Result<TxSuccess, String> {
        let observation_sequence = next_control_sequence(
            self.primary_control_sequences(asset_index as usize)
                .oracle_observation,
        );
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let authority = copy_keypair(&self.actors[actor_index].signer);
        self.send_program(
            ProgInstruction::ConfigureAuthMark {
                asset_index,
                market_id,
                now_slot,
                initial_mark_e6: mark,
                observation_sequence,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn configure_ewma_mark(
        &mut self,
        asset_index: u16,
        now_slot: u64,
        mark: u64,
        halflife_slots: u64,
        mark_min_fee: u64,
    ) -> Result<TxSuccess, String> {
        let observation_sequence = next_control_sequence(
            self.primary_control_sequences(asset_index as usize)
                .oracle_observation,
        );
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let authority = copy_keypair(&self.admin);
        self.send_program(
            ProgInstruction::ConfigureEwmaMark {
                asset_index,
                market_id,
                now_slot,
                initial_mark_e6: mark,
                mark_ewma_halflife_slots: halflife_slots,
                mark_min_fee,
                observation_sequence,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn configure_ewma_mark_for_actor(
        &mut self,
        actor_index: usize,
        asset_index: u16,
        now_slot: u64,
        mark: u64,
        halflife_slots: u64,
        mark_min_fee: u64,
    ) -> Result<TxSuccess, String> {
        let observation_sequence = next_control_sequence(
            self.primary_control_sequences(asset_index as usize)
                .oracle_observation,
        );
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let authority = copy_keypair(&self.actors[actor_index].signer);
        self.send_program(
            ProgInstruction::ConfigureEwmaMark {
                asset_index,
                market_id,
                now_slot,
                initial_mark_e6: mark,
                mark_ewma_halflife_slots: halflife_slots,
                mark_min_fee,
                observation_sequence,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn push_ewma_mark(
        &mut self,
        asset_index: u16,
        now_slot: u64,
        mark: u64,
    ) -> Result<TxSuccess, String> {
        let observation_sequence = next_control_sequence(
            self.primary_control_sequences(asset_index as usize)
                .oracle_observation,
        );
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let authority = copy_keypair(&self.admin);
        self.send_program(
            ProgInstruction::PushEwmaMark {
                asset_index,
                market_id,
                now_slot,
                mark_e6: mark,
                observation_sequence,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn push_auth_mark(
        &mut self,
        asset_index: u16,
        now_slot: u64,
        mark: u64,
    ) -> Result<TxSuccess, String> {
        let observation_sequence = next_control_sequence(
            self.primary_control_sequences(asset_index as usize)
                .oracle_observation,
        );
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let authority = copy_keypair(&self.admin);
        self.send_program(
            ProgInstruction::PushAuthMark {
                asset_index,
                market_id,
                now_slot,
                mark_e6: mark,
                observation_sequence,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn push_auth_mark_for_actor(
        &mut self,
        actor_index: usize,
        asset_index: u16,
        now_slot: u64,
        mark: u64,
    ) -> Result<TxSuccess, String> {
        let observation_sequence = next_control_sequence(
            self.primary_control_sequences(asset_index as usize)
                .oracle_observation,
        );
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let authority = copy_keypair(&self.actors[actor_index].signer);
        self.send_program(
            ProgInstruction::PushAuthMark {
                asset_index,
                market_id,
                now_slot,
                mark_e6: mark,
                observation_sequence,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn update_backing_fee_policy(
        &mut self,
        domain: u16,
        fee_bps: u16,
        insurance_share_bps: u16,
    ) -> Result<TxSuccess, String> {
        let sequences = self.primary_control_sequences(domain as usize / 2);
        let policy_sequence = next_control_sequence(if domain % 2 == 0 {
            sequences.backing_fee_long
        } else {
            sequences.backing_fee_short
        });
        let market_id = self.primary_market_state().1.assets[domain as usize / 2].market_id;
        let authority = copy_keypair(&self.admin);
        self.send_program(
            ProgInstruction::UpdateBackingFeePolicy {
                domain,
                market_id,
                fee_bps,
                insurance_share_bps,
                policy_sequence,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn update_backing_fee_policy_for_actor(
        &mut self,
        actor_index: usize,
        domain: u16,
        fee_bps: u16,
        insurance_share_bps: u16,
    ) -> Result<TxSuccess, String> {
        let sequences = self.primary_control_sequences(domain as usize / 2);
        let policy_sequence = next_control_sequence(if domain % 2 == 0 {
            sequences.backing_fee_long
        } else {
            sequences.backing_fee_short
        });
        let market_id = self.primary_market_state().1.assets[domain as usize / 2].market_id;
        let authority = copy_keypair(&self.actors[actor_index].signer);
        self.send_program(
            ProgInstruction::UpdateBackingFeePolicy {
                domain,
                market_id,
                fee_bps,
                insurance_share_bps,
                policy_sequence,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn build_retained_backing_fee_policy_for_actor(
        &mut self,
        actor_index: usize,
        domain: u16,
        fee_bps: u16,
        insurance_share_bps: u16,
    ) -> Transaction {
        let sequences = self.primary_control_sequences(domain as usize / 2);
        let policy_sequence = next_control_sequence(if domain % 2 == 0 {
            sequences.backing_fee_long
        } else {
            sequences.backing_fee_short
        });
        let market_id = self.primary_market_state().1.assets[domain as usize / 2].market_id;
        let authority = copy_keypair(&self.actors[actor_index].signer);
        self.build_program_transaction(
            ProgInstruction::UpdateBackingFeePolicy {
                domain,
                market_id,
                fee_bps,
                insurance_share_bps,
                policy_sequence,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn update_market_init_fee_policy(
        &mut self,
        min_init_fee: u128,
    ) -> Result<TxSuccess, String> {
        let policy_sequence =
            next_control_sequence(self.primary_control_sequences(0).market_init_fee);
        let authority = copy_keypair(&self.admin);
        self.send_program(
            ProgInstruction::UpdateMarketInitFeePolicy {
                min_init_fee,
                policy_sequence,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn update_trade_fee_policy(
        &mut self,
        trade_fee_base_bps: u64,
    ) -> Result<TxSuccess, String> {
        let policy_sequence = next_control_sequence(self.primary_control_sequences(0).trade_fee);
        let authority = copy_keypair(&self.admin);
        self.send_program(
            ProgInstruction::UpdateTradeFeePolicy {
                trade_fee_base_bps,
                policy_sequence,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn build_retained_trade_fee_policy(&mut self, trade_fee_base_bps: u64) -> Transaction {
        let policy_sequence = next_control_sequence(self.primary_control_sequences(0).trade_fee);
        let authority = copy_keypair(&self.admin);
        self.build_program_transaction(
            ProgInstruction::UpdateTradeFeePolicy {
                trade_fee_base_bps,
                policy_sequence,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn update_fee_redirect_policy(&mut self, redirect_bps: u16) -> Result<TxSuccess, String> {
        let policy_sequence = next_control_sequence(self.primary_control_sequences(0).fee_redirect);
        let authority = copy_keypair(&self.admin);
        self.send_program(
            ProgInstruction::UpdateFeeRedirectPolicy {
                redirect_bps,
                policy_sequence,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn build_retained_fee_redirect_policy(&mut self, redirect_bps: u16) -> Transaction {
        let policy_sequence = next_control_sequence(self.primary_control_sequences(0).fee_redirect);
        let authority = copy_keypair(&self.admin);
        self.build_program_transaction(
            ProgInstruction::UpdateFeeRedirectPolicy {
                redirect_bps,
                policy_sequence,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn build_retained_market_init_fee_policy(&mut self, min_init_fee: u128) -> Transaction {
        let policy_sequence =
            next_control_sequence(self.primary_control_sequences(0).market_init_fee);
        let authority = copy_keypair(&self.admin);
        self.build_program_transaction(
            ProgInstruction::UpdateMarketInitFeePolicy {
                min_init_fee,
                policy_sequence,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn update_liquidation_fee_policy(
        &mut self,
        cranker_share_bps: u16,
    ) -> Result<TxSuccess, String> {
        let policy_sequence =
            next_control_sequence(self.primary_control_sequences(0).liquidation_fee);
        let authority = copy_keypair(&self.admin);
        self.send_program(
            ProgInstruction::UpdateLiquidationFeePolicy {
                cranker_share_bps,
                policy_sequence,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn build_retained_liquidation_fee_policy(&mut self, cranker_share_bps: u16) -> Transaction {
        let policy_sequence =
            next_control_sequence(self.primary_control_sequences(0).liquidation_fee);
        let authority = copy_keypair(&self.admin);
        self.build_program_transaction(
            ProgInstruction::UpdateLiquidationFeePolicy {
                cranker_share_bps,
                policy_sequence,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn update_maintenance_fee_policy(
        &mut self,
        cranker_share_bps: u16,
    ) -> Result<TxSuccess, String> {
        let policy_sequence =
            next_control_sequence(self.primary_control_sequences(0).maintenance_fee);
        let authority = copy_keypair(&self.admin);
        self.send_program(
            ProgInstruction::UpdateMaintenanceFeePolicy {
                cranker_share_bps,
                policy_sequence,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn build_retained_maintenance_fee_policy(&mut self, cranker_share_bps: u16) -> Transaction {
        let policy_sequence =
            next_control_sequence(self.primary_control_sequences(0).maintenance_fee);
        let authority = copy_keypair(&self.admin);
        self.build_program_transaction(
            ProgInstruction::UpdateMaintenanceFeePolicy {
                cranker_share_bps,
                policy_sequence,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn retire_asset(&mut self, asset_index: u16, now_slot: u64) -> Result<TxSuccess, String> {
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let authority = copy_keypair(&self.admin);
        self.send_program(
            ProgInstruction::UpdateAssetLifecycle {
                action: percolator_prog::processor::ASSET_ACTION_RETIRE,
                asset_index,
                market_id,
                now_slot,
                initial_price: 0,
                max_init_fee: u128::MAX,
                insurance_authority: authority.pubkey().to_bytes(),
                insurance_operator: authority.pubkey().to_bytes(),
                backing_bucket_authority: authority.pubkey().to_bytes(),
                oracle_authority: authority.pubkey().to_bytes(),
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn drain_only_asset(
        &mut self,
        asset_index: u16,
        now_slot: u64,
    ) -> Result<TxSuccess, String> {
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let authority = copy_keypair(&self.admin);
        self.send_program(
            ProgInstruction::UpdateAssetLifecycle {
                action: percolator_prog::processor::ASSET_ACTION_DRAIN_ONLY,
                asset_index,
                market_id,
                now_slot,
                initial_price: 0,
                max_init_fee: u128::MAX,
                insurance_authority: authority.pubkey().to_bytes(),
                insurance_operator: authority.pubkey().to_bytes(),
                backing_bucket_authority: authority.pubkey().to_bytes(),
                oracle_authority: authority.pubkey().to_bytes(),
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn shutdown_asset(&mut self, asset_index: u16, now_slot: u64) -> Result<TxSuccess, String> {
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let authority = copy_keypair(&self.admin);
        self.send_program(
            ProgInstruction::UpdateAssetLifecycle {
                action: percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
                asset_index,
                market_id,
                now_slot,
                initial_price: 0,
                max_init_fee: u128::MAX,
                insurance_authority: authority.pubkey().to_bytes(),
                insurance_operator: authority.pubkey().to_bytes(),
                backing_bucket_authority: authority.pubkey().to_bytes(),
                oracle_authority: authority.pubkey().to_bytes(),
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn restart_asset_oracle(
        &mut self,
        asset_index: u16,
        now_slot: u64,
        initial_price: u64,
    ) -> Result<TxSuccess, String> {
        let observation_sequence = next_control_sequence(
            self.primary_control_sequences(asset_index as usize)
                .oracle_observation,
        );
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let authority = copy_keypair(&self.admin);
        self.send_program(
            ProgInstruction::RestartAssetOracle {
                asset_index,
                market_id,
                now_slot,
                initial_price,
                observation_sequence,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn restart_asset_oracle_for_actor(
        &mut self,
        actor_index: usize,
        asset_index: u16,
        now_slot: u64,
        initial_price: u64,
    ) -> Result<TxSuccess, String> {
        let observation_sequence = next_control_sequence(
            self.primary_control_sequences(asset_index as usize)
                .oracle_observation,
        );
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let authority = copy_keypair(&self.actors[actor_index].signer);
        self.send_program(
            ProgInstruction::RestartAssetOracle {
                asset_index,
                market_id,
                now_slot,
                initial_price,
                observation_sequence,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn build_retained_restart_asset_oracle_for_actor_with_sequence(
        &mut self,
        actor_index: usize,
        asset_index: u16,
        now_slot: u64,
        initial_price: u64,
        observation_sequence: u64,
    ) -> Transaction {
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let authority = copy_keypair(&self.actors[actor_index].signer);
        self.build_program_transaction(
            ProgInstruction::RestartAssetOracle {
                asset_index,
                market_id,
                now_slot,
                initial_price,
                observation_sequence,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn force_close_abandoned_asset(
        &mut self,
        cranker_index: usize,
        account_a_index: usize,
        account_b_index: usize,
        asset_index: u16,
        now_slot: u64,
        close_q: u128,
    ) -> Result<TxSuccess, String> {
        let cranker = copy_keypair(&self.actors[cranker_index].signer);
        self.send_program(
            ProgInstruction::ForceCloseAbandonedAsset {
                asset_index,
                now_slot,
                close_q,
            },
            vec![
                AccountMeta::new(cranker.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.actors[account_a_index].portfolio, false),
                AccountMeta::new(self.actors[account_b_index].portfolio, false),
            ],
            &[cranker],
        )
    }

    pub fn activate_permissionless_asset(
        &mut self,
        creator_index: usize,
        asset_index: u16,
        now_slot: u64,
        initial_price: u64,
        fee: u128,
    ) -> Result<TxSuccess, String> {
        let authority = self.admin.pubkey();
        self.activate_permissionless_asset_with_authority(
            creator_index,
            asset_index,
            now_slot,
            initial_price,
            authority,
            fee,
        )
    }

    pub fn activate_permissionless_asset_for_actor(
        &mut self,
        creator_index: usize,
        asset_index: u16,
        now_slot: u64,
        initial_price: u64,
        authority_index: usize,
        fee: u128,
    ) -> Result<TxSuccess, String> {
        let authority = self.actors[authority_index].signer.pubkey();
        self.activate_permissionless_asset_with_authority(
            creator_index,
            asset_index,
            now_slot,
            initial_price,
            authority,
            fee,
        )
    }

    pub fn activate_permissionless_asset_with_actor_authorities(
        &mut self,
        creator_index: usize,
        asset_index: u16,
        now_slot: u64,
        initial_price: u64,
        insurance_authority_index: usize,
        insurance_operator_index: usize,
        backing_bucket_authority_index: usize,
        oracle_authority_index: usize,
        fee: u128,
    ) -> Result<TxSuccess, String> {
        if fee == 0 {
            return Err("permissionless activation adapter requires a nonzero fee".into());
        }
        let market_id = self.primary_market_state().1.next_market_id;
        let creator = copy_keypair(&self.actors[creator_index].signer);
        self.send_program(
            ProgInstruction::UpdateAssetLifecycle {
                action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
                asset_index,
                market_id,
                now_slot,
                initial_price,
                max_init_fee: fee,
                insurance_authority: self.actors[insurance_authority_index]
                    .signer
                    .pubkey()
                    .to_bytes(),
                insurance_operator: self.actors[insurance_operator_index]
                    .signer
                    .pubkey()
                    .to_bytes(),
                backing_bucket_authority: self.actors[backing_bucket_authority_index]
                    .signer
                    .pubkey()
                    .to_bytes(),
                oracle_authority: self.actors[oracle_authority_index]
                    .signer
                    .pubkey()
                    .to_bytes(),
            },
            vec![
                AccountMeta::new(creator.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.actors[creator_index].source_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[creator],
        )
    }

    fn activate_permissionless_asset_with_authority(
        &mut self,
        creator_index: usize,
        asset_index: u16,
        now_slot: u64,
        initial_price: u64,
        authority: Pubkey,
        fee: u128,
    ) -> Result<TxSuccess, String> {
        if fee == 0 {
            return Err("permissionless activation adapter requires a nonzero fee".into());
        }
        let market_id = self.primary_market_state().1.next_market_id;
        let creator = copy_keypair(&self.actors[creator_index].signer);
        self.send_program(
            ProgInstruction::UpdateAssetLifecycle {
                action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
                asset_index,
                market_id,
                now_slot,
                initial_price,
                max_init_fee: fee,
                insurance_authority: authority.to_bytes(),
                insurance_operator: authority.to_bytes(),
                backing_bucket_authority: authority.to_bytes(),
                oracle_authority: authority.to_bytes(),
            },
            vec![
                AccountMeta::new(creator.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.actors[creator_index].source_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[creator],
        )
    }

    pub fn withdraw_insurance_asset(
        &mut self,
        actor_index: usize,
        asset_index: u16,
        amount: u128,
    ) -> Result<TxSuccess, String> {
        let authority = copy_keypair(&self.actors[actor_index].signer);
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        self.send_program(
            ProgInstruction::WithdrawInsuranceAsset {
                asset_index,
                market_id,
                amount,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.actors[actor_index].destination_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[authority],
        )
    }

    pub fn withdraw_insurance_asset_as_admin(
        &mut self,
        asset_index: u16,
        amount: u128,
    ) -> Result<TxSuccess, String> {
        let authority = copy_keypair(&self.admin);
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        self.send_program(
            ProgInstruction::WithdrawInsuranceAsset {
                asset_index,
                market_id,
                amount,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.provider_destination_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[authority],
        )
    }

    pub fn withdraw_terminal_insurance_for_actor(
        &mut self,
        actor_index: usize,
        amount: u128,
    ) -> Result<TxSuccess, String> {
        let authority = copy_keypair(&self.actors[actor_index].signer);
        self.send_program(
            ProgInstruction::WithdrawInsurance { amount },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.actors[actor_index].destination_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[authority],
        )
    }

    pub fn update_asset_authority_from_admin(
        &mut self,
        asset_index: u16,
        kind: u8,
        new_actor_index: usize,
    ) -> Result<TxSuccess, String> {
        let current = copy_keypair(&self.admin);
        let incoming = copy_keypair(&self.actors[new_actor_index].signer);
        self.send_asset_authority_handoff(asset_index, kind, current, incoming)
    }

    pub fn update_asset_authority_between_actors(
        &mut self,
        asset_index: u16,
        kind: u8,
        current_actor_index: usize,
        incoming_actor_index: usize,
    ) -> Result<TxSuccess, String> {
        let current = copy_keypair(&self.actors[current_actor_index].signer);
        let incoming = copy_keypair(&self.actors[incoming_actor_index].signer);
        self.send_asset_authority_handoff(asset_index, kind, current, incoming)
    }

    pub fn burn_asset_admin(&mut self, asset_index: u16) -> Result<TxSuccess, String> {
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let current = copy_keypair(&self.admin);
        self.send_program(
            ProgInstruction::UpdateAssetAuthority {
                asset_index,
                market_id,
                kind: percolator_prog::processor::ASSET_AUTH_ADMIN,
                new_pubkey: [0; 32],
            },
            vec![
                AccountMeta::new(current.pubkey(), true),
                AccountMeta::new_readonly(Pubkey::default(), false),
                AccountMeta::new(self.market, false),
            ],
            &[current],
        )
    }

    fn send_asset_authority_handoff(
        &mut self,
        asset_index: u16,
        kind: u8,
        current: Keypair,
        incoming: Keypair,
    ) -> Result<TxSuccess, String> {
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        self.send_program(
            ProgInstruction::UpdateAssetAuthority {
                asset_index,
                market_id,
                kind,
                new_pubkey: incoming.pubkey().to_bytes(),
            },
            vec![
                AccountMeta::new(current.pubkey(), true),
                AccountMeta::new(incoming.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[current, incoming],
        )
    }

    pub fn top_up_backing_bucket(
        &mut self,
        domain: u16,
        amount: u128,
        expiry_slot: u64,
    ) -> Result<TxSuccess, String> {
        let authority = copy_keypair(&self.admin);
        let asset_index = domain as usize / 2;
        let market_id = self.primary_market_state().1.assets[asset_index].market_id;
        let intent_id =
            next_control_sequence(self.primary_control_sequences(asset_index).backing_top_up);
        self.send_program(
            ProgInstruction::TopUpBackingBucket {
                intent_id,
                domain,
                market_id,
                amount,
                expiry_slot,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.provider_source_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(self.backing_domain_ledger, false),
            ],
            &[authority],
        )
    }

    pub fn top_up_backing_bucket_without_ledger(
        &mut self,
        domain: u16,
        amount: u128,
        expiry_slot: u64,
    ) -> Result<TxSuccess, String> {
        let authority = copy_keypair(&self.admin);
        let asset_index = domain as usize / 2;
        let market_id = self.primary_market_state().1.assets[asset_index].market_id;
        let intent_id =
            next_control_sequence(self.primary_control_sequences(asset_index).backing_top_up);
        self.send_program(
            ProgInstruction::TopUpBackingBucket {
                intent_id,
                domain,
                market_id,
                amount,
                expiry_slot,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.provider_source_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[authority],
        )
    }

    pub fn top_up_insurance_domain_for_actor(
        &mut self,
        actor_index: usize,
        domain: u16,
        amount: u128,
    ) -> Result<TxSuccess, String> {
        let authority = copy_keypair(&self.actors[actor_index].signer);
        let asset_index = domain as usize / 2;
        let market_id = self.primary_market_state().1.assets[asset_index].market_id;
        let intent_id =
            next_control_sequence(self.primary_control_sequences(asset_index).insurance_top_up);
        self.send_program(
            ProgInstruction::TopUpInsuranceDomain {
                intent_id,
                domain,
                market_id,
                amount,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.actors[actor_index].source_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[authority],
        )
    }

    pub fn top_up_insurance_domain(
        &mut self,
        domain: u16,
        amount: u128,
    ) -> Result<TxSuccess, String> {
        let authority = copy_keypair(&self.admin);
        let asset_index = domain as usize / 2;
        let market_id = self.primary_market_state().1.assets[asset_index].market_id;
        let intent_id =
            next_control_sequence(self.primary_control_sequences(asset_index).insurance_top_up);
        self.send_program(
            ProgInstruction::TopUpInsuranceDomain {
                intent_id,
                domain,
                market_id,
                amount,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.provider_source_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[authority],
        )
    }

    pub fn top_up_backing_bucket_for_actor(
        &mut self,
        actor_index: usize,
        domain: u16,
        amount: u128,
        expiry_slot: u64,
    ) -> Result<TxSuccess, String> {
        let authority = copy_keypair(&self.actors[actor_index].signer);
        let asset_index = domain as usize / 2;
        let market_id = self.primary_market_state().1.assets[asset_index].market_id;
        let intent_id =
            next_control_sequence(self.primary_control_sequences(asset_index).backing_top_up);
        self.send_program(
            ProgInstruction::TopUpBackingBucket {
                intent_id,
                domain,
                market_id,
                amount,
                expiry_slot,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.actors[actor_index].source_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(self.backing_domain_ledger, false),
            ],
            &[authority],
        )
    }

    pub fn withdraw_backing_bucket_for_actor(
        &mut self,
        actor_index: usize,
        domain: u16,
        amount: u128,
    ) -> Result<TxSuccess, String> {
        let authority = copy_keypair(&self.actors[actor_index].signer);
        let market_id = self.primary_market_state().1.assets[domain as usize / 2].market_id;
        self.send_program(
            ProgInstruction::WithdrawBackingBucket {
                domain,
                market_id,
                amount,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.actors[actor_index].destination_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[authority],
        )
    }

    pub fn build_retained_backing_bucket_withdrawal_for_actor(
        &mut self,
        actor_index: usize,
        domain: u16,
        amount: u128,
    ) -> Transaction {
        let authority = copy_keypair(&self.actors[actor_index].signer);
        let market_id = self.primary_market_state().1.assets[domain as usize / 2].market_id;
        self.build_program_transaction(
            ProgInstruction::WithdrawBackingBucket {
                domain,
                market_id,
                amount,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.actors[actor_index].destination_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[authority],
        )
    }

    pub fn withdraw_backing_bucket(
        &mut self,
        domain: u16,
        amount: u128,
    ) -> Result<TxSuccess, String> {
        let authority = copy_keypair(&self.admin);
        let market_id = self.primary_market_state().1.assets[domain as usize / 2].market_id;
        self.send_program(
            ProgInstruction::WithdrawBackingBucket {
                domain,
                market_id,
                amount,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.provider_destination_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[authority],
        )
    }

    pub fn withdraw_backing_bucket_earnings(
        &mut self,
        domain: u16,
        amount: u128,
    ) -> Result<TxSuccess, String> {
        let authority = copy_keypair(&self.admin);
        let market_id = self.primary_market_state().1.assets[domain as usize / 2].market_id;
        self.send_program(
            ProgInstruction::WithdrawBackingBucketEarnings {
                domain,
                market_id,
                amount,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.backing_domain_ledger, false),
                AccountMeta::new(self.provider_destination_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[authority],
        )
    }

    pub fn withdraw_backing_bucket_earnings_for_actor(
        &mut self,
        actor_index: usize,
        domain: u16,
        amount: u128,
    ) -> Result<TxSuccess, String> {
        let authority = copy_keypair(&self.actors[actor_index].signer);
        let market_id = self.primary_market_state().1.assets[domain as usize / 2].market_id;
        self.send_program(
            ProgInstruction::WithdrawBackingBucketEarnings {
                domain,
                market_id,
                amount,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.backing_domain_ledger, false),
                AccountMeta::new(self.actors[actor_index].destination_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[authority],
        )
    }

    pub fn crank(
        &mut self,
        actor_index: usize,
        now_slot: u64,
        observations: Vec<CrankObservationHint>,
    ) -> Result<TxSuccess, String> {
        self.send_program(
            ProgInstruction::PermissionlessCrank {
                now_slot,
                observations,
            },
            vec![
                AccountMeta::new(self.payer.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.actors[actor_index].portfolio, false),
            ],
            &[],
        )
    }

    pub fn crank_resolved_primary_signed(
        &mut self,
        actor_index: usize,
        now_slot: u64,
        observations: Vec<CrankObservationHint>,
    ) -> Result<TxSuccess, String> {
        let actor = &self.actors[actor_index];
        let owner = copy_keypair(&actor.signer);
        self.send_program(
            ProgInstruction::PermissionlessCrank {
                now_slot,
                observations,
            },
            vec![
                AccountMeta::new_readonly(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(actor.portfolio, false),
                AccountMeta::new(actor.destination_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[owner],
        )
    }

    pub fn crank_with_oracles(
        &mut self,
        actor_index: usize,
        now_slot: u64,
        observations: Vec<CrankObservationHint>,
        oracle_accounts: &[Pubkey],
    ) -> Result<TxSuccess, String> {
        let mut accounts = vec![
            AccountMeta::new(self.payer.pubkey(), true),
            AccountMeta::new(self.market, false),
            AccountMeta::new(self.actors[actor_index].portfolio, false),
        ];
        accounts.extend(
            oracle_accounts
                .iter()
                .copied()
                .map(|key| AccountMeta::new_readonly(key, false)),
        );
        self.send_program(
            ProgInstruction::PermissionlessCrank {
                now_slot,
                observations,
            },
            accounts,
            &[],
        )
    }

    pub fn crank_with_reward(
        &mut self,
        cranker_index: usize,
        actor_index: usize,
        now_slot: u64,
        observations: Vec<CrankObservationHint>,
        oracle_accounts: &[Pubkey],
    ) -> Result<TxSuccess, String> {
        let cranker = copy_keypair(&self.actors[cranker_index].signer);
        let mut accounts = vec![
            AccountMeta::new(cranker.pubkey(), true),
            AccountMeta::new(self.market, false),
            AccountMeta::new(self.actors[actor_index].portfolio, false),
        ];
        accounts.extend(
            oracle_accounts
                .iter()
                .copied()
                .map(|key| AccountMeta::new_readonly(key, false)),
        );
        accounts.push(AccountMeta::new(
            self.actors[cranker_index].portfolio,
            false,
        ));
        self.send_program(
            ProgInstruction::PermissionlessCrank {
                now_slot,
                observations,
            },
            accounts,
            &[cranker],
        )
    }

    pub fn sync_maintenance_fee(
        &mut self,
        actor_index: usize,
        now_slot: u64,
    ) -> Result<TxSuccess, String> {
        self.send_program(
            ProgInstruction::SyncMaintenanceFee { now_slot },
            vec![
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.actors[actor_index].portfolio, false),
            ],
            &[],
        )
    }

    pub fn sync_maintenance_fee_with_reward(
        &mut self,
        actor_index: usize,
        cranker_index: usize,
        now_slot: u64,
    ) -> Result<TxSuccess, String> {
        self.send_program(
            ProgInstruction::SyncMaintenanceFee { now_slot },
            vec![
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.actors[actor_index].portfolio, false),
                AccountMeta::new(self.actors[cranker_index].portfolio, false),
            ],
            &[],
        )
    }

    pub fn cross_market_trade_substitution(
        &mut self,
        actor_index: usize,
        size_q: i128,
    ) -> Result<TxSuccess, String> {
        let market_id = self.primary_market_state().1.assets[0].market_id;
        let account_a_portfolio_id = self.primary_portfolio_id(actor_index);
        let account_a_position_epoch = self.primary_portfolio_position_epoch(actor_index);
        let account_b_portfolio_id = self.foreign_portfolio_id();
        let account_b_position_epoch = self.foreign_portfolio_position_epoch();
        let primary_owner = copy_keypair(&self.actors[actor_index].signer);
        let foreign_owner = copy_keypair(&self.foreign_actor.signer);
        self.send_program(
            ProgInstruction::TradeNoCpi {
                account_a_portfolio_id,
                account_a_position_epoch,
                account_b_portfolio_id,
                account_b_position_epoch,
                asset_index: 0,
                market_id,
                size_q,
                exec_price: INITIAL_PRICE,
                fee_bps: 0,
            },
            vec![
                AccountMeta::new(primary_owner.pubkey(), true),
                AccountMeta::new(foreign_owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.actors[actor_index].portfolio, false),
                AccountMeta::new(self.foreign_actor.portfolio, false),
            ],
            &[primary_owner, foreign_owner],
        )
    }

    pub fn cross_market_deposit_vault_substitution(
        &mut self,
        actor_index: usize,
        amount: u128,
    ) -> Result<TxSuccess, String> {
        let actor = &self.actors[actor_index];
        let owner = copy_keypair(&actor.signer);
        let portfolio_id = self.primary_portfolio_id(actor_index);
        let expected_sequence = self.primary_portfolio_matcher_sequence(actor_index);
        self.send_program(
            ProgInstruction::Deposit {
                portfolio_id,
                expected_sequence,
                amount,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(actor.portfolio, false),
                AccountMeta::new(actor.source_token, false),
                AccountMeta::new(self.foreign_vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[owner],
        )
    }

    pub fn cross_market_withdraw_vault_substitution(
        &mut self,
        actor_index: usize,
        amount: u128,
    ) -> Result<TxSuccess, String> {
        let actor = &self.actors[actor_index];
        let owner = copy_keypair(&actor.signer);
        let portfolio_id = self.primary_portfolio_id(actor_index);
        let expected_sequence = self.primary_portfolio_matcher_sequence(actor_index);
        self.send_program(
            ProgInstruction::Withdraw {
                portfolio_id,
                expected_sequence,
                amount,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(actor.portfolio, false),
                AccountMeta::new(actor.destination_token, false),
                AccountMeta::new(self.foreign_vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[owner],
        )
    }

    pub fn cross_market_crank_portfolio_substitution(
        &mut self,
        now_slot: u64,
    ) -> Result<TxSuccess, String> {
        self.send_program(
            ProgInstruction::PermissionlessCrank {
                now_slot,
                observations: vec![],
            },
            vec![
                AccountMeta::new(self.payer.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.foreign_actor.portfolio, false),
            ],
            &[],
        )
    }

    pub fn cpi_matcher_binding_substitution(
        &mut self,
        taker: usize,
        maker: usize,
        substituted_binding: usize,
    ) -> Result<TxSuccess, String> {
        let market_id = self.primary_market_state().1.assets[0].market_id;
        let account_a_portfolio_id = self.primary_portfolio_id(taker);
        let account_a_position_epoch = self.primary_portfolio_position_epoch(taker);
        let account_b_portfolio_id = self.primary_portfolio_id(maker);
        let account_b_position_epoch = self.primary_portfolio_position_epoch(maker);
        let taker_owner = copy_keypair(&self.actors[taker].signer);
        let maker_portfolio = self.actors[maker].portfolio;
        let binding = &self.actors[substituted_binding];
        self.send_program(
            ProgInstruction::TradeCpi {
                account_a_portfolio_id,
                account_a_position_epoch,
                account_b_portfolio_id,
                account_b_position_epoch,
                asset_index: 0,
                market_id,
                size_q: POS_SCALE as i128 / 4,
                fee_bps: 0,
                limit_price: 0,
            },
            vec![
                AccountMeta::new(taker_owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.actors[taker].portfolio, false),
                AccountMeta::new(maker_portfolio, false),
                AccountMeta::new_readonly(self.matcher_program, false),
                AccountMeta::new(binding.matcher_context, false),
                AccountMeta::new_readonly(binding.matcher_delegate, false),
            ],
            &[taker_owner],
        )
    }

    pub fn build_retained_no_cpi_trade(
        &mut self,
        taker: usize,
        maker: usize,
        asset_index: u16,
        size_q: i128,
        exec_price: u64,
    ) -> Transaction {
        self.build_retained_no_cpi_trade_with_fee(taker, maker, asset_index, size_q, exec_price, 0)
    }

    pub fn build_retained_convert_released_pnl(
        &mut self,
        actor_index: usize,
        amount: u128,
    ) -> Transaction {
        let actor = &self.actors[actor_index];
        let owner = copy_keypair(&actor.signer);
        let portfolio_id = self.primary_portfolio_id(actor_index);
        let position_epoch = self.primary_portfolio_position_epoch(actor_index);
        self.build_program_transaction(
            ProgInstruction::ConvertReleasedPnl {
                portfolio_id,
                position_epoch,
                amount,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(actor.portfolio, false),
            ],
            &[owner],
        )
    }

    pub fn build_retained_rebalance_reduce(
        &mut self,
        actor_index: usize,
        asset_index: u16,
        reduce_q: u128,
    ) -> Transaction {
        let portfolio_id = self.primary_portfolio_id(actor_index);
        let position_epoch = self.primary_portfolio_position_epoch(actor_index);
        let actor = &self.actors[actor_index];
        let owner = copy_keypair(&actor.signer);
        self.build_program_transaction(
            ProgInstruction::RebalanceReduce {
                portfolio_id,
                position_epoch,
                asset_index,
                reduce_q,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(actor.portfolio, false),
            ],
            &[owner],
        )
    }

    pub fn build_retained_forfeit_recovery_leg(
        &mut self,
        actor_index: usize,
        asset_index: u16,
        b_delta_budget: u128,
    ) -> Transaction {
        let portfolio_id = self.primary_portfolio_id(actor_index);
        let position_epoch = self.primary_portfolio_position_epoch(actor_index);
        let actor = &self.actors[actor_index];
        let owner = copy_keypair(&actor.signer);
        self.build_program_transaction(
            ProgInstruction::ForfeitRecoveryLeg {
                portfolio_id,
                position_epoch,
                asset_index,
                b_delta_budget,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(actor.portfolio, false),
            ],
            &[owner],
        )
    }

    pub fn build_retained_no_cpi_trade_with_fee(
        &mut self,
        taker: usize,
        maker: usize,
        asset_index: u16,
        size_q: i128,
        exec_price: u64,
        fee_bps: u64,
    ) -> Transaction {
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let account_a_portfolio_id = self.primary_portfolio_id(taker);
        let account_a_position_epoch = self.primary_portfolio_position_epoch(taker);
        let account_b_portfolio_id = self.primary_portfolio_id(maker);
        let account_b_position_epoch = self.primary_portfolio_position_epoch(maker);
        let taker_owner = copy_keypair(&self.actors[taker].signer);
        let maker_owner = copy_keypair(&self.actors[maker].signer);
        self.build_program_transaction(
            ProgInstruction::TradeNoCpi {
                account_a_portfolio_id,
                account_a_position_epoch,
                account_b_portfolio_id,
                account_b_position_epoch,
                asset_index,
                market_id,
                size_q,
                exec_price,
                fee_bps,
            },
            vec![
                AccountMeta::new(taker_owner.pubkey(), true),
                AccountMeta::new(maker_owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.actors[taker].portfolio, false),
                AccountMeta::new(self.actors[maker].portfolio, false),
            ],
            &[taker_owner, maker_owner],
        )
    }

    pub fn build_retained_withdrawal(&mut self, actor_index: usize, amount: u128) -> Transaction {
        let actor = &self.actors[actor_index];
        let owner = copy_keypair(&actor.signer);
        let portfolio_id = self.primary_portfolio_id(actor_index);
        let expected_sequence = self.primary_portfolio_matcher_sequence(actor_index);
        self.build_program_transaction(
            ProgInstruction::Withdraw {
                portfolio_id,
                expected_sequence,
                amount,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(actor.portfolio, false),
                AccountMeta::new(actor.destination_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[owner],
        )
    }

    pub fn build_retained_resolve_market(&mut self) -> Transaction {
        let admin = copy_keypair(&self.admin);
        let asset_generation_frontier = self.primary_market_state().1.next_market_id;
        self.build_program_transaction(
            ProgInstruction::ResolveMarket {
                asset_generation_frontier,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[admin],
        )
    }

    pub fn build_retained_permissionless_resolve_policy(
        &mut self,
        stale_slots: u64,
        force_close_delay_slots: u64,
    ) -> Transaction {
        let policy_sequence =
            next_control_sequence(self.primary_control_sequences(0).permissionless_resolve);
        let asset_generation_frontier = self.primary_market_state().1.next_market_id;
        let admin = copy_keypair(&self.admin);
        self.build_program_transaction(
            ProgInstruction::ConfigurePermissionlessResolve {
                asset_generation_frontier,
                stale_slots,
                force_close_delay_slots,
                policy_sequence,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[admin],
        )
    }

    pub fn build_retained_shutdown_asset(
        &mut self,
        asset_index: u16,
        now_slot: u64,
    ) -> Transaction {
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let admin = copy_keypair(&self.admin);
        self.build_program_transaction(
            ProgInstruction::UpdateAssetLifecycle {
                action: percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
                asset_index,
                market_id,
                now_slot,
                initial_price: 0,
                max_init_fee: u128::MAX,
                insurance_authority: admin.pubkey().to_bytes(),
                insurance_operator: admin.pubkey().to_bytes(),
                backing_bucket_authority: admin.pubkey().to_bytes(),
                oracle_authority: admin.pubkey().to_bytes(),
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[admin],
        )
    }

    pub fn build_retained_drain_only_asset(&mut self, asset_index: u16) -> Transaction {
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let admin = copy_keypair(&self.admin);
        self.build_program_transaction(
            ProgInstruction::UpdateAssetLifecycle {
                action: percolator_prog::processor::ASSET_ACTION_DRAIN_ONLY,
                asset_index,
                market_id,
                now_slot: 0,
                initial_price: 0,
                max_init_fee: u128::MAX,
                insurance_authority: admin.pubkey().to_bytes(),
                insurance_operator: admin.pubkey().to_bytes(),
                backing_bucket_authority: admin.pubkey().to_bytes(),
                oracle_authority: admin.pubkey().to_bytes(),
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[admin],
        )
    }

    pub fn build_retained_retire_asset(&mut self, asset_index: u16, now_slot: u64) -> Transaction {
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let admin = copy_keypair(&self.admin);
        self.build_program_transaction(
            ProgInstruction::UpdateAssetLifecycle {
                action: percolator_prog::processor::ASSET_ACTION_RETIRE,
                asset_index,
                market_id,
                now_slot,
                initial_price: 0,
                max_init_fee: u128::MAX,
                insurance_authority: admin.pubkey().to_bytes(),
                insurance_operator: admin.pubkey().to_bytes(),
                backing_bucket_authority: admin.pubkey().to_bytes(),
                oracle_authority: admin.pubkey().to_bytes(),
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[admin],
        )
    }

    pub fn build_retained_deposit(&mut self, actor_index: usize, amount: u128) -> Transaction {
        let actor = &self.actors[actor_index];
        let owner = copy_keypair(&actor.signer);
        let portfolio_id = self.primary_portfolio_id(actor_index);
        let expected_sequence = self.primary_portfolio_matcher_sequence(actor_index);
        self.build_program_transaction(
            ProgInstruction::Deposit {
                portfolio_id,
                expected_sequence,
                amount,
            },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(actor.portfolio, false),
                AccountMeta::new(actor.source_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[owner],
        )
    }

    pub fn build_retained_cpi_trade(
        &mut self,
        taker: usize,
        maker: usize,
        asset_index: u16,
        size_q: i128,
        limit_price: u64,
    ) -> Transaction {
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let account_a_portfolio_id = self.primary_portfolio_id(taker);
        let account_a_position_epoch = self.primary_portfolio_position_epoch(taker);
        let account_b_portfolio_id = self.primary_portfolio_id(maker);
        let account_b_position_epoch = self.primary_portfolio_position_epoch(maker);
        let taker_owner = copy_keypair(&self.actors[taker].signer);
        let binding = &self.actors[maker];
        self.build_program_transaction(
            ProgInstruction::TradeCpi {
                account_a_portfolio_id,
                account_a_position_epoch,
                account_b_portfolio_id,
                account_b_position_epoch,
                asset_index,
                market_id,
                size_q,
                fee_bps: 0,
                limit_price,
            },
            vec![
                AccountMeta::new(taker_owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.actors[taker].portfolio, false),
                AccountMeta::new(binding.portfolio, false),
                AccountMeta::new_readonly(self.matcher_program, false),
                AccountMeta::new(binding.matcher_context, false),
                AccountMeta::new_readonly(binding.matcher_delegate, false),
            ],
            &[taker_owner],
        )
    }

    pub fn build_retained_batch_no_cpi_trade(
        &mut self,
        taker: usize,
        maker: usize,
        asset_index: u16,
        size_q: i128,
        exec_price: u64,
    ) -> Transaction {
        self.build_retained_batch_no_cpi_trade_with_fee(
            taker,
            maker,
            asset_index,
            size_q,
            exec_price,
            0,
        )
    }

    pub fn build_retained_batch_no_cpi_trade_with_fee(
        &mut self,
        taker: usize,
        maker: usize,
        asset_index: u16,
        size_q: i128,
        exec_price: u64,
        fee_bps: u64,
    ) -> Transaction {
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let account_a_portfolio_id = self.primary_portfolio_id(taker);
        let account_a_position_epoch = self.primary_portfolio_position_epoch(taker);
        let account_b_portfolio_id = self.primary_portfolio_id(maker);
        let account_b_position_epoch = self.primary_portfolio_position_epoch(maker);
        let taker_owner = copy_keypair(&self.actors[taker].signer);
        let maker_owner = copy_keypair(&self.actors[maker].signer);
        self.build_program_transaction(
            ProgInstruction::BatchTradeNoCpi {
                account_a_portfolio_id,
                account_a_position_epoch,
                account_b_portfolio_id,
                account_b_position_epoch,
                legs: vec![BatchTradeLeg {
                    asset_index,
                    market_id,
                    size_q,
                    exec_price,
                    fee_bps,
                }],
            },
            vec![
                AccountMeta::new(taker_owner.pubkey(), true),
                AccountMeta::new(maker_owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.actors[taker].portfolio, false),
                AccountMeta::new(self.actors[maker].portfolio, false),
            ],
            &[taker_owner, maker_owner],
        )
    }

    pub fn build_retained_batch_cpi_trade(
        &mut self,
        taker: usize,
        maker: usize,
        asset_index: u16,
        size_q: i128,
        limit_price: u64,
    ) -> Transaction {
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let account_a_portfolio_id = self.primary_portfolio_id(taker);
        let account_a_position_epoch = self.primary_portfolio_position_epoch(taker);
        let account_b_portfolio_id = self.primary_portfolio_id(maker);
        let account_b_position_epoch = self.primary_portfolio_position_epoch(maker);
        let taker_owner = copy_keypair(&self.actors[taker].signer);
        let binding = &self.actors[maker];
        self.build_program_transaction(
            ProgInstruction::BatchTradeCpi {
                account_a_portfolio_id,
                account_a_position_epoch,
                account_b_portfolio_id,
                account_b_position_epoch,
                legs: vec![BatchTradeCpiLeg {
                    asset_index,
                    market_id,
                    size_q,
                    fee_bps: 0,
                    limit_price,
                }],
            },
            vec![
                AccountMeta::new(taker_owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.actors[taker].portfolio, false),
                AccountMeta::new(binding.portfolio, false),
                AccountMeta::new_readonly(self.matcher_program, false),
                AccountMeta::new(binding.matcher_context, false),
                AccountMeta::new_readonly(binding.matcher_delegate, false),
            ],
            &[taker_owner],
        )
    }

    pub fn build_retained_asset_authority_handoff_from_admin(
        &mut self,
        asset_index: u16,
        kind: u8,
        new_actor_index: usize,
    ) -> Transaction {
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let current = copy_keypair(&self.admin);
        let incoming = copy_keypair(&self.actors[new_actor_index].signer);
        self.build_program_transaction(
            ProgInstruction::UpdateAssetAuthority {
                asset_index,
                market_id,
                kind,
                new_pubkey: incoming.pubkey().to_bytes(),
            },
            vec![
                AccountMeta::new(current.pubkey(), true),
                AccountMeta::new(incoming.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[current, incoming],
        )
    }

    pub fn build_retained_asset_authority_handoff_between_actors(
        &mut self,
        asset_index: u16,
        kind: u8,
        current_actor_index: usize,
        incoming_actor_index: usize,
    ) -> Transaction {
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let current = copy_keypair(&self.actors[current_actor_index].signer);
        let incoming = copy_keypair(&self.actors[incoming_actor_index].signer);
        self.build_program_transaction(
            ProgInstruction::UpdateAssetAuthority {
                asset_index,
                market_id,
                kind,
                new_pubkey: incoming.pubkey().to_bytes(),
            },
            vec![
                AccountMeta::new(current.pubkey(), true),
                AccountMeta::new(incoming.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[current, incoming],
        )
    }

    pub fn build_retained_market_authority_handoff_from_admin(
        &mut self,
        new_actor_index: usize,
    ) -> Transaction {
        let current = copy_keypair(&self.admin);
        let incoming = copy_keypair(&self.actors[new_actor_index].signer);
        self.build_program_transaction(
            ProgInstruction::UpdateAuthority {
                new_pubkey: incoming.pubkey().to_bytes(),
            },
            vec![
                AccountMeta::new(current.pubkey(), true),
                AccountMeta::new(incoming.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[current, incoming],
        )
    }

    pub fn update_market_authority_from_admin(
        &mut self,
        new_actor_index: usize,
    ) -> Result<TxSuccess, String> {
        let current = copy_keypair(&self.admin);
        let incoming = copy_keypair(&self.actors[new_actor_index].signer);
        self.send_market_authority_handoff(current, incoming)
    }

    pub fn update_market_authority_to_admin(
        &mut self,
        current_actor_index: usize,
    ) -> Result<TxSuccess, String> {
        let current = copy_keypair(&self.actors[current_actor_index].signer);
        let incoming = copy_keypair(&self.admin);
        self.send_market_authority_handoff(current, incoming)
    }

    fn send_market_authority_handoff(
        &mut self,
        current: Keypair,
        incoming: Keypair,
    ) -> Result<TxSuccess, String> {
        self.send_program(
            ProgInstruction::UpdateAuthority {
                new_pubkey: incoming.pubkey().to_bytes(),
            },
            vec![
                AccountMeta::new(current.pubkey(), true),
                AccountMeta::new(incoming.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[current, incoming],
        )
    }

    pub fn build_retained_insurance_domain_top_up_for_actor(
        &mut self,
        actor_index: usize,
        domain: u16,
        amount: u128,
    ) -> Transaction {
        let authority = copy_keypair(&self.actors[actor_index].signer);
        let asset_index = domain as usize / 2;
        let market_id = self.primary_market_state().1.assets[asset_index].market_id;
        let intent_id =
            next_control_sequence(self.primary_control_sequences(asset_index).insurance_top_up);
        self.build_program_transaction(
            ProgInstruction::TopUpInsuranceDomain {
                intent_id,
                domain,
                market_id,
                amount,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.actors[actor_index].source_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[authority],
        )
    }

    pub fn build_retained_insurance_top_up_for_actor(
        &mut self,
        actor_index: usize,
        amount: u128,
    ) -> Transaction {
        let authority = copy_keypair(&self.actors[actor_index].signer);
        let market_id = self.primary_market_state().1.assets[0].market_id;
        let intent_id = next_control_sequence(self.primary_control_sequences(0).insurance_top_up);
        self.build_program_transaction(
            ProgInstruction::TopUpInsurance {
                market_id,
                intent_id,
                amount,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.actors[actor_index].source_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[authority],
        )
    }

    pub fn build_retained_backing_bucket_top_up_for_actor(
        &mut self,
        actor_index: usize,
        domain: u16,
        amount: u128,
        expiry_slot: u64,
    ) -> Transaction {
        let authority = copy_keypair(&self.actors[actor_index].signer);
        let asset_index = domain as usize / 2;
        let market_id = self.primary_market_state().1.assets[asset_index].market_id;
        let intent_id =
            next_control_sequence(self.primary_control_sequences(asset_index).backing_top_up);
        self.build_program_transaction(
            ProgInstruction::TopUpBackingBucket {
                intent_id,
                domain,
                market_id,
                amount,
                expiry_slot,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.actors[actor_index].source_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[authority],
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn build_retained_permissionless_asset_activation(
        &mut self,
        creator_index: usize,
        asset_index: u16,
        now_slot: u64,
        initial_price: u64,
        max_init_fee: u128,
        insurance_authority_index: usize,
        insurance_operator_index: usize,
        backing_bucket_authority_index: usize,
        oracle_authority_index: usize,
    ) -> Transaction {
        let market_id = self.primary_market_state().1.next_market_id;
        let creator = copy_keypair(&self.actors[creator_index].signer);
        self.build_program_transaction(
            ProgInstruction::UpdateAssetLifecycle {
                action: percolator_prog::processor::ASSET_ACTION_ACTIVATE,
                asset_index,
                market_id,
                now_slot,
                initial_price,
                max_init_fee,
                insurance_authority: self.actors[insurance_authority_index]
                    .signer
                    .pubkey()
                    .to_bytes(),
                insurance_operator: self.actors[insurance_operator_index]
                    .signer
                    .pubkey()
                    .to_bytes(),
                backing_bucket_authority: self.actors[backing_bucket_authority_index]
                    .signer
                    .pubkey()
                    .to_bytes(),
                oracle_authority: self.actors[oracle_authority_index]
                    .signer
                    .pubkey()
                    .to_bytes(),
            },
            vec![
                AccountMeta::new(creator.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.actors[creator_index].source_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[creator],
        )
    }

    pub fn build_retained_insurance_withdrawal_for_actor(
        &mut self,
        actor_index: usize,
        asset_index: u16,
        amount: u128,
    ) -> Transaction {
        let operator = copy_keypair(&self.actors[actor_index].signer);
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        self.build_program_transaction(
            ProgInstruction::WithdrawInsuranceAsset {
                asset_index,
                market_id,
                amount,
            },
            vec![
                AccountMeta::new(operator.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(self.actors[actor_index].destination_token, false),
                AccountMeta::new(self.vault, false),
                AccountMeta::new_readonly(self.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[operator],
        )
    }

    pub fn build_retained_auth_mark(&mut self, asset_index: u16, mark_e6: u64) -> Transaction {
        let observation_sequence = next_control_sequence(
            self.primary_control_sequences(asset_index as usize)
                .oracle_observation,
        );
        self.build_retained_auth_mark_with_sequence(asset_index, mark_e6, observation_sequence)
    }

    pub fn build_retained_auth_mark_with_sequence(
        &mut self,
        asset_index: u16,
        mark_e6: u64,
        observation_sequence: u64,
    ) -> Transaction {
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let authority = copy_keypair(&self.admin);
        self.build_program_transaction(
            ProgInstruction::PushAuthMark {
                asset_index,
                market_id,
                now_slot: 0,
                mark_e6,
                observation_sequence,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn build_retained_ewma_mark(&mut self, asset_index: u16, mark_e6: u64) -> Transaction {
        let observation_sequence = next_control_sequence(
            self.primary_control_sequences(asset_index as usize)
                .oracle_observation,
        );
        self.build_retained_ewma_mark_with_sequence(asset_index, mark_e6, observation_sequence)
    }

    pub fn build_retained_ewma_mark_with_sequence(
        &mut self,
        asset_index: u16,
        mark_e6: u64,
        observation_sequence: u64,
    ) -> Transaction {
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let authority = copy_keypair(&self.admin);
        self.build_program_transaction(
            ProgInstruction::PushEwmaMark {
                asset_index,
                market_id,
                now_slot: 0,
                mark_e6,
                observation_sequence,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn build_retained_auth_config(
        &mut self,
        asset_index: u16,
        initial_mark_e6: u64,
    ) -> Transaction {
        let observation_sequence = next_control_sequence(
            self.primary_control_sequences(asset_index as usize)
                .oracle_observation,
        );
        self.build_retained_auth_config_with_sequence(
            asset_index,
            initial_mark_e6,
            observation_sequence,
        )
    }

    pub fn build_retained_auth_config_with_sequence(
        &mut self,
        asset_index: u16,
        initial_mark_e6: u64,
        observation_sequence: u64,
    ) -> Transaction {
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let authority = copy_keypair(&self.admin);
        self.build_program_transaction(
            ProgInstruction::ConfigureAuthMark {
                asset_index,
                market_id,
                now_slot: 0,
                initial_mark_e6,
                observation_sequence,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn build_retained_ewma_config(
        &mut self,
        asset_index: u16,
        initial_mark_e6: u64,
        halflife_slots: u64,
        mark_min_fee: u64,
    ) -> Transaction {
        let observation_sequence = next_control_sequence(
            self.primary_control_sequences(asset_index as usize)
                .oracle_observation,
        );
        self.build_retained_ewma_config_with_sequence(
            asset_index,
            initial_mark_e6,
            halflife_slots,
            mark_min_fee,
            observation_sequence,
        )
    }

    pub fn build_retained_ewma_config_with_sequence(
        &mut self,
        asset_index: u16,
        initial_mark_e6: u64,
        halflife_slots: u64,
        mark_min_fee: u64,
        observation_sequence: u64,
    ) -> Transaction {
        let market_id = self.primary_market_state().1.assets[asset_index as usize].market_id;
        let authority = copy_keypair(&self.admin);
        self.build_program_transaction(
            ProgInstruction::ConfigureEwmaMark {
                asset_index,
                market_id,
                now_slot: 0,
                initial_mark_e6,
                mark_ewma_halflife_slots: halflife_slots,
                mark_min_fee,
                observation_sequence,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn begin_public_trace(&mut self) {
        assert!(self.public_trace.is_none(), "public trace already active");
        self.public_trace = Some(PublicTraceCapture {
            expected_state: self.trace_state_snapshot(),
            steps: Vec::new(),
            out_of_band_economic_mutations: 0,
        });
    }

    pub fn finish_public_trace(&mut self) -> PublicTraceEvidence {
        let current_state = self.trace_state_snapshot();
        let mut capture = self
            .public_trace
            .take()
            .expect("public trace is not active");
        if current_state != capture.expected_state {
            capture.out_of_band_economic_mutations += 1;
        }
        PublicTraceEvidence {
            steps: capture.steps,
            out_of_band_economic_mutations: capture.out_of_band_economic_mutations,
        }
    }

    pub fn land_retained(&mut self, tx: Transaction) -> Result<TxSuccess, String> {
        let pending_trace = self.prepare_public_trace_step(&tx);
        let result = self.svm.send_transaction(tx);
        let compute_units = result.as_ref().ok().map(|meta| meta.compute_units_consumed);
        self.complete_public_trace_step(pending_trace, compute_units);
        result
            .map(|meta| {
                assert!(
                    meta.compute_units_consumed <= TX_CU_LIMIT,
                    "successful retained transaction consumed {} CUs",
                    meta.compute_units_consumed
                );
                TxSuccess {
                    compute_units: meta.compute_units_consumed,
                }
            })
            .map_err(|err| format!("{err:?}"))
    }

    fn trace_account_state(&self, key: Pubkey) -> Option<TraceAccountState> {
        self.svm.get_account(&key).map(|account| TraceAccountState {
            lamports: account.lamports,
            data: account.data,
            owner: account.owner,
            executable: account.executable,
            rent_epoch: account.rent_epoch,
        })
    }

    fn trace_state_keys(&self) -> Vec<Pubkey> {
        let mut keys = vec![
            self.market,
            self.foreign_market,
            self.mint,
            self.vault,
            self.foreign_vault,
            self.provider_source_token,
            self.provider_destination_token,
            self.backing_domain_ledger,
            self.market_admin_destination_token,
            self.payer.pubkey(),
            self.admin.pubkey(),
            self.foreign_admin.pubkey(),
            self.foreign_actor.signer.pubkey(),
            self.foreign_actor.portfolio,
            self.foreign_actor.source_token,
            self.foreign_actor.destination_token,
        ];
        keys.extend(self.token_accounts.iter().copied());
        for actor in &self.actors {
            keys.extend([
                actor.signer.pubkey(),
                actor.portfolio,
                actor.source_token,
                actor.destination_token,
                actor.matcher_context,
                actor.matcher_delegate,
            ]);
        }
        keys.sort_unstable_by_key(|key| key.to_bytes());
        keys.dedup();
        keys
    }

    fn trace_state_snapshot(&self) -> TraceStateSnapshot {
        TraceStateSnapshot(
            self.trace_state_keys()
                .into_iter()
                .map(|key| (key, self.trace_account_state(key)))
                .collect(),
        )
    }

    fn trace_token_balances(&self) -> Vec<(Pubkey, u64)> {
        let mut balances: Vec<_> = self
            .token_accounts
            .iter()
            .copied()
            .map(|key| {
                let amount = self
                    .svm
                    .get_account(&key)
                    .and_then(|account| TokenAccount::unpack(&account.data).ok())
                    .map(|account| account.amount)
                    .unwrap_or(0);
                (key, amount)
            })
            .collect();
        balances.sort_unstable_by_key(|(key, _)| key.to_bytes());
        balances.dedup_by_key(|(key, _)| *key);
        balances
    }

    fn prepare_public_trace_step(&mut self, tx: &Transaction) -> Option<PendingPublicTraceStep> {
        self.public_trace.as_ref()?;
        let current_state = self.trace_state_snapshot();
        let message = &tx.message;
        let instruction = message
            .instructions
            .iter()
            .rev()
            .find(|instruction| {
                message.account_keys[instruction.program_id_index as usize]
                    != solana_sdk::compute_budget::id()
            })
            .expect("traced transaction has a non-compute instruction");
        let required_signers = message.header.num_required_signatures as usize;
        let writable_signed = required_signers
            .checked_sub(message.header.num_readonly_signed_accounts as usize)
            .expect("message signed-account header");
        let writable_unsigned_end = message
            .account_keys
            .len()
            .checked_sub(message.header.num_readonly_unsigned_accounts as usize)
            .expect("message unsigned-account header");
        let transaction_signers = message.account_keys[..required_signers].to_vec();
        let accounts: Vec<_> = instruction
            .accounts
            .iter()
            .map(|raw_index| {
                let index = *raw_index as usize;
                let is_signer = index < required_signers;
                let is_writable = if is_signer {
                    index < writable_signed
                } else {
                    index < writable_unsigned_end
                };
                PublicTraceAccountMeta {
                    key: message.account_keys[index],
                    is_signer,
                    is_writable,
                }
            })
            .collect();
        let mut writable_keys: Vec<_> = accounts
            .iter()
            .filter_map(|meta| meta.is_writable.then_some(meta.key))
            .collect();
        writable_keys.sort_unstable_by_key(|key| key.to_bytes());
        writable_keys.dedup();
        let writable_before = writable_keys
            .into_iter()
            .map(|key| (key, self.trace_account_state(key)))
            .collect();
        let mut lamport_keys = transaction_signers.clone();
        lamport_keys.extend(accounts.iter().map(|meta| meta.key));
        lamport_keys.sort_unstable_by_key(|key| key.to_bytes());
        lamport_keys.dedup();
        let lamports_before = lamport_keys
            .into_iter()
            .map(|key| (key, self.account_lamports(key)))
            .collect();

        let capture = self.public_trace.as_mut().expect("trace checked above");
        if current_state != capture.expected_state {
            capture.out_of_band_economic_mutations += 1;
        }
        capture.expected_state = current_state;

        Some(PendingPublicTraceStep {
            program_id: message.account_keys[instruction.program_id_index as usize],
            instruction_data: instruction.data.clone(),
            fee_payer: message.account_keys[0],
            transaction_signers,
            accounts,
            writable_before,
            token_balances_before: self.trace_token_balances(),
            lamports_before,
        })
    }

    fn complete_public_trace_step(
        &mut self,
        pending: Option<PendingPublicTraceStep>,
        compute_units: Option<u64>,
    ) {
        let Some(pending) = pending else {
            return;
        };
        let current_state = self.trace_state_snapshot();
        let rejected_exact_writable_rollback = compute_units.is_none().then(|| {
            pending.writable_before.iter().all(|(key, before)| {
                let after = self.trace_account_state(*key);
                if *key != pending.fee_payer {
                    return after == *before;
                }
                match (before, after) {
                    (Some(before), Some(after)) => {
                        before.data == after.data
                            && before.owner == after.owner
                            && before.executable == after.executable
                            && before.rent_epoch == after.rent_epoch
                    }
                    (None, None) => true,
                    _ => false,
                }
            })
        });
        let token_balances_after = self.trace_token_balances();
        let token_deltas = pending
            .token_balances_before
            .iter()
            .map(|(key, before)| {
                let after = token_balances_after
                    .iter()
                    .find_map(|(candidate, amount)| (candidate == key).then_some(*amount))
                    .unwrap_or(0);
                (*key, i128::from(after) - i128::from(*before))
            })
            .collect();
        let lamport_deltas = pending
            .lamports_before
            .iter()
            .map(|(key, before)| {
                (
                    *key,
                    i128::from(self.account_lamports(*key)) - i128::from(*before),
                )
            })
            .collect::<Vec<_>>();
        let rejected_no_program_lamport_delta = compute_units.is_none().then(|| {
            lamport_deltas.iter().all(|(key, delta)| {
                if *key == pending.fee_payer {
                    *delta <= 0
                } else {
                    *delta == 0
                }
            })
        });
        let capture = self
            .public_trace
            .as_mut()
            .expect("public trace remained active");
        capture.expected_state = current_state;
        capture.steps.push(PublicTraceStep {
            program_id: pending.program_id,
            instruction_data: pending.instruction_data,
            fee_payer: pending.fee_payer,
            transaction_signers: pending.transaction_signers,
            accounts: pending.accounts,
            succeeded: compute_units.is_some(),
            compute_units,
            rejected_exact_writable_rollback,
            rejected_no_program_lamport_delta,
            token_deltas,
            lamport_deltas,
        });
    }

    pub fn expire_blockhash(&mut self) {
        self.svm.expire_blockhash();
    }

    pub fn warp_to_slot(&mut self, slot: u64) {
        let current = self.current_slot();
        if slot > current {
            self.svm.warp_to_slot(slot);
        }
    }

    pub fn set_clock(&mut self, slot: u64, unix_timestamp: i64) {
        self.warp_to_slot(slot);
        let mut clock = self.svm.get_sysvar::<Clock>();
        clock.unix_timestamp = unix_timestamp;
        self.svm.set_sysvar(&clock);
    }

    pub fn set_pyth_price(
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
                    owner: percolator_prog::oracle_v16::PYTH_RECEIVER_PROGRAM_ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .expect("install external Pyth fixture");
        key
    }

    pub fn current_slot(&self) -> u64 {
        self.svm.get_sysvar::<Clock>().slot
    }

    pub fn primary_market_state(&self) -> (state::WrapperConfigV16, MarketGroupV16) {
        let account = self.svm.get_account(&self.market).expect("primary market");
        state::read_market(&account.data).expect("decode primary market")
    }

    pub fn foreign_market_state(&self) -> (state::WrapperConfigV16, MarketGroupV16) {
        let account = self
            .svm
            .get_account(&self.foreign_market)
            .expect("foreign market");
        state::read_market(&account.data).expect("decode foreign market")
    }

    pub fn primary_profile(&self, asset_index: usize) -> AssetOracleProfileV16 {
        let account = self.svm.get_account(&self.market).expect("primary market");
        state::read_asset_oracle_profile(&account.data, asset_index)
            .expect("decode primary oracle profile")
    }

    pub fn primary_control_sequences(&self, asset_index: usize) -> state::AssetControlSequencesV16 {
        let account = self.svm.get_account(&self.market).expect("primary market");
        state::read_asset_control_sequences(&account.data, asset_index)
            .expect("decode primary control sequences")
    }

    pub fn foreign_control_sequences(&self, asset_index: usize) -> state::AssetControlSequencesV16 {
        let account = self
            .svm
            .get_account(&self.foreign_market)
            .expect("foreign market");
        state::read_asset_control_sequences(&account.data, asset_index)
            .expect("decode foreign control sequences")
    }

    pub fn primary_portfolio(&self, actor_index: usize) -> PortfolioAccountV16 {
        let account = self
            .svm
            .get_account(&self.actors[actor_index].portfolio)
            .expect("primary portfolio");
        state::read_portfolio(&account.data).expect("decode primary portfolio")
    }

    pub fn foreign_portfolio(&self) -> PortfolioAccountV16 {
        let account = self
            .svm
            .get_account(&self.foreign_actor.portfolio)
            .expect("foreign portfolio");
        state::read_portfolio(&account.data).expect("decode foreign portfolio")
    }

    pub fn primary_portfolio_data(&self, actor_index: usize) -> Vec<u8> {
        self.svm
            .get_account(&self.actors[actor_index].portfolio)
            .expect("primary portfolio")
            .data
    }

    pub fn foreign_portfolio_data(&self) -> Vec<u8> {
        self.svm
            .get_account(&self.foreign_actor.portfolio)
            .expect("foreign portfolio")
            .data
    }

    pub fn market_data(&self, foreign: bool) -> Vec<u8> {
        let key = if foreign {
            self.foreign_market
        } else {
            self.market
        };
        self.svm.get_account(&key).expect("market").data
    }

    pub fn backing_domain_ledger_data(&self) -> Vec<u8> {
        self.svm
            .get_account(&self.backing_domain_ledger)
            .expect("backing domain ledger")
            .data
    }

    pub fn token_amount(&self, key: Pubkey) -> u64 {
        let account = self.svm.get_account(&key).expect("token account");
        TokenAccount::unpack(&account.data)
            .expect("decode token account")
            .amount
    }

    pub fn account_lamports(&self, key: Pubkey) -> u64 {
        self.svm
            .get_account(&key)
            .map(|account| account.lamports)
            .unwrap_or(0)
    }

    pub fn all_economic_account_lamports(&self) -> Vec<(Pubkey, u64)> {
        let mut keys = vec![
            self.market,
            self.foreign_market,
            self.mint,
            self.backing_domain_ledger,
        ];
        keys.extend(self.token_accounts.iter().copied());
        for actor in &self.actors {
            keys.extend([
                actor.portfolio,
                actor.matcher_context,
                actor.matcher_delegate,
            ]);
        }
        keys.push(self.foreign_actor.portfolio);
        keys.sort_unstable_by_key(|key| key.to_bytes());
        keys.dedup();
        keys.into_iter()
            .map(|key| (key, self.account_lamports(key)))
            .collect()
    }

    pub fn token_supply_observed(&self) -> u128 {
        self.token_accounts
            .iter()
            .map(|key| self.token_amount(*key) as u128)
            .sum()
    }

    pub fn all_token_account_data(&self) -> Vec<(Pubkey, Vec<u8>)> {
        self.token_accounts
            .iter()
            .map(|key| {
                (
                    *key,
                    self.svm
                        .get_account(key)
                        .expect("tracked token account")
                        .data,
                )
            })
            .collect()
    }

    pub fn all_matcher_context_data(&self) -> Vec<Vec<u8>> {
        self.actors
            .iter()
            .map(|actor| {
                self.svm
                    .get_account(&actor.matcher_context)
                    .expect("tracked matcher context")
                    .data
            })
            .collect()
    }

    pub fn all_primary_portfolio_data(&self) -> Vec<Vec<u8>> {
        (0..PRIMARY_ACTOR_COUNT)
            .map(|i| self.primary_portfolio_data(i))
            .collect()
    }

    fn send_program(
        &mut self,
        instruction: ProgInstruction,
        accounts: Vec<AccountMeta>,
        extra_signers: &[Keypair],
    ) -> Result<TxSuccess, String> {
        let tx = self.build_program_transaction(instruction, accounts, extra_signers);
        self.land_retained(tx)
    }

    fn build_program_transaction(
        &mut self,
        instruction: ProgInstruction,
        accounts: Vec<AccountMeta>,
        extra_signers: &[Keypair],
    ) -> Transaction {
        self.build_transaction(
            Instruction {
                program_id: self.program_id,
                accounts,
                data: instruction.encode(),
            },
            extra_signers,
        )
    }

    fn send_raw_instruction(
        &mut self,
        instruction: Instruction,
        extra_signers: &[Keypair],
    ) -> Result<TxSuccess, String> {
        let tx = self.build_transaction(instruction, extra_signers);
        self.land_retained(tx)
    }

    fn build_transaction(
        &mut self,
        instruction: Instruction,
        extra_signers: &[Keypair],
    ) -> Transaction {
        self.tx_sequence = self.tx_sequence.checked_add(1).expect("tx sequence");
        let payer = copy_keypair(&self.payer);
        let mut signer_refs: Vec<&Keypair> = Vec::with_capacity(extra_signers.len() + 1);
        signer_refs.push(&payer);
        signer_refs.extend(extra_signers.iter());
        Transaction::new_signed_with_payer(
            &[
                ComputeBudgetInstruction::request_heap_frame(256 * 1024),
                ComputeBudgetInstruction::set_compute_unit_limit(TX_CU_LIMIT as u32),
                ComputeBudgetInstruction::set_compute_unit_price(self.tx_sequence),
                instruction,
            ],
            Some(&payer.pubkey()),
            &signer_refs,
            self.svm.latest_blockhash(),
        )
    }
}

fn deterministic_keypair(seed: &[u8; 32], label: u8) -> Keypair {
    let derived = hashv(&[b"percolator-stateful-fuzz", seed, &[label]]).to_bytes();
    keypair_from_seed(&derived).expect("deterministic keypair")
}

fn copy_keypair(keypair: &Keypair) -> Keypair {
    Keypair::from_bytes(&keypair.to_bytes()).expect("copy keypair")
}

fn program_path() -> PathBuf {
    if let Some(path) = std::env::var_os("PERCOLATOR_FUZZ_SBF") {
        return PathBuf::from(path);
    }
    if let Some(target_dir) = std::env::var_os("CARGO_TARGET_DIR") {
        return PathBuf::from(target_dir)
            .join("deploy")
            .join("percolator_prog.so");
    }
    artifact_path("target/deploy/percolator_prog.so", "production Percolator")
}

fn auth_matcher_program_path() -> PathBuf {
    artifact_path(
        "tests/fixtures/auth_matcher/target/deploy/auth_matcher.so",
        "authenticated matcher",
    )
}

fn artifact_path(relative: &str, label: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    assert!(
        path.exists(),
        "{label} SBF not found at {path:?}; build the exact artifact before running fuzz tests"
    );
    path
}

fn spl_token_program_path() -> PathBuf {
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").expect("HOME")).join(".cargo"));
    let registry_src = cargo_home.join("registry/src");
    for registry in std::fs::read_dir(&registry_src).expect("registry/src") {
        let registry = registry.expect("registry entry").path();
        let candidate = registry.join("litesvm-0.1.0/src/spl/programs/spl_token-3.5.0.so");
        if candidate.exists() {
            return candidate;
        }
    }
    panic!("could not locate LiteSVM SPL Token BPF under {registry_src:?}");
}

fn associated_token_program_path() -> PathBuf {
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").expect("HOME")).join(".cargo"));
    let registry_src = cargo_home.join("registry/src");
    for registry in std::fs::read_dir(&registry_src).expect("registry/src") {
        let registry = registry.expect("registry entry").path();
        let candidate =
            registry.join("litesvm-0.1.0/src/spl/programs/spl_associated_token_account-1.1.1.so");
        if candidate.exists() {
            return candidate;
        }
    }
    panic!("could not locate LiteSVM Associated Token BPF under {registry_src:?}");
}

fn associated_token_program_id() -> Pubkey {
    solana_sdk::pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")
}

fn canonical_vault_ata(vault_authority: Pubkey, mint: Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[
            vault_authority.as_ref(),
            spl_token::ID.as_ref(),
            mint.as_ref(),
        ],
        &associated_token_program_id(),
    )
    .0
}

fn matcher_delegate_key(
    program_id: &Pubkey,
    market: &Pubkey,
    maker: &Pubkey,
    maker_owner: &Pubkey,
    matcher_program: &Pubkey,
    matcher_context: &Pubkey,
) -> Pubkey {
    Pubkey::find_program_address(
        &[
            b"matcher",
            market.as_ref(),
            maker.as_ref(),
            maker_owner.as_ref(),
            matcher_program.as_ref(),
            matcher_context.as_ref(),
        ],
        program_id,
    )
    .0
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

fn set_program_account(svm: &mut LiteSVM, key: Pubkey, owner: Pubkey, data_len: usize) {
    svm.set_account(
        key,
        Account {
            lamports: 1_000_000_000,
            data: vec![0u8; data_len],
            owner,
            executable: false,
            rent_epoch: 0,
        },
    )
    .expect("set program account fixture");
}

fn set_mint_account(svm: &mut LiteSVM, key: Pubkey, supply: u64) {
    let mut data = vec![0u8; Mint::LEN];
    Mint::pack(
        Mint {
            mint_authority: COption::None,
            supply,
            decimals: 0,
            is_initialized: true,
            freeze_authority: COption::None,
        },
        &mut data,
    )
    .expect("encode mint");
    svm.set_account(
        key,
        Account {
            lamports: 1_000_000_000,
            data,
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .expect("set mint fixture");
}

fn set_token_account(svm: &mut LiteSVM, key: Pubkey, mint: Pubkey, owner: Pubkey, amount: u64) {
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
    .expect("encode token account");
    svm.set_account(
        key,
        Account {
            lamports: 1_000_000_000,
            data,
            owner: spl_token::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .expect("set token fixture");
}
