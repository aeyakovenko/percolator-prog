//! INV-067: retained junior receipts survive a later source-backed claimant's realization.
//! Fresh realization removes only the converted face from the unreceipted bound; expiry
//! instead releases backing into residual. Each path has its own input-derived entitlements.

use super::{late_expiry::World, *};
use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

const ORIGINAL_FACE: [u128; 5] = [700, 0, 1_000, 0, 1_300];
const CAPITAL: [u128; 5] = [1_000, 0, 1_000, 0, 1_000];
const INITIAL_RESIDUAL: u128 = 501;
const BACKING: u128 = 100 + 250;
const TOTAL_FACE: u128 = 3_000;

#[test]
fn v16_program_retained_receipts_preserve_identity_across_fresh_realization_or_expiry() {
    let mut peak_cu = 0;
    for expired in [false, true] {
        let converted = if expired { 0 } else { BACKING };
        let residual = INITIAL_RESIDUAL + if expired { BACKING } else { 0 };
        let denominator = TOTAL_FACE - converted;
        let mut faces = ORIGINAL_FACE;
        faces[2] -= converted;
        let junior = faces.map(|face| face * residual / denominator);
        let expected: [u128; 5] = std::array::from_fn(|actor| {
            CAPITAL[actor] + junior[actor] + if actor == 2 { converted } else { 0 }
        });
        let rounding = residual - junior.iter().sum::<u128>();
        assert_eq!(rounding, 2);
        assert_eq!(expected.iter().sum::<u128>() + rounding + 1, 3_852);

        for order in [[0, 4], [4, 0]] {
            let mut world = World::new();
            let retained = order.map(|actor| world.payout(actor, true));
            let original = order.map(|actor| world.receipt(actor));
            let identities = order.map(|actor| {
                let portfolio = world.actors[actor].portfolio;
                (
                    world.env.portfolio_id(portfolio),
                    world.env.portfolio_position_epoch(portfolio),
                    state::read_portfolio_owner_preflight(
                        &world.env.svm.get_account(&portfolio).unwrap().data,
                    )
                    .unwrap(),
                )
            });
            let snapshot = world.env.market_state().1.resolved_payout_ledger;
            assert_eq!(snapshot.snapshot_slot, 12);
            assert_eq!(snapshot.snapshot_residual, INITIAL_RESIDUAL);
            assert_eq!(
                snapshot.terminal_claim_exact_receipts_num,
                2_000 * BOUND_SCALE
            );
            assert_eq!(
                snapshot.terminal_claim_bound_unreceipted_num,
                1_000 * BOUND_SCALE
            );
            assert_eq!(
                world.env.market_state().1.source_credit[3].fresh_reserved_backing_num,
                BACKING * BOUND_SCALE
            );
            let bucket = world.env.market_state().1.source_backing_buckets[3];
            assert_eq!(bucket.status, BackingBucketStatusV16::Fresh);
            assert_eq!(bucket.expiry_slot, 13);
            assert_eq!(bucket.consumed_liened_backing_num, 0);

            // Both already-paid requests run before the new stock disposition. Their zero
            // due must preserve the claim episode needed by the later, unchanged requests.
            let before = world.frame();
            world.land(&retained, false).unwrap();
            assert_eq!(world.frame(), before);

            // The same source claim can still expire after a rolled-back realization.
            // Both SPL payouts must execute before the suffix rejects; rollback must
            // restore the original receipts, unreceipted face, backing and destinations.
            let source_close = world.payout(2, false);
            let failure = world
                .land(
                    &[
                        source_close.clone(),
                        retained[0].clone(),
                        Instruction {
                            program_id: solana_sdk::system_program::ID,
                            accounts: vec![],
                            data: vec![],
                        },
                    ],
                    false,
                )
                .expect_err("reject after fresh realization and older receipt top-up");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(4, InstructionError::InvalidInstructionData,)
            );
            for program in [world.env.program_id, spl_token::ID] {
                assert_eq!(
                    failure
                        .meta
                        .logs
                        .iter()
                        .filter(|line| { **line == format!("Program {program} success") })
                        .count(),
                    2,
                    "both paying prefixes must execute"
                );
            }
            assert_cu_within(
                "source realization and top-up rollback",
                failure.meta.compute_units_consumed,
                500_000,
            );
            assert_eq!(
                world.frame(),
                before,
                "rejected stock disposition must roll back exactly"
            );
            world.custody();

            world.env.svm.warp_to_slot(if expired { 13 } else { 12 });
            for _ in 0..8 {
                if resolved_portfolio_is_terminal(&world.env, world.actors[2].portfolio) {
                    break;
                }
                let before = world.frame();
                let tokens = world.env.token_amount(world.actors[2].token);
                let vault = world.env.token_amount(world.env.vault);
                let meta = world.land(&[source_close.clone()], false).unwrap();
                assert_cu_within(
                    "later source claimant",
                    meta.compute_units_consumed,
                    CUSTODY_CU_LIMIT,
                );
                assert_ne!(
                    world.frame(),
                    before,
                    "source claimant must make bounded progress"
                );
                let paid = world.env.token_amount(world.actors[2].token) - tokens;
                assert_eq!(vault - world.env.token_amount(world.env.vault), paid);
                world.assert_frame_except(
                    &before,
                    &[
                        world.env.market,
                        world.env.vault,
                        world.actors[2].portfolio,
                        world.actors[2].token,
                    ],
                );
                assert_eq!(order.map(|actor| world.receipt(actor)), original);
                world.custody();
            }
            assert!(resolved_portfolio_is_terminal(
                &world.env,
                world.actors[2].portfolio
            ));
            assert_eq!(
                u128::from(world.env.token_amount(world.actors[2].token)),
                expected[2]
            );
            let group = world.env.market_state().1;
            let ledger = group.resolved_payout_ledger;
            assert_eq!(ledger.snapshot_slot, snapshot.snapshot_slot);
            assert_eq!(ledger.snapshot_residual, residual);
            assert_eq!(ledger.current_payout_rate_num, residual * BOUND_SCALE);
            assert_eq!(ledger.current_payout_rate_den, denominator * BOUND_SCALE);
            assert_eq!(ledger.terminal_claim_bound_unreceipted_num, 0);
            assert_eq!(
                ledger.terminal_claim_exact_receipts_num,
                denominator * BOUND_SCALE
            );
            assert!(!ledger.payout_halted);
            assert_eq!(group.source_credit[3].positive_claim_bound_num, 0);
            assert_eq!(group.source_credit[3].fresh_reserved_backing_num, 0);
            // Full consumption also retires the bucket tag. The consumed stock and
            // provider receivable distinguish realization from an expiry release.
            assert_eq!(
                group.source_backing_buckets[3].status,
                BackingBucketStatusV16::Expired
            );
            assert_eq!(
                group.source_backing_buckets[3].consumed_liened_backing_num,
                converted * BOUND_SCALE
            );
            assert_eq!(
                group.source_credit[3].provider_receivable_num,
                converted * BOUND_SCALE
            );

            // At zero unreceipted bound, the surviving receipts still have positive due.
            // Paying in either order cannot erase the peer or restore the converted face.
            for (index, actor) in order.into_iter().enumerate() {
                let before = world.frame();
                let tokens = world.env.token_amount(world.actors[actor].token);
                let vault = world.env.token_amount(world.env.vault);
                let due = junior[actor] - original[index].paid_effective;
                assert!(due > 0);
                let meta = world.land(&[retained[index].clone()], false).unwrap();
                assert_cu_within(
                    "retained receipt after source realization",
                    meta.compute_units_consumed,
                    CUSTODY_CU_LIMIT,
                );
                assert_eq!(
                    u128::from(world.env.token_amount(world.actors[actor].token) - tokens),
                    due
                );
                assert_eq!(
                    u128::from(vault - world.env.token_amount(world.env.vault)),
                    due
                );
                let mut receipt = original[index];
                receipt.paid_effective = junior[actor];
                assert_eq!(world.receipt(actor), receipt);
                assert_eq!(world.env.market_state().1.resolved_payout_ledger, ledger);
                let portfolio = world.actors[actor].portfolio;
                assert_eq!(
                    (
                        world.env.portfolio_id(portfolio),
                        world.env.portfolio_position_epoch(portfolio),
                        state::read_portfolio_owner_preflight(
                            &world.env.svm.get_account(&portfolio).unwrap().data
                        )
                        .unwrap(),
                    ),
                    identities[index]
                );
                world.assert_frame_except(
                    &before,
                    &[
                        world.env.market,
                        world.env.vault,
                        portfolio,
                        world.actors[actor].token,
                    ],
                );
                world.custody();
            }

            // The first zero-due retry may clear a fully diluted receipt; later retries
            // at the same authenticated slot must be completely inert.
            for index in [1, 0] {
                let actor = order[index];
                let before = world.frame();
                world.land(&[retained[index].clone()], false).unwrap();
                assert!(!world.receipt(actor).present);
                world.assert_frame_except(&before, &[world.actors[actor].portfolio]);
                world.custody();
            }
            let before = world.frame();
            world
                .land(
                    &[
                        retained[1].clone(),
                        retained[0].clone(),
                        retained[1].clone(),
                    ],
                    false,
                )
                .unwrap();
            assert_eq!(world.frame(), before, "retired claims cannot be paid twice");
            assert_eq!(world.env.market_state().1.resolved_payout_ledger, ledger);

            for actor in [2, order[1], order[0], 1, 3] {
                let portfolio = world.actors[actor].portfolio;
                assert!(resolved_portfolio_is_terminal(&world.env, portfolio));
                assert_eq!(
                    u128::from(world.env.token_amount(world.actors[actor].token)),
                    expected[actor]
                );
                let before = world.frame();
                let cu = world
                    .env
                    .close_portfolio_with_cu(&world.actors[actor].owner, portfolio);
                assert_cu_within("source-realization portfolio close", cu, CUSTODY_CU_LIMIT);
                peak_cu = peak_cu.max(cu);
                world.assert_frame_except(&before, &[world.env.market, portfolio]);
                world.custody();
            }
            let terminal = world.env.market_state().1;
            assert_eq!(terminal.materialized_portfolio_count, 0);
            assert_eq!(
                (
                    terminal.c_tot,
                    terminal.pnl_pos_tot,
                    terminal.insurance,
                    terminal.source_claim_bound_total_num,
                    terminal.backing_provider_earnings_total
                ),
                (0, 0, 0, 0, 0)
            );
            assert_eq!(terminal.vault, rounding);
            assert_eq!(world.env.token_amount(world.provider_token), 1);
            crate::support::fuzz_model::assert_reservation_encumbrance_census(
                "INV-067 source-realization terminal stock",
                &terminal,
                &[],
            )
            .unwrap();
            peak_cu = peak_cu.max(world.peak_cu);
            println!("INV-067 fresh/expired={expired}, order={order:?}: payouts={expected:?}, residual={residual}, face={denominator}, rounding={rounding}");
        }
    }
    assert_cu_within("INV-067 receipt source realization", peak_cu, 500_000);
}
