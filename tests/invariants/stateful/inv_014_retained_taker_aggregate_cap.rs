//! INV-011/014/024/036/080: a retained two-leg CPI aggregate cap remains local
//! to the taker through committed and atomic policy changes with a permissive LP.
//! A real deposit prefix and optional policy write roll back on the cap rejection.
//! This covers batch CPI's explicit atom cap, not single-CPI fee-bps enforcement.

use crate::support::{
    fuzz_model::{assert_public_encumbrance_census, assert_public_stock_census},
    v16_svm::{MarketConfig, V16Svm, PRIMARY_ACTOR_COUNT, TX_CU_LIMIT},
};
use percolator::POS_SCALE;
use percolator_prog::{
    error::PercolatorError,
    ix::{BatchTradeCpiLeg, Instruction as ProgInstruction},
    processor::{ASSET_AUTH_INSURANCE, ASSET_AUTH_INSURANCE_OPERATOR},
    state::{read_portfolio_matcher_config, read_portfolio_matcher_expiry},
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
const CAP_BPS: u64 = 37;
const LP_CAP: u16 = 503;
const DEPOSITOR: usize = 2;
const INSURER: usize = 4;
const DEPOSIT: u128 = 17;

fn fees(sizes: [i128; 2], bps: u64) -> [u128; 2] {
    sizes.map(|q| {
        ((q.unsigned_abs() * u128::from(PRICE)).div_ceil(POS_SCALE) * u128::from(bps))
            .div_ceil(10_000)
    })
}

fn trade(env: &V16Svm, sizes: [i128; 2], cap: u128) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(env.actors[0].signer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.actors[0].portfolio, false),
            AccountMeta::new(env.actors[1].portfolio, false),
            AccountMeta::new_readonly(env.matcher_program, false),
            AccountMeta::new(env.actors[1].matcher_context, false),
            AccountMeta::new_readonly(env.actors[1].matcher_delegate, false),
        ],
        data: ProgInstruction::BatchTradeCpi {
            account_a_portfolio_id: env.primary_portfolio_id(0),
            account_a_position_epoch: env.primary_portfolio_position_epoch(0),
            account_b_portfolio_id: env.primary_portfolio_id(1),
            account_b_position_epoch: env.primary_portfolio_position_epoch(1),
            account_b_matcher_sequence: env.primary_portfolio_matcher_sequence(1),
            max_slippage_atoms: 0,
            max_fee_atoms: cap,
            legs: sizes
                .into_iter()
                .enumerate()
                .map(|(asset, size_q)| BatchTradeCpiLeg {
                    asset_index: asset as u16,
                    market_id: env.primary_market_state().1.assets[asset].market_id,
                    size_q,
                    fee_bps: u64::from(LP_CAP),
                    limit_price: PRICE,
                })
                .collect(),
        }
        .encode(),
    }
}

fn policy(env: &V16Svm, bps: u64, sequence: u64) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(env.actors[INSURER].signer.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        data: ProgInstruction::UpdateTradeFeePolicy {
            trade_fee_base_bps: bps,
            policy_sequence: sequence,
            authority_epoch: env.primary_control_sequences(0).authority_epoch,
        }
        .encode(),
    }
}

fn sign(env: &V16Svm, payer: &Keypair, ixs: &[Instruction], nonce: &mut u32) -> Transaction {
    *nonce += 1;
    let mut instructions = vec![
        ComputeBudgetInstruction::request_heap_frame(256 * 1024),
        ComputeBudgetInstruction::set_compute_unit_limit(TX_CU_LIMIT as u32 - *nonce),
    ];
    instructions.extend_from_slice(ixs);
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
        &instructions,
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
    rollbacks: usize,
    success_cu: u64,
    rejection_cu: u64,
}

fn deliver(
    env: &mut V16Svm,
    tx: Transaction,
    changed: &[Pubkey],
    rejected_index: Option<u8>,
    calls: [usize; 3],
    evidence: &mut Evidence,
) {
    let before = frame(env, &tx);
    let payer = tx.message.account_keys[0];
    let mut expected_payer = env.svm.get_account(&payer).unwrap();
    expected_payer.lamports -= FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let result = env.svm.send_transaction(tx);
    let meta = if let Some(index) = rejected_index {
        let failed =
            result.expect_err("taker's aggregate cap rejects the complete retained bundle");
        assert_eq!(
            failed.err,
            TransactionError::InstructionError(
                index,
                InstructionError::Custom(PercolatorError::InvalidInstruction as u32)
            )
        );
        evidence.rollbacks += 1;
        evidence.rejection_cu = evidence
            .rejection_cu
            .max(failed.meta.compute_units_consumed);
        failed.meta
    } else {
        let meta = result.expect("public instruction within signed bounds succeeds");
        evidence.successes += 1;
        evidence.success_cu = evidence.success_cu.max(meta.compute_units_consumed);
        meta
    };
    for (key, account) in before {
        if key == payer {
            continue;
        }
        let actual = env.svm.get_account(&key);
        if rejected_index.is_some() || !changed.contains(&key) {
            assert_eq!(actual, account, "complete tracked/compiled Account: {key}");
        } else {
            let mut expected = account.unwrap();
            expected.data = actual.as_ref().unwrap().data.clone();
            assert_eq!(actual, Some(expected), "only data changes: {key}");
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
    deposited: u128,
    fees: [u128; 2],
    positions: [i128; 2],
    paid: [u128; PRIMARY_ACTOR_COUNT],
    insurance_paid: [u128; 2],
}

impl Budget {
    fn check(&self, env: &V16Svm, config: MarketConfig, supply: u128) {
        assert_public_stock_census("retained taker aggregate cap", env).unwrap();
        assert_public_encumbrance_census("retained taker aggregate cap", env).unwrap();
        let group = env.primary_market_state().1;
        let total_fee: u128 = self.fees.iter().sum();
        let capital = config.actor_deposits.iter().sum::<u128>() + self.deposited
            - 2 * total_fee
            - self.paid.iter().sum::<u128>();
        let insurance = 2 * total_fee - self.insurance_paid.iter().sum::<u128>();
        assert_eq!(
            (group.c_tot, group.insurance, group.vault),
            (capital, insurance, capital + insurance)
        );
        assert_eq!(u128::from(env.token_amount(env.vault)), group.vault);
        assert_eq!(env.token_supply_observed(), supply);
        assert_eq!(u128::from(env.mint_supply()), supply);
        for asset in 0..2 {
            assert_eq!(group.assets[asset].effective_price, PRICE);
            assert_eq!(
                group.assets[asset].oi_eff_long_q,
                self.positions[asset].unsigned_abs()
            );
            assert_eq!(
                group.assets[asset].oi_eff_short_q,
                self.positions[asset].unsigned_abs()
            );
            let long_paid = self.insurance_paid[asset].min(self.fees[asset]);
            assert_eq!(
                &group.insurance_domain_budget[2 * asset..2 * asset + 2],
                &[
                    self.fees[asset] - long_paid,
                    self.fees[asset] - (self.insurance_paid[asset] - long_paid),
                ]
            );
        }
        assert!(group.insurance_domain_budget[4..].iter().all(|v| *v == 0));
        for actor in 0..PRIMARY_ACTOR_COUNT {
            let owner = env.actors[actor].signer.pubkey();
            let portfolio = env.primary_portfolio(actor);
            let deposit = if actor == DEPOSITOR {
                self.deposited
            } else {
                0
            };
            let debit = if actor < 2 { total_fee } else { 0 };
            assert_eq!(portfolio.owner, owner.to_bytes());
            assert_eq!(
                portfolio.capital.get(),
                config.actor_deposits[actor] + deposit - debit - self.paid[actor]
            );
            assert_eq!(portfolio.pnl.get(), 0, "no hidden economic debit");
            let legs: Vec<_> = portfolio
                .legs
                .iter()
                .map(|leg| leg.try_to_runtime().unwrap())
                .filter(|leg| leg.active)
                .collect();
            assert_eq!(
                legs.len(),
                if actor < 2 {
                    self.positions.iter().filter(|q| **q != 0).count()
                } else {
                    0
                }
            );
            for asset in 0..2 {
                let actual = legs
                    .iter()
                    .find(|leg| leg.asset_index as usize == asset)
                    .map_or(0, |leg| leg.basis_pos_q);
                let expected = match actor {
                    0 => self.positions[asset],
                    1 => -self.positions[asset],
                    _ => 0,
                };
                assert_eq!(
                    actual, expected,
                    "signed quantity: actor={actor}, asset={asset}"
                );
            }
            for (key, amount) in [
                (
                    env.actors[actor].source_token,
                    u128::from(config.actor_token_balances[actor])
                        - config.actor_deposits[actor]
                        - deposit,
                ),
                (
                    env.actors[actor].destination_token,
                    self.paid[actor]
                        + if actor == INSURER {
                            self.insurance_paid.iter().sum()
                        } else {
                            0
                        },
                ),
            ] {
                let account = env.svm.get_account(&key).unwrap();
                assert_eq!(account.owner, spl_token::ID);
                let token = TokenAccount::unpack(&account.data).unwrap();
                assert_eq!(
                    (token.owner, token.mint, u128::from(token.amount)),
                    (owner, env.mint, amount)
                );
            }
        }
    }
}

#[test]
fn v16_program_retained_taker_aggregate_cap_rolls_back_deposit_and_policy_prefix() {
    let mut evidence = Evidence::default();
    let mut worlds = 0;
    let mut endpoint = None;
    for direction in [-1, 1] {
        for atomic_policy in [false, true] {
            let config = MarketConfig {
                initial_price: PRICE,
                ..MarketConfig::default()
            };
            let mut env = V16Svm::new([0x68; 32], config);
            let payer = Keypair::from_seed(&[0x27; 32]).unwrap();
            env.svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
            let mut nonce = 0;
            let supply = env.token_supply_observed();
            let mut budget = Budget::default();
            for asset in 0..2 {
                for role in [ASSET_AUTH_INSURANCE, ASSET_AUTH_INSURANCE_OPERATOR] {
                    env.update_asset_authority_from_admin(asset, role, INSURER)
                        .unwrap();
                }
            }
            env.set_matcher_config_with_trade_fee_cap(1, 1, LP_CAP)
                .unwrap();
            let market = env.market;
            let tx = sign(
                &env,
                &payer,
                &[policy(
                    &env,
                    19,
                    env.primary_control_sequences(0).trade_fee + 1,
                )],
                &mut nonce,
            );
            deliver(&mut env, tx, &[market], None, [1, 0, 0], &mut evidence);
            budget.check(&env, config, supply);

            let sizes = [
                direction * (POS_SCALE as i128 + 1),
                -direction * (3 * POS_SCALE as i128 + 7),
            ];
            let signed_fees = fees(sizes, CAP_BPS);
            let cap = signed_fees.iter().sum::<u128>();
            let excessive = fees(sizes, CAP_BPS + 1);
            assert!(excessive.iter().all(|fee| *fee < cap));
            assert!(excessive.iter().sum::<u128>() > cap);
            assert!(CAP_BPS + 1 < u64::from(LP_CAP));
            let sequences = [0, 1].map(|asset| env.primary_control_sequences(asset));
            let epochs = [0, 1].map(|actor| env.primary_portfolio_position_epoch(actor));
            let matcher_sequence = env.primary_portfolio_matcher_sequence(1);
            let req_seq = env.primary_market_state().0.matcher_req_seq;
            let grant = read_portfolio_matcher_config(&env.primary_portfolio_data(1)).unwrap();
            let expiry = read_portfolio_matcher_expiry(&env.primary_portfolio_data(1)).unwrap();
            let context = env.svm.get_account(&env.actors[1].matcher_context);
            let deposit = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(env.actors[DEPOSITOR].signer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(env.actors[DEPOSITOR].portfolio, false),
                    AccountMeta::new(env.actors[DEPOSITOR].source_token, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: ProgInstruction::Deposit {
                    portfolio_id: env.primary_portfolio_id(DEPOSITOR),
                    expected_sequence: env.primary_portfolio_matcher_sequence(DEPOSITOR),
                    amount: DEPOSIT,
                }
                .encode(),
            };
            let open = trade(&env, sizes, cap);
            let retained = sign(&env, &payer, &[deposit.clone(), open.clone()], &mut nonce);
            let retained_bytes = bincode::serialize(&retained).unwrap();
            let mut rejected_ixs = vec![deposit];
            if atomic_policy {
                rejected_ixs.push(policy(&env, CAP_BPS + 1, sequences[0].trade_fee + 2));
            }
            rejected_ixs.push(open);
            let rejected = sign(&env, &payer, &rejected_ixs, &mut nonce);
            let rejected_bytes = bincode::serialize(&rejected).unwrap();
            assert!(!retained.message.account_keys
                [..retained.message.header.num_required_signatures as usize]
                .contains(&env.actors[1].signer.pubkey()));
            let before = frame(&env, &retained);
            env.svm
                .simulate_transaction(retained.clone().into())
                .expect("signed deposit and both fills executable at signing");
            assert_eq!(frame(&env, &retained), before);

            let landing_bps = CAP_BPS + u64::from(!atomic_policy);
            let tx = sign(
                &env,
                &payer,
                &[policy(&env, landing_bps, sequences[0].trade_fee + 1)],
                &mut nonce,
            );
            deliver(&mut env, tx, &[market], None, [1, 0, 0], &mut evidence);
            let mut expected_sequences = sequences;
            expected_sequences[0].trade_fee += 1;
            assert_eq!(
                [0, 1].map(|asset| env.primary_control_sequences(asset)),
                expected_sequences
            );
            budget.check(&env, config, supply);
            assert_eq!(bincode::serialize(&rejected).unwrap(), rejected_bytes);
            deliver(
                &mut env,
                rejected,
                &[],
                Some(3 + u8::from(atomic_policy)),
                [1 + usize::from(atomic_policy), 1, 1],
                &mut evidence,
            );
            budget.check(&env, config, supply);
            assert_eq!(
                [0, 1].map(|asset| env.primary_control_sequences(asset)),
                expected_sequences
            );
            assert_eq!(env.primary_market_state().0.trade_fee_base_bps, landing_bps);
            assert_eq!(env.primary_market_state().0.matcher_req_seq, req_seq);
            assert_eq!(
                [0, 1].map(|actor| env.primary_portfolio_position_epoch(actor)),
                epochs
            );

            if !atomic_policy {
                let tx = sign(
                    &env,
                    &payer,
                    &[policy(&env, CAP_BPS, sequences[0].trade_fee + 2)],
                    &mut nonce,
                );
                deliver(&mut env, tx, &[market], None, [1, 0, 0], &mut evidence);
                expected_sequences[0].trade_fee += 1;
            }
            assert_eq!(bincode::serialize(&retained).unwrap(), retained_bytes);
            let changed = [
                env.market,
                env.vault,
                env.actors[0].portfolio,
                env.actors[1].portfolio,
                env.actors[DEPOSITOR].portfolio,
                env.actors[DEPOSITOR].source_token,
            ];
            deliver(&mut env, retained, &changed, None, [2, 1, 1], &mut evidence);
            budget.deposited = DEPOSIT;
            budget.fees = signed_fees;
            budget.positions = sizes;
            budget.check(&env, config, supply);
            assert_eq!(
                config.actor_deposits[0] - env.primary_portfolio(0).capital.get(),
                cap
            );
            assert!(cap < fees(sizes, u64::from(LP_CAP)).iter().sum::<u128>());
            assert_eq!(
                [0, 1].map(|asset| env.primary_control_sequences(asset)),
                expected_sequences
            );
            assert_eq!(env.primary_market_state().0.matcher_req_seq, req_seq + 1);
            assert_eq!(
                [0, 1].map(|actor| env.primary_portfolio_position_epoch(actor)),
                epochs.map(|epoch| epoch + 1)
            );
            assert_eq!(env.primary_portfolio_matcher_sequence(1), matcher_sequence);
            assert_eq!(env.svm.get_account(&env.actors[1].matcher_context), context);
            let current = read_portfolio_matcher_config(&env.primary_portfolio_data(1)).unwrap();
            assert_eq!(
                (
                    current.matcher_program,
                    current.matcher_context,
                    current.matcher_delegate
                ),
                (
                    grant.matcher_program,
                    grant.matcher_context,
                    grant.matcher_delegate
                )
            );
            assert_eq!(
                (
                    current.enabled(),
                    current.trade_fee_cap_bps(),
                    current.position_epoch()
                ),
                (1, LP_CAP, grant.position_epoch() + 1)
            );
            assert_eq!(
                read_portfolio_matcher_expiry(&env.primary_portfolio_data(1)).unwrap(),
                expiry
            );

            let tx = sign(
                &env,
                &payer,
                &[policy(&env, 0, expected_sequences[0].trade_fee + 1)],
                &mut nonce,
            );
            deliver(&mut env, tx, &[market], None, [1, 0, 0], &mut evidence);
            let tx = sign(
                &env,
                &payer,
                &[trade(&env, sizes.map(|q| -q), 0)],
                &mut nonce,
            );
            let changed = [market, env.actors[0].portfolio, env.actors[1].portfolio];
            deliver(&mut env, tx, &changed, None, [1, 0, 1], &mut evidence);
            budget.positions = [0; 2];
            budget.check(&env, config, supply);
            for actor in 0..PRIMARY_ACTOR_COUNT {
                let amount = config.actor_deposits[actor]
                    + if actor == DEPOSITOR { DEPOSIT } else { 0 }
                    - if actor < 2 { cap } else { 0 };
                env.withdraw_primary(actor, amount).unwrap();
                budget.paid[actor] = amount;
                budget.check(&env, config, supply);
            }
            for asset in 0..2 {
                let amount = 2 * signed_fees[asset];
                env.withdraw_insurance_asset(INSURER, asset as u16, amount)
                    .unwrap();
                budget.insurance_paid[asset] = amount;
                budget.check(&env, config, supply);
            }
            assert_eq!(env.token_amount(env.vault), 0);
            let tokens = env.all_token_account_data();
            if let Some(expected) = &endpoint {
                assert_eq!(&tokens, expected);
            } else {
                endpoint = Some(tokens);
            }
            worlds += 1;
        }
    }
    assert_eq!((worlds, evidence.rollbacks), (4, 4));
    eprintln!("INV-014 retained taker aggregate cap: worlds={worlds}, exact_rollbacks={}, measured_successes={}, success_cu={}, rejection_cu={}", evidence.rollbacks, evidence.successes, evidence.success_cu, evidence.rejection_cu);
}
