//! Row 423 / INV-028/057/073/077: closing an unmaterialized leg frees its future
//! domain pair for an unrelated admission without converting historical claims.

use super::*;

fn assert_latent_pair(h: &SparseHistory, asset: u16, historical: [u128; 2 * ASSETS]) {
    assert_eq!(h.claims_for(0), historical);
    assert_eq!(
        historical.iter().filter(|claim| **claim != 0).count(),
        CAPACITY - 2
    );
    let mut resources: BTreeSet<_> = historical
        .iter()
        .enumerate()
        .filter_map(|(domain, claim)| (*claim != 0).then_some(domain))
        .collect();
    assert_eq!(historical[2 * asset as usize], 0);
    assert_eq!(historical[2 * asset as usize + 1], 0);
    resources.extend([2 * asset as usize, 2 * asset as usize + 1]);
    assert_eq!(resources.len(), CAPACITY);
    assert_eq!(h.position.unwrap().0, asset);
    h.check();
}

#[test]
fn v16_program_latent_pair_reuse_preserves_historical_claims_and_replacement_exit() {
    assert_certified_engine_pin("INV-028 latent capacity reuse");
    let historical_assets = CAPACITY - 2;
    let historical_gain: u128 = (0..historical_assets)
        .map(|asset| 1 + (asset % 3) as u128)
        .sum();
    let mut worlds = 0;
    let mut calls = 0;
    let mut maxima = [0; 5];

    for batch in [false, true] {
        for direction in [-1i128, 1] {
            for reverse in [false, true] {
                let mut h = SparseHistory::new();
                let order = if reverse { [1, 0] } else { [0, 1] };
                for asset in 0..historical_assets as u16 {
                    let q = direction * (1 + i128::from(asset % 3)) * POS_SCALE as i128;
                    h.trade(asset, q);
                    h.settle_mark((PRICE as i128 + direction) as u64, order);
                    h.trade(asset, -q);
                }
                let historical = std::array::from_fn(|domain| {
                    if domain / 2 < historical_assets && domain % 2 == usize::from(direction > 0) {
                        (1 + (domain / 2 % 3) as u128) * BOUND_SCALE
                    } else {
                        0
                    }
                });
                assert_eq!(h.claims_for(0), historical);
                assert_eq!(
                    historical.iter().sum::<u128>(),
                    historical_gain * BOUND_SCALE
                );
                assert!(h.position.is_none());
                let retained = h
                    .portfolios
                    .map(|p| h.env.portfolio_state(p).source_domains);
                let identities = h.portfolios.map(|p| h.env.portfolio_id(p));
                let capital = h.portfolios.map(|p| h.env.portfolio_state(p).capital.get());
                let vault = h.env.svm.get_account(&h.env.vault);

                let old_asset = (historical_assets + usize::from(reverse)) as u16;
                let new_asset = (historical_assets + usize::from(!reverse)) as u16;
                let old_q = direction * 6 * POS_SCALE as i128;
                let new_q = -direction * 7 * POS_SCALE as i128;
                h.trade(old_asset, old_q);
                assert_latent_pair(&h, old_asset, historical);
                h.trade(old_asset, -old_q / 2);
                assert_latent_pair(&h, old_asset, historical);

                // The final close relinquishes two future domains. Compare one public batch
                // with its separate-instruction continuation while every historical claim stays.
                if batch {
                    h.env.svm.expire_blockhash();
                    let ix = h.env.batch_trade_no_cpi_ix(
                        h.portfolios[0],
                        h.portfolios[1],
                        [(old_asset, -old_q / 2), (new_asset, new_q)]
                            .into_iter()
                            .map(|(asset_index, size_q)| BatchTradeLeg {
                                asset_index,
                                market_id: h.env.asset_market_id(asset_index),
                                size_q,
                                exec_price: PRICE,
                                fee_bps: 0,
                            })
                            .collect(),
                    );
                    let cu = h
                        .env
                        .send(
                            ix,
                            vec![
                                AccountMeta::new(h.owners[0].pubkey(), true),
                                AccountMeta::new(h.owners[1].pubkey(), true),
                                AccountMeta::new(h.env.market, false),
                                AccountMeta::new(h.portfolios[0], false),
                                AccountMeta::new(h.portfolios[1], false),
                            ],
                            &[&h.owners[0], &h.owners[1]],
                        )
                        .expect("close then replacement admission fits the future-domain budget");
                    h.position = Some((new_asset, new_q));
                    h.observe_cu(0, cu);
                } else {
                    h.trade(old_asset, -old_q / 2);
                    assert!(h.position.is_none());
                    assert_eq!(h.claims_for(0), historical);
                    h.trade(new_asset, new_q);
                }
                assert_latent_pair(&h, new_asset, historical);
                assert_eq!(
                    h.portfolios
                        .map(|p| h.env.portfolio_state(p).source_domains),
                    retained
                );
                assert_eq!(
                    h.portfolios.map(|p| h.env.portfolio_state(p).capital.get()),
                    capital
                );
                assert_eq!(h.env.svm.get_account(&h.env.vault), vault);

                // The replacement must materialize both sides, including the second domain
                // after a cross-zero admission. No historical conversion funds this reuse.
                h.settle_mark((PRICE as i128 - direction) as u64, order);
                let first_domain = 2 * new_asset as usize + usize::from(new_q > 0);
                let mut expected = historical;
                expected[first_domain] = 7 * BOUND_SCALE;
                assert_eq!(h.claims_for(0), expected);
                assert_eq!(
                    expected.iter().filter(|claim| **claim != 0).count(),
                    CAPACITY - 1
                );
                h.trade(new_asset, direction * 18 * POS_SCALE as i128);
                assert_eq!(h.claims_for(0), expected);
                assert_eq!(
                    h.position,
                    Some((new_asset, direction * 11 * POS_SCALE as i128))
                );
                h.settle_mark(PRICE, [order[1], order[0]]);
                expected[first_domain ^ 1] = 11 * BOUND_SCALE;
                assert_eq!(h.claims_for(0), expected);
                assert_eq!(
                    expected.iter().filter(|claim| **claim != 0).count(),
                    CAPACITY
                );
                assert_eq!(expected[2 * old_asset as usize], 0);
                assert_eq!(expected[2 * old_asset as usize + 1], 0);
                h.trade(new_asset, -direction * 11 * POS_SCALE as i128);
                assert_eq!(h.claims_for(0), expected);
                assert_eq!(h.portfolios.map(|p| h.env.portfolio_id(p)), identities);
                assert_eq!(
                    h.maxima[2], 0,
                    "no conversion before replacement settlement"
                );
                assert_eq!(
                    h.payout([order[1], order[0]]),
                    [
                        CAPITAL + historical_gain + 18,
                        CAPITAL - historical_gain - 18
                    ]
                );
                worlds += 1;
                calls += h.calls;
                for (maximum, cu) in maxima.iter_mut().zip(h.maxima) {
                    *maximum = (*maximum).max(cu);
                }
            }
        }
    }
    assert_eq!(worlds, 8);
    println!("INV-028 latent capacity reuse: worlds={worlds}, successful post-funding calls={calls}, max CU trade/mark-crank/convert/withdraw/close={maxima:?}");
}
