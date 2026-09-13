//! INV-008/011/014/024/031/036/047/064/080/081: retained fee consent and
//! attributed insurance stock compose through transaction rollback and retries.
//! Insurance requests are retried only after rollback of their entire transaction.
//! Successful insurance-debit consumption and stock-epoch binding remain OPEN.

use crate::support::{
    fuzz_model::{assert_public_encumbrance_census, assert_public_stock_census, TradeRoute},
    v16_svm::{MarketConfig, V16Svm, PRIMARY_ACTOR_COUNT, TX_CU_LIMIT},
};
use percolator::POS_SCALE;
use percolator_prog::{
    error::PercolatorError,
    ix::{BatchTradeCpiLeg, BatchTradeLeg, Instruction as ProgInstruction},
    processor::{ASSET_AUTH_INSURANCE, ASSET_AUTH_INSURANCE_OPERATOR},
};
use solana_sdk::{
    account::Account,
    compute_budget::ComputeBudgetInstruction,
    fee::FeeStructure,
    instruction::{AccountMeta, Instruction, InstructionError},
    program_pack::Pack,
    pubkey::Pubkey,
    signature::{Keypair, SeedDerivable, Signer},
    transaction::{Transaction, TransactionError},
};
use spl_token::state::Account as TokenAccount;
use std::collections::BTreeSet;

const PRICE: u64 = 100_003;
const SIGNED_BPS: u64 = 37;
const LP_CAP: u16 = 503;
const INSURER: usize = 3;
const INITIAL_STOCK: [u128; 2] = [101, 211];
const FIRST_PAYOUT: u128 = 137;
const REFILL: u128 = 43;
const ROUTES: [TradeRoute; 4] = [
    TradeRoute::NoCpi,
    TradeRoute::BatchNoCpi,
    TradeRoute::Cpi,
    TradeRoute::BatchCpi,
];

fn cpi(route: TradeRoute) -> bool {
    matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi)
}

fn fee(size: i128, bps: u64) -> u128 {
    (size.unsigned_abs() * u128::from(PRICE))
        .div_ceil(POS_SCALE)
        .checked_mul(u128::from(bps))
        .unwrap()
        .div_ceil(10_000)
}

fn trade(env: &V16Svm, route: TradeRoute, size_q: i128) -> Instruction {
    let account_a_portfolio_id = env.primary_portfolio_id(0);
    let account_b_portfolio_id = env.primary_portfolio_id(1);
    let account_a_position_epoch = env.primary_portfolio_position_epoch(0);
    let account_b_position_epoch = env.primary_portfolio_position_epoch(1);
    let account_b_matcher_sequence = env.primary_portfolio_matcher_sequence(1);
    let market_id = env.primary_market_state().1.assets[0].market_id;
    let ix = match route {
        TradeRoute::NoCpi => ProgInstruction::TradeNoCpi {
            account_a_portfolio_id,
            account_a_position_epoch,
            account_b_portfolio_id,
            account_b_position_epoch,
            asset_index: 0,
            market_id,
            size_q,
            exec_price: PRICE,
            fee_bps: SIGNED_BPS,
            backing_fee_cap_bps: 0,
        },
        TradeRoute::BatchNoCpi => ProgInstruction::BatchTradeNoCpi {
            account_a_portfolio_id,
            account_a_position_epoch,
            account_b_portfolio_id,
            account_b_position_epoch,
            legs: vec![BatchTradeLeg {
                asset_index: 0,
                market_id,
                size_q,
                exec_price: PRICE,
                fee_bps: SIGNED_BPS,
            }],
        },
        TradeRoute::Cpi => ProgInstruction::TradeCpi {
            account_a_portfolio_id,
            account_a_position_epoch,
            account_b_portfolio_id,
            account_b_position_epoch,
            account_b_matcher_sequence,
            asset_index: 0,
            market_id,
            size_q,
            fee_bps: SIGNED_BPS,
            limit_price: PRICE,
            backing_fee_cap_bps: 0,
        },
        TradeRoute::BatchCpi => ProgInstruction::BatchTradeCpi {
            account_a_portfolio_id,
            account_a_position_epoch,
            account_b_portfolio_id,
            account_b_position_epoch,
            account_b_matcher_sequence,
            max_slippage_atoms: 0,
            max_fee_atoms: fee(size_q, SIGNED_BPS),
            legs: vec![BatchTradeCpiLeg {
                asset_index: 0,
                market_id,
                size_q,
                fee_bps: SIGNED_BPS,
                limit_price: PRICE,
            }],
        },
    };
    let mut accounts = vec![AccountMeta::new(env.actors[0].signer.pubkey(), true)];
    if !cpi(route) {
        accounts.push(AccountMeta::new(env.actors[1].signer.pubkey(), true));
    }
    accounts.extend([
        AccountMeta::new(env.market, false),
        AccountMeta::new(env.actors[0].portfolio, false),
        AccountMeta::new(env.actors[1].portfolio, false),
    ]);
    if cpi(route) {
        accounts.extend([
            AccountMeta::new_readonly(env.matcher_program, false),
            AccountMeta::new(env.actors[1].matcher_context, false),
            AccountMeta::new_readonly(env.actors[1].matcher_delegate, false),
        ]);
    }
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ix.encode(),
    }
}

fn insurance(env: &V16Svm, amount: u128) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(env.actors[INSURER].signer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.actors[INSURER].destination_token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::WithdrawInsuranceAsset {
            asset_index: 0,
            market_id: env.primary_market_state().1.assets[0].market_id,
            authority_epoch: env.primary_control_sequences(0).authority_epoch,
            amount,
        }
        .encode(),
    }
}

fn set_policy(
    env: &mut V16Svm,
    payer: &Keypair,
    bps: u64,
    nonce: &mut u32,
    evidence: &mut Evidence,
) {
    let sequences = env.primary_control_sequences(0);
    let ix = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(env.actors[INSURER].signer.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        data: ProgInstruction::UpdateTradeFeePolicy {
            trade_fee_base_bps: bps,
            policy_sequence: sequences.trade_fee + 1,
            authority_epoch: sequences.authority_epoch,
        }
        .encode(),
    };
    let tx = sign(env, payer, &[ix], nonce);
    deliver(env, tx, None, [1, 0, 0], evidence);
}

fn sign(env: &V16Svm, payer: &Keypair, ixs: &[Instruction], nonce: &mut u32) -> Transaction {
    *nonce += 1;
    let mut message = vec![
        ComputeBudgetInstruction::request_heap_frame(256 * 1024),
        ComputeBudgetInstruction::set_compute_unit_limit(TX_CU_LIMIT as u32 - *nonce),
    ];
    message.extend_from_slice(ixs);
    let mut signers = vec![payer];
    for actor in &env.actors {
        if ixs
            .iter()
            .flat_map(|ix| &ix.accounts)
            .any(|meta| meta.is_signer && meta.pubkey == actor.signer.pubkey())
        {
            signers.push(&actor.signer);
        }
    }
    let tx = Transaction::new_signed_with_payer(
        &message,
        Some(&payer.pubkey()),
        &signers,
        env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= solana_sdk::packet::PACKET_DATA_SIZE as u64);
    tx
}

fn frame(env: &V16Svm, tx: &Transaction) -> Vec<(Pubkey, Option<Account>)> {
    let keys: BTreeSet<_> = env
        .all_economic_account_lamports()
        .into_iter()
        .map(|(key, _)| key)
        .chain(tx.message.account_keys.iter().copied())
        .chain(env.actors.iter().map(|actor| actor.signer.pubkey()))
        .collect();
    keys.into_iter()
        .map(|key| (key, env.svm.get_account(&key)))
        .collect()
}

#[derive(Default)]
struct Evidence {
    worlds: usize,
    simulations: usize,
    failures: usize,
    transfers_rolled_back: usize,
    matcher_calls_rolled_back: usize,
    successes: usize,
    peak_cu: u64,
}

// Counts refer to successful wrapper, SPL and matcher calls, including rolled-back prefixes.
fn deliver(
    env: &mut V16Svm,
    tx: Transaction,
    failure_index: Option<u8>,
    counts: [usize; 3],
    evidence: &mut Evidence,
) {
    let before = frame(env, &tx);
    let payer = tx.message.account_keys[0];
    let mut expected_payer = env.svm.get_account(&payer).unwrap();
    expected_payer.lamports -= FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let writable: BTreeSet<_> = tx
        .message
        .account_keys
        .iter()
        .enumerate()
        .filter(|(i, _)| tx.message.is_writable(*i))
        .map(|(_, key)| *key)
        .collect();
    let result = env.svm.send_transaction(tx);
    let meta = if let Some(index) = failure_index {
        let failure = result.expect_err("a consumed trade aborts the complete transaction");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(
                index + 2,
                InstructionError::Custom(PercolatorError::EngineStale as u32)
            )
        );
        evidence.failures += 1;
        evidence.transfers_rolled_back += counts[1];
        evidence.matcher_calls_rolled_back += counts[2];
        failure.meta
    } else {
        evidence.successes += 1;
        result.expect("current unconsumed consent makes public progress")
    };
    for (key, account) in before {
        if key != payer && (failure_index.is_some() || !writable.contains(&key)) {
            assert_eq!(
                env.svm.get_account(&key),
                account,
                "complete Account frame at {key}"
            );
        }
    }
    assert_eq!(env.svm.get_account(&payer), Some(expected_payer));
    for (program, count) in [env.program_id, spl_token::ID, env.matcher_program]
        .into_iter()
        .zip(counts)
    {
        assert_eq!(
            meta.logs
                .iter()
                .filter(|log| **log == format!("Program {program} success"))
                .count(),
            count
        );
    }
    assert!(meta.compute_units_consumed > 0 && meta.compute_units_consumed <= TX_CU_LIMIT);
    evidence.peak_cu = evidence.peak_cu.max(meta.compute_units_consumed);
}

#[derive(Default)]
struct Budget {
    domains: [u128; 2],
    funded: u128,
    fee_per_trader: u128,
    insurance_paid: u128,
    capital_paid: [u128; PRIMARY_ACTOR_COUNT],
    position: i128,
}

impl Budget {
    fn fund(&mut self, domain: usize, amount: u128) {
        self.domains[domain] += amount;
        self.funded += amount;
    }

    fn fill(&mut self, size: i128, bps: u64) {
        let charged = fee(size, bps);
        assert!(charged <= fee(size, SIGNED_BPS));
        self.fee_per_trader += charged;
        for domain in &mut self.domains {
            *domain += charged;
        }
        self.position += size;
    }

    fn pay_insurance(&mut self, amount: u128) {
        let long = amount.min(self.domains[0]);
        self.domains[0] -= long;
        self.domains[1] -= amount - long;
        self.insurance_paid += amount;
    }

    fn check(&self, env: &V16Svm, config: MarketConfig, supply: u128) {
        assert_public_stock_census("retained fee stock", env).unwrap();
        assert_public_encumbrance_census("retained fee stock", env).unwrap();
        let group = env.primary_market_state().1;
        let capital = config.actor_deposits.iter().sum::<u128>()
            - 2 * self.fee_per_trader
            - self.capital_paid.iter().sum::<u128>();
        let insurance = self.funded + 2 * self.fee_per_trader - self.insurance_paid;
        assert_eq!(
            (group.c_tot, group.insurance, group.vault),
            (capital, insurance, capital + insurance)
        );
        assert_eq!(&group.insurance_domain_budget[..2], &self.domains);
        assert!(group.insurance_domain_budget[2..]
            .iter()
            .all(|amount| *amount == 0));
        assert_eq!(u128::from(env.token_amount(env.vault)), capital + insurance);
        assert_eq!(env.token_supply_observed(), supply);
        assert_eq!(u128::from(env.mint_supply()), supply);
        assert_eq!(group.assets[0].effective_price, PRICE);
        assert_eq!(group.assets[0].oi_eff_long_q, self.position.unsigned_abs());
        assert_eq!(group.assets[0].oi_eff_short_q, self.position.unsigned_abs());
        for actor in 0..PRIMARY_ACTOR_COUNT {
            let owner = env.actors[actor].signer.pubkey();
            let portfolio = env.primary_portfolio(actor);
            assert_eq!(portfolio.owner, owner.to_bytes());
            let fee = if actor < 2 { self.fee_per_trader } else { 0 };
            assert_eq!(
                portfolio.capital.get(),
                config.actor_deposits[actor] - fee - self.capital_paid[actor]
            );
            assert_eq!(portfolio.pnl.get(), 0);
            let legs: Vec<_> = portfolio
                .legs
                .iter()
                .map(|leg| leg.try_to_runtime().unwrap())
                .filter(|leg| leg.active)
                .collect();
            let position = match actor {
                0 => self.position,
                1 => -self.position,
                _ => 0,
            };
            assert_eq!(legs.len(), usize::from(position != 0));
            if position != 0 {
                assert_eq!(legs[0].asset_index, 0);
                assert_eq!(legs[0].basis_pos_q, position);
            }
            let insurance_paid = if actor == INSURER {
                self.insurance_paid
            } else {
                0
            };
            let funded = if actor == INSURER { self.funded } else { 0 };
            for (key, expected) in [
                (
                    env.actors[actor].source_token,
                    u128::from(config.actor_token_balances[actor])
                        - config.actor_deposits[actor]
                        - funded,
                ),
                (
                    env.actors[actor].destination_token,
                    self.capital_paid[actor] + insurance_paid,
                ),
            ] {
                let account = env.svm.get_account(&key).unwrap();
                assert_eq!(account.owner, spl_token::ID);
                let token = TokenAccount::unpack(&account.data).unwrap();
                assert_eq!(
                    (token.owner, token.mint, u128::from(token.amount)),
                    (owner, env.mint, expected)
                );
            }
        }
    }
}

#[test]
fn v16_program_retained_fee_stock_bundles_preserve_owner_budgets_across_policy_and_replenishment() {
    let mut evidence = Evidence::default();
    for live_bps in [0, 19] {
        let mut endpoints = [None, None];
        for route in ROUTES {
            for direction in [-1, 1] {
                for withdrawal_first in [false, true] {
                    let config = MarketConfig {
                        initial_price: PRICE,
                        ..MarketConfig::default()
                    };
                    let mut env = V16Svm::new([0x6b; 32], config);
                    let payer = Keypair::from_seed(&[0x27; 32]).unwrap();
                    env.svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
                    let supply = env.token_supply_observed();
                    let size = direction * (POS_SCALE as i128 + 1);
                    let mut nonce = 0;
                    let mut budget = Budget::default();
                    for role in [ASSET_AUTH_INSURANCE, ASSET_AUTH_INSURANCE_OPERATOR] {
                        env.update_asset_authority_from_admin(0, role, INSURER)
                            .unwrap();
                    }
                    for (domain, amount) in INITIAL_STOCK.into_iter().enumerate() {
                        env.top_up_insurance_domain_for_actor(INSURER, domain as u16, amount)
                            .unwrap();
                        budget.fund(domain, amount);
                    }
                    set_policy(&mut env, &payer, SIGNED_BPS, &mut nonce, &mut evidence);
                    env.set_matcher_config_with_trade_fee_cap(1, 1, LP_CAP)
                        .unwrap();
                    budget.check(&env, config, supply);

                    let open = trade(&env, route, size);
                    let payout = insurance(&env, FIRST_PAYOUT);
                    let ordered = if withdrawal_first {
                        vec![payout, open.clone()]
                    } else {
                        vec![open.clone(), payout]
                    };
                    let clean = sign(&env, &payer, &ordered, &mut nonce);
                    let clean_bytes = bincode::serialize(&clean).unwrap();
                    let mut duplicated = ordered.clone();
                    duplicated.push(open);
                    let failed = sign(&env, &payer, &duplicated, &mut nonce);
                    let retained: Vec<_> = (0..2)
                        .map(|_| {
                            ROUTES.map(|route| {
                                let ix = trade(&env, route, size);
                                (ix.clone(), sign(&env, &payer, &[ix], &mut nonce))
                            })
                        })
                        .collect();
                    let signatures: BTreeSet<_> = std::iter::once(&clean)
                        .chain(std::iter::once(&failed))
                        .chain(retained.iter().flatten().map(|(_, tx)| tx))
                        .map(|tx| tx.signatures[0])
                        .collect();
                    assert_eq!(signatures.len(), 10);
                    for tx in
                        std::iter::once(&clean).chain(retained.iter().flatten().map(|(_, tx)| tx))
                    {
                        let before = frame(&env, tx);
                        env.svm
                            .simulate_transaction(tx.clone().into())
                            .expect("every retained alternative is initially executable");
                        assert_eq!(frame(&env, tx), before);
                        evidence.simulations += 1;
                    }

                    let epochs = [0, 1].map(|actor| env.primary_portfolio_position_epoch(actor));
                    let sequence = env.primary_portfolio_matcher_sequence(1);
                    deliver(
                        &mut env,
                        failed,
                        Some(2),
                        [2, 1, usize::from(cpi(route))],
                        &mut evidence,
                    );
                    budget.check(&env, config, supply);
                    assert_eq!(
                        [0, 1].map(|actor| env.primary_portfolio_position_epoch(actor)),
                        epochs
                    );
                    let policy = env.primary_control_sequences(0);
                    set_policy(&mut env, &payer, live_bps, &mut nonce, &mut evidence);
                    let mut expected_policy = policy;
                    expected_policy.trade_fee += 1;
                    assert_eq!(env.primary_control_sequences(0), expected_policy);
                    assert_eq!(env.primary_portfolio_matcher_sequence(1), sequence);
                    assert_eq!(bincode::serialize(&clean).unwrap(), clean_bytes);
                    deliver(
                        &mut env,
                        clean,
                        None,
                        [2, 1, usize::from(cpi(route))],
                        &mut evidence,
                    );
                    let charged_bps = if cpi(route) { live_bps } else { SIGNED_BPS };
                    if withdrawal_first {
                        budget.pay_insurance(FIRST_PAYOUT);
                    }
                    budget.fill(size, charged_bps);
                    if !withdrawal_first {
                        budget.pay_insurance(FIRST_PAYOUT);
                    }
                    budget.check(&env, config, supply);
                    assert_eq!(
                        [0, 1].map(|actor| env.primary_portfolio_position_epoch(actor)),
                        epochs.map(|epoch| epoch + 1)
                    );

                    for (phase, alternatives) in retained.into_iter().enumerate() {
                        if phase == 1 {
                            set_policy(&mut env, &payer, SIGNED_BPS, &mut nonce, &mut evidence);
                            let close = trade(&env, route, -size);
                            let tx = sign(&env, &payer, &[close], &mut nonce);
                            deliver(
                                &mut env,
                                tx,
                                None,
                                [1, 0, usize::from(cpi(route))],
                                &mut evidence,
                            );
                            budget.fill(-size, SIGNED_BPS);
                            env.top_up_insurance_domain_for_actor(INSURER, 1, REFILL)
                                .unwrap();
                            budget.fund(1, REFILL);
                            budget.check(&env, config, supply);
                        }
                        for (stale_ix, original_tx) in alternatives {
                            deliver(&mut env, original_tx, Some(0), [0, 0, 0], &mut evidence);
                            // The reserve prefix is freshly authorized against this phase's stock.
                            // Only the trade payload reuses the consumed position bindings.
                            let prefix = insurance(&env, 1);
                            let tx = sign(&env, &payer, &[prefix, stale_ix], &mut nonce);
                            deliver(&mut env, tx, Some(1), [1, 1, 0], &mut evidence);
                            budget.check(&env, config, supply);
                        }
                    }

                    for actor in 0..PRIMARY_ACTOR_COUNT {
                        let amount = config.actor_deposits[actor]
                            - if actor < 2 { budget.fee_per_trader } else { 0 };
                        env.withdraw_primary(actor, amount).unwrap();
                        budget.capital_paid[actor] = amount;
                        budget.check(&env, config, supply);
                    }
                    let remaining = budget.domains.iter().sum();
                    let payout = insurance(&env, remaining);
                    let tx = sign(&env, &payer, &[payout], &mut nonce);
                    deliver(&mut env, tx, None, [1, 1, 0], &mut evidence);
                    budget.pay_insurance(remaining);
                    budget.check(&env, config, supply);
                    assert_eq!(env.token_amount(env.vault), 0);
                    let tokens = env.all_token_account_data();
                    let endpoint = &mut endpoints[usize::from(cpi(route))];
                    if let Some(expected) = endpoint.as_ref() {
                        assert_eq!(
                            &tokens, expected,
                            "direction, payout ordering and single/batch transport preserve final owner entitlements"
                        );
                    } else {
                        *endpoint = Some(tokens);
                    }
                    evidence.worlds += 1;
                }
            }
        }
    }
    assert_eq!(
        (evidence.worlds, evidence.simulations, evidence.failures),
        (32, 288, 544)
    );
    assert_eq!(
        (
            evidence.transfers_rolled_back,
            evidence.matcher_calls_rolled_back,
            evidence.successes
        ),
        (288, 16, 192)
    );
    eprintln!("INV-014 retained fee stock: {} worlds, {} simulations, {} exact rollbacks, {} rolled-back SPL transfers, {} rolled-back matcher calls, {} measured successes, peak CU={}", evidence.worlds, evidence.simulations, evidence.failures, evidence.transfers_rolled_back, evidence.matcher_calls_rolled_back, evidence.successes, evidence.peak_cu);
}
