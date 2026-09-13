//! INV-028 / row 423: future settlement capacity survives automatic matcher revocation
//! and rollback of a renewed CPI episode. A bilateral partial reduction must retain
//! latent domains; an unchanged signed renewal/fill retry must still settle the last
//! domain and permit funded exit after another revocation. Public instructions only.
//!
//! Limits: one active asset, 26 detached fully backed claims, integral AuthMark moves,
//! zero fees/funding, single/one-leg-batch CPI, and cooperative live owner exit. This
//! does not close arbitrary resource admission, maximum active shapes or terminal,
//! Recovery, expiry, lien, asset-reuse and permissionless-exit products.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census, assert_source_credit_rates,
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

fn checkpoint(h: &History, expected: [u128; DOMAINS]) {
    h.assert_accounting();
    assert_eq!(h.source_claims(0), expected);
    let market = h.env.svm.get_account(&h.env.market).unwrap();
    let group = h.env.market_state().1;
    let accounts = h.portfolios.map(|p| h.env.portfolio_state(p));
    assert_market_stock_census(
        "reserved domain renewal",
        &group,
        &market.data,
        &accounts,
        h.env.token_amount(h.env.vault) as u128,
    )
    .unwrap();
    assert_reservation_encumbrance_census("reserved domain renewal", &group, &accounts).unwrap();
    assert_source_credit_rates("reserved domain renewal", &group).unwrap();
    let mut resources: BTreeSet<_> = expected
        .iter()
        .enumerate()
        .filter_map(|(domain, claim)| (*claim != 0).then_some(domain))
        .collect();
    for (asset, quantity) in h.positions.iter().enumerate() {
        if *quantity != 0 {
            resources.extend([2 * asset, 2 * asset + 1]);
        }
    }
    assert_eq!(resources.len(), DOMAINS, "reserved capacity stays full");
}

fn assert_revoked(h: &History, sequence: u64, previous_epoch: u64) {
    assert_eq!(h.env.portfolio_matcher_config(h.portfolios[1]).enabled(), 0);
    assert_eq!(h.env.portfolio_matcher_expiry(h.portfolios[1]), 0);
    assert_eq!(h.env.portfolio_matcher_sequence(h.portfolios[1]), sequence);
    assert!(h.env.portfolio_position_epoch(h.portfolios[1]) > previous_epoch);
}

fn renew_flip_rollback_retry(h: &mut History, batch: bool, asset: u16, size_q: i128) -> [u64; 3] {
    let sequence = h.env.portfolio_matcher_sequence(h.portfolios[1]);
    assert_eq!(h.env.portfolio_matcher_config(h.portfolios[1]).enabled(), 0);
    let grant = Instruction {
        program_id: h.env.program_id,
        accounts: vec![
            AccountMeta::new(h.owners[1].pubkey(), true),
            AccountMeta::new_readonly(h.env.market, false),
            AccountMeta::new(h.portfolios[1], false),
            AccountMeta::new_readonly(h.matcher.0, false),
            AccountMeta::new_readonly(h.matcher.1, false),
            AccountMeta::new_readonly(h.matcher.2, false),
        ],
        data: ProgInstruction::SetMatcherConfig {
            portfolio_id: h.env.portfolio_id(h.portfolios[1]),
            expected_sequence: sequence,
            enabled: 1,
            trade_fee_cap_bps: 0,
            expiry_slot: u64::MAX,
        }
        .encode(),
    };
    let mut fill = if batch {
        h.env.batch_trade_cpi_ix(
            h.portfolios[0],
            h.portfolios[1],
            vec![BatchTradeCpiLeg {
                asset_index: asset,
                market_id: h.env.asset_market_id(asset),
                size_q,
                fee_bps: 0,
                limit_price: 0,
            }],
        )
    } else {
        h.env
            .trade_cpi_ix(h.portfolios[0], h.portfolios[1], asset, size_q, 0, 0)
    };
    match &mut fill {
        ProgInstruction::TradeCpi {
            account_b_matcher_sequence,
            ..
        }
        | ProgInstruction::BatchTradeCpi {
            account_b_matcher_sequence,
            ..
        } => *account_b_matcher_sequence = sequence.checked_add(1).unwrap(),
        _ => unreachable!(),
    }
    let fill = Instruction {
        program_id: h.env.program_id,
        accounts: vec![
            AccountMeta::new(h.owners[0].pubkey(), true),
            AccountMeta::new(h.env.market, false),
            AccountMeta::new(h.portfolios[0], false),
            AccountMeta::new(h.portfolios[1], false),
            AccountMeta::new_readonly(h.matcher.0, false),
            AccountMeta::new(h.matcher.1, false),
            AccountMeta::new_readonly(h.matcher.2, false),
        ],
        data: fill.encode(),
    };
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
    h.env.svm.expire_blockhash();
    let mut ixs = vec![heap_ix(), cu_ix(), grant, fill];
    let sign = |ixs: &[Instruction]| {
        Transaction::new_signed_with_payer(
            ixs,
            Some(&h.env.payer.pubkey()),
            &[&h.env.payer, &h.owners[0], &h.owners[1]],
            h.env.svm.latest_blockhash(),
        )
    };
    let retry = sign(&ixs);
    let retained_bytes = bincode::serialize(&retry).unwrap();
    ixs.push(suffix);
    let rejected = sign(&ixs);
    let packet = bincode::serialized_size(&rejected).unwrap();
    assert!(packet <= 1232);
    let keys: BTreeSet<_> = rejected
        .message
        .account_keys
        .iter()
        .copied()
        .chain(h.tokens)
        .chain([
            h.env.mint,
            h.env.admin.pubkey(),
            solana_sdk::sysvar::clock::ID,
        ])
        .collect();
    let frame = |h: &History| {
        keys.iter()
            .map(|key| (*key, h.env.svm.get_account(key)))
            .collect::<Vec<_>>()
    };
    let before = frame(h);
    let expected = h.source_claims(0);
    let simulation = h
        .env
        .svm
        .simulate_transaction(retry.clone().into())
        .expect("renewal and cross-zero fill are independently admissible at 27+1 domains");
    assert_cu_within(
        "reserved domain renewal simulation",
        simulation.compute_units_consumed,
        CU_LIMIT,
    );
    assert_eq!(frame(h), before, "simulation cannot write the fixture");
    let failure = h
        .env
        .svm
        .send_transaction(rejected)
        .expect_err("wrong owner suffix rolls back the successful renewal and CPI fill");
    assert_eq!(
        failure.err,
        TransactionError::InstructionError(
            4,
            InstructionError::Custom(PercolatorError::Unauthorized as u32)
        )
    );
    let successes = |logs: &[String], program: Pubkey| {
        logs.iter()
            .filter(|line| **line == format!("Program {program} success"))
            .count()
    };
    assert_eq!(successes(&failure.meta.logs, h.env.program_id), 2);
    assert_eq!(successes(&failure.meta.logs, h.matcher.0), 1);
    let fee = u64::from(retry.message.header.num_required_signatures)
        * FeeStructure::default().lamports_per_signature;
    for (key, mut account) in before {
        if key == h.env.payer.pubkey() {
            account.as_mut().unwrap().lamports -= fee;
        }
        assert_eq!(
            h.env.svm.get_account(&key),
            account,
            "full Account rollback {key}"
        );
    }
    checkpoint(h, expected);
    assert_eq!(h.env.portfolio_matcher_config(h.portfolios[1]).enabled(), 0);
    assert_eq!(h.env.portfolio_matcher_sequence(h.portfolios[1]), sequence);

    // Deliver the already signed prefix with the same bytes, guards and blockhash.
    let before_retry = frame(h);
    assert_eq!(bincode::serialize(&retry).unwrap(), retained_bytes);
    retry.verify().unwrap();
    let committed = h
        .env
        .svm
        .send_transaction(retry)
        .expect("unchanged reserved-episode retry");
    assert_eq!(successes(&committed.logs, h.env.program_id), 2);
    assert_eq!(successes(&committed.logs, h.matcher.0), 1);
    for (key, mut account) in before_retry {
        if key == h.env.payer.pubkey() {
            account.as_mut().unwrap().lamports -= fee;
        } else if [h.env.market, h.portfolios[0], h.portfolios[1], h.matcher.1].contains(&key) {
            continue;
        }
        assert_eq!(
            h.env.svm.get_account(&key),
            account,
            "retry Account frame {key}"
        );
    }
    h.positions[asset as usize] += size_q;
    h.calls += 1;
    h.max_trade = h.max_trade.max(committed.compute_units_consumed);
    checkpoint(h, expected);
    assert_eq!(
        h.env.portfolio_matcher_sequence(h.portfolios[1]),
        sequence + 1
    );
    assert_eq!(h.env.portfolio_matcher_config(h.portfolios[1]).enabled(), 1);
    for cu in [
        failure.meta.compute_units_consumed,
        committed.compute_units_consumed,
    ] {
        assert_cu_within("reserved domain renewal delivery", cu, CU_LIMIT);
    }
    [
        failure.meta.compute_units_consumed,
        committed.compute_units_consumed,
        packet,
    ]
}

#[test]
fn v16_program_reserved_domains_survive_revocation_and_renewed_cpi_rollback() {
    assert_certified_engine_pin("INV-028 reserved domains through matcher renewal rollback");
    let asset = (ASSETS - 1) as u16;
    let bilateral = AccountResidualCounterTradePath::TradeNoCpi;
    let mut worlds = 0;
    let mut calls = 0;
    let mut maxima = [0; 5];
    let mut renewal_maxima = [0; 3];
    for batch in [false, true] {
        for direction in [-1i128, 1] {
            for order in [[0, 1], [1, 0]] {
                let mut h = History::new();
                for old_asset in 0..asset {
                    let q = (1 + i128::from(old_asset % 3)) * POS_SCALE as i128;
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
                let historical = expected;
                assert_eq!(h.source_claims(0), historical);
                assert_eq!(historical.iter().filter(|claim| **claim > 0).count(), 26);
                let identities = h.portfolios.map(|p| h.env.portfolio_id(p));
                let route = if batch {
                    AccountResidualCounterTradePath::BatchTradeCpi
                } else {
                    AccountResidualCounterTradePath::TradeCpi
                };
                let lot = direction * POS_SCALE as i128;
                h.trade(route, &[(asset, 8 * lot, PRICE)]);
                checkpoint(&h, expected);
                assert_eq!(h.env.portfolio_matcher_config(h.portfolios[1]).enabled(), 1);
                let sequence = h.env.portfolio_matcher_sequence(h.portfolios[1]);
                let epoch = h.env.portfolio_position_epoch(h.portfolios[1]);
                h.trade(bilateral, &[(asset, -3 * lot, PRICE)]);
                assert_revoked(&h, sequence, epoch);
                checkpoint(&h, expected);
                let mark = (PRICE as i128 + direction) as u64;
                h.mark_and_settle(&[(asset, mark)], order);
                let first = 2 * asset as usize + usize::from(direction > 0);
                expected[first] = 5 * BOUND_SCALE;
                checkpoint(&h, expected);
                assert_eq!(expected.iter().filter(|claim| **claim > 0).count(), 27);
                let evidence = renew_flip_rollback_retry(&mut h, batch, asset, -9 * lot);
                for (maximum, value) in renewal_maxima.iter_mut().zip(evidence) {
                    *maximum = (*maximum).max(value);
                }
                assert_eq!(h.positions[asset as usize], -4 * lot);
                let epoch = h.env.portfolio_position_epoch(h.portfolios[1]);
                h.trade(bilateral, &[(asset, lot, mark)]);
                assert_revoked(&h, sequence + 1, epoch);
                checkpoint(&h, expected);

                // The last reserved domain must materialize even after renewed consent is revoked.
                h.mark_and_settle(&[(asset, PRICE)], [order[1], order[0]]);
                expected[first ^ 1] = 3 * BOUND_SCALE;
                checkpoint(&h, expected);
                assert!(expected.iter().all(|claim| *claim > 0));
                assert_eq!(
                    &expected[..2 * asset as usize],
                    &historical[..2 * asset as usize]
                );
                assert_eq!(
                    h.max_convert, 0,
                    "no historical reclamation funds settlement"
                );
                assert_eq!(h.env.portfolio_matcher_config(h.portfolios[1]).enabled(), 0);
                h.trade(bilateral, &[(asset, 3 * lot, PRICE)]);
                assert_eq!(h.positions, [0; ASSETS]);
                checkpoint(&h, expected);
                assert_eq!(h.portfolios.map(|p| h.env.portfolio_id(p)), identities);
                assert_eq!(expected.iter().sum::<u128>(), 58 * BOUND_SCALE);
                assert_eq!(h.payout([order[1], order[0]]), [CAPITAL + 58, CAPITAL - 58]);
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
    println!("INV-028 reserved renewal: worlds={worlds}, successful post-funding calls={calls}, exact rollbacks={worlds}, unchanged retries={worlds}, max CU trade/crank/convert/withdraw/close={maxima:?}, max rollback CU/retry CU/packet={renewal_maxima:?}");
}
