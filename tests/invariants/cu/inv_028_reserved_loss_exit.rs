//! Row 423: admission preserves historical backing for later loss settlement and exit.
//! Provider surplus leaves before the loss; no refill or senior owner capital funds exit.
//! Live loss consumption leaves residual custody, realized by signed Resolved exits.
//! Junior-first payout retains a partial receipt until historical source realization
//! raises its rate; a keeper-only top-up must survive a rejected withdrawal suffix.
//! Recovery also carries obsolete historical liens into the partial-receipt interval.

use super::*;

const EXIT_UNITS: i128 = 25;
const LOSS: u128 = EXIT_UNITS as u128 * 10;
const REMAINING_CLAIM: u128 = 3_000 - LOSS;
const EARLY_JUNIOR_PAYOUT: u128 = LOSS * LOSS / (REMAINING_CLAIM + LOSS);

fn assert_loss_resources(s: &ReservedExit, loss: u128) {
    let group = s.h.env.market_state().1;
    let accounts = s.h.portfolios.map(|p| s.h.env.portfolio_state(p));
    let claims = s.claims();
    let mut burned = 0;
    let mut consumed_domains = 0;
    for (domain, initial) in s.h.claims.iter().copied().enumerate().take(2 * LIVE) {
        let remaining = claims[domain];
        let debit = initial * BOUND_SCALE - remaining;
        let source = group.source_credit[domain];
        let bucket = group.source_backing_buckets[domain];
        assert_eq!(source.exact_positive_claim_num, remaining);
        assert_eq!(source.positive_claim_bound_num, remaining);
        assert_eq!(source.spent_backing_num, debit);
        assert_eq!(source.provider_receivable_num, debit);
        assert_eq!(bucket.consumed_liened_backing_num, debit);
        assert_eq!(
            bucket.fresh_unliened_backing_num + bucket.valid_liened_backing_num,
            remaining + s.provider_remaining[domain] * BOUND_SCALE,
            "burned historical backing cannot also fund the remaining exit claim"
        );
        burned += debit;
        consumed_domains += usize::from(debit > 0);
    }
    assert_eq!(burned, loss * BOUND_SCALE);
    if loss == LOSS {
        assert!(
            consumed_domains >= 2,
            "loss uses several historical domains"
        );
    }
    let new_domain = 2 * LIVE;
    let peer_claims: Vec<_> = accounts[1]
        .source_domains
        .iter()
        .filter(|source| source.is_occupied())
        .collect();
    assert_eq!(peer_claims.len(), usize::from(loss > 0));
    if loss > 0 {
        assert_eq!(peer_claims[0].domain.get() as usize, new_domain);
        assert_eq!(
            peer_claims[0].source_claim_bound_num.get(),
            loss * BOUND_SCALE
        );
    }
    assert_eq!(
        group.source_credit[new_domain].exact_positive_claim_num,
        loss * BOUND_SCALE
    );
    assert_eq!(
        group.source_backing_buckets[new_domain].fresh_unliened_backing_num, 0,
        "claim-funded loss is residual custody, not a new principal-backed source deposit"
    );
    let backing = group
        .source_backing_buckets
        .iter()
        .map(|bucket| bucket.fresh_unliened_backing_num + bucket.valid_liened_backing_num)
        .sum::<u128>();
    assert_eq!(
        (group.vault - group.c_tot) * BOUND_SCALE - backing,
        loss * BOUND_SCALE
    );
    assert_eq!(accounts[0].capital.get(), 0);
    assert_eq!(accounts[0].pnl.get(), (3_000 - loss) as i128);
    assert_eq!(accounts[1].capital.get(), CAPITAL - 3_000);
    assert_eq!(accounts[1].pnl.get(), loss as i128);
    assert_eq!(group.c_tot, CAPITAL - 3_000);
    assert_eq!(s.h.env.token_amount(s.h.tokens[0]) as u128, CAPITAL);
    assert_eq!(s.h.env.token_amount(s.h.tokens[1]), 0);
    s.audit();
}

#[test]
fn v16_program_admission_preserves_backing_for_loss_and_bounded_resolved_exit() {
    run_reserved_loss_exit(false);
}

#[test]
fn v16_program_recovery_force_close_preserves_historical_liens_and_partial_receipt_exit() {
    run_reserved_loss_exit(true);
}

fn run_reserved_loss_exit(recovery: bool) {
    // Revalidate this witness locally without recertifying the older global roster.
    assert!(include_str!("../../../Cargo.lock").contains(
        "git+https://github.com/Commoneffort/percolator?rev=a87d5057e94ccbb6446aeb6c7d0096d62e25cf96#\
         a87d5057e94ccbb6446aeb6c7d0096d62e25cf96"
    ));
    let mut calls = 0;
    let mut rollbacks = 0;
    let mut settlement_calls = 0;
    let mut terminal_calls = 0;
    let mut max_cu = 0;
    let mut max_packet = 0;
    let mut max_cleanup = 0;
    let mut partial_receipts = 0;
    let mut keeper_topups = 0;
    let mut force_calls = 0;
    let mut max_force_cu = 0;
    let cases: &[(bool, bool)] = if recovery {
        &[(false, true), (true, true)]
    } else {
        &[(false, false), (true, false), (false, true), (true, true)]
    };
    for &(split, junior_first) in cases {
        let order = if split { [0, 1] } else { [1, 0] };
        let mut s = funded_history(order);
        if recovery {
            s.h.env.configure_permissionless_resolve_with_cu(1_000, 5);
        }
        s.submit(&[s.withdraw(0, CAPITAL)], &[0], None);
        for actor in order {
            s.submit(&[s.crank(actor)], &[], None);
        }
        let q = EXIT_UNITS * POS_SCALE as i128;
        s.submit(&[s.trade(q, PRICE, split)], &[0, 1], None);
        s.frontier(q);
        assert_eq!(
            s.lien(),
            EXIT_UNITS as u128 * u128::from(PRICE) * BOUND_SCALE
        );
        assert_eq!(
            s.claims().iter().sum::<u128>() - s.lien(),
            500 * BOUND_SCALE
        );
        s.return_provider(false);
        assert_eq!(s.h.env.token_amount(s.provider), 3_900);
        assert_loss_resources(&s, 0);

        // The requested total notional exceeds all 3,000 atoms of historical claim support.
        s.submit(
            &[s.trade(6 * POS_SCALE as i128, PRICE, split)],
            &[0, 1],
            Some((2, PercolatorError::EngineLockActive)),
        );
        assert_loss_resources(&s, 0);
        let custody = [
            s.h.env.mint,
            s.h.env.vault,
            s.h.tokens[0],
            s.h.tokens[1],
            s.provider,
        ]
        .map(|key| s.h.env.svm.get_account(&key));
        let mut movement = 0;
        let parts: &[u64] = if split { &[4, 6] } else { &[10] };
        for &part in parts {
            movement += part;
            s.h.slot += part;
            s.h.env.svm.warp_to_slot(s.h.slot);
            let cu =
                s.h.env
                    .push_auth_mark_for_asset_as_admin(LIVE as u16, s.h.slot, PRICE - movement);
            assert_cu_within("reserved loss mark", cu, CU_LIMIT);
            s.max_cu = s.max_cu.max(cu);
            s.calls += 1;
            let loss = EXIT_UNITS as u128 * u128::from(movement);
            for actor in order {
                let rank = |s: &ReservedExit| {
                    let group = s.h.env.market_state().1;
                    let account = s.h.env.portfolio_state(s.h.portfolios[actor]);
                    u128::from(s.h.slot - group.assets[LIVE].slot_last)
                        + account.pnl.get().abs_diff(if actor == 0 {
                            (3_000 - loss) as i128
                        } else {
                            loss as i128
                        })
                };
                for _ in 0..4 {
                    let before = rank(&s);
                    if before == 0 {
                        break;
                    }
                    let peer = s.h.env.svm.get_account(&s.h.portfolios[1 - actor]);
                    s.submit(&[s.crank(actor)], &[], None);
                    settlement_calls += 1;
                    assert!(
                        rank(&s) < before,
                        "each settlement consumes pending loss/mark work"
                    );
                    assert_eq!(s.h.env.svm.get_account(&s.h.portfolios[1 - actor]), peer);
                }
                assert_eq!(rank(&s), 0);
            }
            assert_loss_resources(&s, loss);
            assert_eq!(
                s.lien(),
                EXIT_UNITS as u128 * u128::from(PRICE) * BOUND_SCALE
            );
            assert_eq!(
                s.claims().iter().sum::<u128>() - s.lien(),
                (500 - loss) * BOUND_SCALE
            );
            for actor in [0, 1] {
                let account = s.h.env.portfolio_state(s.h.portfolios[actor]);
                assert_eq!(
                    active_leg_for_asset(&account, LIVE).basis_pos_q,
                    q * if actor == 0 { 1 } else { -1 }
                );
            }
            assert_eq!(
                [
                    s.h.env.mint,
                    s.h.env.vault,
                    s.h.tokens[0],
                    s.h.tokens[1],
                    s.provider
                ]
                .map(|key| s.h.env.svm.get_account(&key)),
                custody,
                "public settlement uses pre-existing custody with no top-up"
            );
        }

        if recovery {
            let portfolios = s.h.portfolios.map(|p| s.h.env.svm.get_account(&p));
            let claims = s.claims();
            let lien_count =
                s.h.env
                    .portfolio_state(s.h.portfolios[0])
                    .source_domains
                    .iter()
                    .filter(|source| source.source_lien_counterparty_backing_num.get() > 0)
                    .count();
            assert!(
                lien_count > 1,
                "Recovery starts with simultaneous historical liens"
            );
            let admin = Keypair::from_bytes(&s.h.env.admin.to_bytes()).unwrap();
            let cu =
                s.h.env
                    .try_shutdown_asset_with_authority(&admin, LIVE as u16, s.h.slot)
                    .expect("claim-funded risk can enter Recovery");
            assert_cu_within("reserved loss shutdown", cu, CU_LIMIT);
            s.max_cu = s.max_cu.max(cu);
            s.calls += 1;
            assert_eq!(
                s.h.portfolios.map(|p| s.h.env.svm.get_account(&p)),
                portfolios
            );
            assert_eq!(
                s.h.env.market_state().1.assets[LIVE].lifecycle,
                AssetLifecycleV16::Recovery
            );
            assert_loss_resources(&s, LOSS);

            s.h.slot += 5;
            s.h.env.svm.warp_to_slot(s.h.slot);
            let chunks: &[u128] = if split { &[10, 15] } else { &[25] };
            let mut remaining = q as u128;
            for &units in chunks {
                let close_q = units * POS_SCALE;
                let force_close = s.instruction(
                    ProgInstruction::ForceCloseAbandonedAsset {
                        asset_index: LIVE as u16,
                        now_slot: s.h.slot,
                        close_q,
                    },
                    vec![
                        AccountMeta::new(s.h.env.payer.pubkey(), true),
                        AccountMeta::new(s.h.env.market, false),
                        AccountMeta::new(s.h.portfolios[0], false),
                        AccountMeta::new(s.h.portfolios[1], false),
                    ],
                );
                assert!(force_close
                    .accounts
                    .iter()
                    .filter(|meta| meta.is_signer)
                    .all(|meta| meta.pubkey == s.h.env.payer.pubkey()));
                // Measure this route separately while retaining the history-wide CU peak.
                let previous_peak = s.max_cu;
                s.max_cu = 0;
                s.submit(&[force_close], &[], None);
                max_force_cu = max_force_cu.max(s.max_cu);
                s.max_cu = s.max_cu.max(previous_peak);
                force_calls += 1;
                remaining -= close_q;
                let group = s.h.env.market_state().1;
                assert_eq!(group.assets[LIVE].lifecycle, AssetLifecycleV16::Recovery);
                assert_eq!(group.assets[LIVE].oi_eff_long_q, remaining);
                assert_eq!(group.assets[LIVE].oi_eff_short_q, remaining);
                for actor in [0, 1] {
                    let account = s.h.env.portfolio_state(s.h.portfolios[actor]);
                    assert_eq!(has_active_leg_for_asset(&account, LIVE), remaining > 0);
                    if remaining > 0 {
                        assert_eq!(
                            active_leg_for_asset(&account, LIVE).basis_pos_q,
                            remaining as i128 * if actor == 0 { 1 } else { -1 }
                        );
                    }
                }
                assert_eq!(
                    s.claims(),
                    claims,
                    "force-close preserves booked historical value"
                );
                assert_loss_resources(&s, LOSS);
                if remaining > 0 {
                    assert!(s.lien() > 0, "partial Recovery exit retains risk backing");
                }
                assert_eq!(
                    [
                        s.h.env.mint,
                        s.h.env.vault,
                        s.h.tokens[0],
                        s.h.tokens[1],
                        s.provider,
                    ]
                    .map(|key| s.h.env.svm.get_account(&key)),
                    custody,
                    "Recovery exit uses existing custody without either owner"
                );
            }
            assert_eq!(remaining, 0);
        } else {
            s.submit(&[s.trade(-q, PRICE - movement, split)], &[0, 1], None);
        }
        let mut cleanup = 0;
        for actor in order {
            assert_eq!(
                percolator::active_bitmap_count_ones(active_bitmap(
                    &s.h.env.portfolio_state(s.h.portfolios[actor])
                )),
                0
            );
        }
        if !recovery {
            for _ in 0..2 * LIVE {
                let before = s.lien();
                if before == 0 {
                    break;
                }
                s.submit(&[s.crank(0)], &[], None);
                cleanup += 1;
                assert!(
                    s.lien() < before,
                    "bounded cleanup releases obsolete reservations"
                );
            }
            assert_eq!(s.lien(), 0);
        } else {
            assert!(
                s.lien() > 0,
                "obsolete Recovery liens survive into resolution"
            );
        }
        max_cleanup = max_cleanup.max(cleanup);
        assert_loss_resources(&s, LOSS);
        // Provider principal was never spent: historical claim backing funded the loss.
        if !recovery {
            for domain in 0..2 * LIVE {
                let amount = s.provider_remaining[domain];
                if amount > 0 {
                    s.submit(&[s.provider_withdraw(domain, amount)], &[], None);
                    s.provider_remaining[domain] = 0;
                }
            }
            assert_eq!(s.h.env.token_amount(s.provider) as u128, PROVIDER_TOTAL);
        }
        assert_loss_resources(&s, LOSS);
        let cu = s.h.env.resolve();
        assert_cu_within("reserved loss resolution", cu, CU_LIMIT);
        s.max_cu = s.max_cu.max(cu);
        s.calls += 1;
        s.audit();
        let payout = |s: &ReservedExit, actor: usize, topup: bool| {
            s.instruction(
                if topup {
                    ProgInstruction::ClaimResolvedPayoutTopup
                } else {
                    ProgInstruction::CloseResolved {
                        fee_rate_per_slot: 0,
                    }
                },
                vec![
                    AccountMeta::new_readonly(s.h.owners[actor].pubkey(), !topup),
                    AccountMeta::new(s.h.env.market, false),
                    AccountMeta::new(s.h.portfolios[actor], false),
                    AccountMeta::new(s.h.tokens[actor], false),
                    AccountMeta::new(s.h.env.vault, false),
                    AccountMeta::new_readonly(s.h.env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
            )
        };
        let payout_order = if junior_first { [1, 0] } else { [0, 1] };
        let economic_frame = |s: &ReservedExit| {
            [
                s.h.env.market,
                s.h.env.vault,
                s.h.env.mint,
                s.h.portfolios[0],
                s.h.portfolios[1],
                s.h.tokens[0],
                s.h.tokens[1],
                s.provider,
            ]
            .map(|key| s.h.env.svm.get_account(&key))
        };
        for actor in payout_order {
            let close_resolved = payout(&s, actor, false);
            let rank = |s: &ReservedExit| {
                let account = s.h.env.portfolio_state(s.h.portfolios[actor]);
                (
                    account
                        .source_domains
                        .iter()
                        .map(|source| source.source_lien_counterparty_backing_num.get())
                        .sum::<u128>(),
                    account
                        .source_domains
                        .iter()
                        .filter(|source| source.is_occupied())
                        .count(),
                    account.capital.get() + account.pnl.get().max(0) as u128,
                )
            };
            for _ in 0..DOMAINS + 2 {
                let account = s.h.env.portfolio_state(s.h.portfolios[actor]);
                if account.capital.get() == 0
                    && account.pnl.get() == 0
                    && account
                        .source_domains
                        .iter()
                        .all(|source| !source.is_occupied())
                {
                    break;
                }
                let before = rank(&s);
                let frame = [s.h.portfolios[1 - actor], s.h.tokens[1 - actor], s.provider]
                    .map(|key| s.h.env.svm.get_account(&key));
                let vault_before = s.h.env.token_amount(s.h.env.vault);
                let wallet_before = s.h.env.token_amount(s.h.tokens[actor]);
                s.submit(&[close_resolved.clone()], &[actor], None);
                terminal_calls += 1;
                assert!(
                    rank(&s) < before,
                    "terminal exit consumes lien, source or payment debt: actor={actor}, before={before:?}, after={:?}", rank(&s)
                );
                assert_eq!(
                    [s.h.portfolios[1 - actor], s.h.tokens[1 - actor], s.provider].map(|key| s
                        .h
                        .env
                        .svm
                        .get_account(&key)),
                    frame
                );
                assert_eq!(
                    vault_before - s.h.env.token_amount(s.h.env.vault),
                    s.h.env.token_amount(s.h.tokens[actor]) - wallet_before
                );
                let ceiling = if actor == 0 {
                    CAPITAL + REMAINING_CLAIM
                } else {
                    CAPITAL - REMAINING_CLAIM
                };
                assert!(u128::from(s.h.env.token_amount(s.h.tokens[actor])) <= ceiling);
            }
            assert_eq!(rank(&s), (0, 0, 0));
            let receipt = resolved_receipt(&s.h.env.portfolio_state(s.h.portfolios[actor]));
            if actor == 0 {
                assert_eq!(receipt, ResolvedPayoutReceiptV16::EMPTY);
                assert_eq!(
                    s.h.env.token_amount(s.h.tokens[0]) as u128,
                    CAPITAL + REMAINING_CLAIM
                );
            } else {
                let paid = if junior_first {
                    EARLY_JUNIOR_PAYOUT
                } else {
                    LOSS
                };
                assert_eq!(
                    receipt,
                    ResolvedPayoutReceiptV16 {
                        present: true,
                        prior_bound_contribution_num: LOSS * BOUND_SCALE,
                        live_released_face_at_receipt: 0,
                        terminal_positive_claim_face: LOSS,
                        paid_effective: paid,
                        finalized: !junior_first,
                    }
                );
                assert_eq!(
                    s.h.env.token_amount(s.h.tokens[1]) as u128,
                    CAPITAL - 3_000 + paid
                );
                let ledger = s.h.env.market_state().1.resolved_payout_ledger;
                let outstanding = if junior_first { REMAINING_CLAIM } else { 0 };
                assert_eq!(ledger.snapshot_residual, LOSS);
                assert_eq!(ledger.terminal_claim_exact_receipts_num, LOSS * BOUND_SCALE);
                assert_eq!(
                    ledger.terminal_claim_bound_unreceipted_num,
                    outstanding * BOUND_SCALE
                );
                assert_eq!(ledger.current_payout_rate_num, LOSS * BOUND_SCALE);
                assert_eq!(
                    ledger.current_payout_rate_den,
                    (LOSS + outstanding) * BOUND_SCALE
                );
                if junior_first {
                    if recovery {
                        let account = s.h.env.portfolio_state(s.h.portfolios[0]);
                        let liens = account
                            .source_domains
                            .iter()
                            .filter(|source| source.source_lien_counterparty_backing_num.get() > 0)
                            .count();
                        assert!(liens > 1, "partial receipt coexists with historical liens");
                        assert!(s.provider_remaining.iter().sum::<u128>() > 0);
                        println!("Recovery receipt overlap: split={split}, liens={liens}, lien_num={}, paid={paid}", s.lien());
                    }
                    assert!(!resolved_portfolio_is_terminal(&s.h.env, s.h.portfolios[1]));
                    let frame = economic_frame(&s);
                    s.submit(&[payout(&s, 1, true)], &[], None);
                    assert_eq!(
                        economic_frame(&s),
                        frame,
                        "early top-up preserves the pending receipt"
                    );
                    partial_receipts += 1;
                }
            }
        }
        assert_eq!(s.lien(), 0);
        let ledger = s.h.env.market_state().1.resolved_payout_ledger;
        assert_eq!(ledger.snapshot_residual, LOSS);
        assert_eq!(ledger.terminal_claim_bound_unreceipted_num, 0);
        assert_eq!(ledger.current_payout_rate_num, LOSS * BOUND_SCALE);
        assert_eq!(ledger.current_payout_rate_den, LOSS * BOUND_SCALE);
        for actor in payout_order {
            if !resolved_portfolio_is_terminal(&s.h.env, s.h.portfolios[actor]) {
                assert!(junior_first && actor == 1);
                let before = resolved_receipt(&s.h.env.portfolio_state(s.h.portfolios[actor]));
                assert_eq!(before.paid_effective, EARLY_JUNIOR_PAYOUT);
                let topup = payout(&s, actor, true);
                assert!(topup.accounts.iter().all(|meta| !meta.is_signer));
                // Resolved ordinary withdrawal is forbidden even for the correct owner.
                // Its rejection must restore the preceding successful SPL receipt payout.
                s.submit(
                    &[topup.clone(), s.withdraw(0, 1)],
                    &[0],
                    Some((3, PercolatorError::EngineLockActive)),
                );
                assert_eq!(
                    resolved_receipt(&s.h.env.portfolio_state(s.h.portfolios[actor])),
                    before
                );
                let frame = [s.h.portfolios[0], s.h.tokens[0], s.provider]
                    .map(|key| s.h.env.svm.get_account(&key));
                let vault = s.h.env.token_amount(s.h.env.vault);
                let wallet = s.h.env.token_amount(s.h.tokens[actor]);
                s.submit(&[topup.clone()], &[], None);
                terminal_calls += 1;
                keeper_topups += 1;
                let receipt = resolved_receipt(&s.h.env.portfolio_state(s.h.portfolios[actor]));
                assert_eq!(
                    receipt,
                    ResolvedPayoutReceiptV16 {
                        paid_effective: LOSS,
                        finalized: true,
                        ..before
                    }
                );
                assert_eq!(
                    vault - s.h.env.token_amount(s.h.env.vault),
                    (LOSS - EARLY_JUNIOR_PAYOUT) as u64
                );
                assert_eq!(
                    s.h.env.token_amount(s.h.tokens[actor]) - wallet,
                    (LOSS - EARLY_JUNIOR_PAYOUT) as u64
                );
                assert_eq!(
                    [s.h.portfolios[0], s.h.tokens[0], s.provider].map(|key| s
                        .h
                        .env
                        .svm
                        .get_account(&key)),
                    frame
                );
                let frame = economic_frame(&s);
                s.submit(&[topup], &[], None);
                assert_eq!(
                    economic_frame(&s),
                    frame,
                    "repeated top-up cannot pay twice"
                );
            }
            assert!(resolved_portfolio_is_terminal(
                &s.h.env,
                s.h.portfolios[actor]
            ));
            let close = s.instruction(
                s.h.env.close_portfolio_ix(s.h.portfolios[actor]),
                vec![
                    AccountMeta::new(s.h.owners[actor].pubkey(), true),
                    AccountMeta::new(s.h.env.market, false),
                    AccountMeta::new(s.h.portfolios[actor], false),
                ],
            );
            s.submit(&[close], &[actor], None);
            assert!(s
                .h
                .env
                .svm
                .get_account(&s.h.portfolios[actor])
                .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
        }
        if recovery {
            for domain in 0..2 * LIVE {
                let amount = s.provider_remaining[domain];
                if amount > 0 {
                    s.submit(&[s.provider_withdraw(domain, amount)], &[], None);
                    s.provider_remaining[domain] = 0;
                }
            }
            assert_eq!(s.h.env.token_amount(s.provider) as u128, PROVIDER_TOTAL);
        }
        assert_eq!(
            s.h.tokens.map(|t| s.h.env.token_amount(t) as u128),
            [CAPITAL + REMAINING_CLAIM, CAPITAL - REMAINING_CLAIM]
        );
        let group = s.h.env.market_state().1;
        assert_eq!(
            (
                group.vault,
                group.c_tot,
                group.insurance,
                group.materialized_portfolio_count
            ),
            (0, 0, 0, 0)
        );
        for domain in 0..DOMAINS {
            let source = group.source_credit[domain];
            let bucket = group.source_backing_buckets[domain];
            assert_eq!(
                (
                    source.exact_positive_claim_num,
                    source.positive_claim_bound_num,
                    source.fresh_reserved_backing_num,
                    source.valid_liened_backing_num
                ),
                (0, 0, 0, 0)
            );
            assert_eq!(source.spent_backing_num, source.provider_receivable_num);
            assert_eq!(source.spent_backing_num, bucket.consumed_liened_backing_num);
            assert_eq!(
                source.spent_backing_num,
                s.h.claims[domain] * BOUND_SCALE,
                "historical face is spent exactly once across loss and final realization"
            );
        }
        calls += s.calls;
        rollbacks += s.rejected;
        max_cu = max_cu.max(s.max_cu);
        max_packet = max_packet.max(s.max_packet);
    }
    assert_eq!(rollbacks, if recovery { 6 } else { 10 });
    assert_eq!((partial_receipts, keeper_topups), (2, 2));
    assert_eq!(force_calls, if recovery { 3 } else { 0 });
    println!("INV-028 reserved loss exit: recovery={recovery}, worlds={}, partial_receipts={partial_receipts}, keeper_topups={keeper_topups}, force_calls={force_calls}, max_force_cu={max_force_cu}, suffix_calls={calls}, exact_rollbacks={rollbacks}, settlement_calls={settlement_calls}, terminal_calls={terminal_calls}, max_cu={max_cu}, headroom={}, max_packet={max_packet}, max_cleanup={max_cleanup}", cases.len(), CU_LIMIT - max_cu);
}
