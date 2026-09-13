//! Row 417: replacing an SPL destination's lifetime must not replace its receipt.
//! After a first expiry/top-up, the ATA disappears through public SPL instructions.
//! The second expiry and every other claim settle before repair of the last receipt.

use super::{late_expiry::World, *};
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};

const FACES: [u128; 6] = [700, 0, 1_000, 0, 1_300, 0];
const RESIDUAL: [u128; 3] = [501, 501 + 161, 501 + 161 + 189];
const SUPPLY: u128 = 3_852;

fn entitlement(actor: usize, stage: usize) -> u128 {
    FACES[actor] * RESIDUAL[stage] / FACES.iter().sum::<u128>()
}

fn successes(logs: &[String], program: Pubkey) -> usize {
    logs.iter()
        .filter(|line| **line == format!("Program {program} success"))
        .count()
}

fn cashflows(world: &World, paid: &[u128; 6], spent: &[u128; 6]) {
    let mut wallets = 0;
    for actor in 0..6 {
        let account = world.env.svm.get_account(&world.actors[actor].token);
        let amount = match account {
            Some(account) if !account.data.is_empty() => {
                u128::from(TokenAccount::unpack(&account.data).unwrap().amount)
            }
            _ => 0,
        };
        let received = if actor == 1 { spent.iter().sum() } else { 0 };
        assert_eq!(amount, paid[actor] - spent[actor] + received);
        wallets += amount;
    }
    let vault = u128::from(world.env.token_amount(world.env.vault));
    assert_eq!(vault, SUPPLY - 1 - paid.iter().sum::<u128>());
    assert_eq!(world.env.market_state().1.vault, vault);
    assert_eq!(world.env.token_amount(world.provider_token), 1);
    assert_eq!(vault + wallets + 1, SUPPLY);
    assert_eq!(
        u128::from(
            Mint::unpack(&world.env.svm.get_account(&world.env.mint).unwrap().data)
                .unwrap()
                .supply
        ),
        SUPPLY
    );
    assert!(
        !world.receipt(1).present,
        "the spend recipient acquires no claim"
    );
}

fn pay(world: &mut World, actor: usize, ix: &Instruction, paid: &mut [u128; 6], due: u128) {
    let before = world.frame();
    let token = world.actors[actor].token;
    let balance = world.env.token_amount(token);
    let vault = world.env.token_amount(world.env.vault);
    let meta = world.land(&[ix.clone()], false).unwrap();
    assert_cu_within(
        "receipt destination retry",
        meta.compute_units_consumed,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(successes(&meta.logs, spl_token::ID), usize::from(due != 0));
    assert_eq!(u128::from(world.env.token_amount(token) - balance), due);
    assert_eq!(
        u128::from(vault - world.env.token_amount(world.env.vault)),
        due
    );
    paid[actor] += due;
    world.assert_frame_except(
        &before,
        &[
            world.env.market,
            world.env.vault,
            world.actors[actor].portfolio,
            token,
        ],
    );
}

#[test]
fn v16_program_recreated_destination_preserves_receipt_identity_across_second_expiry_retry() {
    assert_eq!(
        [entitlement(0, 0), entitlement(0, 1), entitlement(0, 2)],
        [116, 154, 198]
    );
    assert_eq!(
        [entitlement(4, 0), entitlement(4, 1), entitlement(4, 2)],
        [217, 286, 368]
    );
    let mut peak_cu = 0;
    for missing in [0, 4] {
        for claim_route in [true, false] {
            let peer = 4 - missing;
            let mut world = World::before_receipts_with_staggered_sources();
            let mut paid = [0; 6];
            let mut spent = [0; 6];
            for actor in [0, 4] {
                for _ in 0..8 {
                    if world.receipt(actor).present {
                        break;
                    }
                    world.land(&[world.payout(actor, false)], false).unwrap();
                }
                paid[actor] = 1_000 + entitlement(actor, 0);
            }
            let original = [0, 4].map(|actor| world.receipt(actor));
            let identities = [0, 4].map(|actor| {
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
            let check = |world: &World, paid: &[u128; 6], stage: usize| {
                for (index, actor) in [0, 4].into_iter().enumerate() {
                    let mut receipt = original[index];
                    assert!(receipt.present && !receipt.finalized);
                    assert_eq!(receipt.terminal_positive_claim_face, FACES[actor]);
                    assert_eq!(
                        receipt.prior_bound_contribution_num,
                        FACES[actor] * BOUND_SCALE
                    );
                    assert_eq!(receipt.live_released_face_at_receipt, 0);
                    receipt.paid_effective = paid[actor] - 1_000;
                    assert_eq!(world.receipt(actor), receipt);
                    let portfolio = world.actors[actor].portfolio;
                    assert_eq!(
                        (
                            world.env.portfolio_id(portfolio),
                            world.env.portfolio_position_epoch(portfolio),
                            state::read_portfolio_owner_preflight(
                                &world.env.svm.get_account(&portfolio).unwrap().data
                            )
                            .unwrap()
                        ),
                        identities[index]
                    );
                }
                let group = world.env.market_state().1;
                let ledger = group.resolved_payout_ledger;
                assert_eq!(ledger.snapshot_slot, 12);
                assert_eq!(ledger.snapshot_residual, RESIDUAL[stage]);
                assert_eq!(
                    ledger.current_payout_rate_num,
                    RESIDUAL[stage] * BOUND_SCALE
                );
                assert_eq!(ledger.current_payout_rate_den, 3_000 * BOUND_SCALE);
                assert_eq!(
                    ledger.terminal_claim_exact_receipts_num,
                    2_000 * BOUND_SCALE
                );
                assert_eq!(
                    ledger.terminal_claim_bound_unreceipted_num,
                    1_000 * BOUND_SCALE
                );
                assert!(!ledger.payout_halted && !ledger.finalized);
                for (index, (domain, reserve, expiry)) in
                    [(3, 161, 13), (5, 189, 15)].into_iter().enumerate()
                {
                    let fresh = stage <= index;
                    let bucket = group.source_backing_buckets[domain];
                    assert_eq!(bucket.expiry_slot, expiry);
                    assert_eq!(
                        bucket.status,
                        if fresh {
                            BackingBucketStatusV16::Fresh
                        } else {
                            BackingBucketStatusV16::Expired
                        }
                    );
                    assert_eq!(bucket.consumed_liened_backing_num, 0);
                    assert_eq!(
                        group.source_credit[domain].fresh_reserved_backing_num,
                        if fresh { reserve * BOUND_SCALE } else { 0 }
                    );
                }
            };
            let retained_missing = world.payout(missing, claim_route);
            let retained_peer = world.payout(peer, !claim_route);
            let source = world.payout(2, false);
            check(&world, &paid, 0);
            cashflows(&world, &paid, &spent);

            world.env.svm.warp_to_slot(13);
            pay(&mut world, 2, &source, &mut paid, 0);
            for (actor, ix) in [(missing, &retained_missing), (peer, &retained_peer)] {
                pay(
                    &mut world,
                    actor,
                    ix,
                    &mut paid,
                    entitlement(actor, 1) - entitlement(actor, 0),
                );
                check(&world, &paid, 1);
                cashflows(&world, &paid, &spent);
            }

            // The claimant spends both capital and cumulative junior payments, then
            // destroys the ATA through SPL. Its embedded receipt must survive intact.
            let token = world.actors[missing].token;
            let owner = world.actors[missing].owner.pubkey();
            let before = world.frame();
            let owner_lamports = world.env.svm.get_account(&owner).unwrap().lamports;
            let rent = world.env.svm.get_account(&token).unwrap().lamports;
            for ix in [
                spl_token::instruction::transfer(
                    &spl_token::ID,
                    &token,
                    &world.actors[1].token,
                    &owner,
                    &[],
                    paid[missing] as u64,
                )
                .unwrap(),
                spl_token::instruction::close_account(&spl_token::ID, &token, &owner, &owner, &[])
                    .unwrap(),
            ] {
                world.env.svm.expire_blockhash();
                let cu = send_raw_tx(
                    &mut world.env.svm,
                    &world.env.payer,
                    ix,
                    &[&world.actors[missing].owner],
                )
                .unwrap();
                assert_cu_within("receipt destination spend/close", cu, CUSTODY_CU_LIMIT);
                world.peak_cu = world.peak_cu.max(cu);
            }
            spent[missing] = paid[missing];
            assert!(world
                .env
                .svm
                .get_account(&token)
                .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
            assert_eq!(
                world.env.svm.get_account(&owner).unwrap().lamports,
                owner_lamports + rent
            );
            world.assert_frame_except(&before, &[token, world.actors[1].token, owner]);
            check(&world, &paid, 1);
            cashflows(&world, &paid, &spent);

            // Second expiry and the other claimant's SPL payout succeed before the
            // absent destination rejects. The previously committed first wave survives.
            world.env.svm.warp_to_slot(15);
            let before = world.frame();
            let mut payer = world
                .env
                .svm
                .get_account(&world.env.payer.pubkey())
                .unwrap();
            payer.lamports -= FeeStructure::default().lamports_per_signature;
            let failure = world
                .land(
                    &[
                        source.clone(),
                        retained_peer.clone(),
                        retained_missing.clone(),
                    ],
                    false,
                )
                .expect_err("closed destination after paying peer");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(
                    4,
                    InstructionError::Custom(PercolatorError::InvalidTokenAccount as u32)
                )
            );
            assert_eq!(successes(&failure.meta.logs, world.env.program_id), 2);
            assert_eq!(successes(&failure.meta.logs, spl_token::ID), 1);
            assert_eq!(
                world.frame(),
                before,
                "exact stock, receipt, token, and rent rollback"
            );
            assert_eq!(
                world.env.svm.get_account(&world.env.payer.pubkey()),
                Some(payer)
            );
            check(&world, &paid, 1);
            cashflows(&world, &paid, &spent);

            // Leave the ATA absent while the peer overtakes this claimant and the
            // source replaces the final unreceipted bound. Only this receipt stays live.
            let pending_receipt = world.receipt(missing);
            pay(&mut world, 2, &source, &mut paid, 0);
            check(&world, &paid, 2);
            pay(
                &mut world,
                peer,
                &retained_peer,
                &mut paid,
                entitlement(peer, 2) - entitlement(peer, 1),
            );
            check(&world, &paid, 2);
            pay(&mut world, 2, &source, &mut paid, 0);
            pay(&mut world, 2, &source, &mut paid, 1_000 + entitlement(2, 2));
            pay(&mut world, peer, &retained_peer, &mut paid, 0);
            cashflows(&world, &paid, &spent);
            for actor in (0..6).filter(|actor| *actor != missing) {
                assert!(resolved_portfolio_is_terminal(
                    &world.env,
                    world.actors[actor].portfolio
                ));
                assert!(!world.receipt(actor).present);
            }
            assert_eq!(world.receipt(missing), pending_receipt);
            let ledger = world.env.market_state().1.resolved_payout_ledger;
            assert_eq!(ledger.terminal_claim_bound_unreceipted_num, 0);
            assert_eq!(
                ledger.terminal_claim_exact_receipts_num,
                3_000 * BOUND_SCALE
            );
            assert_eq!(ledger.current_payout_rate_num, RESIDUAL[2] * BOUND_SCALE);
            assert_eq!(ledger.current_payout_rate_den, 3_000 * BOUND_SCALE);
            assert!(!ledger.payout_halted && !ledger.finalized);
            let due = entitlement(missing, 2) - entitlement(missing, 1);
            assert_eq!(world.env.market_state().1.vault, due + 2);

            let before = world.frame();
            world.env.svm.expire_blockhash();
            assert_eq!(
                create_ata_for_test(&mut world.env.svm, &world.env.payer, owner, world.env.mint),
                token
            );
            assert_eq!(world.env.token_amount(token), 0);
            assert_eq!(world.env.svm.get_account(&token).unwrap().lamports, rent);
            world.assert_frame_except(&before, &[token]);
            cashflows(&world, &paid, &spent);

            // Reuse the pre-expiry instruction: the fresh ATA receives only the
            // unpaid second-wave difference, not capital or prior junior payments.
            pay(&mut world, missing, &retained_missing, &mut paid, due);
            if claim_route {
                let mut completed_receipt = pending_receipt;
                completed_receipt.paid_effective = entitlement(missing, 2);
                assert_eq!(world.receipt(missing), completed_receipt);
                pay(&mut world, missing, &retained_missing, &mut paid, 0);
            }
            assert_eq!(u128::from(world.env.token_amount(token)), due);
            assert_eq!(world.env.market_state().1.resolved_payout_ledger, ledger);
            let portfolio = world.actors[missing].portfolio;
            assert_eq!(
                (
                    world.env.portfolio_id(portfolio),
                    world.env.portfolio_position_epoch(portfolio),
                    state::read_portfolio_owner_preflight(
                        &world.env.svm.get_account(&portfolio).unwrap().data
                    )
                    .unwrap()
                ),
                identities[missing / 4]
            );
            assert!(!world.receipt(missing).present);
            let before = world.frame();
            let retry = world.land(&[retained_missing], false);
            let meta = if claim_route {
                retry.unwrap()
            } else {
                let failure = retry.expect_err("retired CloseResolved has no further progress");
                assert_eq!(
                    failure.err,
                    TransactionError::InstructionError(
                        2,
                        InstructionError::Custom(PercolatorError::EngineNonProgress as u32)
                    )
                );
                failure.meta
            };
            assert_eq!(successes(&meta.logs, spl_token::ID), 0);
            assert_eq!(
                world.frame(),
                before,
                "a new ATA lifetime cannot replenish a receipt"
            );
            assert_eq!(paid, [1_198, 0, 1_283, 0, 1_368, 0]);
            cashflows(&world, &paid, &spent);
            world.custody();
            assert_eq!(world.env.token_amount(world.env.vault), 2);
            for actor in 0..6 {
                assert!(resolved_portfolio_is_terminal(
                    &world.env,
                    world.actors[actor].portfolio
                ));
            }
            assert_cu_within(
                "receipt destination recreation history",
                world.peak_cu,
                600_000,
            );
            peak_cu = peak_cu.max(world.peak_cu);
        }
    }
    println!("INV-067 destination recreation: 4 worlds, 4 paying-prefix rollbacks, 4 same-address ATA repairs, 8 second-wave top-ups; peak {peak_cu} CU");
}
