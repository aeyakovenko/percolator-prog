//! INV-039/048/076/081, rows 419/435: expire backing AFTER mixed-role K
//! settlement consumes partial support, but BEFORE pending B and source conversion.
//! Unlike unsettled expiry (debt charged at par) or fractional retirement (claims
//! already converted), expiry here must preserve the earlier support face burn.
//! The mixed owner's remaining face becomes a receipt without reviving that
//! discount or moving its pending loss/debt to the peer. Fresh conversion is the
//! control. The underfunded ADL receipt product expires the peer's backing after
//! the mixed receipt exists; here the mixed owner's own source expires first.
//! This finite comparison does not establish generic INV-086 equivalence.

use super::*;

const DEBT: u128 = 36_000;
const BURNED_FACE: u128 = DEBT * GAIN / DEPOSITS[1];
const RECEIPT_FACE: u128 = GAIN - BURNED_FACE - RESIDUAL;
const CREDITOR_EXPIRY: u64 = 1_005;

fn check(world: &AttributionWorld, book: &Book, expired: bool) {
    let env = &world.env;
    let group = env.market_state().1;
    let side = usize::from(book.sign < 0);
    let domain = book.assets[0] * 2 + 1 - side;
    let target_b = RESIDUAL * SOCIAL_LOSS_DEN / POS_SCALE;
    let mut oi = [[0u128; 2]; 3];
    let mut weights = oi;
    let mut stored = [[0u64; 2]; 3];
    let mut pending = stored;
    let mut sources = [0u128; 6];
    let mut portfolios = Vec::new();
    let mut paid = [0u128; 5];
    let mut charged = false;
    let mut converted = false;

    assert_eq!(group.mode, MarketModeV16::Resolved);
    for (actor, owner) in world.actors.iter().enumerate() {
        let p = env.portfolio_state(owner.portfolio);
        assert_eq!(p.owner, owner.owner.pubkey().to_bytes());
        paid[actor] = env.token_amount(owner.token) as u128;
        assert!(paid[actor] <= book.payouts()[actor]);
        let legs: Vec<_> = p
            .legs
            .iter()
            .map(|l| l.try_to_runtime().unwrap())
            .filter(|l| l.active)
            .collect();
        for l in &legs {
            let asset = l.asset_index as usize;
            let s = usize::from(l.side == SideV16::Short);
            let creditor = asset == book.assets[0];
            assert!(actor == 0 || (actor == 2 && !creditor));
            assert_eq!(s, side ^ usize::from(actor == 0 && !creditor));
            assert_eq!(
                l.loss_weight,
                if creditor { POS_SCALE } else { 2 * POS_SCALE }
            );
            if creditor {
                assert_eq!(l.basis_pos_q, 0);
                assert!(l.b_snap == 0 || l.b_snap == target_b);
            } else {
                assert_eq!(asset, book.assets[1]);
                assert_eq!(
                    l.k_snap,
                    if actor == 0 {
                        book.debt_k
                    } else {
                        -book.debt_k
                    }
                );
                assert_eq!(l.b_snap, 0, "the creditor's B charge cannot move domains");
            }
            assert_eq!(l.b_rem, 0);
            oi[asset][s] += l.basis_pos_q.unsigned_abs();
            weights[asset][s] += l.loss_weight;
            stored[asset][s] += 1;
            pending[asset][s] += u64::from(l.basis_pos_q == 0);
        }
        if actor == 0 {
            charged = legs
                .iter()
                .find(|l| l.asset_index as usize == book.assets[0])
                .is_none_or(|l| l.b_snap == target_b);
            converted = p.source_domains.iter().all(|s| !s.is_occupied());
            if !charged {
                assert_eq!(paid[actor], 0, "pending loss cannot authorize payment");
                assert!(!converted);
            }
        }
        let receipt = resolved_receipt(&p);
        let due = if receipt.present {
            assert!(charged && (actor == 2 || (actor == 0 && expired)));
            let face = if actor == 0 { RECEIPT_FACE } else { DEBT };
            assert_eq!(receipt.terminal_positive_claim_face, face);
            assert_eq!(receipt.prior_bound_contribution_num, face * BOUND_SCALE);
            face.checked_sub(receipt.paid_effective).unwrap()
        } else {
            0
        };
        let expected = book.payouts()[actor] + if actor == 0 && !charged { RESIDUAL } else { 0 };
        assert_eq!(
            p.capital.get() as i128 + p.pnl.get() + due as i128 + paid[actor] as i128,
            expected as i128,
            "owner {actor}: expiry cannot restore burned face or reassign debt"
        );
        for s in p.source_domains.iter().filter(|s| s.is_occupied()) {
            assert!(actor == 0 || actor == 2);
            assert_eq!(
                s.domain.get() as usize,
                book.assets[usize::from(actor == 2)] * 2 + 1 - side
            );
            let face = if actor == 0 {
                GAIN - BURNED_FACE - if charged { RESIDUAL } else { 0 }
            } else {
                DEBT
            };
            assert_eq!(s.source_claim_bound_num.get(), face * BOUND_SCALE);
            sources[s.domain.get() as usize] += s.source_claim_bound_num.get();
        }
        verify_close_residual_partition("settled mixed-role expiry", &close_progress(&p)).unwrap();
        portfolios.push(p);
    }
    for asset in 0..3 {
        let a = group.assets[asset];
        assert_eq!([a.a_long, a.a_short], [ADL_ONE; 2]);
        assert_eq!([a.oi_eff_long_q, a.oi_eff_short_q], oi[asset]);
        assert_eq!(
            [a.loss_weight_sum_long, a.loss_weight_sum_short],
            weights[asset]
        );
        assert_eq!(
            [a.stored_pos_count_long, a.stored_pos_count_short],
            stored[asset]
        );
        assert_eq!(
            [
                a.pending_obligation_count_long,
                a.pending_obligation_count_short
            ],
            pending[asset]
        );
        let mut b = [0; 2];
        if asset == book.assets[0] {
            b[side] = target_b;
        }
        assert_eq!([a.b_long_num, a.b_short_num], b);
    }
    let realized = if !expired && converted {
        RECEIPT_FACE
    } else {
        0
    };
    for (d, s) in group.source_credit.iter().enumerate() {
        assert_eq!(s.positive_claim_bound_num, sources[d]);
        assert_eq!(
            s.fresh_reserved_backing_num,
            if d == domain && !expired {
                (DEPOSITS[1] - DEBT - realized) * BOUND_SCALE
            } else {
                0
            }
        );
        let spent = if d == domain {
            (DEBT + realized) * BOUND_SCALE
        } else {
            0
        };
        assert_eq!(s.spent_backing_num, spent);
        assert_eq!(s.provider_receivable_num, spent);
    }
    assert_eq!(group.insurance, 0);
    let vault = env.token_amount(env.vault) as u128;
    assert_eq!(
        vault + paid.iter().sum::<u128>(),
        DEPOSITS.iter().sum::<u128>()
    );
    assert_eq!(
        Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
            .unwrap()
            .supply as u128,
        DEPOSITS.iter().sum::<u128>()
    );
    assert_market_stock_census(
        "settled mixed-role expiry",
        &group,
        &env.svm.get_account(&env.market).unwrap().data,
        &portfolios,
        vault,
    )
    .unwrap();
    assert_reservation_encumbrance_census("settled mixed-role expiry", &group, &portfolios)
        .unwrap();
}

#[test]
fn v16_program_backing_expiry_after_mixed_debt_settlement_preserves_burn_and_receipt() {
    assert_certified_engine_pin("INV-039 settled mixed-role backing expiry");
    assert_eq!((BURNED_FACE, RECEIPT_FACE), (40_000, 140_000));
    let mut peak = 0;
    let mut worlds = 0;
    let mut rollbacks = 0;
    let mut receipt_rollbacks = 0;
    for reverse in [false, true] {
        for assets in [[1, 2], [2, 1]] {
            for order in [[0, 2, 1, 3, 4], [2, 1, 0, 4, 3]] {
                for slot in [CREDITOR_EXPIRY - 1, CREDITOR_EXPIRY, CREDITOR_EXPIRY + 6] {
                    let expired = slot >= CREDITOR_EXPIRY;
                    let (mut world, mut book) = setup(reverse, assets, DEBT);
                    let settle = ProgInstruction::PermissionlessCrank {
                        now_slot: 10,
                        observations: crank_observations_for_assets(&[1, 2]),
                    };
                    peak = peak.max(world.env.crank(world.actors[0].portfolio, settle.clone()));
                    book.check(&world);
                    assert!(book.debt_settled && !book.booked && !book.charged && !book.converted);
                    let mixed = world.env.portfolio_state(world.actors[0].portfolio);
                    assert_eq!(
                        (mixed.capital.get(), mixed.pnl.get()),
                        (DEPOSITS[0], (GAIN - BURNED_FACE) as i128)
                    );
                    // Book the residual while fresh, leaving the mixed owner's B
                    // charge pending. No expired-close preemption is involved.
                    peak = peak.max(world.env.crank(world.actors[1].portfolio, settle));
                    book.check(&world);
                    assert!(book.booked && !book.charged && !book.converted);
                    world.env.resolve();
                    book.check(&world);
                    let domain = assets[0] * 2 + usize::from(!reverse);
                    assert_eq!(
                        world.env.market_state().1.source_backing_buckets[domain].expiry_slot,
                        CREDITOR_EXPIRY
                    );
                    let before = world.frame();
                    world.env.svm.warp_to_slot(slot);
                    assert_eq!(world.frame(), before);
                    let denied = deletion(&world, 3, false);
                    if expired {
                        let normalize = payout(&world, 0, false);
                        let before = world.env.portfolio_state(world.actors[0].portfolio);
                        let group = world.env.market_state().1;
                        peak = peak.max(land(
                            &mut world,
                            &[normalize.clone(), denied.clone()],
                            &[],
                            &[],
                            Some(1),
                        ));
                        rollbacks += 1;
                        book.check(&world);
                        let changed = [world.env.market, world.actors[0].portfolio];
                        peak = peak.max(land(&mut world, &[normalize], &[], &changed, None));
                        let after = world.env.portfolio_state(world.actors[0].portfolio);
                        assert_eq!((after.capital, after.pnl), (before.capital, before.pnl));
                        assert_eq!(
                            after.legs, before.legs,
                            "expiry neither settles B nor repeats K"
                        );
                        assert_eq!(after.source_domains, before.source_domains);
                        assert_eq!(world.env.market_state().1.assets, group.assets);
                        assert_eq!(
                            world.env.market_state().1.source_backing_buckets[domain].status,
                            percolator::BackingBucketStatusV16::Expired
                        );
                    }
                    check(&world, &book, expired);
                    for _ in 0..16 {
                        let round = world.frame();
                        for actor in order {
                            if resolved_portfolio_is_terminal(
                                &world.env,
                                world.actors[actor].portfolio,
                            ) {
                                continue;
                            }
                            let ix = payout(&world, actor, false);
                            let before = world.frame();
                            // Stage every successful continuation, including receipt
                            // creation and SPL payout, before the rejecting suffix.
                            world.env.svm.expire_blockhash();
                            let simulation = world.env.svm.simulate_transaction(
                                Transaction::new_signed_with_payer(
                                    &[heap_ix(), cu_ix(), ix.clone()],
                                    Some(&world.env.payer.pubkey()),
                                    &[&world.env.payer],
                                    world.env.svm.latest_blockhash(),
                                )
                                .into(),
                            );
                            if simulation.is_err() {
                                let error = world
                                    .payout(actor, false)
                                    .expect_err("waiting debt continuation");
                                assert!(is_engine_non_progress_error(&error), "{error}");
                                assert_eq!(world.frame(), before);
                                continue;
                            }
                            let had_receipt = resolved_receipt(
                                &world.env.portfolio_state(world.actors[actor].portfolio),
                            )
                            .present;
                            peak = peak.max(land(
                                &mut world,
                                &[ix.clone(), denied.clone()],
                                &[],
                                &[],
                                Some(1),
                            ));
                            rollbacks += 1;
                            check(&world, &book, expired);
                            let changed = [
                                world.env.market,
                                world.env.vault,
                                world.actors[actor].portfolio,
                                world.actors[actor].token,
                            ];
                            peak = peak.max(land(&mut world, &[ix], &[], &changed, None));
                            let receipt = resolved_receipt(
                                &world.env.portfolio_state(world.actors[actor].portfolio),
                            );
                            receipt_rollbacks +=
                                usize::from(actor == 0 && !had_receipt && receipt.present);
                            check(&world, &book, expired);
                        }
                        if (0..5).all(|i| {
                            resolved_portfolio_is_terminal(&world.env, world.actors[i].portfolio)
                        }) {
                            break;
                        }
                        assert_ne!(
                            world.frame(),
                            round,
                            "bounded public continuation must progress"
                        );
                    }
                    for actor in 0..5 {
                        assert!(resolved_portfolio_is_terminal(
                            &world.env,
                            world.actors[actor].portfolio
                        ));
                        assert_eq!(
                            world.env.token_amount(world.actors[actor].token) as u128,
                            book.payouts()[actor]
                        );
                    }
                    let mixed_receipt =
                        resolved_receipt(&world.env.portfolio_state(world.actors[0].portfolio));
                    assert_eq!(mixed_receipt.present, expired);
                    if expired {
                        assert!(mixed_receipt.finalized);
                        assert_eq!(mixed_receipt.paid_effective, RECEIPT_FACE);
                        let retry = payout(&world, 0, true);
                        peak = peak.max(land(&mut world, &[retry], &[], &[], None));
                    }
                    check(&world, &book, expired);
                    assert_eq!(world.env.market_state().1.vault, BURNED_FACE - DEBT);
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!((worlds, receipt_rollbacks), (24, 16));
    assert_cu_within("settled mixed-role expiry", peak, 600_000);
    println!("INV-039 settled expiry: {worlds} worlds, {rollbacks} exact prefix rollbacks, {receipt_rollbacks} mixed receipt/payout rollbacks; peak CU={peak}");
}
