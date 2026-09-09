//! INV-067/071/073/078: actual insurance consumption preserves bounded unsigned user exits.
//! Public System/SPL/ATA/wrapper genesis, resolved bankruptcy, and isolated reserve custody.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

const CAPITAL: [u128; 3] = [1_000, 100, 137];
const GAIN: u128 = 10 * (130 - 100);
const DEFICIT: u128 = GAIN - CAPITAL[1];
// The losing principal becomes senior source backing. Quantize its credit rate before
// rounding the converted capital; the remaining source atom never enters the junior pool.
const SOURCE_RATE: u128 = CAPITAL[1] * percolator::CREDIT_RATE_SCALE / GAIN;
const CONVERTED: u128 = GAIN * SOURCE_RATE / percolator::CREDIT_RATE_SCALE;
const RECEIPT_FACE: u128 = GAIN - CONVERTED;
const JUNIOR_PAID: u128 = RECEIPT_FACE * DEFICIT / RECEIPT_FACE;
const BACKING_REMAINDER: u128 = CAPITAL[1] - CONVERTED;
const FOREIGN_INSURANCE: u128 = 113;
const RESOLVE_SLOT: u64 = 40;
const EXIT_SLOT: u64 = RESOLVE_SLOT + 3;
const CALL_BOUND: usize = 8;

struct World {
    env: V16CuEnv,
    owners: [Keypair; 3],
    portfolios: [Pubkey; 3],
    tokens: [Pubkey; 3],
    insurer_token: Pubkey,
    asset: usize,
    insurance: u128,
    peak_cu: u64,
    calls: [usize; 3],
    refusals: usize,
}

impl World {
    fn new(asset: usize, insurance: u128) -> Self {
        let mut env = inv018_public_spl_market_with_params(
            0,
            V16CuMarketParams {
                max_portfolio_assets: 2,
                maintenance_margin_bps: 1_000,
                initial_margin_bps: 1_000,
                max_price_move_bps_per_slot: 500,
                ..V16CuMarketParams::default()
            },
        );
        env.svm.warp_to_slot(1);
        for index in [0, 1] {
            env.configure_auth_mark_for_asset_as_admin(index, 1, 100);
        }
        env.configure_permissionless_resolve_with_cu(20, 3);
        let owners: [Keypair; 3] = std::array::from_fn(|_| Keypair::new());
        let portfolios = owners.each_ref().map(|owner| {
            env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
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
            env.portfolios.push(key.pubkey());
            key.pubkey()
        });
        let tokens = owners
            .each_ref()
            .map(|owner| create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint));
        let insurer_token =
            create_ata_for_test(&mut env.svm, &env.payer, env.admin.pubkey(), env.mint);
        for (token, amount) in tokens
            .into_iter()
            .zip(CAPITAL)
            .chain([(insurer_token, insurance + FOREIGN_INSURANCE)])
        {
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &token,
                    &env.admin.pubkey(),
                    &[],
                    u64::try_from(amount).unwrap(),
                )
                .unwrap(),
                &[&env.admin],
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
                &env.admin.pubkey(),
                &[],
            )
            .unwrap(),
            &[&env.admin],
        )
        .unwrap();
        for actor in 0..3 {
            env.send(
                env.deposit_ix(portfolios[actor], CAPITAL[actor]),
                vec![
                    AccountMeta::new(owners[actor].pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolios[actor], false),
                    AccountMeta::new(tokens[actor], false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&owners[actor]],
            )
            .unwrap();
        }
        for (domain, amount) in [(2 * asset, insurance), (2 * (1 - asset), FOREIGN_INSURANCE)] {
            let admin = env.admin.insecure_clone();
            env.send(
                ProgInstruction::TopUpInsuranceDomain {
                    domain: domain as u16,
                    market_id: env.asset_market_id((domain / 2) as u16),
                    authority_epoch: 0,
                    intent_id: 0,
                    amount,
                },
                vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(insurer_token, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[&admin],
            )
            .unwrap();
        }
        env.trade_asset_with_cu(
            asset as u16,
            &owners[0],
            portfolios[0],
            &owners[1],
            portfolios[1],
            (10 * POS_SCALE) as i128,
            100,
            0,
        );
        for (offset, mark) in (105..=130).step_by(5).enumerate() {
            let slot = offset as u64 + 2;
            env.svm.warp_to_slot(slot);
            env.push_auth_mark_for_asset_as_admin(asset as u16, slot, mark);
            env.crank(
                portfolios[2],
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(asset as u16),
                },
            );
        }
        for actor in [0, 1] {
            env.crank(
                portfolios[actor],
                ProgInstruction::PermissionlessCrank {
                    now_slot: 7,
                    observations: crank_observations(asset as u16),
                },
            );
        }
        assert_eq!(env.market_state().1.assets[asset].effective_price, 130);
        assert_eq!(env.portfolio_state(portfolios[0]).pnl.get(), GAIN as i128);
        assert_eq!(env.portfolio_state(portfolios[1]).capital.get(), 0);
        assert_eq!(
            env.portfolio_state(portfolios[1]).pnl.get(),
            -(DEFICIT as i128)
        );
        assert_eq!(env.market_state().1.insurance_domain_spent, vec![0; 4]);
        let world = Self {
            env,
            owners,
            portfolios,
            tokens,
            insurer_token,
            asset,
            insurance,
            peak_cu: 0,
            calls: [0; 3],
            refusals: 0,
        };
        world.custody();
        world
    }

    fn frame(&self) -> Vec<(Pubkey, Option<Account>)> {
        [
            self.env.market,
            self.env.vault,
            self.env.mint,
            self.env.vault_authority,
            self.env.admin.pubkey(),
            self.insurer_token,
            solana_sdk::sysvar::clock::id(),
        ]
        .into_iter()
        .chain(self.portfolios)
        .chain(self.tokens)
        .chain(self.owners.iter().map(Signer::pubkey))
        .map(|key| (key, self.env.svm.get_account(&key)))
        .collect()
    }

    fn custody(&self) {
        let group = self.env.market_state().1;
        let supply = CAPITAL.iter().sum::<u128>() + self.insurance + FOREIGN_INSURANCE;
        let paid: u128 = self
            .tokens
            .iter()
            .map(|&key| u128::from(self.env.token_amount(key)))
            .sum();
        assert_eq!(
            group.vault,
            u128::from(self.env.token_amount(self.env.vault))
        );
        assert_eq!(group.vault + paid, supply);
        assert_eq!(self.env.token_amount(self.insurer_token), 0);
        assert_eq!(
            group.c_tot,
            self.portfolios
                .iter()
                .map(|&key| self.env.portfolio_state(key).capital.get())
                .sum()
        );
        assert_eq!(
            group.insurance,
            group
                .insurance_domain_budget
                .iter()
                .zip(&group.insurance_domain_spent)
                .map(|(budget, spent)| budget.checked_sub(*spent).unwrap())
                .sum()
        );
        assert_eq!(
            group.insurance_domain_budget[2 * self.asset],
            self.insurance
        );
        assert_eq!(
            group.insurance_domain_budget[2 * (1 - self.asset)],
            FOREIGN_INSURANCE
        );
        assert_eq!(group.insurance_domain_spent[2 * (1 - self.asset)], 0);
        let mint = Mint::unpack(&self.env.svm.get_account(&self.env.mint).unwrap().data).unwrap();
        assert_eq!(u128::from(mint.supply), supply);
        assert_eq!(mint.mint_authority, COption::None);
    }

    fn payout(&self, actor: usize, crank: bool) -> Instruction {
        Instruction {
            program_id: self.env.program_id,
            accounts: vec![
                AccountMeta::new_readonly(self.owners[actor].pubkey(), false),
                AccountMeta::new(self.env.market, false),
                AccountMeta::new(self.portfolios[actor], false),
                AccountMeta::new(self.tokens[actor], false),
                AccountMeta::new(self.env.vault, false),
                AccountMeta::new_readonly(self.env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: if crank {
                ProgInstruction::PermissionlessCrank {
                    now_slot: 0,
                    observations: vec![],
                }
            } else {
                ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                }
            }
            .encode(),
        }
    }

    fn transaction(&mut self, instruction: Instruction) -> Transaction {
        self.env.svm.expire_blockhash();
        let tx = Transaction::new_signed_with_payer(
            &[
                heap_ix(),
                ComputeBudgetInstruction::set_compute_unit_limit(CRANK_CU_LIMIT as u32),
                instruction,
            ],
            Some(&self.env.payer.pubkey()),
            &[&self.env.payer],
            self.env.svm.latest_blockhash(),
        );
        assert_eq!(tx.message.header.num_required_signatures, 1);
        assert_eq!(tx.message.account_keys[0], self.env.payer.pubkey());
        assert!(self
            .owners
            .iter()
            .all(|owner| owner.pubkey() != self.env.payer.pubkey()));
        assert_ne!(self.env.admin.pubkey(), self.env.payer.pubkey());
        tx.verify().unwrap();
        assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
        tx
    }

    fn reject(&mut self, instruction: Instruction, error: PercolatorError) {
        let before = self.frame();
        let tx = self.transaction(instruction);
        let failure = self
            .env
            .svm
            .send_transaction(tx)
            .expect_err("exact terminal refusal");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(2, InstructionError::Custom(error as u32))
        );
        assert_eq!(
            self.frame(),
            before,
            "all economic Accounts roll back, excluding only the network fee payer"
        );
        self.refusals += 1;
        self.custody();
    }

    // At fixed resolved time, liabilities precede exposure, source cleanup, and cash/receipt work.
    fn rank(&self, actor: usize) -> (u128, usize, usize, u128, u128, bool) {
        let account = self.env.portfolio_state(self.portfolios[actor]);
        (
            account.pnl.get().min(0).unsigned_abs(),
            account.legs.iter().filter(|leg| leg.active != 0).count(),
            account
                .source_domains
                .iter()
                .filter(|source| source.is_occupied())
                .count(),
            account.capital.get(),
            account.pnl.get().max(0) as u128,
            !resolved_portfolio_is_terminal(&self.env, self.portfolios[actor]),
        )
    }

    fn advance(&mut self, actor: usize, crank: bool) {
        assert!(
            self.calls[actor] < CALL_BOUND,
            "keeper call bound for actor {actor}"
        );
        let rank = self.rank(actor);
        let before = self.frame();
        let tx = self.transaction(self.payout(actor, crank));
        let meta = self
            .env
            .svm
            .send_transaction(tx)
            .expect("bounded keeper continuation");
        self.peak_cu = self.peak_cu.max(meta.compute_units_consumed);
        assert_cu_within(
            "spent insurance terminal continuation",
            meta.compute_units_consumed,
            CRANK_CU_LIMIT,
        );
        assert!(
            self.rank(actor) < rank,
            "actor {actor}: {rank:?} -> {:?}",
            self.rank(actor)
        );
        for (key, account) in before {
            if ![
                self.env.market,
                self.env.vault,
                self.portfolios[actor],
                self.tokens[actor],
            ]
            .contains(&key)
            {
                assert_eq!(
                    self.env.svm.get_account(&key),
                    account,
                    "unrelated Account {key}"
                );
            }
        }
        self.calls[actor] += 1;
        assert!(self.calls[actor] <= CALL_BOUND);
        self.custody();
    }
}

#[test]
fn v16_program_spent_insurance_preserves_bounded_keeper_terminal_payouts() {
    assert_eq!(
        (CONVERTED, RECEIPT_FACE, JUNIOR_PAID, BACKING_REMAINDER),
        (99, 201, 200, 1)
    );
    let mut peak_cu = 0;
    let mut accepted = 0;
    let mut refused = 0;
    for asset in [0, 1] {
        for insurance in [DEFICIT, DEFICIT + 51] {
            for winner_first in [false, true] {
                let mut world = World::new(asset, insurance);
                world.env.svm.warp_to_slot(RESOLVE_SLOT);
                let before = world.frame();
                let tx = world.transaction(Instruction {
                    program_id: world.env.program_id,
                    accounts: vec![AccountMeta::new(world.env.market, false)],
                    data: ProgInstruction::ResolveStalePermissionless { now_slot: 0 }.encode(),
                });
                world
                    .env
                    .svm
                    .send_transaction(tx)
                    .expect("permissionless stale resolution");
                assert_eq!(world.env.market_state().1.mode, MarketModeV16::Resolved);
                assert_eq!(world.env.market_state().1.resolved_slot, RESOLVE_SLOT);
                for (key, account) in before {
                    if key != world.env.market {
                        assert_eq!(world.env.svm.get_account(&key), account);
                    }
                }
                world.env.svm.warp_to_slot(EXIT_SLOT - 1);
                for crank in [false, true] {
                    world.reject(world.payout(0, crank), PercolatorError::ExpectedSigner);
                }
                world.env.svm.warp_to_slot(EXIT_SLOT);

                // Settle the insolvent account publicly. Its deficit consumes only its paired
                // insurance domain, before either the profitable or idle owner seeks payout.
                while !resolved_portfolio_is_terminal(&world.env, world.portfolios[1]) {
                    world.advance(1, world.calls[1] % 2 == 0);
                }
                let spent = world.env.market_state().1;
                assert_eq!(spent.insurance_domain_spent[2 * asset], DEFICIT);
                assert_eq!(spent.insurance, insurance - DEFICIT + FOREIGN_INSURANCE);
                assert_eq!(world.env.token_amount(world.tokens[1]), 0);
                assert_eq!(spent.assets[asset].b_long_num, 0);
                assert_eq!(spent.assets[asset].b_short_num, 0);
                assert_eq!(spent.mode, MarketModeV16::Resolved);
                let source_domain = 2 * asset + 1;
                assert_eq!(
                    spent.source_credit[source_domain].credit_rate_num,
                    SOURCE_RATE
                );
                assert_eq!(
                    spent.source_credit[source_domain].positive_claim_bound_num,
                    GAIN * BOUND_SCALE
                );
                assert_eq!(
                    spent.source_backing_buckets[source_domain].fresh_unliened_backing_num,
                    CAPITAL[1] * BOUND_SCALE
                );

                for actor in if winner_first { [0, 2] } else { [2, 0] } {
                    let mut payout_rollback = false;
                    while !resolved_portfolio_is_terminal(&world.env, world.portfolios[actor]) {
                        let crank = world.calls[actor] % 2 == usize::from(winner_first);
                        let instruction = world.payout(actor, crank);
                        let tx = world.transaction(instruction.clone());
                        let before = world.frame();
                        let simulation = world.env.svm.simulate_transaction(tx.into()).unwrap();
                        assert_eq!(world.frame(), before);
                        if simulation
                            .logs
                            .iter()
                            .any(|line| *line == format!("Program {} success", spl_token::ID))
                        {
                            assert!(!payout_rollback, "each owner has one input-derived payout");
                            // This is a valid funded same-mint ATA belonging to the reserve owner.
                            // The wrapper must roll back its already computed payout/receipt.
                            for alias in [false, true] {
                                let mut wrong_destination = world.payout(actor, alias);
                                wrong_destination.accounts[3].pubkey = world.insurer_token;
                                world.reject(
                                    wrong_destination,
                                    PercolatorError::InvalidTokenAccount,
                                );
                            }
                            payout_rollback = true;
                        }
                        world.advance(actor, crank);
                    }
                    assert!(payout_rollback);
                    assert_eq!(
                        u128::from(world.env.token_amount(world.tokens[actor])),
                        CAPITAL[actor]
                            + if actor == 0 {
                                CONVERTED + JUNIOR_PAID
                            } else {
                                0
                            },
                    );
                }

                for actor in 0..3 {
                    for crank in [false, true] {
                        world.reject(
                            world.payout(actor, crank),
                            PercolatorError::EngineNonProgress,
                        );
                    }
                }
                let group = world.env.market_state().1;
                assert_eq!(group.c_tot, 0);
                assert_eq!(group.source_claim_bound_total_num, 0);
                assert_eq!(group.insurance_domain_spent[2 * asset], DEFICIT);
                assert_eq!(
                    group.vault,
                    insurance - DEFICIT + FOREIGN_INSURANCE + BACKING_REMAINDER
                );
                assert_eq!(group.vault, group.insurance + BACKING_REMAINDER);
                let source = group.source_credit[source_domain];
                assert_eq!(source.provider_receivable_num, CONVERTED * BOUND_SCALE);
                assert_eq!(source.spent_backing_num, CONVERTED * BOUND_SCALE);
                assert_eq!(
                    source.fresh_reserved_backing_num,
                    BACKING_REMAINDER * BOUND_SCALE
                );
                assert_eq!(
                    group.source_backing_buckets[source_domain].fresh_unliened_backing_num,
                    BACKING_REMAINDER * BOUND_SCALE
                );
                let ledger = group.resolved_payout_ledger;
                assert_eq!(ledger.snapshot_slot, EXIT_SLOT);
                assert_eq!(ledger.snapshot_residual, DEFICIT);
                assert_eq!(
                    ledger.terminal_claim_exact_receipts_num,
                    RECEIPT_FACE * BOUND_SCALE
                );
                assert_eq!(ledger.terminal_claim_bound_unreceipted_num, 0);
                assert_eq!(ledger.current_payout_rate_num, DEFICIT * BOUND_SCALE);
                assert_eq!(ledger.current_payout_rate_den, RECEIPT_FACE * BOUND_SCALE);
                assert!(!ledger.payout_halted);
                assert_eq!(
                    resolved_receipt(&world.env.portfolio_state(world.portfolios[0])),
                    ResolvedPayoutReceiptV16::EMPTY
                );
                assert_eq!(group.materialized_portfolio_count, 3);
                assert!(group
                    .assets
                    .iter()
                    .all(|asset| asset.oi_eff_long_q == 0 && asset.oi_eff_short_q == 0));
                world.custody();
                peak_cu = peak_cu.max(world.peak_cu);
                accepted += world.calls.iter().sum::<usize>();
                refused += world.refusals;
                println!("spent insurance asset={asset} funded={insurance} winner_first={winner_first}: calls={:?}, paid=[1299,0,137], insurance={}, backing={BACKING_REMAINDER}, peak={} CU", world.calls, group.insurance, world.peak_cu);
            }
        }
    }
    println!("INV-067/071/073/078 spent insurance: 8 worlds, {accepted} continuations, {refused} exact rollbacks, peak {peak_cu} CU");
}
