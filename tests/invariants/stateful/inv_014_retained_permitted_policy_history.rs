//! INV-011/014/024/036/047/080: independently retained CPI fills delivered at
//! different points in a permitted policy history charge each owner exactly.
//! Intermediate policy changes and an aborted policy/fill transaction must not
//! change the endpoint. All policies stay within both participants' consent.
//! Above-consent execution and successful reserve-debit replay remain outside
//! this coverage; no initialized program account is modified out of band.

use crate::support::{
    fuzz_model::{assert_public_encumbrance_census, assert_public_stock_census},
    v16_svm::{MarketConfig, V16Svm, PRIMARY_ACTOR_COUNT, TX_CU_LIMIT},
};
use percolator::POS_SCALE;
use percolator_prog::{
    error::PercolatorError,
    ix::{BatchTradeCpiLeg, Instruction as ProgInstruction},
    processor::{ASSET_AUTH_INSURANCE, ASSET_AUTH_INSURANCE_OPERATOR},
    state::read_portfolio_matcher_config,
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

#[path = "inv_014_retained_fee_expiry.rs"]
mod retained_fee_expiry;

const PRICE: u64 = 100_003;
const INITIAL_BPS: u64 = 19;
const DELIVERY_BPS: [u64; 2] = [7, 31];
const INSURER: usize = 4;

fn fee(size: i128, bps: u64) -> u128 {
    (size.unsigned_abs() * u128::from(PRICE))
        .div_ceil(POS_SCALE)
        .checked_mul(u128::from(bps))
        .unwrap()
        .div_ceil(10_000)
}

fn trade(env: &V16Svm, pair: usize, batch: bool, size_q: i128, cap: u64) -> Instruction {
    let (a, b) = (2 * pair, 2 * pair + 1);
    let market_id = env.primary_market_state().1.assets[pair].market_id;
    let instruction = if batch {
        ProgInstruction::BatchTradeCpi {
            account_a_portfolio_id: env.primary_portfolio_id(a),
            account_a_position_epoch: env.primary_portfolio_position_epoch(a),
            account_b_portfolio_id: env.primary_portfolio_id(b),
            account_b_position_epoch: env.primary_portfolio_position_epoch(b),
            account_b_matcher_sequence: env.primary_portfolio_matcher_sequence(b),
            max_slippage_atoms: 0,
            max_fee_atoms: fee(size_q, cap),
            legs: vec![BatchTradeCpiLeg {
                asset_index: pair as u16,
                market_id,
                size_q,
                fee_bps: cap,
                limit_price: PRICE,
            }],
        }
    } else {
        ProgInstruction::TradeCpi {
            account_a_portfolio_id: env.primary_portfolio_id(a),
            account_a_position_epoch: env.primary_portfolio_position_epoch(a),
            account_b_portfolio_id: env.primary_portfolio_id(b),
            account_b_position_epoch: env.primary_portfolio_position_epoch(b),
            account_b_matcher_sequence: env.primary_portfolio_matcher_sequence(b),
            asset_index: pair as u16,
            market_id,
            size_q,
            fee_bps: cap,
            limit_price: PRICE,
            backing_fee_cap_bps: 0,
        }
    };
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(env.actors[a].signer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.actors[a].portfolio, false),
            AccountMeta::new(env.actors[b].portfolio, false),
            AccountMeta::new_readonly(env.matcher_program, false),
            AccountMeta::new(env.actors[b].matcher_context, false),
            AccountMeta::new_readonly(env.actors[b].matcher_delegate, false),
        ],
        data: instruction.encode(),
    }
}

fn policy(env: &V16Svm, bps: u64) -> Instruction {
    let sequences = env.primary_control_sequences(0);
    Instruction {
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
    }
}

fn sign(env: &V16Svm, payer: &Keypair, ixs: &[Instruction], nonce: &mut u32) -> Transaction {
    *nonce += 1;
    let mut message = vec![
        ComputeBudgetInstruction::request_heap_frame(256 * 1024),
        ComputeBudgetInstruction::set_compute_unit_limit(TX_CU_LIMIT as u32 - *nonce),
    ];
    message.extend_from_slice(ixs);
    let mut signers = vec![payer];
    for signer in env.actors.iter().map(|a| &a.signer) {
        if ixs
            .iter()
            .flat_map(|ix| &ix.accounts)
            .any(|meta| meta.is_signer && meta.pubkey == signer.pubkey())
        {
            signers.push(signer);
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
    successes: usize,
    failures: usize,
    success_cu: u64,
    failure_cu: u64,
}

fn deliver(
    env: &mut V16Svm,
    tx: Transaction,
    changed: &[Pubkey],
    failure: bool,
    calls: [usize; 3],
    evidence: &mut Evidence,
) {
    let before = frame(env, &tx);
    let payer = tx.message.account_keys[0];
    let mut expected_payer = env.svm.get_account(&payer).unwrap();
    expected_payer.lamports -= FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let result = env.svm.send_transaction(tx);
    let meta = if failure {
        let failed = result.expect_err("duplicate policy suffix aborts policy and fill together");
        assert_eq!(
            failed.err,
            TransactionError::InstructionError(
                4,
                InstructionError::Custom(PercolatorError::EngineStale as u32)
            )
        );
        evidence.failures += 1;
        evidence.failure_cu = evidence.failure_cu.max(failed.meta.compute_units_consumed);
        failed.meta
    } else {
        let meta = result.expect("consented public instruction succeeds");
        evidence.successes += 1;
        evidence.success_cu = evidence.success_cu.max(meta.compute_units_consumed);
        meta
    };
    for (key, account) in before {
        if key == payer {
            continue;
        }
        let actual = env.svm.get_account(&key);
        if failure || !changed.contains(&key) {
            assert_eq!(actual, account, "complete Account frame: {key}");
        } else {
            let mut expected = account.unwrap();
            expected.data = actual.as_ref().unwrap().data.clone();
            assert_eq!(actual, Some(expected), "only data may change: {key}");
        }
    }
    assert_eq!(env.svm.get_account(&payer), Some(expected_payer));
    for (program, count) in [env.program_id, spl_token::ID, env.matcher_program]
        .into_iter()
        .zip(calls)
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
}

#[derive(Default)]
struct Budget {
    fees: [u128; 2],
    positions: [i128; 2],
    paid: [u128; PRIMARY_ACTOR_COUNT],
    insurance_paid: [u128; 2],
}

impl Budget {
    fn check(&self, env: &V16Svm, config: MarketConfig, supply: u128) {
        assert_public_stock_census("retained permitted policy history", env).unwrap();
        assert_public_encumbrance_census("retained permitted policy history", env).unwrap();
        let group = env.primary_market_state().1;
        let capital = config.actor_deposits.iter().sum::<u128>()
            - 2 * self.fees.iter().sum::<u128>()
            - self.paid.iter().sum::<u128>();
        let insurance =
            2 * self.fees.iter().sum::<u128>() - self.insurance_paid.iter().sum::<u128>();
        assert_eq!(
            (group.c_tot, group.insurance, group.vault),
            (capital, insurance, capital + insurance)
        );
        assert_eq!(u128::from(env.token_amount(env.vault)), group.vault);
        assert_eq!(env.token_supply_observed(), supply);
        assert_eq!(u128::from(env.mint_supply()), supply);
        for pair in 0..2 {
            assert_eq!(group.assets[pair].effective_price, PRICE);
            assert_eq!(
                group.assets[pair].oi_eff_long_q,
                self.positions[pair].unsigned_abs()
            );
            assert_eq!(
                group.assets[pair].oi_eff_short_q,
                self.positions[pair].unsigned_abs()
            );
            let long_paid = self.insurance_paid[pair].min(self.fees[pair]);
            assert_eq!(
                &group.insurance_domain_budget[2 * pair..2 * pair + 2],
                &[
                    self.fees[pair] - long_paid,
                    self.fees[pair] - (self.insurance_paid[pair] - long_paid),
                ]
            );
        }
        assert!(group.insurance_domain_budget[4..].iter().all(|v| *v == 0));
        for actor in 0..PRIMARY_ACTOR_COUNT {
            let owner = env.actors[actor].signer.pubkey();
            let portfolio = env.primary_portfolio(actor);
            let fees = if actor < 4 { self.fees[actor / 2] } else { 0 };
            assert_eq!(portfolio.owner, owner.to_bytes());
            assert_eq!(
                portfolio.capital.get(),
                config.actor_deposits[actor] - fees - self.paid[actor]
            );
            assert_eq!(portfolio.pnl.get(), 0);
            let position = if actor < 4 {
                self.positions[actor / 2] * if actor % 2 == 0 { 1 } else { -1 }
            } else {
                0
            };
            let legs: Vec<_> = portfolio
                .legs
                .iter()
                .map(|leg| leg.try_to_runtime().unwrap())
                .filter(|leg| leg.active)
                .collect();
            assert_eq!(legs.len(), usize::from(position != 0));
            if position != 0 {
                assert_eq!(legs[0].asset_index as usize, actor / 2);
                assert_eq!(legs[0].basis_pos_q, position);
            }
            for (key, amount) in [
                (
                    env.actors[actor].source_token,
                    u128::from(config.actor_token_balances[actor]) - config.actor_deposits[actor],
                ),
                (
                    env.actors[actor].destination_token,
                    self.paid[actor]
                        + if actor == INSURER {
                            self.insurance_paid.iter().sum::<u128>()
                        } else {
                            0
                        },
                ),
            ] {
                assert_token(env, key, owner, amount);
            }
        }
    }
}

fn assert_token(env: &V16Svm, key: Pubkey, owner: Pubkey, amount: u128) {
    let account = env.svm.get_account(&key).unwrap();
    assert_eq!(account.owner, spl_token::ID);
    let token = TokenAccount::unpack(&account.data).unwrap();
    assert_eq!(
        (token.owner, token.mint, u128::from(token.amount)),
        (owner, env.mint, amount)
    );
}

#[test]
fn v16_program_retained_cpi_owner_budgets_ignore_permitted_policy_detours() {
    let mut evidence = Evidence::default();
    let mut worlds = 0;
    let mut simulations = 0;
    let mut endpoint = None;
    for routes in [[false, false], [false, true], [true, false], [true, true]] {
        for taker_tighter in [true, false] {
            for direction in [-1, 1] {
                for detour in [false, true] {
                    let config = MarketConfig {
                        initial_price: PRICE,
                        ..MarketConfig::default()
                    };
                    let mut env = V16Svm::new([0x73; 32], config);
                    let payer = Keypair::from_seed(&[0x39; 32]).unwrap();
                    env.svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
                    let supply = env.token_supply_observed();
                    let mut nonce = 0;
                    let mut budget = Budget::default();
                    let caps = if taker_tighter {
                        [[37, 503], [83, 211]]
                    } else {
                        [[503, 37], [211, 83]]
                    };
                    let sizes = [
                        direction * (POS_SCALE as i128 + 1),
                        -direction * (3 * POS_SCALE as i128 + 7),
                    ];
                    for asset in 0..2 {
                        for role in [ASSET_AUTH_INSURANCE, ASSET_AUTH_INSURANCE_OPERATOR] {
                            env.update_asset_authority_from_admin(asset, role, INSURER)
                                .unwrap();
                        }
                    }
                    let tx = sign(&env, &payer, &[policy(&env, INITIAL_BPS)], &mut nonce);
                    let market = env.market;
                    deliver(&mut env, tx, &[market], false, [1, 0, 0], &mut evidence);
                    for pair in 0..2 {
                        env.set_matcher_config_with_trade_fee_cap(
                            2 * pair + 1,
                            1,
                            caps[pair][1] as u16,
                        )
                        .unwrap();
                    }
                    budget.check(&env, config, supply);
                    let grants = [1, 3].map(|actor| {
                        read_portfolio_matcher_config(&env.primary_portfolio_data(actor)).unwrap()
                    });
                    let epochs =
                        [0, 1, 2, 3].map(|actor| env.primary_portfolio_position_epoch(actor));
                    let matcher_sequences =
                        [1, 3].map(|actor| env.primary_portfolio_matcher_sequence(actor));
                    let opens = [0, 1]
                        .map(|pair| trade(&env, pair, routes[pair], sizes[pair], caps[pair][0]));
                    let retained = opens
                        .each_ref()
                        .map(|ix| sign(&env, &payer, std::slice::from_ref(ix), &mut nonce));
                    let bytes = retained
                        .each_ref()
                        .map(|tx| bincode::serialize(tx).unwrap());
                    for tx in &retained {
                        let before = frame(&env, tx);
                        env.svm
                            .simulate_transaction(tx.clone().into())
                            .expect("retained fill initially executable");
                        assert_eq!(frame(&env, tx), before);
                        simulations += 1;
                    }
                    for pair in 0..2 {
                        let history = if detour {
                            vec![37, 0, DELIVERY_BPS[pair]]
                        } else {
                            vec![DELIVERY_BPS[pair]]
                        };
                        for bps in history {
                            assert!(caps.iter().flatten().all(|cap| bps <= *cap));
                            let old_sequences = env.primary_control_sequences(0);
                            let tx = sign(&env, &payer, &[policy(&env, bps)], &mut nonce);
                            let market = env.market;
                            deliver(&mut env, tx, &[market], false, [1, 0, 0], &mut evidence);
                            let mut expected = old_sequences;
                            expected.trade_fee += 1;
                            assert_eq!(env.primary_control_sequences(0), expected);
                            budget.check(&env, config, supply);
                        }
                        // The fill executes at 23 bps before the duplicate control rejects.
                        // Its fee, position, matcher context, and the policy prefix all roll back.
                        let temporary = policy(&env, 23);
                        let failed = sign(
                            &env,
                            &payer,
                            &[temporary.clone(), opens[pair].clone(), temporary],
                            &mut nonce,
                        );
                        deliver(&mut env, failed, &[], true, [2, 0, 1], &mut evidence);
                        budget.check(&env, config, supply);
                        assert_eq!(bincode::serialize(&retained[pair]).unwrap(), bytes[pair]);
                        let changed = [
                            env.market,
                            env.actors[2 * pair].portfolio,
                            env.actors[2 * pair + 1].portfolio,
                            env.actors[2 * pair + 1].matcher_context,
                        ];
                        deliver(
                            &mut env,
                            retained[pair].clone(),
                            &changed,
                            false,
                            [1, 0, 1],
                            &mut evidence,
                        );
                        let charged = fee(sizes[pair], DELIVERY_BPS[pair]);
                        assert!(charged > 0);
                        assert!(caps[pair]
                            .iter()
                            .all(|cap| charged < fee(sizes[pair], *cap)));
                        budget.fees[pair] += charged;
                        budget.positions[pair] = sizes[pair];
                        budget.check(&env, config, supply);
                        for actor in 0..4 {
                            assert_eq!(
                                env.primary_portfolio_position_epoch(actor),
                                epochs[actor] + u64::from(actor / 2 <= pair)
                            );
                        }
                        for lp in 0..2 {
                            let current = read_portfolio_matcher_config(
                                &env.primary_portfolio_data(2 * lp + 1),
                            )
                            .unwrap();
                            assert_eq!(current.matcher_program, grants[lp].matcher_program);
                            assert_eq!(current.matcher_context, grants[lp].matcher_context);
                            assert_eq!(current.matcher_delegate, grants[lp].matcher_delegate);
                            assert_eq!(current.trade_fee_cap_bps(), grants[lp].trade_fee_cap_bps());
                            assert_eq!(current.enabled(), grants[lp].enabled());
                            assert_eq!(
                                current.position_epoch(),
                                grants[lp].position_epoch() + u64::from(lp <= pair)
                            );
                            assert_eq!(
                                env.primary_portfolio_matcher_sequence(2 * lp + 1),
                                matcher_sequences[lp]
                            );
                        }
                    }
                    let tx = sign(&env, &payer, &[policy(&env, INITIAL_BPS)], &mut nonce);
                    let market = env.market;
                    deliver(&mut env, tx, &[market], false, [1, 0, 0], &mut evidence);
                    for pair in 0..2 {
                        let close = trade(&env, pair, routes[pair], -sizes[pair], caps[pair][0]);
                        let tx = sign(&env, &payer, &[close], &mut nonce);
                        let changed = [
                            env.market,
                            env.actors[2 * pair].portfolio,
                            env.actors[2 * pair + 1].portfolio,
                            env.actors[2 * pair + 1].matcher_context,
                        ];
                        deliver(&mut env, tx, &changed, false, [1, 0, 1], &mut evidence);
                        budget.fees[pair] += fee(sizes[pair], INITIAL_BPS);
                        budget.positions[pair] = 0;
                        budget.check(&env, config, supply);
                    }
                    for actor in 0..PRIMARY_ACTOR_COUNT {
                        let amount = config.actor_deposits[actor]
                            - if actor < 4 { budget.fees[actor / 2] } else { 0 };
                        let ix = Instruction {
                            program_id: env.program_id,
                            accounts: vec![
                                AccountMeta::new(env.actors[actor].signer.pubkey(), true),
                                AccountMeta::new(env.market, false),
                                AccountMeta::new(env.actors[actor].portfolio, false),
                                AccountMeta::new(env.actors[actor].destination_token, false),
                                AccountMeta::new(env.vault, false),
                                AccountMeta::new_readonly(env.vault_authority, false),
                                AccountMeta::new_readonly(spl_token::ID, false),
                            ],
                            data: ProgInstruction::Withdraw {
                                portfolio_id: env.primary_portfolio_id(actor),
                                expected_sequence: env.primary_portfolio_matcher_sequence(actor),
                                amount,
                            }
                            .encode(),
                        };
                        let tx = sign(&env, &payer, &[ix], &mut nonce);
                        let changed = [
                            env.market,
                            env.actors[actor].portfolio,
                            env.actors[actor].destination_token,
                            env.vault,
                        ];
                        deliver(&mut env, tx, &changed, false, [1, 1, 0], &mut evidence);
                        budget.paid[actor] = amount;
                        budget.check(&env, config, supply);
                    }
                    for pair in 0..2 {
                        let amount = 2 * budget.fees[pair];
                        let ix = Instruction {
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
                                asset_index: pair as u16,
                                market_id: env.primary_market_state().1.assets[pair].market_id,
                                authority_epoch: env
                                    .primary_control_sequences(pair)
                                    .authority_epoch,
                                amount,
                            }
                            .encode(),
                        };
                        let tx = sign(&env, &payer, &[ix], &mut nonce);
                        let changed =
                            [env.market, env.actors[INSURER].destination_token, env.vault];
                        deliver(&mut env, tx, &changed, false, [1, 1, 0], &mut evidence);
                        budget.insurance_paid[pair] = amount;
                        budget.check(&env, config, supply);
                    }
                    assert_eq!(env.token_amount(env.vault), 0);
                    let tokens = env.all_token_account_data();
                    if let Some(expected) = &endpoint {
                        assert_eq!(&tokens, expected, "history, consent ordering, direction and CPI transport preserve each owner endpoint");
                    } else {
                        endpoint = Some(tokens);
                    }
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!(
        (worlds, simulations, evidence.failures, evidence.successes),
        (32, 64, 64, 544)
    );
    eprintln!("INV-014 retained permitted policy history: worlds={worlds}, simulations={simulations}, exact_rollbacks={}, rolled_back_matcher_calls={}, measured_successes={}, success_cu={}, failure_cu={}", evidence.failures, evidence.failures, evidence.successes, evidence.success_cu, evidence.failure_cu);
}
