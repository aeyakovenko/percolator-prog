//! INV-028/020/045/053: historical source capacity composes with live Hybrid carry.
//! A full future-domain budget must preserve old claims while a fresh observation,
//! partitioned owner reduction and later health refresh determine the new entitlement.
//! Only public construction and instructions; Clock and external Pyth inputs are fixtures.

use super::*;
use crate::support::fuzz_model::{
    assert_current_certificate_matches_independent, assert_market_stock_census,
    assert_reservation_encumbrance_census, assert_source_credit_rates,
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const HYBRID: usize = ASSETS - 1;
const CAP_BPS: u64 = 150;
const OPEN_LOTS: i128 = 7;
const REDUCE_LOTS: i128 = 2;

fn census(h: &History) -> [bool; 2] {
    h.assert_accounting();
    let market = h.env.svm.get_account(&h.env.market).unwrap();
    let group = h.env.market_state().1;
    let accounts = h.portfolios.map(|p| h.env.portfolio_state(p));
    assert_market_stock_census(
        "Hybrid capacity/carry",
        &group,
        &market.data,
        &accounts,
        h.env.token_amount(h.env.vault) as u128,
    )
    .unwrap();
    assert_reservation_encumbrance_census("Hybrid capacity/carry", &group, &accounts).unwrap();
    assert_source_credit_rates("Hybrid capacity/carry", &group).unwrap();
    accounts.each_ref().map(|account| {
        assert_current_certificate_matches_independent("Hybrid capacity/carry", &group, account)
            .unwrap()
    })
}

fn assert_frontier(h: &History, start: u64, elapsed: u64, sign: i128) {
    let group = h.env.market_state().1;
    let profile = state::read_asset_oracle_profile(
        &h.env.svm.get_account(&h.env.market).unwrap().data,
        HYBRID,
    )
    .unwrap();
    let numerator = PRICE * CAP_BPS * elapsed;
    let expected = (PRICE as i128 + sign * (numerator / 10_000) as i128) as u64;
    assert_eq!(group.assets[HYBRID].effective_price, expected);
    assert_eq!(group.assets[HYBRID].slot_last, start + elapsed);
    assert_eq!(group.assets[HYBRID].fund_px_last, PRICE);
    assert_eq!(profile.mark_ewma_e6, expected);
    assert_eq!(
        profile.price_move_remainder_bps_num as u64,
        numerator % 10_000
    );
    if elapsed > 0 {
        let target = (PRICE as i128 + sign * 10) as u64;
        assert_eq!(group.assets[HYBRID].raw_oracle_target_price, target);
        assert_eq!(profile.oracle_target_price_e6, target);
        assert_eq!(profile.oracle_target_publish_time, 1_000 + elapsed as i64);
        assert_eq!(profile.last_good_oracle_slot, start + elapsed);
    }
    let account = h.env.portfolio_state(h.portfolios[0]);
    let occupied: BTreeSet<_> = account
        .source_domains
        .iter()
        .filter(|s| s.is_occupied())
        .map(|s| s.domain.get())
        .collect();
    assert!(
        (2 * HYBRID..=2 * HYBRID + usize::from(elapsed > 0)).contains(&occupied.len()),
        "only the current Hybrid source may materialize between account settlements"
    );
    let mut needed = occupied;
    needed.extend([2 * HYBRID as u32, 2 * HYBRID as u32 + 1]);
    assert_eq!(
        needed.len(),
        DOMAINS,
        "the live leg retains both future domains"
    );
}

fn crank_ix(h: &History, actor: usize, report: Pubkey) -> Instruction {
    Instruction {
        program_id: h.env.program_id,
        accounts: vec![
            AccountMeta::new(h.env.payer.pubkey(), true),
            AccountMeta::new(h.env.market, false),
            AccountMeta::new(h.portfolios[actor], false),
            AccountMeta::new_readonly(report, false),
        ],
        data: ProgInstruction::PermissionlessCrank {
            now_slot: h.slot,
            observations: crank_observations_with_accounts(HYBRID as u16, 1),
        }
        .encode(),
    }
}

fn reject_late_owner_mismatch(h: &mut History, report: Pubkey) -> u64 {
    h.env.svm.expire_blockhash();
    let prefix = crank_ix(h, 0, report);
    let control = Transaction::new_signed_with_payer(
        &[heap_ix(), cu_ix(), prefix.clone()],
        Some(&h.env.payer.pubkey()),
        &[&h.env.payer],
        h.env.svm.latest_blockhash(),
    );
    h.env
        .svm
        .simulate_transaction(control.into())
        .expect("the authenticated settlement prefix is independently admissible");
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
        &[heap_ix(), cu_ix(), prefix, suffix],
        Some(&h.env.payer.pubkey()),
        &[&h.env.payer, &h.owners[1]],
        h.env.svm.latest_blockhash(),
    );
    let mut keys: BTreeSet<_> = tx.message.account_keys.iter().copied().collect();
    keys.extend(h.portfolios);
    keys.extend(h.tokens);
    keys.extend([
        h.env.mint,
        h.env.admin.pubkey(),
        h.matcher.1,
        solana_sdk::sysvar::clock::ID,
    ]);
    let before: Vec<_> = keys
        .iter()
        .map(|key| (*key, h.env.svm.get_account(key)))
        .collect();
    let signatures = tx.message.header.num_required_signatures;
    let failure = h
        .env
        .svm
        .send_transaction(tx)
        .expect_err("owner binding is required");
    assert_eq!(
        failure.err,
        TransactionError::InstructionError(
            3,
            InstructionError::Custom(PercolatorError::Unauthorized as u32)
        )
    );
    for (key, mut account) in before {
        if key == h.env.payer.pubkey() {
            account.as_mut().unwrap().lamports -=
                u64::from(signatures) * FeeStructure::default().lamports_per_signature;
        }
        assert_eq!(
            h.env.svm.get_account(&key),
            account,
            "exact rollback for {key}"
        );
    }
    census(h);
    let cu = failure.meta.compute_units_consumed;
    assert_cu_within("Hybrid capacity/carry late rollback", cu, CU_LIMIT);
    cu
}

fn settle(
    h: &mut History,
    report: Pubkey,
    order: [usize; 2],
    start: u64,
    elapsed: u64,
    sign: i128,
) {
    let price = (PRICE as i128 + sign * (PRICE * CAP_BPS * elapsed / 10_000) as i128) as u64;
    h.previous_prices = h.prices;
    h.previous_claims = h.claims;
    let units = h.positions[HYBRID] / POS_SCALE as i128;
    let gain = units * (price as i128 - h.prices[HYBRID] as i128);
    assert!(gain > 0);
    h.prices[HYBRID] = price;
    h.claims[2 * HYBRID + usize::from(sign > 0)] += gain as u128;
    let expected_gain = h.claims.iter().sum::<u128>();
    for _ in 0..4 {
        for actor in order {
            let peer = h.env.svm.get_account(&h.portfolios[1 - actor]);
            h.env.svm.expire_blockhash();
            let ix = crank_ix(h, actor, report);
            let cu = send_raw_tx(&mut h.env.svm, &h.env.payer, ix, &[])
                .expect("bounded Hybrid settlement at full future-domain capacity");
            h.max_crank = h.max_crank.max(cu);
            h.calls += 1;
            assert_cu_within("Hybrid capacity/carry settlement", cu, CU_LIMIT);
            assert_eq!(h.env.svm.get_account(&h.portfolios[1 - actor]), peer);
            census(h);
            assert_frontier(h, start, elapsed, sign);
            if census(h) == [true; 2]
                && h.source_claims(0) == h.claims.map(|claim| claim * BOUND_SCALE)
                && h.env.portfolio_state(h.portfolios[1]).capital.get() == CAPITAL - expected_gain
            {
                h.previous_prices = h.prices;
                h.previous_claims = h.claims;
                return;
            }
        }
    }
    panic!("complete evidence must produce both full health certificates within eight cranks");
}

#[test]
fn v16_program_historical_capacity_preserves_hybrid_carry_health_and_owner_entitlement() {
    let mut worlds = 0;
    let mut checked_calls = 0;
    let mut peak_cu = 0;
    let mut endpoint = None;
    for sign in [-1i128, 1] {
        for reverse in [false, true] {
            for split in [false, true] {
                let mut h = History::new();
                let order = if reverse { [1, 0] } else { [0, 1] };
                set_test_clock(&mut h.env, 0, 1_000);
                let feed = [0xd3; 32];
                let initial = h
                    .env
                    .set_pyth_price_with_conf(&feed, PRICE as i64, -6, 0, 1_000);
                let cu = h
                    .env
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
                peak_cu = peak_cu.max(cu);
                census(&h);
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
                    census(&h);
                }
                let historical = h.source_claims(0);
                let historical_gain: u128 =
                    (0..HYBRID).map(|asset| 2 * (1 + asset as u128 % 3)).sum();
                assert_eq!(
                    historical.iter().sum::<u128>(),
                    historical_gain * BOUND_SCALE
                );
                assert_eq!(historical.iter().filter(|c| **c != 0).count(), DOMAINS - 2);
                let start = h.slot;
                set_test_clock(&mut h.env, start, 1_000);
                let cu = h.env.crank_with_oracle_tail(
                    h.portfolios[0],
                    ProgInstruction::PermissionlessCrank {
                        now_slot: start,
                        observations: crank_observations_with_accounts(HYBRID as u16, 1),
                    },
                    &[initial],
                );
                h.calls += 1;
                h.max_crank = h.max_crank.max(cu);
                census(&h);
                h.trade(
                    AccountResidualCounterTradePath::TradeNoCpi,
                    &[(HYBRID as u16, sign * OPEN_LOTS * POS_SCALE as i128, PRICE)],
                );
                census(&h);
                assert_frontier(&h, start, 0, sign);
                for elapsed in 1..=2 {
                    h.slot = start + elapsed;
                    set_test_clock(&mut h.env, h.slot, 1_000 + elapsed as i64);
                    let report = h.env.set_pyth_price_with_conf(
                        &feed,
                        (PRICE as i128 + sign * 10) as i64,
                        -6,
                        0,
                        1_000 + elapsed as i64,
                    );
                    peak_cu = peak_cu.max(reject_late_owner_mismatch(&mut h, report));
                    assert_frontier(&h, start, elapsed - 1, sign);
                    settle(&mut h, report, order, start, elapsed, sign);
                    assert_eq!(&h.source_claims(0)[..2 * HYBRID], &historical[..2 * HYBRID]);
                    if elapsed == 1 {
                        assert_eq!(PRICE * CAP_BPS % 10_000, 5_000, "nonzero carry witness");
                        let pieces = if split { vec![1, 1] } else { vec![REDUCE_LOTS] };
                        for units in pieces {
                            h.trade(
                                if split {
                                    AccountResidualCounterTradePath::TradeNoCpi
                                } else {
                                    AccountResidualCounterTradePath::BatchTradeNoCpi
                                },
                                &[(
                                    HYBRID as u16,
                                    -sign * units * POS_SCALE as i128,
                                    h.prices[HYBRID],
                                )],
                            );
                            census(&h);
                            assert_frontier(&h, start, elapsed, sign);
                            assert_eq!(
                                &h.source_claims(0)[..2 * HYBRID],
                                &historical[..2 * HYBRID]
                            );
                        }
                    }
                }
                assert_eq!(
                    census(&h),
                    [true; 2],
                    "later owner decision has complete recertification"
                );
                let hybrid_gain = OPEN_LOTS as u128 + 2 * (OPEN_LOTS - REDUCE_LOTS) as u128;
                assert_eq!(h.claims.iter().sum::<u128>(), historical_gain + hybrid_gain);
                h.trade(
                    AccountResidualCounterTradePath::TradeNoCpi,
                    &[(HYBRID as u16, -h.positions[HYBRID], h.prices[HYBRID])],
                );
                census(&h);
                let payouts = h.payout(order);
                assert_eq!(
                    payouts,
                    [
                        CAPITAL + historical_gain + hybrid_gain,
                        CAPITAL - historical_gain - hybrid_gain
                    ]
                );
                if let Some(expected) = endpoint {
                    assert_eq!(
                        payouts, expected,
                        "source orientation, order and partition preserve entitlements"
                    );
                } else {
                    endpoint = Some(payouts);
                }
                peak_cu = peak_cu
                    .max(h.max_trade)
                    .max(h.max_crank)
                    .max(h.max_convert)
                    .max(h.max_withdraw)
                    .max(h.max_close);
                checked_calls += h.calls;
                worlds += 1;
            }
        }
    }
    eprintln!("INV-028/020/045/053 Hybrid capacity/carry: {worlds} worlds, {checked_calls} accepted history calls, {} exact late rollbacks, peak CU={peak_cu}", 2 * worlds);
}
