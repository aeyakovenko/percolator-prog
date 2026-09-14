//! INV-008/024/031/080/081: backing principal and earned utilization fees become
//! portfolio capital only through their owner's signed payout/deposit. Later
//! stock cannot revive a consumed portfolio withdrawal. Backing routes retain
//! current-authority/balance semantics, separate from the portfolio sequence
//! assertion. Rows 415/428 remain OPEN.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census,
};
use std::collections::BTreeSet;

const INITIAL: u64 = 101;
const FIRST: u64 = 37;
const DONOR: u64 = 211;
const RETAINED_BACKING: u64 = BACKING - INITIAL - DONOR;
const FIRST_RESERVE: [u64; 2] = [17, 11];
const LIEN: u64 = 1_050 * 105 / 2 - CAPITAL[0];
const NEXT_LIEN: u64 = 1_060 * 105 / 2 - (CAPITAL[0] - EARNINGS);
const NEXT_FEE: u64 = ((NEXT_LIEN - LIEN) * RATE as u64).div_ceil(10_000);
const LIMIT: u64 = 900_000;
const ORDERS: [[usize; 3]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];

#[derive(Clone, Copy, Debug)]
enum Class {
    Principal,
    Earnings,
}

impl Class {
    fn index(self) -> usize {
        match self {
            Self::Principal => 0,
            Self::Earnings => 1,
        }
    }
}

#[derive(Default, Clone)]
struct Book {
    reserve_paid: [u64; 2],
    capital_paid: u64,
    capital_debits: u64,
    capital_credits: [u64; 3],
    deposits: u64,
    backing_added: u64,
    donor_transferred: u64,
    accrued: bool,
    observed_fees: u64,
}

#[derive(Default)]
struct Evidence {
    transactions: u64,
    rollbacks: u64,
    restored_spl: u64,
    peak_cu: u64,
    packet: u64,
}

fn wrap(env: &V16CuEnv, ix: ProgInstruction, accounts: Vec<AccountMeta>) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ix.encode(),
    }
}

fn portfolio_accounts(world: &TerminalEarningsWorld, portfolio: Pubkey) -> Vec<AccountMeta> {
    vec![
        AccountMeta::new(world.incumbent.pubkey(), true),
        AccountMeta::new(world.env.market, false),
        AccountMeta::new(portfolio, false),
        AccountMeta::new(world.tokens[2], false),
        AccountMeta::new(world.env.vault, false),
        AccountMeta::new_readonly(spl_token::ID, false),
    ]
}

fn debit(
    world: &TerminalEarningsWorld,
    portfolio: Pubkey,
    sequence: u64,
    amount: u64,
) -> Instruction {
    let mut accounts = portfolio_accounts(world, portfolio);
    accounts.insert(
        5,
        AccountMeta::new_readonly(world.env.vault_authority, false),
    );
    wrap(
        &world.env,
        ProgInstruction::Withdraw {
            portfolio_id: world.env.portfolio_id(portfolio),
            expected_sequence: sequence,
            amount: amount.into(),
        },
        accounts,
    )
}

fn reserve(
    world: &TerminalEarningsWorld,
    ledger: Pubkey,
    class: Class,
    amount: u64,
) -> Instruction {
    let env = &world.env;
    let mut accounts = vec![
        AccountMeta::new(world.incumbent.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(world.tokens[2], false),
        AccountMeta::new(env.vault, false),
        AccountMeta::new_readonly(env.vault_authority, false),
        AccountMeta::new_readonly(spl_token::ID, false),
    ];
    let market_id = env.asset_market_id(0);
    let authority_epoch = env.control_sequences(0).authority_epoch;
    let ix = match class {
        Class::Principal => ProgInstruction::WithdrawBackingBucket {
            domain: 1,
            market_id,
            authority_epoch,
            amount: amount.into(),
        },
        Class::Earnings => {
            accounts.insert(2, AccountMeta::new(ledger, false));
            ProgInstruction::WithdrawBackingBucketEarnings {
                domain: 1,
                market_id,
                authority_epoch,
                amount: amount.into(),
            }
        }
    };
    wrap(env, ix, accounts)
}

fn signed(env: &V16CuEnv, ixs: &[Instruction], owners: &[&Keypair], nonce: u32) -> Transaction {
    let instructions = [
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(LIMIT as u32 - nonce),
    ]
    .into_iter()
    .chain(ixs.iter().cloned())
    .collect::<Vec<_>>();
    let mut signers = vec![&env.payer];
    signers.extend(owners.iter().copied().filter(|owner| {
        ixs.iter()
            .flat_map(|ix| &ix.accounts)
            .any(|meta| meta.is_signer && meta.pubkey == owner.pubkey())
    }));
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&env.payer.pubkey()),
        &signers,
        env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    tx
}

fn land(
    env: &mut V16CuEnv,
    tx: &Transaction,
    tracked: &[Pubkey],
    expected: Option<TransactionError>,
    successes: [usize; 2],
    evidence: &mut Evidence,
) {
    tx.verify().unwrap();
    let wire = bincode::serialize(tx).unwrap();
    evidence.packet = evidence.packet.max(wire.len() as u64);
    let fee = FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let keys = tracked
        .iter()
        .chain(&tx.message.account_keys)
        .copied()
        .collect::<BTreeSet<_>>();
    let before = keys
        .into_iter()
        .map(|key| (key, env.svm.get_account(&key)))
        .collect::<Vec<_>>();
    let result = env
        .svm
        .send_transaction(bincode::deserialize::<Transaction>(&wire).unwrap());
    let meta = if let Some(error) = &expected {
        let failure = result.expect_err("retained adjacent-stock rejection");
        assert_eq!(&failure.err, error, "{:?}", failure.meta.logs);
        evidence.rollbacks += 1;
        evidence.restored_spl += successes[1] as u64;
        failure.meta
    } else {
        result.expect("authorized stock continuation")
    };
    for (key, mut account) in before {
        if key == env.payer.pubkey() {
            account.as_mut().unwrap().lamports -= fee;
        }
        let writable = tx
            .message
            .account_keys
            .iter()
            .position(|candidate| *candidate == key)
            .is_some_and(|index| tx.message.is_writable(index));
        if expected.is_some() || key == env.payer.pubkey() || !writable {
            assert_eq!(
                env.svm.get_account(&key),
                account,
                "complete Account at {key}"
            );
        } else if let Some(before) = account {
            let after = env.svm.get_account(&key).unwrap();
            assert_eq!(
                (
                    after.lamports,
                    after.owner,
                    after.executable,
                    after.rent_epoch,
                    after.data.len()
                ),
                (
                    before.lamports,
                    before.owner,
                    before.executable,
                    before.rent_epoch,
                    before.data.len()
                ),
                "successful account metadata at {key}",
            );
        }
    }
    for (program, count) in [env.program_id, spl_token::ID].into_iter().zip(successes) {
        assert_eq!(
            meta.logs
                .iter()
                .filter(|line| **line == format!("Program {program} success"))
                .count(),
            count,
            "completed wrapper/SPL prefix: {:?}",
            meta.logs
        );
    }
    assert!(meta.compute_units_consumed > 0);
    assert_cu_within(
        "backing/earnings stock epochs",
        meta.compute_units_consumed,
        LIMIT,
    );
    evidence.peak_cu = evidence.peak_cu.max(meta.compute_units_consumed);
    evidence.transactions += 1;
    assert_eq!(bincode::serialize(tx).unwrap(), wire);
}

fn error(index: u8, code: u32) -> Option<TransactionError> {
    Some(TransactionError::InstructionError(
        index + 2,
        InstructionError::Custom(code),
    ))
}

#[test]
fn v16_retained_withdrawal_cannot_acquire_reclassified_backing_principal_or_earnings() {
    assert_eq!((LIEN, NEXT_LIEN, NEXT_FEE), (2_623, 4_023, 467));
    let mut evidence = Evidence::default();
    let mut histories = 0;
    for amounts in [[1u64, 3, 19, 23], [137, 211, 31, 41]] {
        let [principal, earnings, external, refill] = amounts;
        let mut endpoints = Vec::new();
        for order in ORDERS {
            for split in [false, true] {
                let (mut world, users) = terminal_earnings_world_with_user_signers(false, None);
                let portfolio_key = Keypair::new();
                let portfolio = portfolio_key.pubkey();
                system_create_account_for_test(
                    &mut world.env.svm,
                    &world.env.payer,
                    &portfolio_key,
                    world.env.portfolio_account_len,
                    world.env.program_id,
                );
                world
                    .env
                    .send(
                        ProgInstruction::InitPortfolio,
                        vec![
                            AccountMeta::new(world.incumbent.pubkey(), true),
                            AccountMeta::new(world.env.market, false),
                            AccountMeta::new(portfolio, false),
                        ],
                        &[&world.incumbent],
                    )
                    .unwrap();
                world.env.portfolios.push(portfolio);
                let ledger_key = Keypair::new();
                let ledger = ledger_key.pubkey();
                system_create_account_for_test(
                    &mut world.env.svm,
                    &world.env.payer,
                    &ledger_key,
                    state::backing_domain_ledger_account_len(),
                    world.env.program_id,
                );
                // Before retention the provider funds its portfolio and gives
                // a distinct signer external custody, using unencumbered principal.
                let endowment = reserve(&world, ledger, Class::Principal, INITIAL + DONOR);
                send_raw_tx(
                    &mut world.env.svm,
                    &world.env.payer,
                    endowment,
                    &[&world.incumbent],
                )
                .unwrap();
                let transfer = |from, to, owner, amount| {
                    spl_token::instruction::transfer(
                        &spl_token::ID,
                        &from,
                        &to,
                        &owner,
                        &[],
                        amount,
                    )
                    .unwrap()
                };
                send_raw_tx(
                    &mut world.env.svm,
                    &world.env.payer,
                    transfer(
                        world.tokens[2],
                        world.tokens[1],
                        world.incumbent.pubkey(),
                        DONOR,
                    ),
                    &[&world.incumbent],
                )
                .unwrap();
                world
                    .env
                    .send(
                        world.env.deposit_ix(portfolio, INITIAL.into()),
                        portfolio_accounts(&world, portfolio),
                        &[&world.incumbent],
                    )
                    .unwrap();

                let sequence = world.env.portfolio_matcher_sequence(portfolio);
                assert_eq!(sequence, 1);
                let identity = (
                    world.env.portfolio_id(portfolio),
                    world.env.portfolio_position_epoch(portfolio),
                );
                let controls = world.env.control_sequences(0);
                let initial = world.env.market_state().1;
                let tokens = world
                    .tokens
                    .into_iter()
                    .chain([world.env.vault])
                    .map(|key| (key, world.env.svm.get_account(&key).unwrap()))
                    .collect::<Vec<_>>();
                let fixed = [
                    world.admin.pubkey(),
                    world.incumbent.pubkey(),
                    world.successor.pubkey(),
                    users[0].pubkey(),
                    users[1].pubkey(),
                    world.env.mint,
                ]
                .map(|key| (key, world.env.svm.get_account(&key)));
                let profile = state::read_asset_oracle_profile(
                    &world.env.svm.get_account(&world.env.market).unwrap().data,
                    0,
                )
                .unwrap();
                let mut tracked = world.env.portfolios.clone();
                tracked.extend(tokens.iter().map(|(key, _)| *key));
                tracked.extend(fixed.iter().map(|(key, _)| *key));
                tracked.extend([world.env.market, ledger, world.env.vault_authority]);
                let check = |world: &TerminalEarningsWorld, book: &Book| {
                    let env = &world.env;
                    let data = env.svm.get_account(&env.market).unwrap().data;
                    let group = env.market_state().1;
                    let fees = EARNINGS + u64::from(book.accrued) * NEXT_FEE;
                    let lien = if book.accrued { NEXT_LIEN } else { LIEN };
                    let credited = book.capital_credits.iter().sum::<u64>();
                    let reserve_paid = book.reserve_paid.iter().sum::<u64>();
                    let custody = SUPPLY - DONOR + book.backing_added + credited
                        - book.capital_paid
                        - reserve_paid;
                    assert_eq!(group.mode, MarketModeV16::Live);
                    state::market_view_mut(&mut data.clone())
                        .unwrap()
                        .1
                        .validate_shape()
                        .unwrap();
                    let oi = u128::from(1_050 + u64::from(book.accrued) * 10) * POS_SCALE;
                    assert_eq!(
                        (
                            group.assets[0].oi_eff_long_q,
                            group.assets[0].oi_eff_short_q
                        ),
                        (oi, oi)
                    );
                    assert_eq!(
                        (
                            env.portfolio_id(portfolio),
                            env.portfolio_position_epoch(portfolio)
                        ),
                        identity
                    );
                    assert_eq!(group.vault, custody.into());
                    assert_eq!(group.insurance, INSURANCE.into());
                    assert_eq!(
                        group.insurance_domain_budget,
                        initial.insurance_domain_budget
                    );
                    assert_eq!(group.insurance_domain_spent, initial.insurance_domain_spent);
                    assert_eq!(
                        group.backing_provider_earnings_total,
                        u128::from(fees - book.reserve_paid[1])
                    );
                    assert_eq!(
                        env.portfolio_state(portfolio).capital.get(),
                        u128::from(INITIAL + credited - book.capital_paid)
                    );
                    assert_eq!(env.portfolio_state(portfolio).pnl.get(), 0);
                    assert_eq!(
                        env.portfolio_matcher_sequence(portfolio),
                        sequence + book.capital_debits + book.deposits
                    );
                    assert_eq!(
                        book.capital_paid,
                        if book.capital_debits == 0 {
                            0
                        } else if book.capital_debits == 1 {
                            FIRST
                        } else {
                            INITIAL + credited
                        }
                    );
                    assert_eq!(
                        env.portfolio_state(world.portfolios[0]).capital.get(),
                        u128::from(CAPITAL[0] - fees)
                    );
                    assert_eq!(
                        env.portfolio_state(world.portfolios[0]).pnl.get(),
                        PROFIT as i128
                    );
                    assert_eq!(
                        env.portfolio_state(world.portfolios[1]).capital.get(),
                        u128::from(CAPITAL[1] - PROFIT)
                    );
                    assert_eq!(env.portfolio_state(world.portfolios[1]).pnl.get(), 0);
                    let mut expected_controls = controls;
                    expected_controls.backing_top_up += u64::from(book.backing_added != 0);
                    assert_eq!(env.control_sequences(0), expected_controls);
                    assert_eq!(state::read_asset_oracle_profile(&data, 0).unwrap(), profile);
                    let mut bucket = initial.source_backing_buckets[1];
                    bucket.fresh_unliened_backing_num = u128::from(
                        RETAINED_BACKING + PROFIT + book.backing_added
                            - book.reserve_paid[0]
                            - lien,
                    ) * BOUND_SCALE;
                    bucket.valid_liened_backing_num = u128::from(lien) * BOUND_SCALE;
                    bucket.utilization_fee_earnings = (fees - book.reserve_paid[1]).into();
                    assert_eq!(group.source_backing_buckets[1], bucket);
                    assert_eq!(
                        group.source_credit[1].fresh_reserved_backing_num,
                        u128::from(
                            RETAINED_BACKING + PROFIT + book.backing_added - book.reserve_paid[0]
                        ) * BOUND_SCALE
                    );
                    assert_eq!(
                        group.source_backing_buckets[0],
                        initial.source_backing_buckets[0]
                    );
                    assert_eq!(group.source_credit[0], initial.source_credit[0]);
                    if book.reserve_paid[1] != 0 || book.backing_added != 0 {
                        let record = state::read_backing_domain_ledger(
                            &env.svm.get_account(&ledger).unwrap().data,
                        )
                        .unwrap();
                        assert_eq!(
                            record,
                            state::BackingDomainLedgerAccountV16 {
                                market_group: env.market.to_bytes(),
                                authority: world.incumbent.pubkey().to_bytes(),
                                domain: 1,
                                total_principal_atoms: book.backing_added.into(),
                                total_deposited_atoms: book.backing_added.into(),
                                total_earnings_atoms: u128::from(book.observed_fees - EARNINGS),
                                total_earnings_withdrawn_atoms: book.reserve_paid[1].into(),
                                last_observed_bucket_earnings_atoms: u128::from(
                                    book.observed_fees - book.reserve_paid[1]
                                ),
                                ..Default::default()
                            }
                        );
                    } else {
                        assert!(env
                            .svm
                            .get_account(&ledger)
                            .unwrap()
                            .data
                            .iter()
                            .all(|b| *b == 0));
                    }
                    for (key, original) in &tokens {
                        let mut expected = original.clone();
                        let mut token = TokenAccount::unpack(&expected.data).unwrap();
                        if *key == env.vault {
                            token.amount = custody;
                        } else if *key == world.tokens[1] {
                            token.amount = DONOR - book.donor_transferred;
                        } else if *key == world.tokens[2] {
                            token.amount =
                                book.capital_paid + reserve_paid + book.donor_transferred
                                    - credited
                                    - book.backing_added;
                        }
                        TokenAccount::pack(token, &mut expected.data).unwrap();
                        assert_eq!(env.svm.get_account(key), Some(expected));
                    }
                    for (key, account) in &fixed {
                        assert_eq!(env.svm.get_account(key), *account);
                    }
                    let portfolios = env
                        .portfolios
                        .iter()
                        .map(|key| env.portfolio_state(*key))
                        .collect::<Vec<_>>();
                    assert_market_stock_census(
                        "adjacent backing/earnings stock",
                        &group,
                        &data,
                        &portfolios,
                        custody.into(),
                    )
                    .unwrap();
                    assert_reservation_encumbrance_census(
                        "adjacent backing/earnings stock",
                        &group,
                        &portfolios,
                    )
                    .unwrap();
                };
                let owners = [&world.incumbent, &users[0], &users[1]];
                let first = [
                    debit(&world, portfolio, sequence, FIRST),
                    reserve(&world, ledger, Class::Principal, FIRST_RESERVE[0]),
                    reserve(&world, ledger, Class::Earnings, FIRST_RESERVE[1]),
                ];
                // Every envelope, including all future sequence projections, is
                // signed before the first execution. Delivery uses LiteSVM directly.
                let first_txs = std::array::from_fn::<_, 3, _>(|i| {
                    signed(&world.env, &[first[i].clone()], &owners, 10 + i as u32)
                });
                let late_fail = transfer(
                    world.tokens[2],
                    world.tokens[1],
                    world.incumbent.pubkey(),
                    SUPPLY + 1,
                );
                let failed_first = std::array::from_fn::<_, 3, _>(|i| {
                    signed(
                        &world.env,
                        &[first[i].clone(), late_fail.clone()],
                        &owners,
                        40 + i as u32,
                    )
                });
                let stale = first[0].clone();
                let stale_tx = signed(&world.env, &[stale.clone()], &owners, 50);
                let topup = wrap(
                    &world.env,
                    ProgInstruction::TopUpBackingBucket {
                        domain: 1,
                        market_id: world.env.asset_market_id(0),
                        authority_epoch: controls.authority_epoch,
                        intent_id: controls.backing_top_up + 1,
                        backing_fee_bps: RATE,
                        insurance_share_bps: 0,
                        amount: refill.into(),
                        expiry_slot: 100,
                    },
                    vec![
                        AccountMeta::new(world.incumbent.pubkey(), true),
                        AccountMeta::new(world.env.market, false),
                        AccountMeta::new(world.tokens[2], false),
                        AccountMeta::new(world.env.vault, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                        AccountMeta::new(ledger, false),
                    ],
                );
                let trade = wrap(
                    &world.env,
                    ProgInstruction::TradeNoCpi {
                        account_a_portfolio_id: world.env.portfolio_id(world.portfolios[0]),
                        account_a_position_epoch: world
                            .env
                            .portfolio_position_epoch(world.portfolios[0]),
                        account_b_portfolio_id: world.env.portfolio_id(world.portfolios[1]),
                        account_b_position_epoch: world
                            .env
                            .portfolio_position_epoch(world.portfolios[1]),
                        asset_index: 0,
                        market_id: world.env.asset_market_id(0),
                        size_q: 10 * POS_SCALE as i128,
                        exec_price: 105,
                        fee_bps: 0,
                        backing_fee_cap_bps: RATE,
                    },
                    vec![
                        AccountMeta::new(users[0].pubkey(), true),
                        AccountMeta::new(users[1].pubkey(), true),
                        AccountMeta::new(world.env.market, false),
                        AccountMeta::new(world.portfolios[0], false),
                        AccountMeta::new(world.portfolios[1], false),
                    ],
                );
                let replenishment = [
                    transfer(world.tokens[1], world.tokens[2], users[1].pubkey(), refill),
                    topup,
                    trade,
                ];
                let replenish_tx = signed(&world.env, &replenishment, &owners, 60);
                let replenish_stale = signed(
                    &world.env,
                    &[replenishment.to_vec(), vec![stale.clone()]].concat(),
                    &owners,
                    61,
                );
                let mut projected_sequence = sequence + 1;
                let mut words = Vec::new();
                for (step, kind) in order.into_iter().enumerate() {
                    let amount = [principal, earnings, external][kind];
                    let payout = if kind == 2 {
                        transfer(world.tokens[1], world.tokens[2], users[1].pubkey(), amount)
                    } else {
                        reserve(
                            &world,
                            ledger,
                            [Class::Principal, Class::Earnings][kind],
                            amount,
                        )
                    };
                    let deposit = wrap(
                        &world.env,
                        ProgInstruction::Deposit {
                            portfolio_id: world.env.portfolio_id(portfolio),
                            expected_sequence: projected_sequence,
                            amount: amount.into(),
                        },
                        portfolio_accounts(&world, portfolio),
                    );
                    projected_sequence += 1;
                    let word = vec![payout, deposit];
                    let nonce = 100 + step as u32 * 10;
                    words.push((
                        kind,
                        signed(
                            &world.env,
                            &[word.clone(), vec![stale.clone()]].concat(),
                            &owners,
                            nonce,
                        ),
                        signed(
                            &world.env,
                            &[word.clone(), vec![late_fail.clone()]].concat(),
                            &owners,
                            nonce + 1,
                        ),
                        signed(&world.env, &word, &owners, nonce + 2),
                        word.iter()
                            .enumerate()
                            .map(|(i, ix)| {
                                signed(&world.env, &[ix.clone()], &owners, nonce + 3 + i as u32)
                            })
                            .collect::<Vec<_>>(),
                    ));
                }
                let remaining = INITIAL - FIRST + principal + earnings + external;
                let final_ix = debit(&world, portfolio, projected_sequence, remaining);
                let final_tx = signed(&world.env, &[final_ix.clone()], &owners, 200);
                let cross_duplicate = signed(
                    &world.env,
                    &[
                        final_ix.clone(),
                        first[1].clone(),
                        first[2].clone(),
                        final_ix.clone(),
                    ],
                    &owners,
                    203,
                );
                let final_fail = signed(&world.env, &[final_ix, late_fail], &owners, 201);
                let post_stale = signed(&world.env, &[stale], &owners, 202);
                let mut book = Book::default();
                check(&world, &book);
                for i in 0..3 {
                    land(
                        &mut world.env,
                        &failed_first[i],
                        &tracked,
                        error(1, spl_token::error::TokenError::InsufficientFunds as u32),
                        [1, 1],
                        &mut evidence,
                    );
                    check(&world, &book);
                    land(
                        &mut world.env,
                        &first_txs[i],
                        &tracked,
                        None,
                        [1, 1],
                        &mut evidence,
                    );
                    if i == 0 {
                        book.capital_paid = FIRST;
                        book.capital_debits = 1;
                    } else {
                        book.reserve_paid[i - 1] = FIRST_RESERVE[i - 1];
                        if i == 2 {
                            book.observed_fees = EARNINGS;
                        }
                    }
                    check(&world, &book);
                }
                land(
                    &mut world.env,
                    &stale_tx,
                    &tracked,
                    error(0, PercolatorError::EngineStale as u32),
                    [0, 0],
                    &mut evidence,
                );
                check(&world, &book);
                land(
                    &mut world.env,
                    &replenish_stale,
                    &tracked,
                    error(3, PercolatorError::EngineStale as u32),
                    [2, 2],
                    &mut evidence,
                );
                check(&world, &book);
                land(
                    &mut world.env,
                    &replenish_tx,
                    &tracked,
                    None,
                    [2, 2],
                    &mut evidence,
                );
                book.backing_added = refill;
                book.donor_transferred = refill;
                book.accrued = true;
                check(&world, &book);
                for (kind, rejected, spl_failed, atomic, parts) in words {
                    let wrapper_count = if kind == 2 { 1 } else { 2 };
                    land(
                        &mut world.env,
                        &rejected,
                        &tracked,
                        error(2, PercolatorError::EngineStale as u32),
                        [wrapper_count, 2],
                        &mut evidence,
                    );
                    check(&world, &book);
                    land(
                        &mut world.env,
                        &spl_failed,
                        &tracked,
                        error(2, spl_token::error::TokenError::InsufficientFunds as u32),
                        [wrapper_count, 2],
                        &mut evidence,
                    );
                    check(&world, &book);
                    let amount = [principal, earnings, external][kind];
                    if split {
                        land(
                            &mut world.env,
                            &parts[0],
                            &tracked,
                            None,
                            [usize::from(kind != 2), 1],
                            &mut evidence,
                        );
                    } else {
                        land(
                            &mut world.env,
                            &atomic,
                            &tracked,
                            None,
                            [wrapper_count, 2],
                            &mut evidence,
                        );
                    }
                    if kind == 2 {
                        book.donor_transferred += amount;
                    } else {
                        let class = [Class::Principal, Class::Earnings][kind];
                        book.reserve_paid[class.index()] += amount;
                        if kind == 1 {
                            book.observed_fees = EARNINGS + NEXT_FEE;
                        }
                    }
                    if split {
                        check(&world, &book);
                        land(
                            &mut world.env,
                            &parts[1],
                            &tracked,
                            None,
                            [1, 1],
                            &mut evidence,
                        );
                    }
                    book.capital_credits[kind] += amount;
                    book.deposits += 1;
                    check(&world, &book);
                }
                land(
                    &mut world.env,
                    &cross_duplicate,
                    &tracked,
                    error(3, PercolatorError::EngineStale as u32),
                    [3, 3],
                    &mut evidence,
                );
                check(&world, &book);
                land(
                    &mut world.env,
                    &final_fail,
                    &tracked,
                    error(1, spl_token::error::TokenError::InsufficientFunds as u32),
                    [1, 1],
                    &mut evidence,
                );
                check(&world, &book);
                land(
                    &mut world.env,
                    &final_tx,
                    &tracked,
                    None,
                    [1, 1],
                    &mut evidence,
                );
                book.capital_paid += remaining;
                book.capital_debits += 1;
                check(&world, &book);
                land(
                    &mut world.env,
                    &post_stale,
                    &tracked,
                    error(0, PercolatorError::EngineStale as u32),
                    [0, 0],
                    &mut evidence,
                );
                check(&world, &book);
                assert_eq!(
                    world.env.token_amount(world.tokens[1]),
                    DONOR - refill - external
                );
                assert_eq!(
                    world.env.token_amount(world.tokens[2]),
                    INITIAL + FIRST_RESERVE.iter().sum::<u64>() + principal + earnings + external
                );
                assert_eq!(
                    world.env.market_state().1.vault,
                    u128::from(
                        SUPPLY - DONOR + refill
                            - INITIAL
                            - FIRST_RESERVE.iter().sum::<u64>()
                            - principal
                            - earnings
                    )
                );
                endpoints.push((
                    book.capital_paid,
                    book.reserve_paid,
                    world.env.token_amount(world.tokens[1]),
                    world.env.token_amount(world.tokens[2]),
                    world.env.market_state().1.vault,
                ));
                histories += 1;
            }
        }
        assert!(endpoints.windows(2).all(|pair| pair[0] == pair[1]));
    }
    assert_eq!(histories, 24);
    eprintln!("adjacent backing/earnings stock: histories={histories}, transactions={}, rollbacks={}, restored_SPL={}, peak_CU={}, packet={}", evidence.transactions, evidence.rollbacks, evidence.restored_spl, evidence.peak_cu, evidence.packet);
}
