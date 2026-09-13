//! INV-012 / row414, with INV-002/007/019/089: replacement asset admission and
//! same-address external context recreation commit only with a current CPI response.
//! The parent rejects a stale management suffix after a successful sibling CPI;
//! reused_asset_return_binding keeps its context incarnation unchanged. Here a used
//! asset activation AND context close/refund/recreation precede return validation.
//! Both replacement orders must restore the frontier, rent, genuine old response,
//! and unchanged signed sibling permission on failure, then admit a complete retry.
//! This is bounded conformance, not a closure of the OPEN capability family.

use super::*;
use percolator_prog::matcher_abi::read_matcher_return;

const CONTEXT_SEED: &str = "row414";

fn control(h: &History, data: Vec<u8>) -> Instruction {
    let mut accounts = vec![
        AccountMeta::new_readonly(h.owners[1].pubkey(), true),
        AccountMeta::new(h.matcher.1, false),
    ];
    if data == [12] {
        accounts.push(AccountMeta::new(h.owners[1].pubkey(), false));
    }
    Instruction {
        program_id: h.matcher.0,
        accounts,
        data,
    }
}

fn create_context(h: &History) -> Instruction {
    // The LP's System seed permits real same-address recreation without another signer.
    system_instruction::create_account_with_seed(
        &h.owners[1].pubkey(),
        &h.matcher.1,
        &h.owners[1].pubkey(),
        CONTEXT_SEED,
        h.env
            .svm
            .minimum_balance_for_rent_exemption(MATCHER_CONTEXT_LEN),
        MATCHER_CONTEXT_LEN as u64,
        &h.matcher.0,
    )
}

fn bundle(
    h: &History,
    activation: &Instruction,
    route: Route,
    sizes: [i128; 3],
    context_first: bool,
    silent: bool,
) -> Vec<Instruction> {
    let mut instructions = vec![
        control(h, vec![12]),
        create_context(h),
        control(h, vec![10]),
        control(
            h,
            if silent {
                vec![11, 13, 1]
            } else {
                vec![11, 9, 0]
            },
        ),
    ];
    instructions.insert(if context_first { 4 } else { 0 }, activation.clone());
    let mut trade = h.instruction(route, sizes, context_first);
    match &mut trade {
        ProgInstruction::TradeCpi { market_id, .. } => *market_id = h.next_id,
        ProgInstruction::BatchTradeCpi { legs, .. } => {
            legs.iter_mut()
                .filter(|leg| leg.asset_index == 1)
                .for_each(|leg| leg.market_id = h.next_id);
        }
        _ => unreachable!(),
    }
    instructions.push(Instruction {
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
        data: trade.encode(),
    });
    instructions
}

fn snapshot(h: &History, tx: &Transaction) -> Vec<(Pubkey, Option<Account>)> {
    tx.message
        .account_keys
        .iter()
        .copied()
        .chain([h.env.mint, h.env.vault, h.tokens[0], h.tokens[1]])
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(|key| (key, h.env.svm.get_account(&key)))
        .collect()
}

#[test]
fn v16_program_context_recreation_and_asset_activation_roll_back_at_cpi_return() {
    let mut evidence = Evidence::default();
    let mut bundle_peak = 0;
    let mut packet_peak = 0;
    for batch in [false, true] {
        for context_first in [false, true] {
            for direction in [-1i128, 1] {
                let mut h = History::new();
                let program = Pubkey::new_unique();
                h.env.svm.add_program(
                    program,
                    &std::fs::read(hostile_matcher_program_path()).unwrap(),
                );
                let context =
                    Pubkey::create_with_seed(&h.owners[1].pubkey(), CONTEXT_SEED, &program)
                        .unwrap();
                let delegate = matcher_delegate_key(
                    &h.env.program_id,
                    &h.env.market,
                    &h.portfolios[1],
                    &h.owners[1].pubkey(),
                    &program,
                    &context,
                );
                h.matcher = (program, context, delegate);
                let setup = sign(&h, &[create_context(&h), control(&h, vec![10])]);
                h.env
                    .svm
                    .send_transaction(setup)
                    .expect("public context setup");
                h.replace(GRANT, &mut evidence);
                let grant = h.grant_sequence;
                let size = 3 * direction * POS_SCALE as i128;
                for delta in [size, -size] {
                    h.fill(Route::Single(1), [0, delta, 0], false, &mut evidence);
                }
                assert_eq!(h.env.market_state().0.matcher_req_seq, 2);
                let old_context = h.env.svm.get_account(&context).unwrap();
                let old_return = read_matcher_return(&old_context.data).unwrap();
                assert_eq!((old_return.req_id, old_return.asset_index), (2, 1));
                assert_eq!(old_return.exec_size, -size);

                let retire = sign(&h, &[lifecycle(&h, 1, false)]);
                h.env
                    .svm
                    .send_transaction(retire)
                    .expect("retire the used, flat asset");
                h.slot += 1;
                h.env.svm.warp_to_slot(h.slot);
                for sibling in [0, 2] {
                    h.env
                        .configure_auth_mark_for_asset_as_admin(sibling, h.slot, PRICE);
                }
                h.assert_state();
                let sibling_route = if batch {
                    Route::Batch
                } else {
                    Route::Single(2)
                };
                let retained = h.sign(&h.instruction(sibling_route, [0, 0, size], false));
                let retained_bytes = bincode::serialize(&retained).unwrap();
                h.simulate(&retained, 0, &mut evidence);
                let activation = lifecycle(&h, 1, true);
                let route = if batch {
                    Route::Batch
                } else {
                    Route::Single(1)
                };
                let sizes = [
                    0,
                    -size,
                    if batch {
                        5 * direction * POS_SCALE as i128
                    } else {
                        0
                    },
                ];
                let good = bundle(&h, &activation, route, sizes, context_first, false);
                let bad = bundle(&h, &activation, route, sizes, context_first, true);
                assert_eq!(good.len(), bad.len());
                assert_eq!(
                    good.iter().zip(&bad).filter(|(a, b)| a != b).count(),
                    1,
                    "only the external producer's response policy differs"
                );
                let good_tx = sign(&h, &good);
                let before = snapshot(&h, &good_tx);
                let live =
                    h.env.svm.simulate_transaction(good_tx.into()).expect(
                        "both replacements and current-generation CPI are jointly executable",
                    );
                assert_eq!(snapshot(&h, &sign(&h, &good)), before);
                bundle_peak = bundle_peak.max(live.compute_units_consumed);
                let tx = sign(&h, &bad);
                packet_peak = packet_peak.max(bincode::serialized_size(&tx).unwrap());
                let fee = FeeStructure::default().lamports_per_signature
                    * u64::from(tx.message.header.num_required_signatures);
                let mut before = snapshot(&h, &tx);
                let failure =
                    h.env.svm.send_transaction(tx).expect_err(
                        "absent current output rejects after both replacement prefixes",
                    );
                assert_eq!(
                    failure.err,
                    TransactionError::InstructionError(
                        7,
                        if batch {
                            InstructionError::Custom(PercolatorError::InvalidInstruction as u32)
                        } else {
                            InstructionError::InvalidAccountData
                        }
                    )
                );
                for (program, count) in [
                    (h.env.program_id, 1),
                    (h.matcher.0, 4),
                    (solana_sdk::system_program::ID, 1),
                ] {
                    assert_eq!(
                        failure
                            .meta
                            .logs
                            .iter()
                            .filter(|line| **line == format!("Program {program} success"))
                            .count(),
                        count
                    );
                }
                assert!(failure
                    .meta
                    .logs
                    .iter()
                    .any(|line| line == &format!("Program {} invoke [2]", h.matcher.0)));
                for (key, account) in &mut before {
                    if *key == h.env.payer.pubkey() {
                        account.as_mut().unwrap().lamports -= fee;
                    }
                    assert_eq!(
                        h.env.svm.get_account(key),
                        *account,
                        "exact rollback: {key}"
                    );
                }
                bundle_peak = bundle_peak.max(failure.meta.compute_units_consumed);
                assert_eq!(h.env.svm.get_account(&context).unwrap(), old_context);
                assert_eq!(h.env.market_state().0.matcher_req_seq, 2);
                h.assert_state();

                assert_eq!(bincode::serialize(&retained).unwrap(), retained_bytes);
                assert_eq!(
                    h.env.svm.latest_blockhash(),
                    retained.message.recent_blockhash
                );
                let landed = h.env.svm.send_transaction(retained)
                    .expect("unchanged pre-signed sibling permission survives both rolled-back replacements");
                assert!(landed
                    .logs
                    .iter()
                    .any(|line| line == &format!("Program {} success", h.matcher.0)));
                h.positions[2] = size;
                h.epoch += 1;
                h.assert_state();
                h.fill(sibling_route, [0, 0, -size], false, &mut evidence);
                assert_eq!(h.env.market_state().0.matcher_req_seq, 4);

                // Only the trade's current episodes change after the sibling roundtrip.
                // The same activation payload consumes the restored frontier exactly once.
                let retry = bundle(&h, &activation, route, sizes, context_first, false);
                assert_eq!(&retry[..5], &good[..5]);
                let tx = sign(&h, &retry);
                let lp_lamports = h
                    .env
                    .svm
                    .get_account(&h.owners[1].pubkey())
                    .unwrap()
                    .lamports;
                let rent = h.env.svm.get_account(&context).unwrap().lamports;
                let meta = h
                    .env
                    .svm
                    .send_transaction(tx)
                    .expect("fresh output commits context recreation and replacement exposure");
                bundle_peak = bundle_peak.max(meta.compute_units_consumed);
                h.ids[1] = 4;
                h.next_id = 5;
                h.positions = sizes;
                h.epoch += 1;
                h.assert_state();
                assert_eq!(
                    h.grant_sequence, grant,
                    "external recreation does not rewrite owner consent"
                );
                assert_eq!(h.env.market_state().0.matcher_req_seq, 5);
                assert_eq!(
                    h.env
                        .svm
                        .get_account(&h.owners[1].pubkey())
                        .unwrap()
                        .lamports,
                    lp_lamports
                );
                let current_context = h.env.svm.get_account(&context).unwrap();
                assert_eq!(current_context.lamports, rent);
                assert_eq!(current_context.owner, h.matcher.0);
                if batch {
                    assert_eq!(&current_context.data[..64], &[0; 64]);
                    assert_eq!(meta.return_data.program_id, h.matcher.0);
                    assert_eq!(meta.return_data.data.len(), 128);
                    let order = if context_first { [2, 1] } else { [1, 2] };
                    for (record, asset) in meta.return_data.data.chunks_exact(64).zip(order) {
                        let response = read_matcher_return(record).unwrap();
                        assert_eq!((response.req_id, response.asset_index), (5, asset as u64));
                        assert_eq!(response.exec_size, sizes[asset]);
                    }
                } else {
                    let response = read_matcher_return(&current_context.data).unwrap();
                    assert_eq!((response.req_id, response.asset_index), (5, 1));
                    assert_eq!(response.exec_size, sizes[1]);
                }
                // Standalone exits have only taker/payer signatures, including the new context.
                if batch {
                    for asset in [1, 2] {
                        let mut exit = [0; 3];
                        exit[asset] = -sizes[asset];
                        h.fill(Route::Single(asset as u16), exit, false, &mut evidence);
                    }
                } else {
                    h.fill(Route::Batch, sizes.map(|q| -q), false, &mut evidence);
                }
                assert_eq!(h.grant_sequence, grant);
                h.withdraw_all(context_first, &mut evidence);
                evidence.worlds += 1;
            }
        }
    }
    assert_eq!(evidence.worlds, 8);
    assert_cu_within(
        "INV-012 context/generation bundle",
        bundle_peak,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );
    println!("INV-012 context/generation rollback: worlds=8, rejected_bundles=8, retained_fills=8, committed_replacements=8, owner_exits=16, bundle_peak={bundle_peak}, packet_peak={packet_peak}");
}
