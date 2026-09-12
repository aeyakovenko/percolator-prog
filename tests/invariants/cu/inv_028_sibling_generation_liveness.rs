//! INV-028/057/073/077/082/089: sibling generation changes preserve admitted exit work.
//!
//! All fourteen positions and one side of every claim predate a sibling append. The
//! optional retirement/reuse changes global epochs again while the other fourteen
//! source domains remain latent. Both shapes must reach the same exact owner payout.

use super::*;

fn current(h: &History, actor: usize) -> bool {
    let group = h.env.market_state().1;
    let account = h.env.portfolio_state(h.portfolios[actor]);
    let cert = health_cert(&account);
    cert.valid
        && account.stale_state == 0
        && account.b_stale_state == 0
        && cert.cert_oracle_epoch == group.oracle_epoch
        && cert.cert_funding_epoch == group.funding_epoch
        && cert.cert_risk_epoch == group.risk_epoch
        && cert.cert_asset_set_epoch == group.asset_set_epoch
        && cert.active_bitmap_at_cert == active_bitmap(&account)
}

fn readiness_rank(h: &History) -> (u64, usize) {
    let group = h.env.market_state().1;
    let pending_slots = h
        .positions
        .iter()
        .enumerate()
        .filter(|(_, q)| **q != 0)
        .map(|(asset, _)| h.slot - group.assets[asset].slot_last)
        .sum();
    (
        pending_slots,
        (0..2).filter(|&actor| !current(h, actor)).count(),
    )
}

fn refresh(h: &mut History, order: [usize; 2]) -> usize {
    let claims = h.source_claims(0);
    let mut calls = 0;
    for step in 0..(2 * ASSETS + 4) {
        let before_rank = readiness_rank(h);
        if before_rank == (0, 0) {
            break;
        }
        let actor = order[step % 2];
        if before_rank.0 == 0 && current(h, actor) {
            continue;
        }
        let peer = h.env.svm.get_account(&h.portfolios[1 - actor]);
        h.env.svm.expire_blockhash();
        let cu = h.env.crank(
            h.portfolios[actor],
            ProgInstruction::PermissionlessCrank {
                now_slot: h.slot,
                observations: crank_observations_for_assets(
                    &(0..ASSETS as u16).collect::<Vec<_>>(),
                ),
            },
        );
        h.calls += 1;
        calls += 1;
        h.max_crank = h.max_crank.max(cu);
        assert_cu_within("sibling generation refresh", cu, CU_LIMIT);
        assert!(
            readiness_rank(h) < before_rank,
            "public readiness rank decreases"
        );
        assert_eq!(
            h.source_claims(0),
            claims,
            "refresh preserves settled attribution"
        );
        assert_eq!(h.env.svm.get_account(&h.portfolios[1 - actor]), peer);
        h.assert_accounting();
    }
    assert_eq!(
        readiness_rank(h),
        (0, 0),
        "bounded public certificate repair"
    );
    calls
}

fn trade_ready(h: &mut History, legs: &[(u16, i128, u64)], order: [usize; 2]) {
    for &leg in legs {
        refresh(h, order);
        h.trade(AccountResidualCounterTradePath::TradeNoCpi, &[leg]);
    }
}

fn change_sibling(h: &mut History, action: u8) -> u64 {
    let before = h.env.market_state().1;
    let accounts = h.portfolios.map(|p| h.env.svm.get_account(&p));
    let custody =
        [h.env.mint, h.env.vault, h.tokens[0], h.tokens[1]].map(|p| h.env.svm.get_account(&p));
    let claims = h.source_claims(0);
    assert_eq!(h.positions.iter().filter(|q| **q != 0).count(), ASSETS);
    assert_eq!(claims.iter().filter(|c| **c != 0).count(), ASSETS);
    h.slot += 1;
    h.env.svm.warp_to_slot(h.slot);
    h.env.svm.expire_blockhash();
    let cu = if action == processor::ASSET_ACTION_ACTIVATE {
        h.env.activate_asset(ASSETS as u16, h.slot, PRICE)
    } else {
        h.env
            .update_asset_lifecycle_as_admin_with_cu(action, ASSETS as u16, h.slot, 0)
    };
    h.calls += 1;
    assert_cu_within("sibling generation transition", cu, CU_LIMIT);
    let after = h.env.market_state().1;
    assert_eq!(after.config.max_market_slots as usize, ASSETS + 1);
    assert!(after.asset_set_epoch > before.asset_set_epoch);
    assert_eq!(h.portfolios.map(|p| h.env.svm.get_account(&p)), accounts);
    assert_eq!(
        [h.env.mint, h.env.vault, h.tokens[0], h.tokens[1]].map(|p| h.env.svm.get_account(&p)),
        custody,
    );
    for asset in 0..ASSETS {
        assert_eq!(after.assets[asset], before.assets[asset]);
        for domain in [2 * asset, 2 * asset + 1] {
            assert_eq!(after.source_credit[domain], before.source_credit[domain]);
            assert_eq!(
                after.source_backing_buckets[domain],
                before.source_backing_buckets[domain],
            );
        }
    }
    for domain in [DOMAINS, DOMAINS + 1] {
        assert_eq!(
            after.source_credit[domain],
            percolator::SourceCreditStateV16::EMPTY
        );
        assert_eq!(
            after.source_backing_buckets[domain].fresh_unliened_backing_num,
            0
        );
        assert_eq!(after.insurance_domain_budget[domain], 0);
    }
    if action == processor::ASSET_ACTION_ACTIVATE {
        assert_eq!(after.assets[ASSETS].market_id, before.next_market_id);
        assert_eq!(after.assets[ASSETS].lifecycle, AssetLifecycleV16::Active);
    } else {
        assert_eq!(after.assets[ASSETS].lifecycle, AssetLifecycleV16::Retired);
    }
    assert_eq!(h.source_claims(0), claims);
    assert!((0..2).any(|actor| !current(h, actor)));
    h.assert_accounting();
    cu
}

#[test]
fn v16_program_sibling_generation_changes_preserve_reserved_settlement_and_owner_exit() {
    assert_certified_engine_pin("INV-028 sibling generation liveness");
    let mut worlds = 0;
    let mut calls = 0;
    let mut max_lifecycle = 0;
    let mut max_repair_calls = 0;
    let mut maxima = [0; 5];
    for reuse in [false, true] {
        for direction in [-1i128, 1] {
            for reverse in [false, true] {
                let mut h = History::with_market_capacity(ASSETS + 1);
                let order = if reverse { [1, 0] } else { [0, 1] };
                let units: [i128; ASSETS] = std::array::from_fn(|asset| {
                    direction * (1 + (asset % 3) as i128) * if asset % 2 == 0 { 1 } else { -1 }
                });
                let opening: Vec<_> = (0..ASSETS)
                    .map(|a| (a as u16, units[a] * POS_SCALE as i128, PRICE))
                    .collect();
                trade_ready(&mut h, &opening, order);
                let marks: Vec<_> = (0..ASSETS)
                    .map(|a| (a as u16, (PRICE as i128 + units[a].signum()) as u64))
                    .collect();
                h.mark_and_settle(&marks, order);
                let flips: Vec<_> = marks
                    .iter()
                    .map(|&(a, p)| (a, -2 * units[a as usize] * POS_SCALE as i128, p))
                    .collect();
                trade_ready(&mut h, &flips, order);
                refresh(&mut h, order);
                assert_eq!(readiness_rank(&h), (0, 0));
                let retained = h.source_claims(0);
                // These fourteen occupied and fourteen latent domains exhaust the account budget.
                assert_eq!(retained.iter().filter(|c| **c != 0).count(), DOMAINS / 2);
                assert_eq!(
                    h.env.market_state().1.config.max_market_slots as usize,
                    ASSETS
                );
                max_lifecycle =
                    max_lifecycle.max(change_sibling(&mut h, processor::ASSET_ACTION_ACTIVATE));
                let first_generation = h.env.asset_market_id(ASSETS as u16);
                if reuse {
                    max_lifecycle =
                        max_lifecycle.max(change_sibling(&mut h, processor::ASSET_ACTION_RETIRE));
                    max_lifecycle =
                        max_lifecycle.max(change_sibling(&mut h, processor::ASSET_ACTION_ACTIVATE));
                    assert_ne!(h.env.asset_market_id(ASSETS as u16), first_generation);
                }
                max_repair_calls = max_repair_calls.max(refresh(&mut h, order));
                assert_eq!(h.source_claims(0), retained);

                h.mark_and_settle(
                    &(0..ASSETS as u16).map(|a| (a, PRICE)).collect::<Vec<_>>(),
                    order,
                );
                let expected =
                    std::array::from_fn(|domain| units[domain / 2].unsigned_abs() * BOUND_SCALE);
                assert_eq!(h.source_claims(0), expected);
                assert!(expected.iter().all(|claim| *claim > 0));
                assert_eq!(h.positions.iter().filter(|q| **q != 0).count(), ASSETS);
                for domain in 0..DOMAINS {
                    if retained[domain] != 0 {
                        assert_eq!(expected[domain], retained[domain]);
                    }
                }
                for &leg in &opening {
                    let before = h.positions.iter().filter(|q| **q != 0).count();
                    trade_ready(&mut h, &[leg], order);
                    assert_eq!(h.positions.iter().filter(|q| **q != 0).count(), before - 1);
                    for portfolio in h.portfolios {
                        assert_eq!(
                            percolator::active_bitmap_count_ones(active_bitmap(
                                &h.env.portfolio_state(portfolio)
                            )) as usize,
                            before - 1,
                            "each owner-authorized exit lowers the exposure rank",
                        );
                    }
                }
                let gain = 2 * units.iter().map(|q| q.unsigned_abs()).sum::<u128>();
                assert_eq!(h.payout(order), [CAPITAL + gain, CAPITAL - gain]);
                let group = h.env.market_state().1;
                assert_eq!(group.assets[ASSETS].lifecycle, AssetLifecycleV16::Active);
                assert_eq!(group.assets[ASSETS].oi_eff_long_q, 0);
                assert_eq!(group.assets[ASSETS].oi_eff_short_q, 0);
                for domain in [DOMAINS, DOMAINS + 1] {
                    assert_eq!(
                        group.source_credit[domain],
                        percolator::SourceCreditStateV16::EMPTY
                    );
                    assert_eq!(group.insurance_domain_budget[domain], 0);
                }
                worlds += 1;
                calls += h.calls;
                for (max, cu) in maxima.iter_mut().zip([
                    h.max_trade,
                    h.max_crank,
                    h.max_convert,
                    h.max_withdraw,
                    h.max_close,
                ]) {
                    *max = (*max).max(cu);
                }
            }
        }
    }
    assert_eq!(worlds, 8);
    assert!(max_repair_calls > 0);
    println!("INV-028 sibling generation: worlds={worlds}, history_calls={calls}, repair_calls<={max_repair_calls}/{}, lifecycle_cu={max_lifecycle}, max CU trade/crank/convert/withdraw/close={maxima:?}", 2 * ASSETS + 4);
}
