//! INV-008/036/040/080: consumed reduction consent cannot commit a preceding clipped
//! maintenance charge or reward. Reuse the fully public attribution fixture; this
//! history does not add pending-loss or cross-slot cadence-invariance evidence.

use super::*;
use solana_sdk::{fee::FeeStructure, instruction::InstructionError, transaction::TransactionError};
use std::collections::BTreeMap;

#[test]
fn v16_program_fee_bumped_reduction_retry_restores_clipped_maintenance_and_reward() {
    const DEPOSITS: [u128; 5] = [3_000, 3_000, 10_000, 777, 777];
    const FEE_RATE: u128 = 1_000;
    const SLOT: u64 = 4;
    const SHARE: u16 = 3_333;
    const LIMIT: u32 = 600_000;
    const CHARGED: u128 = DEPOSITS[0];
    const REWARD: u128 = CHARGED * SHARE as u128 / 10_000;
    const RETAINED: u128 = CHARGED - REWARD;
    let mut peak = 0;

    for reverse in [false, true] {
        for recipient in [0, 2] {
            let mut world = AttributionWorld::new_with_deposits(
                reverse,
                V16CuMarketParams {
                    max_portfolio_assets: 3,
                    maintenance_fee_per_slot: FEE_RATE,
                    max_accrual_dt_slots: SLOT,
                    min_funding_lifetime_slots: SLOT,
                    max_price_move_bps_per_slot: 1_000,
                    ..V16CuMarketParams::default()
                },
                DEPOSITS,
            );
            world.env.update_maintenance_fee_policy_with_cu(SHARE);
            let portfolios: Vec<_> = world.actors.iter().map(|a| a.portfolio).collect();
            let direction = if reverse { -1 } else { 1 };
            world.env.trade_asset_with_cu(
                0,
                &world.actors[0].owner,
                portfolios[0],
                &world.actors[1].owner,
                portfolios[1],
                direction * 2 * POS_SCALE as i128,
                100,
                0,
            );
            let epoch = world.env.portfolio_position_epoch(portfolios[0]);
            let reduction = Instruction {
                program_id: world.env.program_id,
                accounts: vec![
                    AccountMeta::new(world.actors[0].owner.pubkey(), true),
                    AccountMeta::new(world.env.market, false),
                    AccountMeta::new(portfolios[0], false),
                ],
                data: ProgInstruction::RebalanceReduce {
                    portfolio_id: world.env.portfolio_id(portfolios[0]),
                    position_epoch: epoch,
                    asset_index: 0,
                    reduce_q: POS_SCALE,
                }
                .encode(),
            };
            let fee = Instruction {
                program_id: world.env.program_id,
                accounts: vec![
                    AccountMeta::new(world.env.market, false),
                    AccountMeta::new(portfolios[0], false),
                    AccountMeta::new(portfolios[recipient], false),
                ],
                data: ProgInstruction::SyncMaintenanceFee { now_slot: u64::MAX }.encode(),
            };
            let sign = |world: &AttributionWorld, ixs: &[Instruction], price: u64| {
                let mut instructions = vec![
                    heap_ix(),
                    ComputeBudgetInstruction::set_compute_unit_limit(LIMIT),
                    ComputeBudgetInstruction::set_compute_unit_price(price),
                ];
                instructions.extend_from_slice(ixs);
                let tx = Transaction::new_signed_with_payer(
                    &instructions,
                    Some(&world.env.payer.pubkey()),
                    &[&world.env.payer, &world.actors[0].owner],
                    world.env.svm.latest_blockhash(),
                );
                tx.verify().unwrap();
                assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
                tx
            };
            // Retain all economic bytes before the first partial consumes the epoch.
            let first = sign(&world, &[reduction.clone()], 1);
            let retry = sign(&world, &[reduction.clone()], 2);
            let bundle = sign(&world, &[fee.clone(), reduction.clone()], 3);
            let mut fee_bumped_message = first.message.clone();
            fee_bumped_message.instructions[2] = retry.message.instructions[2].clone();
            assert_eq!(fee_bumped_message, retry.message);
            assert_ne!(first.signatures, retry.signatures);
            let retained_wire = [
                bincode::serialize(&retry).unwrap(),
                bincode::serialize(&bundle).unwrap(),
            ];
            peak = peak.max(
                world
                    .env
                    .svm
                    .send_transaction(first)
                    .unwrap()
                    .compute_units_consumed,
            );
            assert_eq!(world.env.portfolio_position_epoch(portfolios[0]), epoch + 1);
            assert_eq!(
                active_leg_for_asset(&world.env.portfolio_state(portfolios[0]), 0).basis_pos_q,
                direction * POS_SCALE as i128,
            );

            world.env.svm.warp_to_slot(SLOT);
            peak = peak.max(world.env.crank(
                portfolios[3],
                ProgInstruction::PermissionlessCrank {
                    now_slot: SLOT,
                    observations: crank_observations(0),
                },
            ));
            assert_eq!(world.env.market_state().1.assets[0].slot_last, SLOT);
            // The separate keeper settles its own debt before earning a withdrawable reward.
            let keeper_fee = if recipient == 2 {
                peak = peak.max(
                    world
                        .env
                        .sync_maintenance_fee_with_cu(portfolios[2], None, SLOT),
                );
                FEE_RATE * u128::from(SLOT)
            } else {
                0
            };
            assert!(FEE_RATE * u128::from(SLOT) > CHARGED);
            assert_eq!(
                world.env.portfolio_state(portfolios[0]).capital.get(),
                CHARGED
            );
            assert_eq!(
                world.env.portfolio_state(portfolios[0]).last_fee_slot.get(),
                0
            );
            let stationary = world.frame();
            let frame = |world: &AttributionWorld, tx: &Transaction| -> BTreeMap<_, _> {
                world
                    .frame()
                    .into_iter()
                    .chain(
                        tx.message
                            .account_keys
                            .iter()
                            .copied()
                            .chain([solana_sdk::sysvar::clock::ID])
                            .map(|key| (key, world.env.svm.get_account(&key))),
                    )
                    .collect()
            };
            for (tx, price, index) in [(&retry, 2u64, 3u8), (&bundle, 3, 4)] {
                let mut expected = frame(&world, tx);
                let transaction_fee = FeeStructure::default().lamports_per_signature
                    * u64::from(tx.message.header.num_required_signatures)
                    + (u64::from(LIMIT) * price).div_ceil(1_000_000);
                expected
                    .get_mut(&world.env.payer.pubkey())
                    .unwrap()
                    .as_mut()
                    .unwrap()
                    .lamports -= transaction_fee;
                let error = world.env.svm.send_transaction(tx.clone()).unwrap_err();
                assert_eq!(
                    error.err,
                    TransactionError::InstructionError(
                        index,
                        InstructionError::Custom(PercolatorError::EngineProvenanceMismatch as u32),
                    )
                );
                assert_eq!(
                    error
                        .meta
                        .logs
                        .iter()
                        .filter(|log| **log == format!("Program {} success", world.env.program_id))
                        .count(),
                    usize::from(index == 4)
                );
                assert_eq!(
                    frame(&world, tx),
                    expected,
                    "reverse={reverse}, recipient={recipient}, index={index}"
                );
                peak = peak.max(error.meta.compute_units_consumed);
            }
            assert_eq!(
                [
                    bincode::serialize(&retry).unwrap(),
                    bincode::serialize(&bundle).unwrap()
                ],
                retained_wire
            );

            // Renew only the consumed position epoch. The identical fee prefix now commits.
            let fresh = Instruction {
                data: ProgInstruction::RebalanceReduce {
                    portfolio_id: world.env.portfolio_id(portfolios[0]),
                    position_epoch: epoch + 1,
                    asset_index: 0,
                    reduce_q: POS_SCALE,
                }
                .encode(),
                ..reduction
            };
            let current = sign(&world, &[fee, fresh], 4);
            peak = peak.max(
                world
                    .env
                    .svm
                    .send_transaction(current)
                    .unwrap()
                    .compute_units_consumed,
            );
            assert_eq!(world.env.portfolio_position_epoch(portfolios[0]), epoch + 2);
            assert!(!has_active_leg_for_asset(
                &world.env.portfolio_state(portfolios[0]),
                0
            ));
            assert_eq!(
                world.env.portfolio_state(portfolios[0]).last_fee_slot.get(),
                SLOT
            );
            let mut capital = DEPOSITS;
            capital[0] -= CHARGED;
            capital[2] -= keeper_fee;
            capital[recipient] += REWARD;
            for (actor, expected) in capital.iter().enumerate() {
                let account = world.env.portfolio_state(portfolios[actor]);
                assert_eq!(account.capital.get(), *expected);
                assert_eq!(account.pnl.get(), 0);
            }
            let group = world.env.market_state().1;
            assert_eq!(group.insurance, keeper_fee + RETAINED);
            assert_eq!(
                &group.insurance_domain_budget[..2],
                &[
                    keeper_fee / 2 + RETAINED / 2,
                    keeper_fee - keeper_fee / 2 + RETAINED - RETAINED / 2,
                ]
            );
            assert!(group.insurance_domain_budget[2..].iter().all(|x| *x == 0));
            assert!(group.insurance_domain_spent.iter().all(|x| *x == 0));
            assert_eq!(group.c_tot, capital.iter().sum::<u128>());
            assert_eq!(group.vault, DEPOSITS.iter().sum::<u128>());
            assert_domain_budget_remaining_total_consistent(
                &group,
                "clipped fee and fresh reduction",
            );
            for (key, before) in &stationary {
                if ![world.env.market, portfolios[0], portfolios[recipient]].contains(key) {
                    assert_eq!(world.env.svm.get_account(key), *before);
                }
            }

            let actor = &world.actors[recipient];
            let amount = capital[recipient];
            peak = peak.max(
                world
                    .env
                    .send(
                        world.env.withdraw_ix(actor.portfolio, amount),
                        vec![
                            AccountMeta::new(actor.owner.pubkey(), true),
                            AccountMeta::new(world.env.market, false),
                            AccountMeta::new(actor.portfolio, false),
                            AccountMeta::new(actor.token, false),
                            AccountMeta::new(world.env.vault, false),
                            AccountMeta::new_readonly(world.env.vault_authority, false),
                            AccountMeta::new_readonly(spl_token::ID, false),
                        ],
                        &[&actor.owner],
                    )
                    .expect("same-slot reward exit cannot recollect the clipped remainder"),
            );
            assert_eq!(u128::from(world.env.token_amount(actor.token)), amount);
            assert_eq!(world.env.portfolio_state(actor.portfolio).capital.get(), 0);
            let group = world.env.market_state().1;
            assert_eq!(group.insurance, keeper_fee + RETAINED);
            assert_eq!(group.c_tot, capital.iter().sum::<u128>() - amount);
            assert_eq!(group.vault, DEPOSITS.iter().sum::<u128>() - amount);
            assert_eq!(group.vault, group.c_tot + group.insurance);
            assert_eq!(
                u128::from(world.env.token_amount(world.env.vault)),
                group.vault
            );
        }
    }
    assert_cu_within(
        "clipped maintenance and fee-bumped reduction retry",
        peak,
        u64::from(LIMIT),
    );
    eprintln!("maintenance/reduction retry: 4 worlds, 8 exact rollbacks, 4 fresh reductions, 4 exact reward exits; peak={peak} CU");
}
