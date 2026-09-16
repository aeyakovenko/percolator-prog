//! INV-067 / row 417: a maintenance-policy change credits new senior capital to
//! an already-paid junior receipt holder across late backing expiry. The fixed
//! fee and deferred-fee owners have no rewards; insurance succession changes the
//! reserve beneficiary, not capital inside a portfolio with a live receipt.
//! The reward must neither replace the receipt nor count as junior value paid.
//! Four fixed SPL histories add evidence only; row 417 remains OPEN.

use super::*;

fn policy(world: &World, share: u16, sequence: u64) -> Instruction {
    Instruction {
        program_id: world.env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(world.env.admin.pubkey(), true),
            AccountMeta::new(world.env.market, false),
        ],
        data: ProgInstruction::UpdateMaintenanceFeePolicy {
            cranker_share_bps: share,
            policy_sequence: sequence,
            authority_epoch: world.env.control_sequences(0).authority_epoch,
        }
        .encode(),
    }
}

fn change_policy(world: &mut World, instruction: &Instruction, share: u16) {
    let before = world.frame();
    let (mut cfg, group) = world.env.market_state();
    let mut sequences = world.env.control_sequences(0);
    world.land(&[instruction.clone()], true).unwrap();
    cfg.maintenance_cranker_fee_share_bps = share;
    sequences.maintenance_fee += 1;
    assert_eq!(world.env.market_state(), (cfg, group));
    assert_eq!(world.env.control_sequences(0), sequences);
    world.assert_frame_except(&before, &[world.env.market]);
}

#[test]
fn v16_program_policy_reward_keeps_paid_receipt_identity_across_late_expiry() {
    let mut peak_cu = 0;
    for recipient in [0, 4] {
        for fee_first in [false, true] {
            // System/SPL/wrapper genesis, real trading claims and partial payouts.
            let mut world = World::before_receipts_with_maintenance_fee(RATE);
            let market = world.env.market;
            for actor in [0, 4] {
                for _ in 0..8 {
                    if world.receipt(actor).present {
                        break;
                    }
                    world.land(&[world.payout(actor, false)], false).unwrap();
                }
            }
            let mut paid = [
                PRINCIPAL + junior(0, false),
                0,
                0,
                0,
                PRINCIPAL + junior(4, false),
            ];
            let original = [world.receipt(0), world.receipt(4)];
            check(&world, &original, &paid, false, false, false);
            for (index, actor) in [0, 4].into_iter().enumerate() {
                assert!(original[index].present && !original[index].finalized);
                assert_eq!(original[index].terminal_positive_claim_face, FACES[actor]);
                assert_eq!(original[index].paid_effective, junior(actor, false));
                let account = world.env.portfolio_state(world.actors[actor].portfolio);
                assert_eq!(account.capital.get(), 0);
            }
            let identities = [0, 2, 4].map(|actor| {
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
            // Retain public requests before the policy epoch and stock change.
            let claims = [world.payout(0, true), world.payout(4, true)];
            let normalize = world.payout(2, false);
            let fee = Instruction {
                program_id: world.env.program_id,
                accounts: vec![
                    AccountMeta::new(world.env.market, false),
                    AccountMeta::new(world.actors[2].portfolio, false),
                    AccountMeta::new(world.actors[recipient].portfolio, false),
                ],
                data: ProgInstruction::SyncMaintenanceFee { now_slot: 12 }.encode(),
            };
            assert_eq!(
                world.env.market_state().0.maintenance_cranker_fee_share_bps,
                0
            );
            let high = policy(
                &world,
                10_000,
                world.env.control_sequences(0).maintenance_fee + 1,
            );
            change_policy(&mut world, &high, 10_000);
            assert_eq!([world.receipt(0), world.receipt(4)], original);
            world.env.svm.warp_to_slot(17);

            let index = usize::from(recipient == 4);
            let mut prefix = if fee_first {
                vec![fee.clone(), normalize.clone()]
            } else {
                vec![normalize.clone(), fee.clone()]
            };
            prefix.push(claims[index].clone());
            let mut rejected = prefix.clone();
            rejected.push(high.clone());
            let before = world.frame();
            let failure = world
                .land(&rejected, true)
                .expect_err("consumed policy sequence");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(
                    5,
                    InstructionError::Custom(PercolatorError::EngineStale as u32),
                )
            );
            assert_eq!(successes(&failure.meta, world.env.program_id), 3);
            assert_eq!(successes(&failure.meta, spl_token::ID), 1);
            assert_eq!(
                world.frame(),
                before,
                "fee reward, expiry and paid receipt roll back together"
            );

            // Only the unrelated payer signs the successful economic continuation.
            let meta = world.land(&prefix, false).unwrap();
            assert_eq!(successes(&meta, spl_token::ID), 1);
            let due = junior(recipient, true) - junior(recipient, false);
            assert!(due > RATE);
            paid[recipient] += due;
            world.assert_frame_except(
                &before,
                &[
                    world.env.market,
                    world.env.vault,
                    world.actors[2].portfolio,
                    world.actors[recipient].portfolio,
                    world.actors[recipient].token,
                ],
            );
            let group = world.env.market_state().1;
            let ledger = group.resolved_payout_ledger;
            assert_eq!(ledger.snapshot_slot, 12);
            assert_eq!(ledger.snapshot_residual, FINAL_RESIDUAL);
            assert_eq!(ledger.current_payout_rate_num, FINAL_RESIDUAL * BOUND_SCALE);
            assert_eq!(ledger.current_payout_rate_den, TOTAL_FACE * BOUND_SCALE);
            assert_eq!(
                ledger.terminal_claim_bound_unreceipted_num,
                FACES[2] * BOUND_SCALE
            );
            assert_eq!(
                ledger.terminal_claim_exact_receipts_num,
                2_000 * BOUND_SCALE
            );
            assert_eq!(group.source_credit[3].fresh_reserved_backing_num, 0);
            assert_eq!(
                group.source_backing_buckets[3].status,
                BackingBucketStatusV16::Expired
            );
            assert_eq!(group.c_tot, PRINCIPAL + RATE);
            assert_eq!(group.insurance, INSURANCE - RATE);
            assert_eq!(
                group.insurance_domain_budget.iter().sum::<u128>(),
                INSURANCE - RATE
            );
            assert!(group.insurance_domain_spent.iter().all(|&value| value == 0));
            let source = world.env.portfolio_state(world.actors[2].portfolio);
            let rewarded = world.env.portfolio_state(world.actors[recipient].portfolio);
            assert_eq!(source.capital.get(), PRINCIPAL);
            assert_eq!(source.last_fee_slot.get(), 12);
            assert_eq!(rewarded.capital.get(), RATE);
            for (i, actor) in [0, 4].into_iter().enumerate() {
                let mut receipt = original[i];
                receipt.paid_effective = junior(actor, actor == recipient);
                assert_eq!(world.receipt(actor), receipt);
                assert_eq!(
                    world.env.token_amount(world.actors[actor].token) as u128,
                    paid[actor]
                );
            }
            world.custody();

            // An ABA policy value cannot re-earn the capped fee or erase new capital.
            let low = policy(
                &world,
                0,
                world.env.control_sequences(0).maintenance_fee + 1,
            );
            change_policy(&mut world, &low, 0);
            let before = world.frame();
            world
                .land(&[fee.clone(), claims[index].clone()], false)
                .unwrap();
            assert_eq!(
                world.frame(),
                before,
                "neither reward nor receipt due replays"
            );

            let peer = if recipient == 0 { 4 } else { 0 };
            pay(
                &mut world,
                &claims[1 - index],
                peer,
                junior(peer, true) - junior(peer, false),
                &mut paid,
            );
            let receipt = world.receipt(recipient);
            let reward_exit = world.payout(recipient, false);
            pay(&mut world, &reward_exit, recipient, RATE, &mut paid);
            assert_eq!(
                world.receipt(recipient),
                receipt,
                "senior reward does not advance junior paid history"
            );
            let rewarded = world.env.portfolio_state(world.actors[recipient].portfolio);
            assert_eq!(rewarded.capital.get(), 0);
            assert_eq!(world.env.market_state().1.c_tot, PRINCIPAL);

            // Materialize the remaining bound and retire every receipt publicly.
            let source_exit = world.payout(2, false);
            pay(
                &mut world,
                &source_exit,
                2,
                PRINCIPAL + junior(2, true),
                &mut paid,
            );
            let mut expected = [1_104, 0, 1_185, 0, 1_266];
            expected[recipient] += RATE;
            assert_eq!(paid, expected);
            for (i, actor) in [0, 2, 4].into_iter().enumerate() {
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
                    identities[i]
                );
                let clear = world.payout(actor, true);
                pay(&mut world, &clear, actor, 0, &mut paid);
                assert!(!world.receipt(actor).present);
                assert!(resolved_portfolio_is_terminal(&world.env, portfolio));
            }
            let before = world.frame();
            world
                .land(&[fee, claims[0].clone(), claims[1].clone()], false)
                .unwrap();
            assert_eq!(world.frame(), before);
            let group = world.env.market_state().1;
            assert_eq!(group.c_tot, 0);
            assert_eq!(group.insurance, INSURANCE - RATE);
            assert_eq!(group.vault, INSURANCE - RATE + 2);
            let ledger = group.resolved_payout_ledger;
            assert_eq!(ledger.terminal_claim_bound_unreceipted_num, 0);
            assert_eq!(
                ledger.terminal_claim_exact_receipts_num,
                TOTAL_FACE * BOUND_SCALE
            );
            for actor in [0, 1, 2, 3, 4] {
                let portfolio = world.actors[actor].portfolio;
                let before = world.frame();
                let mut closed = world.env.svm.get_account(&portfolio).unwrap();
                let rent = closed.lamports;
                let market_lamports = world.env.svm.get_account(&market).unwrap().lamports;
                let cu = world
                    .env
                    .close_portfolio_with_cu(&world.actors[actor].owner, portfolio);
                world.peak_cu = world.peak_cu.max(cu);
                closed.lamports = 0;
                closed.data.clear();
                assert_eq!(world.env.svm.get_account(&portfolio), Some(closed));
                assert_eq!(
                    world.env.svm.get_account(&market).unwrap().lamports,
                    market_lamports + rent
                );
                world.assert_frame_except(&before, &[market, portfolio]);
            }
            assert_eq!(world.env.market_state().1.materialized_portfolio_count, 0);
            world.custody();
            assert_cu_within(
                "receipt policy reward and late expiry",
                world.peak_cu,
                CU_LIMIT,
            );
            peak_cu = peak_cu.max(world.peak_cu);
        }
    }
    println!("INV-067 policy reward: 4 histories, 4 paid-prefix rollbacks, 20 public portfolio closes; peak {peak_cu} CU");
}
