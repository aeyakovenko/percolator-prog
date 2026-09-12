//! INV-039: collateral cannot carry a retained loss obligation to another portfolio.
//!
//! Unlike the deposit-only and resolved-detach witnesses, this crosses the Live withdrawal
//! gate with a zero-basis, nonzero-loss-weight leg. A valid Withdraw/SPL transfer/Deposit
//! sequence rejects both before debtor settlement and after settlement but before holder
//! release. The identical public route succeeds after release, preserving claim ownership
//! and exact terminal payouts. All setup uses the parent's System/SPL/ATA/wrapper fixture.
//! This covers two side orientations and one-atom/full-principal transfers, not bankruptcy,
//! nonzero fees/funding, CPI trading, or arbitrary transaction histories.

use super::*;
use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

fn collateral_transfer(world: &AttributionWorld, amount: u128) -> Vec<Instruction> {
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
    ]
}

#[test]
fn v16_program_pending_obligation_cannot_follow_withdraw_transfer_redeposit() {
    let mut peak_route_cu = 0;
    let mut transfer = |world: &mut AttributionWorld, amount| {
        world.env.svm.expire_blockhash();
        let mut instructions = vec![heap_ix(), cu_ix()];
        instructions.extend(collateral_transfer(world, amount));
        let tx = Transaction::new_signed_with_payer(
            &instructions,
            Some(&world.env.payer.pubkey()),
            &[
                &world.env.payer,
                &world.actors[0].owner,
                &world.actors[4].owner,
            ],
            world.env.svm.latest_blockhash(),
        );
        let result = world.env.svm.send_transaction(tx);
        let meta = match &result {
            Ok(meta) => meta,
            Err(failure) => &failure.meta,
        };
        peak_route_cu = peak_route_cu.max(meta.compute_units_consumed);
        assert_cu_within(
            "INV-039 withdrawal/transfer/redeposit transaction",
            meta.compute_units_consumed,
            CUSTODY_CU_LIMIT,
        );
        result
    };

    for reverse_sides in [false, true] {
        for amount in [1, ATTRIBUTION_DEPOSITS[0]] {
            let mut world = AttributionWorld::new(reverse_sides);
            let holder = world.actors[0].portfolio;
            let debtor = world.actors[1].portfolio;
            let recipient = world.actors[4].portfolio;
            let q = world.quantities[0];
            let debt = world.debt(0);
            let cu = world.env.trade_asset_with_cu(
                1,
                &world.actors[0].owner,
                holder,
                &world.actors[1].owner,
                debtor,
                q,
                1_000_000,
                0,
            );
            assert_cu_within("INV-039 transfer-route opening", cu, TRADE_CU_LIMIT);
            let debtor_unsettled = world.env.svm.get_account(&debtor);
            world.env.svm.warp_to_slot(20);
            let mark = (1_000_000 + ATTRIBUTION_PRICE_MOVES[0] * q.signum()) as u64;
            world.env.push_auth_mark_for_asset_as_admin(1, 20, mark);
            for portfolio in [recipient, holder] {
                world.env.crank(
                    portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 20,
                        observations: crank_observations(1),
                    },
                );
            }
            assert_eq!(world.env.market_state().1.assets[1].effective_price, mark);
            world.env.update_asset_lifecycle_as_admin_with_cu(
                processor::ASSET_ACTION_SHUTDOWN,
                1,
                20,
                0,
            );
            world.forfeit(0);
            let mut basis = [0, -q, 0, 0];
            world.check(basis, [true, false, false, false]);
            assert_eq!(world.env.market_state().1.mode, MarketModeV16::Live);
            assert_eq!(world.env.svm.get_account(&debtor), debtor_unsettled);
            let retained = world.env.portfolio_state(holder);
            assert_eq!(retained.capital.get(), ATTRIBUTION_DEPOSITS[0]);
            assert_eq!(retained.pnl.get(), debt as i128);
            let untouched_recipient = world.env.svm.get_account(&recipient);

            for debtor_settled in [false, true] {
                if debtor_settled {
                    world.forfeit(1);
                    basis[1] = 0;
                    assert_eq!(
                        world.env.portfolio_state(debtor).capital.get(),
                        ATTRIBUTION_DEPOSITS[1] - debt
                    );
                    assert_eq!(world.env.portfolio_state(debtor).pnl.get(), 0);
                }
                world.check(basis, [true, false, false, false]);
                let before = world.frame();
                let failure = transfer(&mut world, amount)
                    .expect_err("zero exposure is not release of a retained obligation");
                assert_eq!(
                    failure.err,
                    TransactionError::InstructionError(
                        2,
                        InstructionError::Custom(PercolatorError::EngineStale as u32)
                    ),
                    "must reach the economic withdrawal gate, not an account/signature error"
                );
                assert_eq!(world.frame(), before, "complete economic-account rollback");
                assert_eq!(world.env.svm.get_account(&recipient), untouched_recipient);
                world.check(basis, [true, false, false, false]);
            }

            let cu = world.env.crank(
                holder,
                ProgInstruction::PermissionlessCrank {
                    now_slot: 20,
                    observations: crank_observations(1),
                },
            );
            assert_cu_within(
                "INV-039 transfer-route obligation release",
                cu,
                CRANK_CU_LIMIT,
            );
            world.check([0; 4], [false; 4]);
            let released = world.env.portfolio_state(holder);
            assert_eq!(released.capital, retained.capital);
            assert_eq!(released.pnl, retained.pnl);
            let before_transfer = world.frame();
            let group_before = world.env.market_state().1;

            // Refresh sequence bindings after honest progress; accounts, amount and route are
            // unchanged. Success distinguishes the pending-leg guard from malformed transport.
            transfer(&mut world, amount).expect("released principal can follow the public route");
            world.check([0; 4], [false; 4]);
            let moved = world.env.portfolio_state(holder);
            let received = world.env.portfolio_state(recipient);
            assert_eq!(moved.capital.get(), ATTRIBUTION_DEPOSITS[0] - amount);
            assert_eq!(moved.pnl, released.pnl);
            assert_eq!(moved.source_domains, released.source_domains);
            assert_eq!(received.capital.get(), ATTRIBUTION_DEPOSITS[4] + amount);
            assert_eq!(
                received.pnl.get(),
                0,
                "recipient inherits no historical claim or debt"
            );
            let group_after = world.env.market_state().1;
            assert_eq!(group_after.assets, group_before.assets);
            assert_eq!(group_after.source_credit, group_before.source_credit);
            assert_eq!(group_after.c_tot, group_before.c_tot);
            assert_eq!(group_after.vault, group_before.vault);
            for (key, account) in before_transfer {
                if key != world.env.market && key != holder && key != recipient {
                    assert_eq!(world.env.svm.get_account(&key), account);
                }
            }

            let mut expected = ATTRIBUTION_DEPOSITS;
            expected[0] += debt;
            expected[0] -= amount;
            expected[1] -= debt;
            expected[4] += amount;
            world.env.resolve();
            world.env.svm.warp_to_slot(25);
            // Pay the recipient first: moving capital never moves the original claim's face.
            for _ in 0..8 {
                for actor in [4, 0, 1, 2, 3] {
                    if resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio) {
                        continue;
                    }
                    let before = world.frame();
                    match world.payout(actor, false) {
                        Ok(cu) => {
                            assert_cu_within(
                                "INV-039 transferred-principal payout",
                                cu,
                                CUSTODY_CU_LIMIT,
                            );
                            assert_ne!(world.frame(), before, "successful payout must progress");
                        }
                        Err(error) => {
                            assert!(is_engine_non_progress_error(&error), "{error}");
                            assert_eq!(world.frame(), before);
                        }
                    }
                    world.check([0; 4], [false; 4]);
                    for (i, limit) in expected.into_iter().enumerate() {
                        assert!(world.env.token_amount(world.actors[i].token) as u128 <= limit);
                    }
                }
            }
            for (i, entitlement) in expected.into_iter().enumerate() {
                assert_eq!(
                    world.env.token_amount(world.actors[i].token) as u128,
                    entitlement
                );
                assert!(resolved_portfolio_is_terminal(
                    &world.env,
                    world.actors[i].portfolio
                ));
                let before = world.frame();
                world.payout(i, true).expect("settled claim retry");
                assert_eq!(
                    world.frame(),
                    before,
                    "no repeated terminal value extraction"
                );
            }
            assert_eq!(world.env.market_state().1.vault, 0);
            for actor in &world.actors {
                let cu = world
                    .env
                    .close_portfolio_with_cu(&actor.owner, actor.portfolio);
                assert_cu_within(
                    "INV-039 transfer-route portfolio deletion",
                    cu,
                    CUSTODY_CU_LIMIT,
                );
            }
            assert_eq!(world.env.market_state().1.materialized_portfolio_count, 0);
        }
    }
    println!("INV-039: 4 transfer-route worlds, 8 exact pending-leg rollbacks, 4 released transfers, 20 exact payouts; peak route {peak_route_cu} CU");
}
