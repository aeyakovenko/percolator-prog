//! Row 423: the last reserved source slot must materialize through asset Recovery.
//! Unlike the existing full-source force-close control, shutdown starts with 27
//! claims and one pending favorable source. Full/split keeper closes precede payout.

use super::*;
use crate::support::fuzz_model::{
    assert_market_stock_census, assert_reservation_encumbrance_census, assert_source_credit_rates,
};

const LIVE: usize = ASSETS - 1;
const UNITS: u128 = 6;
const OWNER_WINDOW: u64 = 5;

fn census(h: &History) {
    let market = h.env.svm.get_account(&h.env.market).unwrap();
    let group = h.env.market_state().1;
    let accounts: Vec<_> = h
        .portfolios
        .iter()
        .filter(|p| {
            h.env.svm.get_account(p).is_some_and(|a| {
                a.owner == h.env.program_id && a.data.len() == h.env.portfolio_account_len
            })
        })
        .map(|p| h.env.portfolio_state(*p))
        .collect();
    assert_market_stock_census(
        "Recovery latent capacity",
        &group,
        &market.data,
        &accounts,
        h.env.token_amount(h.env.vault) as u128,
    )
    .unwrap();
    assert_reservation_encumbrance_census("Recovery latent capacity", &group, &accounts).unwrap();
    assert_source_credit_rates("Recovery latent capacity", &group).unwrap();
    assert_eq!(group.insurance, 0);
    assert_eq!(
        group.vault
            + h.tokens
                .iter()
                .map(|t| h.env.token_amount(*t) as u128)
                .sum::<u128>(),
        2 * CAPITAL
    );
    assert_eq!(
        Mint::unpack(&h.env.svm.get_account(&h.env.mint).unwrap().data)
            .unwrap()
            .supply as u128,
        2 * CAPITAL
    );
}

fn terminal_step(h: &mut History, actor: usize) -> (u64, usize) {
    let claims_before = h.source_claims(actor);
    let peer = h.env.svm.get_account(&h.portfolios[1 - actor]);
    let peer_token = h.env.svm.get_account(&h.tokens[1 - actor]);
    let mint = h.env.svm.get_account(&h.env.mint);
    let vault = h.env.token_amount(h.env.vault);
    let wallet = h.env.token_amount(h.tokens[actor]);
    h.env.svm.expire_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &[
            heap_ix(),
            ComputeBudgetInstruction::set_compute_unit_limit(CU_LIMIT as u32),
            Instruction {
                program_id: h.env.program_id,
                accounts: vec![
                    AccountMeta::new_readonly(h.owners[actor].pubkey(), false),
                    AccountMeta::new(h.env.market, false),
                    AccountMeta::new(h.portfolios[actor], false),
                    AccountMeta::new(h.tokens[actor], false),
                    AccountMeta::new(h.env.vault, false),
                    AccountMeta::new_readonly(h.env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                data: ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                }
                .encode(),
            },
        ],
        Some(&h.env.payer.pubkey()),
        &[&h.env.payer],
        h.env.svm.latest_blockhash(),
    );
    assert_eq!(tx.message.header.num_required_signatures, 1);
    let bytes = bincode::serialize(&tx).unwrap().len();
    assert!(bytes <= 1232);
    let cu = h
        .env
        .svm
        .send_transaction(tx)
        .expect("Recovery source history retains permissionless terminal progress")
        .compute_units_consumed;
    assert_cu_within("Recovery full-source terminal step", cu, CU_LIMIT);
    let claims_after = h.source_claims(actor);
    let occupied_before = claims_before.iter().filter(|c| **c != 0).count();
    let occupied_after = claims_after.iter().filter(|c| **c != 0).count();
    if occupied_before > 0 {
        assert_eq!(occupied_after + 1, occupied_before);
        assert_eq!(
            claims_before
                .iter()
                .zip(claims_after)
                .filter(|(before, after)| **before != *after)
                .count(),
            1,
            "exactly one source retires; other historical claims are unchanged"
        );
    } else {
        assert!(resolved_portfolio_is_terminal(&h.env, h.portfolios[actor]));
        assert!(h.env.token_amount(h.tokens[actor]) > wallet);
    }
    let paid = h.env.token_amount(h.tokens[actor]) - wallet;
    assert_eq!(vault - h.env.token_amount(h.env.vault), paid);
    if occupied_after > 0 {
        assert_eq!(paid, 0, "partial source cleanup cannot pay early");
    }
    assert_eq!(h.env.svm.get_account(&h.portfolios[1 - actor]), peer);
    assert_eq!(h.env.svm.get_account(&h.tokens[1 - actor]), peer_token);
    assert_eq!(h.env.svm.get_account(&h.env.mint), mint);
    census(h);
    (cu, bytes)
}

#[test]
fn v16_program_last_latent_domain_survives_split_recovery_and_terminal_payout() {
    assert_certified_engine_pin("INV-028 Recovery last latent domain");
    let mut worlds = 0;
    let mut force_calls = 0;
    let mut terminal_calls = 0;
    let mut max_force_cu = 0;
    let mut max_terminal_cu = 0;
    let mut max_packet = 0;
    for direction in [-1i128, 1] {
        for split in [false, true] {
            let mut h = History::new();
            h.env
                .configure_permissionless_resolve_with_cu(1_000, OWNER_WINDOW);
            let route = AccountResidualCounterTradePath::TradeNoCpi;
            let order = if direction > 0 { [0, 1] } else { [1, 0] };
            for asset in 0..LIVE as u16 {
                let q = (1 + i128::from(asset % 3)) * POS_SCALE as i128;
                h.trade(route, &[(asset, q, PRICE)]);
                h.mark_and_settle(&[(asset, PRICE + 1)], order);
                h.trade(route, &[(asset, -2 * q, PRICE + 1)]);
                h.mark_and_settle(&[(asset, PRICE)], order);
                h.trade(route, &[(asset, q, PRICE)]);
            }
            assert_eq!(
                h.source_claims(0).iter().filter(|c| **c != 0).count(),
                DOMAINS - 2
            );
            let q = direction * UNITS as i128 * POS_SCALE as i128;
            let mark = (PRICE as i128 + direction) as u64;
            h.trade(route, &[(LIVE as u16, q, PRICE)]);
            h.mark_and_settle(&[(LIVE as u16, mark)], order);
            h.trade(route, &[(LIVE as u16, -2 * q, mark)]);
            let retained = h.env.portfolio_state(h.portfolios[0]).source_domains;
            let retained_claims = h.source_claims(0);
            let pending_domain = 2 * LIVE + usize::from(direction < 0);
            assert_eq!(retained_claims[pending_domain], 0);
            assert_eq!(
                retained_claims.iter().filter(|c| **c != 0).count(),
                DOMAINS - 1
            );

            // Only the debtor accrues the final mark. The absent winner still needs
            // its last slot when shutdown freezes this asset's settlement price.
            h.previous_claims = h.claims;
            h.previous_prices = h.prices;
            h.claims[pending_domain] = UNITS;
            h.prices[LIVE] = PRICE;
            h.slot += 1;
            h.env.svm.warp_to_slot(h.slot);
            let winner = h.env.svm.get_account(&h.portfolios[0]);
            let cu = h
                .env
                .push_auth_mark_for_asset_as_admin(LIVE as u16, h.slot, PRICE);
            assert_cu_within("Recovery latent mark", cu, CU_LIMIT);
            let cu = h.env.crank(
                h.portfolios[1],
                ProgInstruction::PermissionlessCrank {
                    now_slot: h.slot,
                    observations: crank_observations(LIVE as u16),
                },
            );
            assert_cu_within("Recovery latent accrual", cu, CU_LIMIT);
            assert_eq!(h.env.market_state().1.assets[LIVE].slot_last, h.slot);
            assert_eq!(h.env.svm.get_account(&h.portfolios[0]), winner);
            h.assert_accounting();
            let portfolios = h.portfolios.map(|p| h.env.svm.get_account(&p));
            let custody = [h.env.vault, h.env.mint, h.tokens[0], h.tokens[1]];
            let custody_before = custody.map(|p| h.env.svm.get_account(&p));
            let admin = Keypair::from_bytes(&h.env.admin.to_bytes()).unwrap();
            let cu = h
                .env
                .try_shutdown_asset_with_authority(&admin, LIVE as u16, h.slot)
                .expect("public shutdown with the last source still latent");
            assert_cu_within("Recovery latent shutdown", cu, CU_LIMIT);
            assert_eq!(h.portfolios.map(|p| h.env.svm.get_account(&p)), portfolios);
            assert_eq!(
                h.env.market_state().1.assets[LIVE].lifecycle,
                AssetLifecycleV16::Recovery
            );
            assert_eq!(h.source_claims(0), retained_claims);
            census(&h);

            h.slot += OWNER_WINDOW;
            h.env.svm.warp_to_slot(h.slot);
            let chunk = if split { UNITS / 2 } else { UNITS } * POS_SCALE;
            let mut remaining = UNITS * POS_SCALE;
            let mut steps = 0;
            while remaining > 0 {
                assert!(steps < 2, "full or two-chunk force-close bound");
                h.env.svm.expire_blockhash();
                let tx = Transaction::new_signed_with_payer(
                    &[
                        heap_ix(),
                        ComputeBudgetInstruction::set_compute_unit_limit(CU_LIMIT as u32),
                        Instruction {
                            program_id: h.env.program_id,
                            accounts: vec![
                                AccountMeta::new(h.env.payer.pubkey(), true),
                                AccountMeta::new(h.env.market, false),
                                AccountMeta::new(h.portfolios[0], false),
                                AccountMeta::new(h.portfolios[1], false),
                            ],
                            data: ProgInstruction::ForceCloseAbandonedAsset {
                                asset_index: LIVE as u16,
                                now_slot: h.slot,
                                close_q: chunk,
                            }
                            .encode(),
                        },
                    ],
                    Some(&h.env.payer.pubkey()),
                    &[&h.env.payer],
                    h.env.svm.latest_blockhash(),
                );
                assert_eq!(tx.message.header.num_required_signatures, 1);
                let bytes = bincode::serialize(&tx).unwrap().len();
                assert!(bytes <= 1232);
                max_packet = max_packet.max(bytes);
                let cu = h
                    .env
                    .svm
                    .send_transaction(tx)
                    .expect("reserved last domain materializes through keeper force-close")
                    .compute_units_consumed;
                assert_cu_within("Recovery latent force-close", cu, CU_LIMIT);
                max_force_cu = max_force_cu.max(cu);
                remaining -= chunk;
                h.positions[LIVE] = -direction * remaining as i128;
                assert_eq!(h.source_claims(0), h.claims.map(|c| c * BOUND_SCALE));
                assert!(h.source_claims(0).iter().all(|c| *c > 0));
                let account = h.env.portfolio_state(h.portfolios[0]);
                for source in retained.iter().filter(|s| s.is_occupied()) {
                    assert_eq!(
                        account
                            .source_domains
                            .iter()
                            .find(|s| s.is_occupied() && s.domain == source.domain),
                        Some(source)
                    );
                }
                assert_eq!(custody.map(|p| h.env.svm.get_account(&p)), custody_before);
                h.assert_accounting();
                census(&h);
                steps += 1;
                force_calls += 1;
            }
            assert_eq!(steps, if split { 2 } else { 1 });
            let gain = h.claims.iter().sum::<u128>();
            let payouts = [CAPITAL + gain, CAPITAL - gain];
            let frames = h.portfolios.map(|p| h.env.svm.get_account(&p));
            let cu = h.env.resolve();
            assert_cu_within("Recovery full-source resolution", cu, CU_LIMIT);
            assert_eq!(h.env.market_state().1.mode, MarketModeV16::Resolved);
            assert_eq!(h.portfolios.map(|p| h.env.svm.get_account(&p)), frames);
            census(&h);
            h.slot = h.env.market_state().1.resolved_slot + OWNER_WINDOW;
            h.env.svm.warp_to_slot(h.slot);
            let mut calls = [0; 2];
            for actor in order {
                while !resolved_portfolio_is_terminal(&h.env, h.portfolios[actor]) {
                    assert!(calls[actor] < DOMAINS + 1, "bounded full-source payout");
                    let (cu, bytes) = terminal_step(&mut h, actor);
                    max_terminal_cu = max_terminal_cu.max(cu);
                    max_packet = max_packet.max(bytes);
                    calls[actor] += 1;
                    terminal_calls += 1;
                }
                assert_eq!(h.env.token_amount(h.tokens[actor]) as u128, payouts[actor]);
            }
            assert_eq!(calls, [DOMAINS, 1]);
            for actor in order {
                h.env.svm.expire_blockhash();
                let cu = h
                    .env
                    .close_portfolio_with_cu(&h.owners[actor], h.portfolios[actor]);
                assert_cu_within(
                    "Recovery full-source portfolio deletion",
                    cu,
                    CUSTODY_CU_LIMIT,
                );
                let closed = h.env.svm.get_account(&h.portfolios[actor]).unwrap();
                assert!(closed.data.is_empty());
                assert_eq!(closed.lamports, 0);
                census(&h);
            }
            let group = h.env.market_state().1;
            assert_eq!(
                (group.c_tot, group.vault, group.materialized_portfolio_count),
                (0, 0, 0)
            );
            for d in 0..DOMAINS {
                assert_eq!(group.source_credit[d].positive_claim_bound_num, 0);
                assert_eq!(
                    group.source_backing_buckets[d].fresh_unliened_backing_num,
                    0
                );
            }
            assert!(group
                .assets
                .iter()
                .all(|a| a.oi_eff_long_q == 0 && a.oi_eff_short_q == 0));
            worlds += 1;
            println!("INV-028 Recovery latent capacity: direction={direction}, split={split}, force_calls={steps}, terminal_calls={calls:?}, payouts={payouts:?}");
        }
    }
    assert_eq!(worlds, 4);
    assert_eq!(force_calls, 6);
    println!("INV-028 Recovery latent capacity: worlds={worlds}, force_calls={force_calls}, terminal_calls={terminal_calls}, max_force_cu={max_force_cu}, max_terminal_cu={max_terminal_cu}, max_packet={max_packet}");
}
