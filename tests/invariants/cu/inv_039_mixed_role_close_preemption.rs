//! INV-039 / rows 419, 435: expiry of an opposing bankrupt close must not erase
//! either role of a pending creditor that also owes unsettled cross-asset debt.
//! Permissionless Recovery/Resolved preempts the close before B booking or K
//! settlement. The input-derived book covers partial support and fractional
//! peer conversion; direct resolution at the same Clock is the route control.
//! This is distinct from participating-asset shutdown and native slab retirement.

use super::*;

#[test]
fn v16_program_expired_close_preserves_mixed_debt_and_fractional_source_attribution() {
    let mut worlds = 0;
    let mut transitions = 0;
    let mut rollbacks = 0;
    let mut grace_rejections = 0;
    let mut waits = 0;
    let mut retries = 0;
    let mut peak = 0;
    for debt in [36_000, 216_000] {
        for reverse in [false, true] {
            for assets in [[1, 2], [2, 1]] {
                for order in [[0, 2, 1, 3, 4], [2, 1, 0, 4, 3]] {
                    let mut control = None;
                    for preempt in [false, true] {
                        let (mut world, mut book) =
                            setup_with_horizons(reverse, assets, debt, false, 12, 1_000);
                        let market = world.env.market;
                        let vault = world.env.vault;
                        let debtor = world.actors[1].portfolio;
                        let initial_close = close_progress(&world.env.portfolio_state(debtor));
                        assert!(initial_close.active && !initial_close.finalized);
                        assert_eq!(initial_close.residual_remaining, RESIDUAL);
                        assert_eq!(initial_close.max_close_slot, 17);
                        let creditor_domain = assets[0] * 2 + usize::from(!reverse);
                        assert_eq!(
                            world.env.market_state().1.source_backing_buckets[creditor_domain]
                                .expiry_slot,
                            1_005,
                            "source backing remains fresh beyond close expiry and payout grace"
                        );
                        let sibling = world.env.market_state().1.assets[0];
                        let denied = deletion(&world, 3, false);
                        let clock_frame = world.frame();
                        world.env.svm.warp_to_slot(initial_close.max_close_slot + 1);
                        assert_eq!(world.frame(), clock_frame);
                        book.check(&world);
                        if preempt {
                            let crank = Instruction {
                                program_id: world.env.program_id,
                                data: ProgInstruction::PermissionlessCrank {
                                    now_slot: 0,
                                    observations: vec![],
                                }
                                .encode(),
                                accounts: vec![
                                    AccountMeta::new(world.env.payer.pubkey(), true),
                                    AccountMeta::new(market, false),
                                    AccountMeta::new(debtor, false),
                                ],
                            };
                            for mode in [MarketModeV16::Recovery, MarketModeV16::Resolved] {
                                let group_before = world.env.market_state().1;
                                peak = peak.max(land(
                                    &mut world,
                                    &[crank.clone(), denied.clone()],
                                    &[],
                                    &[],
                                    Some(1),
                                ));
                                rollbacks += 1;
                                book.check(&world);
                                assert!(!book.booked && !book.charged && !book.debt_settled);
                                let cu = land(&mut world, &[crank.clone()], &[], &[market], None);
                                peak = peak.max(cu);
                                assert_cu_within("mixed-role close preemption", cu, CRANK_CU_LIMIT);
                                transitions += 1;
                                let group = world.env.market_state().1;
                                assert_eq!(group.mode, mode);
                                assert_eq!(
                                    group.recovery_reason,
                                    Some(PermissionlessRecoveryReasonV16::ActiveBankruptCloseCannotProgress)
                                );
                                assert_eq!(group.assets, group_before.assets);
                                assert_eq!(group.source_credit, group_before.source_credit);
                                assert_eq!(
                                    group.source_backing_buckets,
                                    group_before.source_backing_buckets
                                );
                                assert_eq!(
                                    close_progress(&world.env.portfolio_state(debtor)),
                                    initial_close
                                );
                                book.check(&world);
                                assert!(!book.booked && !book.charged && !book.debt_settled);
                            }
                        } else {
                            peak = peak.max(world.env.resolve());
                        }
                        assert_eq!(world.env.market_state().1.mode, MarketModeV16::Resolved);
                        book.check(&world);
                        assert!(!book.booked && !book.charged && !book.debt_settled);

                        let first = payout(&world, 1, false);
                        peak = peak.max(land(&mut world, &[first.clone()], &[], &[], Some(0)));
                        grace_rejections += 1;
                        book.check(&world);
                        world
                            .env
                            .svm
                            .warp_to_slot(initial_close.max_close_slot + 1 + 5);

                        // Roll back the first residual-booking continuation, then retain
                        // precisely that instruction before settling either mixed role.
                        peak = peak.max(land(
                            &mut world,
                            &[first.clone(), denied.clone()],
                            &[],
                            &[],
                            Some(1),
                        ));
                        rollbacks += 1;
                        book.check(&world);
                        assert!(!book.booked && !book.charged && !book.debt_settled);
                        let changed = [market, vault, debtor, world.actors[1].token];
                        peak = peak.max(land(&mut world, &[first], &[], &changed, None));
                        book.check(&world);
                        assert!(book.booked && !book.charged && !book.debt_settled);

                        for _ in 0..16 {
                            let before = world.frame();
                            for actor in order {
                                if !resolved_portfolio_is_terminal(
                                    &world.env,
                                    world.actors[actor].portfolio,
                                ) {
                                    waits += usize::from(book.close(&mut world, actor, &mut peak));
                                }
                            }
                            if world
                                .actors
                                .iter()
                                .all(|a| resolved_portfolio_is_terminal(&world.env, a.portfolio))
                            {
                                break;
                            }
                            assert_ne!(world.frame(), before, "a nonterminal round must progress");
                        }
                        assert!(book.booked && book.charged && book.debt_settled);
                        assert!(world
                            .actors
                            .iter()
                            .all(|a| { resolved_portfolio_is_terminal(&world.env, a.portfolio) }));
                        let receipt =
                            resolved_receipt(&world.env.portfolio_state(world.actors[2].portfolio));
                        let face = if debt == 36_000 { 36_000 } else { 180_001 };
                        assert!(receipt.present && receipt.finalized);
                        assert_eq!(
                            (receipt.terminal_positive_claim_face, receipt.paid_effective),
                            (face, face)
                        );
                        let peer_domain = assets[1] * 2 + usize::from(!reverse);
                        assert_eq!(
                            world.env.market_state().1.source_credit[peer_domain].fresh_reserved_backing_num,
                            u128::from(debt == 216_000) * BOUND_SCALE,
                            "fractional conversion's extra atom remains reserved in the peer domain"
                        );
                        let retry = payout(&world, 2, true);
                        peak = peak.max(land(&mut world, &[retry], &[], &[], None));
                        retries += 1;
                        book.check(&world);

                        let paid: [u128; 5] = std::array::from_fn(|i| {
                            world.env.token_amount(world.actors[i].token) as u128
                        });
                        assert_eq!(paid, book.payouts());
                        let outcome = (paid, world.env.token_amount(vault) as u128);
                        assert_eq!(outcome.1, book.face_discount());
                        if let Some(expected) = control {
                            assert_eq!(
                                outcome, expected,
                                "preemption preserves each owner's entitlement"
                            );
                        } else {
                            control = Some(outcome);
                        }
                        for actor in order {
                            let owner = world.actors[actor].owner.insecure_clone();
                            let portfolio = world.actors[actor].portfolio;
                            let delete = deletion(&world, actor, true);
                            let rent = world.env.svm.get_account(&portfolio).unwrap().lamports;
                            let market_rent = world.env.svm.get_account(&market).unwrap().lamports;
                            peak = peak.max(land(
                                &mut world,
                                &[delete],
                                &[&owner],
                                &[market, portfolio],
                                None,
                            ));
                            assert_eq!(
                                world.env.svm.get_account(&market).unwrap().lamports,
                                market_rent + rent
                            );
                            book.deleted[actor] = true;
                            book.check(&world);
                        }
                        let group = world.env.market_state().1;
                        assert_eq!(group.assets[0], sibling);
                        assert_eq!(
                            (
                                group.c_tot,
                                group.pnl_pos_tot,
                                group.materialized_portfolio_count
                            ),
                            (0, 0, 0)
                        );
                        assert_eq!(group.vault, book.face_discount());
                        worlds += 1;
                    }
                }
            }
        }
    }
    assert_eq!((worlds, transitions, rollbacks, retries), (32, 32, 64, 32));
    assert_eq!(grace_rejections, 32);
    assert!(waits > 0);
    println!("INV-039 mixed close preemption: {worlds} worlds, {transitions} permissionless transitions, {rollbacks} successful-prefix rollbacks, {grace_rejections} grace-period rejections, {waits} waiting rollbacks, {retries} receipt retries; peak CU={peak}");
}
