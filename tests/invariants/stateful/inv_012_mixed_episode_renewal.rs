//! INV-012 / row 412: joint renewal rollback preserves an automatic revocation.
//! Renewals retained after partial reduction mix a disabled subject and a live
//! peer. A competing peer grant must not consume the subject's independent retry.

use super::*;
use solana_sdk::fee::FeeStructure;

const PRICE: u64 = 100;
const OPEN: i128 = 12 * POS_SCALE as i128;
const REDUCTION: i128 = 3 * POS_SCALE as i128;
const DEPOSITS: [u128; 5] = [1_000_000, 2_000_000, 3_000_000, 4_000_000, 5_000_000];

#[derive(Default, Debug)]
struct Evidence {
    worlds: usize,
    transactions: usize,
    simulations: usize,
    rejections: usize,
    rolled_back_grants: usize,
    fills: usize,
    max_success_cu: u64,
    max_rejection_cu: u64,
    max_bundle_cu: u64,
    nonce: u32,
}

struct Retained {
    tx: Transaction,
    bytes: Vec<u8>,
}

fn retain(env: &V16Svm, instructions: &[Instruction], evidence: &mut Evidence) -> Retained {
    evidence.nonce += 1;
    let mut ixs = vec![ComputeBudgetInstruction::request_heap_frame(256 * 1024)];
    ixs.extend_from_slice(instructions);
    let tx = sign(env, &ixs, evidence.nonce);
    let bytes = bincode::serialize(&tx).unwrap();
    Retained { tx, bytes }
}

fn frame(
    env: &V16Svm,
    history: &AuthorizationHistory,
    retained: &Retained,
) -> Vec<(Pubkey, Option<Account>)> {
    capability_frame(env, history)
        .into_iter()
        .map(|(key, _)| key)
        .chain(retained.tx.message.account_keys.iter().copied())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(|key| (key, env.svm.get_account(&key)))
        .collect()
}

fn values(env: &V16Svm, history: &AuthorizationHistory) {
    history.assert_prefix(env);
    let group = env.primary_market_state().1;
    let total = DEPOSITS.iter().sum::<u128>();
    assert_eq!(
        (group.c_tot, group.vault, group.insurance),
        (total, total, 0)
    );
    assert_eq!(u128::from(env.token_amount(env.vault)), total);
    assert_eq!(u128::from(env.mint_supply()), env.initial_token_supply);
    for (actor, deposit) in DEPOSITS.into_iter().enumerate() {
        let portfolio = env.primary_portfolio(actor);
        assert_eq!(
            portfolio.owner,
            env.actors[actor].signer.pubkey().to_bytes()
        );
        assert_eq!(portfolio.capital.get(), deposit, "owner {actor} capital");
        assert_eq!(portfolio.pnl.get(), 0, "owner {actor} PnL");
        assert_eq!(env.token_amount(env.actors[actor].source_token), 0);
        assert_eq!(env.token_amount(env.actors[actor].destination_token), 0);
    }
    // The consumer asset has no cohort reduction; its OI is the committed fill sum.
    let grants = history.replay();
    let long: u128 = grants.iter().map(|g| g.positions[1].max(0) as u128).sum();
    let short: u128 = grants
        .iter()
        .map(|g| (-g.positions[1]).max(0) as u128)
        .sum();
    assert_eq!(long, short);
    assert_eq!(
        (
            group.assets[1].oi_eff_long_q,
            group.assets[1].oi_eff_short_q
        ),
        (long, short)
    );
}

fn simulate_live(
    env: &mut V16Svm,
    history: &AuthorizationHistory,
    retained: &Retained,
    evidence: &mut Evidence,
) {
    let before = frame(env, history, retained);
    assert_eq!(bincode::serialize(&retained.tx).unwrap(), retained.bytes);
    let simulation = env
        .svm
        .simulate_transaction(retained.tx.clone().into())
        .expect("the exact retained request is executable before the competing transition");
    assert!(simulation.compute_units_consumed <= TX_CU_LIMIT);
    assert_eq!(
        frame(env, history, retained),
        before,
        "simulation is not a committed writer"
    );
    evidence.simulations += 1;
}

fn deliver(
    env: &mut V16Svm,
    history: &mut AuthorizationHistory,
    retained: &Retained,
    events: &[AuthorizationEvent],
    rejection: Option<(u8, PercolatorError)>,
    evidence: &mut Evidence,
) {
    values(env, history);
    assert_eq!(bincode::serialize(&retained.tx).unwrap(), retained.bytes);
    retained.tx.verify().unwrap();
    let before = frame(env, history, retained);
    let fee = FeeStructure::default().lamports_per_signature
        * u64::from(retained.tx.message.header.num_required_signatures);
    let result = env.svm.send_transaction(retained.tx.clone());
    let failed = rejection.is_some();
    let meta = if let Some((index, error)) = rejection {
        let failure = result.expect_err("retained bundle or capability must reject exactly");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(index + 2, InstructionError::Custom(error as u32))
        );
        assert_eq!(
            failure
                .meta
                .logs
                .iter()
                .filter(|line| **line == format!("Program {} success", env.program_id))
                .count(),
            usize::from(index)
        );
        assert!(
            !failure
                .meta
                .logs
                .iter()
                .any(|line| line.starts_with(&format!("Program {} invoke", env.matcher_program))),
            "grant rollback and denied consumers must not invoke the matcher"
        );
        evidence.rejections += 1;
        evidence.rolled_back_grants += usize::from(index);
        evidence.max_rejection_cu = evidence
            .max_rejection_cu
            .max(failure.meta.compute_units_consumed);
        failure.meta
    } else {
        let meta = result.expect("current authorization and unconsumed renewal remain executable");
        assert_eq!(
            meta.logs
                .iter()
                .filter(|line| **line == format!("Program {} success", env.program_id))
                .count(),
            events.len()
        );
        assert_eq!(
            meta.logs
                .iter()
                .filter(|line| **line == format!("Program {} success", env.matcher_program))
                .count(),
            events
                .iter()
                .filter(|event| matches!(event, AuthorizationEvent::Fill { cpi: true, .. }))
                .count()
        );
        history.accepted.extend_from_slice(events);
        evidence.fills += events
            .iter()
            .filter(|event| matches!(event, AuthorizationEvent::Fill { .. }))
            .count();
        evidence.max_success_cu = evidence.max_success_cu.max(meta.compute_units_consumed);
        meta
    };
    let mut allowed = std::collections::BTreeSet::new();
    if !failed {
        for event in events {
            match *event {
                AuthorizationEvent::Grant { actor, .. } => {
                    allowed.insert(env.actors[actor].portfolio);
                }
                AuthorizationEvent::OwnerEpisode { actor, .. } => {
                    allowed.extend([env.market, env.actors[actor].portfolio]);
                }
                AuthorizationEvent::Fill { taker, lp, cpi, .. } => {
                    allowed.extend([
                        env.market,
                        env.actors[taker].portfolio,
                        env.actors[lp].portfolio,
                    ]);
                    if cpi {
                        allowed.insert(env.actors[lp].matcher_context);
                    }
                }
                _ => unreachable!(),
            }
        }
    }
    for (key, mut account) in before {
        if key == env.actors[4].signer.pubkey() {
            account.as_mut().unwrap().lamports -= fee;
        } else if allowed.contains(&key) {
            continue;
        }
        assert_eq!(
            env.svm.get_account(&key),
            account,
            "complete Account scope/rollback at {key}"
        );
    }
    assert!(meta.compute_units_consumed <= TX_CU_LIMIT);
    if retained.tx.message.instructions.len() == 4 {
        evidence.max_bundle_cu = evidence.max_bundle_cu.max(meta.compute_units_consumed);
    }
    evidence.transactions += 1;
    values(env, history);
}

fn consumer(
    env: &V16Svm,
    history: &AuthorizationHistory,
    taker: usize,
    lp: usize,
    asset: u16,
    size: i128,
    batch: bool,
) -> Instruction {
    let grants = history.replay();
    let a = grants[taker];
    let b = grants[lp];
    let market_id = env.primary_market_state().1.assets[asset as usize].market_id;
    let instruction = if batch {
        ProgInstruction::BatchTradeCpi {
            account_a_portfolio_id: a.portfolio_id,
            account_a_position_epoch: a.epoch,
            account_b_portfolio_id: b.portfolio_id,
            account_b_position_epoch: b.epoch,
            account_b_matcher_sequence: b.sequence,
            max_slippage_atoms: 0,
            max_fee_atoms: 0,
            legs: vec![BatchTradeCpiLeg {
                asset_index: asset,
                market_id,
                size_q: size,
                fee_bps: 0,
                limit_price: PRICE,
            }],
        }
    } else {
        ProgInstruction::TradeCpi {
            account_a_portfolio_id: a.portfolio_id,
            account_a_position_epoch: a.epoch,
            account_b_portfolio_id: b.portfolio_id,
            account_b_position_epoch: b.epoch,
            account_b_matcher_sequence: b.sequence,
            asset_index: asset,
            market_id,
            size_q: size,
            fee_bps: 0,
            limit_price: PRICE,
            backing_fee_cap_bps: 0,
        }
    };
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(a.scope[3], true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(a.scope[2], false),
            AccountMeta::new(b.scope[2], false),
            AccountMeta::new_readonly(b.scope[4], false),
            AccountMeta::new(b.scope[5], false),
            AccountMeta::new_readonly(b.scope[6], false),
        ],
        data: instruction.encode(),
    }
}

fn run_case(
    owner_reduce: bool,
    peer_first: bool,
    batch: bool,
    direction: i128,
    evidence: &mut Evidence,
) {
    let mut env = V16Svm::new(
        [0x4e; 32],
        MarketConfig {
            initial_price: PRICE,
            actor_deposits: DEPOSITS,
            actor_token_balances: DEPOSITS.map(|deposit| deposit as u64),
            ..MarketConfig::default()
        },
    );
    let mut history = AuthorizationHistory::new(&env);
    let grant_event = |actor, cap, expiry| AuthorizationEvent::Grant {
        actor,
        cap: Some(cap),
        expiry,
        matcher_scope: [
            env.matcher_program,
            env.actors[actor].matcher_context,
            env.actors[actor].matcher_delegate,
        ],
    };
    let subject_event = grant_event(1, 37, 100);
    let peer_event = grant_event(2, 47, 100);
    let winner_event = grant_event(2, 53, 120);
    let initial = history.replay();
    let setup = retain(
        &env,
        &[
            grant_ix(&env, initial[1], Some(37), 100),
            grant_ix(&env, initial[2], Some(47), 100),
        ],
        evidence,
    );
    deliver(
        &mut env,
        &mut history,
        &setup,
        &[subject_event, peer_event],
        None,
        evidence,
    );
    let fill = |taker, lp, asset, size, cpi| AuthorizationEvent::Fill {
        taker,
        lp,
        asset,
        size,
        cpi,
    };
    let open = retain(
        &env,
        &[consumer(&env, &history, 0, 1, 0, direction * OPEN, batch)],
        evidence,
    );
    deliver(
        &mut env,
        &mut history,
        &open,
        &[fill(0, 1, 0, direction * OPEN, true)],
        None,
        evidence,
    );

    let size = direction * POS_SCALE as i128;
    let old = history.replay();
    let old_consumer = retain(
        &env,
        &[consumer(&env, &history, 3, 1, 1, size, batch)],
        evidence,
    );
    simulate_live(&mut env, &history, &old_consumer, evidence);
    let (writer, event) = if owner_reduce {
        (
            Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(old[1].scope[3], true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(old[1].scope[2], false),
                ],
                data: ProgInstruction::RebalanceReduce {
                    portfolio_id: old[1].portfolio_id,
                    position_epoch: old[1].epoch,
                    asset_index: 0,
                    reduce_q: REDUCTION as u128,
                }
                .encode(),
            },
            AuthorizationEvent::OwnerEpisode {
                actor: 1,
                asset: 0,
                position_delta: direction * REDUCTION,
            },
        )
    } else {
        (
            Instruction {
                program_id: env.program_id,
                accounts: vec![
                    AccountMeta::new(old[0].scope[3], true),
                    AccountMeta::new(old[1].scope[3], true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(old[0].scope[2], false),
                    AccountMeta::new(old[1].scope[2], false),
                ],
                data: ProgInstruction::TradeNoCpi {
                    account_a_portfolio_id: old[0].portfolio_id,
                    account_a_position_epoch: old[0].epoch,
                    account_b_portfolio_id: old[1].portfolio_id,
                    account_b_position_epoch: old[1].epoch,
                    asset_index: 0,
                    market_id: env.primary_market_state().1.assets[0].market_id,
                    size_q: -direction * REDUCTION,
                    exec_price: PRICE,
                    fee_bps: 0,
                    backing_fee_cap_bps: 0,
                }
                .encode(),
            },
            fill(0, 1, 0, -direction * REDUCTION, false),
        )
    };
    let writer = retain(&env, &[writer], evidence);
    deliver(&mut env, &mut history, &writer, &[event], None, evidence);
    let reduced = history.replay();
    assert_eq!(reduced[1].epoch, old[1].epoch + 1);
    assert_eq!(reduced[1].sequence, old[1].sequence);
    assert_eq!(reduced[1].positions[0], -direction * (OPEN - REDUCTION));
    assert!(!reduced[1].enabled);
    assert_eq!(reduced[1].expiry, 0);
    assert_eq!(reduced[2], old[2], "peer grant and episode are untouched");

    // Retain renewal consent only after the committed reduction. This tests its
    // rollback/retry disposition, not admission of pre-revocation grant consent.
    let subject_ix = grant_ix(&env, reduced[1], Some(37), 100);
    let peer_ix = grant_ix(&env, reduced[2], Some(47), 100);
    let ordered = if peer_first {
        [peer_ix, subject_ix.clone()]
    } else {
        [subject_ix.clone(), peer_ix]
    };
    let bundle = retain(&env, &ordered, evidence);
    let bundle_retry = retain(&env, &ordered, evidence);
    let independent = retain(&env, &[subject_ix], evidence);
    assert_eq!(
        bundle.tx.message.instructions[2..],
        bundle_retry.tx.message.instructions[2..]
    );
    assert_ne!(bundle.tx.signatures, bundle_retry.tx.signatures);
    for request in [&bundle, &bundle_retry, &independent] {
        simulate_live(&mut env, &history, request, evidence);
    }
    let disabled = retain(
        &env,
        &[consumer(&env, &history, 3, 1, 1, size, !batch)],
        evidence,
    );
    let winner = retain(&env, &[grant_ix(&env, reduced[2], Some(53), 120)], evidence);
    deliver(
        &mut env,
        &mut history,
        &winner,
        &[winner_event],
        None,
        evidence,
    );
    let peer = retain(
        &env,
        &[consumer(&env, &history, 3, 2, 1, size, batch)],
        evidence,
    );
    simulate_live(&mut env, &history, &peer, evidence);
    let stale_index = u8::from(!peer_first);
    for request in [&bundle, &bundle_retry] {
        deliver(
            &mut env,
            &mut history,
            request,
            &[],
            Some((stale_index, PercolatorError::EngineStale)),
            evidence,
        );
        assert_eq!(
            history.replay()[1],
            reduced[1],
            "rolled-back renewal cannot revive the revoked grant"
        );
    }
    deliver(
        &mut env,
        &mut history,
        &old_consumer,
        &[],
        Some((0, PercolatorError::EngineStale)),
        evidence,
    );
    deliver(
        &mut env,
        &mut history,
        &disabled,
        &[],
        Some((0, PercolatorError::Unauthorized)),
        evidence,
    );
    assert_eq!(peer.tx.signatures.len(), 2, "no peer LP owner signature");
    deliver(
        &mut env,
        &mut history,
        &peer,
        &[fill(3, 2, 1, size, true)],
        None,
        evidence,
    );
    deliver(
        &mut env,
        &mut history,
        &independent,
        &[subject_event],
        None,
        evidence,
    );
    let renewed = history.replay();
    assert_eq!(renewed[1].epoch, reduced[1].epoch);
    assert_eq!(renewed[1].sequence, reduced[1].sequence + 1);
    assert!(renewed[1].enabled);
    assert_eq!(
        (renewed[1].scope, renewed[1].cap, renewed[1].expiry),
        (old[1].scope, old[1].cap, old[1].expiry)
    );
    let fresh = retain(
        &env,
        &[consumer(&env, &history, 3, 1, 1, size, !batch)],
        evidence,
    );
    assert_eq!(
        fresh.tx.signatures.len(),
        2,
        "no subject LP owner signature"
    );
    deliver(
        &mut env,
        &mut history,
        &fresh,
        &[fill(3, 1, 1, size, true)],
        None,
        evidence,
    );
    assert_eq!(history.replay()[1].positions[1], -size);
    assert_eq!(history.replay()[2].positions[1], -size);
    assert_eq!(history.replay()[3].positions[1], 2 * size);
    evidence.worlds += 1;
}

#[test]
fn v16_program_retained_mixed_renewals_preserve_reduced_episode_and_independent_retry() {
    let mut evidence = Evidence::default();
    for owner_reduce in [false, true] {
        for peer_first in [false, true] {
            for batch in [false, true] {
                for direction in [-1, 1] {
                    eprintln!("mixed renewal: owner_reduce={owner_reduce}, peer_first={peer_first}, batch={batch}, direction={direction}");
                    run_case(owner_reduce, peer_first, batch, direction, &mut evidence);
                }
            }
        }
    }
    assert_eq!(
        (
            evidence.worlds,
            evidence.transactions,
            evidence.simulations,
            evidence.rejections,
            evidence.rolled_back_grants,
            evidence.fills
        ),
        (16, 176, 80, 64, 16, 56)
    );
    eprintln!("INV-012 mixed episode renewal: {evidence:?}; row 412 OPEN");
}
