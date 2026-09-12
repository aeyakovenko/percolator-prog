//! INV-008/010/024/031: partial reserve payouts, two kinds of replenishment, and
//! retained earnings compose at one provider destination. Exact signed replay is
//! validator-cache evidence only; no withdrawal-specific stock sequence is claimed.

use super::retained_reserve_replenishment::{frame, sign};
use crate::support::{
    fuzz_model::{assert_public_encumbrance_census, assert_public_stock_census},
    v16_svm::{MarketConfig, V16Svm, ASSET_COUNT, PRIMARY_ACTOR_COUNT},
};
use percolator::{MarketModeV16, BOUND_SCALE, POS_SCALE};
use percolator_prog::{
    error::PercolatorError,
    ix::{CrankObservationHint, Instruction as ProgInstruction},
    processor, state,
};
use solana_sdk::{
    fee::FeeStructure,
    instruction::{AccountMeta, Instruction, InstructionError},
    program_pack::Pack,
    pubkey::Pubkey,
    signature::Signer,
    transaction::{Transaction, TransactionError},
};
use spl_token::state::Account as TokenAccount;

const PROVIDER: usize = 2;
const PAYER: usize = 4;
const DEPOSITS: [u128; PRIMARY_ACTOR_COUNT] = [52_502, 2_000_000, 0, 0, 0];
const BACKING: u128 = 100_000;
const PART: u128 = 137;
const RATE: u16 = 3_333;
const EXPIRY: u64 = 100;
const PROFIT: u128 = 1_000 * (105 - 100);
const FIRST_LIEN: u128 = 1_050 * 105 / 2 - DEPOSITS[0];
const FIRST_FEE: u128 = (FIRST_LIEN * RATE as u128).div_ceil(10_000);
const NEXT_LIEN: u128 = 1_060 * 105 / 2 - (DEPOSITS[0] - FIRST_FEE);
const NEXT_FEE: u128 = ((NEXT_LIEN - FIRST_LIEN) * RATE as u128).div_ceil(10_000);
const ORDERS: [[usize; 3]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];

fn payout(env: &V16Svm, asset: u16, earnings: bool, amount: u128) -> Instruction {
    let mut accounts = vec![
        AccountMeta::new(env.actors[PROVIDER].signer.pubkey(), true),
        AccountMeta::new(env.market, false),
    ];
    if earnings {
        accounts.push(AccountMeta::new(env.backing_domain_ledger, false));
    }
    accounts.extend([
        AccountMeta::new(env.actors[PROVIDER].destination_token, false),
        AccountMeta::new(env.vault, false),
        AccountMeta::new_readonly(env.vault_authority, false),
        AccountMeta::new_readonly(spl_token::ID, false),
    ]);
    let domain = 2 * asset + 1;
    let market_id = env.primary_market_state().1.assets[asset as usize].market_id;
    let authority_epoch = env
        .primary_control_sequences(asset as usize)
        .authority_epoch;
    let ix = if earnings {
        ProgInstruction::WithdrawBackingBucketEarnings {
            domain,
            market_id,
            authority_epoch,
            amount,
        }
    } else {
        accounts.push(AccountMeta::new(env.backing_domain_ledger, false));
        ProgInstruction::WithdrawBackingBucket {
            domain,
            market_id,
            authority_epoch,
            amount,
        }
    };
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ix.encode(),
    }
}

#[derive(Default)]
struct Evidence {
    successes: usize,
    rollbacks: usize,
    cached_retries: usize,
    spl_prefixes: usize,
    peak_cu: u64,
}

fn land(
    env: &mut V16Svm,
    tx: Transaction,
    changed: &[Pubkey],
    rejection: Option<TransactionError>,
    transfers: usize,
    evidence: &mut Evidence,
) {
    let before = frame(env, &tx);
    let cached = rejection == Some(TransactionError::AlreadyProcessed);
    let fee = if cached {
        0
    } else {
        FeeStructure::default().lamports_per_signature
            * u64::from(tx.message.header.num_required_signatures)
    };
    let instruction_count = tx.message.instructions.len() - 2;
    let result = env.svm.send_transaction(tx);
    let meta = if let Some(expected) = &rejection {
        let failure = result.expect_err("retained reserve rejection");
        assert_eq!(&failure.err, expected, "{:?}", failure.meta.logs);
        if cached {
            evidence.cached_retries += 1;
        } else {
            evidence.rollbacks += 1;
            evidence.spl_prefixes += transfers;
        }
        failure.meta
    } else {
        evidence.successes += 1;
        result.expect("authorized reserve continuation")
    };
    for (key, mut original) in before {
        let payer = key == env.actors[PAYER].signer.pubkey();
        if payer {
            original.as_mut().unwrap().lamports -= fee;
        }
        let after = env.svm.get_account(&key);
        if rejection.is_some() || payer || !changed.contains(&key) {
            assert_eq!(after, original, "complete Account frame at {key}");
        } else {
            let (before, after) = (original.unwrap(), after.unwrap());
            assert_eq!(
                (
                    after.lamports,
                    after.owner,
                    after.executable,
                    after.rent_epoch
                ),
                (
                    before.lamports,
                    before.owner,
                    before.executable,
                    before.rent_epoch
                )
            );
        }
    }
    let completed = match rejection {
        Some(TransactionError::AlreadyProcessed) => 0,
        Some(TransactionError::InstructionError(index, _)) => usize::from(index - 2),
        None => instruction_count,
        _ => unreachable!(),
    };
    for (program, count) in [(env.program_id, completed), (spl_token::ID, transfers)] {
        assert_eq!(
            meta.logs
                .iter()
                .filter(|line| **line == format!("Program {program} success"))
                .count(),
            count
        );
    }
    if !cached {
        assert!(
            meta.compute_units_consumed > 0 && meta.compute_units_consumed <= 1_000_000,
            "reserve history used {} CU",
            meta.compute_units_consumed
        );
    }
    evidence.peak_cu = evidence.peak_cu.max(meta.compute_units_consumed);
}

#[derive(Default)]
struct Books {
    principal_paid: u128,
    earnings_paid: u128,
    replenished: bool,
    accrued: bool,
    observed_fees: u128,
}

#[test]
fn v16_program_partial_reserve_payouts_preserve_replenished_earnings_across_orders() {
    assert_eq!(
        (FIRST_LIEN, FIRST_FEE, NEXT_LIEN, NEXT_FEE),
        (2_623, 875, 4_023, 467)
    );
    let mut evidence = Evidence::default();
    for asset in [0u16, 1] {
        let mut endpoint: Option<Vec<(Pubkey, Option<solana_sdk::account::Account>)>> = None;
        for order in ORDERS {
            let domain = usize::from(2 * asset + 1);
            let mut env = V16Svm::new(
                [0xb9; 32],
                MarketConfig {
                    initial_price: 100,
                    initial_margin_bps: 5_000,
                    maintenance_margin_bps: 1_000,
                    max_price_move_bps_per_slot: 500,
                    max_accrual_dt_slots: 1,
                    min_funding_lifetime_slots: 1,
                    actor_deposits: DEPOSITS,
                    ..MarketConfig::default()
                },
            );
            env.update_asset_authority_from_admin(
                asset,
                processor::ASSET_AUTH_BACKING_BUCKET,
                PROVIDER,
            )
            .unwrap();
            env.update_backing_fee_policy(domain as u16, RATE, 0)
                .unwrap();
            env.top_up_backing_bucket_for_actor(PROVIDER, domain as u16, BACKING, EXPIRY)
                .unwrap();
            env.trade_no_cpi(0, 1, asset, 1_000 * POS_SCALE as i128, 100, 0)
                .unwrap();
            env.warp_to_slot(2);
            env.push_auth_mark(asset, 2, 105).unwrap();
            for actor in [1, 0] {
                env.crank(
                    actor,
                    2,
                    vec![CrankObservationHint {
                        asset_index: asset,
                        oracle_accounts: 0,
                    }],
                )
                .unwrap();
            }
            env.trade_no_cpi_with_backing_fee_cap(
                0,
                1,
                asset,
                50 * POS_SCALE as i128,
                105,
                0,
                RATE,
            )
            .unwrap();
            let initial = env.primary_market_state().1;
            let ledger =
                state::read_backing_domain_ledger(&env.backing_domain_ledger_data()).unwrap();
            let sequences: [_; ASSET_COUNT] =
                std::array::from_fn(|i| env.primary_control_sequences(i));
            let profiles: [_; ASSET_COUNT] = std::array::from_fn(|i| env.primary_profile(i));
            let mint = env.svm.get_account(&env.mint);
            let tokens: Vec<_> = env
                .all_token_account_data()
                .into_iter()
                .map(|(key, _)| (key, env.svm.get_account(&key).unwrap()))
                .collect();
            let check = |env: &V16Svm, books: &Books| {
                let added = u128::from(books.replenished) * PART;
                let fees = FIRST_FEE + u128::from(books.accrued) * NEXT_FEE;
                let lien = if books.accrued { NEXT_LIEN } else { FIRST_LIEN };
                let paid = books.principal_paid + books.earnings_paid;
                let group = env.primary_market_state().1;
                assert_eq!(group.mode, MarketModeV16::Live);
                assert_eq!(
                    group.vault,
                    DEPOSITS.iter().sum::<u128>() + BACKING + added - paid
                );
                assert_eq!(group.c_tot, DEPOSITS.iter().sum::<u128>() - PROFIT - fees);
                assert_eq!(env.primary_portfolio(0).capital.get(), DEPOSITS[0] - fees);
                assert_eq!(env.primary_portfolio(0).pnl.get(), PROFIT as i128);
                assert_eq!(env.primary_portfolio(1).capital.get(), DEPOSITS[1] - PROFIT);
                assert_eq!(env.primary_portfolio(1).pnl.get(), 0);
                assert_eq!(group.insurance, 0);
                assert_eq!(
                    group.insurance_domain_budget,
                    initial.insurance_domain_budget
                );
                assert_eq!(
                    group.backing_provider_earnings_total,
                    fees - books.earnings_paid
                );
                for i in 0..ASSET_COUNT {
                    let mut expected = sequences[i];
                    if i == asset as usize {
                        expected.backing_top_up += u64::from(books.replenished);
                    } else {
                        assert_eq!(group.assets[i], initial.assets[i]);
                    }
                    assert_eq!(env.primary_control_sequences(i), expected);
                    assert_eq!(env.primary_profile(i), profiles[i]);
                }
                for d in 0..group.source_backing_buckets.len() {
                    let mut expected = initial.source_backing_buckets[d];
                    if d == domain {
                        expected.fresh_unliened_backing_num =
                            (BACKING + PROFIT + added - books.principal_paid - lien) * BOUND_SCALE;
                        expected.valid_liened_backing_num = lien * BOUND_SCALE;
                        expected.utilization_fee_earnings = fees - books.earnings_paid;
                        assert_eq!(
                            group.source_credit[d].fresh_reserved_backing_num,
                            (BACKING + PROFIT + added - books.principal_paid) * BOUND_SCALE
                        );
                    } else {
                        assert_eq!(group.source_credit[d], initial.source_credit[d]);
                    }
                    assert_eq!(group.source_backing_buckets[d], expected);
                }
                let mut expected = ledger;
                expected.total_deposited_atoms += added;
                expected.total_principal_atoms += added;
                expected.total_principal_atoms -= books.principal_paid;
                expected.total_principal_withdrawn_atoms += books.principal_paid;
                expected.total_earnings_atoms += books.observed_fees;
                expected.last_observed_bucket_earnings_atoms =
                    books.observed_fees - books.earnings_paid;
                expected.total_earnings_withdrawn_atoms += books.earnings_paid;
                assert_eq!(
                    state::read_backing_domain_ledger(&env.backing_domain_ledger_data()).unwrap(),
                    expected
                );
                for (key, original) in &tokens {
                    let mut expected = original.clone();
                    let mut token = TokenAccount::unpack(&expected.data).unwrap();
                    if *key == env.vault {
                        token.amount =
                            u64::try_from(u128::from(token.amount) + added - paid).unwrap();
                    } else if *key == env.actors[PROVIDER].destination_token {
                        token.amount += paid as u64;
                    } else if *key == env.actors[PROVIDER].source_token {
                        token.amount -= added as u64;
                    }
                    TokenAccount::pack(token, &mut expected.data).unwrap();
                    assert_eq!(env.svm.get_account(key), Some(expected));
                }
                assert_eq!(env.svm.get_account(&env.mint), mint);
                assert_eq!(env.token_supply_observed(), env.initial_token_supply);
                assert_public_stock_census("partial backing/earnings replenishment", env).unwrap();
                assert_public_encumbrance_census("partial backing/earnings replenishment", env)
                    .unwrap();
            };
            let mut books = Books::default();
            check(&env, &books);
            let principal = payout(&env, asset, false, PART);
            let partial = payout(&env, asset, true, PART);
            let residual = payout(&env, asset, true, FIRST_FEE - PART);
            // These standalone messages are frozen before either partial payout or refill.
            let partial_tx = sign(&env, &[partial], 1);
            let principal_tx = sign(&env, &[principal.clone()], 2);
            let residual_tx = sign(&env, &[residual.clone()], 3);
            let retained = [&partial_tx, &principal_tx, &residual_tx]
                .map(|tx| bincode::serialize(tx).unwrap());
            let payout_accounts = [
                env.market,
                env.backing_domain_ledger,
                env.vault,
                env.actors[PROVIDER].destination_token,
            ];
            for (tx, earnings) in [(&partial_tx, true), (&principal_tx, false)] {
                land(
                    &mut env,
                    tx.clone(),
                    &payout_accounts,
                    None,
                    1,
                    &mut evidence,
                );
                if earnings {
                    books.earnings_paid += PART;
                } else {
                    books.principal_paid += PART;
                }
                books.observed_fees = FIRST_FEE;
                check(&env, &books);
            }
            let refill = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(env.actors[PROVIDER].signer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(env.actors[PROVIDER].source_token, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(env.backing_domain_ledger, false),
                ],
                data: ProgInstruction::TopUpBackingBucket {
                    domain: domain as u16,
                    market_id: initial.assets[asset as usize].market_id,
                    authority_epoch: sequences[asset as usize].authority_epoch,
                    intent_id: sequences[asset as usize].backing_top_up + 1,
                    backing_fee_bps: RATE,
                    insurance_share_bps: 0,
                    amount: PART,
                    expiry_slot: EXPIRY,
                }
                .encode(),
            };
            let trade = Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(env.actors[0].signer.pubkey(), true),
                    AccountMeta::new(env.actors[1].signer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(env.actors[0].portfolio, false),
                    AccountMeta::new(env.actors[1].portfolio, false),
                ],
                data: ProgInstruction::TradeNoCpi {
                    account_a_portfolio_id: env.primary_portfolio_id(0),
                    account_a_position_epoch: env.primary_portfolio_position_epoch(0),
                    account_b_portfolio_id: env.primary_portfolio_id(1),
                    account_b_position_epoch: env.primary_portfolio_position_epoch(1),
                    asset_index: asset,
                    market_id: initial.assets[asset as usize].market_id,
                    size_q: 10 * POS_SCALE as i128,
                    exec_price: 105,
                    fee_bps: 0,
                    backing_fee_cap_bps: RATE,
                }
                .encode(),
            };
            let instructions = [refill, trade, residual];
            let transactions = [
                sign(&env, &[instructions[0].clone()], 4),
                sign(&env, &[instructions[1].clone()], 5),
                residual_tx.clone(),
            ];
            let overdraw = payout(&env, asset, true, FIRST_FEE + NEXT_FEE + 1);
            assert!(
                BACKING > FIRST_FEE + NEXT_FEE + 1,
                "earnings overdraw cannot borrow principal despite shared custody"
            );
            for kind in order {
                let failed = sign(
                    &env,
                    &[instructions[kind].clone(), overdraw.clone()],
                    10 + kind as u32,
                );
                land(
                    &mut env,
                    failed,
                    &[],
                    Some(TransactionError::InstructionError(
                        3,
                        InstructionError::Custom(PercolatorError::EngineLockActive as u32),
                    )),
                    usize::from(kind != 1),
                    &mut evidence,
                );
                check(&env, &books);
                let changed = match kind {
                    0 => vec![
                        env.market,
                        env.backing_domain_ledger,
                        env.vault,
                        env.actors[PROVIDER].source_token,
                    ],
                    1 => vec![env.market, env.actors[0].portfolio, env.actors[1].portfolio],
                    2 => payout_accounts.to_vec(),
                    _ => unreachable!(),
                };
                land(
                    &mut env,
                    transactions[kind].clone(),
                    &changed,
                    None,
                    usize::from(kind != 1),
                    &mut evidence,
                );
                match kind {
                    0 => books.replenished = true,
                    1 => books.accrued = true,
                    2 => books.earnings_paid += FIRST_FEE - PART,
                    _ => unreachable!(),
                }
                if kind != 1 {
                    books.observed_fees = FIRST_FEE + u128::from(books.accrued) * NEXT_FEE;
                }
                check(&env, &books);
                for tx in [&partial_tx, &principal_tx] {
                    land(
                        &mut env,
                        tx.clone(),
                        &[],
                        Some(TransactionError::AlreadyProcessed),
                        0,
                        &mut evidence,
                    );
                    check(&env, &books);
                }
            }
            assert_eq!(books.earnings_paid, FIRST_FEE);
            assert_eq!(
                env.primary_market_state().1.backing_provider_earnings_total,
                NEXT_FEE
            );
            for (tx, wire) in [&partial_tx, &principal_tx, &residual_tx]
                .into_iter()
                .zip(retained)
            {
                assert_eq!(bincode::serialize(tx).unwrap(), wire);
                land(
                    &mut env,
                    tx.clone(),
                    &[],
                    Some(TransactionError::AlreadyProcessed),
                    0,
                    &mut evidence,
                );
                check(&env, &books);
            }
            // New signatures authorize the later stock; the equal-amount principal
            // handler must leave all 467 newly earned atoms available to the fee handler.
            let fresh_principal = sign(&env, &[principal], 20);
            land(
                &mut env,
                fresh_principal,
                &payout_accounts,
                None,
                1,
                &mut evidence,
            );
            books.principal_paid += PART;
            books.observed_fees = FIRST_FEE + NEXT_FEE;
            check(&env, &books);
            let fresh_earnings = sign(&env, &[payout(&env, asset, true, NEXT_FEE)], 21);
            land(
                &mut env,
                fresh_earnings,
                &payout_accounts,
                None,
                1,
                &mut evidence,
            );
            books.earnings_paid += NEXT_FEE;
            check(&env, &books);
            assert_eq!(
                env.token_amount(env.actors[PROVIDER].destination_token),
                (2 * PART + FIRST_FEE + NEXT_FEE) as u64
            );
            // Replenishment changes the market risk epoch. Public recertification
            // removes the trade-order-dependent certificate snapshot before comparison.
            for actor in [0, 1] {
                let portfolio = env.actors[actor].portfolio;
                let refresh = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(env.actors[PAYER].signer.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(portfolio, false),
                    ],
                    data: ProgInstruction::PermissionlessCrank {
                        now_slot: 2,
                        observations: vec![],
                    }
                    .encode(),
                };
                let tx = sign(&env, &[refresh], 30 + actor as u32);
                land(&mut env, tx, &[portfolio], None, 0, &mut evidence);
                check(&env, &books);
                assert_eq!(
                    env.primary_portfolio(actor)
                        .health_cert
                        .cert_risk_epoch
                        .get(),
                    env.primary_market_state().1.risk_epoch
                );
            }
            let current: Vec<_> = frame(&env, &residual_tx)
                .into_iter()
                .filter(|(key, _)| *key != env.actors[PAYER].signer.pubkey())
                .collect();
            if let Some(expected) = &endpoint {
                assert_eq!(current.len(), expected.len());
                for ((key, actual), (expected_key, before)) in current.iter().zip(expected) {
                    assert_eq!(key, expected_key);
                    assert_eq!(actual, before, "order {order:?}: exact endpoint at {key}");
                }
            } else {
                endpoint = Some(current);
            }
        }
    }
    assert_eq!(
        (
            evidence.successes,
            evidence.rollbacks,
            evidence.cached_retries,
            evidence.spl_prefixes
        ),
        (108, 36, 108, 24)
    );
    println!("partial backing/earnings replenishment: 12 worlds, 108 successes, 36 exact rollbacks, 108 cached retries, 24 rolled-back SPL prefixes; peak CU {}", evidence.peak_cu);
}
