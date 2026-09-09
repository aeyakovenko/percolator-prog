//! INV-008 / row 415: a-value-withdrawal-intent-cannot-spend-stock-created-after-first-execution.
//! Passive maintenance rewards create recipient capital without owner consent or a sequence
//! advance. This differs from the indexed owner-signed redeposit history and does not exercise
//! WithdrawInsuranceAsset or the stock-sequence production fixes owned by PRs #428/#415.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const INITIAL: [u64; 3] = [2_003, 4_007, 7];
const CREATED: [u64; 3] = [0, 1, 3];
const SLOT: u64 = 3;
const RATE: u64 = 11;
const SUBJECT: usize = 2;
const AMOUNT: u64 = INITIAL[SUBJECT];

#[derive(Default)]
struct Evidence {
    transactions: usize,
    rejections: usize,
    simulations: usize,
    max_cu: [u64; 3], // withdrawal, passive credit, rejecting transaction
}

fn signed(
    env: &V16CuEnv,
    owner: Option<&Keypair>,
    instructions: &[Instruction],
    envelope: u32,
) -> Transaction {
    let mut message = vec![
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(1_400_000 - envelope),
    ];
    message.extend_from_slice(instructions);
    let mut signers = vec![&env.payer];
    signers.extend(owner);
    let tx = Transaction::new_signed_with_payer(
        &message,
        Some(&env.payer.pubkey()),
        &signers,
        env.svm.latest_blockhash(),
    );
    tx.verify().expect("valid signature-distinct envelope");
    tx
}

fn checked_send(
    env: &mut V16CuEnv,
    tx: Transaction,
    frame: &[Pubkey],
    stale_prefix: Option<usize>,
    lane: usize,
    evidence: &mut Evidence,
) {
    let keys: BTreeSet<_> = frame
        .iter()
        .chain(&tx.message.account_keys)
        .copied()
        .collect();
    let before: Vec<_> = keys
        .iter()
        .map(|key| (*key, env.svm.get_account(key)))
        .collect();
    let mut payer_after = env.svm.get_account(&env.payer.pubkey()).unwrap();
    payer_after.lamports -= FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let result = env.svm.send_transaction(tx);
    let meta = if let Some(prefix) = stale_prefix {
        let error = result.expect_err("consumed intent must reject before spending later stock");
        assert_eq!(
            error.err,
            TransactionError::InstructionError(
                (2 + prefix) as u8,
                InstructionError::Custom(PercolatorError::EngineStale as u32),
            )
        );
        assert_eq!(
            error
                .meta
                .logs
                .iter()
                .filter(|line| **line == format!("Program {} success", env.program_id))
                .count(),
            prefix,
            "the passive credit prefix really executed before rollback"
        );
        assert!(
            !error
                .meta
                .logs
                .iter()
                .any(|line| line.starts_with(&format!("Program {} invoke", spl_token::ID))),
            "stale withdrawals stop before SPL CPI, even after a reward prefix"
        );
        for (key, account) in before {
            if key != env.payer.pubkey() {
                assert_eq!(
                    env.svm.get_account(&key),
                    account,
                    "exact rollback at {key}"
                );
            }
        }
        evidence.rejections += 1;
        error.meta
    } else {
        result.expect("current public intent remains live")
    };
    assert_eq!(
        env.svm.get_account(&env.payer.pubkey()).unwrap(),
        payer_after
    );
    assert!(meta.compute_units_consumed > 0);
    assert_cu_within(
        "INV-008 passive stock",
        meta.compute_units_consumed,
        CUSTODY_CU_LIMIT,
    );
    evidence.max_cu[lane] = evidence.max_cu[lane].max(meta.compute_units_consumed);
    evidence.transactions += 1;
}

#[test]
fn v16_program_consumed_withdrawal_cannot_spend_passively_created_reward_stock() {
    let mut evidence = Evidence::default();
    let mut worlds = 0;
    for share in [3_333u16, 10_000] {
        for order in [[0, 1], [1, 0]] {
            for pay_each in [false, true] {
                let context = format!("share={share}, sources={order:?}, pay_each={pay_each}");
                let mut env = inv018_public_spl_market_with_params(
                    6,
                    V16CuMarketParams {
                        maintenance_fee_per_slot: RATE.into(),
                        ..V16CuMarketParams::default()
                    },
                );
                env.update_maintenance_fee_policy_with_cu(share);
                let owners: [Keypair; 3] = std::array::from_fn(|_| Keypair::new());
                let mut portfolios = [Pubkey::default(); 3];
                let mut tokens = [Pubkey::default(); 3];
                for actor in 0..3 {
                    env.svm.warp_to_slot(CREATED[actor]);
                    env.svm
                        .airdrop(&owners[actor].pubkey(), 1_000_000_000)
                        .unwrap();
                    let portfolio = Keypair::new();
                    system_create_account_for_test(
                        &mut env.svm,
                        &env.payer,
                        &portfolio,
                        env.portfolio_account_len,
                        env.program_id,
                    );
                    portfolios[actor] = portfolio.pubkey();
                    env.send(
                        ProgInstruction::InitPortfolio,
                        vec![
                            AccountMeta::new(owners[actor].pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(portfolios[actor], false),
                        ],
                        &[&owners[actor]],
                    )
                    .expect("publicly initialize System-created portfolio");
                    tokens[actor] = create_ata_for_test(
                        &mut env.svm,
                        &env.payer,
                        owners[actor].pubkey(),
                        env.mint,
                    );
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::mint_to(
                            &spl_token::ID,
                            &env.mint,
                            &tokens[actor],
                            &env.admin.pubkey(),
                            &[],
                            INITIAL[actor],
                        )
                        .unwrap(),
                        &[&env.admin],
                    )
                    .expect("finite public SPL endowment");
                    env.send(
                        env.deposit_ix(portfolios[actor], INITIAL[actor].into()),
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
                    .expect("public collateral deposit");
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
                .expect("no later minting can supply the new stock");
                let mint_before = env.svm.get_account(&env.mint).unwrap();
                let mint = Mint::unpack(&mint_before.data).unwrap();
                assert_eq!(mint.supply, INITIAL.iter().sum::<u64>());
                assert_eq!(mint.mint_authority, COption::None);
                let controls = env.control_sequences(0);
                let ids = portfolios.map(|key| env.portfolio_id(key));
                let epochs = portfolios.map(|key| env.portfolio_position_epoch(key));
                let owner_accounts = owners
                    .each_ref()
                    .map(|owner| env.svm.get_account(&owner.pubkey()));
                let frame: Vec<_> = portfolios
                    .into_iter()
                    .chain(tokens)
                    .chain(owners.iter().map(Signer::pubkey))
                    .chain([env.market, env.vault, env.mint, env.admin.pubkey()])
                    .collect();
                let withdrawal = |sequence: u64, amount: u64| Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(owners[SUBJECT].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[SUBJECT], false),
                        AccountMeta::new(tokens[SUBJECT], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: ProgInstruction::Withdraw {
                        portfolio_id: ids[SUBJECT],
                        expected_sequence: sequence,
                        amount: amount.into(),
                    }
                    .encode(),
                };
                let retained_ix = withdrawal(1, AMOUNT);
                let credit = [0, 1].map(|source| Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[source], false),
                        AccountMeta::new(portfolios[SUBJECT], false),
                    ],
                    data: ProgInstruction::SyncMaintenanceFee { now_slot: SLOT }.encode(),
                });
                // Freeze all original withdrawal signatures before the first economic execution.
                let retained: Vec<_> = (1..=5)
                    .map(|nonce| {
                        signed(&env, Some(&owners[SUBJECT]), &[retained_ix.clone()], nonce)
                    })
                    .collect();
                assert_eq!(
                    retained
                        .iter()
                        .map(|tx| tx.signatures[0])
                        .collect::<BTreeSet<_>>()
                        .len(),
                    5
                );
                for tx in &retained {
                    env.svm
                        .simulate_transaction(tx.clone().into())
                        .expect("every retained variant is initially executable");
                    evidence.simulations += 1;
                }
                let bundles: Vec<_> = (0..2)
                    .flat_map(|source| {
                        [false, true].map(|credit_first| {
                            let ixs = if credit_first {
                                vec![credit[source].clone(), retained_ix.clone()]
                            } else {
                                vec![retained_ix.clone(), credit[source].clone()]
                            };
                            signed(
                                &env,
                                Some(&owners[SUBJECT]),
                                &ixs,
                                10 + (source * 2 + usize::from(credit_first)) as u32,
                            )
                        })
                    })
                    .collect();
                let fresh_ix = withdrawal(2, AMOUNT);
                let check = |env: &V16CuEnv, charged: [u64; 2], paid: u64, withdrawals: u64| {
                    let rewards: u64 = charged
                        .iter()
                        .map(|fee| fee * u64::from(share) / 10_000)
                        .sum();
                    let expected = [
                        INITIAL[0] - charged[0],
                        INITIAL[1] - charged[1],
                        INITIAL[2] + rewards - paid,
                    ];
                    for actor in 0..3 {
                        let p = env.portfolio_state(portfolios[actor]);
                        assert_eq!(
                            p.capital.get(),
                            u128::from(expected[actor]),
                            "{context}: actor {actor}"
                        );
                        assert_eq!(p.pnl.get(), 0);
                        assert!(p.active_bitmap.iter().all(|word| word.get() == 0));
                        assert_eq!(env.portfolio_id(portfolios[actor]), ids[actor]);
                        assert_eq!(
                            env.portfolio_position_epoch(portfolios[actor]),
                            epochs[actor]
                        );
                        assert_eq!(
                            env.portfolio_matcher_sequence(portfolios[actor]),
                            1 + if actor == SUBJECT { withdrawals } else { 0 }
                        );
                        let fee_slot = if actor < 2 && charged[actor] > 0 {
                            SLOT
                        } else {
                            CREATED[actor]
                        };
                        assert_eq!(p.last_fee_slot.get(), fee_slot);
                        assert_eq!(
                            env.token_amount(tokens[actor]),
                            if actor == SUBJECT { paid } else { 0 }
                        );
                        assert_eq!(
                            env.svm.get_account(&owners[actor].pubkey()),
                            owner_accounts[actor]
                        );
                    }
                    let group = env.market_state().1;
                    let insurance = charged.iter().sum::<u64>() - rewards;
                    assert_eq!(group.mode, MarketModeV16::Live);
                    assert_eq!(group.c_tot, u128::from(expected.iter().sum::<u64>()));
                    assert_eq!(group.insurance, u128::from(insurance));
                    assert_eq!(group.pnl_pos_tot, 0);
                    assert_eq!(group.vault, group.c_tot + group.insurance);
                    assert_eq!(env.token_amount(env.vault), mint.supply - paid);
                    assert_eq!(group.vault, u128::from(mint.supply - paid));
                    assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint_before);
                    assert_eq!(env.control_sequences(0), controls);
                    assert_domain_budget_remaining_total_consistent(&group, &context);
                };
                let mut charged = [0; 2];
                let mut paid = 0;
                let mut withdrawals = 0;
                check(&env, charged, paid, withdrawals);
                checked_send(
                    &mut env,
                    retained[0].clone(),
                    &frame,
                    None,
                    0,
                    &mut evidence,
                );
                paid += AMOUNT;
                withdrawals += 1;
                check(&env, charged, paid, withdrawals);
                assert_eq!(env.portfolio_state(portfolios[SUBJECT]).capital.get(), 0);
                checked_send(
                    &mut env,
                    retained[1].clone(),
                    &frame,
                    Some(0),
                    2,
                    &mut evidence,
                );
                for (step, source) in order.into_iter().enumerate() {
                    for prefix in 0..2 {
                        checked_send(
                            &mut env,
                            bundles[source * 2 + prefix].clone(),
                            &frame,
                            Some(prefix),
                            2,
                            &mut evidence,
                        );
                        check(&env, charged, paid, withdrawals);
                    }
                    let tx = signed(&env, None, &[credit[source].clone()], 20 + step as u32);
                    assert_eq!(
                        tx.message.header.num_required_signatures, 1,
                        "only the relayer signs passive creation, never either economic owner"
                    );
                    let custody = tokens
                        .into_iter()
                        .chain([env.vault, env.mint])
                        .map(|key| (key, env.svm.get_account(&key)))
                        .collect::<Vec<_>>();
                    checked_send(&mut env, tx, &frame, None, 1, &mut evidence);
                    charged[source] = (SLOT - CREATED[source]) * RATE;
                    check(&env, charged, paid, withdrawals);
                    for (key, before) in custody {
                        assert_eq!(
                            env.svm.get_account(&key),
                            before,
                            "passive credit does not replenish custody"
                        );
                    }
                    assert!(
                        env.portfolio_state(portfolios[SUBJECT]).capital.get() >= AMOUNT.into()
                    );
                    let fresh = Instruction {
                        data: ProgInstruction::Withdraw {
                            portfolio_id: ids[SUBJECT],
                            expected_sequence: 1 + withdrawals,
                            amount: AMOUNT.into(),
                        }
                        .encode(),
                        ..fresh_ix.clone()
                    };
                    let fresh = signed(&env, Some(&owners[SUBJECT]), &[fresh], 30 + step as u32);
                    env.svm.simulate_transaction(fresh.clone().into())
                        .expect("same amount, owner, incarnation, destination and stock are payable with current consent");
                    evidence.simulations += 1;
                    checked_send(
                        &mut env,
                        retained[2 + step].clone(),
                        &frame,
                        Some(0),
                        2,
                        &mut evidence,
                    );
                    check(&env, charged, paid, withdrawals);
                    if pay_each || step == 1 {
                        checked_send(&mut env, fresh, &frame, None, 0, &mut evidence);
                        paid += AMOUNT;
                        withdrawals += 1;
                        check(&env, charged, paid, withdrawals);
                    }
                }
                let rewards = charged
                    .iter()
                    .map(|fee| fee * u64::from(share) / 10_000)
                    .sum::<u64>();
                let remaining = INITIAL[SUBJECT] + rewards - paid;
                assert!(remaining > 0);
                let drain = Instruction {
                    data: ProgInstruction::Withdraw {
                        portfolio_id: ids[SUBJECT],
                        expected_sequence: 1 + withdrawals,
                        amount: remaining.into(),
                    }
                    .encode(),
                    ..fresh_ix
                };
                let drain = signed(&env, Some(&owners[SUBJECT]), &[drain], 40);
                checked_send(&mut env, drain, &frame, None, 0, &mut evidence);
                paid += remaining;
                withdrawals += 1;
                check(&env, charged, paid, withdrawals);
                checked_send(
                    &mut env,
                    retained[4].clone(),
                    &frame,
                    Some(0),
                    2,
                    &mut evidence,
                );
                check(&env, charged, paid, withdrawals);
                assert_eq!(
                    paid,
                    AMOUNT + rewards,
                    "only fresh intents pay the later rewards"
                );
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 8);
    assert_eq!(evidence.rejections, 64);
    assert_eq!(evidence.simulations, 56);
    assert_eq!(evidence.transactions, 108);
    eprintln!("INV-008 passive stock: {worlds} worlds, {} public transactions, {} exact stale rollbacks, {} live simulations; peak CU withdrawal/credit/rejection={:?}",
        evidence.transactions, evidence.rejections, evidence.simulations, evidence.max_cu);
}
