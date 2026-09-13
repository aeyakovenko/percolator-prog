//! INV-008/010/012/080: only committed grant updates consume owner consent.
//! Competing owner-local updates may invalidate either end of a retained joint
//! update. Its untouched owner's separately retained update must remain usable.

use super::*;
use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

#[path = "inv_012_mixed_episode_renewal.rs"]
mod mixed_episode_renewal;

#[derive(Default)]
struct Evidence {
    worlds: usize,
    grants: usize,
    rejections: usize,
    rolled_back_prefixes: usize,
    fills: usize,
    max_cu: u64,
}

fn grant_ix(env: &V16Svm, grant: GrantOracle, cap: Option<u16>, expiry: u64) -> Instruction {
    let mut accounts = vec![
        AccountMeta::new(grant.scope[3], true),
        AccountMeta::new_readonly(env.market, false),
        AccountMeta::new(grant.scope[2], false),
    ];
    if cap.is_some() {
        accounts.extend(
            grant.scope[4..]
                .iter()
                .map(|key| AccountMeta::new_readonly(*key, false)),
        );
    }
    Instruction {
        program_id: env.program_id,
        accounts,
        data: ProgInstruction::SetMatcherConfig {
            portfolio_id: grant.portfolio_id,
            expected_sequence: grant.sequence,
            enabled: u8::from(cap.is_some()),
            trade_fee_cap_bps: cap.unwrap_or(0),
            expiry_slot: expiry,
        }
        .encode(),
    }
}

fn sign(env: &V16Svm, ixs: &[Instruction], nonce: u32) -> Transaction {
    let payer = &env.actors[4].signer;
    let mut instructions = vec![ComputeBudgetInstruction::set_compute_unit_limit(
        TX_CU_LIMIT as u32 - nonce,
    )];
    instructions.extend_from_slice(ixs);
    let mut signers = vec![payer];
    signers.extend(env.actors[..4].iter().filter_map(|actor| {
        ixs.iter()
            .flat_map(|ix| &ix.accounts)
            .any(|meta| meta.is_signer && meta.pubkey == actor.signer.pubkey())
            .then_some(&actor.signer)
    }));
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &signers,
        env.svm.latest_blockhash(),
    );
    tx.verify()
        .expect("every retained message is signed before delivery");
    assert!(bincode::serialized_size(&tx).unwrap() <= solana_sdk::packet::PACKET_DATA_SIZE as u64);
    tx
}

fn deliver(
    env: &mut V16Svm,
    history: &mut AuthorizationHistory,
    tx: Transaction,
    event: Option<AuthorizationEvent>,
    stale_index: Option<usize>,
    evidence: &mut Evidence,
) {
    history.assert_prefix(env);
    let mut keys: Vec<_> = capability_frame(env, history)
        .into_iter()
        .map(|(key, _)| key)
        .chain(tx.message.account_keys.iter().copied())
        .collect();
    keys.sort_unstable();
    keys.dedup();
    let before: Vec<_> = keys
        .into_iter()
        .map(|key| (key, env.svm.get_account(&key)))
        .collect();
    let portfolios: Vec<_> = (0..env.actors.len())
        .map(|actor| bytemuck::bytes_of(&env.primary_portfolio(actor)).to_vec())
        .collect();
    let result = env.svm.send_transaction(tx);
    let changed_portfolio = event.map(|event| match event {
        AuthorizationEvent::Grant { actor, .. } => env.actors[actor].portfolio,
        _ => unreachable!(),
    });
    let meta = if let Some(index) = stale_index {
        let failure = result.expect_err("a competing committed grant consumes only its own scope");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(
                (index + 1) as u8,
                InstructionError::Custom(PercolatorError::EngineStale as u32),
            ),
        );
        assert_eq!(
            failure
                .meta
                .logs
                .iter()
                .filter(|line| **line == format!("Program {} success", env.program_id))
                .count(),
            index,
            "the valid grant prefix must execute before the stale suffix rejects",
        );
        evidence.rejections += 1;
        evidence.rolled_back_prefixes += index;
        failure.meta
    } else {
        let meta = result.expect("unconsumed retained owner consent must remain usable");
        history.accepted.push(event.expect("accepted grant event"));
        evidence.grants += 1;
        meta
    };
    for (key, account) in before {
        if key != env.actors[4].signer.pubkey()
            && (stale_index.is_some() || Some(key) != changed_portfolio)
        {
            assert_eq!(
                env.svm.get_account(&key),
                account,
                "complete Account frame at {key}"
            );
        }
    }
    for (actor, portfolio) in portfolios.iter().enumerate() {
        assert_eq!(
            bytemuck::bytes_of(&env.primary_portfolio(actor)),
            portfolio,
            "grant control cannot change owner {actor}'s economic state",
        );
    }
    assert!(meta.compute_units_consumed <= TX_CU_LIMIT);
    evidence.max_cu = evidence.max_cu.max(meta.compute_units_consumed);
    history.assert_prefix(env);
}

#[test]
fn v16_program_retained_joint_grants_consume_only_committed_owner_scopes() {
    let mut evidence = Evidence::default();
    for blocked in [1, 2] {
        for order in [[1, 2], [2, 1]] {
            for enabled in [false, true] {
                for route in [CpiRoute::Single, CpiRoute::Batch] {
                    let mut env = V16Svm::new(
                        [0xd2; 32],
                        MarketConfig {
                            initial_price: 100,
                            actor_deposits: [1_000_000; 5],
                            actor_token_balances: [2_000_000; 5],
                            ..MarketConfig::default()
                        },
                    );
                    let mut history = AuthorizationHistory::new(&env);
                    let initial = history.replay();
                    let survivor = 3 - blocked;
                    let cap = |actor| enabled.then_some(if actor == 1 { 31 } else { 47 });
                    let expiry = |actor| if enabled { 20 + actor as u64 } else { 0 };
                    let event = |actor, cap, expiry| AuthorizationEvent::Grant {
                        actor,
                        cap,
                        expiry,
                        matcher_scope: initial[actor].scope[4..].try_into().unwrap(),
                    };
                    let grants: Vec<_> = (1..=2)
                        .map(|actor| grant_ix(&env, initial[actor], cap(actor), expiry(actor)))
                        .collect();
                    let bundle = sign(
                        &env,
                        &[grants[order[0] - 1].clone(), grants[order[1] - 1].clone()],
                        1,
                    );
                    let survivor_tx = sign(&env, &[grants[survivor - 1].clone()], 2);
                    let survivor_retry = sign(&env, &[grants[survivor - 1].clone()], 3);
                    let blocked_tx = sign(&env, &[grants[blocked - 1].clone()], 4);
                    let winner = sign(&env, &[grant_ix(&env, initial[blocked], Some(53), 40)], 5);
                    assert_eq!(
                        survivor_tx.message.instructions[1],
                        survivor_retry.message.instructions[1]
                    );
                    assert_ne!(survivor_tx.signatures, survivor_retry.signatures);
                    let before = capability_frame(&env, &history);
                    for tx in [&bundle, &survivor_tx, &blocked_tx, &winner] {
                        let simulation = env.svm.simulate_transaction(tx.clone().into()).expect(
                            "each exact retained authorization is live before the competing commit",
                        );
                        assert!(simulation.compute_units_consumed <= TX_CU_LIMIT);
                        assert_eq!(capability_frame(&env, &history), before);
                    }
                    deliver(
                        &mut env,
                        &mut history,
                        winner,
                        Some(event(blocked, Some(53), 40)),
                        None,
                        &mut evidence,
                    );
                    let stale_index = order.iter().position(|actor| *actor == blocked).unwrap();
                    deliver(
                        &mut env,
                        &mut history,
                        bundle,
                        None,
                        Some(stale_index),
                        &mut evidence,
                    );
                    assert_eq!(history.replay()[survivor], initial[survivor]);
                    deliver(
                        &mut env,
                        &mut history,
                        survivor_tx,
                        Some(event(survivor, cap(survivor), expiry(survivor))),
                        None,
                        &mut evidence,
                    );
                    deliver(
                        &mut env,
                        &mut history,
                        survivor_retry,
                        None,
                        Some(0),
                        &mut evidence,
                    );
                    deliver(
                        &mut env,
                        &mut history,
                        blocked_tx,
                        None,
                        Some(0),
                        &mut evidence,
                    );
                    for actor in [1, 2] {
                        assert_eq!(
                            history.replay()[actor].sequence,
                            initial[actor].sequence + 1
                        );
                    }
                    if !enabled {
                        grant_capability(&mut env, &mut history, survivor, Some(47), 30);
                    }
                    for lp in [1, 2] {
                        let size = POS_SCALE as i128;
                        let tx = match route {
                            CpiRoute::Single => env.build_retained_cpi_trade(0, lp, 0, size, 0),
                            CpiRoute::Batch => {
                                env.build_retained_batch_cpi_trade(0, lp, 0, size, 0)
                            }
                        };
                        capability_step(
                            &mut env,
                            &mut history,
                            true,
                            Some(AuthorizationEvent::Fill {
                                taker: 0,
                                lp,
                                asset: 0,
                                size,
                                cpi: true,
                            }),
                            |env| {
                                let result = env.land_retained(tx);
                                if let Ok(success) = &result {
                                    evidence.max_cu = evidence.max_cu.max(success.compute_units);
                                }
                                result
                            },
                        );
                        evidence.fills += 1;
                    }
                    let (_, market) = env.primary_market_state();
                    assert_eq!(market.assets[0].oi_eff_long_q, 2 * POS_SCALE);
                    assert_eq!(market.assets[0].oi_eff_short_q, 2 * POS_SCALE);
                    evidence.worlds += 1;
                }
            }
        }
    }
    assert_eq!(
        (
            evidence.worlds,
            evidence.grants,
            evidence.rejections,
            evidence.rolled_back_prefixes,
            evidence.fills
        ),
        (16, 32, 48, 8, 32)
    );
    println!("INV-012 retained joint grants: 16 worlds, 64 live simulations, 48 exact rejections, 8 rolled-back grant prefixes, 32 CPI fills; peak measured CU={}", evidence.max_cu);
}
