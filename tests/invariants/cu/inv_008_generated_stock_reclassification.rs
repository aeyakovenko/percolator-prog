//! INV-008/010/011/024/031/080/081: generated retained capital/insurance histories.
//! A receipt book binds each debit to its stock class, consumed guard and exact
//! destination. Signed wallet transfers reattribute stock between separate owners.
//! INV-064 evidence is local Live allowance only. Rows 415/428 remain OPEN.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use rand::{Rng, SeedableRng};
use rand_xorshift::XorShiftRng;

const LIMIT: u64 = 600_000;
const PRINCIPAL: u128 = 101;
const RESERVE: [u128; 2] = [13, 47];
const PEER: u128 = 103;
const EXTERNAL: u128 = 211;
const TOTAL: u128 = PRINCIPAL + RESERVE[0] + RESERVE[1] + PEER + EXTERNAL;

fn inv008_source_defines_test(source: &str, function: &str) -> bool {
    let expected = format!("fn {function}");
    let mut test_attribute = false;

    for line in source.lines() {
        let line = line.trim();
        if line == "#[test]" {
            test_attribute = true;
        } else if line.starts_with("fn ") {
            if test_attribute
                && line
                    .strip_prefix(&expected)
                    .is_some_and(|tail| tail.trim_start().starts_with('('))
            {
                return true;
            }
            test_attribute = false;
        } else if test_attribute && !line.is_empty() && !line.starts_with('#') {
            test_attribute = false;
        }
    }

    false
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    Capital,
    Insurance,
}

impl Class {
    fn variant(self) -> &'static str {
        match self {
            Self::Capital => "Withdraw",
            Self::Insurance => "WithdrawInsuranceAsset",
        }
    }
}

#[derive(Clone, Debug)]
enum Op {
    Debit {
        class: Class,
        guard: u64,
        amount: u128,
        wallet: usize,
        observed: bool,
    },
    Credit {
        class: Class,
        guard: u64,
        intent: u64,
        domain: usize,
        amount: u128,
        wallet: usize,
    },
    Transfer {
        from: usize,
        to: usize,
        amount: u128,
    },
}

#[derive(Clone, Debug)]
struct Receipt {
    amount: u128,
    stock: [u128; 3],
    wallet: usize,
}

#[derive(Clone)]
struct Book {
    stock: [u128; 3],
    credited: [u128; 3],
    wallets: [u128; 5],
    moved: [i128; 5],
    funded: [u128; 5],
    sequence: u64,
    controls: state::AssetControlSequencesV16,
    receipts: BTreeMap<(Class, u64), Receipt>,
    ledger: Option<state::InsuranceLedgerAccountV16>,
}

impl Book {
    fn initial() -> Self {
        Self {
            stock: [PRINCIPAL, RESERVE[0], RESERVE[1]],
            credited: [PRINCIPAL, RESERVE[0], RESERVE[1]],
            wallets: [0, 0, 0, 0, EXTERNAL],
            moved: [0; 5],
            funded: [0; 5],
            sequence: 1,
            controls: state::AssetControlSequencesV16 {
                insurance_top_up: 2,
                ..state::AssetControlSequencesV16::default()
            },
            receipts: BTreeMap::new(),
            ledger: None,
        }
    }

    fn guard(&self, class: Class) -> u64 {
        match class {
            Class::Capital => self.sequence,
            Class::Insurance => self.controls.authority_epoch,
        }
    }

    fn debit(&self, class: Class, amount: u128, wallet: usize, observed: bool) -> Op {
        Op::Debit {
            class,
            guard: self.guard(class),
            amount,
            wallet,
            observed,
        }
    }

    fn credit(&self, class: Class, amount: u128, wallet: usize, domain: usize) -> Op {
        Op::Credit {
            class,
            guard: self.guard(class),
            intent: self.controls.insurance_top_up + 1,
            domain,
            amount,
            wallet,
        }
    }

    fn apply(&mut self, op: &Op, market: Pubkey, insurer: Pubkey) {
        match *op {
            Op::Debit {
                class,
                guard,
                amount,
                wallet,
                observed,
            } => {
                assert_eq!(guard, self.guard(class));
                assert!(amount > 0);
                let mut stock = [0; 3];
                match class {
                    Class::Capital => {
                        assert!(amount <= self.stock[0]);
                        stock[0] = amount;
                        self.sequence += 1;
                    }
                    Class::Insurance => {
                        let available = self.stock[1] + self.stock[2];
                        assert!(amount <= available);
                        stock[1] = amount.min(self.stock[1]);
                        stock[2] = amount - stock[1];
                        if observed {
                            let ledger =
                                self.ledger.get_or_insert(state::InsuranceLedgerAccountV16 {
                                    market_group: market.to_bytes(),
                                    authority: insurer.to_bytes(),
                                    last_observed_insurance_atoms: available,
                                    ..state::InsuranceLedgerAccountV16::default()
                                });
                            if available >= ledger.last_observed_insurance_atoms {
                                ledger.cumulative_profit_atoms +=
                                    available - ledger.last_observed_insurance_atoms;
                            } else {
                                ledger.cumulative_loss_atoms +=
                                    ledger.last_observed_insurance_atoms - available;
                            }
                            ledger.total_withdrawn_atoms += amount;
                            ledger.last_observed_insurance_atoms = available - amount;
                        }
                        self.controls.authority_epoch += 1;
                    }
                }
                for (remaining, consumed) in self.stock.iter_mut().zip(stock) {
                    *remaining -= consumed;
                }
                assert!(
                    self.receipts
                        .insert(
                            (class, guard),
                            Receipt {
                                amount,
                                stock,
                                wallet
                            }
                        )
                        .is_none(),
                    "a consumed guard has no second budget"
                );
                self.wallets[wallet] += amount;
            }
            Op::Credit {
                class,
                guard,
                intent,
                domain,
                amount,
                wallet,
            } => {
                assert_eq!(guard, self.guard(class));
                self.wallets[wallet] -= amount;
                self.funded[wallet] += amount;
                let stock = match class {
                    Class::Capital => {
                        self.sequence += 1;
                        0
                    }
                    Class::Insurance => {
                        assert_eq!(intent, self.controls.insurance_top_up + 1);
                        self.controls.insurance_top_up = intent;
                        1 + domain
                    }
                };
                self.stock[stock] += amount;
                self.credited[stock] += amount;
            }
            Op::Transfer { from, to, amount } => {
                self.wallets[from] -= amount;
                self.wallets[to] += amount;
                self.moved[from] -= amount as i128;
                self.moved[to] += amount as i128;
            }
        }
    }

    fn accepts(&self, stock: [u128; 3], wallets: [u128; 5]) -> bool {
        let mut consumed = [0; 3];
        let mut paid = [0; 5];
        for receipt in self.receipts.values() {
            assert_eq!(receipt.stock.iter().sum::<u128>(), receipt.amount);
            for (total, atoms) in consumed.iter_mut().zip(receipt.stock) {
                *total += atoms;
            }
            paid[receipt.wallet] += receipt.amount;
        }
        (0..3).all(|i| stock[i] + consumed[i] == self.credited[i])
            && (0..5).all(|i| {
                wallets[i] as i128
                    == (if i == 4 { EXTERNAL } else { 0 }) as i128 + paid[i] as i128 + self.moved[i]
                        - self.funded[i] as i128
            })
    }
}

struct World {
    env: V16CuEnv,
    capital_owner: Keypair,
    insurer: Keypair,
    portfolios: [Pubkey; 2],
    wallets: [Pubkey; 5],
    wallet_frames: [solana_sdk::account::Account; 5],
    vault_frame: solana_sdk::account::Account,
    mint_frame: solana_sdk::account::Account,
    peer_frame: solana_sdk::account::Account,
    ledger: Pubkey,
    ledger_frame: solana_sdk::account::Account,
    portfolio_id: u64,
    market_id: u64,
    profile: state::AssetOracleProfileV16,
}

impl World {
    fn new(coheld: bool) -> Self {
        let mut env = inv018_public_spl_market_with_params(6, V16CuMarketParams::default());
        let insurer = env.admin.insecure_clone();
        let capital_owner = if coheld {
            insurer.insecure_clone()
        } else {
            Keypair::new()
        };
        let peer = Keypair::new();
        let portfolios = [&capital_owner, &peer].map(|owner| {
            if owner.pubkey() != insurer.pubkey() {
                env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
            }
            let key = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &key,
                env.portfolio_account_len,
                env.program_id,
            );
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(key.pubkey(), false),
                ],
                &[owner],
            )
            .unwrap();
            key.pubkey()
        });
        let wallets = std::array::from_fn(|i| {
            let owner = if i < 2 {
                capital_owner.pubkey()
            } else {
                insurer.pubkey()
            };
            let key = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &key,
                TokenAccount::LEN,
                spl_token::ID,
            );
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::initialize_account3(
                    &spl_token::ID,
                    &key.pubkey(),
                    &env.mint,
                    &owner,
                )
                .unwrap(),
                &[],
            )
            .unwrap();
            key.pubkey()
        });
        let peer_wallet = create_ata_for_test(&mut env.svm, &env.payer, peer.pubkey(), env.mint);
        for (key, amount) in [
            (wallets[0], PRINCIPAL),
            (wallets[2], RESERVE.iter().sum()),
            (wallets[4], EXTERNAL),
            (peer_wallet, PEER),
        ] {
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &key,
                    &insurer.pubkey(),
                    &[],
                    amount as u64,
                )
                .unwrap(),
                &[&insurer],
            )
            .unwrap();
        }
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::set_authority(
                &spl_token::ID,
                &env.mint,
                None,
                spl_token::instruction::AuthorityType::MintTokens,
                &insurer.pubkey(),
                &[],
            )
            .unwrap(),
            &[&insurer],
        )
        .unwrap();
        for (i, owner, wallet, amount) in [
            (0, &capital_owner, wallets[0], PRINCIPAL),
            (1, &peer, peer_wallet, PEER),
        ] {
            env.send(
                env.deposit_ix(portfolios[i], amount),
                vec![
                    AccountMeta::new(owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[i], false),
                    AccountMeta::new(wallet, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[owner],
            )
            .unwrap();
        }
        for (domain, amount) in RESERVE.into_iter().enumerate() {
            env.send(
                ProgInstruction::TopUpInsuranceDomain {
                    domain: domain as u16,
                    market_id: env.asset_market_id(0),
                    authority_epoch: 0,
                    intent_id: domain as u64 + 1,
                    amount,
                },
                vec![
                    AccountMeta::new(insurer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(wallets[2], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&insurer],
            )
            .unwrap();
        }
        let ledger = Keypair::new();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &ledger,
            state::insurance_ledger_account_len(),
            env.program_id,
        );
        Self {
            wallet_frames: wallets.map(|key| env.svm.get_account(&key).unwrap()),
            vault_frame: env.svm.get_account(&env.vault).unwrap(),
            mint_frame: env.svm.get_account(&env.mint).unwrap(),
            peer_frame: env.svm.get_account(&portfolios[1]).unwrap(),
            ledger_frame: env.svm.get_account(&ledger.pubkey()).unwrap(),
            ledger: ledger.pubkey(),
            portfolio_id: env.portfolio_id(portfolios[0]),
            market_id: env.asset_market_id(0),
            profile: state::read_asset_oracle_profile(
                &env.svm.get_account(&env.market).unwrap().data,
                0,
            )
            .unwrap(),
            env,
            capital_owner,
            insurer,
            portfolios,
            wallets,
        }
    }

    fn ix(&self, op: &Op) -> Instruction {
        let (class, debit, wallet, ix, observed) = match *op {
            Op::Debit {
                class,
                guard,
                amount,
                wallet,
                observed,
            } => (
                class,
                true,
                wallet,
                match class {
                    Class::Capital => ProgInstruction::Withdraw {
                        portfolio_id: self.portfolio_id,
                        expected_sequence: guard,
                        amount,
                    },
                    Class::Insurance => ProgInstruction::WithdrawInsuranceAsset {
                        asset_index: 0,
                        market_id: self.market_id,
                        authority_epoch: guard,
                        amount,
                    },
                },
                observed,
            ),
            Op::Credit {
                class,
                guard,
                intent,
                domain,
                amount,
                wallet,
            } => (
                class,
                false,
                wallet,
                match class {
                    Class::Capital => ProgInstruction::Deposit {
                        portfolio_id: self.portfolio_id,
                        expected_sequence: guard,
                        amount,
                    },
                    Class::Insurance => ProgInstruction::TopUpInsuranceDomain {
                        domain: domain as u16,
                        market_id: self.market_id,
                        authority_epoch: guard,
                        intent_id: intent,
                        amount,
                    },
                },
                false,
            ),
            Op::Transfer { from, to, amount } => {
                return spl_token::instruction::transfer(
                    &spl_token::ID,
                    &self.wallets[from],
                    &self.wallets[to],
                    &if from < 2 {
                        self.capital_owner.pubkey()
                    } else {
                        self.insurer.pubkey()
                    },
                    &[],
                    amount as u64,
                )
                .unwrap()
            }
        };
        let owner = if class == Class::Capital {
            self.capital_owner.pubkey()
        } else {
            self.insurer.pubkey()
        };
        let mut accounts = vec![
            AccountMeta::new(owner, true),
            AccountMeta::new(self.env.market, false),
        ];
        if class == Class::Capital {
            accounts.push(AccountMeta::new(self.portfolios[0], false));
        }
        accounts.extend([
            AccountMeta::new(self.wallets[wallet], false),
            AccountMeta::new(self.env.vault, false),
        ]);
        if debit {
            accounts.push(AccountMeta::new_readonly(self.env.vault_authority, false));
        }
        accounts.push(AccountMeta::new_readonly(spl_token::ID, false));
        if class == Class::Insurance && observed {
            accounts.push(AccountMeta::new(self.ledger, false));
        }
        Instruction {
            program_id: self.env.program_id,
            accounts,
            data: ix.encode(),
        }
    }

    fn check(&self, book: &Book) {
        let group = self.env.market_state().1;
        let portfolio = self.env.portfolio_state(self.portfolios[0]);
        let stock = [
            portfolio.capital.get(),
            group.insurance_domain_budget[0],
            group.insurance_domain_budget[1],
        ];
        let wallets = self.wallets.map(|key| self.env.token_amount(key) as u128);
        assert!(
            book.accepts(stock, wallets),
            "receipt-derived stock and destination entitlement"
        );
        assert_eq!((stock, wallets), (book.stock, book.wallets));
        let custody = stock.iter().sum::<u128>() + PEER;
        assert_eq!(custody + wallets.iter().sum::<u128>(), TOTAL);
        assert_eq!(
            (group.c_tot, group.insurance, group.vault),
            (stock[0] + PEER, stock[1] + stock[2], custody)
        );
        assert_eq!(group.insurance_domain_spent, [0; 2]);
        assert_eq!(
            group.insurance_domain_budget_remaining_total,
            stock[1] + stock[2]
        );
        assert_eq!(group.mode, MarketModeV16::Live);
        assert_eq!(group.materialized_portfolio_count, 2);
        assert_eq!(group.pnl_pos_tot, 0);
        assert_eq!(portfolio.pnl.get(), 0);
        assert!(portfolio.active_bitmap.iter().all(|word| word.get() == 0));
        assert_eq!(self.env.portfolio_id(self.portfolios[0]), self.portfolio_id);
        assert_eq!(
            self.env.portfolio_matcher_sequence(self.portfolios[0]),
            book.sequence
        );
        assert_eq!(self.env.control_sequences(0), book.controls);
        assert_eq!(self.env.asset_market_id(0), self.market_id);
        assert_eq!(
            state::read_asset_oracle_profile(
                &self.env.svm.get_account(&self.env.market).unwrap().data,
                0
            )
            .unwrap(),
            self.profile
        );
        assert_eq!(
            self.env.svm.get_account(&self.portfolios[1]).unwrap(),
            self.peer_frame
        );
        for (key, frame, amount) in self
            .wallets
            .iter()
            .zip(&self.wallet_frames)
            .zip(wallets)
            .map(|((key, frame), amount)| (*key, frame, amount))
            .chain(std::iter::once((
                self.env.vault,
                &self.vault_frame,
                custody,
            )))
        {
            let mut expected = frame.clone();
            let mut token = TokenAccount::unpack(&expected.data).unwrap();
            token.amount = amount as u64;
            TokenAccount::pack(token, &mut expected.data).unwrap();
            assert_eq!(self.env.svm.get_account(&key).unwrap(), expected);
        }
        let mut ledger = self.ledger_frame.clone();
        if let Some(record) = &book.ledger {
            state::init_insurance_ledger(&mut ledger.data, record).unwrap();
        }
        assert_eq!(self.env.svm.get_account(&self.ledger).unwrap(), ledger);
        assert_eq!(
            self.env.svm.get_account(&self.env.mint).unwrap(),
            self.mint_frame
        );
        let mint = Mint::unpack(&self.mint_frame.data).unwrap();
        assert_eq!(mint.supply as u128, TOTAL);
        assert_eq!(mint.mint_authority, COption::None);
        let portfolios = self.portfolios.map(|key| self.env.portfolio_state(key));
        assert_market_stock_census(
            "stock reclassification",
            &group,
            &self.env.svm.get_account(&self.env.market).unwrap().data,
            &portfolios,
            custody,
        )
        .unwrap();
        assert_reservation_encumbrance_census("stock reclassification", &group, &portfolios)
            .unwrap();
        let mut data = self.env.svm.get_account(&self.env.market).unwrap().data;
        state::market_view_mut(&mut data)
            .unwrap()
            .1
            .validate_shape()
            .unwrap();
    }
}

struct Delivery {
    wire: Vec<u8>,
    committed: Vec<Op>,
    failure: Option<(usize, InstructionError, [usize; 2])>,
}

fn retain(
    world: &World,
    deliveries: &mut Vec<Delivery>,
    ops: Vec<Op>,
    suffix: Option<(Op, InstructionError)>,
) {
    let mut ixs = vec![
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(LIMIT as u32 - deliveries.len() as u32),
    ];
    ixs.extend(ops.iter().map(|op| world.ix(op)));
    let failure = suffix.map(|(op, error)| {
        ixs.push(world.ix(&op));
        let wrappers = ops
            .iter()
            .filter(|op| !matches!(op, Op::Transfer { .. }))
            .count();
        (ops.len(), error, [wrappers, ops.len()])
    });
    let mut signers = vec![&world.env.payer];
    for key in [&world.capital_owner, &world.insurer] {
        if !signers.iter().any(|signer| signer.pubkey() == key.pubkey())
            && ixs
                .iter()
                .flat_map(|ix| &ix.accounts)
                .any(|meta| meta.is_signer && meta.pubkey == key.pubkey())
        {
            signers.push(key);
        }
    }
    let tx = Transaction::new_signed_with_payer(
        &ixs,
        Some(&world.env.payer.pubkey()),
        &signers,
        world.env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    let wire = bincode::serialize(&tx).unwrap();
    assert!(
        wire.len() <= 1232,
        "public transaction packet: {}",
        wire.len()
    );
    deliveries.push(Delivery {
        wire,
        committed: if failure.is_none() { ops } else { Vec::new() },
        failure,
    });
}

#[test]
fn v16_generated_retained_stock_reclassification_preserves_each_owners_budget() {
    let mut evidence = Evidence::default();
    let mut worlds = 0;
    let mut rollbacks = 0;
    let mut restored = 0;
    let mut max_packet = 0;
    let mut outcomes = BTreeMap::new();
    let mut shapes = BTreeSet::new();
    for seed in 0..2u64 {
        let mut rng = XorShiftRng::seed_from_u64(0x4753_544f_434b + seed);
        let amounts: [[u128; 3]; 3] = std::array::from_fn(|_| {
            [
                rng.gen_range(3..13),
                rng.gen_range(3..13),
                rng.gen_range(19..42),
            ]
        });
        assert!(shapes.insert(amounts));
        for order in ORDERS {
            for coheld in [false, true] {
                for alternate in [false, true] {
                    for split in [false, true] {
                        let context = format!("seed={seed} order={order:?} coheld={coheld} alternate={alternate} split={split}");
                        let mut world = World::new(coheld);
                        let mut planned = Book::initial();
                        world.check(&planned);
                        let mut deliveries = Vec::new();
                        let stale = [
                            planned.debit(Class::Capital, 17, 0, false),
                            planned.debit(Class::Insurance, 19, 2, true),
                        ];
                        let late = Op::Transfer {
                            from: 4,
                            to: 0,
                            amount: EXTERNAL + 1,
                        };
                        let late_error = InstructionError::Custom(
                            spl_token::error::TokenError::InsufficientFunds as u32,
                        );
                        let stale_error =
                            InstructionError::Custom(PercolatorError::EngineStale as u32);
                        retain(
                            &world,
                            &mut deliveries,
                            stale.to_vec(),
                            Some((late.clone(), late_error.clone())),
                        );
                        retain(&world, &mut deliveries, stale.to_vec(), None);
                        for op in &stale {
                            planned.apply(op, world.env.market, world.insurer.pubkey());
                        }
                        for (round, amounts) in amounts.iter().enumerate() {
                            let mut ops = Vec::new();
                            let target = usize::from(alternate && round % 2 == 0);
                            for action in order {
                                // Each word is an authorized payout/transfer/credit path. Both
                                // owners sign cross-owner transfers; deposits do not imply gifts.
                                let (from, to, class, amount) = match action {
                                    0 => (target, 2 + target, Class::Insurance, amounts[0]),
                                    1 => (2 + target, target, Class::Capital, amounts[1]),
                                    2 => (
                                        4,
                                        if round % 2 == 0 { target } else { 2 + target },
                                        if round % 2 == 0 {
                                            Class::Capital
                                        } else {
                                            Class::Insurance
                                        },
                                        amounts[2],
                                    ),
                                    _ => unreachable!(),
                                };
                                if action != 2 {
                                    let source_class = if action == 0 {
                                        Class::Capital
                                    } else {
                                        Class::Insurance
                                    };
                                    // The guard is projected only while compiling the full history.
                                    let op = planned.debit(source_class, amount, from, round != 1);
                                    planned.apply(&op, world.env.market, world.insurer.pubkey());
                                    ops.push(op);
                                }
                                let op = Op::Transfer { from, to, amount };
                                planned.apply(&op, world.env.market, world.insurer.pubkey());
                                ops.push(op);
                                let op = planned.credit(class, amount, to, round % 2);
                                planned.apply(&op, world.env.market, world.insurer.pubkey());
                                ops.push(op);
                            }
                            for old in &stale {
                                retain(
                                    &world,
                                    &mut deliveries,
                                    ops.clone(),
                                    Some((old.clone(), stale_error.clone())),
                                );
                            }
                            retain(
                                &world,
                                &mut deliveries,
                                ops.clone(),
                                Some((late.clone(), late_error.clone())),
                            );
                            if split {
                                for op in ops {
                                    retain(&world, &mut deliveries, vec![op], None);
                                }
                            } else {
                                retain(&world, &mut deliveries, ops, None);
                            }
                            for old in &stale {
                                let mut variant = old.clone();
                                if let Op::Debit {
                                    wallet, observed, ..
                                } = &mut variant
                                {
                                    *wallet += 1;
                                    *observed = false;
                                }
                                retain(
                                    &world,
                                    &mut deliveries,
                                    Vec::new(),
                                    Some((variant, stale_error.clone())),
                                );
                            }
                        }
                        let economic_end = (
                            [planned.stock[0], planned.stock[1] + planned.stock[2]],
                            planned.wallets,
                            planned.sequence,
                            planned.controls,
                        );
                        let mut exits = Vec::new();
                        for (class, amount, wallet) in [
                            (Class::Capital, planned.stock[0], 0),
                            (Class::Insurance, planned.stock[1] + planned.stock[2], 2),
                        ] {
                            let op = planned.debit(class, amount, wallet, true);
                            planned.apply(&op, world.env.market, world.insurer.pubkey());
                            exits.push(op);
                        }
                        retain(
                            &world,
                            &mut deliveries,
                            exits.clone(),
                            Some((late, late_error)),
                        );
                        retain(&world, &mut deliveries, exits, None);
                        let frame = [
                            world.env.market,
                            world.env.mint,
                            world.env.vault,
                            world.ledger,
                            world.portfolios[0],
                            world.portfolios[1],
                            world.wallets[0],
                            world.wallets[1],
                            world.wallets[2],
                            world.wallets[3],
                            world.wallets[4],
                            world.capital_owner.pubkey(),
                            world.insurer.pubkey(),
                        ];
                        let mut actual = Book::initial();
                        let mut signatures = BTreeSet::new();
                        for (step, delivery) in deliveries.into_iter().enumerate() {
                            max_packet = max_packet.max(delivery.wire.len());
                            let tx: Transaction = bincode::deserialize(&delivery.wire).unwrap();
                            assert_eq!(bincode::serialize(&tx).unwrap(), delivery.wire);
                            assert!(signatures.insert(tx.signatures[0]));
                            tx.verify().unwrap();
                            if let Some((_, _, successes)) = &delivery.failure {
                                rollbacks += 1;
                                restored += successes[1];
                            }
                            checked_send_with_limit(
                                &mut world.env,
                                tx,
                                &frame,
                                delivery.failure,
                                &mut evidence,
                                LIMIT,
                            );
                            for op in &delivery.committed {
                                actual.apply(op, world.env.market, world.insurer.pubkey());
                            }
                            world.check(&actual);
                            let mut wrong_stock = actual.stock;
                            if wrong_stock[1] + wrong_stock[2] > 0 {
                                let i = if wrong_stock[1] > 0 { 1 } else { 2 };
                                wrong_stock[i] -= 1;
                                wrong_stock[0] += 1;
                                assert!(
                                    !actual.accepts(wrong_stock, actual.wallets),
                                    "wrong class {context} step={step}"
                                );
                            }
                            let mut wrong_wallets = actual.wallets;
                            wrong_wallets[4] -= 1;
                            wrong_wallets[0] += 1;
                            assert!(
                                !actual.accepts(actual.stock, wrong_wallets),
                                "wrong recipient {context} step={step}"
                            );
                        }
                        assert_eq!(actual.stock, [0; 3]);
                        assert_eq!(world.env.token_amount(world.env.vault) as u128, PEER);
                        let into_insurance: u128 = amounts.iter().map(|round| round[0]).sum();
                        let into_capital: u128 = amounts.iter().map(|round| round[1]).sum();
                        assert_eq!(
                            actual.wallets,
                            [
                                PRINCIPAL + into_capital + amounts[0][2] + amounts[2][2]
                                    - into_insurance,
                                0,
                                RESERVE.iter().sum::<u128>() + into_insurance + amounts[1][2]
                                    - into_capital,
                                0,
                                EXTERNAL - amounts.iter().map(|round| round[2]).sum::<u128>(),
                            ],
                            "input-derived final owner entitlement: {context}"
                        );
                        let endpoint = (economic_end, actual.wallets);
                        if let Some(previous) = outcomes.insert((seed, coheld, alternate), endpoint)
                        {
                            assert_eq!(
                                previous, endpoint,
                                "order/split economic equivalence: {context}"
                            );
                        }
                        worlds += 1;
                    }
                }
            }
        }
    }
    assert_eq!(worlds, 96);
    assert_eq!(rollbacks, worlds * 17);
    assert_eq!(restored, worlds * 76);
    assert_eq!(evidence.transactions, 3120);
    eprintln!("INV-008 Scope G: {worlds} histories, {} transactions, {rollbacks} exact rollbacks, {restored} restored SPL transfers, peak {} CU, max packet {max_packet} bytes", evidence.transactions, evidence.max_cu);
}

#[test]
fn v16_retained_stock_epoch_route_matrix_accounts_for_every_signed_outflow() {
    let source = include_str!("../../../src/v16_program.rs");
    let mut handlers = BTreeSet::new();
    for (offset, _) in source.match_indices("    fn handle_") {
        let suffix = &source[offset + "    fn ".len()..];
        let name = suffix.split(['<', '(']).next().unwrap();
        let body = super::super::braced_block_after(source, &format!("fn {name}"));
        if body.contains("transfer_tokens_signed(") {
            handlers.insert(name);
        }
    }
    let effects: BTreeSet<_> = include_str!("../inv_024_entitlement_route_dispositions.tsv")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.starts_with("tag\t"))
        .filter_map(|line| {
            let columns: Vec<_> = line.split('\t').collect();
            (columns.len() == 5
                && (columns[2].ends_with("Out")
                    || matches!(columns[2], "TerminalPayout" | "ReserveSwap")))
            .then(|| columns[1])
        })
        .collect();
    let variants = super::super::instruction_variants(source);
    let mut listed_handlers = BTreeSet::new();
    let mut listed_variants = BTreeSet::new();
    let mut generated = BTreeSet::new();
    for line in include_str!("../inv_008_stock_epoch_routes.tsv")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.starts_with("variant\t"))
    {
        let columns: Vec<_> = line.split('\t').collect();
        assert_eq!(columns.len(), 5);
        assert!(variants.contains(columns[0]));
        let dispatch = source
            .split_once(&format!("Instruction::{} ", columns[0]))
            .unwrap()
            .1
            .split("\n            Instruction::")
            .next()
            .unwrap();
        assert!(
            dispatch.contains(&format!("{}(", columns[1])),
            "route/handler binding"
        );
        assert!(listed_variants.insert(columns[0]));
        assert!(listed_handlers.insert(columns[1]));
        assert!(!columns[4].is_empty());
        let (path, function) = columns[3].split_once('#').unwrap();
        assert!(
            path.starts_with("cu/inv_") || path.starts_with("stateful/inv_"),
            "stock-epoch owner points outside invariant tests: {path}"
        );
        assert!(
            path.ends_with(".rs") && !path.contains(".."),
            "stock-epoch owner path is not a direct Rust invariant file: {path}"
        );
        assert!(
            function.starts_with("v16_"),
            "stock-epoch owner uses an unreviewed witness name: {function}"
        );
        let owner = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/invariants")
                .join(path),
        )
        .unwrap();
        assert!(
            inv008_source_defines_test(&owner, function),
            "stock-epoch owner {path}#{function} is missing or is not a #[test]"
        );
        match columns[2] {
            "generated-live" => {
                generated.insert(columns[0]);
            }
            "adjacent-only" => {}
            _ => panic!("unknown stock-epoch coverage disposition"),
        }
    }
    assert_eq!(
        listed_handlers, handlers,
        "every direct signed SPL outflow needs a disposition"
    );
    assert_eq!(
        listed_variants, effects,
        "INV-024 stock/terminal/custody outflows agree"
    );
    assert_eq!(
        generated,
        [Class::Capital, Class::Insurance]
            .map(Class::variant)
            .into_iter()
            .collect()
    );
    assert_eq!(handlers.len(), 8);
}
