//! INV-028 / row 423: all admitted legs precede source-table growth.
//!
//! The parent owns historical-first admission with at most two active legs. Here all fourteen
//! positions are admitted without claims, then retained two-sided claims grow while every leg
//! stays active. Independent schedules must preserve the same remaining latent capacity and
//! converge to the same complete claim and owner payout. No over-capacity admission is attempted.

use super::*;

fn readiness_rank(h: &History) -> u64 {
    let group = h.env.market_state().1;
    let pending_slots: u64 = h
        .positions
        .iter()
        .enumerate()
        .filter(|(_, q)| **q != 0)
        .map(|(asset, _)| h.slot - group.assets[asset].slot_last)
        .sum();
    pending_slots + (0..2).filter(|&actor| !is_current(h, actor)).count() as u64
}

fn is_current(h: &History, actor: usize) -> bool {
    let group = h.env.market_state().1;
    let account = h.env.portfolio_state(h.portfolios[actor]);
    if percolator::active_bitmap_is_empty(active_bitmap(&account)) {
        return true;
    }
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

fn trade_current(h: &mut History, legs: &[(u16, i128, u64)]) {
    for &leg in legs {
        // Large-portfolio trades require current certificates. Include the public catch-up and
        // recertification work instead of relying on the small-shape trade fast path.
        for step in 0..(2 * ASSETS + 4) {
            let before_rank = readiness_rank(h);
            if before_rank == 0 {
                break;
            }
            let actor = step % 2;
            if is_current(h, actor) {
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
            h.max_crank = h.max_crank.max(cu);
            assert_cu_within("concurrent latent readiness", cu, CU_LIMIT);
            h.assert_accounting();
            assert_eq!(h.env.svm.get_account(&h.portfolios[1 - actor]), peer);
            assert!(readiness_rank(h) < before_rank);
        }
        assert_eq!(readiness_rank(h), 0, "bounded public trade readiness");
        h.trade(AccountResidualCounterTradePath::TradeNoCpi, &[leg]);
    }
}

fn assert_concurrent_frontier(h: &History, settled: &BTreeSet<u16>, units: &[i128; ASSETS]) {
    let expected_claims = std::array::from_fn(|domain| {
        if settled.contains(&((domain / 2) as u16)) {
            units[domain / 2].unsigned_abs() * BOUND_SCALE
        } else {
            0
        }
    });
    assert_eq!(h.source_claims(0), expected_claims);
    for actor in 0..2 {
        let account = h.env.portfolio_state(h.portfolios[actor]);
        assert_eq!(
            percolator::active_bitmap_count_ones(active_bitmap(&account)) as usize,
            ASSETS,
            "no historical leg may be flattened to make settlement cheap"
        );
        for (asset, &quantity) in units.iter().enumerate() {
            let direction = if settled.contains(&(asset as u16)) {
                -1
            } else {
                1
            };
            assert_eq!(
                active_leg_for_asset(&account, asset).basis_pos_q,
                quantity * POS_SCALE as i128 * direction * if actor == 0 { 1 } else { -1 }
            );
        }
    }
    let occupied = expected_claims.iter().filter(|claim| **claim != 0).count();
    assert_eq!(occupied, 2 * settled.len());
    assert_eq!(
        occupied + 2 * (ASSETS - settled.len()),
        DOMAINS,
        "materialized history cannot displace another admitted leg's future domains"
    );
    h.assert_accounting();
}

#[test]
fn v16_program_concurrent_latent_cohorts_preserve_full_shape_settlement_and_exit() {
    let units: [i128; ASSETS] = std::array::from_fn(|asset| {
        (1 + (asset % 5) as i128) * if asset % 2 == 0 { 1 } else { -1 }
    });
    let expected_gain = 2 * units.iter().map(|q| q.unsigned_abs()).sum::<u128>();
    let mut worlds = 0;
    let mut calls = 0;
    let mut maxima = [0; 5];
    let mut endpoint = None;
    // The singleton edges and balanced split retain different concurrent worksets. The unsplit
    // control settles every initially latent asset in one observation cohort.
    for first_cohort in [1, ASSETS / 2, ASSETS - 1, ASSETS] {
        for reverse_assets in [false, true] {
            for loser_first in [false, true] {
                let order = if loser_first { [1, 0] } else { [0, 1] };
                let mut h = History::new();
                let mut assets: Vec<_> = (0..ASSETS as u16).collect();
                if reverse_assets {
                    assets.reverse();
                }
                let opening: Vec<_> = assets
                    .iter()
                    .map(|&a| (a, units[a as usize] * POS_SCALE as i128, PRICE))
                    .collect();
                trade_current(&mut h, &opening);
                let mut settled = BTreeSet::new();
                assert_concurrent_frontier(&h, &settled, &units);

                for cohort in [&assets[..first_cohort], &assets[first_cohort..]] {
                    if cohort.is_empty() {
                        continue;
                    }
                    let historical = h.source_claims(0);
                    let marks: Vec<_> = cohort
                        .iter()
                        .map(|&a| (a, (PRICE as i128 + units[a as usize].signum()) as u64))
                        .collect();
                    h.mark_and_settle(&marks, order);
                    assert_eq!(
                        h.source_claims(0).iter().filter(|c| **c != 0).count(),
                        2 * settled.len() + cohort.len(),
                        "every concurrent favorable obligation materializes exactly once"
                    );
                    let flips: Vec<_> = marks
                        .iter()
                        .map(|&(a, price)| (a, -2 * units[a as usize] * POS_SCALE as i128, price))
                        .collect();
                    trade_current(&mut h, &flips);
                    h.mark_and_settle(
                        &cohort.iter().map(|&a| (a, PRICE)).collect::<Vec<_>>(),
                        [order[1], order[0]],
                    );
                    let claims = h.source_claims(0);
                    for &asset in &settled {
                        let first = 2 * asset as usize;
                        assert_eq!(
                            &claims[first..first + 2],
                            &historical[first..first + 2],
                            "later cohorts must frame previously realized attribution"
                        );
                    }
                    settled.extend(cohort.iter().copied());
                    assert_concurrent_frontier(&h, &settled, &units);
                }
                assert_eq!(settled.len(), ASSETS);
                assert_eq!(h.claims.iter().sum::<u128>(), expected_gain);
                let group = h.env.market_state().1;
                let economic_endpoint = (
                    h.source_claims(0),
                    h.portfolios.map(|p| {
                        let account = h.env.portfolio_state(p);
                        (account.capital.get(), account.pnl.get())
                    }),
                    group.c_tot,
                    group.vault,
                );
                if let Some(expected) = endpoint {
                    assert_eq!(economic_endpoint, expected);
                }
                endpoint = Some(economic_endpoint);

                trade_current(&mut h, &opening);
                assert_eq!(
                    h.payout([order[1], order[0]]),
                    [CAPITAL + expected_gain, CAPITAL - expected_gain]
                );
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
    assert_eq!(worlds, 16);
    println!("INV-028 concurrent latent cohorts: worlds={worlds}, post-funding calls={calls}, max CU trade/crank/convert/withdraw/close={maxima:?}");
}
