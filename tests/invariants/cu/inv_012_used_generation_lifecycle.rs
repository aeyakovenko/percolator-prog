//! INV-012 / row 414: authorized reuse after real CPI exposure, with a live sibling.
//! The parent covers empty-slot identity substitutions. This positive lifecycle
//! keeps the same portfolios, matcher tuple and old single-return record through
//! two used-slot replacements, then requires fresh-grant entry and complete exit.
//! Explicit reauthorization is intentional: standing-grant generation confinement
//! remains open. No retained trade/config replay or owner-revocation claim is made.

use super::*;
use percolator_prog::matcher_abi::{read_matcher_return, MatcherReturn};

#[path = "inv_012_used_scope_succession.rs"]
mod used_scope_succession;

fn commit(
    history: &mut History,
    route: Route,
    sizes: [i128; 3],
    reverse: bool,
    requests: &mut u64,
    evidence: &mut Evidence,
) {
    assert_eq!(history.env.market_state().0.matcher_req_seq, *requests);
    let context_before = history.env.svm.get_account(&history.matcher.1).unwrap();
    history.fill(route, sizes, reverse, evidence);
    *requests += 1;
    assert_eq!(history.env.market_state().0.matcher_req_seq, *requests);
    let context = history.env.svm.get_account(&history.matcher.1).unwrap();
    match route {
        Route::Single(asset) => {
            assert_eq!(
                read_matcher_return(&context.data).unwrap(),
                MatcherReturn {
                    abi_version: 3,
                    flags: 1,
                    exec_price_e6: PRICE,
                    exec_size: sizes[asset as usize],
                    req_id: *requests,
                    lp_account_id: u64::from_le_bytes(
                        history.matcher.2.to_bytes()[..8].try_into().unwrap()
                    ),
                    oracle_price_e6: PRICE,
                    asset_index: asset as u64,
                },
                "a used slot cannot reset invocation identity or quote a prior fill"
            );
            let mut expected = context_before;
            expected.data[..64].copy_from_slice(&context.data[..64]);
            assert_eq!(
                context, expected,
                "only the single-return record may change"
            );
        }
        Route::Batch => {
            // The honest batch fixture uses return data, leaving its single record intact.
            assert_eq!(context, context_before);
        }
    }
}

#[test]
fn v16_program_used_asset_reuse_with_live_sibling_preserves_authorized_exit() {
    let mut evidence = Evidence::default();
    let mut replacements = 0;
    for target in [1usize, 2] {
        let sibling = 3 - target;
        for batch_entry in [false, true] {
            for sign in [-1, 1] {
                for reverse in [false, true] {
                    let mut history = History::new();
                    let portfolio_ids = history.portfolios.map(|p| history.env.portfolio_id(p));
                    let matcher = history.matcher;
                    let grant = history.grant_sequence;
                    let mut requests = 0;
                    let mut sizes = [0; 3];
                    sizes[target] = sign * 3 * POS_SCALE as i128;
                    sizes[sibling] = -sign * 7 * POS_SCALE as i128;
                    if batch_entry {
                        commit(
                            &mut history,
                            Route::Batch,
                            sizes,
                            reverse,
                            &mut requests,
                            &mut evidence,
                        );
                    } else {
                        for asset in if reverse { [2, 1] } else { [1, 2] } {
                            commit(
                                &mut history,
                                Route::Single(asset),
                                sizes,
                                reverse,
                                &mut requests,
                                &mut evidence,
                            );
                        }
                    }

                    for cycle in 0..2 {
                        let mut close = [0; 3];
                        close[target] = -sizes[target];
                        commit(
                            &mut history,
                            if batch_entry {
                                Route::Single(target as u16)
                            } else {
                                Route::Batch
                            },
                            close,
                            reverse,
                            &mut requests,
                            &mut evidence,
                        );
                        assert_eq!(history.positions[target], 0);
                        assert_ne!(history.positions[sibling], 0);
                        let portfolios =
                            history.portfolios.map(|p| history.env.svm.get_account(&p));
                        let context = history.env.svm.get_account(&matcher.1).unwrap();
                        let old_return = read_matcher_return(&context.data).unwrap();
                        assert!(old_return.req_id > 0 && old_return.req_id <= requests);
                        let old_generation = history.ids[target];

                        history.replace_without_mark_refresh(target as u8, &mut evidence);
                        replacements += 1;
                        assert_eq!(history.ids[target], 4 + cycle);
                        assert!(history.ids[target] > old_generation);
                        assert_eq!(history.ids[sibling], sibling as u64 + 1);
                        assert_eq!(history.env.market_state().0.matcher_req_seq, requests);
                        assert_eq!(
                            history.portfolios.map(|p| history.env.svm.get_account(&p)),
                            portfolios,
                            "reuse cannot rewrite either live portfolio"
                        );

                        // Lifecycle changes stale the live certificates. Refresh the unchanged
                        // feeds and both portfolios before configuring the replacement oracle.
                        for asset in [0, sibling] {
                            let cu = history.env.push_auth_mark_for_asset_as_admin(
                                asset as u16,
                                history.slot,
                                PRICE,
                            );
                            evidence.writer_cu = evidence.writer_cu.max(cu);
                        }
                        for portfolio in history.portfolios {
                            let cu = history.env.crank(
                                portfolio,
                                ProgInstruction::PermissionlessCrank {
                                    now_slot: history.slot,
                                    observations: crank_observations(sibling as u16),
                                },
                            );
                            evidence.writer_cu = evidence.writer_cu.max(cu);
                            history.assert_state();
                        }
                        let cu = history.env.configure_auth_mark_for_asset_as_admin(
                            target as u16,
                            history.slot,
                            PRICE,
                        );
                        evidence.writer_cu = evidence.writer_cu.max(cu);

                        history.replace(GRANT, &mut evidence);
                        assert_eq!(history.grant_sequence, grant + cycle + 1);
                        assert_eq!(history.env.market_state().0.matcher_req_seq, requests);
                        assert_eq!(history.env.svm.get_account(&matcher.1).unwrap(), context);
                        assert_eq!(history.matcher, matcher);
                        assert_eq!(
                            history.portfolios.map(|p| history.env.portfolio_id(p)),
                            portfolio_ids
                        );
                        // Reuse never reinitializes the external context. A fresh batch must
                        // use its current return data despite the earlier generation's record.
                        let mut reopen = sizes;
                        reopen[sibling] = -sign * POS_SCALE as i128;
                        commit(
                            &mut history,
                            if batch_entry {
                                Route::Batch
                            } else {
                                Route::Single(target as u16)
                            },
                            reopen,
                            reverse,
                            &mut requests,
                            &mut evidence,
                        );
                    }

                    let exit = history.positions.map(|q| -q);
                    if batch_entry {
                        for asset in if reverse { [2, 1] } else { [1, 2] } {
                            commit(
                                &mut history,
                                Route::Single(asset),
                                exit,
                                reverse,
                                &mut requests,
                                &mut evidence,
                            );
                        }
                    } else {
                        commit(
                            &mut history,
                            Route::Batch,
                            exit,
                            reverse,
                            &mut requests,
                            &mut evidence,
                        );
                    }
                    assert_eq!(requests, 7);
                    assert_eq!(history.epoch, requests);
                    history.withdraw_all(reverse, &mut evidence);
                    assert_eq!(history.env.market_state().0.matcher_req_seq, requests);
                    evidence.worlds += 1;
                }
            }
        }
    }
    assert_eq!(evidence.worlds, 16);
    assert_eq!(replacements, 32);
    assert_eq!(evidence.fills, 112);
    assert_cu_within(
        "used-generation lifecycle",
        evidence.writer_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_cu_within(
        "used-generation entry/exit",
        evidence.fill_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );
    assert_cu_within(
        "used-generation complete withdrawal",
        evidence.custody_cu,
        CUSTODY_CU_LIMIT,
    );
    println!("INV-012 used-generation lifecycle: {evidence:?}, replacements={replacements}");
}
