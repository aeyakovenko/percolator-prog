use litesvm::LiteSVM;
use percolator_prog::{
    constants::{MARKET_ACCOUNT_LEN, PORTFOLIO_ACCOUNT_LEN},
    ix::Instruction as ProgInstruction,
    state,
};
use solana_sdk::{
    account::Account,
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};
use std::path::PathBuf;

const CRANK_CU_LIMIT: u64 = 300_000;

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

fn cu_ix() -> Instruction {
    ComputeBudgetInstruction::set_compute_unit_limit(1_400_000)
}

struct V13CuEnv {
    svm: LiteSVM,
    program_id: Pubkey,
    payer: Keypair,
    market: Pubkey,
}

impl V13CuEnv {
    fn new() -> Self {
        let mut svm = LiteSVM::new();
        let program_id = percolator_prog::id();
        let program_bytes = std::fs::read(program_path()).expect("read BPF");
        svm.add_program(program_id, &program_bytes);

        let payer = Keypair::new();
        let admin = Keypair::new();
        let market = Pubkey::new_unique();
        svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
        svm.airdrop(&admin.pubkey(), 1_000_000_000).unwrap();
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
                maintenance_margin_bps: 10_000,
                initial_margin_bps: 10_000,
                max_trading_fee_bps: 10_000,
                max_price_move_bps_per_slot: 10_000,
                max_accrual_dt_slots: 1,
                maintenance_fee_per_slot: 0,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(market, false),
            ],
            &[&admin],
        )
        .expect("init market");
        Self {
            svm,
            program_id,
            payer,
            market,
        }
    }

    fn create_portfolio(&mut self, owner: &Keypair) -> Pubkey {
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
        self.send(
            ProgInstruction::InitPortfolio,
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[owner],
        )
        .expect("init portfolio");
        portfolio
    }

    fn deposit(&mut self, owner: &Keypair, portfolio: Pubkey, amount: u128) {
        self.send(
            ProgInstruction::Deposit { amount },
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(self.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[owner],
        )
        .expect("deposit");
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
