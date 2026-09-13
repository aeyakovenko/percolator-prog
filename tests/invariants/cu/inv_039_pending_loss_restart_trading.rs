//! Row 419: fresh peer-generation trading while an old cohort retains unbooked debt.
//! Public restart rejects occupied cohorts; a fresh-generation round trip on the
//! peer preserves original claims through later resolution and debtor settlement.

use super::*;
use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

fn restart_instruction(world: &AttributionWorld, pair: usize) -> Instruction {
    let asset = pair + 1;
    let sequences = world.env.control_sequences(asset);
    Instruction {
        program_id: world.env.program_id,
        accounts: vec![
            AccountMeta::new(world.env.admin.pubkey(), true),
            AccountMeta::new(world.env.market, false),
        ],
        data: ProgInstruction::RestartAssetOracle {
            market_id: world.env.asset_market_id(asset as u16),
            asset_index: asset as u16,
            now_slot: 20,
            initial_price: 700_000,
            observation_sequence: next_control_sequence(sequences.oracle_observation),
            authority_epoch: sequences.authority_epoch,
        }
        .encode(),
    }
}

fn reject_restart(world: &mut AttributionWorld, pair: usize, stage_debtor: bool) -> u64 {
    let before = world.frame();
    let mut instructions = vec![heap_ix(), cu_ix()];
    let debtor = &world.actors[2 * pair + 1];
    let mut signers = vec![&world.env.payer, &world.env.admin];
    if stage_debtor {
        instructions.push(Instruction {
            program_id: world.env.program_id,
            accounts: vec![
                AccountMeta::new(debtor.owner.pubkey(), true),
                AccountMeta::new(world.env.market, false),
                AccountMeta::new(debtor.portfolio, false),
            ],
            data: ProgInstruction::ForfeitRecoveryLeg {
                portfolio_id: world.env.portfolio_id(debtor.portfolio),
                position_epoch: world.env.portfolio_position_epoch(debtor.portfolio),
                asset_index: (pair + 1) as u16,
                b_delta_budget: u128::MAX,
            }
            .encode(),
        });
        signers.push(&debtor.owner);
    }
    instructions.push(restart_instruction(world, pair));
    world.env.svm.expire_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &instructions,
        Some(&world.env.payer.pubkey()),
        &signers,
        world.env.svm.latest_blockhash(),
    );
    assert!(bincode::serialize(&tx).unwrap().len() <= 1232);
    let failure = world
        .env
        .svm
        .send_transaction(tx)
        .expect_err("restart cannot erase a live obligation or claim");
    assert_eq!(
        failure.err,
        TransactionError::InstructionError(
            2 + u8::from(stage_debtor),
            InstructionError::Custom(PercolatorError::EngineLockActive as u32),
        ),
        "the bound, authorized restart must reach the economic emptiness gate"
    );
    assert_eq!(
        world.frame(),
        before,
        "including staged debt, sequences, custody and rent"
    );
    let cu = failure.meta.compute_units_consumed;
    assert_cu_within("INV-039 pending cohort restart", cu, CRANK_CU_LIMIT);
    cu
}

#[test]
fn v16_program_peer_restart_preserves_pending_debt_through_fresh_trade_and_resolution() {
    assert_certified_engine_pin("INV-039 pending cohort restart");
    let mut peak_restart = 0;
    for reverse_sides in [false, true] {
        for first_pair in 0..2 {
            for reverse_payouts in [false, true] {
                let mut world = AttributionWorld::new(reverse_sides);
                let mut model = AttributionModel {
                    debt: [world.debt(0), world.debt(1)],
                    basis: world.quantities,
                    pending: [false; 4],
                };
                assert_eq!(model.debt, [30_000, 40_000]);
                for pair in 0..2 {
                    let holder = &world.actors[2 * pair];
                    let debtor = &world.actors[2 * pair + 1];
                    world.env.trade_asset_with_cu(
                        (pair + 1) as u16,
                        &holder.owner,
                        holder.portfolio,
                        &debtor.owner,
                        debtor.portfolio,
                        world.quantities[2 * pair],
                        1_000_000,
                        0,
                    );
                }
                world.env.svm.warp_to_slot(20);
                for pair in 0..2 {
                    let mark = (1_000_000
                        + ATTRIBUTION_PRICE_MOVES[pair] * world.quantities[2 * pair].signum())
                        as u64;
                    world
                        .env
                        .push_auth_mark_for_asset_as_admin((pair + 1) as u16, 20, mark);
                }
                world.env.crank(
                    world.actors[4].portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 20,
                        observations: crank_observations_for_assets(&[1, 2]),
                    },
                );
                for pair in 0..2 {
                    let debtor = world
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
                        debtor
                    );
                }
                model.assert_matches(&world);
                assert_eq!(model.pending, [true, false, true, false]);

                let holder = 2 * first_pair;
                let debtor = holder + 1;
                let other = 1 - first_pair;
                let asset = (first_pair + 1) as u16;
                let other_accounts = [2 * other, 2 * other + 1]
                    .map(|i| world.env.svm.get_account(&world.actors[i].portfolio));
                let market = world.env.svm.get_account(&world.env.market).unwrap();
                let other_slot = market_engine_slot_bytes(&market.data, other + 1).to_vec();
                peak_restart = peak_restart.max(reject_restart(&mut world, first_pair, false));
                // The suffix reaches restart only after the debtor paid. Its failure must
                // restore that payment as well as the holder's retained attribution.
                peak_restart = peak_restart.max(reject_restart(&mut world, first_pair, true));
                model.assert_matches(&world);
                world.forfeit(debtor);
                model.basis[debtor] = 0;
                model.assert_matches(&world);
                assert_eq!(
                    world
                        .env
                        .portfolio_state(world.actors[debtor].portfolio)
                        .capital
                        .get(),
                    ATTRIBUTION_DEPOSITS[debtor] - model.debt[first_pair]
                );
                peak_restart = peak_restart.max(reject_restart(&mut world, first_pair, false));
                let cu = world.env.crank(
                    world.actors[holder].portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 20,
                        observations: crank_observations(asset),
                    },
                );
                assert_cu_within("INV-039 pre-restart pending release", cu, CRANK_CU_LIMIT);
                model.pending[holder] = false;
                model.assert_matches(&world);
                peak_restart = peak_restart.max(reject_restart(&mut world, first_pair, false));

                let old_generation = world.env.asset_market_id(0);
                let before_peer_restart = world.frame();
                let market = world.env.svm.get_account(&world.env.market).unwrap();
                let old_cohorts =
                    [1, 2].map(|i| market_engine_slot_bytes(&market.data, i).to_vec());
                world.env.update_asset_lifecycle_as_admin_with_cu(
                    processor::ASSET_ACTION_SHUTDOWN,
                    0,
                    20,
                    0,
                );
                let admin = world.env.admin.insecure_clone();
                let cu = world
                    .env
                    .try_restart_asset_oracle_with_authority(&admin, 0, 20, 700_000)
                    .expect("empty peer restarts despite an unrelated pending obligation");
                assert_cu_within("INV-039 empty peer restart", cu, CRANK_CU_LIMIT);
                peak_restart = peak_restart.max(cu);
                for (key, account) in before_peer_restart {
                    if key != world.env.market {
                        assert_eq!(world.env.svm.get_account(&key), account);
                    }
                }
                let restarted = world.env.market_state().1;
                assert_eq!(restarted.mode, MarketModeV16::Live);
                assert_eq!(restarted.assets[0].lifecycle, AssetLifecycleV16::Active);
                assert_eq!(restarted.assets[0].effective_price, 700_000);
                assert!(restarted.assets[0].market_id > old_generation);
                let market = world.env.svm.get_account(&world.env.market).unwrap();
                for (i, cohort) in [1, 2].into_iter().zip(&old_cohorts) {
                    assert_eq!(market_engine_slot_bytes(&market.data, i), cohort);
                }
                model.assert_matches(&world);

                // These owners retain the first cohort's settled debt and historical
                // positive claim while trading a different, newly restarted generation.
                let historical_sources = [holder, debtor].map(|i| {
                    world
                        .env
                        .portfolio_state(world.actors[i].portfolio)
                        .source_domains
                });
                for opening in [true, false] {
                    let before_trade = world.frame();
                    let a = &world.actors[holder];
                    let b = &world.actors[debtor];
                    let q = world.quantities[holder] * if opening { 1 } else { -1 };
                    let cu = world.env.trade_asset_with_cu(
                        0,
                        &a.owner,
                        a.portfolio,
                        &b.owner,
                        b.portfolio,
                        q,
                        700_000,
                        0,
                    );
                    assert_cu_within("INV-039 restarted cohort round trip", cu, TRADE_CU_LIMIT);
                    for (key, account) in before_trade {
                        if ![world.env.market, a.portfolio, b.portfolio].contains(&key) {
                            assert_eq!(world.env.svm.get_account(&key), account);
                        }
                    }
                    let market = world.env.svm.get_account(&world.env.market).unwrap();
                    for (i, cohort) in [1, 2].into_iter().zip(&old_cohorts) {
                        assert_eq!(market_engine_slot_bytes(&market.data, i), cohort);
                    }
                    let new_asset = world.env.market_state().1.assets[0];
                    let oi = if opening { q.unsigned_abs() } else { 0 };
                    assert_eq!([new_asset.oi_eff_long_q, new_asset.oi_eff_short_q], [oi; 2]);
                    assert_eq!(
                        [
                            new_asset.loss_weight_sum_long,
                            new_asset.loss_weight_sum_short
                        ],
                        [oi; 2]
                    );
                    assert_eq!(
                        [
                            new_asset.stored_pos_count_long,
                            new_asset.stored_pos_count_short
                        ],
                        [u64::from(opening); 2]
                    );
                    assert_eq!(
                        [
                            new_asset.pending_obligation_count_long,
                            new_asset.pending_obligation_count_short
                        ],
                        [0; 2]
                    );
                    for (i, source) in [holder, debtor].into_iter().zip(historical_sources) {
                        let account = world.env.portfolio_state(world.actors[i].portfolio);
                        assert_eq!(account.source_domains, source);
                        assert_eq!(
                            account.pnl.get(),
                            if i == holder {
                                model.debt[first_pair] as i128
                            } else {
                                0
                            }
                        );
                        assert_eq!(
                            account.capital.get(),
                            if i == holder {
                                ATTRIBUTION_DEPOSITS[i]
                            } else {
                                ATTRIBUTION_DEPOSITS[i] - model.debt[first_pair]
                            }
                        );
                        if opening {
                            assert_eq!(
                                active_leg_for_asset(&account, 0).market_id,
                                restarted.assets[0].market_id
                            );
                            assert_eq!(
                                active_leg_for_asset(&account, 0).basis_pos_q,
                                if i == holder { q } else { -q }
                            );
                        }
                    }
                }
                model.assert_matches(&world);
                assert_eq!(
                    [2 * other, 2 * other + 1]
                        .map(|i| world.env.svm.get_account(&world.actors[i].portfolio)),
                    other_accounts
                );
                assert_eq!(
                    market_engine_slot_bytes(
                        &world.env.svm.get_account(&world.env.market).unwrap().data,
                        other + 1
                    ),
                    other_slot
                );
                peak_restart = peak_restart.max(reject_restart(&mut world, other, false));

                let before_resolve = world.frame();
                world.env.resolve();
                for (key, account) in before_resolve {
                    if key != world.env.market {
                        assert_eq!(world.env.svm.get_account(&key), account);
                    }
                }
                world.env.svm.warp_to_slot(25);
                model.assert_matches(&world);
                model.close(&mut world, 2 * other);
                assert!(!model.pending[2 * other]);
                assert_ne!(model.basis[2 * other + 1], 0);
                let before_wait = world.frame();
                let error = world
                    .payout(2 * other, false)
                    .expect_err("restart did not settle the other debtor");
                assert!(is_engine_non_progress_error(&error), "{error}");
                assert_eq!(world.frame(), before_wait);
                model.assert_matches(&world);

                let order = if reverse_payouts {
                    [4, 3, 2, 1, 0]
                } else {
                    [0, 1, 2, 3, 4]
                };
                for _ in 0..4 {
                    for actor in order {
                        if !resolved_portfolio_is_terminal(
                            &world.env,
                            world.actors[actor].portfolio,
                        ) {
                            model.close(&mut world, actor);
                        }
                    }
                }
                model.assert_matches(&world);
                assert_eq!(model.basis, [0; 4]);
                assert_eq!(model.pending, [false; 4]);
                assert_eq!(world.env.market_state().1.vault, 0);
                for actor in order {
                    let expected = match actor {
                        0 | 2 => ATTRIBUTION_DEPOSITS[actor] + model.debt[actor / 2],
                        1 | 3 => ATTRIBUTION_DEPOSITS[actor] - model.debt[actor / 2],
                        _ => ATTRIBUTION_DEPOSITS[actor],
                    };
                    assert_eq!(
                        world.env.token_amount(world.actors[actor].token) as u128,
                        expected
                    );
                    assert!(resolved_portfolio_is_terminal(
                        &world.env,
                        world.actors[actor].portfolio
                    ));
                    let before = world.frame();
                    world.payout(actor, true).expect("settled topup retry");
                    assert_eq!(world.frame(), before);
                }
                for actor in order {
                    let a = &world.actors[actor];
                    let cu = world.env.close_portfolio_with_cu(&a.owner, a.portfolio);
                    assert_cu_within(
                        "INV-039 restarted history portfolio deletion",
                        cu,
                        CUSTODY_CU_LIMIT,
                    );
                }
                assert_eq!(world.env.market_state().1.materialized_portfolio_count, 0);
            }
        }
    }
    println!("INV-039 restart: 8 worlds, 40 restart rollbacks (8 staged debt payments), 8 restarts, 16 fresh trades, 8 waiting rollbacks, 40 exact payouts/retries/deletions; peak restart transaction {peak_restart} CU");
}
