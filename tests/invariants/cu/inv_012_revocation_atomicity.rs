//! INV-012 / row 412: retained authority follows committed position history.
//! A successful CPI fill and revoking bilateral fill both roll back when a
//! current-episode consumer rejects later in the transaction. The original
//! retained request stays live; a committed position round trip revokes it.

use super::*;
use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

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

fn episodes(ix: &mut ProgInstruction, epoch: u64) {
    match ix {
        ProgInstruction::TradeCpi {
            account_a_position_epoch,
            account_b_position_epoch,
            ..
        }
        | ProgInstruction::BatchTradeCpi {
            account_a_position_epoch,
            account_b_position_epoch,
            ..
        }
        | ProgInstruction::TradeNoCpi {
            account_a_position_epoch,
            account_b_position_epoch,
            ..
        }
        | ProgInstruction::BatchTradeNoCpi {
            account_a_position_epoch,
            account_b_position_epoch,
            ..
        } => {
            *account_a_position_epoch = epoch;
            *account_b_position_epoch = epoch;
        }
        _ => unreachable!(),
    }
}

fn writer(h: &History, batch: bool, size: i128, epoch: u64) -> Instruction {
    let mut ix = if batch {
        h.env.batch_trade_no_cpi_ix(
            h.portfolios[0],
            h.portfolios[1],
            vec![BatchTradeLeg {
                asset_index: 2,
                market_id: h.ids[2],
                size_q: size,
                exec_price: PRICE,
                fee_bps: 0,
            }],
        )
    } else {
        h.env
            .trade_no_cpi_ix(h.portfolios[0], h.portfolios[1], 2, size, PRICE, 0)
    };
    episodes(&mut ix, epoch);
    Instruction {
        program_id: h.env.program_id,
        accounts: vec![
            AccountMeta::new(h.owners[0].pubkey(), true),
            AccountMeta::new(h.owners[1].pubkey(), true),
            AccountMeta::new(h.env.market, false),
            AccountMeta::new(h.portfolios[0], false),
            AccountMeta::new(h.portfolios[1], false),
        ],
        data: ix.encode(),
    }
}

fn signed_writer(h: &History, instructions: Vec<Instruction>) -> Transaction {
    let mut all = vec![heap_ix(), cu_ix()];
    all.extend(instructions);
    let tx = Transaction::new_signed_with_payer(
        &all,
        Some(&h.env.payer.pubkey()),
        &[&h.env.payer, &h.owners[0], &h.owners[1]],
        h.env.svm.latest_blockhash(),
    );
    tx.verify().unwrap();
    assert_eq!(tx.signatures.len(), 3);
    assert!(bincode::serialized_size(&tx).unwrap() <= solana_sdk::packet::PACKET_DATA_SIZE as u64);
    tx
}

fn assert_revoked(h: &History) {
    let config = h.env.portfolio_matcher_config(h.portfolios[1]);
    assert_eq!(config.enabled(), 0);
    assert_eq!(config.trade_fee_cap_bps(), FEE_CAP);
    assert_eq!(config.matcher_program, h.matcher.0.to_bytes());
    assert_eq!(config.matcher_context, h.matcher.1.to_bytes());
    assert_eq!(config.matcher_delegate, h.matcher.2.to_bytes());
    assert_eq!(h.env.portfolio_matcher_expiry(h.portfolios[1]), 0);
    assert_eq!(
        h.env.portfolio_matcher_sequence(h.portfolios[1]),
        h.grant_sequence,
        "automatic revocation does not stand in for a new owner grant"
    );
    let group = h.env.market_state().1;
    for (actor, portfolio) in h.portfolios.iter().enumerate() {
        let state = h.env.portfolio_state(*portfolio);
        assert_eq!(h.env.portfolio_position_epoch(*portfolio), h.epoch);
        assert_eq!(state.capital.get(), CAPITAL);
        assert_eq!(state.pnl.get(), 0);
        for asset in 0..3 {
            assert_eq!(
                has_active_leg_for_asset(&state, asset),
                h.positions[asset] != 0
            );
            if h.positions[asset] != 0 {
                let leg = active_leg_for_asset(&state, asset);
                assert_eq!(leg.market_id, h.ids[asset]);
                assert_eq!(
                    leg.basis_pos_q,
                    h.positions[asset] * if actor == 0 { 1 } else { -1 }
                );
            }
            assert_eq!(group.assets[asset].market_id, h.ids[asset]);
            assert_eq!(
                group.assets[asset].oi_eff_long_q,
                h.positions[asset].unsigned_abs()
            );
            assert_eq!(
                group.assets[asset].oi_eff_short_q,
                h.positions[asset].unsigned_abs()
            );
        }
        assert_eq!(h.env.token_amount(h.tokens[actor]), 0);
    }
    assert_eq!(group.c_tot, 2 * CAPITAL);
    assert_eq!(group.insurance, 0);
    assert_eq!(group.vault, 2 * CAPITAL);
    assert_eq!(h.env.token_amount(h.env.vault) as u128, 2 * CAPITAL);
    assert_eq!(
        Mint::unpack(&h.env.svm.get_account(&h.env.mint).unwrap().data)
            .unwrap()
            .supply as u128,
        2 * CAPITAL
    );
}

fn reject(h: &mut History, tx: Transaction, error: PercolatorError, evidence: &mut Evidence) {
    tx.verify().unwrap();
    assert_eq!(tx.signatures.len(), 2, "no LP owner on retained consumers");
    let before = h.frame();
    let failed = h
        .env
        .svm
        .send_transaction(tx)
        .expect_err("retained authority rejects");
    assert_eq!(
        failed.err,
        TransactionError::InstructionError(2, InstructionError::Custom(error as u32))
    );
    assert!(failed
        .meta
        .logs
        .iter()
        .all(|line| !line.starts_with(&format!("Program {} invoke", h.matcher.0))));
    assert_eq!(h.frame(), before, "exact economic Account rollback");
    evidence.rejections += 1;
    evidence.rejection_cu = evidence
        .rejection_cu
        .max(failed.meta.compute_units_consumed);
}

#[test]
fn v16_program_retained_capability_tracks_committed_revocation_after_bundle_rollback() {
    let mut evidence = Evidence::default();
    let mut bundle_cu = 0;
    for batch_writer in [false, true] {
        for route in [Route::Single(1), Route::Batch] {
            for sign in [-1i128, 1] {
                let mut h = History::new();
                let size = sign * POS_SCALE as i128;
                let open_sizes = [0, size, 0];
                let close_sizes = [0, -size, 0];
                let ids = h.portfolios.map(|p| h.env.portfolio_id(p));
                let original_grant = h.env.portfolio_matcher_config(h.portfolios[1]);
                let original = h.instruction(route, open_sizes, false);
                let retained = h.sign(&original);
                retained.verify().unwrap();
                let retained_bytes = bincode::serialize(&retained).unwrap();
                let blockhash = retained.message.recent_blockhash;
                h.simulate(&retained, 0, &mut evidence);

                // Every instruction binds its own predicted prefix epoch. The suffix
                // can fail only after the CPI fill and automatic revocation succeeded.
                let mut suffix = h.instruction(route, close_sizes, false);
                episodes(&mut suffix, 2);
                let bundle = signed_writer(
                    &h,
                    vec![
                        cpi(&h, &original),
                        writer(&h, batch_writer, size, 1),
                        cpi(&h, &suffix),
                    ],
                );
                let before = h.frame();
                let failed = h
                    .env
                    .svm
                    .send_transaction(bundle)
                    .expect_err("revoked current-episode suffix");
                assert_eq!(
                    failed.err,
                    TransactionError::InstructionError(
                        4,
                        InstructionError::Custom(PercolatorError::Unauthorized as u32)
                    )
                );
                assert_eq!(
                    failed
                        .meta
                        .logs
                        .iter()
                        .filter(|line| *line == &format!("Program {} success", h.env.program_id))
                        .count(),
                    2
                );
                assert_eq!(
                    failed
                        .meta
                        .logs
                        .iter()
                        .filter(|line| line.starts_with(&format!("Program {} invoke", h.matcher.0)))
                        .count(),
                    1
                );
                assert!(failed
                    .meta
                    .logs
                    .iter()
                    .any(|line| line == &format!("Program {} success", h.matcher.0)));
                assert_eq!(
                    h.frame(),
                    before,
                    "rollback restores grant, episodes, matcher writes, SPL and owner lamports"
                );
                h.assert_state();
                assert_eq!(
                    h.env.portfolio_matcher_config(h.portfolios[1]),
                    original_grant
                );
                bundle_cu = bundle_cu.max(failed.meta.compute_units_consumed);
                evidence.rejections += 1;

                assert_eq!(bincode::serialize(&retained).unwrap(), retained_bytes);
                assert_eq!(h.env.svm.latest_blockhash(), blockhash);
                let filled = h
                    .env
                    .svm
                    .send_transaction(retained)
                    .expect("unchanged retained request survives rolled-back revocation");
                assert!(filled
                    .logs
                    .iter()
                    .any(|line| line == &format!("Program {} success", h.matcher.0)));
                h.positions[1] = size;
                h.epoch += 1;
                evidence.fills += 1;
                evidence.fill_cu = evidence.fill_cu.max(filled.compute_units_consumed);
                h.assert_state();
                h.fill(route, close_sizes, false, &mut evidence);

                let before_roundtrip = h.instruction(route, open_sizes, false);
                let retained = h.sign(&before_roundtrip);
                h.simulate(&retained, 0, &mut evidence);
                for delta in [size, -size] {
                    let tx = signed_writer(&h, vec![writer(&h, batch_writer, delta, h.epoch)]);
                    let frame = h.frame();
                    let meta = h
                        .env
                        .svm
                        .send_transaction(tx)
                        .expect("committed bilateral position writer");
                    evidence.writer_cu = evidence.writer_cu.max(meta.compute_units_consumed);
                    h.positions[2] += delta;
                    h.epoch += 1;
                    assert_revoked(&h);
                    // Only market and the two portfolios may change on a zero-fee bilateral fill.
                    assert_eq!(&h.frame()[3..], &frame[3..]);
                }
                assert_eq!(h.positions, [0; 3]);
                assert_eq!(h.portfolios.map(|p| h.env.portfolio_id(p)), ids);
                assert_eq!(h.env.svm.get_sysvar::<Clock>().slot, SLOT);
                assert!(SLOT < EXPIRY);
                assert_eq!(h.env.svm.latest_blockhash(), blockhash);
                reject(
                    &mut h,
                    retained,
                    PercolatorError::EngineStale,
                    &mut evidence,
                );

                // Repair only the two episodes, retaining the original grant and bounds.
                let mut repaired = before_roundtrip;
                episodes(&mut repaired, h.epoch);
                assert_eq!(
                    repaired.encode(),
                    h.instruction(route, open_sizes, false).encode()
                );
                let current = h.sign(&repaired);
                reject(
                    &mut h,
                    current,
                    PercolatorError::Unauthorized,
                    &mut evidence,
                );
                assert_revoked(&h);

                h.env
                    .try_set_matcher_config_with_trade_fee_cap_and_expiry(
                        h.matcher.0,
                        &h.owners[1],
                        h.portfolios[1],
                        h.matcher.1,
                        h.matcher.2,
                        1,
                        FEE_CAP,
                        EXPIRY,
                    )
                    .expect("only an explicit owner grant restores trading authority");
                h.grant_sequence += 1;
                h.assert_state();
                // Distinct transport avoids replay-cache rejection without repairing consent.
                h.env.svm.expire_blockhash();
                let stale_grant = h.sign(&repaired);
                reject(
                    &mut h,
                    stale_grant,
                    PercolatorError::EngineStale,
                    &mut evidence,
                );
                h.fill(route, open_sizes, false, &mut evidence);
                h.fill(route, close_sizes, false, &mut evidence);
                h.withdraw_all(sign < 0, &mut evidence);
                evidence.worlds += 1;
            }
        }
    }
    assert_eq!(evidence.worlds, 8);
    assert_eq!(evidence.live_simulations, 16);
    assert_eq!(evidence.rejections, 32);
    assert_eq!(evidence.fills, 32);
    assert_cu_within(
        "retained revocation bundle",
        bundle_cu,
        crate::support::v16_svm::TX_CU_LIMIT,
    );
    assert_cu_within(
        "retained capability preflight",
        evidence.rejection_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_cu_within(
        "committed revocation writer",
        evidence.writer_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );
    assert_cu_within(
        "retained/fresh capability fill",
        evidence.fill_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );
    assert_cu_within(
        "owner entitlement exit",
        evidence.custody_cu,
        CUSTODY_CU_LIMIT,
    );
    println!("INV-012 committed revocation / atomic rollback: bundle_cu={bundle_cu}, {evidence:?}");
}
