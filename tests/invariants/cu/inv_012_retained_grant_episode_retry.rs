//! Row 412: grants signed before partial reduction or direct cross-zero bind
//! admission to the committed episode, including rollback and unchanged retry.

use super::*;
use solana_sdk::fee::FeeStructure;
use std::collections::BTreeSet;

fn grant(h: &History, epoch: u64) -> Instruction {
    Instruction {
        program_id: h.env.program_id,
        accounts: vec![
            AccountMeta::new(h.owners[1].pubkey(), true),
            AccountMeta::new_readonly(h.env.market, false),
            AccountMeta::new(h.portfolios[1], false),
            AccountMeta::new_readonly(h.matcher.0, false),
            AccountMeta::new_readonly(h.matcher.1, false),
            AccountMeta::new_readonly(h.matcher.2, false),
        ],
        data: ProgInstruction::SetMatcherConfig {
            portfolio_id: 2,
            expected_sequence: h.grant_sequence,
            position_epoch: epoch,
            asset_generation_frontier: 4,
            enabled: 1,
            trade_fee_cap_bps: FEE_CAP,
            expiry_slot: EXPIRY,
        }
        .encode(),
    }
}

fn transfer(h: &History) -> Instruction {
    solana_sdk::system_instruction::transfer(&h.owners[0].pubkey(), &h.owners[1].pubkey(), 7)
}

fn sign(h: &History, instructions: &[Instruction], envelope: u32) -> Transaction {
    let mut all = vec![
        heap_ix(),
        ComputeBudgetInstruction::set_compute_unit_limit(1_000_000 + envelope),
    ];
    all.extend_from_slice(instructions);
    let mut signers = vec![&h.env.payer];
    for owner in &h.owners {
        if instructions.iter().any(|ix| {
            ix.accounts
                .iter()
                .any(|a| a.is_signer && a.pubkey == owner.pubkey())
        }) {
            signers.push(owner);
        }
    }
    let tx = Transaction::new_signed_with_payer(
        &all,
        Some(&h.env.payer.pubkey()),
        &signers,
        h.env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= solana_sdk::packet::PACKET_DATA_SIZE as u64);
    tx
}

fn frame(h: &History, tx: &Transaction) -> Vec<(Pubkey, Option<Account>)> {
    tx.message
        .account_keys
        .iter()
        .copied()
        .chain([
            h.env.market,
            h.env.mint,
            h.env.vault,
            h.env.admin.pubkey(),
            h.portfolios[0],
            h.portfolios[1],
            h.tokens[0],
            h.tokens[1],
            h.owners[0].pubkey(),
            h.owners[1].pubkey(),
            h.matcher.0,
            h.matcher.1,
            h.matcher.2,
        ])
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|key| (key, h.env.svm.get_account(&key)))
        .collect()
}

fn successes(logs: &[String], program: Pubkey) -> usize {
    logs.iter()
        .filter(|line| **line == format!("Program {program} success"))
        .count()
}

fn simulate_ok(h: &mut History, tx: &Transaction, peak: &mut u64) {
    let before = frame(h, tx);
    let meta = h
        .env
        .svm
        .simulate_transaction(tx.clone().into())
        .expect("episode-only repair or original retained consent is admissible");
    assert_eq!(frame(h, tx), before, "successful simulation is read-only");
    *peak = (*peak).max(meta.compute_units_consumed);
}

fn reject_exact(
    h: &mut History,
    tx: Transaction,
    index: u8,
    error: PercolatorError,
    peak: &mut u64,
) {
    let before = frame(h, &tx);
    let fee = FeeStructure::default().lamports_per_signature * tx.signatures.len() as u64;
    let failed = h.env.svm.send_transaction(tx).expect_err("stale consent");
    assert_eq!(
        failed.err,
        TransactionError::InstructionError(index, InstructionError::Custom(error as u32))
    );
    assert_eq!(
        successes(&failed.meta.logs, h.env.program_id),
        usize::from(index.saturating_sub(3))
    );
    assert_eq!(
        successes(&failed.meta.logs, solana_sdk::system_program::ID),
        usize::from(index > 2)
    );
    assert!(failed
        .meta
        .logs
        .iter()
        .all(|line| !line.starts_with(&format!("Program {} invoke", h.matcher.0))));
    for (key, mut expected) in before {
        if key == h.env.payer.pubkey() {
            expected.as_mut().unwrap().lamports -= fee;
        }
        assert_eq!(
            h.env.svm.get_account(&key),
            expected,
            "exact Account rollback: {key}"
        );
    }
    *peak = (*peak).max(failed.meta.compute_units_consumed);
}

#[test]
fn v16_program_retained_grant_admission_tracks_partial_and_cross_zero_commit_or_rollback() {
    let mut evidence = Evidence::default();
    let mut peak = 0;
    let mut rollbacks = 0;
    for batch in [false, true] {
        for flip in [false, true] {
            for direction in [-1i128, 1] {
                let mut h = History::new();
                let unit = direction * POS_SCALE as i128;
                let delta = if flip { -6 * unit } else { -2 * unit };
                let remaining = 4 * unit + delta;
                let route = if batch {
                    Route::Batch
                } else {
                    Route::Single(2)
                };
                h.fill(route, [0, 0, 4 * unit], false, &mut evidence);
                assert_eq!(h.epoch, 1);
                assert_eq!(h.portfolios.map(|p| h.env.portfolio_id(p)), [1, 2]);

                let old_grant = grant(&h, 1);
                let retained = sign(&h, &[old_grant.clone()], 1);
                let consumed = sign(&h, &[old_grant.clone()], 2);
                let retained_bytes = bincode::serialize(&retained).unwrap();
                simulate_ok(&mut h, &retained, &mut peak);
                let owner_writer = writer(&h, batch, delta, 1);
                let bundle = [transfer(&h), owner_writer.clone(), old_grant.clone()];
                let rejected = sign(&h, &bundle, 3);
                let mut repaired = bundle.clone();
                repaired[2] = grant(&h, 2);
                let repaired = sign(&h, &repaired, 4);
                simulate_ok(&mut h, &repaired, &mut peak);
                reject_exact(&mut h, rejected, 4, PercolatorError::EngineStale, &mut peak);
                rollbacks += 1;
                h.assert_state();

                assert_eq!(bincode::serialize(&retained).unwrap(), retained_bytes);
                h.env
                    .svm
                    .send_transaction(retained)
                    .expect("unchanged signed grant survives rolled-back writer");
                h.grant_sequence += 1;
                h.assert_state();
                reject_exact(&mut h, consumed, 2, PercolatorError::EngineStale, &mut peak);
                rollbacks += 1;

                // Retain the next grant and both consumer routes before committing
                // the exact writer that was rolled back. Only the journal advances.
                let next_grant = grant(&h, 1);
                let retained_grant = sign(&h, &[transfer(&h), next_grant.clone()], 5);
                let grant_bytes = bincode::serialize(&retained_grant).unwrap();
                simulate_ok(&mut h, &retained_grant, &mut peak);
                let consumers = [Route::Single(2), Route::Batch].map(|consumer_route| {
                    let old = h.instruction(consumer_route, [0, 0, -remaining], false);
                    let mut current = old.clone();
                    episodes(&mut current, 2);
                    let old_tx = sign(&h, &[cpi(&h, &old)], 6);
                    let current_tx = sign(&h, &[cpi(&h, &current)], 7);
                    let after_renewal = sign(&h, &[cpi(&h, &current)], 8);
                    assert_eq!(
                        old_tx.signatures.len(),
                        2,
                        "LP owner never signs consumption"
                    );
                    (old_tx, current_tx, after_renewal)
                });
                for (old, _, _) in &consumers {
                    simulate_ok(&mut h, old, &mut peak);
                }
                let matcher = h.env.svm.get_account(&h.matcher.1);
                let requests = h.env.market_state().0.matcher_req_seq;
                let tx = sign(&h, &[owner_writer], 9);
                let meta = h
                    .env
                    .svm
                    .send_transaction(tx)
                    .expect("public partial reduction or direct flip commits");
                peak = peak.max(meta.compute_units_consumed);
                h.positions[2] = remaining;
                h.epoch = 2;
                assert_revoked(&h);
                assert_eq!(h.env.market_state().0.matcher_req_seq, requests);
                assert_eq!(h.env.svm.get_account(&h.matcher.1), matcher);

                assert_eq!(bincode::serialize(&retained_grant).unwrap(), grant_bytes);
                reject_exact(
                    &mut h,
                    retained_grant,
                    3,
                    PercolatorError::EngineStale,
                    &mut peak,
                );
                rollbacks += 1;
                for (old, current, _) in &consumers {
                    reject_exact(
                        &mut h,
                        old.clone(),
                        2,
                        PercolatorError::EngineStale,
                        &mut peak,
                    );
                    reject_exact(
                        &mut h,
                        current.clone(),
                        2,
                        PercolatorError::Unauthorized,
                        &mut peak,
                    );
                    rollbacks += 2;
                }
                assert_revoked(&h);

                // Portfolio, sequence, asset frontier, matcher and economic terms
                // remain identical: only the pre-writer grant's episode is renewed.
                let fresh = grant(&h, 2);
                let mut expected = ProgInstruction::decode(&next_grant.data).unwrap();
                match &mut expected {
                    ProgInstruction::SetMatcherConfig { position_epoch, .. } => *position_epoch = 2,
                    _ => unreachable!(),
                }
                assert_eq!(fresh.accounts, next_grant.accounts);
                assert_eq!(fresh.data, expected.encode());
                let tx = sign(&h, &[fresh], 10);
                h.env
                    .svm
                    .send_transaction(tx)
                    .expect("episode-only fresh owner consent restores authority");
                h.grant_sequence += 1;
                h.assert_state();
                for (_, _, after_renewal) in consumers {
                    reject_exact(
                        &mut h,
                        after_renewal,
                        2,
                        PercolatorError::EngineStale,
                        &mut peak,
                    );
                    rollbacks += 1;
                }
                let exit_route = if batch {
                    Route::Single(2)
                } else {
                    Route::Batch
                };
                h.fill(exit_route, [0, 0, -remaining], false, &mut evidence);
                assert_eq!(h.epoch, 3);
                assert_eq!(h.env.market_state().0.matcher_req_seq, requests + 1);
                h.withdraw_all(flip, &mut evidence);
                evidence.worlds += 1;
            }
        }
    }
    assert_eq!(evidence.worlds, 8);
    assert_eq!(rollbacks, 72);
    peak = peak.max(evidence.fill_cu).max(evidence.custody_cu);
    assert_cu_within("row412 retained grant admission", peak, 1_000_000);
    println!("row412 retained grant episode: 8 worlds, 72 exact rollbacks, 8 unchanged signed grant retries, 8 episode-only renewals, 16 full owner payouts; peak_cu={peak}");
}
