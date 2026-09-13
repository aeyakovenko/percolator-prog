//! INV-008/010/011/024/031/064/080/081: generated Live insurance stock histories.
//! Each epoch admits one debit across retained quote-rail/telemetry envelopes.
//! Input-owned receipts reconcile domain stock, entitlement and exact destinations;
//! custody-only donations never enlarge entitlement. Rows 415/428 remain OPEN.

use super::*;
use crate::inv_018_quote_mint_vault_token_program_and_authority_integrity::{
    inv018_create_public_spl_mint, inv018_public_spl_market_with_params,
};
use rand::{Rng, SeedableRng};
use rand_xorshift::XorShiftRng;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const ORDERS: [[usize; 3]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];
const DONATION: u128 = 160;
const CU_LIMIT: u32 = 300_000;

#[derive(Clone, Debug)]
enum Op {
    Refill {
        domain: usize,
        amount: u128,
    },
    Donate,
    Debit {
        asset: usize,
        epoch: u64,
        amount: u128,
        rail: usize,
        observed: bool,
    },
    LateError,
}

#[derive(Clone, Debug)]
struct Receipt {
    signed_amount: u128,
    consumed: [u128; 2],
    paid: [u128; 2],
}

#[derive(Clone)]
struct Book {
    credited: [u128; 4],
    budgets: [u128; 4],
    sources: [u128; 2],
    custody: [u128; 2],
    donated: u128,
    controls: [state::AssetControlSequencesV16; 2],
    receipts: BTreeMap<(usize, u64), Receipt>,
    ledgers: [Option<state::InsuranceLedgerAccountV16>; 2],
}

impl Book {
    fn available(&self, asset: usize) -> u128 {
        self.budgets[2 * asset] + self.budgets[2 * asset + 1]
    }

    fn debit(&self, asset: usize, amount: u128, rail: usize, observed: bool) -> Op {
        Op::Debit {
            asset,
            epoch: self.controls[asset].authority_epoch,
            amount,
            rail,
            observed,
        }
    }

    fn apply(&mut self, op: &Op, market: Pubkey, authority: Pubkey) {
        match *op {
            Op::Refill { domain, amount } => {
                self.credited[domain] += amount;
                self.budgets[domain] += amount;
                self.sources[0] -= amount;
                self.custody[0] += amount;
                self.controls[domain / 2].insurance_top_up += 1;
            }
            Op::Donate => {
                self.sources[1] -= DONATION;
                self.custody[1] += DONATION;
                self.donated += DONATION;
            }
            Op::Debit {
                asset,
                epoch,
                amount,
                rail,
                observed,
            } => {
                assert_eq!(epoch, self.controls[asset].authority_epoch);
                assert!(amount > 0 && amount <= self.available(asset));
                assert!(amount <= self.custody[rail]);
                let available = self.available(asset);
                if observed {
                    let ledger =
                        self.ledgers[asset].get_or_insert(state::InsuranceLedgerAccountV16 {
                            market_group: market.to_bytes(),
                            authority: authority.to_bytes(),
                            last_observed_insurance_atoms: available,
                            ..state::InsuranceLedgerAccountV16::default()
                        });
                    if available >= ledger.last_observed_insurance_atoms {
                        ledger.cumulative_profit_atoms +=
                            available - ledger.last_observed_insurance_atoms;
                    } else {
                        ledger.cumulative_loss_atoms +=
                            ledger.last_observed_insurance_atoms - available;
                    }
                    ledger.total_withdrawn_atoms += amount;
                    ledger.last_observed_insurance_atoms = available - amount;
                }
                let long = amount.min(self.budgets[2 * asset]);
                let consumed = [long, amount - long];
                for (side, atoms) in consumed.into_iter().enumerate() {
                    self.budgets[2 * asset + side] -= atoms;
                }
                let mut paid = [0; 2];
                paid[rail] = amount;
                assert!(
                    self.receipts
                        .insert(
                            (asset, epoch),
                            Receipt {
                                signed_amount: amount,
                                consumed,
                                paid,
                            }
                        )
                        .is_none(),
                    "an intent cannot consume a second stock tranche"
                );
                self.custody[rail] -= amount;
                self.controls[asset].authority_epoch += 1;
            }
            Op::LateError => panic!("a rejected suffix has no committed effect"),
        }
    }

    fn payments(&self) -> [[u128; 2]; 2] {
        let mut paid = [[0; 2]; 2];
        let mut consumed = [0; 4];
        for (&(asset, _epoch), receipt) in &self.receipts {
            assert_eq!(receipt.consumed.iter().sum::<u128>(), receipt.signed_amount);
            assert_eq!(receipt.paid.iter().sum::<u128>(), receipt.signed_amount);
            for side in 0..2 {
                consumed[2 * asset + side] += receipt.consumed[side];
                paid[asset][side] += receipt.paid[side];
            }
        }
        for domain in 0..4 {
            assert_eq!(
                self.credited[domain],
                self.budgets[domain] + consumed[domain]
            );
        }
        paid
    }

    fn accepts_payments(&self, observed: [[u128; 2]; 2]) -> bool {
        observed == self.payments()
    }
}

#[derive(Default)]
struct Evidence {
    transactions: usize,
    rollbacks: usize,
    restored_transfers: usize,
    peak: u64,
    signatures: BTreeSet<solana_sdk::signature::Signature>,
}

#[test]
fn v16_generated_live_insurance_stock_epochs_preserve_retained_rail_entitlement() {
    let mut evidence = Evidence::default();
    let mut shapes = BTreeSet::new();
    let mut outcomes = BTreeMap::new();
    for seed in 0..4u64 {
        let mut rng = XorShiftRng::seed_from_u64(0x5154_4f43_4b00 + seed);
        let long = rng.gen_range(1..10u128);
        let short = rng.gen_range(13..27u128);
        let first_amount = long + rng.gen_range(1..short);
        let refills: [u128; 3] = std::array::from_fn(|_| rng.gen_range(31..48));
        let peer_amounts: [u128; 3] = std::array::from_fn(|_| rng.gen_range(2..10));
        assert!(shapes.insert((long, short, first_amount, refills, peer_amounts)));
        for asset in 0..2 {
            for order in ORDERS {
                let context = format!("seed={seed} asset={asset} order={order:?}");
                let mut env = inv018_public_spl_market_with_params(
                    6,
                    V16CuMarketParams {
                        max_portfolio_assets: 2,
                        ..V16CuMarketParams::default()
                    },
                );
                let admin = env.admin.insecure_clone();
                let secondary =
                    inv018_create_public_spl_mint(&mut env.svm, &env.payer, admin.pubkey(), 6);
                env.update_base_unit_mints_with_cu(env.mint, secondary);
                let mints = [env.mint, secondary];
                let vaults = [
                    env.vault,
                    create_ata_for_test(&mut env.svm, &env.payer, env.vault_authority, secondary),
                ];
                let sources = mints.map(|mint| {
                    create_ata_for_test(&mut env.svm, &env.payer, admin.pubkey(), mint)
                });
                let destinations: [[Pubkey; 2]; 2] = std::array::from_fn(|_| {
                    mints.map(|mint| {
                        let key = Keypair::new();
                        system_create_account_for_test(
                            &mut env.svm,
                            &env.payer,
                            &key,
                            TokenAccount::LEN,
                            spl_token::ID,
                        );
                        send_raw_tx(
                            &mut env.svm,
                            &env.payer,
                            spl_token::instruction::initialize_account3(
                                &spl_token::ID,
                                &key.pubkey(),
                                &mint,
                                &admin.pubkey(),
                            )
                            .unwrap(),
                            &[],
                        )
                        .unwrap();
                        key.pubkey()
                    })
                });
                let ledgers = std::array::from_fn::<_, 2, _>(|_| {
                    let key = Keypair::new();
                    system_create_account_for_test(
                        &mut env.svm,
                        &env.payer,
                        &key,
                        state::insurance_ledger_account_len(),
                        env.program_id,
                    );
                    key.pubkey()
                });
                let mut initial = [113, 127, 113, 127];
                initial[2 * asset] = long;
                initial[2 * asset + 1] = short;
                let initial_total = initial.iter().sum::<u128>();
                let refill_total = refills.iter().sum::<u128>();
                let supplies = [
                    initial_total + refill_total,
                    first_amount - 1 + 3 * DONATION,
                ];
                for (rail, key, amount) in [
                    (0, sources[0], supplies[0]),
                    (1, sources[1], 3 * DONATION),
                    (1, vaults[1], first_amount - 1),
                ] {
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::mint_to(
                            &spl_token::ID,
                            &mints[rail],
                            &key,
                            &admin.pubkey(),
                            &[],
                            amount as u64,
                        )
                        .unwrap(),
                        &[&admin],
                    )
                    .unwrap();
                }
                for mint in mints {
                    send_raw_tx(
                        &mut env.svm,
                        &env.payer,
                        spl_token::instruction::set_authority(
                            &spl_token::ID,
                            &mint,
                            None,
                            spl_token::instruction::AuthorityType::MintTokens,
                            &admin.pubkey(),
                            &[],
                        )
                        .unwrap(),
                        &[&admin],
                    )
                    .unwrap();
                }
                for (domain, amount) in initial.into_iter().enumerate() {
                    env.send(
                        ProgInstruction::TopUpInsuranceDomain {
                            domain: domain as u16,
                            market_id: env.asset_market_id(domain as u16 / 2),
                            authority_epoch: 0,
                            intent_id: 0,
                            amount,
                        },
                        vec![
                            AccountMeta::new(admin.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(sources[0], false),
                            AccountMeta::new(vaults[0], false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[&admin],
                    )
                    .unwrap();
                }
                let initial_controls = [env.control_sequences(0), env.control_sequences(1)];
                let ids = [env.asset_market_id(0), env.asset_market_id(1)];
                let profiles = [0, 1].map(|i| {
                    state::read_asset_oracle_profile(
                        &env.svm.get_account(&env.market).unwrap().data,
                        i,
                    )
                    .unwrap()
                });
                let mint_frames = mints.map(|key| env.svm.get_account(&key).unwrap());
                let ledger_frames = ledgers.map(|key| env.svm.get_account(&key).unwrap());
                let tokens = [
                    sources[0],
                    sources[1],
                    destinations[0][0],
                    destinations[0][1],
                    destinations[1][0],
                    destinations[1][1],
                    vaults[0],
                    vaults[1],
                ];
                let token_frames = tokens.map(|key| env.svm.get_account(&key).unwrap());
                let initial_book = Book {
                    credited: initial,
                    budgets: initial,
                    sources: [refill_total, 3 * DONATION],
                    custody: [initial_total, first_amount - 1],
                    donated: 0,
                    controls: initial_controls,
                    receipts: BTreeMap::new(),
                    ledgers: [None, None],
                };
                let instruction = |book: &Book, op: &Op| match *op {
                    Op::Refill { domain, amount } => Instruction {
                        program_id: env.program_id,
                        accounts: vec![
                            AccountMeta::new(admin.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(sources[0], false),
                            AccountMeta::new(vaults[0], false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        data: ProgInstruction::TopUpInsuranceDomain {
                            domain: domain as u16,
                            market_id: ids[domain / 2],
                            authority_epoch: book.controls[domain / 2].authority_epoch,
                            intent_id: book.controls[domain / 2].insurance_top_up + 1,
                            amount,
                        }
                        .encode(),
                    },
                    Op::Debit {
                        asset,
                        epoch,
                        amount,
                        rail,
                        observed,
                    } => {
                        let mut accounts = vec![
                            AccountMeta::new(admin.pubkey(), true),
                            AccountMeta::new(env.market, false),
                            AccountMeta::new(destinations[asset][rail], false),
                            AccountMeta::new(vaults[rail], false),
                            AccountMeta::new_readonly(env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ];
                        if observed {
                            accounts.push(AccountMeta::new(ledgers[asset], false));
                        }
                        Instruction {
                            program_id: env.program_id,
                            accounts,
                            data: ProgInstruction::WithdrawInsuranceAsset {
                                asset_index: asset as u16,
                                market_id: ids[asset],
                                authority_epoch: epoch,
                                amount,
                            }
                            .encode(),
                        }
                    }
                    Op::Donate => spl_token::instruction::transfer(
                        &spl_token::ID,
                        &sources[1],
                        &vaults[1],
                        &admin.pubkey(),
                        &[],
                        DONATION as u64,
                    )
                    .unwrap(),
                    Op::LateError => spl_token::instruction::transfer(
                        &spl_token::ID,
                        &sources[0],
                        &destinations[asset][0],
                        &admin.pubkey(),
                        &[],
                        (supplies[0] + 1) as u64,
                    )
                    .unwrap(),
                };
                // The entire transaction inventory, including successor epochs and late
                // retries, is serialized before the first delivery. No env.send rebinding.
                let mut steps = Vec::new();
                let mut planned = initial_book.clone();
                let mut schedule =
                    |ops: Vec<Op>, error: Option<(u8, u32)>, completed: [usize; 2]| {
                        let mut tentative = planned.clone();
                        let mut ixs = vec![
                            heap_ix(),
                            ComputeBudgetInstruction::set_compute_unit_limit(
                                CU_LIMIT - steps.len() as u32,
                            ),
                        ];
                        for (index, op) in ops.iter().enumerate() {
                            ixs.push(instruction(&tentative, op));
                            if error.is_none_or(|(at, _)| index < usize::from(at)) {
                                tentative.apply(op, env.market, admin.pubkey());
                            }
                        }
                        if error.is_none() {
                            planned = tentative;
                        }
                        let tx = Transaction::new_signed_with_payer(
                            &ixs,
                            Some(&env.payer.pubkey()),
                            &[&env.payer, &admin],
                            env.svm.latest_blockhash(),
                        );
                        steps.push((
                            bincode::serialize(&tx).unwrap(),
                            planned.clone(),
                            error,
                            completed,
                        ));
                        planned.clone()
                    };
                let stale = PercolatorError::EngineStale as u32;
                let late = spl_token::error::TokenError::InsufficientFunds as u32;
                let variants = |book: &Book, amount, observed| {
                    let ops =
                        [0, 1].map(|rail| book.debit(asset, amount, rail, observed ^ (rail == 1)));
                    assert_eq!(
                        instruction(book, &ops[0]).data,
                        instruction(book, &ops[1]).data
                    );
                    ops
                };
                let retained = variants(&initial_book, first_amount, seed % 2 == 0);
                schedule(
                    vec![
                        initial_book.debit(1 - asset, 1, 0, true),
                        retained[1].clone(),
                    ],
                    Some((1, PercolatorError::InvalidTokenAccount as u32)),
                    [1, 1],
                );
                schedule(
                    vec![retained[0].clone(), Op::LateError],
                    Some((1, late)),
                    [1, 1],
                );
                let mut book = schedule(vec![retained[0].clone()], None, [1, 1]);
                let mut archive = vec![retained];
                for round in 0..3 {
                    let ops = [
                        Op::Refill {
                            domain: 2 * asset + (round + seed as usize) % 2,
                            amount: refills[round],
                        },
                        book.debit(1 - asset, peer_amounts[round], 0, round % 2 == 0),
                        Op::Donate,
                    ];
                    let prefix: Vec<_> = order.map(|index| ops[index].clone()).into();
                    let mut after_prefix = book.clone();
                    for op in &prefix {
                        after_prefix.apply(op, env.market, admin.pubkey());
                    }
                    let available = after_prefix.available(asset);
                    let amount = if round == 1 {
                        available
                    } else {
                        available / 2 + 1
                    };
                    let fresh = variants(&after_prefix, amount, (round + seed as usize) % 2 == 0);
                    let rail = (round + seed as usize) % 2;
                    let mut early = vec![archive[0][0].clone()];
                    early.extend(prefix.clone());
                    schedule(early, Some((0, stale)), [0, 0]);
                    let mut aborted = prefix.clone();
                    aborted.push(fresh[rail].clone());
                    aborted.push(archive[0][1 - rail].clone());
                    schedule(aborted, Some((4, stale)), [3, 4]);
                    let mut late_bundle = prefix.clone();
                    late_bundle.extend([fresh[rail].clone(), Op::LateError]);
                    schedule(late_bundle, Some((4, late)), [3, 4]);
                    schedule(prefix, None, [2, 3]);
                    // Both old routes now have ample physical custody. Their exact stale
                    // errors cannot be masked by the underfunded-rail preflight.
                    let mut retries: Vec<_> = archive.iter().flatten().cloned().collect();
                    if order[0] % 2 == 1 {
                        retries.reverse();
                    }
                    for old in retries {
                        schedule(vec![old], Some((0, stale)), [0, 0]);
                    }
                    schedule(
                        vec![fresh[rail].clone(), fresh[1 - rail].clone()],
                        Some((1, stale)),
                        [1, 1],
                    );
                    book = schedule(vec![fresh[rail].clone()], None, [1, 1]);
                    assert_eq!(book.available(asset) == 0, round == 1);
                    for old in &fresh {
                        schedule(vec![old.clone()], Some((0, stale)), [0, 0]);
                    }
                    archive.push(fresh);
                }
                // A funded rail and sibling stock do not extend the target's allowance.
                schedule(
                    vec![book.debit(asset, book.available(asset) + 1, 0, true)],
                    Some((0, PercolatorError::EngineLockActive as u32)),
                    [0, 0],
                );
                book = schedule(
                    vec![book.debit(asset, book.available(asset), 0, true)],
                    None,
                    [1, 1],
                );
                book = schedule(
                    vec![book.debit(1 - asset, book.available(1 - asset), 0, true)],
                    None,
                    [1, 1],
                );
                for old in archive.iter().rev().map(|pair| pair[1].clone()) {
                    schedule(vec![old], Some((0, stale)), [0, 0]);
                }
                assert_eq!(book.available(0) + book.available(1), 0);
                assert!(
                    book.custody.iter().sum::<u128>() > 0,
                    "raw surplus is not insurance"
                );
                drop(schedule);
                let check = |env: &V16CuEnv, book: &Book| {
                    let paid = book.payments();
                    let observed_payments = destinations
                        .map(|rails| rails.map(|key| u128::from(env.token_amount(key))));
                    assert!(
                        book.accepts_payments(observed_payments),
                        "{context}: per-intent destination attribution"
                    );
                    let group = env.market_state().1;
                    let remaining = book.budgets.iter().sum::<u128>();
                    assert_eq!(group.mode, MarketModeV16::Live);
                    assert_eq!(group.insurance_domain_budget, book.budgets, "{context}");
                    assert_eq!(group.insurance_domain_spent, [0; 4]);
                    assert_eq!(group.insurance_domain_budget_remaining_total, remaining);
                    assert_eq!(
                        (group.insurance, group.vault, group.c_tot),
                        (remaining, remaining, 0)
                    );
                    assert_eq!(group.materialized_portfolio_count, 0);
                    assert_eq!(group.pnl_pos_tot, 0);
                    assert_eq!(group.source_claim_bound_total_num, 0);
                    assert_eq!(group.source_insurance_credit_reserved_total_atoms, 0);
                    assert_eq!(
                        book.custody.iter().sum::<u128>(),
                        remaining + first_amount - 1 + book.donated
                    );
                    for target in 0..2 {
                        assert_eq!(
                            env.control_sequences(target),
                            book.controls[target],
                            "{context}"
                        );
                        assert_eq!(
                            book.controls[target].authority_epoch,
                            initial_controls[target].authority_epoch
                                + book
                                    .receipts
                                    .keys()
                                    .filter(|(owner, _)| *owner == target)
                                    .count() as u64
                        );
                        assert_eq!(env.asset_market_id(target as u16), ids[target]);
                        assert_eq!(
                            state::read_asset_oracle_profile(
                                &env.svm.get_account(&env.market).unwrap().data,
                                target
                            )
                            .unwrap(),
                            profiles[target]
                        );
                        let mut expected = ledger_frames[target].clone();
                        if let Some(record) = &book.ledgers[target] {
                            state::init_insurance_ledger(&mut expected.data, record).unwrap();
                        }
                        assert_eq!(env.svm.get_account(&ledgers[target]).unwrap(), expected);
                        assert_eq!(group.assets[target].oi_eff_long_q, 0);
                        assert_eq!(group.assets[target].oi_eff_short_q, 0);
                    }
                    let amounts = [
                        book.sources[0],
                        book.sources[1],
                        paid[0][0],
                        paid[0][1],
                        paid[1][0],
                        paid[1][1],
                        book.custody[0],
                        book.custody[1],
                    ];
                    for (index, key) in tokens.into_iter().enumerate() {
                        let mut expected = token_frames[index].clone();
                        let mut token = TokenAccount::unpack(&expected.data).unwrap();
                        token.amount = u64::try_from(amounts[index]).unwrap();
                        TokenAccount::pack(token, &mut expected.data).unwrap();
                        assert_eq!(
                            env.svm.get_account(&key).unwrap(),
                            expected,
                            "{context} token {index}"
                        );
                    }
                    for rail in 0..2 {
                        assert_eq!(
                            env.svm.get_account(&mints[rail]).unwrap(),
                            mint_frames[rail]
                        );
                        let mint = Mint::unpack(&mint_frames[rail].data).unwrap();
                        assert_eq!(mint.mint_authority, COption::None);
                        assert_eq!(u128::from(mint.supply), supplies[rail]);
                        assert_eq!(
                            book.sources[rail] + paid[0][rail] + paid[1][rail] + book.custody[rail],
                            supplies[rail]
                        );
                    }
                    let mut data = env.svm.get_account(&env.market).unwrap().data;
                    state::market_view_mut(&mut data)
                        .unwrap()
                        .1
                        .validate_shape()
                        .unwrap();
                };
                check(&env, &initial_book);
                let watched: Vec<_> = tokens
                    .into_iter()
                    .chain(mints)
                    .chain(ledgers)
                    .chain([env.market, admin.pubkey(), env.vault_authority])
                    .collect();
                for (index, (wire, expected, error, completed)) in steps.into_iter().enumerate() {
                    let tx: Transaction = bincode::deserialize(&wire).unwrap();
                    tx.verify().unwrap();
                    assert_eq!(bincode::serialize(&tx).unwrap(), wire);
                    assert!(evidence.signatures.insert(tx.signatures[0]));
                    let before: BTreeMap<_, _> = watched
                        .iter()
                        .chain(&tx.message.account_keys)
                        .map(|key| (*key, env.svm.get_account(key)))
                        .collect();
                    let mut payer = env.svm.get_account(&env.payer.pubkey()).unwrap();
                    payer.lamports -= FeeStructure::default().lamports_per_signature
                        * u64::from(tx.message.header.num_required_signatures);
                    let result = env.svm.send_transaction(tx);
                    let meta = if let Some((at, code)) = error {
                        let failure = result
                            .expect_err("rejected retained intent must restore its entire prefix");
                        assert_eq!(
                            failure.err,
                            TransactionError::InstructionError(
                                2 + at,
                                InstructionError::Custom(code)
                            ),
                            "{context} step {index}: {:?}",
                            failure.meta.logs
                        );
                        for (key, account) in before {
                            if key != env.payer.pubkey() {
                                assert_eq!(
                                    env.svm.get_account(&key),
                                    account,
                                    "{context} step {index} rollback {key}"
                                );
                            }
                        }
                        evidence.rollbacks += 1;
                        evidence.restored_transfers += completed[1];
                        failure.meta
                    } else {
                        result
                            .unwrap_or_else(|failure| panic!("{context} step {index}: {failure:?}"))
                    };
                    for (program, count) in
                        [env.program_id, spl_token::ID].into_iter().zip(completed)
                    {
                        assert_eq!(
                            meta.logs
                                .iter()
                                .filter(|line| **line == format!("Program {program} success"))
                                .count(),
                            count,
                            "{context} step {index}"
                        );
                    }
                    assert_eq!(env.svm.get_account(&env.payer.pubkey()).unwrap(), payer);
                    assert!(meta.compute_units_consumed > 0);
                    assert_cu_within(
                        "Scope Q insurance stock epochs",
                        meta.compute_units_consumed,
                        u64::from(CU_LIMIT),
                    );
                    evidence.peak = evidence.peak.max(meta.compute_units_consumed);
                    evidence.transactions += 1;
                    check(&env, &expected);
                }
                let paid =
                    destinations.map(|rails| rails.map(|key| u128::from(env.token_amount(key))));
                let mut misattributed = paid;
                assert!(misattributed[asset][0] > 0);
                misattributed[asset][0] -= 1;
                misattributed[1 - asset][0] += 1;
                assert_eq!(
                    paid.iter().flatten().sum::<u128>(),
                    misattributed.iter().flatten().sum::<u128>()
                );
                assert!(
                    !book.accepts_payments(misattributed),
                    "a conserved atom at the wrong destination must fail the oracle"
                );
                let outcome = (
                    tokens.map(|key| env.token_amount(key)),
                    [env.control_sequences(0), env.control_sequences(1)],
                );
                if let Some(reference) = outcomes.insert((seed, asset), outcome.clone()) {
                    assert_eq!(
                        outcome, reference,
                        "{context}: all six landing orders preserve entitlement and epochs"
                    );
                }
            }
        }
    }
    assert_eq!(shapes.len(), 4);
    assert_eq!(outcomes.len(), 8);
    assert_eq!(
        (
            evidence.transactions,
            evidence.rollbacks,
            evidence.restored_transfers
        ),
        (2208, 1776, 1392)
    );
    assert_eq!(evidence.signatures.len(), evidence.transactions);
    eprintln!("Scope Q: 48 histories, {} transactions, {} exact rollbacks, {} restored SPL transfers, peak {} CU", evidence.transactions, evidence.rollbacks, evidence.restored_transfers, evidence.peak);
}
