//! INV-008/014/024/064/080: a paid reserve request stays bound across replenishment
//! and operator succession. Authority ABA supplies the stale-request boundary;
//! standalone withdrawal consumption without an authority update remains unproved.
//! Funded insurer succession also composes with rolled-back payout prefixes when
//! the unchanged operator shares a peer asset or the provider's identity.

use crate::support::{
    fuzz_model::{assert_public_encumbrance_census, assert_public_stock_census},
    v16_svm::{MarketConfig, V16Svm, ASSET_COUNT, PRIMARY_ACTOR_COUNT, TX_CU_LIMIT},
};
use percolator_prog::{
    error::PercolatorError, ix::Instruction as ProgInstruction, processor, state,
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

const FUNDER: usize = 0;
const OPERATOR: usize = 1;
const SUCCESSOR: usize = 2;
const PEER: usize = 3;
const PAYER: usize = 4;
const INITIAL: [u128; 2] = [101, 211];
const PEER_STOCK: u128 = 43;
const LATE_TOP_UP: u128 = 17;

fn withdraw(env: &V16Svm, asset: u16, owner: usize, amount: u128) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(env.actors[owner].signer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.actors[owner].destination_token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::WithdrawInsuranceAsset {
            asset_index: asset,
            market_id: env.primary_market_state().1.assets[asset as usize].market_id,
            authority_epoch: env
                .primary_control_sequences(asset as usize)
                .authority_epoch,
            amount,
        }
        .encode(),
    }
}

fn top_up(env: &V16Svm, domain: u16, amount: u128) -> Instruction {
    let asset = domain as usize / 2;
    let sequences = env.primary_control_sequences(asset);
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(env.actors[FUNDER].signer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.actors[FUNDER].source_token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: ProgInstruction::TopUpInsuranceDomain {
            domain,
            market_id: env.primary_market_state().1.assets[asset].market_id,
            authority_epoch: sequences.authority_epoch,
            intent_id: sequences.insurance_top_up + 1,
            amount,
        }
        .encode(),
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
    peak_success_cu: u64,
    peak_rejection_cu: u64,
}

struct Books {
    budgets: [u128; 2 * ASSET_COUNT],
    paid: [u128; PRIMARY_ACTOR_COUNT],
    replenished: u128,
    sequences: [state::AssetControlSequencesV16; ASSET_COUNT],
    profiles: [state::AssetOracleProfileV16; ASSET_COUNT],
    market_ids: [u64; ASSET_COUNT],
    tokens: Vec<(Pubkey, Account, Pubkey)>,
    mint: Account,
    payer: Account,
    network_fees: u64,
}

impl Books {
    fn new(env: &V16Svm, asset: usize) -> Self {
        let mut budgets = [0; 2 * ASSET_COUNT];
        budgets[1] = PEER_STOCK;
        budgets[2 * asset..2 * asset + 2].copy_from_slice(&INITIAL);
        let endowments = MarketConfig::default().actor_token_balances;
        for (i, actor) in env.actors.iter().enumerate() {
            let funded = if i == FUNDER {
                u64::try_from(INITIAL.iter().sum::<u128>() + PEER_STOCK).unwrap()
            } else {
                0
            };
            assert_eq!(env.token_amount(actor.source_token), endowments[i] - funded);
            assert_eq!(env.token_amount(actor.destination_token), 0);
        }
        let mut tokens = vec![(
            env.vault,
            env.svm.get_account(&env.vault).unwrap(),
            env.vault_authority,
        )];
        for actor in &env.actors {
            for key in [actor.source_token, actor.destination_token] {
                tokens.push((
                    key,
                    env.svm.get_account(&key).unwrap(),
                    actor.signer.pubkey(),
                ));
            }
        }
        Self {
            budgets,
            paid: [0; PRIMARY_ACTOR_COUNT],
            replenished: 0,
            sequences: std::array::from_fn(|i| env.primary_control_sequences(i)),
            profiles: std::array::from_fn(|i| env.primary_profile(i)),
            market_ids: std::array::from_fn(|i| env.primary_market_state().1.assets[i].market_id),
            tokens,
            mint: env.svm.get_account(&env.mint).unwrap(),
            payer: env
                .svm
                .get_account(&env.actors[PAYER].signer.pubkey())
                .unwrap(),
            network_fees: 0,
        }
    }

    fn debit(&mut self, asset: usize, owner: usize, amount: u128) {
        let long = 2 * asset;
        let from_long = amount.min(self.budgets[long]);
        self.budgets[long] -= from_long;
        self.budgets[long + 1] -= amount - from_long;
        self.paid[owner] += amount;
    }

    fn credit(&mut self, domain: usize, amount: u128) {
        self.budgets[domain] += amount;
        self.replenished += amount;
        self.sequences[domain / 2].insurance_top_up += 1;
    }

    fn check(&self, env: &V16Svm) {
        let group = env.primary_market_state().1;
        let remaining = self.budgets.iter().sum::<u128>();
        assert_eq!(
            (group.insurance, group.vault, group.c_tot),
            (remaining, remaining, 0)
        );
        assert_eq!(group.mode, percolator::MarketModeV16::Live);
        for (domain, budget) in group.insurance_domain_budget.iter().enumerate() {
            assert_eq!(*budget, self.budgets.get(domain).copied().unwrap_or(0));
        }
        for i in 0..ASSET_COUNT {
            assert_eq!(group.assets[i].market_id, self.market_ids[i]);
            assert_eq!(env.primary_control_sequences(i), self.sequences[i]);
            assert_eq!(env.primary_profile(i), self.profiles[i]);
        }
        for actor in 0..PRIMARY_ACTOR_COUNT {
            assert_eq!(env.primary_portfolio(actor).capital.get(), 0);
        }
        for (key, original, owner) in &self.tokens {
            let mut expected = original.clone();
            let mut token = TokenAccount::unpack(&expected.data).unwrap();
            assert_eq!((token.mint, token.owner), (env.mint, *owner));
            if *key == env.vault {
                token.amount = u64::try_from(remaining).unwrap();
            } else if *key == env.actors[FUNDER].source_token {
                token.amount -= u64::try_from(self.replenished).unwrap();
            } else if let Some(actor) = env
                .actors
                .iter()
                .position(|actor| actor.destination_token == *key)
            {
                token.amount += u64::try_from(self.paid[actor]).unwrap();
            }
            TokenAccount::pack(token, &mut expected.data).unwrap();
            assert_eq!(
                env.svm.get_account(key),
                Some(expected),
                "complete SPL endpoint {key}"
            );
        }
        assert_eq!(env.svm.get_account(&env.mint), Some(self.mint.clone()));
        assert_eq!(env.token_supply_observed(), env.initial_token_supply);
        assert_eq!(u128::from(env.mint_supply()), env.initial_token_supply);
        assert_eq!(
            self.paid.iter().sum::<u128>() + remaining,
            INITIAL.iter().sum::<u128>() + PEER_STOCK + self.replenished
        );
        let mut payer = self.payer.clone();
        payer.lamports -= self.network_fees;
        assert_eq!(
            env.svm.get_account(&env.actors[PAYER].signer.pubkey()),
            Some(payer)
        );
        assert_public_stock_census("retained reserve replenishment", env).unwrap();
        assert_public_encumbrance_census("retained reserve replenishment", env).unwrap();
    }
}

fn simulate(env: &mut V16Svm, tx: &Transaction, evidence: &mut Evidence) {
    let before = frame(env, tx);
    let meta = env
        .svm
        .simulate_transaction(tx.clone().into())
        .expect("retained public request is executable");
    assert!(meta.compute_units_consumed > 0 && meta.compute_units_consumed <= 300_000);
    assert_eq!(
        frame(env, tx),
        before,
        "simulation consumes no signed stock"
    );
    evidence.simulations += 1;
}

fn land(
    env: &mut V16Svm,
    tx: Transaction,
    changed: &[Pubkey],
    rejection: Option<u8>,
    transfers: usize,
    books: &mut Books,
    evidence: &mut Evidence,
) {
    let before = frame(env, &tx);
    books.network_fees += FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let instruction_count = tx.message.instructions.len() - 2;
    let result = env.svm.send_transaction(tx);
    let meta = if let Some(index) = rejection {
        let failure = result.expect_err("superseded reserve authority epoch rejects");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(
                index,
                InstructionError::Custom(PercolatorError::EngineStale as u32)
            )
        );
        evidence.rollbacks += 1;
        evidence.transfers_rolled_back += transfers;
        evidence.peak_rejection_cu = evidence
            .peak_rejection_cu
            .max(failure.meta.compute_units_consumed);
        failure.meta
    } else {
        let success = result.expect("current signed reserve request succeeds");
        evidence.successes += 1;
        evidence.peak_success_cu = evidence.peak_success_cu.max(success.compute_units_consumed);
        success
    };
    for (key, account) in before {
        if key == env.actors[PAYER].signer.pubkey() {
            continue;
        }
        let after = env.svm.get_account(&key);
        if rejection.is_some() || !changed.contains(&key) {
            assert_eq!(after, account, "complete Account rollback/frame at {key}");
        } else {
            let (before, after) = (account.unwrap(), after.unwrap());
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
            rejection
                .map(|index| usize::from(index - 2))
                .unwrap_or(instruction_count),
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
    assert!(meta.compute_units_consumed > 0 && meta.compute_units_consumed <= 300_000);
    books.check(env);
}

#[test]
fn v16_program_paid_retained_insurance_stays_bound_after_replenishment_and_operator_aba() {
    let mut evidence = Evidence::default();
    let mut worlds = 0;
    for asset in [1u16, 2] {
        for (amount, refill_side) in [(137u128, 0), (312, 1)] {
            for replenish_first in [false, true] {
                let mut seed = [0xb8; 32];
                seed[0] = worlds;
                let mut env = V16Svm::new(
                    seed,
                    MarketConfig {
                        actor_deposits: [0; PRIMARY_ACTOR_COUNT],
                        ..MarketConfig::default()
                    },
                );
                for (target, operator) in [(asset, OPERATOR), (0, PEER)] {
                    for (role, owner) in [
                        (processor::ASSET_AUTH_INSURANCE, FUNDER),
                        (processor::ASSET_AUTH_INSURANCE_OPERATOR, operator),
                    ] {
                        env.update_asset_authority_from_admin(target, role, owner)
                            .unwrap();
                    }
                }
                for (domain, stock) in [
                    (2 * asset, INITIAL[0]),
                    (2 * asset + 1, INITIAL[1]),
                    (1, PEER_STOCK),
                ] {
                    env.top_up_insurance_domain_for_actor(FUNDER, domain, stock)
                        .unwrap();
                }
                let mut books = Books::new(&env, asset as usize);
                books.check(&env);
                let payload = withdraw(&env, asset, OPERATOR, amount);
                let paid = sign(&env, &[payload.clone()], 1);
                let stale = sign(&env, &[payload.clone()], 2);
                let peer = sign(&env, &[withdraw(&env, 0, PEER, PEER_STOCK)], 3);
                let retained_wire =
                    [&paid, &stale, &peer].map(|tx| bincode::serialize(tx).unwrap());
                assert_ne!(paid.signatures, stale.signatures);
                assert_eq!(paid.message.instructions[2], stale.message.instructions[2]);
                for tx in [&paid, &stale, &peer] {
                    simulate(&mut env, tx, &mut evidence);
                }
                let debit_accounts = [
                    env.market,
                    env.vault,
                    env.actors[OPERATOR].destination_token,
                ];
                assert_eq!(bincode::serialize(&paid).unwrap(), retained_wire[0]);
                books.debit(asset as usize, OPERATOR, amount);
                land(
                    &mut env,
                    paid,
                    &debit_accounts,
                    None,
                    1,
                    &mut books,
                    &mut evidence,
                );
                assert_eq!(
                    books.paid[OPERATOR], amount,
                    "original signed amount pays exactly once"
                );

                let domain = 2 * asset + refill_side;
                for replenish in [replenish_first, !replenish_first] {
                    if replenish {
                        let refill = sign(&env, &[top_up(&env, domain, amount)], 4);
                        books.credit(domain as usize, amount);
                        let changed = [env.market, env.vault, env.actors[FUNDER].source_token];
                        land(
                            &mut env,
                            refill,
                            &changed,
                            None,
                            1,
                            &mut books,
                            &mut evidence,
                        );
                    } else {
                        for (from, to) in [(OPERATOR, SUCCESSOR), (SUCCESSOR, OPERATOR)] {
                            let instruction = Instruction {
                                program_id: env.program_id,
                                accounts: vec![
                                    AccountMeta::new(env.actors[from].signer.pubkey(), true),
                                    AccountMeta::new(env.actors[to].signer.pubkey(), true),
                                    AccountMeta::new(env.market, false),
                                ],
                                data: ProgInstruction::UpdateAssetAuthority {
                                    asset_index: asset,
                                    market_id: books.market_ids[asset as usize],
                                    authority_epoch: books.sequences[asset as usize]
                                        .authority_epoch,
                                    kind: processor::ASSET_AUTH_INSURANCE_OPERATOR,
                                    new_pubkey: env.actors[to].signer.pubkey().to_bytes(),
                                }
                                .encode(),
                            };
                            let handoff = sign(&env, &[instruction], 5);
                            books.sequences[asset as usize].authority_epoch += 1;
                            books.profiles[asset as usize].insurance_operator =
                                env.actors[to].signer.pubkey().to_bytes();
                            let changed = [env.market];
                            land(
                                &mut env,
                                handoff,
                                &changed,
                                None,
                                0,
                                &mut books,
                                &mut evidence,
                            );
                        }
                    }
                }
                assert_eq!(
                    books.budgets[2 * asset as usize..2 * asset as usize + 2]
                        .iter()
                        .sum::<u128>(),
                    312
                );
                assert_eq!(bincode::serialize(&stale).unwrap(), retained_wire[1]);
                land(&mut env, stale, &[], Some(2), 0, &mut books, &mut evidence);

                // A real top-up prefix must restore its SPL transfer and sequence when the
                // retained withdrawal rejects. Its original standalone envelope can then land.
                let refill_ix = top_up(&env, domain, LATE_TOP_UP);
                let refill = sign(&env, &[refill_ix.clone()], 6);
                let refill_wire = bincode::serialize(&refill).unwrap();
                simulate(&mut env, &refill, &mut evidence);
                let bundle = sign(&env, &[refill_ix, payload.clone()], 7);
                assert_eq!(bundle.message.instructions[3].data, payload.data);
                land(&mut env, bundle, &[], Some(3), 1, &mut books, &mut evidence);
                assert_eq!(bincode::serialize(&refill).unwrap(), refill_wire);
                books.credit(domain as usize, LATE_TOP_UP);
                let changed = [env.market, env.vault, env.actors[FUNDER].source_token];
                land(
                    &mut env,
                    refill,
                    &changed,
                    None,
                    1,
                    &mut books,
                    &mut evidence,
                );
                assert_eq!(
                    books.paid[OPERATOR], amount,
                    "all old-payload deliveries share the original debit bound"
                );

                let fresh_amount = 312 + LATE_TOP_UP;
                let fresh = sign(&env, &[withdraw(&env, asset, OPERATOR, fresh_amount)], 8);
                books.debit(asset as usize, OPERATOR, fresh_amount);
                land(
                    &mut env,
                    fresh,
                    &debit_accounts,
                    None,
                    1,
                    &mut books,
                    &mut evidence,
                );
                assert_eq!(bincode::serialize(&peer).unwrap(), retained_wire[2]);
                books.debit(0, PEER, PEER_STOCK);
                let changed = [env.market, env.vault, env.actors[PEER].destination_token];
                land(&mut env, peer, &changed, None, 1, &mut books, &mut evidence);
                assert_eq!(env.token_amount(env.vault), 0);
                assert_eq!(books.paid, [0, amount + fresh_amount, 0, PEER_STOCK, 0]);
                assert_eq!(books.network_fees, 105_000);
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
        (8, 32, 56, 16, 8)
    );
    println!("INV-008 retained reserve: 8 worlds, 32 simulations, 56 successes, 16 exact rollbacks, 8 rolled-back SPL top-ups; peak success CU {}, rejection CU {}", evidence.peak_success_cu, evidence.peak_rejection_cu);
}

#[test]
fn v16_program_retained_insurance_payout_prefix_survives_rolled_back_insurer_succession() {
    const ASSET: usize = 1;
    const PARTIAL: u128 = 137;
    let mut evidence = Evidence::default();
    let mut worlds = 0;
    for operator in [OPERATOR, FUNDER] {
        for peer in [PEER, operator] {
            for handoff_first in [false, true] {
                let mut seed = [0xc8; 32];
                seed[0] = worlds;
                let mut env = V16Svm::new(
                    seed,
                    MarketConfig {
                        actor_deposits: [0; PRIMARY_ACTOR_COUNT],
                        ..MarketConfig::default()
                    },
                );
                for (asset, beneficiary) in [(ASSET as u16, operator), (0, peer)] {
                    for (role, owner) in [
                        (processor::ASSET_AUTH_INSURANCE, FUNDER),
                        (processor::ASSET_AUTH_INSURANCE_OPERATOR, beneficiary),
                    ] {
                        env.update_asset_authority_from_admin(asset, role, owner)
                            .unwrap();
                    }
                }
                for (domain, stock) in [(2, INITIAL[0]), (3, INITIAL[1]), (1, PEER_STOCK)] {
                    env.top_up_insurance_domain_for_actor(FUNDER, domain, stock)
                        .unwrap();
                }
                let mut books = Books::new(&env, ASSET);
                books.check(&env);
                let payload = withdraw(&env, ASSET as u16, operator, PARTIAL);
                let peer_payload = withdraw(&env, 0, peer, PEER_STOCK);
                let handoff_payload = Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new(env.actors[FUNDER].signer.pubkey(), true),
                        AccountMeta::new(env.actors[SUCCESSOR].signer.pubkey(), true),
                        AccountMeta::new(env.market, false),
                    ],
                    data: ProgInstruction::UpdateAssetAuthority {
                        asset_index: ASSET as u16,
                        market_id: books.market_ids[ASSET],
                        authority_epoch: books.sequences[ASSET].authority_epoch,
                        kind: processor::ASSET_AUTH_INSURANCE,
                        new_pubkey: env.actors[SUCCESSOR].signer.pubkey().to_bytes(),
                    }
                    .encode(),
                };
                let partial = sign(&env, &[payload.clone()], 11);
                let stale = sign(&env, &[payload.clone()], 12);
                let peer_tx = sign(&env, &[peer_payload.clone()], 13);
                let handoff = sign(&env, &[handoff_payload.clone()], 14);
                let retained_wire = [&partial, &stale, &peer_tx, &handoff]
                    .map(|tx| bincode::serialize(tx).unwrap());
                assert_ne!(partial.signatures, stale.signatures);
                assert_eq!(
                    partial.message.instructions[2],
                    stale.message.instructions[2]
                );
                for tx in [&partial, &peer_tx, &handoff] {
                    simulate(&mut env, tx, &mut evidence);
                }

                // Both withdrawals really pay before the stale suffix. The failed
                // transaction must restore their stock and the insurer's authority epoch.
                let middle = if handoff_first {
                    [handoff_payload, peer_payload.clone()]
                } else {
                    [peer_payload.clone(), handoff_payload]
                };
                let bundle = sign(
                    &env,
                    &[
                        payload.clone(),
                        middle[0].clone(),
                        middle[1].clone(),
                        payload.clone(),
                    ],
                    15,
                );
                land(&mut env, bundle, &[], Some(5), 2, &mut books, &mut evidence);
                assert_eq!(bincode::serialize(&partial).unwrap(), retained_wire[0]);
                let debit_accounts = [
                    env.market,
                    env.vault,
                    env.actors[operator].destination_token,
                ];
                books.debit(ASSET, operator, PARTIAL);
                land(
                    &mut env,
                    partial,
                    &debit_accounts,
                    None,
                    1,
                    &mut books,
                    &mut evidence,
                );
                assert_eq!(&books.budgets[2..4], &[0, 175]);
                let refill = sign(&env, &[top_up(&env, 2, PARTIAL)], 16);
                books.credit(2, PARTIAL);
                let changed = [env.market, env.vault, env.actors[FUNDER].source_token];
                land(
                    &mut env,
                    refill,
                    &changed,
                    None,
                    1,
                    &mut books,
                    &mut evidence,
                );
                assert_eq!(&books.budgets[2..4], &[137, 175]);

                assert_eq!(bincode::serialize(&handoff).unwrap(), retained_wire[3]);
                books.sequences[ASSET].authority_epoch += 1;
                books.profiles[ASSET].insurance_authority =
                    env.actors[SUCCESSOR].signer.pubkey().to_bytes();
                let changed = [env.market];
                land(
                    &mut env,
                    handoff,
                    &changed,
                    None,
                    0,
                    &mut books,
                    &mut evidence,
                );
                assert_eq!(
                    env.primary_profile(ASSET).insurance_operator,
                    env.actors[operator].signer.pubkey().to_bytes(),
                    "the signer and recipient remain authorized; only the bound epoch changed"
                );

                let (instructions, index, transfers) = if handoff_first {
                    ([payload.clone(), peer_payload], 2, 0)
                } else {
                    ([peer_payload, payload.clone()], 3, 1)
                };
                let stale_bundle = sign(&env, &instructions, 17);
                land(
                    &mut env,
                    stale_bundle,
                    &[],
                    Some(index),
                    transfers,
                    &mut books,
                    &mut evidence,
                );
                assert_eq!(bincode::serialize(&stale).unwrap(), retained_wire[1]);
                land(&mut env, stale, &[], Some(2), 0, &mut books, &mut evidence);
                assert_eq!(books.paid[operator], PARTIAL);

                // The peer retains its own asset epoch even when the SPL beneficiary
                // and funding provider share keys with the superseded target request.
                assert_eq!(bincode::serialize(&peer_tx).unwrap(), retained_wire[2]);
                books.debit(0, peer, PEER_STOCK);
                let changed = [env.market, env.vault, env.actors[peer].destination_token];
                land(
                    &mut env,
                    peer_tx,
                    &changed,
                    None,
                    1,
                    &mut books,
                    &mut evidence,
                );
                let fresh_amount = INITIAL.iter().sum();
                let fresh = sign(
                    &env,
                    &[withdraw(&env, ASSET as u16, operator, fresh_amount)],
                    18,
                );
                books.debit(ASSET, operator, fresh_amount);
                land(
                    &mut env,
                    fresh,
                    &debit_accounts,
                    None,
                    1,
                    &mut books,
                    &mut evidence,
                );
                let mut entitlement = [0; PRIMARY_ACTOR_COUNT];
                entitlement[operator] += PARTIAL + fresh_amount;
                entitlement[peer] += PEER_STOCK;
                assert_eq!(books.paid, entitlement);
                assert_eq!(books.paid[SUCCESSOR], 0);
                assert_eq!(env.token_amount(env.vault), 0);
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
            evidence.transfers_rolled_back,
        ),
        (8, 24, 40, 24, 20)
    );
    println!("INV-008 insurer succession: 8 worlds, 24 simulations, 40 successes, 24 exact rollbacks, 20 rolled-back SPL payouts; peak success CU {}, rejection CU {}", evidence.peak_success_cu, evidence.peak_rejection_cu);
}
