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
const EXIT_MAKER_TOKEN_BALANCE: u64 = 2_500_000_000;
const FOREIGN_TOKEN_BALANCE: u64 = 200_000_000;
const MATCHER_CONTEXT_LEN: usize = 320;

#[derive(Clone, Copy, Debug)]
pub struct MarketConfig {
    pub h_max: u64,
    pub min_nonzero_mm_req: u128,
    pub min_nonzero_im_req: u128,
    pub maintenance_margin_bps: u64,
    pub initial_margin_bps: u64,
    pub liquidation_fee_bps: u64,
    pub liquidation_fee_cap: u128,
    pub max_price_move_bps_per_slot: u64,
    pub max_accrual_dt_slots: u64,
    pub max_abs_funding_e9_per_slot: u64,
    pub min_funding_lifetime_slots: u64,
    pub maintenance_fee_per_slot: u128,
    pub actor_deposits: [u128; PRIMARY_ACTOR_COUNT],
}

impl Default for MarketConfig {
    fn default() -> Self {
        Self {
            h_max: 10,
            min_nonzero_mm_req: 1,
            min_nonzero_im_req: 2,
            maintenance_margin_bps: 10_000,
            initial_margin_bps: 10_000,
            liquidation_fee_bps: 0,
            liquidation_fee_cap: 0,
            max_price_move_bps_per_slot: 1_000,
            max_accrual_dt_slots: 4,
            max_abs_funding_e9_per_slot: 0,
            min_funding_lifetime_slots: 4,
            maintenance_fee_per_slot: 0,
            actor_deposits: [
                USER_DEPOSIT,
                USER_DEPOSIT,
                USER_DEPOSIT,
                USER_DEPOSIT,
                EXIT_MAKER_DEPOSIT,
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
    pub actors: Vec<Actor>,
    pub foreign_actor: ForeignActor,
    pub initial_token_supply: u128,
    pub loaded_program_hash: Hash,
    payer: Keypair,
    admin: Keypair,
    foreign_admin: Keypair,
    token_accounts: Vec<Pubkey>,
    tx_sequence: u64,
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
            let source_balance = if i == EXIT_MAKER_INDEX {
                EXIT_MAKER_TOKEN_BALANCE
            } else {
                TOKEN_BALANCE_PER_USER
            };
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
        const PROVIDER_TOKEN_BALANCE: u64 = 1_000_000_000;
        token_supply += FOREIGN_TOKEN_BALANCE as u128;
        token_supply += PROVIDER_TOKEN_BALANCE as u128;
        token_accounts.extend([
            foreign_source,
            foreign_destination,
            provider_source_token,
            provider_destination_token,
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
            actors,
            foreign_actor,
            initial_token_supply: token_supply,
            loaded_program_hash,
            payer,
            admin,
            foreign_admin,
            token_accounts,
            tx_sequence: 0,
        };
        out.initialize_world(config);
        out
    }

    fn initialize_world(&mut self, config: MarketConfig) {
        self.init_market(false, config);
        self.init_market(true, config);
        self.warp_to_slot(1);
        for asset_index in 0..ASSET_COUNT as u16 {
            self.configure_auth_mark(false, asset_index, 1, INITIAL_PRICE)
                .expect("configure primary AuthMark");
            self.configure_auth_mark(true, asset_index, 1, INITIAL_PRICE)
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
                initial_price: INITIAL_PRICE,
                min_nonzero_mm_req: config.min_nonzero_mm_req,
                min_nonzero_im_req: config.min_nonzero_im_req,
                maintenance_margin_bps: config.maintenance_margin_bps,
                initial_margin_bps: config.initial_margin_bps,
                max_trading_fee_bps: 10_000,
                trade_fee_base_bps: 0,
                liquidation_fee_bps: config.liquidation_fee_bps,
                liquidation_fee_cap: config.liquidation_fee_cap,
                min_liquidation_abs: 0,
                max_price_move_bps_per_slot: config.max_price_move_bps_per_slot,
                max_accrual_dt_slots: config.max_accrual_dt_slots,
                max_abs_funding_e9_per_slot: config.max_abs_funding_e9_per_slot,
                min_funding_lifetime_slots: config.min_funding_lifetime_slots,
                max_account_b_settlement_chunks: 1,
                max_bankrupt_close_chunks: 1,
                max_bankrupt_close_lifetime_slots: 100,
                public_b_chunk_atoms: percolator::MAX_VAULT_TVL,
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
        let actor = &self.actors[actor_index];
        let owner = copy_keypair(&actor.signer);
        let context = actor.matcher_context;
        let delegate = actor.matcher_delegate;
        let portfolio = actor.portfolio;
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
        self.send_program(
            ProgInstruction::SetMatcherConfig { enabled: 1 },
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

    pub fn deposit_primary(
        &mut self,
        actor_index: usize,
        amount: u128,
    ) -> Result<TxSuccess, String> {
        let actor = &self.actors[actor_index];
        let owner = copy_keypair(&actor.signer);
        let portfolio = actor.portfolio;
        let source = actor.source_token;
        self.send_program(
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
    }

    fn deposit_foreign(&mut self, amount: u128) -> Result<TxSuccess, String> {
        let owner = copy_keypair(&self.foreign_actor.signer);
        self.send_program(
            ProgInstruction::Deposit { amount },
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
        self.send_program(
            ProgInstruction::Withdraw { amount },
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
        self.send_program(
            ProgInstruction::Withdraw { amount },
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

    pub fn convert_released_pnl(
        &mut self,
        actor_index: usize,
        amount: u128,
    ) -> Result<TxSuccess, String> {
        let actor = &self.actors[actor_index];
        let owner = copy_keypair(&actor.signer);
        self.send_program(
            ProgInstruction::ConvertReleasedPnl { amount },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(actor.portfolio, false),
            ],
            &[owner],
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
        let taker_owner = copy_keypair(&self.actors[taker].signer);
        let maker_owner = copy_keypair(&self.actors[maker].signer);
        self.send_program(
            ProgInstruction::TradeNoCpi {
                asset_index,
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
        let taker_owner = copy_keypair(&self.actors[taker].signer);
        let maker_owner = copy_keypair(&self.actors[maker].signer);
        self.send_program(
            ProgInstruction::BatchTradeNoCpi { legs },
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
        let taker_owner = copy_keypair(&self.actors[taker].signer);
        let binding = &self.actors[maker];
        self.send_program(
            ProgInstruction::TradeCpi {
                asset_index,
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
        let taker_owner = copy_keypair(&self.actors[taker].signer);
        let binding = &self.actors[maker];
        self.send_program(
            ProgInstruction::BatchTradeCpi { legs },
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
        let (authority, market) = if foreign {
            (copy_keypair(&self.foreign_admin), self.foreign_market)
        } else {
            (copy_keypair(&self.admin), self.market)
        };
        self.send_program(
            ProgInstruction::ConfigureAuthMark {
                asset_index,
                now_slot,
                initial_mark_e6: mark,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(market, false),
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
        let authority = copy_keypair(&self.admin);
        self.send_program(
            ProgInstruction::ConfigureEwmaMark {
                asset_index,
                now_slot,
                initial_mark_e6: mark,
                mark_ewma_halflife_slots: halflife_slots,
                mark_min_fee,
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
        let authority = copy_keypair(&self.admin);
        self.send_program(
            ProgInstruction::PushEwmaMark {
                asset_index,
                now_slot,
                mark_e6: mark,
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
        let authority = copy_keypair(&self.admin);
        self.send_program(
            ProgInstruction::PushAuthMark {
                asset_index,
                now_slot,
                mark_e6: mark,
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
        let authority = copy_keypair(&self.admin);
        self.send_program(
            ProgInstruction::UpdateBackingFeePolicy {
                domain,
                fee_bps,
                insurance_share_bps,
            },
            vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(self.market, false),
            ],
            &[authority],
        )
    }

    pub fn top_up_backing_bucket(
        &mut self,
        domain: u16,
        amount: u128,
        expiry_slot: u64,
    ) -> Result<TxSuccess, String> {
        let authority = copy_keypair(&self.admin);
        self.send_program(
            ProgInstruction::TopUpBackingBucket {
                domain,
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

    pub fn withdraw_backing_bucket_earnings(
        &mut self,
        domain: u16,
        amount: u128,
    ) -> Result<TxSuccess, String> {
        let authority = copy_keypair(&self.admin);
        self.send_program(
            ProgInstruction::WithdrawBackingBucketEarnings { domain, amount },
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

    pub fn cross_market_trade_substitution(
        &mut self,
        actor_index: usize,
        size_q: i128,
    ) -> Result<TxSuccess, String> {
        let primary_owner = copy_keypair(&self.actors[actor_index].signer);
        let foreign_owner = copy_keypair(&self.foreign_actor.signer);
        self.send_program(
            ProgInstruction::TradeNoCpi {
                asset_index: 0,
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
        self.send_program(
            ProgInstruction::Deposit { amount },
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
        self.send_program(
            ProgInstruction::Withdraw { amount },
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
        let taker_owner = copy_keypair(&self.actors[taker].signer);
        let maker_portfolio = self.actors[maker].portfolio;
        let binding = &self.actors[substituted_binding];
        self.send_program(
            ProgInstruction::TradeCpi {
                asset_index: 0,
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
        let taker_owner = copy_keypair(&self.actors[taker].signer);
        let maker_owner = copy_keypair(&self.actors[maker].signer);
        self.build_program_transaction(
            ProgInstruction::TradeNoCpi {
                asset_index,
                size_q,
                exec_price,
                fee_bps: 0,
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

    pub fn build_retained_cpi_trade(
        &mut self,
        taker: usize,
        maker: usize,
        asset_index: u16,
        size_q: i128,
        limit_price: u64,
    ) -> Transaction {
        let taker_owner = copy_keypair(&self.actors[taker].signer);
        let binding = &self.actors[maker];
        self.build_program_transaction(
            ProgInstruction::TradeCpi {
                asset_index,
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
        let taker_owner = copy_keypair(&self.actors[taker].signer);
        let maker_owner = copy_keypair(&self.actors[maker].signer);
        self.build_program_transaction(
            ProgInstruction::BatchTradeNoCpi {
                legs: vec![BatchTradeLeg {
                    asset_index,
                    size_q,
                    exec_price,
                    fee_bps: 0,
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
        let taker_owner = copy_keypair(&self.actors[taker].signer);
        let binding = &self.actors[maker];
        self.build_program_transaction(
            ProgInstruction::BatchTradeCpi {
                legs: vec![BatchTradeCpiLeg {
                    asset_index,
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

    pub fn land_retained(&mut self, tx: Transaction) -> Result<TxSuccess, String> {
        self.svm
            .send_transaction(tx)
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

    pub fn expire_blockhash(&mut self) {
        self.svm.expire_blockhash();
    }

    pub fn warp_to_slot(&mut self, slot: u64) {
        let current = self.current_slot();
        if slot > current {
            self.svm.warp_to_slot(slot);
        }
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

    pub fn token_supply_observed(&self) -> u128 {
        self.token_accounts
            .iter()
            .map(|key| self.token_amount(*key) as u128)
            .sum()
    }

    pub fn all_token_account_data(&self) -> Vec<Vec<u8>> {
        self.token_accounts
            .iter()
            .map(|key| {
                self.svm
                    .get_account(key)
                    .expect("tracked token account")
                    .data
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
