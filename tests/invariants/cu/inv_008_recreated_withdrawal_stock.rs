//! INV-008 / row415: a paid withdrawal cannot acquire a replacement incarnation's stock.
//! Public close/refund/init/deposit cycles restore the same owner, address, sequence and amount.
//! A stale suffix must undo the entire cycle, even when it also follows the first SPL payout.
//! Separate per-incarnation books distinguish new principal from custody-only donations.
//! This finite live-portfolio product does not certify insurance withdrawal stock binding.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const PEER: u64 = 103;

#[derive(Clone, Copy)]
enum Step {
    Pay(u64),
    Close,
    Refund,
    Init,
    Deposit(u64),
    Donate(u64),
}

struct Episode {
    id: u64,
    deposited: u64,
    paid: u64,
    sequence: u64,
}

struct Consent {
    instruction: Instruction,
    envelopes: Vec<Transaction>,
}

#[derive(Default)]
struct Evidence {
    transactions: usize,
    rejections: usize,
    payout_cycle_rollbacks: usize,
    stock_cycle_rollbacks: usize,
    peak_cu: u64,
}

struct World {
    env: V16CuEnv,
    owner: Keypair,
    portfolio: Pubkey,
    token: Pubkey,
    peer: Pubkey,
    peer_token: Pubkey,
    peer_frame: Account,
    mint_frame: Account,
    controls: state::AssetControlSequencesV16,
    frame: Vec<Pubkey>,
    endowment: u64,
    portfolio_rent: u64,
    market_lamports: u64,
    owner_lamports: u64,
    episodes: Vec<Episode>,
    active: bool,
    funded: bool,
    closes: u64,
    refunds: u64,
    donated: u64,
    nonce: u32,
    signatures: BTreeSet<solana_sdk::signature::Signature>,
    evidence: Evidence,
}

impl World {
    fn new(amount: u64, cycles: u64) -> Self {
        let mut env = inv018_public_spl_market(6);
        let owners = [Keypair::new(), Keypair::new()];
        let endowment = (cycles + 1) * amount + cycles * (amount + 1);
        let mut portfolios = [Pubkey::default(); 2];
        let mut tokens = [Pubkey::default(); 2];
        for actor in 0..2 {
            env.svm
                .airdrop(&owners[actor].pubkey(), (cycles + 1) * 1_000_000_000)
                .unwrap();
            let key = Keypair::new();
            system_create_account_for_test(
                &mut env.svm,
                &env.payer,
                &key,
                env.portfolio_account_len,
                env.program_id,
            );
            portfolios[actor] = key.pubkey();
            env.send(
                ProgInstruction::InitPortfolio,
                vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(key.pubkey(), false),
                ],
                &[&owners[actor]],
            )
            .unwrap();
            assert_eq!(env.portfolio_id(key.pubkey()), actor as u64 + 1);
            assert_eq!(env.portfolio_state(key.pubkey()).capital.get(), 0);
            tokens[actor] =
                create_ata_for_test(&mut env.svm, &env.payer, owners[actor].pubkey(), env.mint);
            let supply = if actor == 0 { endowment } else { PEER };
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &tokens[actor],
                    &env.admin.pubkey(),
                    &[],
                    supply,
                )
                .unwrap(),
                &[&env.admin],
            )
            .unwrap();
            assert_eq!(env.token_amount(tokens[actor]), supply);
            let principal = if actor == 0 { amount } else { PEER };
            env.send(
                env.deposit_ix(key.pubkey(), principal.into()),
                vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(key.pubkey(), false),
                    AccountMeta::new(tokens[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[actor]],
            )
            .unwrap();
            assert_eq!(
                env.portfolio_state(key.pubkey()).capital.get(),
                principal.into()
            );
            assert_eq!(env.token_amount(tokens[actor]), supply - principal);
            let total = amount + if actor == 1 { PEER } else { 0 };
            assert_eq!(env.token_amount(env.vault), total);
            assert_eq!(
                (env.market_state().1.c_tot, env.market_state().1.vault),
                (total.into(), total.into())
            );
        }
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::set_authority(
                &spl_token::ID,
                &env.mint,
                None,
                spl_token::instruction::AuthorityType::MintTokens,
                &env.admin.pubkey(),
                &[],
            )
            .unwrap(),
            &[&env.admin],
        )
        .unwrap();
        let owner = owners[0].insecure_clone();
        let world = Self {
            peer_frame: env.svm.get_account(&portfolios[1]).unwrap(),
            mint_frame: env.svm.get_account(&env.mint).unwrap(),
            controls: env.control_sequences(0),
            portfolio_rent: env.svm.get_account(&portfolios[0]).unwrap().lamports,
            market_lamports: env.svm.get_account(&env.market).unwrap().lamports,
            owner_lamports: env.svm.get_account(&owner.pubkey()).unwrap().lamports,
            frame: vec![
                env.market,
                env.vault,
                env.mint,
                env.vault_authority,
                env.admin.pubkey(),
                owner.pubkey(),
                owners[1].pubkey(),
                portfolios[0],
                portfolios[1],
                tokens[0],
                tokens[1],
            ],
            env,
            owner,
            portfolio: portfolios[0],
            token: tokens[0],
            peer: portfolios[1],
            peer_token: tokens[1],
            endowment,
            episodes: vec![Episode {
                id: 1,
                deposited: amount,
                paid: 0,
                sequence: 1,
            }],
            active: true,
            funded: true,
            closes: 0,
            refunds: 0,
            donated: 0,
            nonce: 0,
            signatures: BTreeSet::new(),
            evidence: Evidence::default(),
        };
        world.check();
        world
    }

    fn check(&self) {
        let episode = self.episodes.last().unwrap();
        let deposited: u64 = self.episodes.iter().map(|entry| entry.deposited).sum();
        let paid: u64 = self.episodes.iter().map(|entry| entry.paid).sum();
        for (index, entry) in self.episodes.iter().enumerate() {
            assert!(
                entry.paid <= entry.deposited,
                "incarnation {} overspent its own stock",
                entry.id
            );
            if index + 1 < self.episodes.len() || !self.active {
                assert_eq!(
                    entry.paid, entry.deposited,
                    "closed incarnation retains no claim"
                );
            }
        }
        let capital = deposited - paid;
        let group = self.env.market_state().1;
        assert_eq!(group.mode, MarketModeV16::Live);
        assert_eq!(
            (group.c_tot, group.vault),
            ((PEER + capital).into(), (PEER + capital).into())
        );
        assert_eq!(
            group.materialized_portfolio_count,
            1 + u64::from(self.active)
        );
        assert_eq!(
            (
                group.insurance,
                group.pnl_pos_tot,
                group.backing_provider_earnings_total
            ),
            (0, 0, 0)
        );
        assert_eq!(group.source_claim_bound_total_num, 0);
        assert_eq!(group.source_insurance_credit_reserved_total_atoms, 0);
        assert_eq!(self.env.control_sequences(0), self.controls);
        assert_eq!(
            self.env.market_state().0.next_portfolio_id,
            self.episodes.len() as u64 + 2
        );
        assert_eq!(
            self.env.token_amount(self.token),
            self.endowment - deposited + paid - self.donated
        );
        assert_eq!(
            self.env.token_amount(self.env.vault),
            PEER + capital + self.donated
        );
        assert_eq!(self.env.token_amount(self.peer_token), 0);
        assert_eq!(
            self.env.svm.get_account(&self.peer).unwrap(),
            self.peer_frame
        );
        assert_eq!(
            self.env.svm.get_account(&self.env.mint).unwrap(),
            self.mint_frame
        );
        let mint = Mint::unpack(&self.mint_frame.data).unwrap();
        assert_eq!(mint.mint_authority, COption::None);
        assert_eq!(mint.supply, self.endowment + PEER);
        assert_eq!(
            self.env.token_amount(self.token) + self.env.token_amount(self.env.vault),
            mint.supply
        );
        assert_eq!(
            self.env.svm.get_account(&self.env.market).unwrap().lamports,
            self.market_lamports + self.closes * self.portfolio_rent
        );
        assert_eq!(
            self.env
                .svm
                .get_account(&self.owner.pubkey())
                .unwrap()
                .lamports,
            self.owner_lamports - self.refunds * self.portfolio_rent
        );
        let account = self.env.svm.get_account(&self.portfolio);
        assert_eq!(
            account.as_ref().map_or(0, |account| account.lamports),
            if self.funded { self.portfolio_rent } else { 0 }
        );
        if self.active {
            assert_eq!(self.env.portfolio_id(self.portfolio), episode.id);
            assert_eq!(
                self.env.portfolio_matcher_sequence(self.portfolio),
                episode.sequence
            );
            assert_eq!(self.env.portfolio_position_epoch(self.portfolio), 0);
            let portfolio = self.env.portfolio_state(self.portfolio);
            assert_eq!(portfolio.owner, self.owner.pubkey().to_bytes());
            assert_eq!(portfolio.capital.get(), capital.into());
            assert_eq!(portfolio.pnl.get(), 0);
        } else if let Some(account) = account {
            assert!(account.data.iter().all(|byte| *byte == 0));
        }
    }

    fn apply(&mut self, step: Step) {
        match step {
            Step::Pay(amount) => {
                let episode = self.episodes.last_mut().unwrap();
                assert!(self.active && amount <= episode.deposited - episode.paid);
                episode.paid += amount;
                episode.sequence += 1;
            }
            Step::Close => {
                let episode = self.episodes.last().unwrap();
                assert!(self.active && episode.paid == episode.deposited);
                self.active = false;
                self.funded = false;
                self.closes += 1;
            }
            Step::Refund => {
                assert!(!self.active && !self.funded);
                self.funded = true;
                self.refunds += 1;
            }
            Step::Init => {
                assert!(!self.active && self.funded);
                self.episodes.push(Episode {
                    id: self.episodes.len() as u64 + 2,
                    deposited: 0,
                    paid: 0,
                    sequence: 0,
                });
                self.active = true;
            }
            Step::Deposit(amount) => {
                assert!(self.active);
                let episode = self.episodes.last_mut().unwrap();
                episode.deposited += amount;
                episode.sequence += 1;
            }
            Step::Donate(amount) => self.donated += amount,
        }
    }

    fn withdrawal(&self, amount: u64) -> Instruction {
        let episode = self.episodes.last().unwrap();
        Instruction {
            program_id: self.env.program_id,
            accounts: vec![
                AccountMeta::new(self.owner.pubkey(), true),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.portfolio, false),
                AccountMeta::new(self.token, false),
                AccountMeta::new(self.env.vault, false),
                AccountMeta::new_readonly(self.env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: ProgInstruction::Withdraw {
                portfolio_id: episode.id,
                expected_sequence: episode.sequence,
                amount: amount.into(),
            }
            .encode(),
        }
    }

    fn signed(&mut self, instructions: &[Instruction]) -> Transaction {
        self.nonce += 1;
        let mut message = vec![
            heap_ix(),
            ComputeBudgetInstruction::set_compute_unit_limit(1_400_000 - self.nonce),
        ];
        message.extend_from_slice(instructions);
        let tx = Transaction::new_signed_with_payer(
            &message,
            Some(&self.env.payer.pubkey()),
            &[&self.env.payer, &self.owner],
            self.env.svm.latest_blockhash(),
        );
        tx.verify().expect("signature-distinct retained envelope");
        assert!(self.signatures.insert(tx.signatures[0]));
        tx
    }

    fn retain(&mut self, amount: u64, retries: usize) -> Consent {
        let instruction = self.withdrawal(amount);
        let envelopes = (0..retries)
            .map(|_| self.signed(&[instruction.clone()]))
            .collect();
        Consent {
            instruction,
            envelopes,
        }
    }

    // The error tuple names the application index and completed wrapper/SPL/System prefixes.
    fn send(
        &mut self,
        tx: Transaction,
        steps: &[Step],
        error: Option<(u8, PercolatorError, [usize; 3])>,
    ) {
        let keys: BTreeSet<_> = self
            .frame
            .iter()
            .chain(&tx.message.account_keys)
            .copied()
            .collect();
        let before: Vec<_> = keys
            .iter()
            .map(|key| (*key, self.env.svm.get_account(key)))
            .collect();
        let mut payer = self.env.svm.get_account(&self.env.payer.pubkey()).unwrap();
        payer.lamports -= FeeStructure::default().lamports_per_signature
            * u64::from(tx.message.header.num_required_signatures);
        let result = self.env.svm.send_transaction(tx);
        let meta = if let Some((index, error, successes)) = error {
            let failure =
                result.expect_err("retained withdrawal must not consume replacement stock");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(
                    2 + index,
                    InstructionError::Custom(error as u32)
                ),
                "rejected transaction logs: {:?}",
                failure.meta.logs
            );
            for (program, count) in [
                self.env.program_id,
                spl_token::ID,
                solana_sdk::system_program::ID,
            ]
            .into_iter()
            .zip(successes)
            {
                assert_eq!(
                    failure
                        .meta
                        .logs
                        .iter()
                        .filter(|line| **line == format!("Program {program} success"))
                        .count(),
                    count,
                    "intended prefix must complete before rejection: {:?}",
                    failure.meta.logs
                );
            }
            for (key, account) in before {
                if key != self.env.payer.pubkey() {
                    assert_eq!(
                        self.env.svm.get_account(&key),
                        account,
                        "complete rollback at {key}"
                    );
                }
            }
            self.evidence.rejections += 1;
            failure.meta
        } else {
            let meta = result.expect("current consent and lifecycle must make public progress");
            for step in steps {
                self.apply(*step);
            }
            meta
        };
        assert_eq!(
            self.env.svm.get_account(&self.env.payer.pubkey()).unwrap(),
            payer
        );
        assert!(meta.compute_units_consumed > 0);
        assert_cu_within(
            "INV-008 recreated withdrawal stock",
            meta.compute_units_consumed,
            CUSTODY_CU_LIMIT,
        );
        self.evidence.peak_cu = self.evidence.peak_cu.max(meta.compute_units_consumed);
        self.evidence.transactions += 1;
        self.check();
    }

    fn replacement(&self, amount: u64, donation_first: bool) -> Vec<(Instruction, Step)> {
        let episode = self.episodes.last().unwrap();
        let roles = vec![
            AccountMeta::new(self.owner.pubkey(), true),
            AccountMeta::new(self.env.market, false),
            AccountMeta::new(self.portfolio, false),
        ];
        let wrap = |data: ProgInstruction, accounts| Instruction {
            program_id: self.env.program_id,
            accounts,
            data: data.encode(),
        };
        let mut deposit_roles = roles.clone();
        deposit_roles.extend([
            AccountMeta::new(self.token, false),
            AccountMeta::new(self.env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ]);
        let mut steps = vec![
            (
                wrap(
                    ProgInstruction::ClosePortfolio {
                        portfolio_id: episode.id,
                        expected_sequence: 2,
                        position_epoch: 0,
                    },
                    roles.clone(),
                ),
                Step::Close,
            ),
            (
                system_instruction::transfer(
                    &self.owner.pubkey(),
                    &self.portfolio,
                    self.portfolio_rent,
                ),
                Step::Refund,
            ),
            (wrap(ProgInstruction::InitPortfolio, roles), Step::Init),
            (
                wrap(
                    ProgInstruction::Deposit {
                        portfolio_id: self.episodes.len() as u64 + 2,
                        expected_sequence: 0,
                        amount: amount.into(),
                    },
                    deposit_roles,
                ),
                Step::Deposit(amount),
            ),
        ];
        let donation = (
            spl_token::instruction::transfer(
                &spl_token::ID,
                &self.token,
                &self.env.vault,
                &self.owner.pubkey(),
                &[],
                amount + 1,
            )
            .unwrap(),
            Step::Donate(amount + 1),
        );
        steps.insert(if donation_first { 0 } else { steps.len() }, donation);
        steps
    }
}

#[test]
fn v16_paid_withdrawal_cannot_acquire_recreated_portfolio_stock_across_atomic_retries() {
    let mut totals = Evidence::default();
    let mut worlds = 0;
    for amount in [1, 37] {
        for cycles in [1, 3] {
            for bundled in [false, true] {
                for donation_first in [false, true] {
                    let mut world = World::new(amount, cycles);
                    let mut consumed: Vec<Consent> = Vec::new();
                    for _ in 0..cycles {
                        let mut consent = world.retain(amount, cycles as usize + 2);
                        let replacement = world.replacement(amount, donation_first);
                        let prefix: Vec<_> = replacement.iter().map(|(ix, _)| ix.clone()).collect();

                        // The first payout, rent movement, ID allocation and replenishment all abort.
                        let mut aborted = vec![consent.instruction.clone()];
                        aborted.extend(prefix.clone());
                        aborted.push(consent.instruction.clone());
                        let tx = world.signed(&aborted);
                        world.send(
                            tx,
                            &[],
                            Some((6, PercolatorError::EngineProvenanceMismatch, [4, 3, 1])),
                        );
                        world.evidence.payout_cycle_rollbacks += 1;
                        world.send(consent.envelopes.pop().unwrap(), &[Step::Pay(amount)], None);

                        let mut aborted = prefix.clone();
                        aborted.push(consent.instruction.clone());
                        let tx = world.signed(&aborted);
                        world.send(
                            tx,
                            &[],
                            Some((5, PercolatorError::EngineProvenanceMismatch, [3, 2, 1])),
                        );
                        world.evidence.stock_cycle_rollbacks += 1;
                        if bundled {
                            let tx = world.signed(&prefix);
                            let steps: Vec<_> = replacement.iter().map(|(_, step)| *step).collect();
                            world.send(tx, &steps, None);
                        } else {
                            for (ix, step) in replacement {
                                let tx = world.signed(&[ix]);
                                world.send(tx, &[step], None);
                            }
                        }
                        consumed.push(consent);
                        assert_eq!(world.env.portfolio_matcher_sequence(world.portfolio), 1);
                        assert_eq!(
                            world.env.portfolio_state(world.portfolio).capital.get(),
                            amount.into()
                        );
                        let fresh = world.withdrawal(amount);
                        for old in &mut consumed {
                            // Only the assigned incarnation differs: owner, sequence, amount and rail match.
                            assert_eq!(old.instruction.accounts, fresh.accounts);
                            let ProgInstruction::Withdraw {
                                portfolio_id,
                                expected_sequence,
                                amount: signed_amount,
                            } = ProgInstruction::decode(&old.instruction.data).unwrap()
                            else {
                                panic!("retained consent must remain a withdrawal");
                            };
                            let current_id = world.episodes.last().unwrap().id;
                            assert_ne!(portfolio_id, current_id);
                            assert_eq!(
                                ProgInstruction::Withdraw {
                                    portfolio_id: current_id,
                                    expected_sequence,
                                    amount: signed_amount,
                                }
                                .encode(),
                                fresh.data
                            );
                            world.send(
                                old.envelopes.pop().unwrap(),
                                &[],
                                Some((0, PercolatorError::EngineProvenanceMismatch, [0, 0, 0])),
                            );
                        }
                    }
                    let mut last = world.retain(amount, 2);
                    let tx =
                        world.signed(&[last.instruction.clone(), consumed[0].instruction.clone()]);
                    world.send(
                        tx,
                        &[],
                        Some((1, PercolatorError::EngineProvenanceMismatch, [1, 1, 0])),
                    );
                    world.send(last.envelopes.pop().unwrap(), &[Step::Pay(amount)], None);
                    consumed.push(last);
                    let current = consumed.len() - 1;
                    for (index, old) in consumed.iter_mut().enumerate() {
                        let error = if index == current {
                            PercolatorError::EngineStale
                        } else {
                            PercolatorError::EngineProvenanceMismatch
                        };
                        world.send(
                            old.envelopes.pop().unwrap(),
                            &[],
                            Some((0, error, [0, 0, 0])),
                        );
                    }
                    assert_eq!(
                        world
                            .episodes
                            .iter()
                            .map(|episode| episode.paid)
                            .sum::<u64>(),
                        (cycles + 1) * amount
                    );
                    assert_eq!(
                        world.env.token_amount(world.env.vault),
                        PEER + cycles * (amount + 1)
                    );
                    totals.transactions += world.evidence.transactions;
                    totals.rejections += world.evidence.rejections;
                    totals.payout_cycle_rollbacks += world.evidence.payout_cycle_rollbacks;
                    totals.stock_cycle_rollbacks += world.evidence.stock_cycle_rollbacks;
                    totals.peak_cu = totals.peak_cu.max(world.evidence.peak_cu);
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!(worlds, 16);
    assert_eq!(totals.payout_cycle_rollbacks, 32);
    assert_eq!(totals.stock_cycle_rollbacks, 32);
    eprintln!("INV-008 recreated stock: {worlds} worlds, {} transactions, {} exact rollbacks, {} payout/cycle rollbacks, {} stock/cycle rollbacks, max CU={}",
        totals.transactions, totals.rejections, totals.payout_cycle_rollbacks, totals.stock_cycle_rollbacks, totals.peak_cu);
}
