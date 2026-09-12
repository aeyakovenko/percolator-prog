//! INV-010/011/014/024/036/047/080/081: grant expiry after policy succession
//! aborts an earlier fee-bearing CPI without consuming either retained intent.
//! An independently signed bilateral alternative realizes the same fee budget.
//! Bounded public histories only; rows 411/432 remain OPEN.

use super::*;
use percolator_prog::{ix::BatchTradeLeg, state::read_portfolio_matcher_expiry};

const CAP: u64 = 37;
const EXPIRY: u64 = 4;

fn bilateral(env: &V16Svm, pair: usize, batch: bool, size_q: i128) -> Instruction {
    let (a, b) = (2 * pair, 2 * pair + 1);
    let market_id = env.primary_market_state().1.assets[pair].market_id;
    let ix = if batch {
        ProgInstruction::BatchTradeNoCpi {
            account_a_portfolio_id: env.primary_portfolio_id(a),
            account_a_position_epoch: env.primary_portfolio_position_epoch(a),
            account_b_portfolio_id: env.primary_portfolio_id(b),
            account_b_position_epoch: env.primary_portfolio_position_epoch(b),
            legs: vec![BatchTradeLeg {
                asset_index: pair as u16,
                market_id,
                size_q,
                exec_price: PRICE,
                fee_bps: CAP,
            }],
        }
    } else {
        ProgInstruction::TradeNoCpi {
            account_a_portfolio_id: env.primary_portfolio_id(a),
            account_a_position_epoch: env.primary_portfolio_position_epoch(a),
            account_b_portfolio_id: env.primary_portfolio_id(b),
            account_b_position_epoch: env.primary_portfolio_position_epoch(b),
            asset_index: pair as u16,
            market_id,
            size_q,
            exec_price: PRICE,
            fee_bps: CAP,
            backing_fee_cap_bps: 0,
        }
    };
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(env.actors[a].signer.pubkey(), true),
            AccountMeta::new(env.actors[b].signer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.actors[a].portfolio, false),
            AccountMeta::new(env.actors[b].portfolio, false),
        ],
        data: ix.encode(),
    }
}

fn changed_pair(env: &V16Svm, pair: usize, cpi: bool) -> Vec<Pubkey> {
    let mut keys = vec![
        env.market,
        env.actors[2 * pair].portfolio,
        env.actors[2 * pair + 1].portfolio,
    ];
    if cpi {
        keys.push(env.actors[2 * pair + 1].matcher_context);
    }
    keys
}

fn reject_expired(env: &mut V16Svm, tx: Transaction, policy_prefix: bool, evidence: &mut Evidence) {
    let before = frame(env, &tx);
    let payer = tx.message.account_keys[0];
    let mut expected_payer = env.svm.get_account(&payer).unwrap();
    expected_payer.lamports -= FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let failed = env
        .svm
        .send_transaction(tx)
        .expect_err("expired LP consent");
    assert_eq!(
        failed.err,
        TransactionError::InstructionError(
            3 + u8::from(policy_prefix),
            InstructionError::Custom(PercolatorError::Unauthorized as u32),
        )
    );
    for (key, account) in before {
        assert_eq!(
            env.svm.get_account(&key),
            if key == payer {
                Some(expected_payer.clone())
            } else {
                account
            },
            "expiry restores the complete Account: {key}"
        );
    }
    for (program, count) in [
        (env.program_id, 1 + usize::from(policy_prefix)),
        (env.matcher_program, 1),
        (spl_token::ID, 0),
    ] {
        assert_eq!(
            failed
                .meta
                .logs
                .iter()
                .filter(|log| **log == format!("Program {program} success"))
                .count(),
            count,
            "the first fee-bearing fill must complete before expiry rejects"
        );
    }
    assert!(failed.meta.compute_units_consumed > 0);
    assert!(failed.meta.compute_units_consumed <= TX_CU_LIMIT);
    evidence.failures += 1;
    evidence.failure_cu = evidence.failure_cu.max(failed.meta.compute_units_consumed);
}

#[test]
fn v16_program_retained_fee_prefix_rolls_back_at_grant_expiry_before_bilateral_retry() {
    let mut evidence = Evidence::default();
    let mut worlds = 0;
    let mut simulations = 0;
    let mut endpoint = None;
    for prefix_batch in [false, true] {
        for expired_batch in [false, true] {
            for slot in [EXPIRY, EXPIRY + 1] {
                for direction in [-1, 1] {
                    let config = MarketConfig {
                        initial_price: PRICE,
                        ..MarketConfig::default()
                    };
                    let mut env = V16Svm::new([0x74; 32], config);
                    let payer = Keypair::from_seed(&[0x3a; 32]).unwrap();
                    env.svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
                    let supply = env.token_supply_observed();
                    let mut budget = Budget::default();
                    let mut nonce = 0;
                    let sizes = [
                        direction * (POS_SCALE as i128 + 1),
                        -direction * (3 * POS_SCALE as i128 + 7),
                    ];
                    env.update_trade_fee_policy(INITIAL_BPS).unwrap();
                    for asset in 0..2 {
                        for role in [ASSET_AUTH_INSURANCE, ASSET_AUTH_INSURANCE_OPERATOR] {
                            env.update_asset_authority_from_admin(asset, role, INSURER)
                                .unwrap();
                        }
                    }
                    env.set_matcher_config_with_trade_fee_cap(1, 1, CAP as u16)
                        .unwrap();
                    let grant = Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(env.actors[3].signer.pubkey(), true),
                            AccountMeta::new_readonly(env.market, false),
                            AccountMeta::new(env.actors[3].portfolio, false),
                            AccountMeta::new_readonly(env.matcher_program, false),
                            AccountMeta::new_readonly(env.actors[3].matcher_context, false),
                            AccountMeta::new_readonly(env.actors[3].matcher_delegate, false),
                        ],
                        data: ProgInstruction::SetMatcherConfig {
                            portfolio_id: env.primary_portfolio_id(3),
                            expected_sequence: env.primary_portfolio_matcher_sequence(3),
                            enabled: 1,
                            trade_fee_cap_bps: CAP as u16,
                            expiry_slot: EXPIRY,
                        }
                        .encode(),
                    };
                    let tx = sign(&env, &payer, &[grant], &mut nonce);
                    let lp = env.actors[3].portfolio;
                    deliver(&mut env, tx, &[lp], false, [1, 0, 0], &mut evidence);
                    env.warp_to_slot(EXPIRY - 1);
                    budget.check(&env, config, supply);
                    let grants = [1, 3].map(|a| {
                        read_portfolio_matcher_config(&env.primary_portfolio_data(a)).unwrap()
                    });
                    let expiries = [1, 3].map(|a| {
                        read_portfolio_matcher_expiry(&env.primary_portfolio_data(a)).unwrap()
                    });
                    assert_eq!(expiries, [u64::MAX, EXPIRY]);
                    let epochs = [0, 1, 2, 3].map(|a| env.primary_portfolio_position_epoch(a));
                    let sequences = [1, 3].map(|a| env.primary_portfolio_matcher_sequence(a));
                    let opens = [
                        trade(&env, 0, prefix_batch, sizes[0], CAP),
                        trade(&env, 1, expired_batch, sizes[1], CAP),
                    ];
                    // Independent signatures predate both the policy handoff and Clock expiry.
                    let rejected = sign(&env, &payer, &opens, &mut nonce);
                    let retry = sign(&env, &payer, &opens, &mut nonce);
                    let retained_prefix = sign(&env, &payer, &opens[..1], &mut nonce);
                    let alternate = bilateral(&env, 1, !expired_batch, sizes[1]);
                    let retained_alternate = sign(&env, &payer, &[alternate], &mut nonce);
                    let retained = [&rejected, &retry, &retained_prefix, &retained_alternate];
                    let bytes = retained.map(|tx| bincode::serialize(tx).unwrap());
                    for tx in retained {
                        let before = frame(&env, tx);
                        env.svm
                            .simulate_transaction(tx.clone().into())
                            .expect("initially admissible retained consent");
                        assert_eq!(frame(&env, tx), before);
                        simulations += 1;
                    }
                    let authority_epoch = env.primary_control_sequences(0).authority_epoch;
                    env.update_market_authority_from_admin(INSURER).unwrap();
                    assert_eq!(
                        env.primary_control_sequences(0).authority_epoch,
                        authority_epoch + 1
                    );
                    let tx = sign(&env, &payer, &[policy(&env, CAP)], &mut nonce);
                    let market = env.market;
                    deliver(&mut env, tx, &[market], false, [1, 0, 0], &mut evidence);
                    let policy_state = env.primary_control_sequences(0);
                    // At the last live slot the same bundle still works at the stricter rate.
                    let before = frame(&env, &rejected);
                    env.svm
                        .simulate_transaction(rejected.clone().into())
                        .expect("repriced bundle remains consented before expiry");
                    assert_eq!(frame(&env, &rejected), before);
                    simulations += 1;
                    env.warp_to_slot(slot);
                    budget.check(&env, config, supply);
                    for (tx, expected) in [&rejected, &retry, &retained_prefix, &retained_alternate]
                        .into_iter()
                        .zip(&bytes)
                    {
                        assert_eq!(&bincode::serialize(tx).unwrap(), expected);
                    }
                    reject_expired(&mut env, rejected, false, &mut evidence);
                    budget.check(&env, config, supply);
                    reject_expired(&mut env, retry, false, &mut evidence);
                    budget.check(&env, config, supply);
                    // A permitted fee decrease and the paid sibling fill both roll back.
                    let relaxed = sign(
                        &env,
                        &payer,
                        &[policy(&env, 7), opens[0].clone(), opens[1].clone()],
                        &mut nonce,
                    );
                    reject_expired(&mut env, relaxed, true, &mut evidence);
                    assert_eq!(env.primary_control_sequences(0), policy_state);
                    assert_eq!(env.primary_market_state().0.trade_fee_base_bps, CAP);
                    assert_eq!(
                        [0, 1, 2, 3].map(|a| env.primary_portfolio_position_epoch(a)),
                        epochs
                    );
                    assert_eq!(
                        [1, 3].map(|a| env.primary_portfolio_matcher_sequence(a)),
                        sequences
                    );
                    assert_eq!(
                        [1, 3].map(|a| read_portfolio_matcher_config(
                            &env.primary_portfolio_data(a)
                        )
                        .unwrap()),
                        grants
                    );
                    assert_eq!(
                        [1, 3].map(|a| read_portfolio_matcher_expiry(
                            &env.primary_portfolio_data(a)
                        )
                        .unwrap()),
                        expiries
                    );
                    budget.check(&env, config, supply);

                    // Bilateral consent remains live without renewing the expired LP grant.
                    for (pair, tx, cpi) in
                        [(1, retained_alternate, false), (0, retained_prefix, true)]
                    {
                        assert_eq!(
                            bincode::serialize(&tx).unwrap(),
                            bytes[if cpi { 2 } else { 3 }]
                        );
                        let changed = changed_pair(&env, pair, cpi);
                        deliver(
                            &mut env,
                            tx,
                            &changed,
                            false,
                            [1, 0, usize::from(cpi)],
                            &mut evidence,
                        );
                        budget.fees[pair] += fee(sizes[pair], CAP);
                        budget.positions[pair] = sizes[pair];
                        budget.check(&env, config, supply);
                    }
                    for pair in 0..2 {
                        let close = if pair == 0 {
                            trade(&env, pair, !prefix_batch, -sizes[pair], CAP)
                        } else {
                            bilateral(&env, pair, expired_batch, -sizes[pair])
                        };
                        let tx = sign(&env, &payer, &[close], &mut nonce);
                        let changed = changed_pair(&env, pair, pair == 0);
                        deliver(
                            &mut env,
                            tx,
                            &changed,
                            false,
                            [1, 0, usize::from(pair == 0)],
                            &mut evidence,
                        );
                        budget.fees[pair] += fee(sizes[pair], CAP);
                        budget.positions[pair] = 0;
                        budget.check(&env, config, supply);
                    }
                    for actor in 0..PRIMARY_ACTOR_COUNT {
                        let amount = config.actor_deposits[actor]
                            - if actor < 4 { budget.fees[actor / 2] } else { 0 };
                        let tx = sign(
                            &env,
                            &payer,
                            &[Instruction {
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
                                    expected_sequence: env
                                        .primary_portfolio_matcher_sequence(actor),
                                    amount,
                                }
                                .encode(),
                            }],
                            &mut nonce,
                        );
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
                        let tx = sign(
                            &env,
                            &payer,
                            &[Instruction {
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
                            }],
                            &mut nonce,
                        );
                        let changed =
                            [env.market, env.actors[INSURER].destination_token, env.vault];
                        deliver(&mut env, tx, &changed, false, [1, 1, 0], &mut evidence);
                        budget.insurance_paid[pair] = amount;
                        budget.check(&env, config, supply);
                    }
                    assert_eq!(env.token_amount(env.vault), 0);
                    let tokens = env.all_token_account_data();
                    if let Some(expected) = &endpoint {
                        assert_eq!(
                            &tokens, expected,
                            "transport, expiry delay and direction preserve every payout"
                        );
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
        (16, 80, 48, 208)
    );
    eprintln!("INV-014 retained fee expiry: worlds={worlds}, simulations={simulations}, exact_rollbacks={}, paid_prefix_rollbacks={}, measured_successes={}, success_cu={}, failure_cu={}", evidence.failures, evidence.failures, evidence.successes, evidence.success_cu, evidence.failure_cu);
}
