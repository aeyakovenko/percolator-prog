//! Row 423 / INV-028/057/073/077: existing active-leg increases share the exit domain budget.
//! Public history retains 26 detached claims, then resizes one continuously active leg at
//! 27 and 28 occupied domains. Later settlement and full owner payouts preserve attribution.
//! This finite route/sign/order product does not close the generic admission/liveness property.

use super::*;

fn assert_frontier(h: &History, asset: u16, units: i128, expected: [u128; DOMAINS]) {
    assert_ne!(units, 0);
    assert_eq!(h.source_claims(0), expected);
    for actor in 0..2 {
        let account = h.env.portfolio_state(h.portfolios[actor]);
        assert_eq!(
            percolator::active_bitmap_count_ones(active_bitmap(&account)),
            1,
            "the resized leg stays active throughout admission and settlement"
        );
        assert_eq!(
            active_leg_for_asset(&account, asset as usize).basis_pos_q,
            units * POS_SCALE as i128 * if actor == 0 { 1 } else { -1 }
        );
        let mut resources: BTreeSet<_> = account
            .source_domains
            .iter()
            .filter(|s| s.is_occupied())
            .map(|s| s.domain.get())
            .collect();
        assert_eq!(
            resources.len(),
            if actor == 0 {
                expected.iter().filter(|claim| **claim != 0).count()
            } else {
                0
            }
        );
        resources.extend([2 * u32::from(asset), 2 * u32::from(asset) + 1]);
        assert_eq!(resources.len(), if actor == 0 { DOMAINS } else { 2 });
    }
    h.assert_accounting();
}

#[test]
fn v16_program_active_leg_increases_preserve_latent_and_full_domain_owner_exit() {
    let routes = [
        AccountResidualCounterTradePath::TradeNoCpi,
        AccountResidualCounterTradePath::TradeCpi,
        AccountResidualCounterTradePath::BatchTradeNoCpi,
        AccountResidualCounterTradePath::BatchTradeCpi,
    ];
    let asset = (ASSETS - 1) as u16;
    let historical_gain = 2 * (0..ASSETS - 1).map(|a| 1 + (a % 3) as u128).sum::<u128>();
    let expected_gain = historical_gain + 3 + 8 + 13;
    let mut worlds = 0;
    let mut calls = 0;
    let mut increases = 0;
    let mut maxima = [0; 5];
    for route in routes {
        for direction in [-1i128, 1] {
            for order in [[0, 1], [1, 0]] {
                let mut h = History::new();
                for old_asset in 0..asset {
                    let q = (1 + i128::from(old_asset % 3)) * POS_SCALE as i128;
                    let bilateral = AccountResidualCounterTradePath::TradeNoCpi;
                    h.trade(bilateral, &[(old_asset, q, PRICE)]);
                    h.mark_and_settle(&[(old_asset, PRICE + 1)], order);
                    h.trade(bilateral, &[(old_asset, -2 * q, PRICE + 1)]);
                    h.mark_and_settle(&[(old_asset, PRICE)], order);
                    h.trade(bilateral, &[(old_asset, q, PRICE)]);
                }
                let mut expected = std::array::from_fn(|domain| {
                    if domain / 2 < ASSETS - 1 {
                        (1 + (domain / 2 % 3) as u128) * BOUND_SCALE
                    } else {
                        0
                    }
                });
                assert_eq!(h.source_claims(0), expected);
                assert_eq!(expected.iter().filter(|c| **c > 0).count(), DOMAINS - 2);
                let identities = h.portfolios.map(|p| h.env.portfolio_id(p));
                h.trade(route, &[(asset, direction * 3 * POS_SCALE as i128, PRICE)]);
                assert_frontier(&h, asset, direction * 3, expected);

                let first_price = (PRICE as i128 + direction) as u64;
                let first_domain = 2 * asset as usize + usize::from(direction > 0);
                let last_domain = 2 * asset as usize + usize::from(direction < 0);
                h.mark_and_settle(&[(asset, first_price)], order);
                expected[first_domain] = 3 * BOUND_SCALE;
                assert_frontier(&h, asset, direction * 3, expected);
                assert_eq!(expected.iter().filter(|c| **c > 0).count(), DOMAINS - 1);
                assert_eq!(expected[last_domain], 0);

                // Same-sign risk increases must preserve the existing leg's latent reservation.
                h.trade(
                    route,
                    &[(asset, direction * 5 * POS_SCALE as i128, first_price)],
                );
                increases += 1;
                assert_frontier(&h, asset, direction * 8, expected);
                h.trade(
                    route,
                    &[(asset, -direction * 16 * POS_SCALE as i128, first_price)],
                );
                assert_frontier(&h, asset, -direction * 8, expected);
                h.mark_and_settle(&[(asset, PRICE)], [order[1], order[0]]);
                expected[last_domain] = 8 * BOUND_SCALE;
                assert_frontier(&h, asset, -direction * 8, expected);
                assert!(expected.iter().all(|c| *c > 0));

                // At full occupancy the same domains support another increase and later exit.
                h.trade(route, &[(asset, -direction * 5 * POS_SCALE as i128, PRICE)]);
                increases += 1;
                assert_frontier(&h, asset, -direction * 13, expected);
                let final_price = (PRICE as i128 - direction) as u64;
                h.mark_and_settle(&[(asset, final_price)], order);
                expected[last_domain] += 13 * BOUND_SCALE;
                assert_frontier(&h, asset, -direction * 13, expected);
                assert_eq!(expected.iter().sum::<u128>(), expected_gain * BOUND_SCALE);

                h.trade(
                    route,
                    &[(asset, direction * 3 * POS_SCALE as i128, final_price)],
                );
                assert_frontier(&h, asset, -direction * 10, expected);
                h.trade(
                    route,
                    &[(asset, direction * 10 * POS_SCALE as i128, final_price)],
                );
                assert_eq!(h.positions, [0; ASSETS]);
                assert_eq!(
                    h.source_claims(0),
                    expected,
                    "flattening keeps all 28 claims"
                );
                assert_eq!(h.portfolios.map(|p| h.env.portfolio_id(p)), identities);
                assert_eq!(
                    h.payout([order[1], order[0]]),
                    [CAPITAL + expected_gain, CAPITAL - expected_gain]
                );
                worlds += 1;
                calls += h.calls;
                for (maximum, cu) in maxima.iter_mut().zip([
                    h.max_trade,
                    h.max_crank,
                    h.max_convert,
                    h.max_withdraw,
                    h.max_close,
                ]) {
                    *maximum = (*maximum).max(cu);
                }
            }
        }
    }
    assert_eq!((worlds, increases), (16, 32));
    assert_eq!(expected_gain, 74);
    println!("INV-028 active-leg admission: worlds={worlds}, increases={increases}, post-funding calls={calls}, max CU trade/crank/convert/withdraw/close={maxima:?}");
}
