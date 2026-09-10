//! INV-008 / row415: consumed withdrawals cannot acquire later-created stock.
//! Generated histories mix capital replenishment, custody-only replenishment, partial payouts,
//! and late transaction failure. WithdrawInsuranceAsset's open stock binding is not certified.
//! Fee-shortened withdraw-all also consumes the entire request, including when fees leave no
//! SPL payout; later passive rewards cannot fund the apparent unspent signed amount.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::inv018_public_spl_market_with_params;
use proptest::{
    prelude::*,
    test_runner::{Config, FileFailurePersistence, RngAlgorithm, TestRng, TestRunner},
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const SLOT: u64 = 8;
const RATE: u64 = 11;
const WALLET: u64 = 4_096;
const DONOR: u64 = 1_003;
const BYSTANDER: u64 = 103;
const ORDERS: [[usize; 3]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];

#[derive(Clone, Debug)]
struct Round {
    amount: u64,
    order: usize,
    refill_first: bool,
    failures: u8,
    half_payout: bool,
}

#[derive(Clone, Debug)]
struct History {
    initial: u64,
    share: u16,
    initial_failures: u8,
    rounds: Vec<Round>,
}

#[derive(Default)]
struct Stock {
    deposited: u64,
    donated: u64,
    paid: u64,
    withdrawals: u64,
    deposits: u64,
    charged: BTreeSet<usize>,
}

#[derive(Default)]
struct Evidence {
    transactions: u64,
    stale: u64,
    late_failures: u64,
    max_cu: u64,
}

fn signed(env: &V16CuEnv, owner: &Keypair, ixs: &[Instruction], nonce: u32) -> Transaction {
    let mut message = vec![
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(1_400_000 - nonce),
    ];
    message.extend_from_slice(ixs);
    let mut signers = vec![&env.payer];
    if ixs
        .iter()
        .flat_map(|ix| &ix.accounts)
        .any(|meta| meta.is_signer)
    {
        signers.push(owner);
    }
    let tx = Transaction::new_signed_with_payer(
        &message,
        Some(&env.payer.pubkey()),
        &signers,
        env.svm.latest_blockhash(),
    );
    tx.verify().expect("valid signature-distinct envelope");
    tx
}

// expected = failing application-instruction index, error, successful wrapper/SPL prefixes.
fn checked_send(
    env: &mut V16CuEnv,
    tx: Transaction,
    frame: &[Pubkey],
    expected: Option<(usize, InstructionError, [usize; 2])>,
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
    let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
    payer.lamports -= FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let result = env.svm.send_transaction(tx);
    let meta = if let Some((index, error, successes)) = expected {
        let failure = result.expect_err("rejected history step must not commit stock or payout");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError((2 + index) as u8, error)
        );
        for (program, count) in [env.program_id, spl_token::ID].into_iter().zip(successes) {
            assert_eq!(
                failure
                    .meta
                    .logs
                    .iter()
                    .filter(|line| **line == format!("Program {program} success"))
                    .count(),
                count,
                "the intended prefix, including real SPL movement, must execute before rollback"
            );
        }
        for (key, account) in before {
            if key != env.payer.pubkey() {
                assert_eq!(env.svm.get_account(&key), account, "full rollback at {key}");
            }
        }
        failure.meta
    } else {
        result.expect("current consent must make public progress")
    };
    assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
    assert!(meta.compute_units_consumed > 0);
    assert_cu_within(
        "INV-008 stock history",
        meta.compute_units_consumed,
        CUSTODY_CU_LIMIT,
    );
    evidence.max_cu = evidence.max_cu.max(meta.compute_units_consumed);
    evidence.transactions += 1;
}

#[test]
fn v16_program_fee_shortened_withdrawal_consumes_intent_before_passive_replenishment() {
    const BIRTHS: [u64; 3] = [0, 1, 1];
    const EXIT_SLOT: u64 = 3;
    const SUBJECT: usize = 1;
    const OWN_FEE: u64 = (EXIT_SLOT - BIRTHS[SUBJECT]) * RATE;
    const REWARD: u64 = (EXIT_SLOT - BIRTHS[0]) * RATE;
    let mut evidence = Evidence::default();

    for first_payout in [0, 1, 7] {
        let initial = OWN_FEE + first_payout;
        let principals = [101, initial, BYSTANDER];
        let supply = principals.iter().sum::<u64>();
        let mut env = inv018_public_spl_market_with_params(
            6,
            V16CuMarketParams {
                maintenance_fee_per_slot: RATE.into(),
                ..V16CuMarketParams::default()
            },
        );
        env.update_maintenance_fee_policy_with_cu(10_000);
        let owners: [Keypair; 3] = std::array::from_fn(|_| Keypair::new());
        let mut portfolios = [Pubkey::default(); 3];
        let mut tokens = [Pubkey::default(); 3];
        for actor in 0..3 {
            env.svm.warp_to_slot(BIRTHS[actor]);
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
            .expect("public portfolio initialization");
            tokens[actor] =
                create_ata_for_test(&mut env.svm, &env.payer, owners[actor].pubkey(), env.mint);
            send_raw_tx(
                &mut env.svm,
                &env.payer,
                spl_token::instruction::mint_to(
                    &spl_token::ID,
                    &env.mint,
                    &tokens[actor],
                    &env.admin.pubkey(),
                    &[],
                    principals[actor],
                )
                .unwrap(),
                &[&env.admin],
            )
            .unwrap();
            env.send(
                env.deposit_ix(portfolios[actor], principals[actor].into()),
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
            .expect("finite public collateral deposit");
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
        env.svm.warp_to_slot(EXIT_SLOT);
        let mint_before = env.svm.get_account(&env.mint).unwrap();
        let mint = Mint::unpack(&mint_before.data).unwrap();
        assert_eq!(mint.supply, supply);
        assert_eq!(mint.mint_authority, COption::None);
        let ids = portfolios.map(|key| env.portfolio_id(key));
        let epochs = portfolios.map(|key| env.portfolio_position_epoch(key));
        let controls = env.control_sequences(0);
        let untouched = [portfolios[2], tokens[2]].map(|key| (key, env.svm.get_account(&key)));
        let owner_accounts = owners
            .each_ref()
            .map(|owner| env.svm.get_account(&owner.pubkey()));
        let frame: Vec<_> = portfolios
            .into_iter()
            .chain(tokens)
            .chain(owners.iter().map(Signer::pubkey))
            .chain([env.market, env.vault, env.mint, env.admin.pubkey()])
            .collect();
        let check = |env: &V16CuEnv, executions: u64, credited: bool, paid: u64| {
            let fee = if executions > 0 { OWN_FEE } else { 0 };
            let reward = if credited { REWARD } else { 0 };
            let capital = [
                principals[0] - reward,
                initial - fee + reward - paid,
                BYSTANDER,
            ];
            for actor in 0..3 {
                let p = env.portfolio_state(portfolios[actor]);
                assert_eq!(p.capital.get(), u128::from(capital[actor]));
                assert_eq!(p.pnl.get(), 0);
                assert!(p.active_bitmap.iter().all(|word| word.get() == 0));
                assert_eq!(env.portfolio_id(portfolios[actor]), ids[actor]);
                assert_eq!(
                    env.portfolio_position_epoch(portfolios[actor]),
                    epochs[actor]
                );
                assert_eq!(
                    env.portfolio_matcher_sequence(portfolios[actor]),
                    1 + if actor == SUBJECT { executions } else { 0 }
                );
                assert_eq!(
                    p.last_fee_slot.get(),
                    if (actor == SUBJECT && executions > 0) || (actor == 0 && credited) {
                        EXIT_SLOT
                    } else {
                        BIRTHS[actor]
                    }
                );
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
            assert_eq!(group.mode, MarketModeV16::Live);
            assert_eq!(group.c_tot, u128::from(capital.iter().sum::<u64>()));
            assert_eq!(group.insurance, u128::from(fee));
            assert_eq!(group.pnl_pos_tot, 0);
            assert_eq!(group.vault, u128::from(supply - paid));
            assert_eq!(group.vault, group.c_tot + group.insurance);
            assert_eq!(env.token_amount(env.vault), supply - paid);
            assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint_before);
            assert_eq!(env.control_sequences(0), controls);
            assert_domain_budget_remaining_total_consistent(&group, "fee-shortened withdrawal");
            for (key, before) in &untouched {
                assert_eq!(env.svm.get_account(key), *before);
            }
        };
        let (program_id, market, vault, vault_authority) =
            (env.program_id, env.market, env.vault, env.vault_authority);
        let withdrawal = |sequence, amount: u64| Instruction {
            program_id,
            accounts: vec![
                AccountMeta::new(owners[SUBJECT].pubkey(), true),
                AccountMeta::new(market, false),
                AccountMeta::new(portfolios[SUBJECT], false),
                AccountMeta::new(tokens[SUBJECT], false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: ProgInstruction::Withdraw {
                portfolio_id: ids[SUBJECT],
                expected_sequence: sequence,
                amount: amount.into(),
            }
            .encode(),
        };
        let original = withdrawal(1, initial);
        let retained: Vec<_> = (1..=4)
            .map(|nonce| signed(&env, &owners[SUBJECT], &[original.clone()], nonce))
            .collect();
        assert_eq!(
            retained
                .iter()
                .map(|tx| tx.signatures[0])
                .collect::<BTreeSet<_>>()
                .len(),
            4
        );
        for tx in &retained {
            env.svm
                .simulate_transaction(tx.clone().into())
                .expect("nonzero signed withdraw-all is initially admissible");
        }
        let fail = spl_token::instruction::transfer(
            &spl_token::ID,
            &tokens[SUBJECT],
            &vault,
            &owners[SUBJECT].pubkey(),
            &[],
            u64::MAX,
        )
        .unwrap();
        let stale = InstructionError::Custom(PercolatorError::EngineStale as u32);
        let mut nonce = 100;
        let mut send =
            |env: &mut V16CuEnv, ixs: &[Instruction], expected, evidence: &mut Evidence| {
                nonce += 1;
                let tx = signed(env, &owners[SUBJECT], ixs, nonce);
                checked_send(env, tx, &frame, expected, evidence);
            };
        check(&env, 0, false, 0);
        send(
            &mut env,
            &[original.clone(), fail],
            Some((
                1,
                InstructionError::Custom(spl_token::error::TokenError::InsufficientFunds as u32),
                [1, usize::from(first_payout > 0)],
            )),
            &mut evidence,
        );
        evidence.late_failures += 1;
        check(&env, 0, false, 0);
        checked_send(&mut env, retained[0].clone(), &frame, None, &mut evidence);
        check(&env, 1, false, first_payout);
        assert!(
            first_payout < initial,
            "successful withdraw-all leaves an apparent signed residual"
        );
        assert_eq!(env.portfolio_state(portfolios[SUBJECT]).capital.get(), 0);
        checked_send(
            &mut env,
            retained[1].clone(),
            &frame,
            Some((0, stale.clone(), [0, 0])),
            &mut evidence,
        );
        evidence.stale += 1;
        check(&env, 1, false, first_payout);

        let credit = Instruction {
            program_id,
            accounts: vec![
                AccountMeta::new(market, false),
                AccountMeta::new(portfolios[0], false),
                AccountMeta::new(portfolios[SUBJECT], false),
            ],
            data: ProgInstruction::SyncMaintenanceFee {
                now_slot: EXIT_SLOT,
            }
            .encode(),
        };
        for credit_first in [false, true] {
            let (ixs, index, successes) = if credit_first {
                (vec![credit.clone(), original.clone()], 1, [1, 0])
            } else {
                (vec![original.clone(), credit.clone()], 0, [0, 0])
            };
            send(
                &mut env,
                &ixs,
                Some((index, stale.clone(), successes)),
                &mut evidence,
            );
            evidence.stale += 1;
            check(&env, 1, false, first_payout);
        }
        let custody = tokens
            .into_iter()
            .chain([vault, env.mint])
            .map(|key| (key, env.svm.get_account(&key)))
            .collect::<Vec<_>>();
        send(&mut env, &[credit], None, &mut evidence);
        check(&env, 1, true, first_payout);
        for (key, before) in custody {
            assert_eq!(env.svm.get_account(&key), before);
        }
        assert!(
            REWARD > initial,
            "new capital can fund the complete original request"
        );
        let fresh = withdrawal(2, initial);
        let fresh_tx = signed(&env, &owners[SUBJECT], &[fresh.clone()], 50);
        env.svm
            .simulate_transaction(fresh_tx.into())
            .expect("same amount and destination are payable with fresh consent");
        checked_send(
            &mut env,
            retained[2].clone(),
            &frame,
            Some((0, stale.clone(), [0, 0])),
            &mut evidence,
        );
        evidence.stale += 1;
        check(&env, 1, true, first_payout);
        send(&mut env, &[fresh], None, &mut evidence);
        check(&env, 2, true, first_payout + initial);
        send(
            &mut env,
            &[withdrawal(3, REWARD - initial)],
            None,
            &mut evidence,
        );
        check(&env, 3, true, first_payout + REWARD);
        checked_send(
            &mut env,
            retained[3].clone(),
            &frame,
            Some((0, stale, [0, 0])),
            &mut evidence,
        );
        evidence.stale += 1;
        check(&env, 3, true, first_payout + REWARD);
    }
    assert_eq!(evidence.transactions, 30);
    assert_eq!(evidence.stale, 15);
    assert_eq!(evidence.late_failures, 3);
    eprintln!("INV-008 fee-shortened withdrawal: 3 worlds, 30 transactions, 15 stale rollbacks, 3 late SPL rollbacks, max CU={}", evidence.max_cu);
}

fn run_history(history: &History) -> Evidence {
    let mut env = inv018_public_spl_market_with_params(
        6,
        V16CuMarketParams {
            maintenance_fee_per_slot: RATE.into(),
            ..V16CuMarketParams::default()
        },
    );
    env.update_maintenance_fee_policy_with_cu(history.share);
    let subject = history.rounds.len();
    let owners: Vec<_> = (0..subject + 2).map(|_| Keypair::new()).collect();
    let mut portfolios = Vec::new();
    let mut tokens = Vec::new();
    let mut initials = vec![DONOR; subject];
    initials.extend([history.initial, BYSTANDER]);
    for (actor, owner) in owners.iter().enumerate() {
        if actor == subject {
            env.svm.warp_to_slot(SLOT);
        }
        env.svm.airdrop(&owner.pubkey(), 1_000_000_000).unwrap();
        let portfolio = Keypair::new();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &portfolio,
            env.portfolio_account_len,
            env.program_id,
        );
        env.send(
            ProgInstruction::InitPortfolio,
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio.pubkey(), false),
            ],
            &[owner],
        )
        .unwrap();
        let token = create_ata_for_test(&mut env.svm, &env.payer, owner.pubkey(), env.mint);
        send_raw_tx(
            &mut env.svm,
            &env.payer,
            spl_token::instruction::mint_to(
                &spl_token::ID,
                &env.mint,
                &token,
                &env.admin.pubkey(),
                &[],
                initials[actor] + if actor == subject { WALLET } else { 0 },
            )
            .unwrap(),
            &[&env.admin],
        )
        .unwrap();
        env.send(
            env.deposit_ix(portfolio.pubkey(), initials[actor].into()),
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio.pubkey(), false),
                AccountMeta::new(token, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[owner],
        )
        .unwrap();
        portfolios.push(portfolio.pubkey());
        tokens.push(token);
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
    let mint_before = env.svm.get_account(&env.mint).unwrap();
    let ids: Vec<_> = portfolios
        .iter()
        .map(|key| env.portfolio_id(*key))
        .collect();
    let epochs: Vec<_> = portfolios
        .iter()
        .map(|key| env.portfolio_position_epoch(*key))
        .collect();
    let controls = env.control_sequences(0);
    let untouched =
        [portfolios[subject + 1], tokens[subject + 1]].map(|key| (key, env.svm.get_account(&key)));
    let frame: Vec<_> = portfolios
        .iter()
        .chain(&tokens)
        .copied()
        .chain(owners.iter().map(Signer::pubkey))
        .chain([env.market, env.vault, env.mint, env.admin.pubkey()])
        .collect();
    let total: u64 = initials.iter().sum();
    let mint = Mint::unpack(&mint_before.data).unwrap();
    assert_eq!(mint.supply, total + WALLET);
    assert_eq!(mint.mint_authority, COption::None);
    let reward = SLOT * RATE * u64::from(history.share) / 10_000;
    let capital = |stock: &Stock| {
        history.initial + stock.deposited + stock.charged.len() as u64 * reward - stock.paid
    };
    let check = |env: &V16CuEnv, stock: &Stock| {
        let fees = stock.charged.len() as u64 * SLOT * RATE;
        for actor in 0..owners.len() {
            let p = env.portfolio_state(portfolios[actor]);
            let expected = if actor == subject {
                capital(stock)
            } else {
                initials[actor] - u64::from(stock.charged.contains(&actor)) * SLOT * RATE
            };
            assert_eq!(
                p.capital.get(),
                u128::from(expected),
                "{history:?}: actor {actor}"
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
                1 + if actor == subject {
                    stock.withdrawals + stock.deposits
                } else {
                    0
                }
            );
            assert_eq!(
                env.token_amount(tokens[actor]),
                if actor == subject {
                    WALLET + stock.paid - stock.deposited - stock.donated
                } else {
                    0
                }
            );
        }
        let market = env.market_state().1;
        assert_eq!(market.mode, MarketModeV16::Live);
        assert_eq!(
            market.vault,
            u128::from(total + stock.deposited - stock.paid)
        );
        assert_eq!(
            market.insurance,
            u128::from(fees - stock.charged.len() as u64 * reward)
        );
        assert_eq!(market.c_tot + market.insurance, market.vault);
        assert_eq!(market.pnl_pos_tot, 0);
        assert_eq!(
            u128::from(env.token_amount(env.vault)),
            market.vault + u128::from(stock.donated),
            "raw custody replenishment is surplus, never new owner credit"
        );
        assert_eq!(
            env.token_amount(env.vault) + env.token_amount(tokens[subject]),
            total + WALLET
        );
        assert_eq!(env.svm.get_account(&env.mint).unwrap(), mint_before);
        assert_eq!(env.control_sequences(0), controls);
        assert_domain_budget_remaining_total_consistent(&market, "INV-008 stock history");
        for (key, before) in &untouched {
            assert_eq!(env.svm.get_account(key), *before);
        }
    };
    let (program_id, market, vault, vault_authority) =
        (env.program_id, env.market, env.vault, env.vault_authority);
    let withdrawal = |sequence, amount: u64| Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(owners[subject].pubkey(), true),
            AccountMeta::new(market, false),
            AccountMeta::new(portfolios[subject], false),
            AccountMeta::new(tokens[subject], false),
            AccountMeta::new(vault, false),
            AccountMeta::new_readonly(vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::Withdraw {
            portfolio_id: ids[subject],
            expected_sequence: sequence,
            amount: amount.into(),
        }
        .encode(),
    };
    let original = withdrawal(1, history.initial);
    // Sign standalone retries before the first execution; only the CU envelope differs.
    let retained: Vec<_> = (1..=2 + 3 * subject)
        .map(|nonce| signed(&env, &owners[subject], &[original.clone()], nonce as u32))
        .collect();
    assert_eq!(
        retained
            .iter()
            .map(|tx| tx.signatures[0])
            .collect::<BTreeSet<_>>()
            .len(),
        retained.len()
    );
    env.svm
        .simulate_transaction(retained[0].clone().into())
        .expect("retained consent initially pays");
    let fail = spl_token::instruction::transfer(
        &spl_token::ID,
        &tokens[subject],
        &env.vault,
        &owners[subject].pubkey(),
        &[],
        u64::MAX,
    )
    .unwrap();
    let stale = InstructionError::Custom(PercolatorError::EngineStale as u32);
    let tail_error =
        InstructionError::Custom(spl_token::error::TokenError::InsufficientFunds as u32);
    let mut nonce = 100u32;
    let mut send = |env: &mut V16CuEnv, ixs: &[Instruction], expected, evidence: &mut Evidence| {
        nonce += 1;
        let tx = signed(env, &owners[subject], ixs, nonce);
        checked_send(env, tx, &frame, expected, evidence);
    };
    let mut stock = Stock::default();
    let mut evidence = Evidence::default();
    check(&env, &stock);
    for _ in 0..history.initial_failures {
        send(
            &mut env,
            &[original.clone(), fail.clone()],
            Some((1, tail_error.clone(), [1, 1])),
            &mut evidence,
        );
        evidence.late_failures += 1;
        check(&env, &stock);
    }
    checked_send(&mut env, retained[0].clone(), &frame, None, &mut evidence);
    stock.paid += history.initial;
    stock.withdrawals += 1;
    check(&env, &stock);
    assert_eq!(
        capital(&stock),
        0,
        "the first intent exhausted its original stock"
    );

    for (round_index, round) in history.rounds.iter().enumerate() {
        let amount = round.amount.max(history.initial);
        for (step, kind) in ORDERS[round.order].into_iter().enumerate() {
            let refill = match kind {
                0 => Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(owners[subject].pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[subject], false),
                        AccountMeta::new(tokens[subject], false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: ProgInstruction::Deposit {
                        portfolio_id: ids[subject],
                        expected_sequence: 1 + stock.withdrawals + stock.deposits,
                        amount: amount.into(),
                    }
                    .encode(),
                },
                1 => Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolios[round_index], false),
                        AccountMeta::new(portfolios[subject], false),
                    ],
                    data: ProgInstruction::SyncMaintenanceFee { now_slot: SLOT }.encode(),
                },
                2 => spl_token::instruction::transfer(
                    &spl_token::ID,
                    &tokens[subject],
                    &env.vault,
                    &owners[subject].pubkey(),
                    &[],
                    amount,
                )
                .unwrap(),
                _ => unreachable!(),
            };
            let (bundle, prefix, successes) = if round.refill_first {
                (
                    vec![refill.clone(), original.clone()],
                    1,
                    [usize::from(kind != 2), usize::from(kind != 1)],
                )
            } else {
                (vec![original.clone(), refill.clone()], 0, [0, 0])
            };
            send(
                &mut env,
                &bundle,
                Some((prefix, stale.clone(), successes)),
                &mut evidence,
            );
            evidence.stale += 1;
            check(&env, &stock);
            send(&mut env, &[refill], None, &mut evidence);
            match kind {
                0 => {
                    stock.deposited += amount;
                    stock.deposits += 1;
                }
                1 => {
                    assert!(stock.charged.insert(round_index));
                }
                2 => stock.donated += amount,
                _ => unreachable!(),
            }
            check(&env, &stock);
            checked_send(
                &mut env,
                retained[1 + round_index * 3 + step].clone(),
                &frame,
                Some((0, stale.clone(), [0, 0])),
                &mut evidence,
            );
            evidence.stale += 1;
            check(&env, &stock);
        }
        assert!(
            capital(&stock) >= history.initial,
            "stale rejection cannot rely on stock depletion"
        );
        let amount = if round.half_payout {
            capital(&stock) / 2
        } else {
            capital(&stock)
        };
        assert!(
            amount >= history.initial,
            "fresh consent can pay at least the retained amount"
        );
        let fresh = withdrawal(1 + stock.withdrawals + stock.deposits, amount);
        for _ in 0..round.failures {
            send(
                &mut env,
                &[fresh.clone(), fail.clone()],
                Some((1, tail_error.clone(), [1, 1])),
                &mut evidence,
            );
            evidence.late_failures += 1;
            check(&env, &stock);
        }
        // The exact request whose successful prefix rolled back remains usable.
        send(&mut env, &[fresh], None, &mut evidence);
        stock.paid += amount;
        stock.withdrawals += 1;
        check(&env, &stock);
    }
    if capital(&stock) > 0 {
        let amount = capital(&stock);
        send(
            &mut env,
            &[withdrawal(1 + stock.withdrawals + stock.deposits, amount)],
            None,
            &mut evidence,
        );
        stock.paid += amount;
        stock.withdrawals += 1;
        check(&env, &stock);
    }
    checked_send(
        &mut env,
        retained.last().unwrap().clone(),
        &frame,
        Some((0, stale, [0, 0])),
        &mut evidence,
    );
    evidence.stale += 1;
    check(&env, &stock);
    assert_eq!(capital(&stock), 0);
    assert_eq!(
        stock.paid,
        history.initial + stock.deposited + subject as u64 * reward,
        "only fresh withdrawals pay the new capital; custody surplus stays in the vault"
    );
    evidence
}

#[test]
fn v16_program_generated_withdrawal_stock_histories_preserve_first_execution_budget() {
    let totals = std::cell::RefCell::new((0u64, Evidence::default()));
    let record = |history: History| {
        let evidence = run_history(&history);
        let mut total = totals.borrow_mut();
        total.0 += 1;
        total.1.transactions += evidence.transactions;
        total.1.stale += evidence.stale;
        total.1.late_failures += evidence.late_failures;
        total.1.max_cu = total.1.max_cu.max(evidence.max_cu);
    };
    for order in 0..6 {
        for refill_first in [false, true] {
            record(History {
                initial: if refill_first { 17 } else { 1 },
                share: 3_333,
                initial_failures: 2,
                rounds: [false, true]
                    .map(|half_payout| Round {
                        amount: 37,
                        order,
                        refill_first,
                        failures: 2,
                        half_payout,
                    })
                    .to_vec(),
            });
        }
    }
    let strategy = (
        1u64..=17,
        prop::sample::select(vec![3_333u16, 10_000]),
        0u8..=2,
        prop::collection::vec(
            (1u64..=97, 0usize..6, any::<bool>(), 0u8..=2, any::<bool>()),
            1..=4,
        ),
    )
        .prop_map(|(initial, share, initial_failures, rounds)| History {
            initial,
            share,
            initial_failures,
            rounds: rounds
                .into_iter()
                .map(
                    |(amount, order, refill_first, failures, half_payout)| Round {
                        amount,
                        order,
                        refill_first,
                        failures,
                        half_payout,
                    },
                )
                .collect(),
        });
    let mut runner = TestRunner::new_with_rng(
        Config {
            cases: 32,
            max_shrink_iters: 128,
            failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
                "proptest-regressions/inv_008_withdrawal_stock_history.txt",
            ))),
            ..Config::default()
        },
        TestRng::from_seed(RngAlgorithm::ChaCha, &[0x08; 32]),
    );
    runner
        .run(&strategy, |history| {
            record(history);
            Ok(())
        })
        .unwrap();
    let (worlds, evidence) = totals.into_inner();
    assert!(evidence.stale > 0 && evidence.late_failures > 0);
    eprintln!("INV-008 stock history: {worlds} worlds, {} transactions, {} stale rollbacks, {} late SPL rollbacks, max CU={}",
        evidence.transactions, evidence.stale, evidence.late_failures, evidence.max_cu);
}
