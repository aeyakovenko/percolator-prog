//! INV-012 / rows 412 and 414: explicit reauthorization and automatic revocation
//! do not commute. Exhaust both orders, both public writer/consumer routes, and
//! both authorized/consumed matcher tuples in one atomic transaction.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

#[derive(Clone, Copy, Debug)]
enum Event {
    Grant(usize),
    OwnerFill,
}

struct Oracle {
    matcher: usize,
    enabled: bool,
    sequence: u64,
    epoch: u64,
}

impl Oracle {
    fn after(h: &History, events: [Event; 2]) -> Self {
        let mut state = Self {
            matcher: 0,
            enabled: true,
            sequence: h.grant_sequence,
            epoch: h.epoch,
        };
        for event in events {
            match event {
                Event::Grant(matcher) => {
                    state.matcher = matcher;
                    state.enabled = true;
                    state.sequence += 1;
                }
                Event::OwnerFill => {
                    state.enabled = false;
                    state.epoch += 1;
                }
            }
        }
        state
    }

    fn authorizes(&self, matcher: usize) -> bool {
        self.enabled && self.matcher == matcher
    }
}

fn grant(h: &History, matcher: Matcher) -> Instruction {
    Instruction {
        program_id: h.env.program_id,
        accounts: vec![
            AccountMeta::new(h.owners[1].pubkey(), true),
            AccountMeta::new_readonly(h.env.market, false),
            AccountMeta::new(h.portfolios[1], false),
            AccountMeta::new_readonly(matcher.0, false),
            AccountMeta::new_readonly(matcher.1, false),
            AccountMeta::new_readonly(matcher.2, false),
        ],
        data: ProgInstruction::SetMatcherConfig {
            portfolio_id: h.env.portfolio_id(h.portfolios[1]),
            expected_sequence: h.grant_sequence,
            enabled: 1,
            trade_fee_cap_bps: FEE_CAP,
            expiry_slot: EXPIRY,
        }
        .encode(),
    }
}

fn writer(h: &History, batch: bool, size: i128) -> Instruction {
    let ix = if batch {
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

fn consumer(h: &History, route: Route, matcher: Matcher, oracle: &Oracle) -> Instruction {
    let mut ix = h.instruction(route, [0, POS_SCALE as i128, 0], false);
    match &mut ix {
        ProgInstruction::TradeCpi {
            account_a_position_epoch,
            account_b_position_epoch,
            account_b_matcher_sequence,
            ..
        }
        | ProgInstruction::BatchTradeCpi {
            account_a_position_epoch,
            account_b_position_epoch,
            account_b_matcher_sequence,
            ..
        } => {
            *account_a_position_epoch = oracle.epoch;
            *account_b_position_epoch = oracle.epoch;
            *account_b_matcher_sequence = oracle.sequence;
        }
        _ => unreachable!(),
    }
    Instruction {
        program_id: h.env.program_id,
        accounts: vec![
            AccountMeta::new(h.owners[0].pubkey(), true),
            AccountMeta::new(h.env.market, false),
            AccountMeta::new(h.portfolios[0], false),
            AccountMeta::new(h.portfolios[1], false),
            AccountMeta::new_readonly(matcher.0, false),
            AccountMeta::new(matcher.1, false),
            AccountMeta::new_readonly(matcher.2, false),
        ],
        data: ix.encode(),
    }
}

#[test]
fn v16_program_grant_writer_order_binds_atomic_cpi_authority() {
    let mut evidence = Evidence::default();
    let mut admitted = 0;
    let mut denied = 0;
    let mut max_bundle_cu = 0;
    for grant_first in [false, true] {
        for granted in 0..2 {
            for consumed in 0..2 {
                for batch_writer in [false, true] {
                    for route in [Route::Single(1), Route::Batch] {
                        let case = (grant_first, granted, consumed, batch_writer, route);
                        let mut h = History::new();
                        let matchers = [h.matcher, alternate_matcher(&mut h)];
                        for field in 0..3 {
                            let tuple = |m: Matcher| [m.0, m.1, m.2];
                            assert_ne!(tuple(matchers[0])[field], tuple(matchers[1])[field]);
                        }
                        let size = POS_SCALE as i128;
                        let original = h.sign(&h.instruction(route, [0, size, 0], false));
                        let retained_bytes = bincode::serialize(&original).unwrap();
                        h.simulate(&original, 0, &mut evidence);
                        let events = if grant_first {
                            [Event::Grant(granted), Event::OwnerFill]
                        } else {
                            [Event::OwnerFill, Event::Grant(granted)]
                        };
                        let oracle = Oracle::after(&h, events);
                        let mut instructions = vec![heap_ix(), cu_ix()];
                        instructions.extend(events.map(|event| match event {
                            Event::Grant(index) => grant(&h, matchers[index]),
                            Event::OwnerFill => writer(&h, batch_writer, size),
                        }));
                        instructions.push(consumer(&h, route, matchers[consumed], &oracle));
                        let tx = Transaction::new_signed_with_payer(
                            &instructions,
                            Some(&h.env.payer.pubkey()),
                            &[&h.env.payer, &h.owners[0], &h.owners[1]],
                            h.env.svm.latest_blockhash(),
                        );
                        tx.verify().unwrap();
                        assert!(
                            bincode::serialized_size(&tx).unwrap()
                                <= solana_sdk::packet::PACKET_DATA_SIZE as u64
                        );
                        let before = frame(&h, &matchers);
                        let mut keys = tx.message.account_keys.clone();
                        keys.extend([h.env.mint, h.env.vault, h.tokens[0], h.tokens[1]]);
                        for (program, context, delegate) in matchers {
                            keys.extend([program, context, delegate]);
                        }
                        keys.sort_unstable();
                        keys.dedup();
                        let accounts: Vec<_> = keys
                            .iter()
                            .map(|key| (*key, h.env.svm.get_account(key)))
                            .collect();
                        let fee = FeeStructure::default().lamports_per_signature
                            * u64::from(tx.message.header.num_required_signatures);
                        let result = h.env.svm.send_transaction(tx);
                        let accepted = oracle.authorizes(consumed);
                        let meta = if accepted {
                            admitted += 1;
                            result.unwrap_or_else(|err| panic!("{case:?}: {err:?}"))
                        } else {
                            denied += 1;
                            evidence.rejections += 1;
                            let failed = result.expect_err("ordered authorization oracle denies");
                            assert_eq!(
                                failed.err,
                                TransactionError::InstructionError(
                                    4,
                                    InstructionError::Custom(PercolatorError::Unauthorized as u32)
                                ),
                                "{case:?}"
                            );
                            assert_eq!(frame(&h, &matchers), before, "{case:?}");
                            evidence.rejection_cu = evidence
                                .rejection_cu
                                .max(failed.meta.compute_units_consumed);
                            failed.meta
                        };
                        max_bundle_cu = max_bundle_cu.max(meta.compute_units_consumed);
                        assert_eq!(
                            meta.logs
                                .iter()
                                .filter(|line| **line
                                    == format!("Program {} success", h.env.program_id))
                                .count(),
                            if accepted { 3 } else { 2 },
                            "both ordered writes must execute: {case:?}"
                        );
                        for (index, matcher) in matchers.iter().enumerate() {
                            assert_eq!(
                                meta.logs
                                    .iter()
                                    .filter(|line| line
                                        .starts_with(&format!("Program {} invoke", matcher.0)))
                                    .count(),
                                usize::from(accepted && index == consumed),
                                "{case:?}"
                            );
                        }
                        for (key, mut account) in accounts {
                            if key == h.env.payer.pubkey() {
                                account.as_mut().unwrap().lamports -= fee;
                            } else if accepted
                                && [
                                    h.env.market,
                                    h.portfolios[0],
                                    h.portfolios[1],
                                    matchers[consumed].1,
                                ]
                                .contains(&key)
                            {
                                continue;
                            }
                            assert_eq!(h.env.svm.get_account(&key), account, "{case:?}: {key}");
                        }
                        assert_eq!(bincode::serialize(&original).unwrap(), retained_bytes);
                        assert_eq!(
                            h.env.svm.latest_blockhash(),
                            original.message.recent_blockhash
                        );
                        if accepted {
                            h.matcher = matchers[consumed];
                            h.grant_sequence = oracle.sequence;
                            h.epoch = oracle.epoch + 1;
                            h.positions = [0, size, size];
                            h.assert_state();
                            let before = frame(&h, &matchers);
                            let mut payer = h.env.svm.get_account(&h.env.payer.pubkey()).unwrap();
                            payer.lamports -= FeeStructure::default().lamports_per_signature
                                * original.signatures.len() as u64;
                            let failed =
                                h.env.svm.send_transaction(original).expect_err(
                                    "committed epoch and grant supersede original consent",
                                );
                            assert_eq!(
                                failed.err,
                                TransactionError::InstructionError(
                                    2,
                                    InstructionError::Custom(PercolatorError::EngineStale as u32)
                                )
                            );
                            for matcher in matchers {
                                assert!(failed.meta.logs.iter().all(|line| !line
                                    .starts_with(&format!("Program {} invoke", matcher.0))));
                            }
                            evidence.rejections += 1;
                            evidence.rejection_cu = evidence
                                .rejection_cu
                                .max(failed.meta.compute_units_consumed);
                            assert_eq!(frame(&h, &matchers), before);
                            assert_eq!(h.env.svm.get_account(&h.env.payer.pubkey()), Some(payer));
                        } else {
                            h.assert_state();
                            let meta =
                                h.env.svm.send_transaction(original).expect(
                                    "unchanged retained capability survives complete rollback",
                                );
                            assert!(meta
                                .logs
                                .iter()
                                .any(|line| line == &format!("Program {} success", matchers[0].0)));
                            h.positions[1] = size;
                            h.epoch += 1;
                            h.assert_state();
                        }
                        evidence.fills += 1;
                        let exit = match route {
                            Route::Single(_) => Route::Batch,
                            Route::Batch => Route::Single(1),
                        };
                        h.fill(exit, [0, -size, 0], false, &mut evidence);
                        if accepted {
                            h.fill(Route::Single(2), [0, 0, -size], false, &mut evidence);
                        }
                        h.withdraw_all(grant_first, &mut evidence);
                        evidence.worlds += 1;
                    }
                }
            }
        }
    }
    assert_eq!((evidence.worlds, admitted, denied), (32, 8, 24));
    assert_eq!(evidence.live_simulations, 32);
    assert_eq!(evidence.rejections, 32);
    assert_eq!(evidence.fills, 72);
    assert_cu_within("ordered grant/writer bundle", max_bundle_cu, 1_400_000);
    assert_cu_within(
        "ordered grant/writer exit",
        evidence.fill_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );
    assert_cu_within(
        "ordered grant/writer custody",
        evidence.custody_cu,
        CUSTODY_CU_LIMIT,
    );
    println!("INV-012 grant/writer order: {admitted} admitted, {denied} atomic denials, bundle_cu={max_bundle_cu}, {evidence:?}");
}
