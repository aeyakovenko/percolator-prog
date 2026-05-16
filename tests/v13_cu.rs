use litesvm::LiteSVM;
use percolator::POS_SCALE;
use percolator_prog::{
    constants::{MARKET_ACCOUNT_LEN, PORTFOLIO_ACCOUNT_LEN},
    ix::Instruction as ProgInstruction,
    state,
};
use solana_sdk::{
    account::Account,
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

const CRANK_CU_LIMIT: u64 = 300_000;
const CUSTODY_CU_LIMIT: u64 = 300_000;
const TRADE_CU_LIMIT: u64 = 300_000;
const MATCHER_CONTEXT_LEN: usize = 320;

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

fn cu_ix() -> Instruction {
    ComputeBudgetInstruction::set_compute_unit_limit(1_400_000)
}

struct V13CuEnv {
    svm: LiteSVM,
    program_id: Pubkey,
    payer: Keypair,
    admin: Keypair,
    market: Pubkey,
    mint: Pubkey,
    vault: Pubkey,
    vault_authority: Pubkey,
}

impl V13CuEnv {
    fn new() -> Self {
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
                data: vec![0u8; MARKET_ACCOUNT_LEN],
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
        }
    }

    fn create_portfolio(&mut self, owner: &Keypair) -> Pubkey {
        self.create_portfolio_with_cu(owner).0
    }

    fn create_portfolio_with_cu(&mut self, owner: &Keypair) -> (Pubkey, u64) {
        let portfolio = Pubkey::new_unique();
        self.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
        self.svm
            .set_account(
                portfolio,
                Account {
                    lamports: 1_000_000_000,
                    data: vec![0u8; PORTFOLIO_ACCOUNT_LEN],
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
        self.send(
            ProgInstruction::TradeNoCpi {
                asset_index: 0,
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
        self.send(
            ProgInstruction::TradeCpi {
                asset_index: 0,
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

    fn token_amount(&self, key: Pubkey) -> u64 {
        let account = self.svm.get_account(&key).expect("token account");
        TokenAccount::unpack(&account.data).unwrap().amount
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
        &[cu_ix(), instruction],
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
        &[cu_ix(), instruction],
        Some(&payer.pubkey()),
        &signer_refs,
        svm.latest_blockhash(),
    );
    svm.send_transaction(tx)
        .map(|meta| meta.compute_units_consumed)
        .map_err(|e| format!("{e:?}"))
}

#[test]
fn v13_bpf_deposit_and_withdraw_move_spl_tokens_with_ledger() {
    let mut env = V13CuEnv::new();
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

    let (insurance_dest, _withdraw_insurance_cu) = env.withdraw_insurance_with_cu(100);
    assert_eq!(env.token_amount(insurance_dest), 100);
    assert_eq!(env.token_amount(env.vault), 750);
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    assert_eq!(group.insurance, 150);
    assert_eq!(group.vault, 750);
    assert_eq!(group.c_tot, 600);
}

#[test]
fn v13_bpf_tradenocpi_executes_and_is_bounded() {
    let mut env = V13CuEnv::new();
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
    println!("v13 TradeNoCpi BPF CU: {trade_cu}");
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
fn v13_bpf_tradecpi_executes_through_external_matcher_and_is_bounded() {
    let mut env = V13CuEnv::new();
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
    println!("v13 matcher init CU: {init_matcher_cu}, TradeCpi BPF CU: {trade_cpi_cu}");
    assert!(
        trade_cpi_cu <= TRADE_CU_LIMIT,
        "TradeCpi CU {} exceeded limit {}",
        trade_cpi_cu,
        TRADE_CU_LIMIT
    );

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let taker_data = env.svm.get_account(&taker_account).unwrap().data;
    let maker_data = env.svm.get_account(&maker_account).unwrap().data;
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
    assert_eq!(group.c_tot + group.insurance, group.vault);
}

#[test]
fn v13_bpf_permissionless_liquidation_is_bounded() {
    let mut env = V13CuEnv::new();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 100);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        100,
        0,
    );

    let liquidation_cu = env.crank(
        short_account,
        ProgInstruction::PermissionlessCrank {
            action: 1,
            asset_index: 0,
            now_slot: 1,
            effective_price: 200,
            funding_rate_e9: 0,
            close_q: POS_SCALE,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
    println!("v13 liquidation crank CU: {liquidation_cu}");
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
    assert_eq!(short.active_bitmap, 0);
}

#[test]
fn v13_bpf_close_resolved_moves_payout_tokens_with_ledger() {
    let mut env = V13CuEnv::new();
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
fn v13_cu_custody_and_resolution_paths_are_bounded() {
    let mut env = V13CuEnv::new();
    let owner = Keypair::new();
    let (portfolio, init_portfolio_cu) = env.create_portfolio_with_cu(&owner);
    let (_source, deposit_cu) = env.deposit_with_cu(&owner, portfolio, 1_000);
    let (_dest, withdraw_cu) = env.withdraw_with_cu(&owner, portfolio, 400);
    let (_insurance_source, top_up_cu) = env.top_up_insurance_with_cu(250);
    let (_insurance_dest, withdraw_insurance_cu) = env.withdraw_insurance_with_cu(100);
    let resolve_cu = env.resolve();
    let (_resolved_dest, close_resolved_cu) = env.close_resolved_with_cu(&owner, portfolio);

    println!(
        "v13 custody CU init_portfolio={init_portfolio_cu}, deposit={deposit_cu}, withdraw={withdraw_cu}, top_up={top_up_cu}, withdraw_insurance={withdraw_insurance_cu}, resolve={resolve_cu}, close_resolved={close_resolved_cu}"
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
fn v13_cu_permissionless_crank_refresh_and_recovery_are_bounded() {
    let mut env = V13CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000_000);

    let refresh_cu = env.crank(
        portfolio,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 1,
            effective_price: 101,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
    println!("v13 refresh crank CU: {refresh_cu}");
    assert!(refresh_cu <= CRANK_CU_LIMIT);

    let recovery_cu = env.crank(
        portfolio,
        ProgInstruction::PermissionlessCrank {
            action: 3,
            asset_index: 0,
            now_slot: 1,
            effective_price: 101,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
    println!("v13 recovery crank CU: {recovery_cu}");
    assert!(recovery_cu <= CRANK_CU_LIMIT);
}

#[test]
fn v13_cu_crank_cost_is_account_local_after_many_portfolios() {
    let mut env = V13CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000_000);

    let before_extra = env.crank(
        portfolio,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 1,
            effective_price: 100,
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
        assert!(state::read_portfolio(&acct.data).is_ok());
    }

    let after_extra = env.crank(
        portfolio,
        ProgInstruction::PermissionlessCrank {
            action: 0,
            asset_index: 0,
            now_slot: 2,
            effective_price: 100,
            funding_rate_e9: 0,
            close_q: 0,
            fee_bps: 0,
            recovery_reason: 0,
        },
    );
    println!(
        "v13 refresh crank CU before extra portfolios: {before_extra}, after 64 extras: {after_extra}"
    );

    assert!(after_extra <= CRANK_CU_LIMIT);
    assert!(
        after_extra.saturating_sub(before_extra) < 10_000,
        "v13 crank should stay account-local rather than scaling with materialized portfolio count"
    );
}
