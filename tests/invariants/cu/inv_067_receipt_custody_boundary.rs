//! INV-067/068: custody tails become mandatory only when a receipt actually pays.
//! Public expiry progress, late account rejection, alternate-route retry and exit.

use super::{late_expiry::World, *};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const FACES: [u128; 5] = [700, 0, 1_000, 0, 1_300];
const CAPITAL: [u128; 5] = [1_000, 0, 1_000, 0, 1_000];

fn entitlement(actor: usize, residual: u128) -> u128 {
    FACES[actor] * residual / 3_000
}

fn route(world: &World, actor: usize, kind: usize) -> Instruction {
    let mut ix = world.payout(actor, kind == 0);
    if kind == 2 {
        ix.data = ProgInstruction::PermissionlessCrank {
            now_slot: 13,
            observations: vec![],
        }
        .encode();
    }
    ix
}

fn transfers(logs: &[String]) -> usize {
    logs.iter()
        .filter(|line| **line == format!("Program {} success", spl_token::ID))
        .count()
}

#[test]
fn v16_program_receipt_custody_tail_boundaries_preserve_alternate_route_payouts() {
    let mut peak_cu = 0;
    let mut rollbacks = 0;
    let mut payouts = 0;
    let mut closes = 0;
    for claimant in [0, 4] {
        for rejected_route in 0..3 {
            let mut world = World::before_receipts();
            for actor in [0, 4] {
                for _ in 0..8 {
                    if world.receipt(actor).present {
                        break;
                    }
                    world.land(&[world.payout(actor, false)], false).unwrap();
                }
                let receipt = world.receipt(actor);
                assert!(receipt.present && !receipt.finalized);
                assert_eq!(receipt.terminal_positive_claim_face, FACES[actor]);
                assert_eq!(
                    receipt.prior_bound_contribution_num,
                    FACES[actor] * BOUND_SCALE
                );
                assert_eq!(receipt.paid_effective, entitlement(actor, 501));
                assert_eq!(
                    u128::from(world.env.token_amount(world.actors[actor].token)),
                    CAPITAL[actor] + entitlement(actor, 501)
                );
            }
            let original = world.receipt(claimant);
            let mut no_custody = world.payout(claimant, true);
            no_custody.accounts.truncate(3);
            let before = world.frame();
            let meta = world.land(&[no_custody.clone()], false).unwrap();
            assert_eq!(transfers(&meta.logs), 0);
            assert_eq!(world.frame(), before, "zero due needs no custody tail");

            world.peak_cu = 0;
            world.env.svm.warp_to_slot(13);
            // The same short account shape must allow a real stock transition:
            // release the expired reserve, without paying or modifying any receipt.
            let mut release = route(&world, 2, 1 + rejected_route % 2);
            release.accounts.truncate(3);
            let before = world.frame();
            let mut source = world.env.portfolio_state(world.actors[2].portfolio);
            source.health_cert.valid = 0;
            let meta = world.land(&[release], false).unwrap();
            assert_eq!(transfers(&meta.logs), 0);
            world.assert_frame_except(&before, &[world.env.market, world.actors[2].portfolio]);
            assert_eq!(world.env.portfolio_state(world.actors[2].portfolio), source);
            assert_eq!(world.receipt(claimant), original);
            let ledger = world.env.market_state().1.resolved_payout_ledger;
            assert_eq!(ledger.snapshot_slot, 12);
            assert_eq!(ledger.snapshot_residual, 851);
            assert_eq!(ledger.current_payout_rate_num, 851 * BOUND_SCALE);
            assert_eq!(ledger.current_payout_rate_den, 3_000 * BOUND_SCALE);
            assert_eq!(
                ledger.terminal_claim_bound_unreceipted_num,
                1_000 * BOUND_SCALE
            );
            let due = entitlement(claimant, 851) - entitlement(claimant, 501);
            assert_eq!(due, if claimant == 0 { 82 } else { 151 });

            let retained = route(&world, claimant, rejected_route);
            for boundary in 0..6 {
                let mut invalid = retained.clone();
                let expected_error = if boundary < 4 {
                    invalid.accounts.truncate(3 + boundary);
                    InstructionError::NotEnoughAccountKeys
                } else {
                    invalid.accounts[boundary - 1].is_writable = false;
                    InstructionError::Custom(PercolatorError::ExpectedWritable as u32)
                };
                let before = world.frame();
                let mut payer = world
                    .env
                    .svm
                    .get_account(&world.env.payer.pubkey())
                    .unwrap();
                payer.lamports -= FeeStructure::default().lamports_per_signature;
                let failure = world
                    .land(&[invalid], false)
                    .expect_err("paying custody boundary");
                assert_eq!(
                    failure.err,
                    TransactionError::InstructionError(2, expected_error),
                    "route {rejected_route}, boundary {boundary}"
                );
                assert_eq!(transfers(&failure.meta.logs), 0);
                assert_eq!(world.frame(), before, "receipt consumption must roll back");
                assert_eq!(
                    world.env.svm.get_account(&world.env.payer.pubkey()),
                    Some(payer)
                );
                assert_eq!(world.receipt(claimant), original);
                assert_eq!(world.env.market_state().1.resolved_payout_ledger, ledger);
                world.custody();
                rollbacks += 1;
            }

            let before = world.frame();
            let token = world.actors[claimant].token;
            let token_before = world.env.token_amount(token);
            let vault_before = world.env.token_amount(world.env.vault);
            let alternate = route(&world, claimant, (rejected_route + 1) % 3);
            let meta = world
                .land(&[alternate], false)
                .expect("alternate route preserves full due");
            assert_eq!(transfers(&meta.logs), 1);
            assert_eq!(
                u128::from(world.env.token_amount(token) - token_before),
                due
            );
            assert_eq!(
                u128::from(vault_before - world.env.token_amount(world.env.vault)),
                due
            );
            let mut paid = original;
            paid.paid_effective = entitlement(claimant, 851);
            assert_eq!(world.receipt(claimant), paid, "only paid_effective changes");
            assert_eq!(world.env.market_state().1.resolved_payout_ledger, ledger);
            world.assert_frame_except(
                &before,
                &[
                    world.env.market,
                    world.env.vault,
                    world.actors[claimant].portfolio,
                    token,
                ],
            );
            world.custody();
            payouts += 1;

            // The exact three-account top-up bytes that were usable before expiry
            // become usable again after another route consumes the positive due.
            let before = world.frame();
            let meta = world.land(&[no_custody], false).unwrap();
            assert_eq!(transfers(&meta.logs), 0);
            assert_eq!(world.frame(), before);

            for _ in 0..16 {
                for actor in [2, 4 - claimant, claimant, 1, 3] {
                    if resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio) {
                        continue;
                    }
                    let before = world.frame();
                    let ix = route(&world, actor, 2);
                    world
                        .land(&[ix], false)
                        .expect("bounded permissionless receipt exit");
                    assert_ne!(world.frame(), before, "exit must progress");
                    world.custody();
                    for actor in 0..5 {
                        assert!(
                            u128::from(world.env.token_amount(world.actors[actor].token))
                                <= CAPITAL[actor] + entitlement(actor, 851)
                        );
                    }
                }
                if world
                    .actors
                    .iter()
                    .all(|actor| resolved_portfolio_is_terminal(&world.env, actor.portfolio))
                {
                    break;
                }
            }
            for actor in 0..5 {
                let portfolio = world.actors[actor].portfolio;
                assert!(resolved_portfolio_is_terminal(&world.env, portfolio));
                assert_eq!(
                    u128::from(world.env.token_amount(world.actors[actor].token)),
                    CAPITAL[actor] + entitlement(actor, 851)
                );
                assert!(!world.receipt(actor).present);
                let before = world.frame();
                let mut replay = world.payout(actor, true);
                replay.accounts.truncate(3);
                world.land(&[replay], false).unwrap();
                assert_eq!(world.frame(), before, "terminal replay pays nothing");
                let rent = world.env.svm.get_account(&portfolio).unwrap().lamports;
                let market_before = world
                    .env
                    .svm
                    .get_account(&world.env.market)
                    .unwrap()
                    .lamports;
                let before = world.frame();
                let cu = world
                    .env
                    .close_portfolio_with_cu(&world.actors[actor].owner, portfolio);
                assert_cu_within(
                    "receipt custody boundary mechanical close",
                    cu,
                    CUSTODY_CU_LIMIT,
                );
                peak_cu = peak_cu.max(cu);
                assert!(world
                    .env
                    .svm
                    .get_account(&portfolio)
                    .is_none_or(|account| { account.lamports == 0 && account.data.is_empty() }));
                assert_eq!(
                    world
                        .env
                        .svm
                        .get_account(&world.env.market)
                        .unwrap()
                        .lamports,
                    market_before + rent
                );
                world.assert_frame_except(&before, &[world.env.market, portfolio]);
                closes += 1;
            }
            let group = world.env.market_state().1;
            assert_eq!(group.materialized_portfolio_count, 0);
            assert_eq!(
                [
                    group.c_tot,
                    group.pnl_pos_tot,
                    group.source_claim_bound_total_num,
                    group.insurance
                ],
                [0; 4]
            );
            assert_eq!(
                group.vault,
                851 - (0..5).map(|actor| entitlement(actor, 851)).sum::<u128>()
            );
            assert_eq!(group.vault, 2);
            assert_eq!(world.env.token_amount(world.provider_token), 1);
            world.custody();
            assert_cu_within(
                "receipt custody boundary suffix",
                world.peak_cu,
                CUSTODY_CU_LIMIT,
            );
            peak_cu = peak_cu.max(world.peak_cu);
        }
    }
    assert_eq!((rollbacks, payouts, closes), (36, 6, 30));
    println!("INV-067 custody boundary: 6 worlds, {rollbacks} exact rollbacks, {payouts} alternate-route top-ups, {closes} portfolio closes; peak CU {peak_cu}");
}
