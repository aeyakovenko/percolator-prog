//! INV-024/031/036/063/080: retained principal and earnings have separate stock
//! and expiry rules while backing encumbrance is nonzero. No withdrawal sequence
//! consumption or replay against replenished earnings/insurance is certified here.

use crate::support::{
    fuzz_model::{assert_public_encumbrance_census, assert_public_stock_census},
    v16_svm::{MarketConfig, V16Svm, ASSET_COUNT, PRIMARY_ACTOR_COUNT, TX_CU_LIMIT},
};
use percolator::{BackingBucketStatusV16, MarketModeV16, BOUND_SCALE, POS_SCALE};
use percolator_prog::{
    error::PercolatorError,
    ix::{CrankObservationHint, Instruction as ProgInstruction},
    processor, state,
};
use solana_sdk::{
    account::Account,
    compute_budget::ComputeBudgetInstruction,
    fee::FeeStructure,
    instruction::{AccountMeta, Instruction, InstructionError},
    program_pack::Pack,
    pubkey::Pubkey,
    signature::Signer,
    transaction::{Transaction, TransactionError},
};
use spl_token::state::Account as TokenAccount;
use std::collections::BTreeSet;

const PROVIDER: usize = 2;
const PAYER: usize = 4;
const DEPOSITS: [u128; PRIMARY_ACTOR_COUNT] = [52_502, 2_000_000, 0, 0, 0];
const BACKING: u128 = 100_000;
const PRINCIPAL: u128 = 137;
const EXPIRY: u64 = 5;
const RATE: u16 = 3_333;
const PROFIT: u128 = 1_000 * (105 - 100);
const LIEN: u128 = 1_050 * 105 / 2 - DEPOSITS[0];
const EARNINGS: u128 = (LIEN * RATE as u128).div_ceil(10_000);

fn payout(env: &V16Svm, asset: u16, earnings: bool) -> Instruction {
    let binding = (
        2 * asset + 1,
        env.primary_market_state().1.assets[asset as usize].market_id,
        env.primary_control_sequences(asset as usize)
            .authority_epoch,
    );
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
    let instruction = if earnings {
        ProgInstruction::WithdrawBackingBucketEarnings {
            domain: binding.0,
            market_id: binding.1,
            authority_epoch: binding.2,
            amount: EARNINGS,
        }
    } else {
        accounts.push(AccountMeta::new(env.backing_domain_ledger, false));
        ProgInstruction::WithdrawBackingBucket {
            domain: binding.0,
            market_id: binding.1,
            authority_epoch: binding.2,
            amount: PRINCIPAL,
        }
    };
    Instruction {
        program_id: env.program_id,
        accounts,
        data: instruction.encode(),
    }
}

fn sign(env: &V16Svm, instructions: &[Instruction], nonce: u32) -> Transaction {
    let mut message = vec![
        ComputeBudgetInstruction::request_heap_frame(256 * 1024),
        ComputeBudgetInstruction::set_compute_unit_limit(TX_CU_LIMIT as u32 - nonce),
    ];
    message.extend_from_slice(instructions);
    let tx = Transaction::new_signed_with_payer(
        &message,
        Some(&env.actors[PAYER].signer.pubkey()),
        &[&env.actors[PAYER].signer, &env.actors[PROVIDER].signer],
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
    simulations: usize,
    successes: usize,
    rollbacks: usize,
    transfers_rolled_back: usize,
    peak_cu: u64,
}

fn land(
    env: &mut V16Svm,
    tx: Transaction,
    rejection: Option<(u8, PercolatorError)>,
    transfers: usize,
    evidence: &mut Evidence,
) {
    let before = frame(env, &tx);
    let network_fee = FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let instruction_count = tx.message.instructions.len() - 2;
    let rejected_at = rejection.as_ref().map(|(index, _)| *index);
    let result = env.svm.send_transaction(tx);
    let meta = if let Some((index, code)) = rejection {
        let failure = result.expect_err("retained payout must respect current stock and expiry");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(index, InstructionError::Custom(code as u32))
        );
        evidence.rollbacks += 1;
        evidence.transfers_rolled_back += transfers;
        failure.meta
    } else {
        evidence.successes += 1;
        result.expect("retained payout remains authorized")
    };
    let changed = [
        env.market,
        env.backing_domain_ledger,
        env.vault,
        env.actors[PROVIDER].destination_token,
    ];
    for (key, mut original) in before {
        let payer = key == env.actors[PAYER].signer.pubkey();
        if payer {
            original.as_mut().unwrap().lamports -= network_fee;
        }
        let after = env.svm.get_account(&key);
        if rejected_at.is_some() || payer || !changed.contains(&key) {
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
    for (program, count) in [
        (spl_token::ID, transfers),
        (
            env.program_id,
            rejected_at.map_or(instruction_count, |index| usize::from(index - 2)),
        ),
    ] {
        assert_eq!(
            meta.logs
                .iter()
                .filter(|line| **line == format!("Program {program} success"))
                .count(),
            count
        );
    }
    assert!(
        meta.compute_units_consumed > 0 && meta.compute_units_consumed <= TX_CU_LIMIT,
        "reserve payout used {} CU",
        meta.compute_units_consumed
    );
    evidence.peak_cu = evidence.peak_cu.max(meta.compute_units_consumed);
}

#[test]
fn v16_program_retained_principal_expiry_preserves_encumbered_backing_and_earned_fee_stock() {
    assert_eq!((LIEN, EARNINGS), (2_623, 875));
    let mut evidence = Evidence::default();
    let mut worlds = 0;
    for asset in [0u16, 1] {
        for slot in [EXPIRY - 1, EXPIRY, EXPIRY + 1] {
            for principal_first in [false, true] {
                let domain = usize::from(2 * asset + 1);
                let mut env = V16Svm::new(
                    [0xd3; 32],
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
                let sequences: [_; ASSET_COUNT] =
                    std::array::from_fn(|i| env.primary_control_sequences(i));
                let profiles: [_; ASSET_COUNT] = std::array::from_fn(|i| env.primary_profile(i));
                let ledger =
                    state::read_backing_domain_ledger(&env.backing_domain_ledger_data()).unwrap();
                let tokens = env
                    .all_token_account_data()
                    .into_iter()
                    .map(|(key, _)| (key, env.svm.get_account(&key).unwrap()))
                    .collect::<Vec<_>>();
                let source = env
                    .primary_portfolio(0)
                    .source_domains
                    .into_iter()
                    .find(|source| source.is_occupied() && source.domain.get() as usize == domain)
                    .unwrap();
                assert_eq!(source.source_claim_bound_num.get(), PROFIT * BOUND_SCALE);
                assert_eq!(source.source_claim_liened_num.get(), LIEN * BOUND_SCALE);
                assert_eq!(
                    source.source_lien_counterparty_backing_num.get(),
                    LIEN * BOUND_SCALE
                );
                assert_eq!(source.source_claim_impaired_num.get(), 0);
                assert_eq!(initial.vault, DEPOSITS.iter().sum::<u128>() + BACKING);
                assert_eq!(
                    initial.source_backing_buckets[domain].fresh_unliened_backing_num,
                    (BACKING + PROFIT - LIEN) * BOUND_SCALE
                );
                assert_eq!(ledger.total_deposited_atoms, BACKING);
                let endowments = MarketConfig::default().actor_token_balances;
                for (i, actor) in env.actors.iter().enumerate() {
                    let funded = DEPOSITS[i] + if i == PROVIDER { BACKING } else { 0 };
                    assert_eq!(
                        env.token_amount(actor.source_token),
                        endowments[i] - funded as u64
                    );
                    assert_eq!(env.token_amount(actor.destination_token), 0);
                }

                let check = |env: &V16Svm, principal: u128, earnings: u128| {
                    let group = env.primary_market_state().1;
                    assert_eq!(group.mode, MarketModeV16::Live);
                    assert_eq!(group.vault, initial.vault - principal - earnings);
                    assert_eq!(
                        group.c_tot,
                        DEPOSITS.iter().sum::<u128>() - PROFIT - EARNINGS
                    );
                    assert_eq!(
                        env.primary_portfolio(0).capital.get(),
                        DEPOSITS[0] - EARNINGS
                    );
                    assert_eq!(env.primary_portfolio(0).pnl.get(), PROFIT as i128);
                    assert_eq!(env.primary_portfolio(1).capital.get(), DEPOSITS[1] - PROFIT);
                    assert_eq!(env.primary_portfolio(1).pnl.get(), 0);
                    assert_eq!(group.insurance, 0);
                    assert_eq!(
                        group.insurance_domain_budget,
                        initial.insurance_domain_budget
                    );
                    assert_eq!(group.backing_provider_earnings_total, EARNINGS - earnings);
                    for i in 0..ASSET_COUNT {
                        assert_eq!(env.primary_control_sequences(i), sequences[i]);
                        assert_eq!(env.primary_profile(i), profiles[i]);
                        assert_eq!(group.assets[i], initial.assets[i]);
                    }
                    for d in 0..group.source_backing_buckets.len() {
                        let mut bucket = initial.source_backing_buckets[d];
                        if d == domain {
                            bucket.fresh_unliened_backing_num -= principal * BOUND_SCALE;
                            bucket.utilization_fee_earnings -= earnings;
                            assert_eq!(bucket.status, BackingBucketStatusV16::Fresh);
                            assert_eq!(bucket.valid_liened_backing_num, LIEN * BOUND_SCALE);
                            assert_eq!(
                                group.source_credit[d].fresh_reserved_backing_num,
                                (BACKING + PROFIT - principal) * BOUND_SCALE
                            );
                        } else {
                            assert_eq!(group.source_credit[d], initial.source_credit[d]);
                        }
                        assert_eq!(group.source_backing_buckets[d], bucket);
                    }
                    let mut expected = ledger;
                    expected.total_principal_atoms -= principal;
                    expected.total_principal_withdrawn_atoms += principal;
                    if principal + earnings != 0 {
                        expected.total_earnings_atoms += EARNINGS;
                        expected.last_observed_bucket_earnings_atoms = EARNINGS - earnings;
                        expected.total_earnings_withdrawn_atoms += earnings;
                    }
                    assert_eq!(
                        state::read_backing_domain_ledger(&env.backing_domain_ledger_data())
                            .unwrap(),
                        expected
                    );
                    for (key, original) in &tokens {
                        let mut expected = original.clone();
                        let mut token = TokenAccount::unpack(&expected.data).unwrap();
                        if *key == env.vault {
                            assert_eq!((token.owner, token.mint), (env.vault_authority, env.mint));
                            token.amount -= (principal + earnings) as u64;
                        } else if *key == env.actors[PROVIDER].destination_token {
                            assert_eq!(
                                (token.owner, token.mint),
                                (env.actors[PROVIDER].signer.pubkey(), env.mint)
                            );
                            token.amount += (principal + earnings) as u64;
                        }
                        TokenAccount::pack(token, &mut expected.data).unwrap();
                        assert_eq!(env.svm.get_account(key), Some(expected));
                    }
                    assert_eq!(env.token_supply_observed(), env.initial_token_supply);
                    assert_public_stock_census("retained reserve expiry stock", env).unwrap();
                    assert_public_encumbrance_census("retained reserve expiry stock", env).unwrap();
                };
                check(&env, 0, 0);
                let principal = payout(&env, asset, false);
                let earnings = payout(&env, asset, true);
                let ordered = if principal_first {
                    [principal.clone(), earnings.clone()]
                } else {
                    [earnings.clone(), principal.clone()]
                };
                let bundle = sign(&env, &ordered, 1);
                let principal = sign(&env, &[principal], 2);
                let fees = sign(&env, &[earnings.clone()], 3);
                let exhausted = sign(&env, &[earnings], 4);
                assert_ne!(fees.signatures, exhausted.signatures);
                assert_eq!(
                    fees.message.instructions[2],
                    exhausted.message.instructions[2]
                );
                let wires = [&bundle, &principal, &fees, &exhausted]
                    .map(|tx| bincode::serialize(tx).unwrap());
                for tx in [&bundle, &principal, &fees, &exhausted] {
                    let before = frame(&env, tx);
                    env.svm.simulate_transaction(tx.clone().into()).unwrap();
                    assert_eq!(frame(&env, tx), before);
                    evidence.simulations += 1;
                }
                env.warp_to_slot(slot);
                assert_eq!(env.primary_market_state().1.current_slot, 2);
                check(&env, 0, 0);
                assert_eq!(bincode::serialize(&bundle).unwrap(), wires[0]);
                let paid_principal = if slot < EXPIRY {
                    land(&mut env, bundle, None, 2, &mut evidence);
                    PRINCIPAL
                } else {
                    let index = if principal_first { 2 } else { 3 };
                    land(
                        &mut env,
                        bundle,
                        Some((index, PercolatorError::EngineStale)),
                        usize::from(!principal_first),
                        &mut evidence,
                    );
                    check(&env, 0, 0);
                    assert_eq!(bincode::serialize(&principal).unwrap(), wires[1]);
                    land(
                        &mut env,
                        principal,
                        Some((2, PercolatorError::EngineStale)),
                        0,
                        &mut evidence,
                    );
                    check(&env, 0, 0);
                    assert_eq!(bincode::serialize(&fees).unwrap(), wires[2]);
                    land(&mut env, fees, None, 1, &mut evidence);
                    0
                };
                check(&env, paid_principal, EARNINGS);
                // Remaining principal and custody exceed this request; only the earned-fee
                // stock is exhausted. A distinct pre-signed envelope rules out cache rejection.
                assert!(env.primary_market_state().1.vault > EARNINGS);
                assert_eq!(bincode::serialize(&exhausted).unwrap(), wires[3]);
                land(
                    &mut env,
                    exhausted,
                    Some((2, PercolatorError::EngineLockActive)),
                    0,
                    &mut evidence,
                );
                check(&env, paid_principal, EARNINGS);
                worlds += 1;
            }
        }
    }
    assert_eq!(
        (
            worlds,
            evidence.simulations,
            evidence.successes,
            evidence.rollbacks,
            evidence.transfers_rolled_back
        ),
        (12, 48, 12, 28, 4)
    );
    println!("retained reserve expiry: 12 worlds, 48 simulations, 12 successes, 28 exact rollbacks, 4 rolled-back earned-fee SPL payouts; peak CU {}", evidence.peak_cu);
}
