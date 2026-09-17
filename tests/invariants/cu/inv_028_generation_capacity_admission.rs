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
    terminal_with_claims(h, m, order, &expected);
}

fn domain_claims(h: &History, actor: usize, count: usize) -> Vec<u128> {
    let mut claims = vec![0; count];
    let account = h.env.portfolio_state(h.portfolios[actor]);
    for source in account.source_domains.iter().filter(|s| s.is_occupied()) {
        let domain = source.domain.get() as usize;
        assert_eq!(claims[domain], 0, "unique source attribution");
        claims[domain] = source.source_claim_bound_num.get();
        assert!(claims[domain] > 0);
        assert_eq!(source.source_claim_liened_num.get(), 0);
        assert_eq!(source.source_lien_counterparty_backing_num.get(), 0);
        assert_eq!(source.source_lien_insurance_backing_num.get(), 0);
        assert_eq!(
            source.source_claim_market_id.get(),
            h.env.asset_market_id((domain / 2) as u16)
        );
    }
    claims
}

fn terminal_with_claims(
    h: &mut History,
    m: &mut Measurements,
    order: [usize; 2],
    expected: &[u128],
) {
    for portfolio in h.portfolios {
        assert_eq!(
            percolator::active_bitmap_count_ones(active_bitmap(&h.env.portfolio_state(portfolio))),
            0
        );
    }
    assert_eq!(domain_claims(h, 0, expected.len()), expected);
    let gain = expected.iter().sum::<u128>() / BOUND_SCALE;
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
            domain_claims(h, actor, expected.len())
                .iter()
                .filter(|c| **c > 0)
                .count()
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
            let remaining = domain_claims(h, 0, expected.len());
            for domain in 0..expected.len() {
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

fn competition_resources(h: &History, claims: &[u128], positions: &[i128]) -> usize {
    let group = h.env.market_state().1;
    let market = h.env.svm.get_account(&h.env.market).unwrap();
    let accounts = h.portfolios.map(|p| h.env.portfolio_state(p));
    assert_market_stock_census(
        "mixed latent reclamation",
        &group,
        &market.data,
        &accounts,
        h.env.token_amount(h.env.vault) as u128,
    )
    .unwrap();
    assert_reservation_encumbrance_census("mixed latent reclamation", &group, &accounts).unwrap();
    assert_source_credit_rates("mixed latent reclamation", &group).unwrap();
    assert_eq!(domain_claims(h, 0, claims.len()), claims);
    assert_eq!(domain_claims(h, 1, claims.len()), vec![0; claims.len()]);
    let gain = claims.iter().sum::<u128>() / BOUND_SCALE;
    assert_eq!(accounts[0].capital.get(), CAPITAL);
    assert_eq!(accounts[0].pnl.get(), gain as i128);
    assert_eq!(accounts[1].capital.get(), CAPITAL - gain);
    assert_eq!(accounts[1].pnl.get(), 0);
    assert_eq!(
        (group.vault, group.c_tot, group.insurance),
        (2 * CAPITAL, 2 * CAPITAL - gain, 0)
    );
    assert_eq!(h.tokens.map(|t| h.env.token_amount(t)), [0; 2]);
    assert_eq!(
        Mint::unpack(&h.env.svm.get_account(&h.env.mint).unwrap().data)
            .unwrap()
            .supply as u128,
        2 * CAPITAL
    );
    for (domain, &claim) in claims.iter().enumerate() {
        assert_eq!(group.source_credit[domain].positive_claim_bound_num, claim);
        assert_eq!(group.source_credit[domain].exact_positive_claim_num, claim);
        assert_eq!(
            group.source_backing_buckets[domain].fresh_unliened_backing_num,
            claim
        );
    }
    let mut reserved: BTreeSet<_> = claims
        .iter()
        .enumerate()
        .filter_map(|(domain, &claim)| (claim > 0).then_some(domain))
        .collect();
    for (actor, account) in accounts.iter().enumerate() {
        assert_eq!(
            percolator::active_bitmap_count_ones(active_bitmap(account)) as usize,
            positions.iter().filter(|q| **q != 0).count()
        );
        for (asset, &q) in positions.iter().enumerate() {
            assert_eq!(has_active_leg_for_asset(account, asset), q != 0);
            if q != 0 {
                assert_eq!(
                    active_leg_for_asset(account, asset).basis_pos_q,
                    if actor == 0 { q } else { -q }
                );
                reserved.extend([2 * asset, 2 * asset + 1]);
            }
            assert_eq!(group.assets[asset].oi_eff_long_q, q.unsigned_abs());
            assert_eq!(group.assets[asset].oi_eff_short_q, q.unsigned_abs());
        }
    }
    assert!(reserved.len() <= DOMAINS);
    reserved.len()
}

fn competition_settle(
    h: &mut History,
    m: &mut Measurements,
    claims: &mut [u128],
    positions: &[i128],
    moves: &[(u16, u64, u64)],
    order: [usize; 2],
) {
    h.slot += 1;
    h.env.svm.warp_to_slot(h.slot);
    for &(asset, before, after) in moves {
        assert_eq!(
            h.env.market_state().1.assets[asset as usize].effective_price,
            before
        );
        let q = positions[asset as usize];
        let gain = q / POS_SCALE as i128 * (after as i128 - before as i128);
        assert!(gain > 0);
        claims[2 * asset as usize + usize::from(q > 0)] += gain as u128 * BOUND_SCALE;
        let cu = h
            .env
            .push_auth_mark_for_asset_as_admin(asset, h.slot, after);
        assert_cu_within("mixed latent reclamation mark", cu, CU_LIMIT);
        m.max_cu = m.max_cu.max(cu);
    }
    let gain = claims.iter().sum::<u128>() / BOUND_SCALE;
    for actor in order {
        let rank = |h: &History| {
            let group = h.env.market_state().1;
            let account = h.env.portfolio_state(h.portfolios[actor]);
            let pending = moves
                .iter()
                .map(|m| h.slot - group.assets[m.0 as usize].slot_last)
                .sum::<u64>();
            u128::from(pending)
                + if actor == 0 {
                    account.pnl.get().abs_diff(gain as i128)
                } else {
                    account.capital.get().abs_diff(CAPITAL - gain)
                }
        };
        for _ in 0..4 {
            let before = rank(h);
            if before == 0 {
                break;
            }
            let crank = instruction(
                h,
                ProgInstruction::PermissionlessCrank {
                    now_slot: h.slot,
                    observations: crank_observations_for_assets(
                        &moves.iter().map(|m| m.0).collect::<Vec<_>>(),
                    ),
                },
                vec![
                    AccountMeta::new(h.env.payer.pubkey(), true),
                    AccountMeta::new(h.env.market, false),
                    AccountMeta::new(h.portfolios[actor], false),
                ],
            );
            h.env.svm.expire_blockhash();
            let tx = signed(h, &[crank], &[]);
            m.submit(h, tx, None);
            assert!(
                rank(h) < before,
                "accepted settlement decreases pending work or economic debt"
            );
        }
        assert_eq!(rank(h), 0, "four public cranks suffice per owner");
    }
    assert_eq!(competition_resources(h, claims, positions), DOMAINS);
}

#[test]
fn v16_program_mixed_materialized_latent_reclamation_admits_only_fitting_replacement() {
    assert!(include_str!("../../../Cargo.lock").contains(
        "git+https://github.com/aeyakovenko/percolator?rev=4db11a8cb0053815e23a35d3a7d3edc265d8d866#\
         4db11a8cb0053815e23a35d3a7d3edc265d8d866"
    ));
    let mut m = Measurements::default();
    let mut worlds = 0;
    let partial = (ASSETS - 2) as u16;
    let latent = (ASSETS - 1) as u16;
    let replacement = ASSETS as u16;
    for direction in [-1i128, 1] {
        for reverse_batch in [false, true] {
            let mut h = History::with_market_capacity(ASSETS + 1);
            h.env.configure_permissionless_resolve_with_cu(1_000, 5);
            h.slot += 1;
            h.env.svm.warp_to_slot(h.slot);
            h.env.activate_asset(replacement, h.slot, PRICE);
            h.env
                .configure_auth_mark_for_asset_as_admin(replacement, h.slot, PRICE);
            let order = if direction > 0 { [0, 1] } else { [1, 0] };
            let route = AccountResidualCounterTradePath::TradeNoCpi;
            for asset in 0..partial {
                let q = (1 + i128::from(asset % 3)) * POS_SCALE as i128;
                h.trade(route, &[(asset, q, PRICE)]);
                h.mark_and_settle(&[(asset, PRICE + 1)], order);
                h.trade(route, &[(asset, -2 * q, PRICE + 1)]);
                h.mark_and_settle(&[(asset, PRICE)], order);
                h.trade(route, &[(asset, q, PRICE)]);
            }
            let partial_q = direction * 4 * POS_SCALE as i128;
            let latent_q = -direction * 6 * POS_SCALE as i128;
            let replacement_q = direction * 7 * POS_SCALE as i128;
            let moved_price = (PRICE as i128 + direction) as u64;
            h.trade(
                route,
                &[(partial, partial_q, PRICE), (latent, latent_q, PRICE)],
            );
            h.mark_and_settle(&[(partial, moved_price)], order);
            let mut claims = vec![0; DOMAINS + 2];
            claims[..DOMAINS].copy_from_slice(&h.claims.map(|c| c * BOUND_SCALE));
            let mut positions = vec![0; ASSETS + 1];
            positions[..ASSETS].copy_from_slice(&h.positions);
            assert_eq!(claims.iter().filter(|c| **c > 0).count(), DOMAINS - 3);
            assert_eq!(competition_resources(&h, &claims, &positions), DOMAINS);
            let retained = h
                .portfolios
                .map(|p| h.env.portfolio_state(p).source_domains);
            let ids = h.portfolios.map(|p| h.env.portfolio_id(p));
            let custody = [h.env.vault, h.env.mint, h.tokens[0], h.tokens[1]]
                .map(|key| h.env.svm.get_account(&key));
            let projected_count = |closing: u16| {
                let mut reserved: BTreeSet<_> = claims
                    .iter()
                    .enumerate()
                    .filter_map(|(domain, &claim)| (claim > 0).then_some(domain))
                    .collect();
                for (asset, &q) in positions.iter().enumerate() {
                    if q != 0 && asset != closing as usize {
                        reserved.extend([2 * asset, 2 * asset + 1]);
                    }
                }
                reserved.extend([2 * replacement as usize, 2 * replacement as usize + 1]);
                reserved.len()
            };
            assert_eq!(projected_count(partial), DOMAINS + 1);
            assert_eq!(projected_count(latent), DOMAINS);
            let batch = |h: &History, closing: u16, q: i128, price: u64| {
                let mut legs = vec![(closing, -q, price), (replacement, replacement_q, PRICE)];
                if reverse_batch {
                    legs.reverse();
                }
                trade(h, &legs, true)
            };

            // Closing the partially materialized leg frees only its unused side:
            // 25 retained claims + 2 surviving latent + 2 replacement = 29.
            let rejected = batch(&h, partial, partial_q, moved_price);
            h.env.svm.expire_blockhash();
            let tx = signed(&h, &[rejected], &[0, 1]);
            m.submit(&mut h, tx, Some((2, PercolatorError::InvalidInstruction)));
            assert_eq!(competition_resources(&h, &claims, &positions), DOMAINS);

            // Closing the wholly latent sibling instead leaves a 28-domain union,
            // including the partially materialized survivor's still-unused side.
            let accepted = batch(&h, latent, latent_q, PRICE);
            rollback_retry(&mut h, &mut m, accepted, &[0, 1]);
            positions[latent as usize] = 0;
            positions[replacement as usize] = replacement_q;
            assert_eq!(competition_resources(&h, &claims, &positions), DOMAINS);
            assert_eq!(
                h.portfolios
                    .map(|p| h.env.portfolio_state(p).source_domains),
                retained
            );
            assert_eq!(
                [h.env.vault, h.env.mint, h.tokens[0], h.tokens[1]]
                    .map(|key| h.env.svm.get_account(&key)),
                custody
            );

            let flip = trade(&h, &[(partial, -2 * partial_q, moved_price)], false);
            rollback_retry(&mut h, &mut m, flip, &[0, 1]);
            positions[partial as usize] = -partial_q;
            assert_eq!(competition_resources(&h, &claims, &positions), DOMAINS);
            competition_settle(
                &mut h,
                &mut m,
                &mut claims,
                &positions,
                &[
                    (partial, moved_price, PRICE),
                    (replacement, PRICE, moved_price),
                ],
                order,
            );
            let flip = trade(&h, &[(replacement, -3 * replacement_q, moved_price)], false);
            rollback_retry(&mut h, &mut m, flip, &[0, 1]);
            positions[replacement as usize] = -2 * replacement_q;
            assert_eq!(competition_resources(&h, &claims, &positions), DOMAINS);
            competition_settle(
                &mut h,
                &mut m,
                &mut claims,
                &positions,
                &[(replacement, moved_price, PRICE)],
                [order[1], order[0]],
            );
            assert_eq!(claims.iter().filter(|c| **c > 0).count(), DOMAINS);
            assert_eq!(
                &claims[2 * latent as usize..2 * latent as usize + 2],
                &[0, 0]
            );
            let historical_gain = 2 * (0..partial).map(|a| 1 + u128::from(a % 3)).sum::<u128>();
            assert_eq!(
                claims.iter().sum::<u128>(),
                (historical_gain + 8 + 21) * BOUND_SCALE
            );
            for asset in [partial, replacement] {
                let close = trade(&h, &[(asset, -positions[asset as usize], PRICE)], false);
                rollback_retry(&mut h, &mut m, close, &[0, 1]);
                positions[asset as usize] = 0;
                assert_eq!(competition_resources(&h, &claims, &positions), DOMAINS);
            }
            assert_eq!(h.portfolios.map(|p| h.env.portfolio_id(p)), ids);
            terminal_with_claims(&mut h, &mut m, order, &claims);
            m.max_cu = m.max_cu.max(h.max_trade).max(h.max_crank);
            worlds += 1;
        }
    }
    assert_eq!(worlds, 4);
    assert_eq!(m.terminal_calls, worlds * (DOMAINS + 1));
    assert_eq!(m.rollbacks, worlds * 8);
    assert_eq!(m.restored_prefixes, worlds * 7);
    println!("INV-028 mixed latent reclamation: worlds={worlds}, checked_transactions={}, exact_rollbacks={}, restored_prefixes={}, terminal_calls={}, max_cu={}, max_packet={}",
        m.transactions, m.rollbacks, m.restored_prefixes, m.terminal_calls, m.max_cu, m.max_packet);
}
