//! INV-003/005/010/024/031/080/081: independent retained debit budgets survive
//! rollback when one portfolio incarnation or reserve authority changes.
//! All permutations put each invalidated family before, between and after the
//! other families. Recovery reuses the two unaffected signed transactions.

use crate::support::{
    fuzz_model::{assert_public_encumbrance_census, assert_public_stock_census},
    v16_svm::{MarketConfig, V16Svm, TX_CU_LIMIT},
};
use percolator::BOUND_SCALE;
use percolator_prog::{
    error::PercolatorError, ix::Instruction as ProgInstruction, processor, state,
};
use solana_sdk::{
    account::Account,
    compute_budget::ComputeBudgetInstruction,
    fee::FeeStructure,
    instruction::{AccountMeta, Instruction, InstructionError},
    pubkey::Pubkey,
    signature::Signer,
    transaction::{Transaction, TransactionError},
};
use std::collections::BTreeSet;

const STOCK: [u128; 3] = [101, 211, 307];
const INSURANCE_ASSET: u16 = 1;
const INSURANCE_DOMAIN: u16 = 2;
const BACKING_ASSET: u16 = 2;
const BACKING_DOMAIN: u16 = 5;
const SUCCESSOR: usize = 3;
const PAYER: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Debit {
    Portfolio,
    Insurance,
    Backing,
}

impl Debit {
    const ALL: [Self; 3] = [Self::Portfolio, Self::Insurance, Self::Backing];

    fn index(self) -> usize {
        match self {
            Self::Portfolio => 0,
            Self::Insurance => 1,
            Self::Backing => 2,
        }
    }

    fn binding_error(self) -> PercolatorError {
        match self {
            Self::Portfolio => PercolatorError::EngineProvenanceMismatch,
            Self::Insurance | Self::Backing => PercolatorError::EngineStale,
        }
    }

    fn instruction(self, env: &V16Svm, amount: u128) -> Instruction {
        let actor = &env.actors[self.index()];
        let mut accounts = vec![
            AccountMeta::new(actor.signer.pubkey(), true),
            AccountMeta::new(env.market, false),
        ];
        if self == Self::Portfolio {
            accounts.push(AccountMeta::new(actor.portfolio, false));
        }
        accounts.extend([
            AccountMeta::new(actor.destination_token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ]);
        let group = env.primary_market_state().1;
        let ix = match self {
            Self::Portfolio => ProgInstruction::Withdraw {
                portfolio_id: env.primary_portfolio_id(0),
                expected_sequence: env.primary_portfolio_matcher_sequence(0),
                amount,
            },
            Self::Insurance => ProgInstruction::WithdrawInsuranceAsset {
                asset_index: INSURANCE_ASSET,
                market_id: group.assets[INSURANCE_ASSET as usize].market_id,
                authority_epoch: env
                    .primary_control_sequences(INSURANCE_ASSET as usize)
                    .authority_epoch,
                amount,
            },
            Self::Backing => {
                accounts.push(AccountMeta::new(env.backing_domain_ledger, false));
                ProgInstruction::WithdrawBackingBucket {
                    domain: BACKING_DOMAIN,
                    market_id: group.assets[BACKING_ASSET as usize].market_id,
                    authority_epoch: env
                        .primary_control_sequences(BACKING_ASSET as usize)
                        .authority_epoch,
                    amount,
                }
            }
        };
        Instruction {
            program_id: env.program_id,
            accounts,
            data: ix.encode(),
        }
    }

    fn replace_binding(self, env: &mut V16Svm) {
        if self == Self::Portfolio {
            let old_id = env.primary_portfolio_id(0);
            let old_sequence = env.primary_portfolio_matcher_sequence(0);
            env.withdraw_primary(0, STOCK[0]).unwrap();
            env.close_primary_portfolio(0).unwrap();
            env.fund_closed_primary_portfolio(0, 1_000_000_000).unwrap();
            env.reinitialize_primary_portfolio(0).unwrap();
            // Match the old scalar sequence through a signed control, so only
            // portfolio identity distinguishes the retained withdrawal.
            env.set_matcher_config(0, 0).unwrap();
            env.deposit_primary(0, STOCK[0]).unwrap();
            assert!(env.primary_portfolio_id(0) > old_id);
            assert_eq!(env.primary_portfolio_matcher_sequence(0), old_sequence);
        } else {
            let (asset, role) = match self {
                Self::Insurance => (INSURANCE_ASSET, processor::ASSET_AUTH_INSURANCE_OPERATOR),
                Self::Backing => (BACKING_ASSET, processor::ASSET_AUTH_BACKING_BUCKET),
                Self::Portfolio => unreachable!(),
            };
            let before = env.primary_control_sequences(asset as usize);
            let profile = env.primary_profile(asset as usize);
            for (from, to) in [(self.index(), SUCCESSOR), (SUCCESSOR, self.index())] {
                env.update_asset_authority_between_actors(asset, role, from, to)
                    .expect("incumbent and successor consent to each funded handoff");
            }
            let mut expected = before;
            expected.authority_epoch += 2;
            assert_eq!(env.primary_control_sequences(asset as usize), expected);
            assert_eq!(env.primary_profile(asset as usize), profile);
        }
    }
}

fn sign(env: &V16Svm, instructions: &[Instruction], nonce: u32) -> Transaction {
    let mut message = vec![
        ComputeBudgetInstruction::request_heap_frame(256 * 1024),
        ComputeBudgetInstruction::set_compute_unit_limit(TX_CU_LIMIT as u32 - nonce),
    ];
    message.extend_from_slice(instructions);
    let mut signers = vec![&env.actors[PAYER].signer];
    for actor in &env.actors[..PAYER] {
        if instructions
            .iter()
            .flat_map(|ix| &ix.accounts)
            .any(|meta| meta.is_signer && meta.pubkey == actor.signer.pubkey())
        {
            signers.push(&actor.signer);
        }
    }
    let tx = Transaction::new_signed_with_payer(
        &message,
        Some(&signers[0].pubkey()),
        &signers,
        env.svm.latest_blockhash(),
    );
    tx.verify()
        .expect("retained public message has valid signatures");
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
    rejections: usize,
    rolled_back_transfers: usize,
    payments: usize,
    peak_cu: u64,
}

fn deliver(
    env: &mut V16Svm,
    tx: Transaction,
    failure: Option<(usize, Debit)>,
    evidence: &mut Evidence,
) {
    let before = frame(env, &tx);
    let present = |key: Pubkey| tx.message.account_keys.contains(&key);
    let mutable: BTreeSet<_> = [
        env.market,
        env.vault,
        env.backing_domain_ledger,
        env.actors[0].portfolio,
    ]
    .into_iter()
    .chain(env.actors[..3].iter().map(|actor| actor.destination_token))
    .filter(|key| present(*key))
    .collect();
    let payer = env.actors[PAYER].signer.pubkey();
    let mut expected_payer = env.svm.get_account(&payer).unwrap();
    expected_payer.lamports -= FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let result = env.svm.send_transaction(tx);
    let meta = if let Some((index, kind)) = failure {
        let failure = result.expect_err("one invalid binding aborts every debit in the bundle");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(
                (index + 2) as u8,
                InstructionError::Custom(kind.binding_error() as u32),
            )
        );
        evidence.rejections += 1;
        evidence.rolled_back_transfers += index;
        failure.meta
    } else {
        evidence.payments += 1;
        result.expect("unconsumed signed consent pays only its own beneficiary")
    };
    for (key, account) in before {
        if key != payer && (failure.is_some() || !mutable.contains(&key)) {
            assert_eq!(
                env.svm.get_account(&key),
                account,
                "complete Account frame at {key}"
            );
        }
    }
    let successful_transfers = failure.map(|(index, _)| index).unwrap_or(1);
    for program in [env.program_id, spl_token::ID] {
        assert_eq!(
            meta.logs
                .iter()
                .filter(|line| **line == format!("Program {program} success"))
                .count(),
            successful_transfers,
            "the expected prefix executes and no consumer after a failure executes"
        );
    }
    assert_eq!(env.svm.get_account(&payer), Some(expected_payer));
    assert!(meta.compute_units_consumed > 0 && meta.compute_units_consumed <= TX_CU_LIMIT);
    evidence.peak_cu = evidence.peak_cu.max(meta.compute_units_consumed);
}

fn assert_budgets(env: &V16Svm, paid: [u128; 3], prior_portfolio_exit: u128) {
    let remaining = std::array::from_fn::<_, 3, _>(|i| STOCK[i] - paid[i]);
    let group = env.primary_market_state().1;
    assert_eq!(env.primary_portfolio(0).capital.get(), remaining[0]);
    assert_eq!(group.c_tot, remaining[0]);
    assert_eq!(group.insurance, remaining[1]);
    for (domain, budget) in group.insurance_domain_budget.iter().enumerate() {
        assert_eq!(
            *budget,
            if domain == INSURANCE_DOMAIN as usize {
                remaining[1]
            } else {
                0
            }
        );
    }
    let bucket = &group.source_backing_buckets[BACKING_DOMAIN as usize];
    assert_eq!(
        bucket.fresh_unliened_backing_num,
        remaining[2] * BOUND_SCALE
    );
    let ledger = state::read_backing_domain_ledger(&env.backing_domain_ledger_data()).unwrap();
    assert_eq!(ledger.authority, env.actors[2].signer.pubkey().to_bytes());
    assert_eq!(ledger.domain, BACKING_DOMAIN);
    assert_eq!(ledger.total_deposited_atoms, STOCK[2]);
    assert_eq!(ledger.total_principal_atoms, remaining[2]);
    assert_eq!(ledger.total_principal_withdrawn_atoms, paid[2]);
    assert_eq!(group.vault, remaining.iter().sum::<u128>());
    assert_eq!(u128::from(env.token_amount(env.vault)), group.vault);
    for (actor, owner) in env.actors.iter().enumerate() {
        let expected = paid.get(actor).copied().unwrap_or(0)
            + if actor == 0 { prior_portfolio_exit } else { 0 };
        assert_eq!(
            u128::from(env.token_amount(owner.destination_token)),
            expected
        );
        if actor > 0 {
            assert_eq!(env.primary_portfolio(actor).capital.get(), 0);
        }
    }
    assert_eq!(env.token_supply_observed(), env.initial_token_supply);
    assert_eq!(u128::from(env.mint_supply()), env.initial_token_supply);
    assert_public_stock_census("retained mixed debit", env).unwrap();
    assert_public_encumbrance_census("retained mixed debit", env).unwrap();
}

#[test]
fn v16_program_retained_debit_permutations_preserve_independent_budgets_after_binding_changes() {
    let mut evidence = Evidence::default();
    for invalidated in Debit::ALL {
        for full in [false, true] {
            for first in Debit::ALL {
                for second in Debit::ALL.into_iter().filter(|kind| *kind != first) {
                    let third = Debit::ALL
                        .into_iter()
                        .find(|kind| *kind != first && *kind != second)
                        .unwrap();
                    let order = [first, second, third];
                    let failed_index = order.iter().position(|kind| *kind == invalidated).unwrap();
                    let mut seed = [0x5d; 32];
                    seed[0] = evidence.worlds as u8;
                    let mut env = V16Svm::new(
                        seed,
                        MarketConfig {
                            actor_deposits: [STOCK[0], 0, 0, 0, 0],
                            ..MarketConfig::default()
                        },
                    );
                    env.update_asset_authority_from_admin(
                        INSURANCE_ASSET,
                        processor::ASSET_AUTH_INSURANCE_OPERATOR,
                        1,
                    )
                    .unwrap();
                    env.update_asset_authority_from_admin(
                        INSURANCE_ASSET,
                        processor::ASSET_AUTH_INSURANCE,
                        1,
                    )
                    .unwrap();
                    env.update_asset_authority_from_admin(
                        BACKING_ASSET,
                        processor::ASSET_AUTH_BACKING_BUCKET,
                        2,
                    )
                    .unwrap();
                    env.top_up_insurance_domain_for_actor(1, INSURANCE_DOMAIN, STOCK[1])
                        .unwrap();
                    env.top_up_backing_bucket_for_actor(2, BACKING_DOMAIN, STOCK[2], 10_000)
                        .unwrap();
                    let amounts = STOCK.map(|stock| if full { stock } else { 1 });
                    let instructions =
                        Debit::ALL.map(|kind| kind.instruction(&env, amounts[kind.index()]));
                    let retained = Debit::ALL.map(|kind| {
                        sign(
                            &env,
                            &[instructions[kind.index()].clone()],
                            10 + kind.index() as u32,
                        )
                    });
                    let bundle = sign(
                        &env,
                        &order.map(|kind| instructions[kind.index()].clone()),
                        20,
                    );
                    let wires = retained
                        .each_ref()
                        .map(|tx| bincode::serialize(tx).unwrap());
                    let bundle_wire = bincode::serialize(&bundle).unwrap();
                    for tx in retained.iter().chain([&bundle]) {
                        let before = frame(&env, tx);
                        let simulation = env.svm.simulate_transaction(tx.clone().into()).expect(
                            "every retained debit and the whole bundle are initially executable",
                        );
                        assert_eq!(
                            frame(&env, tx),
                            before,
                            "simulation consumes no consent or stock"
                        );
                        evidence.peak_cu = evidence.peak_cu.max(simulation.compute_units_consumed);
                        evidence.simulations += 1;
                    }
                    assert_budgets(&env, [0; 3], 0);
                    invalidated.replace_binding(&mut env);
                    let prior_exit = if invalidated == Debit::Portfolio {
                        STOCK[0]
                    } else {
                        0
                    };
                    assert_budgets(&env, [0; 3], prior_exit);
                    assert_eq!(bincode::serialize(&bundle).unwrap(), bundle_wire);
                    deliver(
                        &mut env,
                        bundle,
                        Some((failed_index, invalidated)),
                        &mut evidence,
                    );
                    assert_budgets(&env, [0; 3], prior_exit);
                    // A distinct original envelope makes application rejection observable independently
                    // of transaction signature deduplication, including when the failed item was first.
                    let old = retained[invalidated.index()].clone();
                    assert_eq!(
                        bincode::serialize(&old).unwrap(),
                        wires[invalidated.index()]
                    );
                    deliver(&mut env, old, Some((0, invalidated)), &mut evidence);
                    let fresh = sign(
                        &env,
                        &[invalidated.instruction(&env, amounts[invalidated.index()])],
                        30,
                    );
                    let mut paid = [0; 3];
                    for kind in order {
                        let tx = if kind == invalidated {
                            fresh.clone()
                        } else {
                            let tx = retained[kind.index()].clone();
                            assert_eq!(bincode::serialize(&tx).unwrap(), wires[kind.index()]);
                            tx
                        };
                        deliver(&mut env, tx, None, &mut evidence);
                        paid[kind.index()] += amounts[kind.index()];
                        assert_budgets(&env, paid, prior_exit);
                    }
                    for kind in order.into_iter().rev() {
                        let remainder = STOCK[kind.index()] - paid[kind.index()];
                        if remainder != 0 {
                            let exit = sign(&env, &[kind.instruction(&env, remainder)], 40);
                            deliver(&mut env, exit, None, &mut evidence);
                            paid[kind.index()] += remainder;
                            assert_budgets(&env, paid, prior_exit);
                        }
                    }
                    assert_eq!(paid, STOCK);
                    evidence.worlds += 1;
                }
            }
        }
    }
    assert_eq!(evidence.worlds, 36);
    assert_eq!(evidence.simulations, 144);
    assert_eq!(evidence.rejections, 72);
    assert_eq!(evidence.rolled_back_transfers, 36);
    assert_eq!(evidence.payments, 162);
    println!("INV-005 mixed retained debits: 36 worlds, 144 live simulations, 72 exact rollbacks, 36 rolled-back SPL transfers, 162 payments, zero remaining custody; peak CU {}", evidence.peak_cu);
}
