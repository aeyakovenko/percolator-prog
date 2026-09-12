//! INV-039: input-derived owner attribution across two pending resolved cohorts.
//!
//! Unlike the parent's pre-resolution release matrix and single-cohort detach witness,
//! these histories resolve with two retained holders and at least one unbooked debtor.
//! Every close prefix reconciles each owner's remaining capital, PnL, receipt and SPL
//! payout against independently computed debt. A disappearing leg is not debt payment.
//! This is solvent, integral-quantity, no-fee/funding evidence, not row419 closure or
//! bankruptcy-residual, exact-partition, crank-rank or arbitrary-history coverage.
//! The terminal-close witness also carries settled obligations through actual vault
//! closure and slab retirement, including rollback of a staged debtor SPL payout.

use super::*;
use proptest::prelude::*;

#[path = "inv_039_pending_destination_recovery.rs"]
mod pending_destination_recovery;

#[derive(Clone, Debug)]
struct History {
    reverse_sides: bool,
    lots: [u8; 2],
    price_moves: [u16; 2],
    early_debtor: Option<usize>,
    close_order: [usize; 5],
    extra_closes: Vec<usize>,
}

struct AttributionModel {
    debt: [u128; 2],
    basis: [i128; 4],
    pending: [bool; 4],
}

impl AttributionModel {
    fn assert_matches(&self, world: &AttributionWorld) {
        self.assert_matches_with_deleted_debtor(world, None);
    }

    fn assert_matches_with_deleted_debtor(&self, world: &AttributionWorld, deleted: Option<usize>) {
        world.check_with_deleted_debtor(self.basis, self.pending, deleted);
        let group = world.env.market_state().1;
        for (actor, initial) in ATTRIBUTION_DEPOSITS.into_iter().enumerate() {
            let expected = match actor {
                0 | 2 => initial + self.debt[actor / 2],
                1 | 3 if self.basis[actor] == 0 => initial - self.debt[actor / 2],
                _ => initial,
            };
            if Some(actor) == deleted {
                assert_eq!(
                    world.env.token_amount(world.actors[actor].token) as u128,
                    expected
                );
                continue;
            }
            let account = world.env.portfolio_state(world.actors[actor].portfolio);
            let receipt = resolved_receipt(&account);
            let receipt_due = if receipt.present {
                receipt
                    .terminal_positive_claim_face
                    .checked_sub(receipt.paid_effective)
                    .expect("a solvent receipt cannot pay more than its own face")
            } else {
                0
            };
            let remaining = account.capital.get() as i128
                + account.pnl.get()
                + receipt_due as i128
                + world.env.token_amount(world.actors[actor].token) as i128;
            assert_eq!(
                remaining, expected as i128,
                "actor {actor}: debt attribution"
            );
            if actor < 4 && actor % 2 == 0 && self.basis[actor + 1] != 0 {
                assert_eq!(account.capital.get(), initial);
                assert_eq!(account.pnl.get(), self.debt[actor / 2] as i128);
                assert!(
                    !receipt.present,
                    "unbooked opposing debt cannot become a receipt"
                );
                assert_eq!(world.env.token_amount(world.actors[actor].token), 0);
                assert!(!group.payout_snapshot_captured);
            }
        }
    }

    fn close(&mut self, world: &mut AttributionWorld, actor: usize) {
        self.close_with_deleted_debtor(world, actor, None);
    }

    fn close_with_deleted_debtor(
        &mut self,
        world: &mut AttributionWorld,
        actor: usize,
        deleted: Option<usize>,
    ) {
        assert_ne!(Some(actor), deleted);
        let before = world.frame();
        match world.payout(actor, false) {
            Ok(cu) => {
                assert_cu_within("INV-039 generated resolved close", cu, CUSTODY_CU_LIMIT);
                // One solvent leg, no B chunks, backing normalization or fee work.
                if actor < 4 {
                    self.basis[actor] = 0;
                    self.pending[actor] = false;
                }
            }
            Err(error) => {
                assert!(
                    is_engine_non_progress_error(&error),
                    "actor {actor}: {error}"
                );
                assert_eq!(
                    world.frame(),
                    before,
                    "rejected close must restore the whole frame"
                );
            }
        }
        for (key, account) in before {
            if ![
                world.env.market,
                world.env.vault,
                world.actors[actor].portfolio,
                world.actors[actor].token,
            ]
            .contains(&key)
            {
                assert_eq!(
                    world.env.svm.get_account(&key),
                    account,
                    "foreign account {key}"
                );
            }
        }
        self.assert_matches_with_deleted_debtor(world, deleted);
    }
}

fn resolve_history(history: &History) -> (AttributionWorld, AttributionModel) {
    let mut world = AttributionWorld::new(history.reverse_sides);
    let sign = if history.reverse_sides { -1 } else { 1 };
    for pair in 0..2 {
        // Integral lots make the economic oracle independent of engine rounding helpers.
        let q = POS_SCALE as i128 * i128::from(history.lots[pair]) * sign;
        world.quantities[2 * pair] = q;
        world.quantities[2 * pair + 1] = -q;
        world.env.trade_asset_with_cu(
            (pair + 1) as u16,
            &world.actors[2 * pair].owner,
            world.actors[2 * pair].portfolio,
            &world.actors[2 * pair + 1].owner,
            world.actors[2 * pair + 1].portfolio,
            q,
            1_000_000,
            0,
        );
    }
    world.env.svm.warp_to_slot(20);
    for pair in 0..2 {
        world.env.push_auth_mark_for_asset_as_admin(
            (pair + 1) as u16,
            20,
            (1_000_000 + i128::from(history.price_moves[pair]) * sign) as u64,
        );
    }
    world.env.crank(
        world.actors[4].portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 20,
            observations: crank_observations_for_assets(&[1, 2]),
        },
    );
    let mut model = AttributionModel {
        debt: std::array::from_fn(|pair| {
            u128::from(history.lots[pair]) * u128::from(history.price_moves[pair])
        }),
        basis: world.quantities,
        pending: [false; 4],
    };
    for pair in 0..2 {
        let debtor_before = world
            .env
            .svm
            .get_account(&world.actors[2 * pair + 1].portfolio);
        world.env.crank(
            world.actors[2 * pair].portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 20,
                observations: crank_observations((pair + 1) as u16),
            },
        );
        world.env.update_asset_lifecycle_as_admin_with_cu(
            processor::ASSET_ACTION_SHUTDOWN,
            (pair + 1) as u16,
            20,
            0,
        );
        world.forfeit(2 * pair);
        model.basis[2 * pair] = 0;
        model.pending[2 * pair] = true;
        assert_eq!(
            world
                .env
                .svm
                .get_account(&world.actors[2 * pair + 1].portfolio),
            debtor_before
        );
    }
    model.assert_matches(&world);
    if let Some(pair) = history.early_debtor {
        world.forfeit(2 * pair + 1);
        model.basis[2 * pair + 1] = 0;
        model.assert_matches(&world);
    }
    assert!(model.basis[1] != 0 || model.basis[3] != 0);
    assert_eq!(model.pending, [true, false, true, false]);
    let before_resolve = world.frame();
    let assets_before = world.env.market_state().1.assets;
    assert_cu_within(
        "INV-039 generated resolution",
        world.env.resolve(),
        CRANK_CU_LIMIT,
    );
    assert_eq!(world.env.market_state().1.mode, MarketModeV16::Resolved);
    assert_eq!(world.env.market_state().1.assets, assets_before);
    for (key, account) in before_resolve {
        if key != world.env.market {
            assert_eq!(world.env.svm.get_account(&key), account);
        }
    }
    model.assert_matches(&world);
    world.env.svm.warp_to_slot(25);
    (world, model)
}

fn run_history(history: &History) -> [u128; 5] {
    let (mut world, mut model) = resolve_history(history);
    for actor in history.extra_closes.iter().copied() {
        model.close(&mut world, actor);
    }
    for actor in history.close_order {
        model.close(&mut world, actor);
        model.close(&mut world, actor);
    }
    // A fixed suffix completes every generated prefix, without invoking the crank-rank oracle.
    for _ in 0..3 {
        for actor in history.close_order {
            if !resolved_portfolio_is_terminal(&world.env, world.actors[actor].portfolio) {
                model.close(&mut world, actor);
            }
        }
    }
    let expected = std::array::from_fn(|actor| match actor {
        0 | 2 => ATTRIBUTION_DEPOSITS[actor] + model.debt[actor / 2],
        1 | 3 => ATTRIBUTION_DEPOSITS[actor] - model.debt[actor / 2],
        _ => ATTRIBUTION_DEPOSITS[actor],
    });
    assert_eq!(model.basis, [0; 4]);
    assert_eq!(model.pending, [false; 4]);
    assert_eq!(world.env.market_state().1.vault, 0);
    for actor in history.close_order.into_iter().rev() {
        let a = &world.actors[actor];
        assert!(resolved_portfolio_is_terminal(&world.env, a.portfolio));
        assert_eq!(world.env.token_amount(a.token) as u128, expected[actor]);
        let before = world.frame();
        world.env.close_portfolio_with_cu(&a.owner, a.portfolio);
        for (key, account) in before {
            if ![world.env.market, a.owner.pubkey(), a.portfolio].contains(&key) {
                assert_eq!(world.env.svm.get_account(&key), account);
            }
        }
    }
    assert_eq!(world.env.market_state().1.materialized_portfolio_count, 0);
    expected
}

#[test]
fn v16_program_resolved_debtor_deletion_preserves_unsettled_cohort_attribution() {
    let mut worlds = 0;
    for reverse_sides in [false, true] {
        for settled_pair in 0..2 {
            for detach_first in [false, true] {
                let history = History {
                    reverse_sides,
                    lots: [1, 2],
                    price_moves: [1, 19_999],
                    early_debtor: None,
                    close_order: [0, 1, 2, 3, 4],
                    extra_closes: Vec::new(),
                };
                let (mut world, mut model) = resolve_history(&history);
                let holder = 2 * settled_pair;
                let debtor = holder + 1;
                let other_holder = 2 * (1 - settled_pair);
                let other_debtor = other_holder + 1;
                if detach_first {
                    model.close(&mut world, holder);
                    assert!(!model.pending[holder]);
                }
                model.close(&mut world, debtor);
                assert!(resolved_portfolio_is_terminal(
                    &world.env,
                    world.actors[debtor].portfolio
                ));
                assert_eq!(model.basis[debtor], 0);
                assert_eq!(model.pending[holder], !detach_first);
                assert!(model.pending[other_holder]);
                assert_ne!(model.basis[other_debtor], 0);
                assert!(!world.env.market_state().1.payout_snapshot_captured);

                // Delete in Resolved mode, before either original holder's claim is paid.
                for actor in [holder, other_holder] {
                    assert_eq!(world.env.token_amount(world.actors[actor].token), 0);
                    assert_eq!(
                        world
                            .env
                            .portfolio_state(world.actors[actor].portfolio)
                            .pnl
                            .get(),
                        model.debt[actor / 2] as i128
                    );
                }
                let before = world.frame();
                let group_before = world.env.market_state().1;
                let a = &world.actors[debtor];
                let rent = world.env.svm.get_account(&a.portfolio).unwrap().lamports;
                let market_lamports = world
                    .env
                    .svm
                    .get_account(&world.env.market)
                    .unwrap()
                    .lamports;
                let cu = world.env.close_portfolio_with_cu(&a.owner, a.portfolio);
                assert_cu_within(
                    "INV-039 resolved debtor deletion with outstanding debt",
                    cu,
                    CUSTODY_CU_LIMIT,
                );
                let group_after = world.env.market_state().1;
                assert_eq!(
                    group_after.materialized_portfolio_count,
                    group_before.materialized_portfolio_count - 1
                );
                assert_eq!(group_after.assets, group_before.assets);
                assert_eq!(group_after.source_credit, group_before.source_credit);
                assert_eq!(
                    group_after.source_backing_buckets,
                    group_before.source_backing_buckets
                );
                assert_eq!(
                    group_after.insurance_domain_spent,
                    group_before.insurance_domain_spent
                );
                assert_eq!(group_after.c_tot, group_before.c_tot);
                assert_eq!(group_after.pnl_pos_tot, group_before.pnl_pos_tot);
                assert_eq!(group_after.vault, group_before.vault);
                assert!(!group_after.payout_snapshot_captured);
                assert_eq!(
                    world
                        .env
                        .svm
                        .get_account(&world.env.market)
                        .unwrap()
                        .lamports,
                    market_lamports + rent
                );
                for (key, account) in before {
                    if ![world.env.market, a.portfolio].contains(&key) {
                        assert_eq!(
                            world.env.svm.get_account(&key),
                            account,
                            "deletion preserves foreign account {key}"
                        );
                    }
                }
                model.assert_matches_with_deleted_debtor(&world, Some(debtor));

                model.close_with_deleted_debtor(&mut world, other_holder, Some(debtor));
                assert!(!model.pending[other_holder]);
                let before_retry = world.frame();
                let error = world
                    .payout(other_holder, false)
                    .expect_err("deletion cannot pay or forgive the surviving debtor's obligation");
                assert!(is_engine_non_progress_error(&error), "{error}");
                assert_eq!(world.frame(), before_retry);
                model.assert_matches_with_deleted_debtor(&world, Some(debtor));

                model.close_with_deleted_debtor(&mut world, other_debtor, Some(debtor));
                assert_eq!(model.basis[other_debtor], 0);
                for _ in 0..4 {
                    for actor in [other_holder, holder, other_debtor, 4] {
                        if !resolved_portfolio_is_terminal(
                            &world.env,
                            world.actors[actor].portfolio,
                        ) {
                            model.close_with_deleted_debtor(&mut world, actor, Some(debtor));
                        }
                    }
                }
                assert_eq!(model.basis, [0; 4]);
                assert_eq!(model.pending, [false; 4]);
                model.assert_matches_with_deleted_debtor(&world, Some(debtor));
                let expected: [u128; 5] = std::array::from_fn(|actor| match actor {
                    0 | 2 => ATTRIBUTION_DEPOSITS[actor] + model.debt[actor / 2],
                    1 | 3 => ATTRIBUTION_DEPOSITS[actor] - model.debt[actor / 2],
                    _ => ATTRIBUTION_DEPOSITS[actor],
                });
                assert_eq!(expected, [200_001, 179_999, 339_998, 210_002, 777]);
                assert_eq!(world.env.market_state().1.vault, 0);
                assert_eq!(world.env.market_state().1.c_tot, 0);
                for actor in [other_holder, holder, other_debtor, 4] {
                    assert!(resolved_portfolio_is_terminal(
                        &world.env,
                        world.actors[actor].portfolio
                    ));
                    let before_retry = world.frame();
                    let error = world
                        .payout(actor, false)
                        .expect_err("a terminal owner cannot close for a second payout");
                    assert!(is_engine_non_progress_error(&error), "{error}");
                    assert_eq!(world.frame(), before_retry);
                    model.assert_matches_with_deleted_debtor(&world, Some(debtor));
                }
                for actor in [4, other_debtor, holder, other_holder] {
                    let a = &world.actors[actor];
                    let cu = world.env.close_portfolio_with_cu(&a.owner, a.portfolio);
                    assert_cu_within("INV-039 remaining cohort deletion", cu, CUSTODY_CU_LIMIT);
                }
                for (actor, entitlement) in expected.into_iter().enumerate() {
                    assert_eq!(
                        world.env.token_amount(world.actors[actor].token) as u128,
                        entitlement
                    );
                }
                assert_eq!(world.env.market_state().1.materialized_portfolio_count, 0);
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 8);
    println!("INV-039: 8 resolved debtor deletions before global debt settlement; 8 waiting rollbacks; 40 exact owner payouts");
}

#[test]
fn v16_program_settled_pending_cohorts_reach_exact_terminal_slab_close() {
    use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

    let mut peak_cu = 0;
    for reverse_sides in [false, true] {
        for last_pair in 0..2 {
            let history = History {
                reverse_sides,
                lots: [1, 2],
                price_moves: [1, 19_999],
                early_debtor: None,
                close_order: [0, 1, 2, 3, 4],
                extra_closes: Vec::new(),
            };
            let (mut world, mut model) = resolve_history(&history);
            let admin = world.env.admin.insecure_clone();
            let destination = create_ata_for_test(
                &mut world.env.svm,
                &world.env.payer,
                admin.pubkey(),
                world.env.mint,
            );
            send_raw_tx(
                &mut world.env.svm,
                &world.env.payer,
                spl_token::instruction::set_authority(
                    &spl_token::ID,
                    &world.env.mint,
                    None,
                    spl_token::instruction::AuthorityType::MintTokens,
                    &admin.pubkey(),
                    &[],
                )
                .unwrap(),
                &[&admin],
            )
            .unwrap();
            let destination_frame = world.env.svm.get_account(&destination);
            let mint_frame = world.env.svm.get_account(&world.env.mint);
            let close_slab = Instruction {
                program_id: world.env.program_id,
                data: ProgInstruction::CloseSlab {
                    authority_epoch: world.env.control_sequences(0).authority_epoch,
                }
                .encode(),
                accounts: vec![
                    AccountMeta::new(admin.pubkey(), true),
                    AccountMeta::new(world.env.market, false),
                    AccountMeta::new(world.env.vault, false),
                    AccountMeta::new_readonly(world.env.vault_authority, false),
                    AccountMeta::new(destination, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                    AccountMeta::new(world.env.mint, false),
                ],
            };
            let last_debtor = 2 * last_pair + 1;
            model.close(&mut world, 2 * (1 - last_pair) + 1);
            for holder in [0, 2] {
                model.close(&mut world, holder);
            }
            assert_eq!(model.pending, [false; 4]);
            assert_ne!(model.basis[last_debtor], 0);
            for holder in [0, 2] {
                assert_eq!(world.env.token_amount(world.actors[holder].token), 0);
                assert_eq!(
                    world
                        .env
                        .portfolio_state(world.actors[holder].portfolio)
                        .pnl
                        .get(),
                    model.debt[holder / 2] as i128
                );
            }

            let debtor = &world.actors[last_debtor];
            let settle = Instruction {
                program_id: world.env.program_id,
                data: ProgInstruction::CloseResolved {
                    fee_rate_per_slot: 0,
                }
                .encode(),
                accounts: vec![
                    AccountMeta::new_readonly(debtor.owner.pubkey(), false),
                    AccountMeta::new(world.env.market, false),
                    AccountMeta::new(debtor.portfolio, false),
                    AccountMeta::new(debtor.token, false),
                    AccountMeta::new(world.env.vault, false),
                    AccountMeta::new_readonly(world.env.vault_authority, false),
                    AccountMeta::new_readonly(spl_token::ID, false),
                ],
            };
            let mut land = |world: &mut AttributionWorld, instructions: &[Instruction]| {
                world.env.svm.expire_blockhash();
                let tx = Transaction::new_signed_with_payer(
                    instructions,
                    Some(&world.env.payer.pubkey()),
                    &[&world.env.payer, &admin],
                    world.env.svm.latest_blockhash(),
                );
                assert_eq!(tx.message.header.num_required_signatures, 2);
                assert!(tx.verify().is_ok());
                assert!(bincode::serialize(&tx).unwrap().len() <= 1_232);
                let result = world.env.svm.send_transaction(tx);
                let meta = match &result {
                    Ok(meta) => meta,
                    Err(failure) => &failure.meta,
                };
                assert_cu_within(
                    "INV-039 obligation terminal transaction",
                    meta.compute_units_consumed,
                    CUSTODY_CU_LIMIT,
                );
                peak_cu = peak_cu.max(meta.compute_units_consumed);
                result
            };

            // A successful debt settlement and SPL payout precede the retirement rejection.
            let before = world.frame();
            let failure = land(
                &mut world,
                &[heap_ix(), cu_ix(), settle, close_slab.clone()],
            )
            .expect_err("unpaid holder claims still prevent slab retirement");
            assert_eq!(
                failure.err,
                TransactionError::InstructionError(
                    3,
                    InstructionError::Custom(PercolatorError::EngineLockActive as u32)
                )
            );
            for program in [world.env.program_id, spl_token::ID] {
                assert_eq!(
                    failure
                        .meta
                        .logs
                        .iter()
                        .filter(|line| { **line == format!("Program {program} success") })
                        .count(),
                    1,
                    "the rejected bundle must first commit a staged debtor payout"
                );
            }
            assert_eq!(world.frame(), before);
            assert_eq!(world.env.svm.get_account(&destination), destination_frame);
            model.assert_matches(&world);

            // Exactly four more resolved calls pay the debtor, both holders and the bystander.
            for actor in [last_debtor, 2 * last_pair, 2 * (1 - last_pair), 4] {
                model.close(&mut world, actor);
                assert!(resolved_portfolio_is_terminal(
                    &world.env,
                    world.actors[actor].portfolio
                ));
            }
            assert_eq!(model.basis, [0; 4]);
            assert_eq!(model.pending, [false; 4]);
            let expected = [200_001u128, 179_999, 339_998, 210_002, 777];
            for (actor, entitlement) in world.actors.iter().zip(expected) {
                assert_eq!(world.env.token_amount(actor.token) as u128, entitlement);
                assert!(resolved_portfolio_is_terminal(&world.env, actor.portfolio));
            }
            let settled = world.env.market_state().1;
            assert_eq!(
                (settled.vault, settled.c_tot, settled.pnl_pos_tot),
                (0, 0, 0)
            );
            assert_eq!(settled.insurance, 0);
            assert_eq!(settled.materialized_portfolio_count, 5);
            for actor in [
                last_debtor,
                2 * last_pair,
                4,
                2 * (1 - last_pair),
                2 * (1 - last_pair) + 1,
            ] {
                let before = world.frame();
                let count = world.env.market_state().1.materialized_portfolio_count;
                let a = &world.actors[actor];
                let rent = world.env.svm.get_account(&a.portfolio).unwrap().lamports;
                let market_rent = world
                    .env
                    .svm
                    .get_account(&world.env.market)
                    .unwrap()
                    .lamports;
                let cu = world.env.close_portfolio_with_cu(&a.owner, a.portfolio);
                assert_cu_within("INV-039 pre-retirement deletion", cu, CUSTODY_CU_LIMIT);
                assert_eq!(
                    world.env.market_state().1.materialized_portfolio_count,
                    count - 1
                );
                assert_eq!(world.env.market_state().1.assets, settled.assets);
                assert_eq!(
                    world
                        .env
                        .svm
                        .get_account(&world.env.market)
                        .unwrap()
                        .lamports,
                    market_rent + rent
                );
                assert!(world
                    .env
                    .svm
                    .get_account(&a.portfolio)
                    .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
                for (key, account) in before {
                    if ![world.env.market, a.portfolio].contains(&key) {
                        assert_eq!(world.env.svm.get_account(&key), account);
                    }
                }
            }
            let terminal = world.env.market_state().1;
            assert_eq!(terminal.materialized_portfolio_count, 0);
            assert_eq!(
                (terminal.vault, terminal.c_tot, terminal.pnl_pos_tot),
                (0, 0, 0)
            );
            crate::support::fuzz_model::assert_reservation_encumbrance_census(
                "INV-039 obligation-complete retirement",
                &terminal,
                &[],
            )
            .unwrap();
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
            let mut expected_admin = world.env.svm.get_account(&admin.pubkey()).unwrap();
            land(&mut world, &[heap_ix(), cu_ix(), close_slab])
                .expect("settled pending cohorts retire in one bounded slab call");
            let tombstone = world.env.svm.get_account(&world.env.market).unwrap();
            assert_closed_market_tombstone(&tombstone);
            assert_eq!(
                tombstone.lamports,
                world
                    .env
                    .svm
                    .minimum_balance_for_rent_exemption(percolator_prog::constants::HEADER_LEN)
            );
            expected_admin.lamports += market_rent + vault_rent - tombstone.lamports;
            assert_eq!(
                world.env.svm.get_account(&admin.pubkey()),
                Some(expected_admin)
            );
            assert!(world
                .env
                .svm
                .get_account(&world.env.vault)
                .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
            for (key, account) in before {
                if ![world.env.market, world.env.vault, admin.pubkey()].contains(&key) {
                    assert_eq!(world.env.svm.get_account(&key), account);
                }
            }
            assert_eq!(world.env.svm.get_account(&destination), destination_frame);
            assert_eq!(world.env.svm.get_account(&world.env.mint), mint_frame);
            let mint = Mint::unpack(&mint_frame.unwrap().data).unwrap();
            assert_eq!(mint.mint_authority, COption::None);
            assert_eq!(mint.supply as u128, expected.iter().sum::<u128>());
        }
    }
    println!("INV-039 terminal retirement: 4 worlds, 4 staged-payout rollbacks, 20 exact payouts, 20 portfolio deletions, 4 single-call slab closes; peak terminal transaction {peak_cu} CU");
}

#[test]
fn v16_program_pending_resolved_cohorts_preserve_attribution_in_every_close_order() {
    let mut worlds = 0;
    let mut baseline = None;
    for reverse_sides in [false, true] {
        for early_debtor in [None, Some(0), Some(1)] {
            for a in 0..4 {
                for b in 0..4 {
                    for c in 0..4 {
                        for d in 0..4 {
                            let mut order = [a, b, c, d];
                            order.sort_unstable();
                            if order != [0, 1, 2, 3] {
                                continue;
                            }
                            let mut close_order = vec![a, b, c, d];
                            close_order.insert(worlds % 5, 4);
                            let history = History {
                                reverse_sides,
                                lots: [1, 2],
                                price_moves: [1, 19_999],
                                early_debtor,
                                close_order: close_order.try_into().unwrap(),
                                extra_closes: Vec::new(),
                            };
                            let payout = run_history(&history);
                            assert_eq!(*baseline.get_or_insert(payout), payout, "{history:?}");
                            worlds += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(worlds, 144);
    println!("INV-039: {worlds} public histories; 24 cohort close orders x 3 resolution boundaries x 2 sides; input-derived per-owner attribution at every close prefix");
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PERCOLATOR_INV039_HISTORY_CASES")
            .ok().and_then(|value| value.parse().ok()).unwrap_or(16),
        max_shrink_iters: 64,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_039_resolved_histories.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_pending_resolved_history_generator_preserves_owner_debt(
        reverse_sides in any::<bool>(),
        lots in proptest::array::uniform2(1u8..=3),
        price_moves in proptest::array::uniform2(1u16..=20_000),
        early in 0usize..3,
        priority in any::<[u8; 5]>(),
        extra_closes in proptest::collection::vec(0usize..5, 0..17),
    ) {
        let mut close_order = [0, 1, 2, 3, 4];
        close_order.sort_by_key(|actor| (priority[*actor], *actor));
        run_history(&History {
            reverse_sides,
            lots,
            price_moves,
            early_debtor: early.checked_sub(1),
            close_order,
            extra_closes,
        });
    }
}
