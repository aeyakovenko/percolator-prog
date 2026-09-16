//! INV-037/039/048/076: mixed creditor residual survives debt-side ADL and rebalance.
//! Peer reduction halves effective debt exposure without changing its raw basis
//! or accrued K debt. Exact-effective and raw-size owner exits must preserve the
//! separate pending creditor weight through side finalization and close expiry.
//! Input-derived owner payouts agree with direct resolution; rows 419/435 stay OPEN.

use super::*;
use crate::support::reference_math::mul_div_ceil;

#[derive(Default)]
struct Evidence {
    peak: u64,
    rollbacks: usize,
    waits: usize,
    retries: usize,
}

fn atomic(
    world: &mut AttributionWorld,
    ix: Instruction,
    signers: &[&Keypair],
    changed: &[Pubkey],
    e: &mut Evidence,
) {
    let denied = deletion(world, 4, false);
    e.peak = e
        .peak
        .max(land(world, &[ix.clone(), denied], signers, &[], Some(1)));
    e.rollbacks += 1;
    e.peak = e.peak.max(land(world, &[ix], signers, changed, None));
}

fn reduce(world: &AttributionWorld, actor: usize, asset: usize, quantity: u128) -> Instruction {
    let a = &world.actors[actor];
    Instruction {
        program_id: world.env.program_id,
        data: ProgInstruction::RebalanceReduce {
            portfolio_id: world.env.portfolio_id(a.portfolio),
            position_epoch: world.env.portfolio_position_epoch(a.portfolio),
            asset_index: asset as u16,
            reduce_q: quantity,
        }
        .encode(),
        accounts: vec![
            AccountMeta::new(a.owner.pubkey(), true),
            AccountMeta::new(world.env.market, false),
            AccountMeta::new(a.portfolio, false),
        ],
    }
}

// The parent's input book supplies debt/support/face arithmetic, while this
// census handles nonunit ADL and previous-reset raw residue explicitly.
fn check(world: &AttributionWorld, book: &Book, settled: bool) {
    let env = &world.env;
    let g = env.market_state().1;
    let side = usize::from(book.sign < 0);
    let target_b = RESIDUAL * SOCIAL_LOSS_DEN / POS_SCALE;
    let a = g.assets[book.assets[0]];
    let b = [a.b_long_num, a.b_short_num];
    assert!(b[side] == 0 || b[side] == target_b);
    assert_eq!(b[1 - side], 0);
    let booked = b[side] == target_b;
    let mut oi = [[0u128; 2]; 3];
    let mut weight = oi;
    let mut stored = [[0u64; 2]; 3];
    let mut pending = stored;
    let mut portfolios = Vec::new();
    let mut paid = [0u128; 5];
    for (actor, owner) in world.actors.iter().enumerate() {
        paid[actor] = env.token_amount(owner.token) as u128;
        assert!(paid[actor] <= book.payouts()[actor]);
        if book.deleted[actor] {
            assert_eq!(paid[actor], book.payouts()[actor]);
            assert!(env
                .svm
                .get_account(&owner.portfolio)
                .is_none_or(|a| a.lamports == 0 && a.data.is_empty()));
            continue;
        }
        let p = env.portfolio_state(owner.portfolio);
        let legs: Vec<_> = p
            .legs
            .iter()
            .map(|l| l.try_to_runtime().unwrap())
            .filter(|l| l.active)
            .collect();
        for l in &legs {
            let index = l.asset_index as usize;
            let s = usize::from(l.side == SideV16::Short);
            let a = g.assets[index];
            let (epoch, current_a, mode) = if s == 0 {
                (a.epoch_long, a.a_long, a.mode_long)
            } else {
                (a.epoch_short, a.a_short, a.mode_short)
            };
            if l.epoch_snap == epoch {
                oi[index][s] +=
                    mul_div_ceil(l.basis_pos_q.unsigned_abs(), current_a, l.a_basis).unwrap();
                weight[index][s] += l.loss_weight;
            } else {
                assert_eq!(l.epoch_snap + 1, epoch);
                assert_eq!(mode, percolator::SideModeV16::ResetPending);
            }
            stored[index][s] += 1;
            pending[index][s] += u64::from(l.basis_pos_q == 0 && l.loss_weight != 0);
            if index == book.assets[0] {
                assert_eq!(actor, 0);
                assert_eq!((l.basis_pos_q, l.loss_weight), (0, POS_SCALE));
                assert_eq!(s, side);
                assert!(l.b_snap == 0 || l.b_snap == target_b);
                assert_eq!(l.b_rem, 0);
            } else {
                assert_eq!(index, book.assets[1]);
                assert_eq!(
                    l.b_snap, 0,
                    "creditor B cannot migrate to debt-side ADL residue"
                );
                assert!(!settled || actor != 0, "rebalanced debt cannot reappear");
            }
        }
        let close = close_progress(&p);
        verify_close_residual_partition("mixed debt ADL rebalance", &close).unwrap();
        if actor == 1 {
            assert!(close.active && !close.canceled);
            assert_eq!(close.asset_index as usize, book.assets[0]);
            assert_eq!(close.gross_loss_at_close_start, RESIDUAL);
            assert_eq!(close.finalized, booked);
            assert_eq!(close.b_loss_booked, if booked { RESIDUAL } else { 0 });
            assert_eq!(close.residual_remaining, if booked { 0 } else { RESIDUAL });
            assert_eq!(
                (
                    close.support_consumed,
                    close.junior_face_burned,
                    close.insurance_spent,
                    close.explicit_loss_assigned,
                    close.drift_consumed,
                    close.quantity_adl_applied_q
                ),
                (0, 0, 0, 0, 0, 0)
            );
        } else {
            assert_eq!(close, CloseProgressLedgerV16::default());
        }
        let expected = match actor {
            0 => {
                let creditor = legs
                    .iter()
                    .find(|l| l.asset_index as usize == book.assets[0]);
                let charged = creditor.is_none_or(|l| l.b_snap == target_b);
                assert!(
                    !charged || booked,
                    "debt exit cannot release pending creditor weight"
                );
                if !charged || !settled {
                    assert_eq!(paid[actor], 0);
                }
                (DEPOSITS[0] + GAIN
                    - if settled {
                        book.debt + book.face_discount()
                    } else {
                        0
                    }
                    - if charged { RESIDUAL } else { 0 }) as i128
            }
            1 => {
                if booked {
                    0
                } else {
                    -(RESIDUAL as i128)
                }
            }
            2 => (DEPOSITS[2] + book.debt) as i128,
            _ => DEPOSITS[actor] as i128,
        };
        let receipt = resolved_receipt(&p);
        let due = if receipt.present {
            assert_eq!(actor, 2);
            assert_eq!(
                receipt.terminal_positive_claim_face,
                book.peer_receipt_face()
            );
            assert_eq!(
                receipt.prior_bound_contribution_num,
                book.peer_receipt_face() * BOUND_SCALE
            );
            receipt
                .terminal_positive_claim_face
                .checked_sub(receipt.paid_effective)
                .unwrap()
        } else {
            0
        };
        assert_eq!(
            p.capital.get() as i128 + p.pnl.get() + due as i128 + paid[actor] as i128,
            expected,
            "input-derived owner {actor} entitlement"
        );
        for source in p.source_domains.iter().filter(|s| s.is_occupied()) {
            assert!(actor == 0 || actor == 2);
            let domain = book.assets[usize::from(actor == 2)] * 2 + 1 - side;
            assert_eq!(source.domain.get() as usize, domain);
        }
        portfolios.push(p);
    }
    for index in 0..3 {
        let a = g.assets[index];
        assert_eq!([a.oi_eff_long_q, a.oi_eff_short_q], oi[index]);
        assert_eq!(
            [a.loss_weight_sum_long, a.loss_weight_sum_short],
            weight[index]
        );
        assert_eq!(
            [a.stored_pos_count_long, a.stored_pos_count_short],
            stored[index]
        );
        assert_eq!(
            [
                a.pending_obligation_count_long,
                a.pending_obligation_count_short
            ],
            pending[index]
        );
        if index != book.assets[0] {
            assert_eq!([a.b_long_num, a.b_short_num], [0; 2]);
        }
        assert_eq!(
            [
                a.social_loss_remainder_long_num,
                a.social_loss_remainder_short_num,
                a.social_loss_dust_long_num,
                a.social_loss_dust_short_num,
                a.explicit_unallocated_loss_long,
                a.explicit_unallocated_loss_short
            ],
            [0; 6]
        );
    }
    let vault = env.token_amount(env.vault) as u128;
    assert_eq!(
        vault + paid.iter().sum::<u128>(),
        DEPOSITS.iter().sum::<u128>()
    );
    assert_eq!(g.vault, vault);
    assert_eq!(g.insurance, 0);
    assert_eq!(
        Mint::unpack(&env.svm.get_account(&env.mint).unwrap().data)
            .unwrap()
            .supply as u128,
        DEPOSITS.iter().sum::<u128>()
    );
    assert_market_stock_census(
        "mixed debt ADL rebalance",
        &g,
        &env.svm.get_account(&env.market).unwrap().data,
        &portfolios,
        vault,
    )
    .unwrap();
    assert_reservation_encumbrance_census("mixed debt ADL rebalance", &g, &portfolios).unwrap();
}

#[test]
fn v16_program_mixed_debt_adl_rebalance_preserves_residual_through_preemption() {
    let mut e = Evidence::default();
    let mut worlds = 0;
    for debt in [36_000, 216_000] {
        for reverse in [false, true] {
            for assets in [[1, 2], [2, 1]] {
                let mut control = None;
                for raw_request in [false, true] {
                    for preempt in [false, true] {
                        let (mut world, mut book) =
                            setup_with_horizons(reverse, assets, debt, false, 12, 1_000);
                        let market = world.env.market;
                        let vault = world.env.vault;
                        let mixed = world.actors[0].portfolio;
                        let debtor = world.actors[1].portfolio;
                        let peer = world.actors[2].portfolio;
                        let initial_close = close_progress(&world.env.portfolio_state(debtor));
                        let pending =
                            active_leg_for_asset(&world.env.portfolio_state(mixed), assets[0]);
                        let debt_leg =
                            active_leg_for_asset(&world.env.portfolio_state(mixed), assets[1]);
                        assert_eq!(debt_leg.k_snap, 0);
                        check(&world, &book, false);
                        let peer_owner = world.actors[2].owner.insecure_clone();
                        let ix = reduce(&world, 2, assets[1], POS_SCALE);
                        atomic(&mut world, ix, &[&peer_owner], &[market, peer], &mut e);
                        check(&world, &book, false);
                        let adl = world.env.market_state().1.assets[assets[1]];
                        let debt_a = if reverse { adl.a_long } else { adl.a_short };
                        assert_eq!(debt_a, ADL_ONE / 2);
                        assert_eq!([adl.oi_eff_long_q, adl.oi_eff_short_q], [POS_SCALE; 2]);
                        assert_eq!(
                            active_leg_for_asset(&world.env.portfolio_state(mixed), assets[1]),
                            debt_leg
                        );
                        assert_eq!(debt_leg.basis_pos_q.unsigned_abs(), 2 * POS_SCALE);
                        assert_eq!(
                            mul_div_ceil(
                                debt_leg.basis_pos_q.unsigned_abs(),
                                debt_a,
                                debt_leg.a_basis
                            )
                            .unwrap(),
                            POS_SCALE
                        );

                        let owner = world.actors[0].owner.insecure_clone();
                        let requested = if raw_request {
                            2 * POS_SCALE
                        } else {
                            POS_SCALE
                        };
                        let ix = reduce(&world, 0, assets[1], requested);
                        atomic(&mut world, ix, &[&owner], &[market, mixed], &mut e);
                        check(&world, &book, true);
                        assert!(!has_active_leg_for_asset(
                            &world.env.portfolio_state(mixed),
                            assets[1]
                        ));
                        assert_eq!(
                            active_leg_for_asset(&world.env.portfolio_state(mixed), assets[0]),
                            pending
                        );
                        assert_eq!(
                            close_progress(&world.env.portfolio_state(debtor)),
                            initial_close
                        );
                        let exited = world.env.market_state().1.assets[assets[1]];
                        assert_eq!([exited.oi_eff_long_q, exited.oi_eff_short_q], [0; 2]);
                        assert_eq!(
                            if reverse {
                                exited.mode_long
                            } else {
                                exited.mode_short
                            },
                            percolator::SideModeV16::ResetPending
                        );
                        let reset = Instruction {
                            program_id: world.env.program_id,
                            data: ProgInstruction::FinalizeResetSide {
                                asset_index: assets[1] as u16,
                                side: u8::from(!reverse),
                            }
                            .encode(),
                            accounts: vec![AccountMeta::new(market, false)],
                        };
                        atomic(&mut world, reset, &[], &[market], &mut e);
                        check(&world, &book, true);
                        let finalized = world.env.market_state().1.assets[assets[1]];
                        assert_eq!(
                            if reverse {
                                (finalized.mode_long, finalized.a_long)
                            } else {
                                (finalized.mode_short, finalized.a_short)
                            },
                            (percolator::SideModeV16::Normal, ADL_ONE)
                        );
                        assert_eq!(
                            close_progress(&world.env.portfolio_state(debtor)),
                            initial_close
                        );
                        assert_eq!(
                            active_leg_for_asset(&world.env.portfolio_state(mixed), assets[0]),
                            pending
                        );

                        world.env.svm.warp_to_slot(initial_close.max_close_slot + 1);
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
                                let before = world.env.market_state().1;
                                atomic(&mut world, crank.clone(), &[], &[market], &mut e);
                                let after = world.env.market_state().1;
                                assert_eq!(after.mode, mode);
                                assert_eq!(after.recovery_reason,
                                    Some(PermissionlessRecoveryReasonV16::ActiveBankruptCloseCannotProgress));
                                assert_eq!(after.assets, before.assets);
                                assert_eq!(after.source_credit, before.source_credit);
                                assert_eq!(
                                    after.source_backing_buckets,
                                    before.source_backing_buckets
                                );
                                assert_eq!(
                                    close_progress(&world.env.portfolio_state(debtor)),
                                    initial_close
                                );
                                check(&world, &book, true);
                            }
                        } else {
                            e.peak = e.peak.max(world.env.resolve());
                        }
                        assert_eq!(world.env.market_state().1.mode, MarketModeV16::Resolved);
                        check(&world, &book, true);
                        world.env.svm.warp_to_slot(initial_close.max_close_slot + 6);
                        let first = payout(&world, 1, false);
                        let changed = [market, vault, debtor, world.actors[1].token];
                        atomic(&mut world, first, &[], &changed, &mut e);
                        check(&world, &book, true);
                        assert!(close_progress(&world.env.portfolio_state(debtor)).finalized);
                        for _ in 0..16 {
                            let round = world.frame();
                            for actor in [0, 2, 1, 3, 4] {
                                if resolved_portfolio_is_terminal(
                                    &world.env,
                                    world.actors[actor].portfolio,
                                ) {
                                    continue;
                                }
                                let before = world.frame();
                                match world.payout(actor, false) {
                                    Ok(cu) => {
                                        e.peak = e.peak.max(cu);
                                        assert_ne!(world.frame(), before);
                                    }
                                    Err(error) => {
                                        assert!(is_engine_non_progress_error(&error), "{error}");
                                        assert_eq!(world.frame(), before);
                                        e.waits += 1;
                                    }
                                }
                                for (key, account) in before {
                                    if ![
                                        market,
                                        vault,
                                        world.actors[actor].portfolio,
                                        world.actors[actor].token,
                                    ]
                                    .contains(&key)
                                    {
                                        assert_eq!(world.env.svm.get_account(&key), account);
                                    }
                                }
                                check(&world, &book, true);
                            }
                            if world
                                .actors
                                .iter()
                                .all(|a| resolved_portfolio_is_terminal(&world.env, a.portfolio))
                            {
                                break;
                            }
                            assert_ne!(world.frame(), round, "nonterminal sweep must progress");
                        }
                        assert!(world
                            .actors
                            .iter()
                            .all(|a| resolved_portfolio_is_terminal(&world.env, a.portfolio)));
                        let receipt = resolved_receipt(&world.env.portfolio_state(peer));
                        assert!(receipt.present && receipt.finalized, "{receipt:?}");
                        let retry = payout(&world, 2, true);
                        e.peak = e.peak.max(land(&mut world, &[retry], &[], &[], None));
                        e.retries += 1;
                        check(&world, &book, true);
                        let paid: [u128; 5] = std::array::from_fn(|i| {
                            world.env.token_amount(world.actors[i].token) as u128
                        });
                        assert_eq!(paid, book.payouts());
                        let outcome = (paid, world.env.token_amount(vault) as u128);
                        assert_eq!(outcome.1, book.face_discount());
                        if let Some(expected) = control {
                            assert_eq!(outcome, expected);
                        } else {
                            control = Some(outcome);
                        }
                        for actor in 0..5 {
                            let owner = world.actors[actor].owner.insecure_clone();
                            let portfolio = world.actors[actor].portfolio;
                            let rent = world.env.svm.get_account(&portfolio).unwrap().lamports;
                            let old_rent = world.env.svm.get_account(&market).unwrap().lamports;
                            let delete = deletion(&world, actor, true);
                            e.peak = e.peak.max(land(
                                &mut world,
                                &[delete],
                                &[&owner],
                                &[market, portfolio],
                                None,
                            ));
                            assert_eq!(
                                world.env.svm.get_account(&market).unwrap().lamports,
                                old_rent + rent
                            );
                            book.deleted[actor] = true;
                            check(&world, &book, true);
                        }
                        worlds += 1;
                    }
                }
            }
        }
    }
    assert_eq!((worlds, e.rollbacks, e.retries), (32, 160, 32));
    assert_eq!(
        e.waits, 0,
        "rebalance settles the cross-asset debt before resolution"
    );
    assert_cu_within("mixed debt ADL rebalance campaign", e.peak, 600_000);
    println!("INV-039 mixed debt ADL rebalance: {worlds} worlds, {} successful-prefix rollbacks, {} waiting rollbacks, {} receipt retries, 32 side finalizations, 160 portfolio deletions; peak CU={}", e.rollbacks, e.waits, e.retries, e.peak);
}
