//! INV-008 / row415: co-owned portfolios retain separate withdrawal budgets.
//! Two original requests share signer, destination, amount and sequence. Passive
//! rewards replenish both after one pays; its stale retry must neither acquire
//! new stock nor consume the sibling's still-unexecuted request on rollback.
//! Finite public-SBF conformance, not a generic withdrawal-stock oracle.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const SLOT: u64 = 2;
const RATE: u64 = 41;
const BUNDLE_CU_LIMIT: u64 = 400_000;
const BIRTHS: [u64; 5] = [0, 1, SLOT, SLOT, SLOT];
const WALLET: [usize; 5] = [0, 1, 2, 2, 3];
const REWARDS: [u64; 2] = [(SLOT - BIRTHS[0]) * RATE, (SLOT - BIRTHS[1]) * RATE];

#[derive(Clone, Copy)]
enum Action {
    Original(usize),
    Credit(usize),
    Fresh(usize),
}

#[derive(Default)]
struct Book {
    credited: [bool; 2],
    paid: [u64; 2],
    withdrawals: [u64; 2],
}

#[test]
fn v16_coowned_withdrawals_preserve_separate_budgets_after_passive_replenishment() {
    let mut transactions = 0;
    let mut rejections = 0;
    let mut rolled_back_payouts = 0;
    let mut peak_cu = 0;
    for amount in [1u64, 37] {
        for first in 0..2 {
            for order in [[0, 1], [1, 0]] {
                let other = 1 - first;
                let context = format!("amount={amount}, first={first}, credit order={order:?}");
                let principals = [503, 601, amount, amount, 103];
                let endowments = [503, 601, 2 * amount, 103];
                let supply = endowments.iter().sum::<u64>();
                let mut env = inv018_public_spl_market_with_params(
                    6,
                    V16CuMarketParams {
                        maintenance_fee_per_slot: RATE.into(),
                        ..V16CuMarketParams::default()
                    },
                );
                env.update_maintenance_fee_policy_with_cu(10_000);
                let owners: [Keypair; 4] = std::array::from_fn(|_| Keypair::new());
                let tokens: [Pubkey; 4] = std::array::from_fn(|wallet| {
                    env.svm
                        .airdrop(&owners[wallet].pubkey(), 1_000_000_000)
                        .unwrap();
                    let token = create_ata_for_test(
                        &mut env.svm,
                        &env.payer,
                        owners[wallet].pubkey(),
                        env.mint,
                    );
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::mint_to(
                            &spl_token::ID,
                            &env.mint,
                            &token,
                            &env.admin.pubkey(),
                            &[],
                            endowments[wallet],
                        )
                        .unwrap(),
                        &[&env.admin],
                    )
                    .unwrap();
                    token
                });
                let portfolios: [Pubkey; 5] = std::array::from_fn(|actor| {
                    env.svm.warp_to_slot(BIRTHS[actor]);
                    let key = Keypair::new();
                    system_create_account_for_test(
                        &mut env.svm,
                        &env.payer,
                        &key,
                        env.portfolio_account_len,
                        env.program_id,
                    );
                    let owner = &owners[WALLET[actor]];
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
                    env.send(
                        env.deposit_ix(key.pubkey(), principals[actor].into()),
                        vec![
                            AccountMeta::new(owner.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(key.pubkey(), false),
                            AccountMeta::new(tokens[WALLET[actor]], false),
                            AccountMeta::new(env.vault, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[owner],
                    )
                    .unwrap();
                    key.pubkey()
                });
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
                let ids = portfolios.map(|key| env.portfolio_id(key));
                assert_ne!(ids[2], ids[3]);
                let epochs = portfolios.map(|key| env.portfolio_position_epoch(key));
                let controls = env.control_sequences(0);
                let mint_frame = env.svm.get_account(&env.mint).unwrap();
                let mint = Mint::unpack(&mint_frame.data).unwrap();
                assert_eq!(mint.supply, supply);
                assert_eq!(mint.mint_authority, COption::None);
                let frame: Vec<_> = portfolios
                    .into_iter()
                    .chain(tokens)
                    .chain(owners.iter().map(Signer::pubkey))
                    .chain([
                        env.market,
                        env.vault,
                        env.mint,
                        env.vault_authority,
                        env.admin.pubkey(),
                    ])
                    .collect();
                let untouched: Vec<_> = owners
                    .iter()
                    .map(Signer::pubkey)
                    .chain([
                        portfolios[4],
                        tokens[0],
                        tokens[1],
                        tokens[3],
                        env.admin.pubkey(),
                    ])
                    .map(|key| (key, env.svm.get_account(&key)))
                    .collect();
                let check = |env: &V16CuEnv, book: &Book| {
                    let rewards = std::array::from_fn::<_, 2, _>(|i| {
                        u64::from(book.credited[i]) * REWARDS[i]
                    });
                    let capital = [
                        principals[0] - rewards[0],
                        principals[1] - rewards[1],
                        amount + rewards[0] - book.paid[0],
                        amount + rewards[1] - book.paid[1],
                        principals[4],
                    ];
                    for subject in 0..2 {
                        assert_eq!(
                            book.paid[subject],
                            match book.withdrawals[subject] {
                                0 => 0,
                                1 => amount,
                                2 => amount + REWARDS[subject],
                                _ => panic!("no additional consent exists"),
                            }
                        );
                    }
                    for actor in 0..5 {
                        let p = env.portfolio_state(portfolios[actor]);
                        assert_eq!(p.capital.get(), u128::from(capital[actor]), "{context}");
                        assert_eq!(p.pnl.get(), 0);
                        assert_eq!(p.reserved_pnl.get(), 0);
                        assert!(p.active_bitmap.iter().all(|word| word.get() == 0));
                        assert_eq!(env.portfolio_id(portfolios[actor]), ids[actor]);
                        assert_eq!(
                            env.portfolio_position_epoch(portfolios[actor]),
                            epochs[actor]
                        );
                        assert_eq!(
                            env.portfolio_matcher_sequence(portfolios[actor]),
                            1 + if (2..4).contains(&actor) {
                                book.withdrawals[actor - 2]
                            } else {
                                0
                            }
                        );
                        assert_eq!(
                            p.last_fee_slot.get(),
                            if actor < 2 && book.credited[actor] {
                                SLOT
                            } else {
                                BIRTHS[actor]
                            }
                        );
                    }
                    let paid = book.paid.iter().sum::<u64>();
                    let group = env.market_state().1;
                    assert_eq!(group.mode, MarketModeV16::Live);
                    assert_eq!(group.materialized_portfolio_count, 5);
                    assert_eq!(group.c_tot, u128::from(capital.iter().sum::<u64>()));
                    assert_eq!(group.vault, u128::from(supply - paid));
                    assert_eq!(group.c_tot, group.vault);
                    assert_eq!(group.insurance, 0);
                    assert_eq!(group.pnl_pos_tot, 0);
                    assert_eq!(group.source_claim_bound_total_num, 0);
                    assert!(group
                        .insurance_domain_budget
                        .iter()
                        .all(|budget| *budget == 0));
                    assert_eq!(env.token_amount(env.vault), supply - paid);
                    assert_eq!(env.token_amount(tokens[2]), paid);
                    assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint_frame);
                    assert_eq!(env.control_sequences(0), controls);
                    assert_domain_budget_remaining_total_consistent(&group, &context);
                    for (key, account) in &untouched {
                        assert_eq!(env.svm.get_account(key), *account, "untouched {key}");
                    }
                };
                let withdrawal = |subject: usize, sequence, atoms: u64| Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(owners[2].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[subject + 2], false),
                        AccountMeta::new(tokens[2], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: ProgInstruction::Withdraw {
                        portfolio_id: ids[subject + 2],
                        expected_sequence: sequence,
                        amount: atoms.into(),
                    }
                    .encode(),
                };
                let original = [withdrawal(0, 1, amount), withdrawal(1, 1, amount)];
                let fresh = [withdrawal(0, 2, REWARDS[0]), withdrawal(1, 2, REWARDS[1])];
                let credits = [0, 1].map(|subject| Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[subject], false),
                        AccountMeta::new(portfolios[subject + 2], false),
                    ],
                    data: ProgInstruction::SyncMaintenanceFee { now_slot: SLOT }.encode(),
                });
                use Action::{Credit, Fresh, Original};
                // All delivery alternatives are signed before the first payment. Only
                // the CU envelope varies; no helper refreshes economic bytes or metas.
                let steps = [
                    (
                        vec![
                            Original(first),
                            Credit(order[0]),
                            Credit(order[1]),
                            Original(other),
                            Original(first),
                        ],
                        Some(4),
                    ),
                    (vec![Original(first)], None),
                    (vec![Original(first)], Some(0)),
                    (
                        vec![
                            Credit(order[0]),
                            Credit(order[1]),
                            Original(other),
                            Original(first),
                        ],
                        Some(3),
                    ),
                    (vec![Credit(order[0])], None),
                    (vec![Credit(order[1]), Original(first)], Some(1)),
                    (vec![Credit(order[1])], None),
                    (vec![Original(first)], Some(0)),
                    (vec![Original(other), Original(first)], Some(1)),
                    (vec![Original(other)], None),
                    (vec![Original(other)], Some(0)),
                    (vec![Original(first)], Some(0)),
                    (
                        vec![Fresh(order[0]), Fresh(order[1]), Original(other)],
                        Some(2),
                    ),
                    (vec![Fresh(order[1])], None),
                    (vec![Fresh(order[0])], None),
                    (vec![Original(0)], Some(0)),
                    (vec![Original(1)], Some(0)),
                ];
                let deliveries: Vec<_> = steps
                    .iter()
                    .enumerate()
                    .map(|(nonce, (actions, _))| {
                        let mut ixs = vec![
                            heap_ix(),
                            ComputeBudgetInstruction::set_compute_unit_limit(
                                1_400_000 - nonce as u32,
                            ),
                        ];
                        ixs.extend(actions.iter().map(|action| match *action {
                            Original(subject) => original[subject].clone(),
                            Credit(subject) => credits[subject].clone(),
                            Fresh(subject) => fresh[subject].clone(),
                        }));
                        let mut signers = vec![&env.payer];
                        if actions.iter().any(|action| !matches!(action, Credit(_))) {
                            signers.push(&owners[2]);
                        }
                        let tx = Transaction::new_signed_with_payer(
                            &ixs,
                            Some(&env.payer.pubkey()),
                            &signers,
                            env.svm.latest_blockhash(),
                        );
                        tx.verify().unwrap();
                        tx
                    })
                    .collect();
                assert_eq!(
                    deliveries
                        .iter()
                        .map(|tx| tx.signatures[0])
                        .collect::<BTreeSet<_>>()
                        .len(),
                    steps.len()
                );
                let retained_bytes: Vec<_> = deliveries
                    .iter()
                    .map(|tx| bincode::serialize(tx).unwrap())
                    .collect();
                let mut book = Book::default();
                check(&env, &book);
                for (index, ((actions, failing), tx)) in steps.iter().zip(&deliveries).enumerate() {
                    assert_eq!(bincode::serialize(tx).unwrap(), retained_bytes[index]);
                    let keys: BTreeSet<_> = frame
                        .iter()
                        .chain(&tx.message.account_keys)
                        .copied()
                        .collect();
                    let before: Vec<_> = keys
                        .iter()
                        .map(|key| (*key, env.svm.get_account(key)))
                        .collect();
                    let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
                    payer.lamports -= FeeStructure::default().lamports_per_signature
                        * u64::from(tx.message.header.num_required_signatures);
                    let result = env.svm.send_transaction(tx.clone());
                    let meta = if let Some(prefix) = failing {
                        let error = result.expect_err("consumed per-portfolio consent must reject");
                        assert_eq!(
                            error.err,
                            TransactionError::InstructionError(
                                (2 + prefix) as u8,
                                InstructionError::Custom(PercolatorError::EngineStale as u32)
                            ),
                            "{context}, step={index}"
                        );
                        for (key, account) in before {
                            if key != env.payer.pubkey() {
                                assert_eq!(
                                    env.svm.get_account(&key),
                                    account,
                                    "{context}, step={index}, full rollback at {key}"
                                );
                            }
                        }
                        rejections += 1;
                        rolled_back_payouts += actions[..*prefix]
                            .iter()
                            .filter(|action| !matches!(action, Credit(_)))
                            .count();
                        error.meta
                    } else {
                        let meta = result.expect("unconsumed per-portfolio consent remains live");
                        for (key, account) in &before {
                            if portfolios.contains(key) && !tx.message.account_keys.contains(key) {
                                assert_eq!(
                                    env.svm.get_account(key),
                                    *account,
                                    "unmentioned portfolio {key}"
                                );
                            }
                            if actions.iter().all(|action| matches!(action, Credit(_)))
                                && (tokens.contains(key) || *key == env.vault || *key == env.mint)
                            {
                                assert_eq!(
                                    env.svm.get_account(key),
                                    *account,
                                    "passive credit preserves SPL custody at {key}"
                                );
                            }
                        }
                        for action in actions {
                            match *action {
                                Original(subject) => {
                                    assert_eq!(book.withdrawals[subject], 0);
                                    book.paid[subject] += amount;
                                    book.withdrawals[subject] += 1;
                                }
                                Credit(subject) => {
                                    assert!(!book.credited[subject]);
                                    book.credited[subject] = true;
                                }
                                Fresh(subject) => {
                                    assert!(book.credited[subject]);
                                    assert_eq!(book.withdrawals[subject], 1);
                                    book.paid[subject] += REWARDS[subject];
                                    book.withdrawals[subject] += 1;
                                }
                            }
                        }
                        meta
                    };
                    let prefix = failing.unwrap_or(actions.len());
                    let spl = actions[..prefix]
                        .iter()
                        .filter(|action| !matches!(action, Credit(_)))
                        .count();
                    for (program, count) in [(env.program_id, prefix), (spl_token::ID, spl)] {
                        assert_eq!(
                            meta.logs
                                .iter()
                                .filter(|line| **line == format!("Program {program} success"))
                                .count(),
                            count,
                            "{context}, step={index}: exact executed prefix"
                        );
                    }
                    assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
                    assert!(meta.compute_units_consumed > 0);
                    assert_cu_within(
                        "INV-008 co-owned stock",
                        meta.compute_units_consumed,
                        if actions.len() == 1 {
                            CUSTODY_CU_LIMIT
                        } else {
                            BUNDLE_CU_LIMIT
                        },
                    );
                    peak_cu = peak_cu.max(meta.compute_units_consumed);
                    transactions += 1;
                    check(&env, &book);
                }
                assert_eq!(book.withdrawals, [2, 2]);
                assert_eq!(book.paid, [amount + REWARDS[0], amount + REWARDS[1]]);
                assert_eq!(
                    env.token_amount(tokens[2]),
                    2 * amount + REWARDS.iter().sum::<u64>()
                );
                assert_eq!(env.token_amount(env.vault), 1084);
            }
        }
    }
    assert_eq!(transactions, 136);
    assert_eq!(rejections, 88);
    assert_eq!(rolled_back_payouts, 48);
    eprintln!("INV-008 co-owned stock: 8 worlds, {transactions} transactions, {rejections} full rollbacks, {rolled_back_payouts} rolled-back SPL payouts, 32 committed payouts, peak CU={peak_cu}");
}
