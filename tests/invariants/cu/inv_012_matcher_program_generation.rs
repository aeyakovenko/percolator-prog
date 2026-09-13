//! INV-012: alternate matcher-program grants compose with asset generations.
//! Isolated program ABA and same-tuple joint replacements already have owners.
//! This increment interleaves asset reuse with A -> B -> A, retaining both grants.
//! The parent supplies public construction, lifecycle, position/OI and payout oracles.

use super::*;

#[path = "inv_012_grant_writer_order.rs"]
mod grant_writer_order;

type Matcher = (Pubkey, Pubkey, Pubkey);

fn alternate_matcher(history: &mut History) -> Matcher {
    let env = &mut history.env;
    let program = Pubkey::new_unique();
    env.svm.add_program(
        program,
        &std::fs::read(auth_matcher_program_path()).expect("read honest matcher SBF"),
    );
    let context = Keypair::new();
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &context,
        MATCHER_CONTEXT_LEN,
        program,
    );
    let delegate = matcher_delegate_key(
        &env.program_id,
        &env.market,
        &history.portfolios[1],
        &history.owners[1].pubkey(),
        &program,
        &context.pubkey(),
    );
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        Instruction {
            program_id: program,
            accounts: vec![
                AccountMeta::new_readonly(history.owners[1].pubkey(), true),
                AccountMeta::new_readonly(delegate, false),
                AccountMeta::new(context.pubkey(), false),
                AccountMeta::new_readonly(env.program_id, false),
                AccountMeta::new_readonly(env.market, false),
                AccountMeta::new_readonly(history.portfolios[1], false),
            ],
            data: vec![2],
        },
        &[&history.owners[1]],
    )
    .expect("owner initializes alternate context without replacing the wrapper grant");
    history.assert_state();
    (program, context.pubkey(), delegate)
}

fn frame(history: &History, matchers: &[Matcher; 2]) -> Vec<Option<Account>> {
    let mut accounts = history.frame();
    for (program, context, delegate) in matchers {
        accounts.extend([program, context, delegate].map(|key| history.env.svm.get_account(key)));
    }
    accounts
}

fn sign(history: &History, ix: &ProgInstruction, matcher: Matcher) -> Transaction {
    let tx = Transaction::new_signed_with_payer(
        &[
            heap_ix(),
            cu_ix(),
            Instruction {
                program_id: history.env.program_id,
                accounts: vec![
                    AccountMeta::new(history.owners[0].pubkey(), true),
                    AccountMeta::new(history.env.market, false),
                    AccountMeta::new(history.portfolios[0], false),
                    AccountMeta::new(history.portfolios[1], false),
                    AccountMeta::new_readonly(matcher.0, false),
                    AccountMeta::new(matcher.1, false),
                    AccountMeta::new_readonly(matcher.2, false),
                ],
                data: ix.encode(),
            },
        ],
        Some(&history.env.payer.pubkey()),
        &[&history.env.payer, &history.owners[0]],
        history.env.svm.latest_blockhash(),
    );
    assert_eq!(tx.signatures.len(), 2, "the LP does not sign consumption");
    assert!(bincode::serialized_size(&tx).unwrap() <= solana_sdk::packet::PACKET_DATA_SIZE as u64);
    tx
}

fn expected_error(
    history: &History,
    ix: &ProgInstruction,
    matcher: Matcher,
) -> Option<PercolatorError> {
    let (stale_asset, sequence) = match ix {
        ProgInstruction::TradeCpi {
            asset_index,
            market_id,
            account_b_matcher_sequence,
            ..
        } => (
            *market_id != history.ids[*asset_index as usize],
            *account_b_matcher_sequence,
        ),
        ProgInstruction::BatchTradeCpi {
            legs,
            account_b_matcher_sequence,
            ..
        } => (
            legs.iter()
                .any(|leg| leg.market_id != history.ids[leg.asset_index as usize]),
            *account_b_matcher_sequence,
        ),
        _ => unreachable!(),
    };
    // These expectations use the event model, never the wrapper's returned state/error.
    if stale_asset {
        Some(PercolatorError::AssetGenerationMismatch)
    } else if sequence != history.grant_sequence {
        Some(PercolatorError::EngineStale)
    } else if matcher != history.matcher {
        Some(PercolatorError::Unauthorized)
    } else {
        None
    }
}

fn assert_rejection(
    failed: &litesvm::types::FailedTransactionMetadata,
    expected: PercolatorError,
    matchers: &[Matcher; 2],
    evidence: &mut Evidence,
) {
    assert_eq!(
        failed.err,
        solana_sdk::transaction::TransactionError::InstructionError(
            2,
            solana_sdk::instruction::InstructionError::Custom(expected as u32),
        ),
    );
    for matcher in matchers {
        assert!(failed
            .meta
            .logs
            .iter()
            .all(|line| !line.starts_with(&format!("Program {} invoke", matcher.0))));
    }
    evidence.rejection_cu = evidence
        .rejection_cu
        .max(failed.meta.compute_units_consumed);
}

struct Retained {
    ix: ProgInstruction,
    tx: Transaction,
    matcher: Matcher,
}

impl Retained {
    fn new(history: &History, route: Route, sizes: [i128; 3], reverse: bool) -> Self {
        let ix = history.instruction(route, sizes, reverse);
        Self {
            tx: sign(history, &ix, history.matcher),
            ix,
            matcher: history.matcher,
        }
    }

    fn simulate(&self, history: &mut History, matchers: &[Matcher; 2], evidence: &mut Evidence) {
        let before = frame(history, matchers);
        let result = history.env.svm.simulate_transaction(self.tx.clone().into());
        match expected_error(history, &self.ix, self.matcher) {
            Some(error) => {
                let failed = result.expect_err("a displaced identity must reject");
                assert_rejection(&failed, error, matchers, evidence);
                evidence.rejected_simulations += 1;
            }
            None => {
                let meta = result.expect("all current bindings remain executable");
                assert!(meta
                    .logs
                    .iter()
                    .any(|line| line == &format!("Program {} success", self.matcher.0)));
                evidence.live_simulations += 1;
                evidence.fill_cu = evidence.fill_cu.max(meta.compute_units_consumed);
            }
        }
        assert_eq!(frame(history, matchers), before);
        history.assert_state();
    }
}

#[test]
fn v16_program_matcher_program_roundtrips_compose_with_asset_reuse() {
    let mut evidence = Evidence::default();
    // Preserve the program roundtrip order; put asset reuse in every possible gap.
    for reuse_at in 0..3 {
        for orientation in [-1, 1] {
            for reverse in [false, true] {
                let mut history = History::new();
                let before_setup = history.frame();
                let alternate = alternate_matcher(&mut history);
                assert_eq!(history.frame(), before_setup);
                let matchers = [history.matcher, alternate];
                assert_ne!(matchers[0].0, matchers[1].0);
                assert_ne!(matchers[0].1, matchers[1].1);
                assert_ne!(matchers[0].2, matchers[1].2);
                let contexts = matchers.map(|m| history.env.svm.get_account(&m.1));
                let portfolio_ids = history.portfolios.map(|p| history.env.portfolio_id(p));
                let blockhash = history.env.svm.latest_blockhash();
                let initial_sequence = history.grant_sequence;
                let sizes = [
                    0,
                    orientation * 3 * POS_SCALE as i128,
                    -orientation * 7 * POS_SCALE as i128,
                ];
                let mut retained: Vec<_> = ROUTES
                    .map(|route| Retained::new(&history, route, sizes, reverse))
                    .into_iter()
                    .collect();
                for request in &retained {
                    request.simulate(&mut history, &matchers, &mut evidence);
                }
                let mut next_program = 1;
                for step in 0..3 {
                    if step == reuse_at {
                        history.replace(1, &mut evidence);
                    } else {
                        history.matcher = matchers[next_program];
                        history.replace(GRANT, &mut evidence);
                        if next_program == 1 {
                            retained.extend(
                                ROUTES.map(|route| Retained::new(&history, route, sizes, reverse)),
                            );
                        }
                        next_program = 0;
                    }
                    assert_eq!(history.env.svm.latest_blockhash(), blockhash);
                    assert_eq!(
                        history.portfolios.map(|p| history.env.portfolio_id(p)),
                        portfolio_ids
                    );
                    assert_eq!(
                        matchers.map(|m| history.env.svm.get_account(&m.1)),
                        contexts
                    );
                    assert_eq!(history.epoch, 0, "no position episode masks scope failures");
                    for request in &retained {
                        request.simulate(&mut history, &matchers, &mut evidence);
                    }
                }
                assert_eq!(history.matcher, matchers[0]);
                assert_eq!(history.grant_sequence, initial_sequence + 2);
                assert_eq!(history.ids, [1, 4, 3]);
                assert_eq!(
                    retained.len(),
                    6,
                    "retain both independently live program grants"
                );

                for (index, route) in ROUTES.into_iter().enumerate() {
                    let original = &retained[index];
                    // Asset 2 did not change. Enumerate only distinct wire repairs.
                    let scope = route.scope() & (1 | GRANT);
                    for repaired in 0..=5 {
                        if repaired & !scope != 0 {
                            continue;
                        }
                        for matcher in matchers {
                            let ix = repair(&original.ix, &history, repaired);
                            let tx = if repaired == 0 && matcher == original.matcher {
                                original.tx.clone()
                            } else {
                                sign(&history, &ix, matcher)
                            };
                            if let Some(error) = expected_error(&history, &ix, matcher) {
                                let before = frame(&history, &matchers);
                                let failed = history.env.svm.send_transaction(tx)
                                    .expect_err("generation, grant incarnation and program scope are conjunctive");
                                assert_rejection(&failed, error, &matchers, &mut evidence);
                                assert_eq!(frame(&history, &matchers), before);
                                evidence.rejections += 1;
                                history.assert_state();
                            } else {
                                assert_eq!(
                                    ix.encode(),
                                    history.instruction(route, sizes, reverse).encode()
                                );
                                Retained { ix, tx, matcher }.simulate(
                                    &mut history,
                                    &matchers,
                                    &mut evidence,
                                );
                            }
                        }
                    }
                }

                for request in &retained[3..] {
                    let error = expected_error(&history, &request.ix, request.matcher)
                        .expect("the alternate program's retained grant is no longer current");
                    let before = frame(&history, &matchers);
                    let failed = history
                        .env
                        .svm
                        .send_transaction(request.tx.clone())
                        .expect_err("returning to A cannot consume B's original signed request");
                    assert_rejection(&failed, error, &matchers, &mut evidence);
                    assert_eq!(frame(&history, &matchers), before);
                    evidence.rejections += 1;
                    history.assert_state();
                }

                // Both actual program IDs commit real fills and opposite-transport exits.
                for program in 0..2 {
                    if program == 1 {
                        history.matcher = matchers[1];
                        history.replace(GRANT, &mut evidence);
                    }
                    let displaced = matchers[1 - program];
                    let displaced_before = [displaced.0, displaced.1, displaced.2]
                        .map(|key| history.env.svm.get_account(&key));
                    if program == 0 {
                        history.fill(Route::Batch, sizes, reverse, &mut evidence);
                    }
                    for asset in if reverse { [2, 1] } else { [1, 2] } {
                        history.fill(
                            Route::Single(asset),
                            sizes.map(|q| if program == 0 { -q } else { q }),
                            reverse,
                            &mut evidence,
                        );
                    }
                    if program == 1 {
                        history.fill(Route::Batch, sizes.map(|q| -q), reverse, &mut evidence);
                    }
                    assert_eq!(history.positions, [0; 3]);
                    assert_eq!(
                        [displaced.0, displaced.1, displaced.2]
                            .map(|key| history.env.svm.get_account(&key)),
                        displaced_before,
                    );
                }
                history.withdraw_all(reverse, &mut evidence);
                evidence.worlds += 1;
            }
        }
    }
    assert_eq!(evidence.worlds, 12);
    assert_eq!(evidence.rejections, 12 * (7 + 3 + 7 + 3));
    assert_eq!(evidence.live_simulations, 116);
    assert_eq!(evidence.rejected_simulations, 160);
    assert_eq!(evidence.fills, 72);
    assert_cu_within(
        "program/generation writers",
        evidence.writer_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_cu_within(
        "program/generation rejection",
        evidence.rejection_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_cu_within(
        "program/generation fill/exit",
        evidence.fill_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );
    assert_cu_within(
        "program/generation withdrawal",
        evidence.custody_cu,
        CUSTODY_CU_LIMIT,
    );
    println!("INV-012 matcher-program/generation product: {evidence:?}");
}
