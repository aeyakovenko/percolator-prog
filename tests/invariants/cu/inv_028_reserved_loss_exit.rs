//! Row 423: admission preserves historical backing for later loss settlement and exit.
//! Provider surplus leaves before the loss; no refill or senior owner capital funds exit.
//! Live loss consumption leaves residual custody, realized by signed Resolved exits.

use super::*;

const EXIT_UNITS: i128 = 25;
const LOSS: u128 = EXIT_UNITS as u128 * 10;
const REMAINING_CLAIM: u128 = 3_000 - LOSS;

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
    assert_certified_engine_pin("INV-028 admission resources through loss and Resolved exit");
    let mut calls = 0;
    let mut rollbacks = 0;
    let mut settlement_calls = 0;
    let mut terminal_calls = 0;
    let mut max_cu = 0;
    let mut max_packet = 0;
    let mut max_cleanup = 0;
    for split in [false, true] {
        let order = if split { [0, 1] } else { [1, 0] };
        let mut s = funded_history(order);
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

        s.submit(&[s.trade(-q, PRICE - movement, split)], &[0, 1], None);
        let mut cleanup = 0;
        for actor in order {
            assert_eq!(
                percolator::active_bitmap_count_ones(active_bitmap(
                    &s.h.env.portfolio_state(s.h.portfolios[actor])
                )),
                0
            );
        }
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
        max_cleanup = max_cleanup.max(cleanup);
        assert_loss_resources(&s, LOSS);
        // Provider principal was never spent: historical claim backing funded the loss.
        for domain in 0..2 * LIVE {
            let amount = s.provider_remaining[domain];
            if amount > 0 {
                s.submit(&[s.provider_withdraw(domain, amount)], &[], None);
                s.provider_remaining[domain] = 0;
            }
        }
        assert_eq!(s.h.env.token_amount(s.provider) as u128, PROVIDER_TOTAL);
        assert_loss_resources(&s, LOSS);
        let cu = s.h.env.resolve();
        assert_cu_within("reserved loss resolution", cu, CU_LIMIT);
        s.max_cu = s.max_cu.max(cu);
        s.calls += 1;
        s.audit();
        // Realize historical backing before paying the new residual-only junior claim.
        for actor in [0, 1] {
            let close_resolved = s.instruction(
                ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                },
                vec![
                    AccountMeta::new(s.h.owners[actor].pubkey(), true),
                    AccountMeta::new(s.h.env.market, false),
                    AccountMeta::new(s.h.portfolios[actor], false),
                    AccountMeta::new(s.h.tokens[actor], false),
                    AccountMeta::new(s.h.env.vault, false),
                    AccountMeta::new_readonly(s.h.env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
            );
            let rank = |s: &ReservedExit| {
                let account = s.h.env.portfolio_state(s.h.portfolios[actor]);
                (
                    account
                        .source_domains
                        .iter()
                        .filter(|source| source.is_occupied())
                        .count(),
                    account.capital.get() + account.pnl.get().max(0) as u128,
                )
            };
            for _ in 0..DOMAINS + 2 {
                if resolved_portfolio_is_terminal(&s.h.env, s.h.portfolios[actor]) {
                    break;
                }
                let before = rank(&s);
                let peer = s.h.env.svm.get_account(&s.h.portfolios[1 - actor]);
                let vault_before = s.h.env.token_amount(s.h.env.vault);
                let wallet_before = s.h.env.token_amount(s.h.tokens[actor]);
                s.submit(&[close_resolved.clone()], &[actor], None);
                terminal_calls += 1;
                assert!(
                    rank(&s) < before,
                    "terminal exit consumes source or payment debt: actor={actor}, before={before:?}, after={:?}", rank(&s)
                );
                assert_eq!(s.h.env.svm.get_account(&s.h.portfolios[1 - actor]), peer);
                assert_eq!(
                    vault_before - s.h.env.token_amount(s.h.env.vault),
                    s.h.env.token_amount(s.h.tokens[actor]) - wallet_before
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
    assert_eq!(rollbacks, 4);
    println!("INV-028 reserved loss exit: worlds=2, suffix_calls={calls}, exact_rollbacks={rollbacks}, settlement_calls={settlement_calls}, terminal_calls={terminal_calls}, max_cu={max_cu}, headroom={}, max_packet={max_packet}, max_cleanup={max_cleanup}", CU_LIMIT - max_cu);
}
