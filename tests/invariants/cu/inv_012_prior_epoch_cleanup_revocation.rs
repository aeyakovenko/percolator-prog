//! INV-004/005/010/012/024/081, row 412: permissionless prior-epoch detachment
//! revokes retained authority on a live sibling asset. Peer owner reduction first
//! resets the LP's side without writing the LP; its later keeper cleanup must
//! consume exactly one portfolio episode and disable the unobserving matcher.
//! Unlike owner forfeit, liquidation and owner-batch round trips, the revoking
//! writer here is automatic cleanup of a prior-epoch leg. INV-028 reset-exit
//! tests own claim materialization, not retained sibling consent or grant rollback.
//! All economic state comes from the parent's System/SPL/wrapper construction.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

fn cleanup(h: &History) -> Instruction {
    Instruction {
        program_id: h.env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(h.env.admin.pubkey(), false),
            AccountMeta::new(h.env.market, false),
            AccountMeta::new(h.portfolios[1], false),
        ],
        data: ProgInstruction::PermissionlessCrank {
            now_slot: SLOT,
            observations: vec![],
        }
        .encode(),
    }
}

fn consumer(h: &History, ix: &ProgInstruction) -> Instruction {
    Instruction {
        program_id: h.env.program_id,
        accounts: vec![
            AccountMeta::new(h.owners[0].pubkey(), true),
            AccountMeta::new(h.env.market, false),
            AccountMeta::new(h.portfolios[0], false),
            AccountMeta::new(h.portfolios[1], false),
            AccountMeta::new_readonly(h.matcher.0, false),
            AccountMeta::new(h.matcher.1, false),
            AccountMeta::new_readonly(h.matcher.2, false),
        ],
        data: ix.encode(),
    }
}

fn current_episodes(ix: &ProgInstruction, taker: u64, lp: u64) -> ProgInstruction {
    let mut ix = ix.clone();
    match &mut ix {
        ProgInstruction::TradeCpi {
            account_a_position_epoch,
            account_b_position_epoch,
            ..
        }
        | ProgInstruction::BatchTradeCpi {
            account_a_position_epoch,
            account_b_position_epoch,
            ..
        } => {
            *account_a_position_epoch = taker;
            *account_b_position_epoch = lp;
        }
        _ => unreachable!(),
    }
    ix
}

fn sign(h: &History, instructions: &[Instruction], with_consumer: bool) -> Transaction {
    let mut all = vec![heap_ix(), cu_ix()];
    all.extend_from_slice(instructions);
    let mut signers = vec![&h.env.payer];
    if with_consumer {
        signers.push(&h.owners[0]);
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

fn reject(h: &mut History, tx: Transaction, index: u8, error: PercolatorError) -> u64 {
    let before = h.frame();
    let mut payer = h.env.svm.get_account(&h.env.payer.pubkey()).unwrap();
    payer.lamports -= FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let failed = h.env.svm.send_transaction(tx).expect_err("revoked consent");
    assert_eq!(
        failed.err,
        TransactionError::InstructionError(index, InstructionError::Custom(error as u32))
    );
    assert_eq!(
        failed
            .meta
            .logs
            .iter()
            .filter(|line| **line == format!("Program {} success", h.env.program_id))
            .count(),
        usize::from(index - 2),
        "cleanup prefix completes before consumer rejection"
    );
    assert!(failed
        .meta
        .logs
        .iter()
        .all(|line| !line.starts_with(&format!("Program {} invoke", h.matcher.0))));
    assert_eq!(h.frame(), before, "complete economic Account rollback");
    assert_eq!(h.env.svm.get_account(&h.env.payer.pubkey()), Some(payer));
    assert_cu_within(
        "prior-epoch capability rollback",
        failed.meta.compute_units_consumed,
        500_000,
    );
    failed.meta.compute_units_consumed
}

#[test]
fn v16_program_prior_epoch_cleanup_revokes_retained_sibling_capability() {
    let mut evidence = Evidence::default();
    let mut cleanup_cu = 0;
    for route in [Route::Single(2), Route::Batch] {
        for direction in [-1i128, 1] {
            let mut h = History::new();
            let unit = direction * POS_SCALE as i128;
            h.fill(Route::Batch, [0, 6 * unit, 2 * unit], false, &mut evidence);
            let retained_ix = h.instruction(route, [0, 0, -2 * unit], false);
            let retained = h.sign(&retained_ix);
            let retained_bytes = bincode::serialize(&retained).unwrap();
            h.simulate(&retained, 0, &mut evidence);
            let blockhash = h.env.svm.latest_blockhash();
            let lp_before = h.env.svm.get_account(&h.portfolios[1]);
            let matcher_before = h.env.svm.get_account(&h.matcher.1);
            let request_sequence = h.env.market_state().0.matcher_req_seq;
            let grant = h.env.portfolio_matcher_config(h.portfolios[1]);
            let sibling = active_leg_for_asset(&h.env.portfolio_state(h.portfolios[1]), 2);

            // Only the peer signs. Global effective OI reaches zero, while the
            // LP still holds its prior-epoch basis leg and the original grant.
            let cu =
                h.env
                    .rebalance_reduce_with_cu(&h.owners[0], h.portfolios[0], 1, 6 * POS_SCALE);
            evidence.writer_cu = evidence.writer_cu.max(cu);
            assert_eq!(h.env.svm.get_account(&h.portfolios[1]), lp_before);
            assert_eq!(h.env.portfolio_position_epoch(h.portfolios[0]), 2);
            assert!(!has_active_leg_for_asset(
                &h.env.portfolio_state(h.portfolios[0]),
                1
            ));
            let reset = h.env.market_state().1.assets[1];
            assert_eq!((reset.oi_eff_long_q, reset.oi_eff_short_q), (0, 0));
            let lp_leg = active_leg_for_asset(&h.env.portfolio_state(h.portfolios[1]), 1);
            let (mode, epoch) = if direction > 0 {
                (reset.mode_short, reset.epoch_short)
            } else {
                (reset.mode_long, reset.epoch_long)
            };
            assert_eq!(mode, SideModeV16::ResetPending);
            assert!(lp_leg.epoch_snap < epoch);

            // Current request episodes cannot stand in for a fresh owner grant.
            // A late rejection must also undo the automatic detachment itself.
            let refreshed = current_episodes(&retained_ix, 2, 2);
            let bundle = sign(&h, &[cleanup(&h), consumer(&h, &refreshed)], true);
            evidence.rejection_cu =
                evidence
                    .rejection_cu
                    .max(reject(&mut h, bundle, 3, PercolatorError::Unauthorized));
            evidence.rejections += 1;
            assert_eq!(h.env.svm.get_account(&h.portfolios[1]), lp_before);

            let peer_before = h.env.svm.get_account(&h.portfolios[0]);
            let tx = sign(&h, &[cleanup(&h)], false);
            assert_eq!(
                tx.signatures.len(),
                1,
                "cleanup needs neither portfolio owner"
            );
            let meta = h
                .env
                .svm
                .send_transaction(tx)
                .expect("permissionless prior-epoch cleanup");
            cleanup_cu = cleanup_cu.max(meta.compute_units_consumed);
            assert_cu_within(
                "prior-epoch cleanup",
                meta.compute_units_consumed,
                CRANK_CU_LIMIT,
            );
            assert_eq!(h.env.svm.get_account(&h.portfolios[0]), peer_before);
            assert_eq!(h.env.svm.get_account(&h.matcher.1), matcher_before);
            assert_eq!(h.env.market_state().0.matcher_req_seq, request_sequence);
            assert_eq!(h.env.portfolio_position_epoch(h.portfolios[1]), 2);
            let lp = h.env.portfolio_state(h.portfolios[1]);
            assert!(!has_active_leg_for_asset(&lp, 1));
            assert_eq!(active_leg_for_asset(&lp, 2), sibling);
            assert_eq!(percolator::active_bitmap_count_ones(active_bitmap(&lp)), 1);
            let revoked = h.env.portfolio_matcher_config(h.portfolios[1]);
            assert_eq!(revoked.enabled(), 0);
            assert_eq!(revoked.trade_fee_cap_bps(), FEE_CAP);
            assert_eq!(revoked.matcher_program, grant.matcher_program);
            assert_eq!(revoked.matcher_context, grant.matcher_context);
            assert_eq!(revoked.matcher_delegate, grant.matcher_delegate);
            assert_eq!(
                h.env.portfolio_matcher_sequence(h.portfolios[1]),
                h.grant_sequence
            );
            assert_eq!(h.env.portfolio_matcher_expiry(h.portfolios[1]), 0);
            for actor in 0..2 {
                let p = h.env.portfolio_state(h.portfolios[actor]);
                assert_eq!(p.capital.get(), CAPITAL);
                assert_eq!(p.pnl.get(), 0);
                assert_eq!(h.env.portfolio_id(h.portfolios[actor]), actor as u64 + 1);
                assert_eq!(h.env.token_amount(h.tokens[actor]), 0);
            }
            let group = h.env.market_state().1;
            assert_eq!(
                (group.c_tot, group.vault, group.insurance),
                (2 * CAPITAL, 2 * CAPITAL, 0)
            );
            assert_eq!(h.env.token_amount(h.env.vault) as u128, 2 * CAPITAL);
            assert_eq!(
                (
                    group.assets[2].oi_eff_long_q,
                    group.assets[2].oi_eff_short_q
                ),
                (2 * POS_SCALE, 2 * POS_SCALE)
            );
            assert_eq!(h.env.svm.latest_blockhash(), blockhash);
            assert_eq!(h.env.svm.get_sysvar::<Clock>().slot, SLOT);
            assert_eq!(bincode::serialize(&retained).unwrap(), retained_bytes);

            // The middle request repairs only the peer's epoch, isolating the
            // LP cleanup's episode guard from the earlier owner reduction.
            let lp_stale = h.sign(&current_episodes(&retained_ix, 2, 1));
            let current = h.sign(&refreshed);
            for (tx, error) in [
                (retained, PercolatorError::EngineStale),
                (lp_stale, PercolatorError::EngineStale),
                (current, PercolatorError::Unauthorized),
            ] {
                evidence.rejection_cu = evidence.rejection_cu.max(reject(&mut h, tx, 2, error));
                evidence.rejections += 1;
            }

            h.positions[1] = 0;
            h.epoch = 2;
            h.replace(GRANT, &mut evidence);
            h.fill(route, [0, 0, -2 * unit], false, &mut evidence);
            h.env
                .finalize_reset_side_with_cu(1, u8::from(direction > 0));
            h.withdraw_all(direction > 0, &mut evidence);
            evidence.worlds += 1;
        }
    }
    assert_eq!(evidence.worlds, 4);
    assert_eq!(evidence.live_simulations, 4);
    assert_eq!(evidence.rejections, 16);
    assert_eq!(evidence.fills, 8);
    println!("INV-012 prior-epoch cleanup: {evidence:?}, cleanup_cu={cleanup_cu}");
}
