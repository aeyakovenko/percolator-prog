//! INV-028/031/057/073/077/078/082: an admitted future domain survives peer ADL/reset.
//! Twenty-six historical claims and one settled side remain occupied while the winner
//! skips refresh across a nonunit ADL price move and the counterparty's complete exit.
//! Public prior-epoch cleanup must materialize the last source and preserve full payout.

use super::*;

const LIVE: usize = ASSETS - 1;
const UNITS: u128 = 6;

fn check_suffix(h: &History, prefixes: &[[u128; DOMAINS]], active: [bool; 2], effective: u128) {
    let group = h.env.market_state().1;
    let claims = h.source_claims(0);
    assert_eq!(h.source_claims(1), [0; DOMAINS]);
    let mut backing = 0;
    for d in 0..DOMAINS {
        assert!(
            prefixes.iter().any(|p| claims[d] == p[d] * BOUND_SCALE),
            "domain {d}: input-derived claim prefix"
        );
        let source = &group.source_credit[d];
        let bucket = &group.source_backing_buckets[d];
        let available = bucket.fresh_unliened_backing_num;
        assert!(
            prefixes.iter().any(|p| available == p[d] * BOUND_SCALE),
            "domain {d}: input-derived backing prefix"
        );
        assert_eq!(source.exact_positive_claim_num, claims[d]);
        assert_eq!(source.positive_claim_bound_num, claims[d]);
        assert_eq!(source.fresh_reserved_backing_num, available);
        assert_eq!(source.valid_liened_backing_num, 0);
        assert_eq!(source.impaired_liened_backing_num, 0);
        assert_eq!(source.insurance_credit_reserved_num, 0);
        assert_eq!(source.provider_receivable_num, 0);
        assert_eq!(bucket.valid_liened_backing_num, 0);
        assert_eq!(bucket.impaired_liened_backing_num, 0);
        assert!(claims[d] * source.credit_rate_num / percolator::CREDIT_RATE_SCALE <= available);
        backing += available / BOUND_SCALE;
    }
    for actor in 0..2 {
        let account = h.env.portfolio_state(h.portfolios[actor]);
        assert_eq!(
            account.capital.get(),
            CAPITAL - if actor == 1 { backing } else { 0 }
        );
        assert_eq!(
            account.pnl.get(),
            if actor == 0 {
                (claims.iter().sum::<u128>() / BOUND_SCALE) as i128
            } else {
                0
            }
        );
        assert_eq!(account.reserved_pnl.get(), 0);
        assert_eq!(
            percolator::active_bitmap_count_ones(active_bitmap(&account)),
            u32::from(active[actor])
        );
        let mut resources: BTreeSet<_> = account
            .source_domains
            .iter()
            .filter(|s| s.is_occupied())
            .map(|s| s.domain.get())
            .collect();
        if active[actor] {
            let leg = active_leg_for_asset(&account, LIVE);
            if effective != 0 {
                assert_eq!(
                    reference_current_epoch_effective_abs(&group, leg),
                    effective
                );
            }
            resources.extend([2 * LIVE as u32, 2 * LIVE as u32 + 1]);
        }
        assert!(
            resources.len() <= DOMAINS,
            "retained and future source resources fit together"
        );
        for a in 0..ASSETS {
            assert_eq!(
                has_active_leg_for_asset(&account, a),
                active[actor] && a == LIVE
            );
        }
    }
    for a in 0..ASSETS {
        let q = if a == LIVE { effective } else { 0 };
        assert_eq!(
            (
                group.assets[a].oi_eff_long_q,
                group.assets[a].oi_eff_short_q
            ),
            (q, q)
        );
    }
    assert_eq!(
        (group.c_tot, group.vault, group.insurance),
        (2 * CAPITAL - backing, 2 * CAPITAL, 0)
    );
    assert_eq!(h.env.token_amount(h.env.vault) as u128, 2 * CAPITAL);
    assert!(h.tokens.iter().all(|t| h.env.token_amount(*t) == 0));
    assert_eq!(
        Mint::unpack(&h.env.svm.get_account(&h.env.mint).unwrap().data)
            .unwrap()
            .supply as u128,
        2 * CAPITAL
    );
}

fn accrue_with_loser(
    h: &mut History,
    price: u64,
    effective: u128,
    prefixes: &mut Vec<[u128; DOMAINS]>,
) {
    let direction = (price as i128 - h.prices[LIVE] as i128).signum();
    assert_eq!(price.abs_diff(h.prices[LIVE]), 1);
    let domain = 2 * LIVE + usize::from(direction > 0);
    h.claims[domain] += effective / POS_SCALE;
    h.prices[LIVE] = price;
    prefixes.push(h.claims);
    h.slot += 1;
    h.env.svm.warp_to_slot(h.slot);
    let winner = h.env.svm.get_account(&h.portfolios[0]);
    let cu = h
        .env
        .push_auth_mark_for_asset_as_admin(LIVE as u16, h.slot, price);
    h.calls += 1;
    assert_cu_within("latent-reset authenticated mark", cu, CU_LIMIT);
    check_suffix(h, prefixes, [true; 2], effective);
    let total = h.claims.iter().sum::<u128>();
    let rank = |h: &History| {
        u128::from(h.slot - h.env.market_state().1.assets[LIVE].slot_last)
            + h.env
                .portfolio_state(h.portfolios[1])
                .capital
                .get()
                .abs_diff(CAPITAL - total)
    };
    for _ in 0..4 {
        let before = rank(h);
        if before == 0 {
            break;
        }
        h.env.svm.expire_blockhash();
        let cu = h.env.crank(
            h.portfolios[1],
            ProgInstruction::PermissionlessCrank {
                now_slot: h.slot,
                observations: crank_observations(LIVE as u16),
            },
        );
        h.calls += 1;
        h.max_crank = h.max_crank.max(cu);
        assert_cu_within("latent-reset loser accrual", cu, CU_LIMIT);
        assert!(
            rank(h) < before,
            "bounded global accrual and principal debit progress"
        );
        assert_eq!(h.env.svm.get_account(&h.portfolios[0]), winner);
        check_suffix(h, prefixes, [true; 2], effective);
    }
    assert_eq!(rank(h), 0);
}

fn reduce_loser(h: &mut History, prefixes: &[[u128; DOMAINS]], remaining: u128) -> u64 {
    let winner = h.env.svm.get_account(&h.portfolios[0]);
    let before = h.env.market_state().1.assets[LIVE].oi_eff_long_q;
    let ix = Instruction {
        program_id: h.env.program_id,
        accounts: vec![
            AccountMeta::new(h.owners[1].pubkey(), true),
            AccountMeta::new(h.env.market, false),
            AccountMeta::new(h.portfolios[1], false),
        ],
        data: ProgInstruction::RebalanceReduce {
            portfolio_id: h.env.portfolio_id(h.portfolios[1]),
            position_epoch: h.env.portfolio_position_epoch(h.portfolios[1]),
            asset_index: LIVE as u16,
            reduce_q: UNITS / 2 * POS_SCALE,
        }
        .encode(),
    };
    h.env.svm.expire_blockhash();
    let cu = send_raw_tx(&mut h.env.svm, &h.owners[1], ix, &[])
        .expect("funded loser can reduce using only their own signature");
    h.calls += 1;
    assert_cu_within("latent-reset owner-only reduction", cu, CU_LIMIT);
    assert_eq!(h.env.svm.get_account(&h.portfolios[0]), winner);
    assert_eq!(before - remaining, UNITS / 2 * POS_SCALE);
    check_suffix(h, prefixes, [true, remaining != 0], remaining);
    cu
}

#[test]
fn v16_program_latent_source_survives_owner_reduction_and_prior_epoch_exit() {
    assert_certified_engine_pin("INV-028 latent source through peer ADL/reset");
    let routes = [
        AccountResidualCounterTradePath::TradeNoCpi,
        AccountResidualCounterTradePath::TradeCpi,
        AccountResidualCounterTradePath::BatchTradeNoCpi,
        AccountResidualCounterTradePath::BatchTradeCpi,
    ];
    let mut worlds = 0;
    let mut calls = 0;
    let mut maxima = [0; 7];
    let mut max_cleanup_calls = 0;
    for route in routes {
        for direction in [-1i128, 1] {
            for drain in [false, true] {
                let mut h = History::new();
                for asset in 0..LIVE as u16 {
                    let q = (1 + i128::from(asset % 3)) * POS_SCALE as i128;
                    h.trade(
                        AccountResidualCounterTradePath::TradeNoCpi,
                        &[(asset, q, PRICE)],
                    );
                    h.mark_and_settle(&[(asset, PRICE + 1)], [1, 0]);
                    h.trade(
                        AccountResidualCounterTradePath::TradeNoCpi,
                        &[(asset, -2 * q, PRICE + 1)],
                    );
                    h.mark_and_settle(&[(asset, PRICE)], [0, 1]);
                    h.trade(
                        AccountResidualCounterTradePath::TradeNoCpi,
                        &[(asset, q, PRICE)],
                    );
                }
                let history = h.source_claims(0);
                assert_eq!(history.iter().filter(|c| **c > 0).count(), DOMAINS - 2);
                let q = direction * UNITS as i128 * POS_SCALE as i128;
                h.trade(route, &[(LIVE as u16, q, PRICE)]);
                assert_eq!(h.source_claims(0), history);
                let first_price = (PRICE as i128 + direction) as u64;
                h.mark_and_settle(&[(LIVE as u16, first_price)], [1, 0]);
                h.trade(route, &[(LIVE as u16, -2 * q, first_price)]);
                let retained = h.source_claims(0);
                let latent = 2 * LIVE + usize::from(direction < 0);
                assert_eq!(retained.iter().filter(|c| **c > 0).count(), DOMAINS - 1);
                assert_eq!(retained[latent], 0);
                let mut prefixes = vec![h.claims];
                let mint = h.env.svm.get_account(&h.env.mint);
                let mut max_lifecycle = 0;
                if drain {
                    let frames = h.portfolios.map(|p| h.env.svm.get_account(&p));
                    max_lifecycle = h.env.update_asset_lifecycle_as_admin_with_cu(
                        processor::ASSET_ACTION_DRAIN_ONLY,
                        LIVE as u16,
                        0,
                        0,
                    );
                    h.calls += 1;
                    assert_cu_within("latent-reset DrainOnly", max_lifecycle, CU_LIMIT);
                    assert_eq!(
                        h.env.market_state().1.assets[LIVE].lifecycle,
                        AssetLifecycleV16::DrainOnly
                    );
                    assert_eq!(h.portfolios.map(|p| h.env.svm.get_account(&p)), frames);
                    check_suffix(&h, &prefixes, [true; 2], UNITS * POS_SCALE);
                }

                accrue_with_loser(&mut h, PRICE, UNITS * POS_SCALE, &mut prefixes);
                let half = UNITS / 2 * POS_SCALE;
                let mut max_reduce = reduce_loser(&mut h, &prefixes, half);
                let asset = h.env.market_state().1.assets[LIVE];
                assert_eq!(
                    if direction > 0 {
                        asset.a_short
                    } else {
                        asset.a_long
                    },
                    ADL_ONE / 2
                );
                assert_eq!(h.source_claims(0), retained);
                accrue_with_loser(
                    &mut h,
                    (PRICE as i128 - direction) as u64,
                    half,
                    &mut prefixes,
                );
                max_reduce = max_reduce.max(reduce_loser(&mut h, &prefixes, 0));
                assert_eq!(
                    h.source_claims(0),
                    retained,
                    "winner's last source is still absent at peer exit"
                );
                let reset_side = u8::from(direction > 0);
                let asset = h.env.market_state().1.assets[LIVE];
                assert_eq!(
                    if reset_side == 0 {
                        asset.mode_long
                    } else {
                        asset.mode_short
                    },
                    SideModeV16::ResetPending
                );
                let winner_leg =
                    active_leg_for_asset(&h.env.portfolio_state(h.portfolios[0]), LIVE);
                assert!(
                    winner_leg.epoch_snap
                        < if reset_side == 0 {
                            asset.epoch_long
                        } else {
                            asset.epoch_short
                        }
                );

                let gain = h.claims.iter().sum::<u128>();
                let rank = |h: &History| {
                    let account = h.env.portfolio_state(h.portfolios[0]);
                    (
                        percolator::active_bitmap_count_ones(active_bitmap(&account)),
                        account.pnl.get().abs_diff(gain as i128),
                    )
                };
                let mut cleanup_calls = 0;
                for _ in 0..4 {
                    let before = rank(&h);
                    if before == (0, 0) {
                        break;
                    }
                    let peer = h.env.svm.get_account(&h.portfolios[1]);
                    h.env.svm.expire_blockhash();
                    let cu = h.env.crank(
                        h.portfolios[0],
                        ProgInstruction::PermissionlessCrank {
                            now_slot: h.slot,
                            observations: vec![],
                        },
                    );
                    h.calls += 1;
                    cleanup_calls += 1;
                    h.max_crank = h.max_crank.max(cu);
                    assert_cu_within("latent source prior-epoch cleanup", cu, CU_LIMIT);
                    assert!(
                        rank(&h) < before,
                        "cleanup reduces retained leg/economic debt rank"
                    );
                    assert_eq!(h.env.svm.get_account(&h.portfolios[1]), peer);
                    check_suffix(&h, &prefixes, [rank(&h).0 != 0, false], 0);
                }
                assert!(cleanup_calls > 0);
                assert_eq!(
                    rank(&h),
                    (0, 0),
                    "funded winner completes bounded public cleanup"
                );
                let full = h.source_claims(0);
                assert!(full.iter().all(|c| *c > 0));
                assert_eq!(full[latent], (UNITS + UNITS / 2) * BOUND_SCALE);
                assert_eq!(&full[..2 * LIVE], &history[..2 * LIVE]);
                assert_eq!(full, h.claims.map(|c| c * BOUND_SCALE));
                h.positions = [0; ASSETS];
                h.previous_claims = h.claims;
                h.previous_prices = h.prices;
                h.assert_accounting();
                let historical_gain = 2 * (0..LIVE).map(|a| 1 + (a % 3) as u128).sum::<u128>();
                assert_eq!(gain, historical_gain + 2 * UNITS + UNITS / 2);
                assert_eq!(h.payout([1, 0]), [CAPITAL + gain, CAPITAL - gain]);
                let frames = h.portfolios.map(|p| h.env.svm.get_account(&p));
                let cu = h.env.finalize_reset_side_with_cu(LIVE as u16, reset_side);
                h.calls += 1;
                max_lifecycle = max_lifecycle.max(cu);
                assert_cu_within("latent-reset side finalization", cu, CUSTODY_CU_LIMIT);
                assert_eq!(h.portfolios.map(|p| h.env.svm.get_account(&p)), frames);
                let group = h.env.market_state().1;
                assert_eq!((group.c_tot, group.vault, group.insurance), (0, 0, 0));
                assert_eq!(group.materialized_portfolio_count, 0);
                assert_eq!(
                    if reset_side == 0 {
                        group.assets[LIVE].mode_long
                    } else {
                        group.assets[LIVE].mode_short
                    },
                    SideModeV16::Normal
                );
                assert_eq!(h.env.svm.get_account(&h.env.mint), mint);
                worlds += 1;
                calls += h.calls;
                max_cleanup_calls = max_cleanup_calls.max(cleanup_calls);
                for (max, cu) in maxima.iter_mut().zip([
                    h.max_trade,
                    h.max_crank,
                    h.max_convert,
                    h.max_withdraw,
                    h.max_close,
                    max_reduce,
                    max_lifecycle,
                ]) {
                    *max = (*max).max(cu);
                }
            }
        }
    }
    assert_eq!(worlds, 16);
    println!("INV-028 latent reset exit: worlds={worlds}, history_calls={calls}, cleanup_calls<={max_cleanup_calls}/4, max CU trade/crank/convert/withdraw/close/reduce/lifecycle={maxima:?}");
}
