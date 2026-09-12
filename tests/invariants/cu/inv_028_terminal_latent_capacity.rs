//! INV-028/057/073/077/078: terminal exit must materialize all reserved latent domains.
//! Fourteen retained claims and fourteen pending favorable settlements cross resolution
//! and owner-window expiry. Public terminal routes grow the source table before draining it.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census, assert_source_credit_rates,
};

const OWNER_WINDOW: u64 = 5;

fn census(h: &History) {
    let market = h.env.svm.get_account(&h.env.market).unwrap();
    let group = h.env.market_state().1;
    let accounts = h.portfolios.map(|p| h.env.portfolio_state(p));
    assert_market_stock_census(
        "terminal latent capacity",
        &group,
        &market.data,
        &accounts,
        h.env.token_amount(h.env.vault) as u128,
    )
    .unwrap();
    assert_reservation_encumbrance_census("terminal latent capacity", &group, &accounts).unwrap();
    assert_source_credit_rates("terminal latent capacity", &group).unwrap();
    let claims = h.source_claims(0);
    assert_eq!(h.source_claims(1), [0; DOMAINS]);
    assert_eq!(group.insurance, 0);
    assert_eq!(
        group.vault
            + h.tokens
                .iter()
                .map(|p| h.env.token_amount(*p) as u128)
                .sum::<u128>(),
        2 * CAPITAL
    );
    for domain in 0..DOMAINS {
        let source = &group.source_credit[domain];
        let claim = claims[domain];
        assert!(claim == 0 || claim == h.claims[domain] * BOUND_SCALE);
        assert_eq!(source.exact_positive_claim_num, claim);
        assert_eq!(source.positive_claim_bound_num, claim);
        assert_eq!(source.valid_liened_backing_num, 0);
        assert_eq!(source.impaired_liened_backing_num, 0);
        assert_eq!(source.insurance_credit_reserved_num, 0);
        assert!(
            claim * source.credit_rate_num / percolator::CREDIT_RATE_SCALE
                <= group.source_backing_buckets[domain].fresh_unliened_backing_num
        );
    }
}

fn terminal_step(h: &mut History, actor: usize, automatic: bool, signed: bool) -> (u64, usize) {
    let before = h.env.portfolio_state(h.portfolios[actor]);
    let active_before = percolator::active_bitmap_count_ones(active_bitmap(&before));
    let claims_before = h.source_claims(actor);
    let sources_before = claims_before.iter().filter(|c| **c != 0).count();
    let peer = h.env.svm.get_account(&h.portfolios[1 - actor]);
    let peer_token = h.env.svm.get_account(&h.tokens[1 - actor]);
    let mint = h.env.svm.get_account(&h.env.mint);
    let vault_before = h.env.token_amount(h.env.vault);
    let paid_before = h.env.token_amount(h.tokens[actor]);
    let instruction = if automatic {
        ProgInstruction::PermissionlessCrank {
            now_slot: h.slot,
            observations: vec![],
        }
    } else {
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        }
    };
    let ix = Instruction {
        program_id: h.env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(h.owners[actor].pubkey(), signed),
            AccountMeta::new(h.env.market, false),
            AccountMeta::new(h.portfolios[actor], false),
            AccountMeta::new(h.tokens[actor], false),
            AccountMeta::new(h.env.vault, false),
            AccountMeta::new_readonly(h.env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: instruction.encode(),
    };
    h.env.svm.expire_blockhash();
    let mut signers = vec![&h.env.payer];
    if signed {
        signers.push(&h.owners[actor]);
    }
    let tx = Transaction::new_signed_with_payer(
        &[
            heap_ix(),
            ComputeBudgetInstruction::set_compute_unit_limit(CU_LIMIT as u32),
            ix,
        ],
        Some(&h.env.payer.pubkey()),
        &signers,
        h.env.svm.latest_blockhash(),
    );
    assert_eq!(
        tx.message.header.num_required_signatures,
        1 + u8::from(signed)
    );
    let bytes = bincode::serialize(&tx).unwrap().len();
    assert!(bytes <= 1232, "terminal route fits the transaction packet");
    let cu = h
        .env
        .svm
        .send_transaction(tx)
        .expect("admitted latent domains retain bounded terminal exit")
        .compute_units_consumed;
    h.calls += 1;
    assert_cu_within("terminal latent source growth and exit", cu, CU_LIMIT);
    let after = h.env.portfolio_state(h.portfolios[actor]);
    let active_after = percolator::active_bitmap_count_ones(active_bitmap(&after));
    let claims_after = h.source_claims(actor);
    let sources_after = claims_after.iter().filter(|c| **c != 0).count();
    let paid = h.env.token_amount(h.tokens[actor]) - paid_before;
    assert_eq!(vault_before - h.env.token_amount(h.env.vault), paid);
    if active_before > 0 {
        assert_eq!(
            active_after + 1,
            active_before,
            "one canonical leg detaches"
        );
        if active_after > 0 {
            assert_eq!(paid, 0, "partial leg exit cannot pay early");
            if actor == 0 {
                assert_eq!(claims_after, h.claims.map(|c| c * BOUND_SCALE));
                assert_eq!(after.pnl.get(), h.claims.iter().sum::<u128>() as i128);
                assert_eq!(after.capital.get(), CAPITAL);
            } else {
                assert_eq!(after.capital.get(), CAPITAL - h.claims.iter().sum::<u128>());
            }
        } else if actor == 0 {
            assert_eq!(sources_after + 1, sources_before);
        }
    } else if sources_before > 0 {
        assert_eq!(
            sources_after + 1,
            sources_before,
            "one canonical source retires"
        );
        assert_eq!(
            claims_before
                .iter()
                .zip(claims_after)
                .filter(|(a, b)| **a != *b)
                .count(),
            1
        );
    } else {
        assert!(resolved_portfolio_is_terminal(&h.env, h.portfolios[actor]));
        assert!(paid > 0, "the final continuation makes economic progress");
    }
    assert_eq!(h.env.svm.get_account(&h.portfolios[1 - actor]), peer);
    assert_eq!(h.env.svm.get_account(&h.tokens[1 - actor]), peer_token);
    assert_eq!(h.env.svm.get_account(&h.env.mint), mint);
    let gain = h.claims.iter().sum::<u128>();
    for (owner, entitlement) in [CAPITAL + gain, CAPITAL - gain].into_iter().enumerate() {
        assert!(h.env.token_amount(h.tokens[owner]) as u128 <= entitlement);
    }
    census(h);
    (cu, bytes)
}

#[test]
fn v16_program_full_latent_settlement_survives_terminal_owner_window_expiry() {
    assert_certified_engine_pin("INV-028 maximum latent terminal settlement");
    let mut worlds = 0;
    let mut calls = 0;
    let mut terminal_calls = 0;
    let mut max_terminal_calls = 0;
    let mut max_bytes = 0;
    let mut maxima = [0; 6];
    let units: [i128; ASSETS] =
        std::array::from_fn(|a| (1 + (a % 3) as i128) * if a % 2 == 0 { 1 } else { -1 });
    let gain = units.iter().map(|q| 2 * q.unsigned_abs() + 1).sum::<u128>();
    let expected_payouts = [CAPITAL + gain, CAPITAL - gain];
    for automatic in [false, true] {
        for late in [0, 1] {
            let mut h = History::new();
            h.env
                .configure_permissionless_resolve_with_cu(1, OWNER_WINDOW);
            let opening: Vec<_> = (0..ASSETS)
                .map(|a| (a as u16, units[a] * POS_SCALE as i128, PRICE))
                .collect();
            trade_current(&mut h, &opening);
            let marks: Vec<_> = (0..ASSETS)
                .map(|a| (a as u16, (PRICE as i128 + units[a].signum()) as u64))
                .collect();
            h.mark_and_settle(&marks, [1, 0]);
            let retained = h.source_claims(0);
            assert_eq!(retained.iter().filter(|c| **c != 0).count(), ASSETS);

            // Increase every existing leg while the historical/future union is already full.
            let increases: Vec<_> = marks
                .iter()
                .map(|&(a, price)| (a, units[a as usize].signum() * POS_SCALE as i128, price))
                .collect();
            trade_current(&mut h, &increases);
            assert_eq!(h.source_claims(0), retained);
            let flips: Vec<_> = marks
                .iter()
                .map(|&(a, price)| {
                    (
                        a,
                        -2 * (units[a as usize] + units[a as usize].signum()) * POS_SCALE as i128,
                        price,
                    )
                })
                .collect();
            trade_current(&mut h, &flips);
            assert_eq!(h.source_claims(0), retained);
            for portfolio in h.portfolios {
                let account = h.env.portfolio_state(portfolio);
                assert_eq!(
                    percolator::active_bitmap_count_ones(active_bitmap(&account)) as usize,
                    ASSETS
                );
                let mut resources: BTreeSet<_> = account
                    .source_domains
                    .iter()
                    .filter(|s| s.is_occupied())
                    .map(|s| s.domain.get())
                    .collect();
                for a in 0..ASSETS as u32 {
                    resources.extend([2 * a, 2 * a + 1]);
                }
                assert_eq!(resources.len(), DOMAINS);
            }

            h.previous_claims = h.claims;
            h.previous_prices = h.prices;
            h.slot += 1;
            h.env.svm.warp_to_slot(h.slot);
            let winner = h.env.svm.get_account(&h.portfolios[0]);
            let mut max_mark = 0;
            for a in 0..ASSETS {
                let domain = 2 * a + usize::from(h.positions[a] > 0);
                assert_eq!(retained[domain], 0);
                h.claims[domain] = units[a].unsigned_abs() + 1;
                h.prices[a] = PRICE;
                let cu = h
                    .env
                    .push_auth_mark_for_asset_as_admin(a as u16, h.slot, PRICE);
                max_mark = max_mark.max(cu);
                h.calls += 1;
                assert_cu_within("terminal latent mark", cu, CU_LIMIT);
                h.assert_accounting();
            }
            let pending = |h: &History| {
                let group = h.env.market_state().1;
                (0..ASSETS)
                    .map(|a| h.slot - group.assets[a].slot_last)
                    .sum::<u64>()
            };
            for _ in 0..ASSETS {
                let before = pending(&h);
                if before == 0 {
                    break;
                }
                h.env.svm.expire_blockhash();
                let cu = h.env.crank(
                    h.portfolios[1],
                    ProgInstruction::PermissionlessCrank {
                        now_slot: h.slot,
                        observations: crank_observations_for_assets(
                            &(0..ASSETS as u16).collect::<Vec<_>>(),
                        ),
                    },
                );
                h.calls += 1;
                h.max_crank = h.max_crank.max(cu);
                assert_cu_within("terminal latent global accrual", cu, CU_LIMIT);
                assert!(pending(&h) < before);
                assert_eq!(h.env.svm.get_account(&h.portfolios[0]), winner);
                h.assert_accounting();
            }
            assert_eq!(pending(&h), 0);
            let frames = h.portfolios.map(|p| h.env.svm.get_account(&p));
            let resolve_cu = h.env.resolve();
            assert_cu_within("full latent resolution", resolve_cu, CU_LIMIT);
            assert_eq!(h.portfolios.map(|p| h.env.svm.get_account(&p)), frames);
            assert_eq!(h.env.market_state().1.mode, MarketModeV16::Resolved);
            assert_eq!(h.source_claims(0), retained);
            let expiry = h.env.market_state().1.resolved_slot + OWNER_WINDOW;
            h.slot = expiry - 1;
            h.env.svm.warp_to_slot(h.slot);
            let (mut max_terminal, bytes) = terminal_step(&mut h, 1, false, true);
            max_bytes = max_bytes.max(bytes);
            assert_eq!(h.env.svm.get_account(&h.portfolios[0]), winner);
            assert_eq!(h.source_claims(0), retained);

            h.slot = expiry + late;
            h.env.svm.warp_to_slot(h.slot);
            let mut steps = 1;
            for _ in 0..(ASSETS + DOMAINS + 2) {
                if h.portfolios
                    .iter()
                    .all(|p| resolved_portfolio_is_terminal(&h.env, *p))
                {
                    break;
                }
                for actor in [0, 1] {
                    if resolved_portfolio_is_terminal(&h.env, h.portfolios[actor]) {
                        continue;
                    }
                    let (cu, bytes) = terminal_step(&mut h, actor, automatic, false);
                    max_terminal = max_terminal.max(cu);
                    max_bytes = max_bytes.max(bytes);
                    steps += 1;
                }
            }
            assert_eq!(steps, 2 * ASSETS + DOMAINS - 1);
            for actor in 0..2 {
                assert!(resolved_portfolio_is_terminal(&h.env, h.portfolios[actor]));
                assert_eq!(
                    h.env.token_amount(h.tokens[actor]) as u128,
                    expected_payouts[actor]
                );
                h.env.svm.expire_blockhash();
                let cu = h
                    .env
                    .close_portfolio_with_cu(&h.owners[actor], h.portfolios[actor]);
                h.calls += 1;
                h.max_close = h.max_close.max(cu);
                assert_cu_within("terminal latent portfolio deletion", cu, CUSTODY_CU_LIMIT);
            }
            let group = h.env.market_state().1;
            assert_eq!((group.c_tot, group.vault, group.insurance), (0, 0, 0));
            assert_eq!(group.materialized_portfolio_count, 0);
            for a in 0..ASSETS {
                assert_eq!(
                    (
                        group.assets[a].oi_eff_long_q,
                        group.assets[a].oi_eff_short_q
                    ),
                    (0, 0)
                );
            }
            for d in 0..DOMAINS {
                assert_eq!(group.source_credit[d].positive_claim_bound_num, 0);
                assert_eq!(
                    group.source_backing_buckets[d].fresh_unliened_backing_num,
                    0
                );
            }
            worlds += 1;
            calls += h.calls;
            terminal_calls += steps;
            max_terminal_calls = max_terminal_calls.max(steps);
            for (maximum, cu) in maxima.iter_mut().zip([
                h.max_trade,
                h.max_crank,
                max_mark,
                resolve_cu,
                max_terminal,
                h.max_close,
            ]) {
                *maximum = (*maximum).max(cu);
            }
        }
    }
    assert_eq!(worlds, 4);
    println!("INV-028 terminal latent capacity: worlds={worlds}, increases={}, history_calls={calls}, terminal_calls={terminal_calls}, max_terminal_calls={max_terminal_calls}, max_packet_bytes={max_bytes}, payouts={expected_payouts:?}, max CU trade/crank/mark/resolve/terminal/delete={maxima:?}", worlds * ASSETS);
}
