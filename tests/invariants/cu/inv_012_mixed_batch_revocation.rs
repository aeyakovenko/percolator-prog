//! INV-004/010/012/024/081, row 412: clearing one leg and crossing another in
//! one owner batch revokes standing authority once, regardless of leg order.
//! A failed batch preserves consent; restoring the original vector after two
//! committed batches cannot restore it. All construction uses System/SPL/wrapper routes.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

fn owner_batch(h: &History, sizes: [i128; 3], reverse: bool) -> Instruction {
    let mut legs = (1..3)
        .map(|asset| BatchTradeLeg {
            asset_index: asset as u16,
            market_id: h.ids[asset],
            size_q: sizes[asset],
            exec_price: PRICE,
            fee_bps: 0,
        })
        .collect::<Vec<_>>();
    if reverse {
        legs.reverse();
    }
    Instruction {
        program_id: h.env.program_id,
        accounts: vec![
            AccountMeta::new(h.owners[0].pubkey(), true),
            AccountMeta::new(h.owners[1].pubkey(), true),
            AccountMeta::new(h.env.market, false),
            AccountMeta::new(h.portfolios[0], false),
            AccountMeta::new(h.portfolios[1], false),
        ],
        data: h
            .env
            .batch_trade_no_cpi_ix(h.portfolios[0], h.portfolios[1], legs)
            .encode(),
    }
}

fn cpi(h: &History, ix: &ProgInstruction) -> Instruction {
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

fn with_episode(ix: &ProgInstruction, epoch: u64) -> ProgInstruction {
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
            *account_a_position_epoch = epoch;
            *account_b_position_epoch = epoch;
        }
        _ => unreachable!(),
    }
    ix
}

fn withdraw(h: &History) -> Instruction {
    Instruction {
        program_id: h.env.program_id,
        accounts: vec![
            AccountMeta::new(h.owners[0].pubkey(), true),
            AccountMeta::new(h.env.market, false),
            AccountMeta::new(h.portfolios[0], false),
            AccountMeta::new(h.tokens[0], false),
            AccountMeta::new(h.env.vault, false),
            AccountMeta::new_readonly(h.env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: h.env.withdraw_ix(h.portfolios[0], 137).encode(),
    }
}

fn sign(h: &History, instructions: &[Instruction], owner_batch: bool) -> Transaction {
    // Distinct from History::sign so a later current-episode/old-grant request
    // reaches the program even if its protocol payload was previously rejected.
    let mut all = vec![
        heap_ix(),
        solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(1_000_000),
    ];
    all.extend_from_slice(instructions);
    let mut signers = vec![&h.env.payer, &h.owners[0]];
    if owner_batch {
        signers.push(&h.owners[1]);
    }
    Transaction::new_signed_with_payer(
        &all,
        Some(&h.env.payer.pubkey()),
        &signers,
        h.env.svm.latest_blockhash(),
    )
}

fn land(h: &mut History, tx: Transaction, failure: Option<(u8, PercolatorError)>) -> u64 {
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= solana_sdk::packet::PACKET_DATA_SIZE as u64);
    let keys = tx
        .message
        .account_keys
        .iter()
        .copied()
        .chain([
            h.env.market,
            h.portfolios[0],
            h.portfolios[1],
            h.tokens[0],
            h.tokens[1],
            h.env.vault,
            h.env.mint,
            h.matcher.0,
            h.matcher.1,
            h.matcher.2,
            h.owners[0].pubkey(),
            h.owners[1].pubkey(),
            h.env.admin.pubkey(),
        ])
        .collect::<std::collections::BTreeSet<_>>();
    let before = keys
        .into_iter()
        .map(|key| (key, h.env.svm.get_account(&key)))
        .collect::<Vec<_>>();
    let fee = FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let should_fail = failure.is_some();
    let result = h.env.svm.send_transaction(tx);
    let meta = if let Some((index, error)) = failure {
        let rejected = result.expect_err("invalid batch or retained authority must reject");
        assert_eq!(
            rejected.err,
            TransactionError::InstructionError(index, InstructionError::Custom(error as u32))
        );
        assert!(rejected
            .meta
            .logs
            .iter()
            .all(|line| !line.starts_with(&format!("Program {} invoke", h.matcher.0))));
        assert_eq!(
            rejected
                .meta
                .logs
                .iter()
                .filter(|line| **line == format!("Program {} success", h.env.program_id))
                .count(),
            usize::from(index - 2),
            "the withdrawal prefix must finish before capability rejection"
        );
        if index == 3 {
            assert!(rejected
                .meta
                .logs
                .iter()
                .any(|line| *line == format!("Program {} success", spl_token::ID)));
        }
        rejected.meta
    } else {
        result.expect("public owner batch must commit")
    };
    for (key, mut account) in before {
        if key == h.env.payer.pubkey() {
            account.as_mut().unwrap().lamports -= fee;
        } else if !should_fail && [h.env.market, h.portfolios[0], h.portfolios[1]].contains(&key) {
            continue;
        }
        assert_eq!(
            h.env.svm.get_account(&key),
            account,
            "complete Account: {key}"
        );
    }
    assert_cu_within(
        "mixed batch revocation",
        meta.compute_units_consumed,
        1_000_000,
    );
    meta.compute_units_consumed
}

fn check_revoked(h: &History) {
    let config = h.env.portfolio_matcher_config(h.portfolios[1]);
    assert_eq!(config.enabled(), 0);
    assert_eq!(config.trade_fee_cap_bps(), FEE_CAP);
    assert_eq!(config.matcher_program, h.matcher.0.to_bytes());
    assert_eq!(config.matcher_context, h.matcher.1.to_bytes());
    assert_eq!(config.matcher_delegate, h.matcher.2.to_bytes());
    assert_eq!(h.env.portfolio_matcher_expiry(h.portfolios[1]), 0);
    assert_eq!(
        h.env.portfolio_matcher_sequence(h.portfolios[1]),
        h.grant_sequence
    );
    let group = h.env.market_state().1;
    assert_eq!(group.next_market_id, 4);
    assert_eq!(group.materialized_portfolio_count, 2);
    for actor in 0..2 {
        let p = h.env.portfolio_state(h.portfolios[actor]);
        assert_eq!(h.env.portfolio_id(h.portfolios[actor]), actor as u64 + 1);
        assert_eq!(h.env.portfolio_position_epoch(h.portfolios[actor]), h.epoch);
        assert_eq!(p.capital.get(), CAPITAL);
        assert_eq!(p.pnl.get(), 0);
        assert_eq!(h.env.token_amount(h.tokens[actor]), 0);
        assert_eq!(
            percolator::active_bitmap_count_ones(active_bitmap(&p)) as usize,
            h.positions.iter().filter(|q| **q != 0).count()
        );
        for asset in 0..3 {
            let q = h.positions[asset] * if actor == 0 { 1 } else { -1 };
            assert_eq!(has_active_leg_for_asset(&p, asset), q != 0);
            if q != 0 {
                let leg = active_leg_for_asset(&p, asset);
                assert_eq!(leg.basis_pos_q, q);
                assert_eq!(leg.market_id, h.ids[asset]);
            }
            assert_eq!(group.assets[asset].market_id, h.ids[asset]);
            assert_eq!(group.assets[asset].oi_eff_long_q, q.unsigned_abs());
            assert_eq!(group.assets[asset].oi_eff_short_q, q.unsigned_abs());
        }
    }
    assert_eq!(group.c_tot, 2 * CAPITAL);
    assert_eq!(group.vault, 2 * CAPITAL);
    assert_eq!(group.insurance, 0);
    assert_eq!(h.env.token_amount(h.env.vault) as u128, 2 * CAPITAL);
    assert_eq!(
        Mint::unpack(&h.env.svm.get_account(&h.env.mint).unwrap().data)
            .unwrap()
            .supply as u128,
        2 * CAPITAL
    );
    assert_eq!(h.env.svm.get_sysvar::<Clock>().slot, SLOT);
}

#[test]
fn v16_program_clear_cross_zero_batch_roundtrip_cannot_revive_retained_capability() {
    let mut evidence = Evidence::default();
    let mut rejection_cu = 0;
    let mut failed_batch_cu = 0;
    for reverse in [false, true] {
        for route in [Route::Single(1), Route::Batch] {
            for direction in [-1i128, 1] {
                let mut h = History::new();
                let unit = direction * POS_SCALE as i128;
                let original = [0, 7 * unit, 5 * unit];
                h.fill(Route::Batch, original, reverse, &mut evidence);
                let request_count = h.env.market_state().0.matcher_req_seq;
                let matcher_state = h.env.svm.get_account(&h.matcher.1);
                let retained_ix = h.instruction(route, [0, -7 * unit, -5 * unit], reverse);
                let retained = h.sign(&retained_ix);
                let retained_bytes = bincode::serialize(&retained).unwrap();
                h.simulate(&retained, 0, &mut evidence);

                // Clearing the sibling cannot subsidize an opposite leg requiring
                // 2,000,000 IM atoms when each owner supplied only 1,000,000.
                let invalid = owner_batch(&h, [0, -200_007 * unit, -5 * unit], reverse);
                let tx = sign(&h, &[invalid], true);
                failed_batch_cu = failed_batch_cu.max(land(
                    &mut h,
                    tx,
                    Some((2, PercolatorError::EngineInvalidConfig)),
                ));
                evidence.rejections += 1;
                h.assert_state();
                h.simulate(&retained, 0, &mut evidence);

                for sizes in [[0, -10 * unit, -5 * unit], [0, 10 * unit, 5 * unit]] {
                    let ix = owner_batch(&h, sizes, reverse);
                    let tx = sign(&h, &[ix], true);
                    evidence.writer_cu = evidence.writer_cu.max(land(&mut h, tx, None));
                    for asset in 1..3 {
                        h.positions[asset] += sizes[asset];
                    }
                    h.epoch += 1;
                    check_revoked(&h);
                    assert_eq!(h.env.market_state().0.matcher_req_seq, request_count);
                    assert_eq!(h.env.svm.get_account(&h.matcher.1), matcher_state);

                    // Refreshing only the two request episodes isolates revoked
                    // standing authority from the stale-request guard.
                    let current_episode = with_episode(&retained_ix, h.epoch);
                    let tx = h.sign(&current_episode);
                    assert_eq!(tx.signatures.len(), 2, "LP owner supplies no signature");
                    rejection_cu = rejection_cu.max(land(
                        &mut h,
                        tx,
                        Some((2, PercolatorError::Unauthorized)),
                    ));
                    evidence.rejections += 1;
                    check_revoked(&h);
                }
                assert_eq!(h.positions, original);
                assert_eq!(h.epoch, 3, "one episode per committed batch, not per leg");
                assert_eq!(bincode::serialize(&retained).unwrap(), retained_bytes);
                rejection_cu = rejection_cu.max(land(
                    &mut h,
                    retained.clone(),
                    Some((2, PercolatorError::EngineStale)),
                ));
                evidence.rejections += 1;

                h.replace(GRANT, &mut evidence);
                let old_grant = sign(&h, &[cpi(&h, &with_episode(&retained_ix, h.epoch))], false);
                rejection_cu = rejection_cu.max(land(
                    &mut h,
                    old_grant,
                    Some((2, PercolatorError::EngineStale)),
                ));
                evidence.rejections += 1;
                h.fill(route, [0, -7 * unit, -5 * unit], reverse, &mut evidence);
                if matches!(route, Route::Single(_)) {
                    h.fill(Route::Single(2), [0, 0, -5 * unit], false, &mut evidence);
                }
                assert_eq!(h.positions, [0; 3]);
                // Withdrawal is public only when flat. Its SPL transfer must roll
                // back with old consent even after regrant and complete fresh exits.
                let tx = sign(&h, &[withdraw(&h), cpi(&h, &retained_ix)], false);
                rejection_cu =
                    rejection_cu.max(land(&mut h, tx, Some((3, PercolatorError::EngineStale))));
                evidence.rejections += 1;
                h.assert_state();
                h.withdraw_all(reverse, &mut evidence);
                evidence.worlds += 1;
            }
        }
    }
    assert_eq!(evidence.worlds, 8);
    assert_eq!(evidence.live_simulations, 16);
    assert_eq!(evidence.rejections, 48);
    evidence.rejection_cu = rejection_cu.max(failed_batch_cu);
    println!(
        "INV-012 mixed clear/cross-zero revocation: {evidence:?}, failed_batch_cu={failed_batch_cu}, rejection_cu={rejection_cu}"
    );
}
