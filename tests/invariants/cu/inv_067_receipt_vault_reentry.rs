//! INV-024/067/068/070, row 417: a paid receipt's tokens reenter the PRIMARY vault
//! before and during late backing expiry. Returned custody is unbooked surplus,
//! not new residual or renewed receipt credit. Unlike spend_replay's debtor wallet,
//! secondary-rail liquidity, and terminal_disposition's post-receipt donation, this
//! crosses source reclassification with already-paid atoms inside primary custody.
//! Public SPL/wrapper instructions only; this finite witness leaves row 417 OPEN.

use super::*;

fn return_to_vault(world: &World, actor: usize, amount: u128) -> Instruction {
    spl_token::instruction::transfer(
        &spl_token::ID,
        &world.actors[actor].token,
        &world.env.vault,
        &world.actors[actor].owner.pubkey(),
        &[],
        u64::try_from(amount).unwrap(),
    )
    .unwrap()
}

fn check_cash(world: &World, donor: usize, paid: [u128; 5], returned: u128) {
    let group = world.env.market_state().1;
    let booked = SUPPLY - 1 - paid.iter().sum::<u128>();
    assert_eq!(
        group.vault, booked,
        "returned tokens are never booked again"
    );
    assert_eq!(
        u128::from(world.env.token_amount(world.env.vault)),
        booked + returned
    );
    let balances = std::array::from_fn::<_, 5, _>(|actor| {
        let expected = paid[actor] - if actor == donor { returned } else { 0 };
        assert_eq!(
            u128::from(world.env.token_amount(world.actors[actor].token)),
            expected
        );
        expected
    });
    assert_eq!(
        booked + returned + balances.iter().sum::<u128>() + 1,
        SUPPLY
    );
    assert_eq!(world.env.token_amount(world.provider_token), 1);
    let mint = Mint::unpack(&world.env.svm.get_account(&world.env.mint).unwrap().data).unwrap();
    assert_eq!(u128::from(mint.supply), SUPPLY);
    assert_eq!(mint.mint_authority, COption::Some(world.env.admin.pubkey()));
}

fn check_receipts(world: &World, original: [ResolvedPayoutReceiptV16; 2], residual: u128) {
    let group = world.env.market_state().1;
    let ledger = group.resolved_payout_ledger;
    assert_eq!(ledger.snapshot_slot, 12);
    assert_eq!(ledger.snapshot_residual, residual);
    assert_eq!(ledger.current_payout_rate_num, residual * BOUND_SCALE);
    assert_eq!(ledger.current_payout_rate_den, TOTAL_FACE * BOUND_SCALE);
    assert_eq!(
        ledger.terminal_claim_bound_unreceipted_num,
        FACES[2] * BOUND_SCALE
    );
    assert_eq!(
        ledger.terminal_claim_exact_receipts_num,
        (FACES[0] + FACES[4]) * BOUND_SCALE
    );
    assert!(!ledger.finalized && !ledger.payout_halted);
    for (actor, mut receipt) in [0, 4].into_iter().zip(original) {
        receipt.paid_effective = claim(actor, residual);
        assert_eq!(
            world.receipt(actor),
            receipt,
            "receipt identity survives vault reentry"
        );
    }
}

#[test]
fn v16_program_returned_receipt_tokens_remain_surplus_across_late_expiry() {
    let expected = std::array::from_fn::<_, 5, _>(|actor| CAPITAL[actor] + claim(actor, FINAL));
    assert_eq!(expected, [1_198, 0, 1_283, 0, 1_368]);
    let rounding = FINAL - (0..5).map(|actor| claim(actor, FINAL)).sum::<u128>();
    assert_eq!(rounding, 2);
    let mut peak_cu = 0;
    for donor in [0, 4] {
        for order in [[0, 4], [4, 0]] {
            let mut world = World::new();
            world.peak_cu = 0;
            let original = [world.receipt(0), world.receipt(4)];
            let retained = order.map(|actor| world.payout(actor, true));
            let source_close = world.payout(2, false);
            let mut paid = [
                CAPITAL[0] + claim(0, INITIAL),
                0,
                0,
                0,
                CAPITAL[4] + claim(4, INITIAL),
            ];
            let mut returned = paid[donor];
            let before = world.frame();
            let donation = return_to_vault(&world, donor, returned);
            land_spending(&mut world, donor, &[donation]).unwrap();
            world.assert_frame_except(&before, &[world.env.vault, world.actors[donor].token]);
            check_cash(&world, donor, paid, returned);
            check_receipts(&world, original, INITIAL);
            let before = world.frame();
            let meta = world.land(&retained, false).unwrap();
            assert_eq!(successes(&meta.logs, spl_token::ID), 0);
            assert_eq!(
                world.frame(),
                before,
                "more primary custody cannot replenish a paid receipt"
            );

            world.env.svm.warp_to_slot(13);
            let due = claim(donor, FINAL) - claim(donor, INITIAL);
            // Normalize backing, pay both older receipts in either order, then return
            // the donor's new payout and replay its unchanged pre-expiry request.
            let bundle = [
                source_close.clone(),
                retained[0].clone(),
                retained[1].clone(),
                return_to_vault(&world, donor, due),
                retained[usize::from(order[1] == donor)].clone(),
            ];
            let before = world.frame();
            let mut rejected = bundle.to_vec();
            rejected.push(Instruction {
                program_id: solana_sdk::system_program::ID,
                accounts: vec![],
                data: vec![],
            });
            let failure = land_spending(&mut world, donor, &rejected).unwrap_err();
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(7, InstructionError::InvalidInstructionData)
            );
            assert_eq!(successes(&failure.meta.logs, world.env.program_id), 4);
            assert_eq!(successes(&failure.meta.logs, spl_token::ID), 3);
            assert_eq!(
                world.frame(),
                before,
                "expiry, payouts and returned custody roll back together"
            );
            check_cash(&world, donor, paid, returned);
            check_receipts(&world, original, INITIAL);

            let meta = land_spending(&mut world, donor, &bundle).unwrap();
            assert_eq!(successes(&meta.logs, world.env.program_id), 4);
            assert_eq!(successes(&meta.logs, spl_token::ID), 3);
            paid[0] = expected[0];
            paid[4] = expected[4];
            returned += due;
            check_cash(&world, donor, paid, returned);
            check_receipts(&world, original, FINAL);
            world.assert_frame_except(
                &before,
                &[
                    world.env.market,
                    world.env.vault,
                    world.actors[0].portfolio,
                    world.actors[2].portfolio,
                    world.actors[4].portfolio,
                    world.actors[0].token,
                    world.actors[4].token,
                ],
            );
            let group = world.env.market_state().1;
            assert_eq!(
                group.source_backing_buckets[3].status,
                BackingBucketStatusV16::Expired
            );
            assert_eq!(group.source_credit[3].fresh_reserved_backing_num, 0);
            assert_eq!(group.source_credit[3].provider_receivable_num, 0);
            assert_eq!(group.c_tot, CAPITAL[2]);
            let source = world.env.portfolio_state(world.actors[2].portfolio);
            assert_eq!(source.capital.get(), CAPITAL[2]);
            assert_eq!(source.pnl.get(), FACES[2] as i128);
            assert_eq!(source.reserved_pnl.get(), 0);
            assert!(!world.receipt(2).present);

            // The delayed source claimant also receives only its original entitlement,
            // although the physical primary vault now contains a large returned surplus.
            world.land(&[source_close], false).unwrap();
            paid[2] = expected[2];
            world.land(&retained, false).unwrap();
            check_cash(&world, donor, paid, returned);
            assert_eq!(returned, expected[donor]);
            for actor in 0..5 {
                assert!(resolved_portfolio_is_terminal(
                    &world.env,
                    world.actors[actor].portfolio
                ));
                assert!(!world.receipt(actor).present);
                let before = world.frame();
                world.land(&[world.payout(actor, true)], false).unwrap();
                assert_eq!(
                    world.frame(),
                    before,
                    "returned payouts cannot revive settled claims"
                );
            }
            for actor in 0..5 {
                let before = world.frame();
                let portfolio = world.actors[actor].portfolio;
                let rent = world.env.svm.get_account(&portfolio).unwrap().lamports;
                let market_rent = world
                    .env
                    .svm
                    .get_account(&world.env.market)
                    .unwrap()
                    .lamports;
                let cu = world
                    .env
                    .close_portfolio_with_cu(&world.actors[actor].owner, portfolio);
                world.peak_cu = world.peak_cu.max(cu);
                assert_eq!(
                    world
                        .env
                        .svm
                        .get_account(&world.env.market)
                        .unwrap()
                        .lamports,
                    market_rent + rent
                );
                world.assert_frame_except(&before, &[world.env.market, portfolio]);
            }
            check_cash(&world, donor, paid, returned);
            let group = world.env.market_state().1;
            assert_eq!(group.materialized_portfolio_count, 0);
            assert_eq!(
                (
                    group.c_tot,
                    group.pnl_pos_tot,
                    group.source_claim_bound_total_num,
                    group.insurance,
                    group.backing_provider_earnings_total
                ),
                (0, 0, 0, 0, 0)
            );
            assert_eq!(group.vault, rounding);

            let before = world.frame();
            let market_rent = world
                .env
                .svm
                .get_account(&world.env.market)
                .unwrap()
                .lamports;
            let vault_rent = world
                .env
                .svm
                .get_account(&world.env.vault)
                .unwrap()
                .lamports;
            let admin_rent = world
                .env
                .svm
                .get_account(&world.env.admin.pubkey())
                .unwrap()
                .lamports;
            let close = Instruction {
                program_id: world.env.program_id,
                accounts: vec![
                    AccountMeta::new(world.env.admin.pubkey(), true),
                    AccountMeta::new(world.env.market, false),
                    AccountMeta::new(world.env.vault, false),
                    AccountMeta::new_readonly(world.env.vault_authority, false),
                    AccountMeta::new(world.provider_token, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(world.env.mint, false),
                ],
                data: ProgInstruction::CloseSlab {
                    authority_epoch: world.env.control_sequences(0).authority_epoch,
                }
                .encode(),
            };
            for _ in 0..8 {
                let step = world.frame();
                world.land(&[close.clone()], true).unwrap();
                assert_ne!(world.frame(), step);
                if world
                    .env
                    .svm
                    .get_account(&world.env.market)
                    .unwrap()
                    .data
                    .len()
                    == percolator_prog::constants::HEADER_LEN
                {
                    break;
                }
                world.assert_frame_except(&step, &[world.env.market]);
                check_cash(&world, donor, paid, returned);
            }
            let tombstone = world.env.svm.get_account(&world.env.market).unwrap();
            assert_closed_market_tombstone(&tombstone);
            let rent = world
                .env
                .svm
                .get_sysvar::<solana_sdk::rent::Rent>()
                .minimum_balance(percolator_prog::constants::HEADER_LEN);
            assert_eq!(tombstone.lamports, rent);
            assert_eq!(
                world
                    .env
                    .svm
                    .get_account(&world.env.admin.pubkey())
                    .unwrap()
                    .lamports,
                admin_rent + market_rent + vault_rent - rent
            );
            assert!(world
                .env
                .svm
                .get_account(&world.env.vault)
                .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
            assert_eq!(
                u128::from(world.env.token_amount(world.provider_token)),
                1 + returned
            );
            let mut expected_mint = before
                .iter()
                .find(|(key, _)| *key == world.env.mint)
                .unwrap()
                .1
                .clone()
                .unwrap();
            let mut mint = Mint::unpack(&expected_mint.data).unwrap();
            mint.supply -= rounding as u64;
            Mint::pack(mint, &mut expected_mint.data).unwrap();
            assert_eq!(
                world.env.svm.get_account(&world.env.mint),
                Some(expected_mint)
            );
            world.assert_frame_except(
                &before,
                &[
                    world.env.market,
                    world.env.vault,
                    world.env.mint,
                    world.provider_token,
                    world.env.admin.pubkey(),
                ],
            );
            peak_cu = peak_cu.max(world.peak_cu);
            println!("INV-067 primary reentry: donor={donor}, order={order:?}, gross={expected:?}, returned={returned}, burned={rounding}, rollback=1, peak_CU={}", world.peak_cu);
        }
    }
    assert_cu_within("receipt primary vault reentry", peak_cu, 500_000);
}
