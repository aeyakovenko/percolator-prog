//! INV-068 - receipt uniqueness and monotonic top-ups.
//!
//! A terminal receipt must be shared by all payout rails: once a public
//! `CloseResolved` pays the resolved entitlement, later `CloseResolved` or
//! `ClaimResolvedPayoutTopup` calls must be exact no-ops for that same episode.
//! Destination and delegated-vault regressions assert rejected top-ups do not
//! burn the pending receipt or move custody. Public lifecycle tests remain the
//! primary reachability evidence; state-seeded terminal probes are narrower
//! receipt-preservation checks.
//! The shared-owner history below keeps two unequal embedded receipts independent even
//! when their claimant and SPL destination are identical, including after one owner exit.

use super::*;

#[test]
fn v16_program_retired_coowned_receipt_rolls_back_live_sibling_topup_bundle() {
    use super::inv_067_terminal_payout_completeness_and_exact_once_settlement::late_expiry::World;
    use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

    const FACES: [u128; 5] = [700, 0, 1_000, 0, 1_300];
    const CAPITAL: [u128; 5] = [1_000, 0, 1_000, 0, 1_000];
    const TOTAL_FACE: u128 = 3_000;
    const INITIAL_RESIDUAL: u128 = 501;
    const FINAL_RESIDUAL: u128 = 851;

    fn entitlement(actor: usize, residual: u128) -> u128 {
        FACES[actor] * residual / TOTAL_FACE
    }

    fn pay(world: &mut World, actor: usize, ix: Instruction, paid: &mut [u128; 5]) -> u128 {
        let before = world.frame();
        let token = world.actors[actor].token;
        let tokens_before = world.env.token_amount(token);
        let vault_before = world.env.market_state().1.vault;
        let receipt_before = world.receipt(actor);
        let meta = world.land(&[ix], false).expect("receipt payout or cleanup");
        assert_cu_within(
            "co-owned receipt payout",
            meta.compute_units_consumed,
            CUSTODY_CU_LIMIT,
        );
        let delta = u128::from(
            world
                .env
                .token_amount(token)
                .checked_sub(tokens_before)
                .unwrap(),
        );
        paid[actor] += delta;
        assert_eq!(
            vault_before.checked_sub(world.env.market_state().1.vault),
            Some(delta)
        );
        let receipt_after = world.receipt(actor);
        if receipt_before.present {
            if receipt_after.present {
                let mut expected = receipt_before;
                expected.paid_effective += delta;
                expected.finalized = receipt_after.finalized;
                assert_eq!(receipt_after, expected, "receipt identity is immutable");
                assert!(!receipt_before.finalized || receipt_after.finalized);
            }
            if !receipt_after.present || receipt_after.finalized {
                assert_eq!(
                    paid[actor],
                    CAPITAL[actor] + entitlement(actor, FINAL_RESIDUAL)
                );
                assert_eq!(
                    world
                        .env
                        .market_state()
                        .1
                        .resolved_payout_ledger
                        .terminal_claim_bound_unreceipted_num,
                    0
                );
            }
        }
        for index in 0..5 {
            assert!(paid[index] <= CAPITAL[index] + entitlement(index, FINAL_RESIDUAL));
            let expected = if index == 0 || index == 4 {
                paid[0] + paid[4]
            } else {
                paid[index]
            };
            assert_eq!(
                u128::from(world.env.token_amount(world.actors[index].token)),
                expected
            );
        }
        world.assert_frame_except(
            &before,
            &[
                world.env.market,
                world.env.vault,
                world.actors[actor].portfolio,
                token,
            ],
        );
        world.custody();
        delta
    }

    fn finish(world: &mut World, actor: usize, paid: &mut [u128; 5]) {
        for _ in 0..8 {
            if resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio) {
                break;
            }
            let ix = world.payout(actor, false);
            pay(world, actor, ix, paid);
        }
        assert!(resolved_portfolio_is_terminal(
            &world.env,
            world.actors[actor].portfolio
        ));
        assert!(!world.receipt(actor).present);
        assert_eq!(
            paid[actor],
            CAPITAL[actor] + entitlement(actor, FINAL_RESIDUAL)
        );
    }

    fn close(world: &mut World, actor: usize) {
        let before = world.frame();
        let portfolio = world.actors[actor].portfolio;
        let mut expected = world.env.svm.get_account(&portfolio).unwrap();
        let rent = expected.lamports;
        let market_lamports = world
            .env
            .svm
            .get_account(&world.env.market)
            .unwrap()
            .lamports;
        let count = world.env.market_state().1.materialized_portfolio_count;
        let cu = world
            .env
            .close_portfolio_with_cu(&world.actors[actor].owner, portfolio);
        assert_cu_within("co-owned receipt portfolio close", cu, CUSTODY_CU_LIMIT);
        world.peak_cu = world.peak_cu.max(cu);
        expected.lamports = 0;
        expected.data.clear();
        assert_eq!(world.env.svm.get_account(&portfolio), Some(expected));
        assert_eq!(
            world
                .env
                .svm
                .get_account(&world.env.market)
                .unwrap()
                .lamports,
            market_lamports + rent
        );
        assert_eq!(
            world.env.market_state().1.materialized_portfolio_count + 1,
            count
        );
        world.assert_frame_except(&before, &[world.env.market, portfolio]);
        world.custody();
    }

    let mut peak_cu = 0;
    let mut rollback_peak_cu = 0;
    for claim_route in [true, false] {
        let owner = Keypair::new();
        let owners = [Keypair::from_bytes(&owner.to_bytes()).unwrap(), owner];
        let mut world = World::before_receipts_with_claimant_owners(owners);
        let mut paid = [0; 5];
        let shared_token = world.actors[0].token;
        assert_eq!(shared_token, world.actors[4].token);
        assert_eq!(
            world.actors[0].owner.pubkey(),
            world.actors[4].owner.pubkey()
        );
        assert_ne!(
            world.env.portfolio_id(world.actors[0].portfolio),
            world.env.portfolio_id(world.actors[4].portfolio)
        );
        for actor in [0, 4] {
            for _ in 0..8 {
                if world.receipt(actor).present {
                    break;
                }
                let ix = world.payout(actor, false);
                pay(&mut world, actor, ix, &mut paid);
            }
            let receipt = world.receipt(actor);
            assert!(receipt.present && !receipt.finalized);
            assert_eq!(receipt.terminal_positive_claim_face, FACES[actor]);
            assert_eq!(
                receipt.prior_bound_contribution_num,
                FACES[actor] * BOUND_SCALE
            );
            assert_eq!(receipt.live_released_face_at_receipt, 0);
            assert_eq!(receipt.paid_effective, entitlement(actor, INITIAL_RESIDUAL));
            assert_eq!(paid[actor], CAPITAL[actor] + receipt.paid_effective);
        }
        let original = world.receipt(4);
        let survivor_identity = (
            world.env.portfolio_id(world.actors[4].portfolio),
            world
                .env
                .portfolio_position_epoch(world.actors[4].portfolio),
        );
        let retained_live = world.payout(4, claim_route);
        let retained_retired = world.payout(0, !claim_route);
        let snapshot = world.env.market_state().1.resolved_payout_ledger;
        assert_eq!(
            (snapshot.snapshot_slot, snapshot.snapshot_residual),
            (12, INITIAL_RESIDUAL)
        );

        world.env.svm.warp_to_slot(13);
        let release = world.payout(2, false);
        assert_eq!(pay(&mut world, 2, release, &mut paid), 0);
        finish(&mut world, 2, &mut paid);
        finish(&mut world, 0, &mut paid);
        close(&mut world, 0);
        assert_eq!(
            world.receipt(4),
            original,
            "retiring one receipt preserves its co-owned sibling"
        );
        assert_eq!(paid[0], 1_198);
        let ledger = world.env.market_state().1.resolved_payout_ledger;
        assert_eq!(ledger.snapshot_slot, snapshot.snapshot_slot);
        assert_eq!(ledger.snapshot_residual, FINAL_RESIDUAL);
        assert_eq!(ledger.current_payout_rate_num, FINAL_RESIDUAL * BOUND_SCALE);
        assert_eq!(ledger.current_payout_rate_den, TOTAL_FACE * BOUND_SCALE);
        assert_eq!(ledger.terminal_claim_bound_unreceipted_num, 0);
        let due = entitlement(4, FINAL_RESIDUAL) - original.paid_effective;
        assert_eq!(due, 151);

        // Unlike isolated stale replays, the live sibling pays into the shared ATA before
        // the retained instruction for the now-dematerialized portfolio rejects.
        let pending = world.frame();
        let mut expected_payer = world
            .env
            .svm
            .get_account(&world.env.payer.pubkey())
            .unwrap();
        expected_payer.lamports -= solana_sdk::fee::FeeStructure::default().lamports_per_signature;
        let failure = world
            .land(&[retained_live.clone(), retained_retired.clone()], false)
            .expect_err("retired receipt rejects after the live sibling's payout");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(
                3,
                InstructionError::Custom(PercolatorError::NotInitialized as u32)
            )
        );
        for program in [world.env.program_id, spl_token::ID] {
            assert_eq!(
                failure
                    .meta
                    .logs
                    .iter()
                    .filter(|line| **line == format!("Program {program} success"))
                    .count(),
                1,
                "the successful wrapper/SPL prefix must execute before the stale tail"
            );
        }
        assert_cu_within(
            "co-owned stale-tail rollback",
            failure.meta.compute_units_consumed,
            600_000,
        );
        rollback_peak_cu = rollback_peak_cu.max(failure.meta.compute_units_consumed);
        assert_eq!(
            world.frame(),
            pending,
            "rollback includes both receipt identities, shared custody and rent"
        );
        assert_eq!(
            world.env.svm.get_account(&world.env.payer.pubkey()),
            Some(expected_payer)
        );
        world.custody();

        assert_eq!(pay(&mut world, 4, retained_live.clone(), &mut paid), due);
        assert_eq!(world.env.market_state().1.resolved_payout_ledger, ledger);
        assert_eq!(
            (
                world.env.portfolio_id(world.actors[4].portfolio),
                world
                    .env
                    .portfolio_position_epoch(world.actors[4].portfolio)
            ),
            survivor_identity
        );
        assert_eq!(paid[4], 1_368);
        assert_eq!(world.env.token_amount(shared_token), 2_566);
        assert_eq!((FACES[0] + FACES[4]) * FINAL_RESIDUAL / TOTAL_FACE, 567);
        assert_eq!(
            paid[0] + paid[4] - CAPITAL[0] - CAPITAL[4],
            566,
            "co-ownership does not merge receipt rounding"
        );

        // Terminal haircut cleanup may clear the exhausted receipt without paying again.
        // Only the resulting fixed point is required to be byte-idempotent across rails.
        finish(&mut world, 4, &mut paid);
        for ix in [retained_live, world.payout(4, !claim_route)] {
            let before = world.frame();
            let meta = match world.land(&[ix], false) {
                Ok(meta) => meta,
                Err(failure) => {
                    assert_eq!(
                        failure.err,
                        TransactionError::InstructionError(
                            2,
                            InstructionError::Custom(PercolatorError::EngineNonProgress as u32)
                        )
                    );
                    failure.meta
                }
            };
            assert_cu_within(
                "co-owned receipt replay",
                meta.compute_units_consumed,
                CUSTODY_CU_LIMIT,
            );
            assert_eq!(
                world.frame(),
                before,
                "fresh-blockhash replay cannot repay the surviving receipt"
            );
            world.custody();
        }
        for actor in [4, 1, 2, 3] {
            finish(&mut world, actor, &mut paid);
            close(&mut world, actor);
        }
        assert_eq!(paid, [1_198, 0, 1_283, 0, 1_368]);
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
            FINAL_RESIDUAL
                - (0..5)
                    .map(|actor| entitlement(actor, FINAL_RESIDUAL))
                    .sum::<u128>()
        );
        assert_eq!(group.vault, 2);
        assert_eq!(world.env.token_amount(world.provider_token), 1);
        world.custody();
        peak_cu = peak_cu.max(world.peak_cu);
    }
    println!("INV-066/067/068 co-owned stale-tail: 2 worlds, 2 paid-prefix rollbacks, 2 exact 151-atom retries, 4 zero-payout replays, 10 portfolio closes; rollback peak CU {rollback_peak_cu}, suffix peak CU {peak_cu}");
}

#[test]
fn v16_program_same_owner_receipts_keep_independent_topups_and_terminal_replays() {
    use super::inv_067_terminal_payout_completeness_and_exact_once_settlement::late_expiry::World;
    use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

    const FACES: [u128; 5] = [14 * 50, 0, 20 * 50, 0, 26 * 50];
    const CAPITAL: [u128; 5] = [1_000, 0, 1_000, 0, 1_000];
    const TOTAL_FACE: u128 = 3_000;
    const INITIAL_RESIDUAL: u128 = 501;
    const FINAL_RESIDUAL: u128 = 851;

    fn entitlement(actor: usize, residual: u128) -> u128 {
        FACES[actor] * residual / TOTAL_FACE
    }

    fn step(
        world: &mut World,
        actor: usize,
        instruction: Instruction,
        paid: &mut [u128; 5],
    ) -> Option<u128> {
        let before = world.frame();
        let token = world.actors[actor].token;
        let tokens_before = world.env.token_amount(token);
        let vault_before = world.env.market_state().1.vault;
        let receipt_before = world.receipt(actor);
        let result = world.land(&[instruction], false);
        let meta = match &result {
            Ok(meta) => meta,
            Err(failure) => &failure.meta,
        };
        assert_cu_within(
            "shared-owner receipt step",
            meta.compute_units_consumed,
            CUSTODY_CU_LIMIT,
        );
        match result {
            Ok(_) => {
                let delta = u128::from(
                    world
                        .env
                        .token_amount(token)
                        .checked_sub(tokens_before)
                        .unwrap(),
                );
                paid[actor] += delta;
                assert_eq!(
                    vault_before.checked_sub(world.env.market_state().1.vault),
                    Some(delta)
                );
                if receipt_before.present {
                    let receipt_after = world.receipt(actor);
                    if receipt_after.present {
                        let mut expected = receipt_before;
                        expected.paid_effective += delta;
                        expected.finalized = receipt_after.finalized;
                        assert_eq!(
                            receipt_after, expected,
                            "only paid value and finalization may change"
                        );
                        assert!(receipt_after.paid_effective <= entitlement(actor, FINAL_RESIDUAL));
                        assert!(!receipt_before.finalized || receipt_after.finalized);
                    }
                    if !receipt_after.present || receipt_after.finalized {
                        assert_eq!(
                            paid[actor],
                            CAPITAL[actor] + entitlement(actor, FINAL_RESIDUAL)
                        );
                        assert_eq!(
                            world
                                .env
                                .market_state()
                                .1
                                .resolved_payout_ledger
                                .terminal_claim_bound_unreceipted_num,
                            0
                        );
                    }
                }
                world.assert_frame_except(
                    &before,
                    &[
                        world.env.market,
                        world.env.vault,
                        world.actors[actor].portfolio,
                        token,
                    ],
                );
                for index in 0..5 {
                    assert!(paid[index] <= CAPITAL[index] + entitlement(index, FINAL_RESIDUAL));
                    let expected = if index == 0 || index == 4 {
                        paid[0] + paid[4]
                    } else {
                        paid[index]
                    };
                    assert_eq!(
                        u128::from(world.env.token_amount(world.actors[index].token)),
                        expected
                    );
                }
                world.custody();
                Some(delta)
            }
            Err(failure) => {
                assert_eq!(
                    failure.err,
                    TransactionError::InstructionError(
                        2,
                        InstructionError::Custom(PercolatorError::EngineNonProgress as u32),
                    )
                );
                assert_eq!(
                    world.frame(),
                    before,
                    "nonprogress must roll back every tracked account"
                );
                world.custody();
                None
            }
        }
    }

    fn finish(world: &mut World, actor: usize, paid: &mut [u128; 5]) {
        for _ in 0..8 {
            if resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio) {
                break;
            }
            let instruction = world.payout(actor, false);
            step(world, actor, instruction, paid);
        }
        assert!(resolved_portfolio_is_terminal(
            &world.env,
            world.actors[actor].portfolio
        ));
        assert_eq!(
            paid[actor],
            CAPITAL[actor] + entitlement(actor, FINAL_RESIDUAL)
        );
        assert!(
            !world.receipt(actor).present,
            "terminal drain must clear the embedded receipt"
        );
    }

    fn close(world: &mut World, actor: usize) {
        let before = world.frame();
        let portfolio = world.actors[actor].portfolio;
        let mut closed_account = world.env.svm.get_account(&portfolio).unwrap();
        let rent = closed_account.lamports;
        let market_rent = world
            .env
            .svm
            .get_account(&world.env.market)
            .unwrap()
            .lamports;
        let count = world.env.market_state().1.materialized_portfolio_count;
        let cu = world
            .env
            .close_portfolio_with_cu(&world.actors[actor].owner, portfolio);
        assert_cu_within("shared-owner portfolio close", cu, CUSTODY_CU_LIMIT);
        world.peak_cu = world.peak_cu.max(cu);
        closed_account.lamports = 0;
        closed_account.data.clear();
        assert_eq!(world.env.svm.get_account(&portfolio), Some(closed_account));
        assert_eq!(
            world
                .env
                .svm
                .get_account(&world.env.market)
                .unwrap()
                .lamports,
            market_rent + rent
        );
        assert_eq!(
            world.env.market_state().1.materialized_portfolio_count + 1,
            count
        );
        world.assert_frame_except(&before, &[world.env.market, portfolio]);
        world.custody();
    }

    fn reject_stale(world: &mut World, instruction: Instruction) {
        let before = world.frame();
        let failure = world
            .land(&[instruction], false)
            .expect_err("dematerialized portfolio cannot claim");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(
                2,
                InstructionError::Custom(PercolatorError::NotInitialized as u32),
            )
        );
        assert_cu_within(
            "shared-owner stale claim",
            failure.meta.compute_units_consumed,
            CUSTODY_CU_LIMIT,
        );
        assert_eq!(
            world.frame(),
            before,
            "stale claim cannot revive a receipt or consume its sibling"
        );
        world.custody();
    }

    let initial_shared = entitlement(0, INITIAL_RESIDUAL) + entitlement(4, INITIAL_RESIDUAL);
    let final_shared = entitlement(0, FINAL_RESIDUAL) + entitlement(4, FINAL_RESIDUAL);
    assert_eq!((initial_shared, final_shared), (333, 566));
    assert_eq!(
        (FACES[0] + FACES[4]) * FINAL_RESIDUAL / TOTAL_FACE,
        final_shared + 1,
        "merging co-owned faces would overpay one atom"
    );
    let mut peak_cu = 0;
    let identity = |world: &World, actor: usize| {
        let portfolio = world.actors[actor].portfolio;
        (
            world.env.portfolio_id(portfolio),
            world.env.portfolio_position_epoch(portfolio),
            state::read_portfolio_owner_preflight(
                &world.env.svm.get_account(&portfolio).unwrap().data,
            )
            .unwrap(),
        )
    };
    for claim_route in [false, true] {
        for first in [0, 4] {
            let sibling = 4 - first;
            let owner = Keypair::new();
            let owners = [Keypair::from_bytes(&owner.to_bytes()).unwrap(), owner];
            let mut world = World::before_receipts_with_claimant_owners(owners);
            assert_eq!(
                world.actors[0].owner.pubkey(),
                world.actors[4].owner.pubkey()
            );
            assert_eq!(world.actors[0].token, world.actors[4].token);
            assert_ne!(world.actors[0].portfolio, world.actors[4].portfolio);
            assert_ne!(
                world.env.portfolio_id(world.actors[0].portfolio),
                world.env.portfolio_id(world.actors[4].portfolio)
            );
            let identities = [0, 4].map(|actor| identity(&world, actor));
            let mut paid = [0; 5];
            assert!(world
                .actors
                .iter()
                .all(|actor| world.env.token_amount(actor.token) == 0));
            for actor in [first, sibling] {
                for _ in 0..8 {
                    if world.receipt(actor).present {
                        break;
                    }
                    let instruction = world.payout(actor, false);
                    step(&mut world, actor, instruction, &mut paid)
                        .expect("public receipt creation must progress");
                }
                let receipt = world.receipt(actor);
                assert!(receipt.present && !receipt.finalized);
                assert_eq!(receipt.terminal_positive_claim_face, FACES[actor]);
                assert_eq!(
                    receipt.prior_bound_contribution_num,
                    FACES[actor] * BOUND_SCALE
                );
                assert_eq!(receipt.live_released_face_at_receipt, 0);
                assert_eq!(receipt.paid_effective, entitlement(actor, INITIAL_RESIDUAL));
                assert_eq!(paid[actor], CAPITAL[actor] + receipt.paid_effective);
            }
            let snapshot = world.env.market_state().1.resolved_payout_ledger;
            assert_eq!(snapshot.snapshot_slot, 12);
            assert_eq!(snapshot.snapshot_residual, INITIAL_RESIDUAL);
            assert_eq!(
                snapshot.current_payout_rate_num,
                INITIAL_RESIDUAL * BOUND_SCALE
            );
            assert_eq!(snapshot.current_payout_rate_den, TOTAL_FACE * BOUND_SCALE);
            let originals = [world.receipt(first), world.receipt(sibling)];
            let retained = world.payout(first, claim_route);
            // This one-field substitution is valid: the owner and destination are shared.
            // It must select the sibling's own face, not reject or reuse the first receipt.
            let mut retargeted = retained.clone();
            retargeted.accounts[2].pubkey = world.actors[sibling].portfolio;
            assert_eq!(retargeted, world.payout(sibling, claim_route));
            world.env.svm.warp_to_slot(13);
            let release = world.payout(2, false);
            assert_eq!(step(&mut world, 2, release, &mut paid), Some(0));
            assert_eq!([world.receipt(first), world.receipt(sibling)], originals);
            let ledger = world.env.market_state().1.resolved_payout_ledger;
            assert_eq!(ledger.snapshot_slot, snapshot.snapshot_slot);
            assert_eq!(ledger.snapshot_residual, FINAL_RESIDUAL);
            assert_eq!(ledger.current_payout_rate_num, FINAL_RESIDUAL * BOUND_SCALE);
            assert_eq!(ledger.current_payout_rate_den, TOTAL_FACE * BOUND_SCALE);
            assert_eq!(
                ledger.terminal_claim_exact_receipts_num,
                (FACES[0] + FACES[4]) * BOUND_SCALE
            );
            assert_eq!(
                ledger.terminal_claim_bound_unreceipted_num,
                FACES[2] * BOUND_SCALE
            );

            let due = entitlement(first, FINAL_RESIDUAL) - originals[0].paid_effective;
            assert!(due > 0);
            assert_eq!(
                step(&mut world, first, retained.clone(), &mut paid),
                Some(due)
            );
            let mut expected_receipt = originals[0];
            expected_receipt.paid_effective += due;
            assert_eq!(world.receipt(first), expected_receipt);
            assert_eq!(world.receipt(sibling), originals[1]);
            assert_eq!(world.env.market_state().1.resolved_payout_ledger, ledger);
            for (index, actor) in [0, 4].into_iter().enumerate() {
                assert_eq!(identity(&world, actor), identities[index]);
            }
            for route in [false, true] {
                let before = world.frame();
                let replay = world.payout(first, route);
                assert!(matches!(
                    step(&mut world, first, replay, &mut paid),
                    Some(0) | None
                ));
                assert_eq!(
                    world.frame(),
                    before,
                    "shared destination does not make a paid receipt due again"
                );
            }

            // Remove the remaining bound, then retire only the paid co-owned portfolio.
            // Its sibling must remain a live, partially paid receipt throughout the drain.
            finish(&mut world, 2, &mut paid);
            assert_eq!(
                world
                    .env
                    .market_state()
                    .1
                    .resolved_payout_ledger
                    .terminal_claim_bound_unreceipted_num,
                0
            );
            finish(&mut world, first, &mut paid);
            assert_eq!(world.receipt(sibling), originals[1]);
            close(&mut world, first);
            for route in [false, true] {
                let stale = world.payout(first, route);
                reject_stale(&mut world, stale);
            }
            assert_eq!(world.receipt(sibling), originals[1]);
            let sibling_due = entitlement(sibling, FINAL_RESIDUAL) - originals[1].paid_effective;
            assert!(sibling_due > 0 && sibling_due != due);
            assert_eq!(
                step(&mut world, sibling, retargeted, &mut paid),
                Some(sibling_due)
            );
            assert_eq!(
                identity(&world, sibling),
                identities[usize::from(sibling == 4)]
            );
            assert_eq!(
                world.env.token_amount(world.actors[sibling].token) as u128,
                CAPITAL[0] + CAPITAL[4] + final_shared
            );
            for actor in [sibling, 1, 2, 3] {
                finish(&mut world, actor, &mut paid);
                close(&mut world, actor);
            }
            for actor in [first, sibling] {
                for route in [false, true] {
                    let stale = world.payout(actor, route);
                    reject_stale(&mut world, stale);
                }
            }
            assert_eq!(
                paid,
                std::array::from_fn(|actor| CAPITAL[actor] + entitlement(actor, FINAL_RESIDUAL))
            );
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
                FINAL_RESIDUAL
                    - (0..5)
                        .map(|actor| entitlement(actor, FINAL_RESIDUAL))
                        .sum::<u128>()
            );
            assert_eq!(group.vault, 2);
            world.custody();
            peak_cu = peak_cu.max(world.peak_cu);
        }
    }
    println!("INV-068 shared owner: 4 worlds, 8 independent positive top-ups, 8 exact live retries, 24 stale rollbacks; shared claim 566 (not merged 567); peak CU {peak_cu}");
}

#[test]
fn v16_program_resolved_receipt_replays_extract_no_value_on_any_public_rail() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let winner_owner = Keypair::new();
    let winner = env.create_portfolio(&winner_owner);
    let loser_owner = Keypair::new();
    let loser = env.create_portfolio(&loser_owner);
    env.deposit(&winner_owner, winner, 1_000_000);
    env.deposit(&loser_owner, loser, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &winner_owner,
        winner,
        &loser_owner,
        loser,
        (10_000 * POS_SCALE) as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110);
    for target in [loser, winner] {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 10,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(target, false),
            ],
            &[],
        )
        .expect("public crank refreshes both accounts before terminal resolution");
    }

    env.resolve();
    let loser_dest = env.close_resolved(&loser_owner, loser);
    assert_eq!(
        env.token_amount(loser_dest),
        900_000,
        "loser payout funds the terminal vault before winner settlement",
    );
    let winner_dest = env.close_resolved(&winner_owner, winner);
    assert_eq!(env.token_amount(winner_dest), 1_100_000);
    let (_, after_first_close) = env.market_state();
    let winner_after_first = env.portfolio_state(winner);
    let receipt_after_first = resolved_receipt(&winner_after_first);
    assert!(
        receipt_after_first.finalized || !receipt_after_first.present,
        "the first public close must exhaust or clear the winner receipt",
    );
    assert_eq!(after_first_close.vault as u64, env.token_amount(env.vault));

    for route in ["CloseResolved", "ClaimResolvedPayoutTopup"] {
        let replay_dest = env.token_account(winner_owner.pubkey(), 0);
        let before_market = env.svm.get_account(&env.market).unwrap();
        let before_winner = env.svm.get_account(&winner).unwrap();
        let before_dest = env.svm.get_account(&replay_dest).unwrap();
        let before_vault = env.svm.get_account(&env.vault).unwrap();
        env.svm.expire_blockhash();
        let _ = match route {
            "CloseResolved" => env.send(
                ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                },
                vec![
                    AccountMeta::new_readonly(winner_owner.pubkey(), false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(winner, false),
                    AccountMeta::new(replay_dest, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[],
            ),
            "ClaimResolvedPayoutTopup" => env.send(
                ProgInstruction::ClaimResolvedPayoutTopup,
                vec![
                    AccountMeta::new_readonly(winner_owner.pubkey(), false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(winner, false),
                    AccountMeta::new(replay_dest, false),
                    AccountMeta::new(env.vault, false),
                    AccountMeta::new_readonly(env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
                &[],
            ),
            _ => unreachable!(),
        };
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            before_market,
            "{route} replay must not mutate terminal market accounting",
        );
        assert_eq!(
            env.svm.get_account(&winner).unwrap(),
            before_winner,
            "{route} replay must not mutate the finalized receipt",
        );
        assert_eq!(
            env.svm.get_account(&replay_dest).unwrap(),
            before_dest,
            "{route} replay must not pay a second token",
        );
        assert_eq!(
            env.svm.get_account(&env.vault).unwrap(),
            before_vault,
            "{route} replay must not move vault custody",
        );
    }

    let stranger_dest = env.token_account(loser_owner.pubkey(), 0);
    let before_market = env.svm.get_account(&env.market).unwrap();
    let before_winner = env.svm.get_account(&winner).unwrap();
    let before_stranger_dest = env.svm.get_account(&stranger_dest).unwrap();
    env.svm.expire_blockhash();
    let stranger_claim = env.send(
        ProgInstruction::ClaimResolvedPayoutTopup,
        vec![
            AccountMeta::new_readonly(loser_owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(winner, false),
            AccountMeta::new(stranger_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        stranger_claim.is_err(),
        "a different owner pubkey cannot claim another portfolio's receipt",
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), before_market);
    assert_eq!(env.svm.get_account(&winner).unwrap(), before_winner);
    assert_eq!(
        env.svm.get_account(&stranger_dest).unwrap(),
        before_stranger_dest
    );
    assert_eq!(
        env.market_state().1.vault as u64,
        env.token_amount(env.vault)
    );
}

#[test]
fn v16_program_resolved_payout_secondary_rail_exhausts_shared_receipt() {
    let mut env = V16CuEnv::new();
    let secondary = env.create_mint();
    env.update_base_unit_mints_with_cu(env.mint, secondary);
    let secondary_vault = canonical_vault_ata(env.vault_authority, secondary);
    env.svm
        .set_account(
            secondary_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(secondary, env.vault_authority, 1_100_000),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    env.configure_auth_mark_with_cu(0, 100);
    let winner_owner = Keypair::new();
    let winner = env.create_portfolio(&winner_owner);
    let loser_owner = Keypair::new();
    let loser = env.create_portfolio(&loser_owner);
    env.deposit(&winner_owner, winner, 1_000_000);
    env.deposit(&loser_owner, loser, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &winner_owner,
        winner,
        &loser_owner,
        loser,
        (10_000 * POS_SCALE) as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110);
    for slot in [10u64, 11] {
        env.svm.warp_to_slot(slot);
        for portfolio in [loser, winner] {
            let _ = env.send_crank_if_actionable(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(portfolio, false),
                ],
                &[],
            );
        }
    }
    env.resolve();

    let _ = env.close_resolved(&loser_owner, loser);
    assert_eq!(env.market_state().1.vault, 1_100_000);
    assert_eq!(env.token_amount(env.vault), 1_100_000);

    let secondary_dest = env.token_account_for_mint(secondary, winner_owner.pubkey(), 0);
    env.svm.expire_blockhash();
    let secondary_close_cu = env
        .send(
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            },
            vec![
                AccountMeta::new_readonly(winner_owner.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(winner, false),
                AccountMeta::new(secondary_dest, false),
                AccountMeta::new(secondary_vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[],
        )
        .expect("winner closes through the secondary reserve");
    assert_cu_within(
        "CloseResolved secondary payout",
        secondary_close_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(env.token_amount(secondary_dest), 1_100_000);
    assert_eq!(env.token_amount(secondary_vault), 0);
    let winner_after_secondary = env.portfolio_state(winner);
    assert!(
        resolved_receipt(&winner_after_secondary).finalized
            || !resolved_receipt(&winner_after_secondary).present
    );
    assert_eq!(
        env.market_state().1.vault,
        0,
        "shared accounting vault exhausted after secondary payout"
    );

    let primary_close_dest = env.token_account_for_mint(env.mint, winner_owner.pubkey(), 0);
    let market_before_primary_retry = env.svm.get_account(&env.market).unwrap();
    let winner_before_primary_retry = env.svm.get_account(&winner).unwrap();
    let vault_before_primary_retry = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let _ = env.send(
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        vec![
            AccountMeta::new_readonly(winner_owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(winner, false),
            AccountMeta::new(primary_close_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_primary_retry
    );
    assert_eq!(
        env.svm.get_account(&winner).unwrap(),
        winner_before_primary_retry
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before_primary_retry
    );
    assert_eq!(env.token_amount(primary_close_dest), 0);
    assert_eq!(env.token_amount(env.vault), 1_100_000);

    let primary_topup_dest = env.token_account_for_mint(env.mint, winner_owner.pubkey(), 0);
    let market_before_topup_retry = env.svm.get_account(&env.market).unwrap();
    let winner_before_topup_retry = env.svm.get_account(&winner).unwrap();
    let vault_before_topup_retry = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let _ = env.send(
        ProgInstruction::ClaimResolvedPayoutTopup,
        vec![
            AccountMeta::new_readonly(winner_owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(winner, false),
            AccountMeta::new(primary_topup_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_topup_retry
    );
    assert_eq!(
        env.svm.get_account(&winner).unwrap(),
        winner_before_topup_retry
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before_topup_retry
    );
    assert_eq!(env.token_amount(primary_topup_dest), 0);
    assert_eq!(env.token_amount(env.vault), 1_100_000);
    assert_eq!(env.market_state().1.vault, 0);
}

// the portfolio owner's valid collateral account. A bad destination must not burn the receipt.
#[test]
fn v16_program_resolved_payout_topup_bad_dest_does_not_burn_receipt() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    {
        let mut market_account = env.svm.get_account(&env.market).expect("market account");
        let mut portfolio_account = env.svm.get_account(&portfolio).expect("portfolio account");
        let (cfg, mut group) = state::read_market(&market_account.data).unwrap();
        let mut account = state::read_portfolio(&portfolio_account.data).unwrap();
        group.mode = MarketModeV16::Resolved;
        group.resolved_slot = 1;
        group.current_slot = 1;
        group.vault = 60;
        group.payout_snapshot_captured = true;
        group.payout_snapshot = 100;
        group.resolved_payout_ledger = ResolvedPayoutLedgerV16 {
            snapshot_residual: 100,
            terminal_claim_exact_receipts_num: 100 * BOUND_SCALE,
            terminal_claim_bound_unreceipted_num: 0,
            current_payout_rate_num: 100 * BOUND_SCALE,
            current_payout_rate_den: 100 * BOUND_SCALE,
            snapshot_slot: 1,
            payout_halted: false,
            finalized: false,
        };
        account.resolved_payout_receipt =
            percolator::ResolvedPayoutReceiptV16Account::from_runtime(&ResolvedPayoutReceiptV16 {
                present: true,
                prior_bound_contribution_num: 100 * BOUND_SCALE,
                live_released_face_at_receipt: 0,
                terminal_positive_claim_face: 100,
                paid_effective: 40,
                finalized: false,
            });
        state::write_market(&mut market_account.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut portfolio_account.data, &account).unwrap();
        env.svm.set_account(env.market, market_account).unwrap();
        env.svm.set_account(portfolio, portfolio_account).unwrap();
    }
    env.set_token_account_amount(env.vault, env.mint, env.vault_authority, 60);

    let attacker = Keypair::new();
    let foreign_dest = env.token_account_for_mint(env.mint, attacker.pubkey(), 0);
    env.svm.expire_blockhash();
    let foreign = env.send(
        ProgInstruction::ClaimResolvedPayoutTopup,
        vec![
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(foreign_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        foreign.is_err(),
        "top-up to a third-party destination must reject"
    );
    assert_eq!(
        env.token_amount(foreign_dest),
        0,
        "no payout to attacker dest"
    );

    let wrong_mint = Pubkey::new_unique();
    let wrong_mint_dest = env.token_account_for_mint(wrong_mint, owner.pubkey(), 0);
    env.svm.expire_blockhash();
    let wrong_mint_claim = env.send(
        ProgInstruction::ClaimResolvedPayoutTopup,
        vec![
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(wrong_mint_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        wrong_mint_claim.is_err(),
        "top-up to a wrong-mint destination must reject"
    );

    let account = env.portfolio_state(portfolio);
    assert_eq!(
        resolved_receipt(&account).paid_effective,
        40,
        "rejected bad destinations must not burn the pending receipt"
    );
    assert!(
        !resolved_receipt(&account).finalized,
        "receipt remains claimable after rejected bad destinations"
    );
    assert_eq!(env.market_state().1.vault, 60, "accounting vault unchanged");
    assert_eq!(env.token_amount(env.vault), 60, "real vault unchanged");

    let good_dest = env.token_account_for_mint(env.mint, owner.pubkey(), 0);
    let cu = env.claim_resolved_payout_topup_with_cu(owner.pubkey(), portfolio, good_dest);
    assert_cu_within(
        "ClaimResolvedPayoutTopup bad-dest regression",
        cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        env.token_amount(good_dest),
        60,
        "correct destination receives the pending top-up"
    );
    let account = env.portfolio_state(portfolio);
    assert_eq!(resolved_receipt(&account).paid_effective, 100);
    assert!(resolved_receipt(&account).finalized);
    assert_eq!(env.market_state().1.vault, 0);
}

// intentionally unsigned so a third party can help finish a user's payout, but it must only pay to
#[test]
fn v16_program_resolved_payout_topup_rejects_delegated_dest_without_burning_receipt() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    {
        let mut market_account = env.svm.get_account(&env.market).expect("market account");
        let mut portfolio_account = env.svm.get_account(&portfolio).expect("portfolio account");
        let (cfg, mut group) = state::read_market(&market_account.data).unwrap();
        let mut account = state::read_portfolio(&portfolio_account.data).unwrap();
        group.mode = MarketModeV16::Resolved;
        group.resolved_slot = 1;
        group.current_slot = 1;
        group.vault = 60;
        group.payout_snapshot_captured = true;
        group.payout_snapshot = 100;
        group.resolved_payout_ledger = ResolvedPayoutLedgerV16 {
            snapshot_residual: 100,
            terminal_claim_exact_receipts_num: 100 * BOUND_SCALE,
            terminal_claim_bound_unreceipted_num: 0,
            current_payout_rate_num: 100 * BOUND_SCALE,
            current_payout_rate_den: 100 * BOUND_SCALE,
            snapshot_slot: 1,
            payout_halted: false,
            finalized: false,
        };
        account.resolved_payout_receipt =
            percolator::ResolvedPayoutReceiptV16Account::from_runtime(&ResolvedPayoutReceiptV16 {
                present: true,
                prior_bound_contribution_num: 100 * BOUND_SCALE,
                live_released_face_at_receipt: 0,
                terminal_positive_claim_face: 100,
                paid_effective: 40,
                finalized: false,
            });
        state::write_market(&mut market_account.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut portfolio_account.data, &account).unwrap();
        env.svm.set_account(env.market, market_account).unwrap();
        env.svm.set_account(portfolio, portfolio_account).unwrap();
    }
    env.set_token_account_amount(env.vault, env.mint, env.vault_authority, 60);

    let attacker = Keypair::new();
    let delegated_dest = Pubkey::new_unique();
    env.svm
        .set_account(
            delegated_dest,
            Account {
                lamports: 1_000_000_000,
                data: make_delegated_token_data(
                    env.mint,
                    owner.pubkey(),
                    0,
                    attacker.pubkey(),
                    u64::MAX,
                ),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let dest_before = env.svm.get_account(&delegated_dest).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::ClaimResolvedPayoutTopup,
        vec![
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(delegated_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        rejected.is_err(),
        "ClaimResolvedPayoutTopup must reject an owner destination with an active delegate"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected delegated-dest top-up leaves payout accounting unchanged"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "rejected delegated-dest top-up must not burn the pending receipt"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected delegated-dest top-up moves no vault custody"
    );
    assert_eq!(
        env.svm.get_account(&delegated_dest).unwrap(),
        dest_before,
        "delegated destination receives no payout"
    );

    let closable_dest = Pubkey::new_unique();
    env.svm
        .set_account(
            closable_dest,
            Account {
                lamports: 1_000_000_000,
                data: make_closable_token_data(env.mint, owner.pubkey(), 0, attacker.pubkey()),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let closable_before = env.svm.get_account(&closable_dest).unwrap();
    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::ClaimResolvedPayoutTopup,
        vec![
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(closable_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        rejected.is_err(),
        "ClaimResolvedPayoutTopup must reject an owner destination with close authority"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&portfolio).unwrap(), portfolio_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    assert_eq!(
        env.svm.get_account(&closable_dest).unwrap(),
        closable_before,
        "close-authority destination receives no payout"
    );

    let account = env.portfolio_state(portfolio);
    assert_eq!(resolved_receipt(&account).paid_effective, 40);
    assert!(
        !resolved_receipt(&account).finalized,
        "receipt remains claimable after delegated-destination rejection"
    );

    let clean_dest = env.token_account_for_mint(env.mint, owner.pubkey(), 0);
    let cu = env.claim_resolved_payout_topup_with_cu(owner.pubkey(), portfolio, clean_dest);
    assert_cu_within(
        "ClaimResolvedPayoutTopup delegated-dest regression",
        cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        env.token_amount(clean_dest),
        60,
        "same top-up succeeds after retrying with a clean owner destination"
    );
    let account = env.portfolio_state(portfolio);
    assert_eq!(resolved_receipt(&account).paid_effective, 100);
    assert!(resolved_receipt(&account).finalized);
    assert_eq!(env.market_state().1.vault, 0);
}

// canonical vault must reject atomically, without burning the remaining receipt or moving custody.
#[test]
fn v16_program_claim_resolved_topup_rejects_delegated_vault_without_burning_receipt() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    {
        let mut market_account = env.svm.get_account(&env.market).expect("market account");
        let mut portfolio_account = env.svm.get_account(&portfolio).expect("portfolio account");
        let (cfg, mut group) = state::read_market(&market_account.data).unwrap();
        let mut account = state::read_portfolio(&portfolio_account.data).unwrap();
        group.mode = MarketModeV16::Resolved;
        group.resolved_slot = 1;
        group.current_slot = 1;
        group.vault = 60;
        group.payout_snapshot_captured = true;
        group.payout_snapshot = 100;
        group.resolved_payout_ledger = ResolvedPayoutLedgerV16 {
            snapshot_residual: 100,
            terminal_claim_exact_receipts_num: 100 * BOUND_SCALE,
            terminal_claim_bound_unreceipted_num: 0,
            current_payout_rate_num: 100 * BOUND_SCALE,
            current_payout_rate_den: 100 * BOUND_SCALE,
            snapshot_slot: 1,
            payout_halted: false,
            finalized: false,
        };
        account.resolved_payout_receipt =
            percolator::ResolvedPayoutReceiptV16Account::from_runtime(&ResolvedPayoutReceiptV16 {
                present: true,
                prior_bound_contribution_num: 100 * BOUND_SCALE,
                live_released_face_at_receipt: 0,
                terminal_positive_claim_face: 100,
                paid_effective: 40,
                finalized: false,
            });
        state::write_market(&mut market_account.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut portfolio_account.data, &account).unwrap();
        env.svm.set_account(env.market, market_account).unwrap();
        env.svm.set_account(portfolio, portfolio_account).unwrap();
    }

    let mut delegated_vault = vec![0u8; TokenAccount::LEN];
    TokenAccount::pack(
        TokenAccount {
            mint: env.mint,
            owner: env.vault_authority,
            amount: 60,
            delegate: COption::Some(Pubkey::new_unique()),
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 60,
            close_authority: COption::None,
        },
        &mut delegated_vault,
    )
    .unwrap();
    env.svm
        .set_account(
            env.vault,
            Account {
                lamports: 1_000_000_000,
                data: delegated_vault,
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let dest = env.token_account_for_mint(env.mint, owner.pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let dest_before = env.svm.get_account(&dest).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::ClaimResolvedPayoutTopup,
        vec![
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        rejected.is_err(),
        "ClaimResolvedPayoutTopup must reject a delegated canonical vault"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected delegated-vault top-up must not mutate market payout accounting"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "rejected delegated-vault top-up must not burn the pending receipt"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "delegated vault remains untouched"
    );
    assert_eq!(
        env.svm.get_account(&dest).unwrap(),
        dest_before,
        "destination receives nothing on rejected delegated-vault top-up"
    );
    let account = env.portfolio_state(portfolio);
    assert_eq!(resolved_receipt(&account).paid_effective, 40);
    assert!(
        !resolved_receipt(&account).finalized,
        "receipt remains claimable after delegated-vault rejection"
    );

    env.svm
        .set_account(
            env.vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, env.vault_authority, 60),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    let cu = env.claim_resolved_payout_topup_with_cu(owner.pubkey(), portfolio, dest);
    assert_cu_within(
        "ClaimResolvedPayoutTopup delegated-vault rollback",
        cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        env.token_amount(dest),
        60,
        "same top-up succeeds after the vault is restored clean"
    );
    let account = env.portfolio_state(portfolio);
    assert_eq!(resolved_receipt(&account).paid_effective, 100);
    assert!(resolved_receipt(&account).finalized);
    assert_eq!(env.market_state().1.vault, 0);
}

#[test]
fn v16_attack_permissionless_close_resolved_rejects_delegated_dest() {
    let mut env = V16CuEnv::new();
    let victim_owner = Keypair::new();
    let victim = env.create_portfolio(&victim_owner);
    env.deposit(&victim_owner, victim, 1_000);
    env.resolve();

    let attacker = Keypair::new();
    let delegated_dest = Pubkey::new_unique();
    env.svm
        .set_account(
            delegated_dest,
            Account {
                lamports: 1_000_000_000,
                data: make_delegated_token_data(
                    env.mint,
                    victim_owner.pubkey(),
                    0,
                    attacker.pubkey(),
                    u64::MAX,
                ),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&victim).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let dest_before = env.svm.get_account(&delegated_dest).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        vec![
            AccountMeta::new_readonly(victim_owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(victim, false),
            AccountMeta::new(delegated_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        rejected.is_err(),
        "permissionless CloseResolved must reject a victim-owned destination with an active delegate"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected delegated-dest close leaves market accounting unchanged"
    );
    assert_eq!(
        env.svm.get_account(&victim).unwrap(),
        portfolio_before,
        "rejected delegated-dest close rolls back payout state"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected delegated-dest close moves no vault custody"
    );
    assert_eq!(
        env.svm.get_account(&delegated_dest).unwrap(),
        dest_before,
        "delegated destination receives no payout"
    );

    let closable_dest = Pubkey::new_unique();
    env.svm
        .set_account(
            closable_dest,
            Account {
                lamports: 1_000_000_000,
                data: make_closable_token_data(
                    env.mint,
                    victim_owner.pubkey(),
                    0,
                    attacker.pubkey(),
                ),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let closable_before = env.svm.get_account(&closable_dest).unwrap();
    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        vec![
            AccountMeta::new_readonly(victim_owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(victim, false),
            AccountMeta::new(closable_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        rejected.is_err(),
        "permissionless CloseResolved must reject a victim-owned destination with close authority"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&victim).unwrap(), portfolio_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    assert_eq!(
        env.svm.get_account(&closable_dest).unwrap(),
        closable_before,
        "close-authority destination receives no payout"
    );

    let clean_dest = env.token_account(victim_owner.pubkey(), 0);
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        vec![
            AccountMeta::new_readonly(victim_owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(victim, false),
            AccountMeta::new(clean_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    )
    .expect("same permissionless close succeeds with a clean victim destination");
    assert_eq!(env.token_amount(clean_dest), 1_000);
    assert_eq!(env.market_state().1.vault, 0);
    assert_eq!(env.portfolio_state(victim).capital.get(), 0);
}

// security.md sweep — CloseResolved dest validation (#44): the resolved payout must reject a dest
// token account of the wrong mint or owned by a third party (verify_withdrawable_token_accounts
// applies here too). No payout to a mismatched/foreign account.
#[test]
fn v16_attack_close_resolved_dest_validation() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    env.resolve();
    let (_, g0) = env.market_state();
    let cr = |env: &mut V16CuEnv, dest: Pubkey, signer: &Keypair| -> Result<u64, String> {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            },
            vec![
                AccountMeta::new_readonly(signer.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(p, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[],
        )
    };
    // wrong-mint dest -> reject.
    let other_mint = Pubkey::new_unique();
    let bad_mint_dest = Pubkey::new_unique();
    env.svm
        .set_account(
            bad_mint_dest,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(other_mint, owner.pubkey(), 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    assert!(
        cr(&mut env, bad_mint_dest, &owner).is_err(),
        "CloseResolved to a wrong-mint dest must reject"
    );
    assert_eq!(
        env.token_amount(bad_mint_dest),
        0,
        "no payout to wrong-mint dest"
    );

    // third-party-owned dest -> reject.
    let other = Keypair::new();
    let foreign_dest = Pubkey::new_unique();
    env.svm
        .set_account(
            foreign_dest,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, other.pubkey(), 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    assert!(
        cr(&mut env, foreign_dest, &owner).is_err(),
        "CloseResolved to a third-party dest must reject"
    );
    assert_eq!(
        env.token_amount(foreign_dest),
        0,
        "no payout to foreign dest"
    );

    assert_eq!(
        env.market_state().1.vault,
        g0.vault,
        "vault unchanged by rejected payouts"
    );
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        1_000_000,
        "portfolio still owed its capital"
    );
    // correct dest works.
    let good = env.close_resolved(&owner, p);
    assert_eq!(
        env.token_amount(good),
        1_000_000,
        "correct-mint own dest receives the resolved payout"
    );
}

#[test]
fn v16_bpf_resolved_payout_tags_are_bounded_and_update_state() {
    let mut claim_env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = claim_env.create_portfolio(&owner);
    {
        let mut market_account = claim_env
            .svm
            .get_account(&claim_env.market)
            .expect("market account");
        let mut portfolio_account = claim_env
            .svm
            .get_account(&portfolio)
            .expect("portfolio account");
        let (cfg, mut group) = state::read_market(&market_account.data).unwrap();
        let mut account = state::read_portfolio(&portfolio_account.data).unwrap();
        group.mode = MarketModeV16::Resolved;
        group.resolved_slot = 1;
        group.current_slot = 1;
        group.vault = 60;
        group.payout_snapshot_captured = true;
        group.payout_snapshot = 100;
        group.resolved_payout_ledger = ResolvedPayoutLedgerV16 {
            snapshot_residual: 100,
            terminal_claim_exact_receipts_num: 100 * BOUND_SCALE,
            terminal_claim_bound_unreceipted_num: 0,
            current_payout_rate_num: 100 * BOUND_SCALE,
            current_payout_rate_den: 100 * BOUND_SCALE,
            snapshot_slot: 1,
            payout_halted: false,
            finalized: false,
        };
        account.resolved_payout_receipt =
            percolator::ResolvedPayoutReceiptV16Account::from_runtime(&ResolvedPayoutReceiptV16 {
                present: true,
                prior_bound_contribution_num: 100 * BOUND_SCALE,
                live_released_face_at_receipt: 0,
                terminal_positive_claim_face: 100,
                paid_effective: 40,
                finalized: false,
            });
        state::write_market(&mut market_account.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut portfolio_account.data, &account).unwrap();
        claim_env
            .svm
            .set_account(claim_env.market, market_account)
            .unwrap();
        claim_env
            .svm
            .set_account(portfolio, portfolio_account)
            .unwrap();
    }
    claim_env.set_token_account_amount(
        claim_env.vault,
        claim_env.mint,
        claim_env.vault_authority,
        60,
    );
    let dest = claim_env.token_account_for_mint(claim_env.mint, owner.pubkey(), 0);
    let claim_cu = claim_env.claim_resolved_payout_topup_with_cu(owner.pubkey(), portfolio, dest);
    assert_cu_within("ClaimResolvedPayoutTopup", claim_cu, CUSTODY_CU_LIMIT);
    assert_eq!(claim_env.token_amount(dest), 60);
    assert_eq!(claim_env.token_amount(claim_env.vault), 0);
    let (_, group) = claim_env.market_state();
    let account = claim_env.portfolio_state(portfolio);
    assert_eq!(group.vault, 0);
    assert_eq!(resolved_receipt(&account).paid_effective, 100);
    assert!(resolved_receipt(&account).finalized);
}
