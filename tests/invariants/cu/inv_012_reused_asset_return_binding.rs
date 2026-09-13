//! INV-012 / row 414, with INV-002/007/019/089: a used slot and its old matcher
//! response cannot substitute for the current asset generation and invocation.
//! Keep price, delegate, live grant and portfolio episodes identical across reuse.
//! All response bytes originate in committed wrapper CPI, including the old close.

use super::*;
use percolator_prog::matcher_abi::{read_matcher_return, MatcherReturn};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

fn control(h: &mut History, data: Vec<u8>) {
    send_raw_tx(
        &mut h.env.svm,
        &h.env.payer,
        Instruction {
            program_id: h.matcher.0,
            accounts: vec![
                AccountMeta::new_readonly(h.owners[1].pubkey(), true),
                AccountMeta::new(h.matcher.1, false),
            ],
            data,
        },
        &[&h.owners[1]],
    )
    .expect("owner configures the external fixture through its public control route");
}

fn context_return(h: &History) -> MatcherReturn {
    read_matcher_return(&h.env.svm.get_account(&h.matcher.1).unwrap().data).unwrap()
}

fn reject(
    h: &mut History,
    tx: Transaction,
    before_cpi: bool,
    expected: InstructionError,
    peak: &mut u64,
) {
    tx.verify().unwrap();
    let keys = tx
        .message
        .account_keys
        .iter()
        .copied()
        .chain([h.env.mint, h.env.vault, h.tokens[0], h.tokens[1]])
        .collect::<std::collections::BTreeSet<_>>();
    let before = keys
        .into_iter()
        .map(|key| (key, h.env.svm.get_account(&key)))
        .collect::<Vec<_>>();
    let fee = FeeStructure::default().lamports_per_signature
        * u64::from(tx.message.header.num_required_signatures);
    let frame = h.frame();
    let failure = h.env.svm.send_transaction(tx).expect_err(
        "old generation or absent current matcher output cannot admit replacement exposure",
    );
    assert_eq!(failure.err, TransactionError::InstructionError(2, expected));
    for log in [
        format!("Program {} invoke [2]", h.matcher.0),
        format!("Program {} success", h.matcher.0),
    ] {
        assert_eq!(
            failure
                .meta
                .logs
                .iter()
                .filter(|line| **line == log)
                .count(),
            usize::from(!before_cpi),
            "generation rejects before CPI; current generation reaches the silent matcher"
        );
    }
    for (key, mut account) in before {
        if key == h.env.payer.pubkey() {
            account.as_mut().unwrap().lamports -= fee;
        }
        assert_eq!(
            h.env.svm.get_account(&key),
            account,
            "exact rollback: {key}"
        );
    }
    assert_eq!(h.frame(), frame);
    h.assert_state();
    *peak = (*peak).max(failure.meta.compute_units_consumed);
    assert_cu_within("INV-012 reused response rejection", *peak, CUSTODY_CU_LIMIT);
}

#[test]
fn v16_program_reused_asset_requires_current_generation_and_fresh_matcher_output() {
    let mut evidence = Evidence::default();
    let mut preflight_peak = 0;
    let mut response_peak = 0;
    let mut replacements = 0;
    let mut rejections = 0;
    for asset in [1usize, 2] {
        for batch_entry in [false, true] {
            for direction in [-1i128, 1] {
                let mut h = History::new();
                let program = Pubkey::new_unique();
                h.env.svm.add_program(
                    program,
                    &std::fs::read(hostile_matcher_program_path()).unwrap(),
                );
                let context = Keypair::new();
                system_create_account_for_test(
                    &mut h.env.svm,
                    &h.env.payer,
                    &context,
                    MATCHER_CONTEXT_LEN,
                    program,
                );
                let delegate = matcher_delegate_key(
                    &h.env.program_id,
                    &h.env.market,
                    &h.portfolios[1],
                    &h.owners[1].pubkey(),
                    &program,
                    &context.pubkey(),
                );
                h.matcher = (program, context.pubkey(), delegate);
                control(&mut h, vec![10]);
                h.replace(GRANT, &mut evidence);
                let grant = h.grant_sequence;
                let portfolio_ids = h.portfolios.map(|p| h.env.portfolio_id(p));
                let route = if batch_entry {
                    Route::Batch
                } else {
                    Route::Single(asset as u16)
                };
                let mut size = [0; 3];
                size[asset] = direction * 3 * POS_SCALE as i128;
                h.fill(route, size, false, &mut evidence);
                size[asset] = -size[asset];
                h.fill(Route::Single(asset as u16), size, false, &mut evidence);
                let mut requests = 2;

                for cycle in 0..2 {
                    assert_eq!(h.positions, [0; 3]);
                    assert_eq!(h.env.market_state().0.matcher_req_seq, requests);
                    let old = context_return(&h);
                    assert_eq!(
                        old,
                        MatcherReturn {
                            abi_version: 3,
                            flags: 1,
                            exec_price_e6: PRICE,
                            exec_size: size[asset],
                            req_id: requests,
                            lp_account_id: u64::from_le_bytes(
                                delegate.to_bytes()[..8].try_into().unwrap()
                            ),
                            oracle_price_e6: PRICE,
                            asset_index: asset as u64,
                        }
                    );
                    let routes = [Route::Single(asset as u16), Route::Batch];
                    let retained = routes.map(|r| h.instruction(r, size, false));
                    let signed = retained.each_ref().map(|ix| h.sign(ix));
                    for tx in &signed {
                        tx.verify().unwrap();
                        h.simulate(tx, 0, &mut evidence);
                    }
                    let portfolios = h.portfolios.map(|p| h.env.svm.get_account(&p));
                    let old_context = h.env.svm.get_account(&h.matcher.1);
                    let old_generation = h.ids[asset];
                    h.replace(asset as u8, &mut evidence);
                    replacements += 1;
                    assert_eq!(h.ids[asset], 4 + cycle);
                    assert_ne!(h.ids[asset], old_generation);
                    assert_eq!(h.portfolios.map(|p| h.env.svm.get_account(&p)), portfolios);
                    assert_eq!(h.env.svm.get_account(&h.matcher.1), old_context);
                    assert_eq!(h.env.market_state().0.matcher_req_seq, requests);
                    assert_eq!(h.grant_sequence, grant);
                    assert_eq!(h.portfolios.map(|p| h.env.portfolio_id(p)), portfolio_ids);

                    for tx in signed {
                        reject(
                            &mut h,
                            tx,
                            true,
                            InstructionError::Custom(
                                PercolatorError::AssetGenerationMismatch as u32,
                            ),
                            &mut preflight_peak,
                        );
                        rejections += 1;
                    }
                    // Repair exactly the asset generation. No owner regrant or episode repair.
                    let current = retained
                        .each_ref()
                        .map(|ix| repair(ix, &h, 1 << (asset - 1)));
                    for ix in &current {
                        h.simulate(&h.sign(ix), 0, &mut evidence);
                    }
                    // Mode 13 with its consumed-call flag returns success without producing data.
                    // This only changes external control bytes; the old response remains genuine.
                    control(&mut h, vec![11, 13, 1]);
                    assert_eq!(context_return(&h), old);
                    for ix in &current {
                        let tx = h.sign(ix);
                        let expected = if matches!(ix, ProgInstruction::BatchTradeCpi { .. }) {
                            InstructionError::Custom(PercolatorError::InvalidInstruction as u32)
                        } else {
                            InstructionError::InvalidAccountData
                        };
                        reject(&mut h, tx, false, expected, &mut response_peak);
                        rejections += 1;
                        assert_eq!(context_return(&h), old);
                        assert_eq!(h.env.market_state().0.matcher_req_seq, requests);
                    }
                    control(&mut h, vec![11, 9, 0]);
                    h.env.svm.expire_blockhash();
                    assert_eq!(
                        h.instruction(route, size, false).encode(),
                        current[usize::from(batch_entry)].encode(),
                        "successful retry changes only the retained request's asset generation"
                    );
                    h.fill(route, size, false, &mut evidence);
                    requests += 1;
                    assert_eq!(h.env.market_state().0.matcher_req_seq, requests);
                    if batch_entry {
                        assert_eq!(context_return(&h), old, "batch consumes fresh return data while retaining the old single record");
                    } else {
                        let fresh = context_return(&h);
                        let mut expected = old;
                        expected.req_id = requests;
                        assert_eq!(fresh, expected, "only invocation identity distinguishes the otherwise identical response");
                    }
                    size[asset] = -size[asset];
                    h.fill(Route::Single(asset as u16), size, false, &mut evidence);
                    requests += 1;
                    assert_eq!(h.env.market_state().0.matcher_req_seq, requests);
                    assert_eq!(h.grant_sequence, grant);
                }
                assert_eq!(requests, 6);
                h.withdraw_all(batch_entry, &mut evidence);
                assert_eq!(h.env.market_state().0.matcher_req_seq, requests);
                evidence.worlds += 1;
            }
        }
    }
    assert_eq!((evidence.worlds, replacements, rejections), (8, 16, 64));
    assert_cu_within(
        "INV-012 reused response entry/exit",
        evidence.fill_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );
    assert_cu_within(
        "INV-012 reused response lifecycle",
        evidence.writer_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_cu_within(
        "INV-012 reused response withdrawal",
        evidence.custody_cu,
        CUSTODY_CU_LIMIT,
    );
    println!(
        "INV-012 row414: worlds={}, previews={}, replacements={replacements}, rejections={rejections}, fills={}, preflight_peak={preflight_peak}, response_peak={response_peak}, fill_peak={}, writer_peak={}, custody_peak={}",
        evidence.worlds, evidence.live_simulations, evidence.fills,
        evidence.fill_cu, evidence.writer_cu, evidence.custody_cu
    );
}
