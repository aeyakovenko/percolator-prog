//! INV-066/067/068: an already-topped-up receipt survives missing custody while
//! two fresh source conversions refine the denominator. No late expiry occurs.
//! Swapping the delayed claimant must preserve the same cumulative entitlements.

use super::{late_expiry::World, *};
use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

const FACES: [u128; 6] = [700, 0, 1_000, 0, 1_300, 0];
const RESIDUAL: u128 = 501;
const CONVERSIONS: [u128; 2] = [61 + 100, 39 + 150];

fn entitlement(actor: usize, converted: u128) -> u128 {
    let face = FACES[actor] - if actor == 2 { converted } else { 0 };
    face * RESIDUAL / (3_000 - converted)
}

fn balance(world: &World, token: Pubkey) -> u128 {
    match world.env.svm.get_account(&token) {
        Some(account) if !account.data.is_empty() => {
            u128::from(TokenAccount::unpack(&account.data).unwrap().amount)
        }
        _ => 0,
    }
}

fn custody(world: &World) {
    let vault = balance(world, world.env.vault);
    assert_eq!(world.env.market_state().1.vault, vault);
    assert_eq!(balance(world, world.provider_token), 1);
    assert_eq!(
        vault
            + 1
            + world
                .actors
                .iter()
                .map(|a| balance(world, a.token))
                .sum::<u128>(),
        3_852
    );
    assert_eq!(
        Mint::unpack(&world.env.svm.get_account(&world.env.mint).unwrap().data)
            .unwrap()
            .supply,
        3_852
    );
}

fn pay(world: &mut World, actor: usize, ix: &Instruction) -> u128 {
    let before = world.frame();
    let token = world.actors[actor].token;
    let tokens = balance(world, token);
    let vault = balance(world, world.env.vault);
    let meta = world
        .land(&[ix.clone()], false)
        .expect("public payout progress");
    assert_cu_within(
        "fresh conversion receipt payout",
        meta.compute_units_consumed,
        CUSTODY_CU_LIMIT,
    );
    let paid = balance(world, token).checked_sub(tokens).unwrap();
    assert_eq!(
        vault.checked_sub(balance(world, world.env.vault)),
        Some(paid)
    );
    world.assert_frame_except(
        &before,
        &[
            world.env.market,
            world.env.vault,
            world.actors[actor].portfolio,
            token,
        ],
    );
    custody(world);
    paid
}

#[test]
fn v16_program_paid_receipt_survives_missing_custody_across_two_fresh_conversions() {
    let converted: u128 = CONVERSIONS.iter().sum();
    let expected = [
        1_000 + entitlement(0, converted),
        0,
        1_000 + converted + entitlement(2, converted),
        0,
        1_000 + entitlement(4, converted),
        0,
    ];
    assert_eq!(expected, [1_132, 0, 1_472, 0, 1_245, 0]);
    assert_eq!(expected.iter().sum::<u128>() + 2 + 1, 3_852);
    let mut baseline = None;
    let mut peak_cu = 0;
    for missing in [0, 4] {
        let peer = 4 - missing;
        let mut world = World::before_receipts_with_staggered_sources();
        for actor in [missing, peer] {
            let ix = world.payout(actor, false);
            for _ in 0..8 {
                if world.receipt(actor).present {
                    break;
                }
                pay(&mut world, actor, &ix);
            }
            let receipt = world.receipt(actor);
            assert!(receipt.present && !receipt.finalized);
            assert_eq!(receipt.terminal_positive_claim_face, FACES[actor]);
            assert_eq!(
                receipt.prior_bound_contribution_num,
                FACES[actor] * BOUND_SCALE
            );
            assert_eq!(receipt.live_released_face_at_receipt, 0);
            assert_eq!(receipt.paid_effective, entitlement(actor, 0));
        }
        let originals = [world.receipt(0), world.receipt(4)];
        let retained = [world.payout(0, true), world.payout(4, true)];
        let identities = [0, 4].map(|actor| {
            let key = world.actors[actor].portfolio;
            (
                world.env.portfolio_id(key),
                world.env.portfolio_position_epoch(key),
                state::read_portfolio_owner_preflight(
                    &world.env.svm.get_account(&key).unwrap().data,
                )
                .unwrap(),
            )
        });
        let source = world.payout(2, false);
        assert_eq!(pay(&mut world, 2, &source), 0);
        let first = world.env.market_state().1.resolved_payout_ledger;
        assert_eq!(first.snapshot_slot, 12);
        assert_eq!(first.snapshot_residual, RESIDUAL);
        assert_eq!(
            first.current_payout_rate_den,
            (3_000 - CONVERSIONS[0]) * BOUND_SCALE
        );
        assert_eq!(first.terminal_claim_exact_receipts_num, 2_000 * BOUND_SCALE);
        assert_eq!(
            first.terminal_claim_bound_unreceipted_num,
            (1_000 - CONVERSIONS[0]) * BOUND_SCALE
        );
        for actor in [missing, peer] {
            let due = entitlement(actor, CONVERSIONS[0]) - entitlement(actor, 0);
            assert!(due > 0);
            assert_eq!(pay(&mut world, actor, &retained[actor / 4]), due);
            let mut receipt = originals[actor / 4];
            receipt.paid_effective = entitlement(actor, CONVERSIONS[0]);
            assert_eq!(world.receipt(actor), receipt);
            assert_eq!(world.env.market_state().1.resolved_payout_ledger, first);
        }

        // Spend the first payments, then end the ATA's lifetime using owner-signed SPL.
        let token = world.actors[missing].token;
        let owner = world.actors[missing].owner.pubkey();
        let spent = balance(&world, token);
        assert_eq!(spent, 1_000 + entitlement(missing, CONVERSIONS[0]));
        let before = world.frame();
        for ix in [
            spl_token::instruction::transfer(
                &spl_token::ID,
                &token,
                &world.actors[1].token,
                &owner,
                &[],
                spent as u64,
            )
            .unwrap(),
            spl_token::instruction::close_account(&spl_token::ID, &token, &owner, &owner, &[])
                .unwrap(),
        ] {
            send_raw_tx(
                &mut world.env.svm,
                &world.env.payer,
                ix,
                &[&world.actors[missing].owner],
            )
            .unwrap();
        }
        assert!(world.env.svm.get_account(&token).map_or(true, |account| {
            account.lamports == 0 && account.data.is_empty()
        }));
        world.assert_frame_except(&before, &[token, owner, world.actors[1].token]);
        let pending = world.receipt(missing);

        // Stay at slot 12: both reserves are consumed, never released by expiry.
        for _ in 0..8 {
            if resolved_portfolio_is_terminal(&world.env, world.actors[2].portfolio) {
                break;
            }
            pay(&mut world, 2, &source);
            assert_eq!(world.receipt(missing), pending);
        }
        assert!(resolved_portfolio_is_terminal(
            &world.env,
            world.actors[2].portfolio
        ));
        assert_eq!(balance(&world, world.actors[2].token), expected[2]);
        let group = world.env.market_state().1;
        for (domain, amount) in [(3, CONVERSIONS[0]), (5, CONVERSIONS[1])] {
            assert_eq!(
                group.source_backing_buckets[domain].consumed_liened_backing_num,
                amount * BOUND_SCALE
            );
            assert_eq!(
                group.source_credit[domain].provider_receivable_num,
                amount * BOUND_SCALE
            );
            assert_eq!(group.source_credit[domain].fresh_reserved_backing_num, 0);
        }
        let ledger = group.resolved_payout_ledger;
        assert_eq!(ledger.snapshot_slot, 12);
        assert_eq!(ledger.snapshot_residual, RESIDUAL);
        assert_eq!(ledger.current_payout_rate_num, RESIDUAL * BOUND_SCALE);
        assert_eq!(
            ledger.current_payout_rate_den,
            (3_000 - converted) * BOUND_SCALE
        );
        assert_eq!(
            ledger.terminal_claim_exact_receipts_num,
            (3_000 - converted) * BOUND_SCALE
        );
        assert_eq!(ledger.terminal_claim_bound_unreceipted_num, 0);
        assert!(!ledger.payout_halted && !ledger.finalized);

        // The peer really pays before the absent destination rejects the atomic tail.
        let before = world.frame();
        let failure = world
            .land(
                &[retained[peer / 4].clone(), retained[missing / 4].clone()],
                false,
            )
            .expect_err("missing destination after successful peer top-up");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(
                3,
                InstructionError::Custom(PercolatorError::InvalidTokenAccount as u32)
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
                1
            );
        }
        assert_cu_within(
            "fresh conversion missing-tail rollback",
            failure.meta.compute_units_consumed,
            600_000,
        );
        assert_eq!(world.frame(), before);

        let due = entitlement(peer, converted) - entitlement(peer, CONVERSIONS[0]);
        assert!(due > 0);
        assert_eq!(pay(&mut world, peer, &retained[peer / 4]), due);
        let mut peer_receipt = originals[peer / 4];
        peer_receipt.paid_effective = entitlement(peer, converted);
        assert_eq!(world.receipt(peer), peer_receipt);
        assert_eq!(pay(&mut world, peer, &retained[peer / 4]), 0);
        assert!(!world.receipt(peer).present);
        assert_eq!(world.receipt(missing), pending);
        let due = entitlement(missing, converted) - entitlement(missing, CONVERSIONS[0]);
        assert!(due > 0);
        assert_eq!(world.env.market_state().1.vault, due + 2);

        let before = world.frame();
        world.env.svm.expire_blockhash();
        assert_eq!(
            create_ata_for_test(&mut world.env.svm, &world.env.payer, owner, world.env.mint),
            token
        );
        world.assert_frame_except(&before, &[token]);
        assert_eq!(balance(&world, token), 0);
        assert_eq!(world.receipt(missing), pending);
        assert_eq!(pay(&mut world, missing, &retained[missing / 4]), due);
        let mut receipt = pending;
        receipt.paid_effective = entitlement(missing, converted);
        assert_eq!(world.receipt(missing), receipt);
        assert_eq!(
            balance(&world, token),
            due,
            "new custody receives only the unpaid delta"
        );
        assert_eq!(pay(&mut world, missing, &retained[missing / 4]), 0);
        assert!(!world.receipt(missing).present);
        let before = world.frame();
        let source_retry = world.payout(2, true);
        world
            .land(
                &[retained[0].clone(), retained[1].clone(), source_retry],
                false,
            )
            .unwrap();
        assert_eq!(
            world.frame(),
            before,
            "retired receipts cannot recreate paid entitlement"
        );
        assert_eq!(world.env.market_state().1.resolved_payout_ledger, ledger);
        let mut cumulative: [u128; 6] =
            std::array::from_fn(|actor| balance(&world, world.actors[actor].token));
        cumulative[missing] += spent;
        cumulative[1] -= spent;
        assert_eq!(cumulative, expected);
        for (index, actor) in [0, 4].into_iter().enumerate() {
            let key = world.actors[actor].portfolio;
            assert_eq!(
                (
                    world.env.portfolio_id(key),
                    world.env.portfolio_position_epoch(key),
                    state::read_portfolio_owner_preflight(
                        &world.env.svm.get_account(&key).unwrap().data
                    )
                    .unwrap()
                ),
                identities[index]
            );
        }
        assert!(world
            .actors
            .iter()
            .all(|a| resolved_portfolio_is_terminal(&world.env, a.portfolio)));
        assert_eq!(world.env.market_state().1.vault, 2);
        custody(&world);
        let endpoint = (cumulative, ledger, world.env.market_state().1.vault);
        if let Some(expected) = &baseline {
            assert_eq!(&endpoint, expected);
        } else {
            baseline = Some(endpoint);
        }
        peak_cu = peak_cu.max(world.peak_cu);
        println!("INV-066/067/068 fresh conversions: missing={missing}, payouts={cumulative:?}, peak_cu={}", world.peak_cu);
    }
    assert_cu_within(
        "fresh conversion receipt custody schedules",
        peak_cu,
        600_000,
    );
}
