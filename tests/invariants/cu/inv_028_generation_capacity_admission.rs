//! INV-028: historical sources and replacement-generation admission share an exit budget.
//! Public position episodes, optional used-slot reuse, admission rollback/retry and
//! full domain materialization precede ranked permissionless terminal payouts.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census, assert_source_credit_rates,
};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

#[derive(Default)]
struct Measurements {
    transactions: usize,
    rollbacks: usize,
    restored_prefixes: usize,
    terminal_calls: usize,
    max_cu: u64,
    max_packet: usize,
}

impl Measurements {
    #[track_caller]
    fn submit(
        &mut self,
        h: &mut History,
        tx: Transaction,
        rejection: Option<(u8, PercolatorError)>,
    ) {
        let bytes = bincode::serialize(&tx).unwrap().len();
        assert!(bytes <= 1232);
        self.max_packet = self.max_packet.max(bytes);
        let keys: BTreeSet<_> = tx
            .message
            .account_keys
            .iter()
            .copied()
            .chain(h.portfolios)
            .chain(h.tokens)
            .chain([
                h.env.market,
                h.env.vault,
                h.env.mint,
                h.env.admin.pubkey(),
                h.owners[0].pubkey(),
                h.owners[1].pubkey(),
                h.matcher.0,
                h.matcher.1,
                h.matcher.2,
            ])
            .collect();
        let before: Vec<_> = keys
            .iter()
            .map(|key| (*key, h.env.svm.get_account(key)))
            .collect();
        let mut payer = h.env.svm.get_account(&h.env.payer.pubkey()).unwrap();
        payer.lamports -=
            tx.signatures.len() as u64 * FeeStructure::default().lamports_per_signature;
        let result = h.env.svm.send_transaction(tx);
        let cu = if let Some((index, error)) = rejection {
            let failure = result.expect_err("specified admission/continuation guard must reject");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(index, InstructionError::Custom(error as u32)),
                "{failure:?}"
            );
            assert_eq!(
                failure
                    .meta
                    .logs
                    .iter()
                    .filter(|line| **line == format!("Program {} success", h.env.program_id))
                    .count(),
                usize::from(index - 2),
                "every intended prefix instruction must actually succeed"
            );
            for (key, account) in before {
                if key != h.env.payer.pubkey() {
                    assert_eq!(
                        h.env.svm.get_account(&key),
                        account,
                        "Account rollback {key}"
                    );
                }
            }
            self.rollbacks += 1;
            self.restored_prefixes += usize::from(index - 2);
            failure.meta.compute_units_consumed
        } else {
            result
                .expect("reserved resources support this public step")
                .compute_units_consumed
        };
        assert_eq!(h.env.svm.get_account(&h.env.payer.pubkey()).unwrap(), payer);
        assert_cu_within("generation capacity admission/exit", cu, CU_LIMIT);
        self.max_cu = self.max_cu.max(cu);
        self.transactions += 1;
    }
}

fn instruction(h: &History, data: ProgInstruction, accounts: Vec<AccountMeta>) -> Instruction {
    Instruction {
        program_id: h.env.program_id,
        accounts,
        data: data.encode(),
    }
}

fn signed(h: &History, instructions: &[Instruction], actors: &[usize]) -> Transaction {
    let mut signers = vec![&h.env.payer];
    signers.extend(actors.iter().map(|&actor| &h.owners[actor]));
    Transaction::new_signed_with_payer(
        &[&[heap_ix(), cu_ix()][..], instructions].concat(),
        Some(&h.env.payer.pubkey()),
        &signers,
        h.env.svm.latest_blockhash(),
    )
}

fn trade(h: &History, legs: &[(u16, i128, u64)], batch: bool) -> Instruction {
    let data = if batch {
        h.env.batch_trade_no_cpi_ix(
            h.portfolios[0],
            h.portfolios[1],
            legs.iter()
                .map(|&(asset_index, size_q, exec_price)| BatchTradeLeg {
                    asset_index,
                    market_id: h.env.asset_market_id(asset_index),
                    size_q,
                    exec_price,
                    fee_bps: 0,
                })
                .collect(),
        )
    } else {
        assert_eq!(legs.len(), 1);
        let (asset, q, price) = legs[0];
        h.env
            .trade_no_cpi_ix(h.portfolios[0], h.portfolios[1], asset, q, price, 0)
    };
    instruction(
        h,
        data,
        vec![
            AccountMeta::new(h.owners[0].pubkey(), true),
            AccountMeta::new(h.owners[1].pubkey(), true),
            AccountMeta::new(h.env.market, false),
            AccountMeta::new(h.portfolios[0], false),
            AccountMeta::new(h.portfolios[1], false),
        ],
    )
}

fn bad_owner(h: &History) -> Instruction {
    instruction(
        h,
        h.env.withdraw_ix(h.portfolios[0], 1),
        vec![
            AccountMeta::new(h.owners[1].pubkey(), true),
            AccountMeta::new(h.env.market, false),
            AccountMeta::new(h.portfolios[0], false),
            AccountMeta::new(h.tokens[1], false),
            AccountMeta::new(h.env.vault, false),
            AccountMeta::new_readonly(h.env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
    )
}

fn rollback_retry(h: &mut History, m: &mut Measurements, prefix: Instruction, actors: &[usize]) {
    h.env.svm.expire_blockhash();
    let retry = signed(h, &[prefix.clone()], actors);
    let wire = bincode::serialize(&retry).unwrap();
    let mut rejected_actors = actors.to_vec();
    if !rejected_actors.contains(&1) {
        rejected_actors.push(1);
    }
    let rejected = signed(h, &[prefix, bad_owner(h)], &rejected_actors);
    let error = if h.env.market_state().1.mode == MarketModeV16::Resolved {
        PercolatorError::EngineLockActive
    } else {
        PercolatorError::Unauthorized
    };
    m.submit(h, rejected, Some((3, error)));
    assert_eq!(bincode::serialize(&retry).unwrap(), wire);
    m.submit(h, retry, None);
}

fn resources(h: &History, expected: [u128; DOMAINS], generations: &[u64; ASSETS]) {
    h.assert_accounting();
    assert_eq!(h.source_claims(0), expected);
    let group = h.env.market_state().1;
    let market = h.env.svm.get_account(&h.env.market).unwrap();
    let accounts = h.portfolios.map(|p| h.env.portfolio_state(p));
    assert_market_stock_census(
        "generation capacity",
        &group,
        &market.data,
        &accounts,
        h.env.token_amount(h.env.vault) as u128,
    )
    .unwrap();
    assert_reservation_encumbrance_census("generation capacity", &group, &accounts).unwrap();
    assert_source_credit_rates("generation capacity", &group).unwrap();
    let mut union: BTreeSet<_> = expected
        .iter()
        .enumerate()
        .filter_map(|(domain, claim)| (*claim > 0).then_some(domain))
        .collect();
    for (asset, &q) in h.positions.iter().enumerate() {
        if q != 0 {
            union.extend([2 * asset, 2 * asset + 1]);
        }
        assert_eq!(group.assets[asset].market_id, generations[asset]);
    }
    assert_eq!(
        union.len(),
        DOMAINS,
        "input-derived admission exhausts exactly the domain budget"
    );
    for source in accounts[0]
        .source_domains
        .iter()
        .filter(|s| s.is_occupied())
    {
        assert_eq!(
            source.source_claim_market_id.get(),
            generations[source.domain.get() as usize / 2]
        );
    }
    for domain in [DOMAINS, DOMAINS + 1] {
        assert_eq!(
            group.source_credit[domain],
            percolator::SourceCreditStateV16::EMPTY
        );
        assert_eq!(
            group.source_backing_buckets[domain].fresh_unliened_backing_num,
            0
        );
        assert_eq!(group.insurance_domain_budget[domain], 0);
    }
}

fn terminal(h: &mut History, m: &mut Measurements, order: [usize; 2]) {
    assert_eq!(h.positions, [0; ASSETS]);
    let expected = h.claims.map(|c| c * BOUND_SCALE);
    let gain = h.claims.iter().sum::<u128>();
    let entitlement = [CAPITAL + gain, CAPITAL - gain];
    let mint = h.env.svm.get_account(&h.env.mint);
    let portfolios = h.portfolios.map(|p| h.env.svm.get_account(&p));
    let cu = h.env.resolve();
    assert_cu_within("generation capacity resolution", cu, CU_LIMIT);
    m.max_cu = m.max_cu.max(cu);
    assert_eq!(h.portfolios.map(|p| h.env.svm.get_account(&p)), portfolios);
    assert_eq!(h.env.market_state().1.mode, MarketModeV16::Resolved);
    h.slot += 5;
    h.env.svm.warp_to_slot(h.slot);
    for actor in order {
        let rank = |h: &History| {
            h.source_claims(actor).iter().filter(|c| **c > 0).count()
                + usize::from((h.env.token_amount(h.tokens[actor]) as u128) < entitlement[actor])
        };
        for step in 0..=DOMAINS {
            if rank(h) == 0 {
                break;
            }
            let before_rank = rank(h);
            let peer = h.env.svm.get_account(&h.portfolios[1 - actor]);
            let peer_token = h.env.svm.get_account(&h.tokens[1 - actor]);
            let close = instruction(
                h,
                ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                },
                vec![
                    AccountMeta::new_readonly(h.owners[actor].pubkey(), false),
                    AccountMeta::new(h.env.market, false),
                    AccountMeta::new(h.portfolios[actor], false),
                    AccountMeta::new(h.tokens[actor], false),
                    AccountMeta::new(h.env.vault, false),
                    AccountMeta::new_readonly(h.env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
            );
            if actor == 1 || step == 0 {
                rollback_retry(h, m, close, &[]);
            } else {
                h.env.svm.expire_blockhash();
                let tx = signed(h, &[close], &[]);
                assert_eq!(tx.signatures.len(), 1);
                m.submit(h, tx, None);
            }
            m.terminal_calls += 1;
            assert!(
                rank(h) < before_rank,
                "each accepted terminal call decreases economic rank"
            );
            assert_eq!(h.env.svm.get_account(&h.portfolios[1 - actor]), peer);
            assert_eq!(h.env.svm.get_account(&h.tokens[1 - actor]), peer_token);
            assert_eq!(h.env.svm.get_account(&h.env.mint), mint);
            let group = h.env.market_state().1;
            let market = h.env.svm.get_account(&h.env.market).unwrap();
            let accounts = h.portfolios.map(|p| h.env.portfolio_state(p));
            assert_market_stock_census(
                "generation terminal",
                &group,
                &market.data,
                &accounts,
                h.env.token_amount(h.env.vault) as u128,
            )
            .unwrap();
            assert_reservation_encumbrance_census("generation terminal", &group, &accounts)
                .unwrap();
            assert_source_credit_rates("generation terminal", &group).unwrap();
            let remaining = h.source_claims(0);
            for domain in 0..DOMAINS {
                assert!(remaining[domain] == 0 || remaining[domain] == expected[domain]);
                assert_eq!(
                    group.source_credit[domain].positive_claim_bound_num,
                    remaining[domain]
                );
                assert_eq!(
                    group.source_backing_buckets[domain].fresh_unliened_backing_num,
                    remaining[domain]
                );
            }
            let paid = h.tokens.map(|t| h.env.token_amount(t) as u128);
            for owner in 0..2 {
                assert!(paid[owner] == 0 || paid[owner] == entitlement[owner]);
            }
            assert_eq!(group.vault, 2 * CAPITAL - paid.iter().sum::<u128>());
            assert_eq!(group.insurance, 0);
        }
        assert_eq!(
            rank(h),
            0,
            "terminal disposition is bounded by source count plus payout"
        );
        assert!(resolved_portfolio_is_terminal(&h.env, h.portfolios[actor]));
    }
    for actor in order {
        assert_eq!(
            h.env.token_amount(h.tokens[actor]) as u128,
            entitlement[actor]
        );
        h.env.svm.expire_blockhash();
        let cu = h
            .env
            .close_portfolio_with_cu(&h.owners[actor], h.portfolios[actor]);
        assert_cu_within("generation capacity deletion", cu, CUSTODY_CU_LIMIT);
        m.max_cu = m.max_cu.max(cu);
    }
    let group = h.env.market_state().1;
    assert_eq!(
        (
            group.vault,
            group.c_tot,
            group.insurance,
            group.materialized_portfolio_count
        ),
        (0, 0, 0, 0)
    );
    assert!(group
        .assets
        .iter()
        .all(|a| a.oi_eff_long_q == 0 && a.oi_eff_short_q == 0));
}

#[test]
fn v16_program_used_generation_admission_reserves_latent_capacity_through_exact_exit() {
    // This witness is revalidated separately from the older global certification roster.
    assert!(include_str!("../../../Cargo.lock").contains(
        "git+https://github.com/aeyakovenko/percolator?rev=4db11a8cb0053815e23a35d3a7d3edc265d8d866#\
         4db11a8cb0053815e23a35d3a7d3edc265d8d866"
    ));
    let mut m = Measurements::default();
    let mut worlds = 0;
    let mut history_calls = 0;
    let mut reused = 0;
    let mut capacity_prefix_retries = 0;
    for historical_assets in [ASSETS - 3, ASSETS - 2, ASSETS - 1] {
        for direction in [-1i128, 1] {
            for reuse in [false, true] {
                for batch in [false, true] {
                    let mut h = History::with_market_capacity(ASSETS + 1);
                    h.env.configure_permissionless_resolve_with_cu(1_000, 5);
                    let route = if batch {
                        AccountResidualCounterTradePath::BatchTradeNoCpi
                    } else {
                        AccountResidualCounterTradePath::TradeNoCpi
                    };
                    let order = if direction > 0 { [0, 1] } else { [1, 0] };
                    let historical = std::array::from_fn(|d| {
                        if d / 2 < historical_assets {
                            (1 + (d / 2 % 3) as u128) * BOUND_SCALE
                        } else {
                            0
                        }
                    });
                    let asset = (ASSETS - 1) as u16;
                    let mut generations = std::array::from_fn(|a| h.env.asset_market_id(a as u16));
                    let ids = h.portfolios.map(|p| h.env.portfolio_id(p));
                    h.trade(route, &[(asset, direction * POS_SCALE as i128, PRICE)]);
                    h.trade(route, &[(asset, -direction * POS_SCALE as i128, PRICE)]);
                    assert_eq!(h.source_claims(0), [0; DOMAINS]);
                    let stale = trade(&h, &[(asset, direction * POS_SCALE as i128, PRICE)], batch);
                    if reuse {
                        let frame = [
                            h.portfolios[0],
                            h.portfolios[1],
                            h.env.vault,
                            h.env.mint,
                            h.tokens[0],
                            h.tokens[1],
                        ]
                        .map(|key| h.env.svm.get_account(&key));
                        h.slot += 1;
                        h.env.svm.warp_to_slot(h.slot);
                        let cu = h.env.update_asset_lifecycle_as_admin_with_cu(
                            processor::ASSET_ACTION_RETIRE,
                            asset,
                            h.slot,
                            0,
                        );
                        assert_cu_within("used empty generation retirement", cu, CU_LIMIT);
                        assert_eq!(
                            h.env.market_state().1.assets[asset as usize].lifecycle,
                            AssetLifecycleV16::Retired
                        );
                        let next = h.env.market_state().1.next_market_id;
                        h.slot += 1;
                        h.env.svm.warp_to_slot(h.slot);
                        let cu = h.env.activate_asset(asset, h.slot, PRICE);
                        assert_cu_within("used generation replacement", cu, CU_LIMIT);
                        assert_ne!(next, generations[asset as usize]);
                        generations[asset as usize] = next;
                        assert_eq!(h.env.asset_market_id(asset), next);
                        assert_eq!(
                            [
                                h.portfolios[0],
                                h.portfolios[1],
                                h.env.vault,
                                h.env.mint,
                                h.tokens[0],
                                h.tokens[1]
                            ]
                            .map(|key| h.env.svm.get_account(&key)),
                            frame
                        );
                        h.env
                            .configure_auth_mark_for_asset_as_admin(asset, h.slot, PRICE);
                        h.env.svm.expire_blockhash();
                        let tx = signed(&h, &[stale], &[0, 1]);
                        m.submit(
                            &mut h,
                            tx,
                            Some((2, PercolatorError::AssetGenerationMismatch)),
                        );
                        reused += 1;
                    }
                    h.slot += 1;
                    h.env.svm.warp_to_slot(h.slot);
                    h.env.activate_asset(ASSETS as u16, h.slot, PRICE);
                    h.env
                        .configure_auth_mark_for_asset_as_admin(ASSETS as u16, h.slot, PRICE);
                    for asset in 0..historical_assets as u16 {
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
                    assert_eq!(h.source_claims(0), historical);
                    let mut opening: Vec<_> = (historical_assets..ASSETS)
                        .map(|a| {
                            let units = direction * (4 + 2 * (a - historical_assets) as i128);
                            (a as u16, units * POS_SCALE as i128, PRICE)
                        })
                        .collect();
                    let chunks = if batch { opening.len() } else { 1 };
                    for legs in opening.chunks(chunks) {
                        let prefix = trade(&h, legs, batch);
                        rollback_retry(&mut h, &mut m, prefix, &[0, 1]);
                        for &(a, q, _) in legs {
                            h.positions[a as usize] += q;
                        }
                        h.assert_accounting();
                    }
                    resources(&h, historical, &generations);
                    let overflow = trade(&h, &[(ASSETS as u16, POS_SCALE as i128, PRICE)], batch);
                    h.env.svm.expire_blockhash();
                    let tx = signed(&h, &[overflow.clone()], &[0, 1]);
                    m.submit(&mut h, tx, Some((2, PercolatorError::InvalidInstruction)));
                    resources(&h, historical, &generations);

                    // Partial reduction retains both latent domains. A later capacity
                    // rejection must restore that real prefix without blocking its retry.
                    let (reducing_asset, q, price) = opening[0];
                    let reduction = trade(&h, &[(reducing_asset, -q / 2, price)], batch);
                    let mut overflow = overflow;
                    let mut request = ProgInstruction::decode(&overflow.data).unwrap();
                    match &mut request {
                        ProgInstruction::TradeNoCpi {
                            account_a_position_epoch,
                            account_b_position_epoch,
                            ..
                        }
                        | ProgInstruction::BatchTradeNoCpi {
                            account_a_position_epoch,
                            account_b_position_epoch,
                            ..
                        } => {
                            *account_a_position_epoch += 1;
                            *account_b_position_epoch += 1;
                        }
                        _ => unreachable!(),
                    }
                    overflow.data = request.encode();
                    h.env.svm.expire_blockhash();
                    let retry = signed(&h, &[reduction.clone()], &[0, 1]);
                    let retry_bytes = bincode::serialize(&retry).unwrap();
                    let rejected = signed(&h, &[reduction, overflow], &[0, 1]);
                    m.submit(
                        &mut h,
                        rejected,
                        Some((3, PercolatorError::InvalidInstruction)),
                    );
                    resources(&h, historical, &generations);
                    assert_eq!(bincode::serialize(&retry).unwrap(), retry_bytes);
                    m.submit(&mut h, retry, None);
                    h.positions[reducing_asset as usize] -= q / 2;
                    opening[0].1 -= q / 2;
                    resources(&h, historical, &generations);
                    capacity_prefix_retries += 1;

                    let marks: Vec<_> = opening
                        .iter()
                        .map(|&(a, _, _)| (a, (PRICE as i128 + direction) as u64))
                        .collect();
                    h.mark_and_settle(&marks, order);
                    let mut expected = historical;
                    for &(a, q, _) in &opening {
                        expected[2 * a as usize + usize::from(q > 0)] +=
                            q.unsigned_abs() / POS_SCALE * BOUND_SCALE;
                    }
                    resources(&h, expected, &generations);
                    let reversals: Vec<_> = opening
                        .iter()
                        .map(|&(a, q, _)| (a, -3 * q, (PRICE as i128 + direction) as u64))
                        .collect();
                    h.trade(route, &reversals);
                    resources(&h, expected, &generations);
                    h.mark_and_settle(
                        &opening.iter().map(|l| (l.0, PRICE)).collect::<Vec<_>>(),
                        [order[1], order[0]],
                    );
                    for &(a, q, _) in &opening {
                        expected[2 * a as usize + usize::from(q < 0)] +=
                            2 * q.unsigned_abs() / POS_SCALE * BOUND_SCALE;
                    }
                    assert!(expected.iter().all(|claim| *claim > 0));
                    resources(&h, expected, &generations);
                    let mut misattributed = h.source_claims(0);
                    misattributed.swap(0, 2);
                    assert_eq!(
                        misattributed.iter().sum::<u128>(),
                        expected.iter().sum::<u128>()
                    );
                    assert_ne!(
                        misattributed, expected,
                        "conserved domain misattribution fails the exact oracle"
                    );
                    h.env.svm.expire_blockhash();
                    let overflow = trade(&h, &[(ASSETS as u16, POS_SCALE as i128, PRICE)], batch);
                    let tx = signed(&h, &[overflow], &[0, 1]);
                    m.submit(&mut h, tx, Some((2, PercolatorError::InvalidInstruction)));
                    for &(a, q, _) in &opening {
                        let before = h.positions.iter().filter(|q| **q != 0).count();
                        let close = trade(&h, &[(a, 2 * q, PRICE)], batch);
                        rollback_retry(&mut h, &mut m, close, &[0, 1]);
                        h.positions[a as usize] = 0;
                        resources(&h, expected, &generations);
                        assert_eq!(h.positions.iter().filter(|q| **q != 0).count() + 1, before);
                    }
                    assert_eq!(h.portfolios.map(|p| h.env.portfolio_id(p)), ids);
                    terminal(&mut h, &mut m, order);
                    history_calls += h.calls;
                    m.max_cu = m.max_cu.max(h.max_trade).max(h.max_crank);
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!((worlds, reused), (24, 12));
    assert_eq!(capacity_prefix_retries, worlds);
    assert_eq!(m.terminal_calls, 24 * (DOMAINS + 1));
    assert!(m.restored_prefixes > 0);
    println!("INV-028 Scope W: worlds={worlds}, reused={reused}, capacity_prefix_retries={capacity_prefix_retries}, history_calls={history_calls}, checked_transactions={}, exact_rollbacks={}, restored_prefixes={}, terminal_calls={}, max_cu={}, max_packet={}",
        m.transactions, m.rollbacks, m.restored_prefixes, m.terminal_calls, m.max_cu, m.max_packet);
}
