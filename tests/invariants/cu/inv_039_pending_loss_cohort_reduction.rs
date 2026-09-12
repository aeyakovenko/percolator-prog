//! INV-039: reducing exposure does not reduce an original cohort's unpaid loss share.
//!
//! Two equal-weight holders share one asset/side. A bankruptcy creates 20,000 atoms
//! of residual; a quarter reduction and then flattening must retain the second
//! holder's full weight. After booking, the first holder pays exactly 10,000 before
//! transferring principal, but cannot yet close with a remaining claim. The untouched
//! second holder still pays its own 10,000, not the first holder's share. Full claim
//! conversion then permits transfer and same-address recreation. Setup and continuation
//! use only System/SPL/ATA/wrapper instructions, with no economic account injection.

use super::*;
use solana_sdk::{instruction::InstructionError, rent::Rent, transaction::TransactionError};

#[track_caller]
fn census(world: &AttributionWorld, basis: [i128; 5], weights: [u128; 5]) {
    let group = world.env.market_state().1;
    let mut oi = [0; 2];
    let mut weight = [0; 2];
    let mut stored = [0; 2];
    let mut pending = [0; 2];
    let mut capital = 0;
    let mut positive = 0;
    let mut tokens = group.vault;
    for (i, actor) in world.actors.iter().enumerate() {
        let account = world.env.portfolio_state(actor.portfolio);
        assert_eq!(account.owner, actor.owner.pubkey().to_bytes());
        capital += account.capital.get();
        positive += account.pnl.get().max(0) as u128;
        tokens += world.env.token_amount(actor.token) as u128;
        let legs: Vec<_> = account
            .legs
            .iter()
            .map(|l| l.try_to_runtime().unwrap())
            .filter(|l| l.active)
            .collect();
        assert_eq!(
            legs.len(),
            usize::from(weights[i] != 0),
            "actor {i}: leg census for {basis:?}"
        );
        if weights[i] != 0 {
            let side = usize::from(world.quantities[i] < 0);
            let leg = legs[0];
            assert_eq!(leg.asset_index, 1);
            assert_eq!(
                leg.side,
                if side == 0 {
                    SideV16::Long
                } else {
                    SideV16::Short
                }
            );
            assert_eq!(leg.basis_pos_q, basis[i]);
            assert_eq!(
                leg.loss_weight, weights[i],
                "actor {i}: original obligation weight"
            );
            oi[side] += basis[i].unsigned_abs();
            weight[side] += weights[i];
            stored[side] += 1;
            pending[side] += u64::from(basis[i] == 0);
        }
    }
    let a = group.assets[1];
    assert_eq!([a.a_long, a.a_short], [ADL_ONE; 2]);
    assert_eq!([a.oi_eff_long_q, a.oi_eff_short_q], oi);
    assert_eq!([a.loss_weight_sum_long, a.loss_weight_sum_short], weight);
    assert_eq!([a.stored_pos_count_long, a.stored_pos_count_short], stored);
    assert_eq!(
        [
            a.pending_obligation_count_long,
            a.pending_obligation_count_short
        ],
        pending
    );
    assert_eq!(group.c_tot, capital);
    assert_eq!(group.pnl_pos_tot, positive);
    assert_eq!(world.env.token_amount(world.env.vault) as u128, group.vault);
    assert_eq!(tokens, ATTRIBUTION_DEPOSITS.iter().sum::<u128>());
    assert_eq!(
        Mint::unpack(&world.env.svm.get_account(&world.env.mint).unwrap().data)
            .unwrap()
            .supply as u128,
        tokens
    );
}

fn transfer_and_recreate(world: &AttributionWorld, amount: u128) -> Vec<Instruction> {
    let env = &world.env;
    let holder = &world.actors[0];
    let recipient = &world.actors[4];
    vec![
        Instruction {
            program_id: env.program_id,
            data: env.withdraw_ix(holder.portfolio, amount).encode(),
            accounts: vec![
                AccountMeta::new(holder.owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(holder.portfolio, false),
                AccountMeta::new(holder.token, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        },
        spl_token::instruction::transfer(
            &spl_token::ID,
            &holder.token,
            &recipient.token,
            &holder.owner.pubkey(),
            &[],
            u64::try_from(amount).unwrap(),
        )
        .unwrap(),
        Instruction {
            program_id: env.program_id,
            data: env.deposit_ix(recipient.portfolio, amount).encode(),
            accounts: vec![
                AccountMeta::new(recipient.owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(recipient.portfolio, false),
                AccountMeta::new(recipient.token, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
        },
        Instruction {
            program_id: env.program_id,
            data: ProgInstruction::ClosePortfolio {
                portfolio_id: env.portfolio_id(holder.portfolio),
                // The preceding Withdraw consumes this portfolio's custody sequence.
                expected_sequence: env
                    .portfolio_matcher_sequence(holder.portfolio)
                    .checked_add(1)
                    .unwrap(),
                position_epoch: env.portfolio_position_epoch(holder.portfolio),
            }
            .encode(),
            accounts: vec![
                AccountMeta::new(holder.owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(holder.portfolio, false),
            ],
        },
        system_instruction::transfer(
            &holder.owner.pubkey(),
            &holder.portfolio,
            env.svm
                .get_sysvar::<Rent>()
                .minimum_balance(env.portfolio_account_len),
        ),
        Instruction {
            program_id: env.program_id,
            data: ProgInstruction::InitPortfolio.encode(),
            accounts: vec![
                AccountMeta::new(holder.owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(holder.portfolio, false),
            ],
        },
    ]
}

#[test]
fn v16_program_reduced_cohort_keeps_exact_loss_after_paid_holder_transfer_and_recreation() {
    let mut peak_route_cu = 0;
    for reverse in [false, true] {
        let mut world = AttributionWorld::new_with_params(
            reverse,
            V16CuMarketParams {
                max_portfolio_assets: 3,
                initial_price: 1_000_000,
                maintenance_margin_bps: 1_000,
                initial_margin_bps: 1_000,
                max_price_move_bps_per_slot: 500,
                max_bankrupt_close_lifetime_slots: 1_000,
                ..V16CuMarketParams::default()
            },
        );
        let q = world.quantities[0];
        let w = q.unsigned_abs();
        world.quantities = [q, -q, q, -q];
        let ids: [Pubkey; 5] = std::array::from_fn(|i| world.actors[i].portfolio);
        let empty_sources = world.env.portfolio_state(ids[4]).source_domains;
        let unused_assets = [
            world.env.market_state().1.assets[0],
            world.env.market_state().1.assets[2],
        ];
        let trade = |world: &mut AttributionWorld, a: usize, b: usize, delta, mark| {
            let cu = world.env.trade_asset_with_cu(
                1,
                &world.actors[a].owner,
                ids[a],
                &world.actors[b].owner,
                ids[b],
                delta,
                mark,
                0,
            );
            assert_cu_within("INV-039 shared-cohort trade/reduction", cu, TRADE_CU_LIMIT);
        };
        let crank = |world: &mut AttributionWorld, actor, slot| {
            let cu = world.env.crank(
                ids[actor],
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(1),
                },
            );
            assert_cu_within("INV-039 shared-cohort settlement", cu, CRANK_CU_LIMIT);
        };
        let mut route = |world: &mut AttributionWorld, amount, recreate: bool| {
            world.env.svm.expire_blockhash();
            let mut ixs = vec![heap_ix(), cu_ix()];
            ixs.extend(transfer_and_recreate(world, amount));
            if !recreate {
                ixs.truncate(5);
            }
            let tx = Transaction::new_signed_with_payer(
                &ixs,
                Some(&world.env.payer.pubkey()),
                &[
                    &world.env.payer,
                    &world.actors[0].owner,
                    &world.actors[4].owner,
                ],
                world.env.svm.latest_blockhash(),
            );
            let result = world.env.svm.send_transaction(tx);
            let cu = match &result {
                Ok(meta) => meta.compute_units_consumed,
                Err(failure) => failure.meta.compute_units_consumed,
            };
            peak_route_cu = peak_route_cu.max(cu);
            assert_cu_within(
                "INV-039 transfer/close/recreate bundle",
                cu,
                3 * CUSTODY_CU_LIMIT,
            );
            result
        };

        trade(&mut world, 0, 1, q, 1_000_000);
        trade(&mut world, 2, 3, q, 1_000_000);
        census(&world, [q, -q, q, -q, 0], [w, w, w, w, 0]);
        let mut mark = 1_000_000;
        for slot in 1..=5 {
            mark = (1_000_000 + 40_000 * slot as i128 * q.signum()) as u64;
            world.env.svm.warp_to_slot(slot);
            world.env.push_auth_mark_for_asset_as_admin(1, slot, mark);
            crank(&mut world, 4, slot);
        }
        assert_eq!(world.env.market_state().1.assets[1].effective_price, mark);
        let gain = (i128::from(mark) - 1_000_000).unsigned_abs();
        let residual = gain - ATTRIBUTION_DEPOSITS[1];
        let share = residual / 2;
        assert_eq!((gain, residual, share), (200_000, 20_000, 10_000));
        trade(&mut world, 0, 1, -q, mark);
        census(&world, [0, 0, q, -q, 0], [w, 0, w, w, 0]);
        let close = close_progress(&world.env.portfolio_state(ids[1]));
        assert!(close.active && !close.finalized);
        assert_eq!(close.residual_remaining, residual);
        assert_eq!(
            world.env.portfolio_state(ids[1]).pnl.get(),
            -(residual as i128)
        );

        // Removing one quarter of exposure cannot discard one quarter of the old B share.
        trade(&mut world, 2, 3, -q / 4, mark);
        census(
            &world,
            [0, 0, 3 * q / 4, -3 * q / 4, 0],
            [w, 0, w, 3 * w / 4, 0],
        );
        trade(&mut world, 2, 3, -3 * q / 4, mark);
        census(&world, [0; 5], [w, 0, w, 0, 0]);
        assert_eq!(close_progress(&world.env.portfolio_state(ids[1])), close);
        for i in [0, 2] {
            assert_eq!(world.env.portfolio_state(ids[i]).pnl.get(), gain as i128);
        }
        assert_eq!(
            world.env.portfolio_state(ids[3]).capital.get(),
            ATTRIBUTION_DEPOSITS[3] - gain
        );
        let untouched_holder = world.env.svm.get_account(&ids[2]);

        for booked in [false, true] {
            if booked {
                crank(&mut world, 1, 5);
                let booked = close_progress(&world.env.portfolio_state(ids[1]));
                assert!(booked.finalized && !booked.canceled);
                assert_eq!(booked.residual_remaining, 0);
                assert_eq!(booked.b_loss_booked, residual);
                assert_eq!(booked.gross_loss_at_close_start, residual);
                assert_eq!(
                    [
                        booked.drift_consumed,
                        booked.support_consumed,
                        booked.junior_face_burned,
                        booked.insurance_spent,
                        booked.explicit_loss_assigned
                    ],
                    [0; 5]
                );
                assert_eq!(world.env.portfolio_state(ids[1]).pnl.get(), 0);
            }
            let before = world.frame();
            let failure = route(&mut world, ATTRIBUTION_DEPOSITS[0], true)
                .expect_err("unpaid holder cannot transfer principal and erase its incarnation");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(
                    2,
                    InstructionError::Custom(PercolatorError::EngineStale as u32)
                )
            );
            assert_eq!(
                world.frame(),
                before,
                "all economic accounts and rent roll back"
            );
            census(&world, [0; 5], [w, 0, w, 0, 0]);
        }

        for _ in 0..4 {
            if !has_active_leg_for_asset(&world.env.portfolio_state(ids[0]), 1) {
                break;
            }
            crank(&mut world, 0, 5);
        }
        census(&world, [0; 5], [0, 0, w, 0, 0]);
        let settled = world.env.portfolio_state(ids[0]);
        assert_eq!(settled.capital.get(), ATTRIBUTION_DEPOSITS[0]);
        assert_eq!(settled.pnl.get(), (gain - share) as i128);
        assert_eq!(world.env.svm.get_account(&ids[2]), untouched_holder);
        // Clearing the leg invalidates its certificate; refresh before the Live exit.
        crank(&mut world, 0, 5);
        let before = world.frame();
        let failure = route(&mut world, ATTRIBUTION_DEPOSITS[0], true)
            .expect_err("paid B does not erase the remaining claim at ClosePortfolio");
        assert_eq!(
            failure.err,
            TransactionError::InstructionError(
                5,
                InstructionError::Custom(PercolatorError::EngineLockActive as u32)
            )
        );
        assert_eq!(
            world.frame(),
            before,
            "late close rejection restores the successful SPL prefix"
        );

        // Commit the same transfer prefix alone, while the other original B share is unpaid.
        let sequence = world.env.portfolio_matcher_sequence(ids[0]);
        let position_epoch = world.env.portfolio_position_epoch(ids[0]);
        let assets_before = world.env.market_state().1.assets;
        let before = world.frame();
        route(&mut world, ATTRIBUTION_DEPOSITS[0], false).expect("paid holder transfers principal");
        census(&world, [0; 5], [0, 0, w, 0, 0]);
        assert_eq!(world.env.portfolio_matcher_sequence(ids[0]), sequence + 1);
        assert_eq!(world.env.portfolio_position_epoch(ids[0]), position_epoch);
        assert_eq!(world.env.market_state().1.assets, assets_before);
        assert_eq!(world.env.svm.get_account(&ids[2]), untouched_holder);
        assert_eq!(world.env.portfolio_state(ids[0]).capital.get(), 0);
        assert_eq!(world.env.portfolio_state(ids[0]).pnl, settled.pnl);
        assert_eq!(world.env.portfolio_state(ids[4]).pnl.get(), 0);
        assert_eq!(
            world.env.portfolio_state(ids[4]).source_domains,
            empty_sources
        );
        assert_eq!(
            world.env.portfolio_state(ids[4]).capital.get(),
            ATTRIBUTION_DEPOSITS[4] + ATTRIBUTION_DEPOSITS[0]
        );
        for (key, account) in before {
            if ![world.env.market, ids[0], ids[4]].contains(&key) {
                assert_eq!(
                    world.env.svm.get_account(&key),
                    account,
                    "unrelated account {key}"
                );
            }
        }

        // The surviving holder pays only its ORIGINAL half even after the other weight vanishes.
        let old_leg = active_leg_for_asset(&world.env.portfolio_state(ids[2]), 1);
        let asset = world.env.market_state().1.assets[1];
        assert!(
            old_leg.b_snap
                < if reverse {
                    asset.b_short_num
                } else {
                    asset.b_long_num
                }
        );
        for _ in 0..4 {
            if !has_active_leg_for_asset(&world.env.portfolio_state(ids[2]), 1) {
                break;
            }
            crank(&mut world, 2, 5);
        }
        census(&world, [0; 5], [0; 5]);
        assert_eq!(
            world.env.portfolio_state(ids[2]).capital.get(),
            ATTRIBUTION_DEPOSITS[2]
        );
        assert_eq!(
            world.env.portfolio_state(ids[2]).pnl.get(),
            (gain - share) as i128
        );
        for i in [0, 2, 4] {
            let value = world.env.portfolio_state(ids[i]);
            crank(&mut world, i, 5);
            let after = world.env.portfolio_state(ids[i]);
            assert_eq!(
                (after.capital, after.pnl),
                (value.capital, value.pnl),
                "no second B payment"
            );
        }

        // Waiting for both debits avoids voluntarily haircutting an early Live conversion.
        let cu =
            world
                .env
                .convert_released_pnl_with_cu(&world.actors[0].owner, ids[0], gain - share);
        assert_cu_within("INV-039 paid cohort claim conversion", cu, CUSTODY_CU_LIMIT);
        assert_eq!(
            world.env.portfolio_state(ids[0]).capital.get(),
            gain - share
        );
        assert_eq!(world.env.portfolio_state(ids[0]).pnl.get(), 0);
        let old_id = world.env.portfolio_id(ids[0]);
        let next_id = world.env.market_state().0.next_portfolio_id;
        let group_before = world.env.market_state().1;
        let before = world.frame();
        let old_rent = world.env.svm.get_account(&ids[0]).unwrap().lamports;
        let new_rent = world
            .env
            .svm
            .get_sysvar::<Rent>()
            .minimum_balance(world.env.portfolio_account_len);
        let owner = world.actors[0].owner.pubkey();
        let owner_lamports = world.env.svm.get_account(&owner).unwrap().lamports;
        let market_lamports = world
            .env
            .svm
            .get_account(&world.env.market)
            .unwrap()
            .lamports;
        route(&mut world, gain - share, true).expect("fully paid holder transfers and recreates");
        census(&world, [0; 5], [0; 5]);
        assert_ne!(world.env.portfolio_id(ids[0]), old_id);
        assert_eq!(world.env.portfolio_id(ids[0]), next_id);
        assert_eq!(
            world.env.svm.get_account(&ids[0]).unwrap().lamports,
            new_rent
        );
        assert_eq!(
            world.env.svm.get_account(&owner).unwrap().lamports,
            owner_lamports - new_rent
        );
        assert_eq!(
            world
                .env
                .svm
                .get_account(&world.env.market)
                .unwrap()
                .lamports,
            market_lamports + old_rent
        );
        let group_after = world.env.market_state().1;
        assert_eq!(group_after.materialized_portfolio_count, 5);
        assert_eq!(group_after.assets, group_before.assets);
        assert_eq!(group_after.source_credit, group_before.source_credit);
        assert_eq!(group_after.c_tot, group_before.c_tot);
        assert_eq!(group_after.vault, group_before.vault);
        for i in [0, 4] {
            let account = world.env.portfolio_state(ids[i]);
            assert_eq!(
                account.pnl.get(),
                0,
                "fresh accounts inherit no historical loss or claim"
            );
            assert_eq!(close_progress(&account), CloseProgressLedgerV16::EMPTY);
            assert_eq!(account.source_domains, empty_sources);
            assert!(!resolved_receipt(&account).present);
        }
        let entitlement = ATTRIBUTION_DEPOSITS[0] + gain - share;
        assert_eq!(world.env.portfolio_state(ids[0]).capital.get(), 0);
        assert_eq!(
            world.env.portfolio_state(ids[4]).capital.get(),
            ATTRIBUTION_DEPOSITS[4] + entitlement
        );
        for (key, account) in before {
            if ![world.env.market, ids[0], ids[4], owner].contains(&key) {
                assert_eq!(
                    world.env.svm.get_account(&key),
                    account,
                    "unrelated account {key}"
                );
            }
        }

        let expected = [
            0,
            0,
            ATTRIBUTION_DEPOSITS[2] + gain - share,
            ATTRIBUTION_DEPOSITS[3] - gain,
            ATTRIBUTION_DEPOSITS[4] + entitlement,
        ];
        assert_eq!(
            [
                world.env.market_state().1.assets[0],
                world.env.market_state().1.assets[2]
            ],
            unused_assets
        );
        world.env.resolve();
        world.env.svm.warp_to_slot(10);
        for _ in 0..8 {
            for i in [4, 0, 2, 1, 3] {
                if resolved_portfolio_is_terminal(&world.env, ids[i]) {
                    continue;
                }
                let before = world.frame();
                match world.payout(i, false) {
                    Ok(cu) => assert_cu_within("INV-039 cohort payout", cu, CUSTODY_CU_LIMIT),
                    Err(error) => {
                        assert!(is_engine_non_progress_error(&error), "{error}");
                        assert_eq!(world.frame(), before);
                    }
                }
                census(&world, [0; 5], [0; 5]);
                for (j, limit) in expected.into_iter().enumerate() {
                    assert!(world.env.token_amount(world.actors[j].token) as u128 <= limit);
                }
            }
        }
        for (i, amount) in expected.into_iter().enumerate() {
            assert!(resolved_portfolio_is_terminal(&world.env, ids[i]));
            assert_eq!(
                world.env.token_amount(world.actors[i].token) as u128,
                amount
            );
            let cu = world
                .env
                .close_portfolio_with_cu(&world.actors[i].owner, ids[i]);
            assert_cu_within("INV-039 cohort terminal deletion", cu, CUSTODY_CU_LIMIT);
        }
        assert_eq!(world.env.market_state().1.vault, 0);
        assert_eq!(world.env.market_state().1.c_tot, 0);
        assert_eq!(world.env.market_state().1.materialized_portfolio_count, 0);
    }
    println!("INV-039: 2 mirrored shared-cohort worlds, 4 retained-weight reductions, 6 exact rollbacks, 2 principal transfers, 2 paid-holder recreations, 10 exact payouts; peak route {peak_route_cu} CU");
}
