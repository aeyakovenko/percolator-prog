//! Row 423 / INV-028/044/057/073/077: reuse occupied domains without reclaiming claims.
//! This is same-generation position admission, not retired asset reactivation (INV-089).

use super::*;

fn move_while_flat(h: &mut History, moves: &[(u16, u64)]) -> u64 {
    assert_eq!(h.positions, [0; ASSETS]);
    let claims = h.source_claims(0);
    h.previous_prices = h.prices;
    h.slot += 1;
    h.env.svm.warp_to_slot(h.slot);
    let mut max_cu = 0;
    for &(asset, price) in moves {
        h.prices[asset as usize] = price;
        let cu = h
            .env
            .push_auth_mark_for_asset_as_admin(asset, h.slot, price);
        max_cu = max_cu.max(cu);
        h.calls += 1;
        h.assert_accounting();
        assert_eq!(h.source_claims(0), claims);
    }
    let rank = |h: &History| {
        let group = h.env.market_state().1;
        moves
            .iter()
            .map(|m| h.slot - group.assets[m.0 as usize].slot_last)
            .sum::<u64>()
    };
    for _ in 0..(2 * ASSETS + 4) {
        let before = rank(h);
        if before == 0 {
            break;
        }
        h.env.svm.expire_blockhash();
        let cu = h.env.crank(
            h.portfolios[0],
            ProgInstruction::PermissionlessCrank {
                now_slot: h.slot,
                observations: crank_observations_for_assets(
                    &moves.iter().map(|m| m.0).collect::<Vec<_>>(),
                ),
            },
        );
        h.calls += 1;
        h.max_crank = h.max_crank.max(cu);
        assert_cu_within("flat retained-domain accrual", cu, CU_LIMIT);
        assert!(rank(h) < before);
        h.assert_accounting();
        assert_eq!(h.source_claims(0), claims);
    }
    assert_eq!(
        rank(h),
        0,
        "public marks are fully accrued before admission"
    );
    h.previous_prices = h.prices;
    h.assert_accounting();
    assert_cu_within("flat retained-domain mark", max_cu, CU_LIMIT);
    max_cu
}

#[test]
fn v16_program_full_history_reused_episodes_preserve_claims_and_drain_exit() {
    let mut worlds = 0;
    let mut calls = 0;
    let mut maxima = [0; 7];
    let mut endpoint = None;
    for batch in [false, true] {
        for direction in [-1i128, 1] {
            for reverse in [false, true] {
                let mut h = History::new();
                let order = if reverse { [1, 0] } else { [0, 1] };
                for asset in 0..ASSETS as u16 {
                    let q = (1 + i128::from(asset % 3)) * POS_SCALE as i128;
                    h.trade(
                        AccountResidualCounterTradePath::TradeNoCpi,
                        &[(asset, q, PRICE)],
                    );
                    h.mark_and_settle(&[(asset, PRICE + 1)], order);
                    h.trade(
                        AccountResidualCounterTradePath::TradeNoCpi,
                        &[(asset, -2 * q, PRICE + 1)],
                    );
                    h.mark_and_settle(&[(asset, PRICE)], order);
                    h.trade(
                        AccountResidualCounterTradePath::TradeNoCpi,
                        &[(asset, q, PRICE)],
                    );
                }
                let historical = h.source_claims(0);
                assert!(historical.iter().all(|c| *c > 0));
                assert_eq!(h.positions, [0; ASSETS]);
                let historical_gain: u128 = 2
                    * (0..ASSETS)
                        .map(|asset| 1 + (asset % 3) as u128)
                        .sum::<u128>();
                assert_eq!(
                    historical.iter().sum::<u128>(),
                    historical_gain * BOUND_SCALE
                );

                // Both future domain pairs are already occupied. Price/index movement while
                // detached must not accrue to the newly admitted episodes or erase old claims.
                let mut opening = vec![
                    (
                        0,
                        direction * 6 * POS_SCALE as i128,
                        (PRICE as i128 + direction) as u64,
                    ),
                    (
                        (ASSETS - 1) as u16,
                        -direction * 10 * POS_SCALE as i128,
                        (PRICE as i128 - direction) as u64,
                    ),
                ];
                if reverse {
                    opening.reverse();
                }
                let max_mark = move_while_flat(
                    &mut h,
                    &opening.iter().map(|l| (l.0, l.2)).collect::<Vec<_>>(),
                );
                let route = if batch {
                    AccountResidualCounterTradePath::BatchTradeNoCpi
                } else {
                    AccountResidualCounterTradePath::TradeNoCpi
                };
                let old_ids = h.portfolios.map(|p| h.env.portfolio_id(p));
                let old_epoch = h.portfolios.map(|p| h.env.portfolio_position_epoch(p));
                h.trade(route, &opening);
                assert_eq!(h.source_claims(0), historical, "admission creates no claim");
                for actor in 0..2 {
                    let account = h.env.portfolio_state(h.portfolios[actor]);
                    assert_eq!(h.env.portfolio_id(h.portfolios[actor]), old_ids[actor]);
                    assert!(h.env.portfolio_position_epoch(h.portfolios[actor]) > old_epoch[actor]);
                    assert_eq!(
                        percolator::active_bitmap_count_ones(active_bitmap(&account)),
                        2
                    );
                }
                let marks: Vec<_> = opening
                    .iter()
                    .map(|&(asset, q, price)| (asset, (price as i128 + q.signum()) as u64))
                    .collect();
                h.mark_and_settle(&marks, order);
                let mut expected = historical;
                for &(asset, q, _) in &opening {
                    let domain = 2 * asset as usize + usize::from(q > 0);
                    expected[domain] += q.unsigned_abs() / POS_SCALE * BOUND_SCALE;
                }
                assert_eq!(h.source_claims(0), expected);
                assert!(
                    expected.iter().all(|c| *c > 0),
                    "no reclamation before exit"
                );
                let retained = h.env.portfolio_state(h.portfolios[0]).source_domains;

                let mut max_lifecycle = 0;
                for &(asset, _, _) in &opening {
                    let frames = h.portfolios.map(|p| h.env.svm.get_account(&p));
                    let cu = h.env.update_asset_lifecycle_as_admin_with_cu(
                        processor::ASSET_ACTION_DRAIN_ONLY,
                        asset,
                        0,
                        0,
                    );
                    max_lifecycle = max_lifecycle.max(cu);
                    h.calls += 1;
                    assert_cu_within("retained-domain DrainOnly", cu, CU_LIMIT);
                    assert_eq!(
                        h.env.market_state().1.assets[asset as usize].lifecycle,
                        AssetLifecycleV16::DrainOnly
                    );
                    assert_eq!(h.portfolios.map(|p| h.env.svm.get_account(&p)), frames);
                    h.assert_accounting();
                }
                // Partial and final reductions must preserve every historical source record.
                let reduction: Vec<_> = opening
                    .iter()
                    .zip(&marks)
                    .map(|(&(asset, q, _), &(_, price))| (asset, -q / 2, price))
                    .collect();
                for _ in 0..2 {
                    h.trade(route, &reduction);
                    assert_eq!(
                        h.env.portfolio_state(h.portfolios[0]).source_domains,
                        retained
                    );
                }
                let payout = h.payout([order[1], order[0]]);
                assert_eq!(
                    payout,
                    [
                        CAPITAL + historical_gain + 16,
                        CAPITAL - historical_gain - 16
                    ]
                );
                if let Some(expected) = endpoint {
                    assert_eq!(
                        payout, expected,
                        "schedule and direction preserve total entitlement"
                    );
                }
                endpoint = Some(payout);
                worlds += 1;
                calls += h.calls;
                for (max, cu) in maxima.iter_mut().zip([
                    h.max_trade,
                    h.max_crank,
                    h.max_convert,
                    h.max_withdraw,
                    h.max_close,
                    max_mark,
                    max_lifecycle,
                ]) {
                    *max = (*max).max(cu);
                }
            }
        }
    }
    assert_eq!(worlds, 8);
    println!("INV-028 retained-domain episodes: worlds={worlds}, post-funding calls={calls}, max CU trade/crank/convert/withdraw/close/flat-mark/lifecycle={maxima:?}");
}
