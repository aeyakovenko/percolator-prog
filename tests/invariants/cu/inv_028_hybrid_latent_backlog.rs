//! Row 423 / INV-028: the final reserved domain survives Hybrid target reversal
//! and multi-chunk catch-up. Cadenced and deferred keepers must realize the same
//! input-derived owner claims, including the carry across the chunk boundary.
//! Public construction only; no liens, fees, funding, Recovery or generic closure.

use super::*;

const BACKLOG: u64 = 64;
const REMAINING_LOTS: i128 = OPEN_LOTS - REDUCE_LOTS;

#[derive(Default)]
struct Measurements {
    rollbacks: usize,
    continuations: usize,
    max_rollback: u64,
    max_packet: usize,
}

// A completed market/settlement prefix must be fully reversible, and the exact
// same instruction must remain usable by a keeper with no owner signature.
fn rollback_then_crank(h: &mut History, actor: usize, report: Pubkey, m: &mut Measurements) {
    h.env.svm.expire_blockhash();
    let prefix = crank_ix(h, actor, report);
    let suffix = Instruction {
        program_id: h.env.program_id,
        accounts: vec![
            AccountMeta::new(h.owners[1].pubkey(), true),
            AccountMeta::new(h.env.market, false),
            AccountMeta::new(h.portfolios[0], false),
            AccountMeta::new(h.tokens[1], false),
            AccountMeta::new(h.env.vault, false),
            AccountMeta::new_readonly(h.env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: h.env.withdraw_ix(h.portfolios[0], 1).encode(),
    };
    let tx = Transaction::new_signed_with_payer(
        &[heap_ix(), cu_ix(), prefix.clone(), suffix],
        Some(&h.env.payer.pubkey()),
        &[&h.env.payer, &h.owners[1]],
        h.env.svm.latest_blockhash(),
    );
    let bytes = bincode::serialized_size(&tx).unwrap() as usize;
    assert!(bytes <= solana_sdk::packet::PACKET_DATA_SIZE);
    m.max_packet = m.max_packet.max(bytes);
    let keys: BTreeSet<_> = tx
        .message
        .account_keys
        .iter()
        .copied()
        .chain(h.portfolios)
        .chain(h.tokens)
        .chain([
            h.env.mint,
            h.env.admin.pubkey(),
            h.owners[0].pubkey(),
            h.matcher.0,
            h.matcher.1,
            h.matcher.2,
            solana_sdk::sysvar::clock::ID,
        ])
        .collect();
    let before: Vec<_> = keys
        .iter()
        .map(|key| (*key, h.env.svm.get_account(key)))
        .collect();
    let signatures = tx.signatures.len() as u64;
    let failure = h.env.svm.send_transaction(tx).expect_err("owner binding");
    assert_eq!(
        failure.err,
        TransactionError::InstructionError(
            3,
            InstructionError::Custom(PercolatorError::Unauthorized as u32)
        )
    );
    assert_eq!(
        failure
            .meta
            .logs
            .iter()
            .filter(|line| **line == format!("Program {} success", h.env.program_id))
            .count(),
        1,
        "the crank prefix actually completed"
    );
    for (key, mut account) in before {
        if key == h.env.payer.pubkey() {
            account.as_mut().unwrap().lamports -=
                signatures * FeeStructure::default().lamports_per_signature;
        }
        assert_eq!(h.env.svm.get_account(&key), account, "rollback {key}");
    }
    assert_cu_within(
        "latent Hybrid backlog rollback",
        failure.meta.compute_units_consumed,
        CU_LIMIT,
    );
    m.max_rollback = m.max_rollback.max(failure.meta.compute_units_consumed);
    m.rollbacks += 1;

    let peer = h.env.svm.get_account(&h.portfolios[1 - actor]);
    let custody = [h.env.mint, h.env.vault, h.tokens[0], h.tokens[1], report];
    let frames = custody.map(|key| h.env.svm.get_account(&key));
    let tx = Transaction::new_signed_with_payer(
        &[heap_ix(), cu_ix(), prefix],
        Some(&h.env.payer.pubkey()),
        &[&h.env.payer],
        h.env.svm.latest_blockhash(),
    );
    assert_eq!(tx.signatures.len(), 1, "keeper-only continuation");
    let bytes = bincode::serialized_size(&tx).unwrap() as usize;
    assert!(bytes <= solana_sdk::packet::PACKET_DATA_SIZE);
    m.max_packet = m.max_packet.max(bytes);
    let cu = h
        .env
        .svm
        .send_transaction(tx)
        .expect("reserved source progress")
        .compute_units_consumed;
    assert_cu_within("latent Hybrid backlog progress", cu, CU_LIMIT);
    h.max_crank = h.max_crank.max(cu);
    h.calls += 1;
    m.continuations += 1;
    assert_eq!(h.env.svm.get_account(&h.portfolios[1 - actor]), peer);
    assert_eq!(custody.map(|key| h.env.svm.get_account(&key)), frames);
}

fn current(h: &History) -> [bool; 2] {
    let group = h.env.market_state().1;
    h.portfolios.map(|portfolio| {
        assert_current_certificate_matches_independent(
            "last Hybrid domain",
            &group,
            &h.env.portfolio_state(portfolio),
        )
        .unwrap()
    })
}

fn rank(h: &History) -> (u64, u128, usize) {
    let expected = h.claims.iter().sum::<u128>();
    let accounts = h.portfolios.map(|p| h.env.portfolio_state(p));
    (
        h.slot - h.env.market_state().1.assets[HYBRID].slot_last,
        accounts[0].pnl.get().abs_diff(expected as i128)
            + accounts[1].capital.get().abs_diff(CAPITAL - expected),
        current(h).iter().filter(|ready| !**ready).count(),
    )
}

fn frontier(
    h: &History,
    start: u64,
    elapsed: u64,
    sign: i128,
    anchor: u64,
    historical: &[u128; DOMAINS],
) {
    let group = h.env.market_state().1;
    let market = h.env.svm.get_account(&h.env.market).unwrap();
    let profile = state::read_asset_oracle_profile(&market.data, HYBRID).unwrap();
    let numerator = anchor * CAP_BPS * elapsed;
    let price = (i128::from(anchor) - sign * i128::from(numerator / 10_000)) as u64;
    assert_eq!(group.assets[HYBRID].effective_price, price);
    assert_eq!(group.assets[HYBRID].slot_last, start + elapsed);
    assert_eq!(group.assets[HYBRID].fund_px_last, anchor);
    assert_eq!(
        profile.price_move_remainder_bps_num as u64,
        numerator % 10_000
    );
    assert_eq!(profile.mark_ewma_e6, price);
    assert_eq!(
        profile.oracle_target_price_e6,
        (i128::from(anchor) - sign * 100) as u64
    );
    assert_eq!(profile.last_good_oracle_slot, h.slot);
    assert_eq!(
        profile.oracle_target_publish_time,
        1_002 + (h.slot - start) as i64
    );
    assert_eq!(&h.source_claims(0)[..2 * HYBRID], &historical[..2 * HYBRID]);
    let accounts = h.portfolios.map(|p| h.env.portfolio_state(p));
    assert_market_stock_census(
        "last Hybrid domain",
        &group,
        &market.data,
        &accounts,
        2 * CAPITAL,
    )
    .unwrap();
    assert_reservation_encumbrance_census("last Hybrid domain", &group, &accounts).unwrap();
    assert_source_credit_rates("last Hybrid domain", &group).unwrap();
    for (actor, account) in accounts.iter().enumerate() {
        assert_eq!(
            percolator::active_bitmap_count_ones(active_bitmap(account)),
            1
        );
        assert_eq!(
            active_leg_for_asset(account, HYBRID).basis_pos_q,
            -sign * REMAINING_LOTS * POS_SCALE as i128 * if actor == 0 { 1 } else { -1 }
        );
        let mut domains: BTreeSet<_> = account
            .source_domains
            .iter()
            .filter(|s| s.is_occupied())
            .map(|s| s.domain.get())
            .collect();
        domains.extend([2 * HYBRID as u32, 2 * HYBRID as u32 + 1]);
        assert_eq!(domains.len(), if actor == 0 { DOMAINS } else { 2 });
    }
    assert_eq!(
        group.assets[HYBRID].oi_eff_long_q,
        REMAINING_LOTS as u128 * POS_SCALE
    );
    assert_eq!(
        group.assets[HYBRID].oi_eff_short_q,
        REMAINING_LOTS as u128 * POS_SCALE
    );
}

#[test]
fn v16_program_last_latent_hybrid_domain_survives_reversal_and_chunked_backlog() {
    assert_eq!(BACKLOG, 2 * percolator::V16_MAX_ACCRUAL_PATH_STEPS as u64);
    let mut measurements = Measurements::default();
    let mut worlds = 0;
    let mut calls = 0;
    let mut maxima = [0; 5];
    for sign in [-1i128, 1] {
        let mut endpoint = None;
        for order in [[0, 1], [1, 0]] {
            for cadenced in [false, true] {
                let mut h = History::new();
                let feed = [0xd4; 32];
                set_test_clock(&mut h.env, 0, 1_000);
                let initial = h
                    .env
                    .set_pyth_price_with_conf(&feed, PRICE as i64, -6, 0, 1_000);
                h.env
                    .try_configure_hybrid_asset_with_conf_filter_cu(
                        HYBRID as u16,
                        1,
                        0,
                        [feed, [0; 32], [0; 32]],
                        &[initial],
                        0,
                        1_000,
                        0,
                        0,
                        100,
                        100,
                    )
                    .unwrap();
                for asset in 0..HYBRID as u16 {
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
                let historical_gain: u128 = (0..HYBRID).map(|a| 2 * (1 + a as u128 % 3)).sum();
                assert_eq!(historical.iter().filter(|c| **c > 0).count(), DOMAINS - 2);
                let first_start = h.slot;
                set_test_clock(&mut h.env, h.slot, 1_000);
                let ix = crank_ix(&h, 0, initial);
                send_raw_tx(&mut h.env.svm, &h.env.payer, ix, &[]).unwrap();
                h.trade(
                    AccountResidualCounterTradePath::TradeNoCpi,
                    &[(HYBRID as u16, sign * OPEN_LOTS * POS_SCALE as i128, PRICE)],
                );
                h.slot += 1;
                set_test_clock(&mut h.env, h.slot, 1_001);
                let first = h.env.set_pyth_price_with_conf(
                    &feed,
                    (PRICE as i128 + sign * 10) as i64,
                    -6,
                    0,
                    1_001,
                );
                settle(&mut h, first, order, first_start, 1, sign);
                assert_eq!(
                    h.source_claims(0).iter().filter(|c| **c > 0).count(),
                    DOMAINS - 1
                );
                let first_domain = 2 * HYBRID + usize::from(sign > 0);
                let last_domain = 2 * HYBRID + usize::from(sign < 0);
                assert_eq!(h.claims[first_domain], OPEN_LOTS as u128);
                assert_eq!(h.claims[last_domain], 0);
                h.trade(
                    AccountResidualCounterTradePath::BatchTradeNoCpi,
                    &[(
                        HYBRID as u16,
                        -sign * (OPEN_LOTS + REMAINING_LOTS) * POS_SCALE as i128,
                        h.prices[HYBRID],
                    )],
                );
                let start = h.slot;
                let anchor = (PRICE as i128 + sign) as u64;
                let target = (i128::from(anchor) - sign * 100) as i64;
                let profile_before = state::read_asset_oracle_profile(
                    &h.env.svm.get_account(&h.env.market).unwrap().data,
                    HYBRID,
                )
                .unwrap();
                assert_eq!(profile_before.price_move_remainder_bps_num, 5_000);
                set_test_clock(&mut h.env, h.slot, 1_002);
                let reversal = h.env.set_pyth_price_with_conf(&feed, target, -6, 0, 1_002);
                rollback_then_crank(&mut h, order[0], reversal, &mut measurements);
                frontier(&h, start, 0, sign, anchor, &historical);
                assert_eq!(
                    h.source_claims(0).iter().filter(|c| **c > 0).count(),
                    DOMAINS - 1
                );
                census(&h);

                for elapsed in if cadenced {
                    vec![BACKLOG / 2, BACKLOG]
                } else {
                    vec![BACKLOG]
                } {
                    h.slot = start + elapsed;
                    set_test_clock(&mut h.env, h.slot, 1_002 + elapsed as i64);
                    let report = h.env.set_pyth_price_with_conf(
                        &feed,
                        target,
                        -6,
                        0,
                        1_002 + elapsed as i64,
                    );
                    let distance = anchor * CAP_BPS * elapsed / 10_000;
                    h.previous_prices = h.prices;
                    h.previous_claims = h.claims;
                    h.prices[HYBRID] = (i128::from(anchor) - sign * i128::from(distance)) as u64;
                    h.claims[last_domain] = REMAINING_LOTS as u128 * u128::from(distance);
                    for _ in 0..4 {
                        for actor in order {
                            let before_rank = rank(&h);
                            if before_rank == (0, 0, 0) {
                                break;
                            }
                            if before_rank.0 == 0 && current(&h)[actor] {
                                continue;
                            }
                            let before_slot = h.env.market_state().1.assets[HYBRID].slot_last;
                            let portfolio_frames = h.portfolios.map(|p| h.env.svm.get_account(&p));
                            rollback_then_crank(&mut h, actor, report, &mut measurements);
                            assert!(rank(&h) < before_rank, "each committed continuation strictly reduces slot/economic/certificate work");
                            let committed = h.env.market_state().1.assets[HYBRID].slot_last - start;
                            frontier(&h, start, committed, sign, anchor, &historical);
                            if before_rank.0 > BACKLOG / 2 {
                                assert_eq!(committed + start - before_slot, BACKLOG / 2);
                                assert_eq!(
                                    h.portfolios.map(|p| h.env.svm.get_account(&p)),
                                    portfolio_frames
                                );
                                assert_eq!(
                                    h.source_claims(0)[last_domain],
                                    0,
                                    "last domain remains latent during market-only catch-up"
                                );
                                assert_ne!(anchor * CAP_BPS * committed % 10_000, 0);
                            }
                        }
                        if rank(&h) == (0, 0, 0) {
                            break;
                        }
                    }
                    assert_eq!(
                        rank(&h),
                        (0, 0, 0),
                        "at most eight keeper calls finish this checkpoint"
                    );
                    assert_eq!(h.source_claims(0), h.claims.map(|c| c * BOUND_SCALE));
                    assert!(h.source_claims(0).iter().all(|c| *c > 0));
                    h.previous_prices = h.prices;
                    h.previous_claims = h.claims;
                    assert_eq!(census(&h), [true; 2]);
                }
                let gain = historical_gain
                    + OPEN_LOTS as u128
                    + REMAINING_LOTS as u128 * u128::from(anchor * CAP_BPS * BACKLOG / 10_000);
                assert_eq!(h.claims.iter().sum::<u128>(), gain);
                h.trade(
                    AccountResidualCounterTradePath::TradeNoCpi,
                    &[(HYBRID as u16, -h.positions[HYBRID], h.prices[HYBRID])],
                );
                let payouts = h.payout([order[1], order[0]]);
                assert_eq!(payouts, [CAPITAL + gain, CAPITAL - gain]);
                if let Some(expected) = endpoint {
                    assert_eq!(payouts, expected);
                }
                endpoint = Some(payouts);
                println!("last Hybrid domain: sign={sign}, order={order:?}, cadenced={cadenced}, gain={gain}, payouts={payouts:?}");
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
    assert_eq!(worlds, 8);
    assert_eq!(measurements.rollbacks, measurements.continuations);
    println!("INV-028 latent Hybrid backlog: worlds={worlds}, accepted history calls={calls}, ranked/reset continuations={}, exact rollbacks={}, max CU trade/crank/convert/withdraw/close={maxima:?}, rollback CU={}, max packet={}", measurements.continuations, measurements.rollbacks, measurements.max_rollback, measurements.max_packet);
}
