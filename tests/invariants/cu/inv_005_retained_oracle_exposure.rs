//! Row 416: empty-asset oracle consent must be rechecked after public exposure.
//! Rejection restores an SPL payout prefix; incumbent succession still permits
//! authenticated settlement and exact principal exits for every fixture owner.
//! Oracle A -> B -> A also cannot revive a retained mark over two funded assets.

use super::*;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction, fee::FeeStructure, instruction::InstructionError,
    transaction::TransactionError,
};

#[path = "inv_005_detached_live_oracle.rs"]
mod detached_live_oracle;

#[test]
fn v16_program_retained_empty_oracle_handoff_rechecks_exposure_before_payout() {
    const COLD: usize = 0;
    const COUNTERPARTY: usize = 1;
    const UNCONSENTED: usize = 2;
    const SUCCESSOR: usize = 3;
    const PREFIX_OWNER: usize = 4;
    const PREFIX: u128 = 7;
    let mut peak_cu = 0;
    for asset in [0u16, 1] {
        for direction in [-1i128, 1] {
            let config = MarketConfig::default();
            let deposits = config.actor_deposits;
            let mut env = V16Svm::new([0x46; 32], config);
            env.begin_public_trace();
            env.configure_permissionless_resolve(2, 1).unwrap();
            env.update_asset_authority_from_admin(asset, processor::ASSET_AUTH_ADMIN, COLD)
                .unwrap();
            let honest = env.primary_profile(asset as usize).oracle_authority;
            assert_ne!(honest, env.actors[COLD].signer.pubkey().to_bytes());
            let epoch = env
                .primary_control_sequences(asset as usize)
                .authority_epoch;
            let market_id = env.primary_market_state().1.assets[asset as usize].market_id;
            let handoff = env.build_retained_asset_authority_handoff_between_actors(
                asset,
                processor::ASSET_AUTH_ORACLE,
                COLD,
                UNCONSENTED,
            );
            let withdrawal = env.build_retained_withdrawal(PREFIX_OWNER, PREFIX);
            let bundle = env.bundle_retained_transactions(&[withdrawal.clone(), handoff.clone()]);
            env.svm
                .simulate_transaction(bundle.clone().into())
                .expect("the original signatures and payout/handoff are admissible while empty");

            env.trade_no_cpi(
                COLD,
                COUNTERPARTY,
                asset,
                direction * 10 * percolator::POS_SCALE as i128,
                INITIAL_PRICE,
                0,
            )
            .unwrap();
            let exposed = env.primary_market_state().1.assets[asset as usize];
            assert!(exposed.oi_eff_long_q > 0 && exposed.oi_eff_short_q > 0);
            assert_eq!(exposed.market_id, market_id);
            assert_eq!(
                env.primary_control_sequences(asset as usize)
                    .authority_epoch,
                epoch
            );
            let peer = (1 - asset) as usize;
            let peer_before = env.primary_profile(peer);
            let peer_sequences = env.primary_control_sequences(peer);

            for (tx, failed_instruction) in [(bundle, 4), (handoff, 3)] {
                let expected = TransactionError::InstructionError(
                    failed_instruction,
                    InstructionError::Custom(PercolatorError::EngineLockActive as u32),
                );
                let frame: Vec<_> = tx
                    .message
                    .account_keys
                    .iter()
                    .map(|key| (*key, env.svm.get_account(key)))
                    .collect();
                let payer = tx.message.account_keys[0];
                // V16Svm varies CU price to give retained requests distinct signatures.
                let budget = |index: usize| {
                    let ix = &tx.message.instructions[index];
                    assert_eq!(
                        tx.message.account_keys[ix.program_id_index as usize],
                        solana_sdk::compute_budget::id()
                    );
                    solana_program::borsh1::try_from_slice_unchecked::<ComputeBudgetInstruction>(
                        &ix.data,
                    )
                    .unwrap()
                };
                let ComputeBudgetInstruction::SetComputeUnitLimit(limit) = budget(1) else {
                    panic!("CU limit")
                };
                let ComputeBudgetInstruction::SetComputeUnitPrice(price) = budget(2) else {
                    panic!("CU price")
                };
                let priority_fee =
                    (u128::from(limit) * u128::from(price)).div_ceil(1_000_000) as u64;
                let fee = u64::from(tx.message.header.num_required_signatures)
                    * FeeStructure::default().lamports_per_signature
                    + priority_fee;
                let error = env
                    .land_retained(tx)
                    .expect_err("cold consent cannot follow exposure");
                assert!(
                    error.contains(&format!("{expected:?}")),
                    "funded state, not stale consent, must reject: {error}"
                );
                for (key, mut before) in frame {
                    if key == payer {
                        before.as_mut().unwrap().lamports -= fee;
                    }
                    assert_eq!(env.svm.get_account(&key), before, "rollback account {key}");
                }
                assert_eq!(env.primary_profile(asset as usize).oracle_authority, honest);
            }

            env.land_retained(withdrawal)
                .expect("the identical SPL prefix remains usable after rollback");
            assert_eq!(
                env.token_amount(env.actors[PREFIX_OWNER].destination_token),
                PREFIX as u64
            );
            env.update_asset_authority_from_admin(asset, processor::ASSET_AUTH_ORACLE, SUCCESSOR)
                .expect("incumbent oracle consent remains live over exposure");
            assert_eq!(
                env.primary_control_sequences(asset as usize)
                    .authority_epoch,
                epoch + 1
            );
            assert_eq!(env.primary_profile(peer), peer_before);
            assert_eq!(env.primary_control_sequences(peer), peer_sequences);

            env.warp_to_slot(2);
            let market_before = env.svm.get_account(&env.market);
            let unauthorized = env
                .push_auth_mark_for_actor(UNCONSENTED, asset, 2, INITIAL_PRICE + INITIAL_PRICE / 10)
                .unwrap_err();
            assert!(
                unauthorized.contains(&format!("Custom({})", PercolatorError::Unauthorized as u32))
            );
            assert_eq!(env.svm.get_account(&env.market), market_before);
            env.push_auth_mark_for_actor(SUCCESSOR, asset, 2, INITIAL_PRICE)
                .expect("consensual successor can publish the settlement observation");
            assert_eq!(env.primary_profile(asset as usize).last_good_oracle_slot, 2);
            assert_eq!(
                env.primary_market_state().1.assets[asset as usize].effective_price,
                INITIAL_PRICE
            );
            env.resolve_stale_permissionless(4).unwrap();
            let order = if direction < 0 {
                [COLD, COUNTERPARTY, 2, 3, 4]
            } else {
                [COUNTERPARTY, COLD, 2, 3, 4]
            };
            for owner in order {
                env.close_resolved_primary_signed(owner)
                    .expect("rejected takeover must not strand user principal");
                assert_eq!(
                    u128::from(env.token_amount(env.actors[owner].destination_token)),
                    deposits[owner]
                );
            }
            let settled = env.primary_market_state().1;
            assert_eq!(settled.c_tot, 0);
            assert_eq!(settled.pnl_pos_tot, 0);
            assert_eq!(settled.vault, 0);
            assert_eq!(env.token_amount(env.vault), 0);
            assert_eq!(env.mint_supply() as u128, env.initial_token_supply);
            assert_eq!(env.token_supply_observed(), env.initial_token_supply);
            let trace = env.finish_public_trace();
            trace.validate_public_execution().unwrap();
            assert_eq!(trace.steps.iter().filter(|step| !step.succeeded).count(), 3);
            peak_cu = peak_cu.max(
                trace
                    .steps
                    .iter()
                    .filter_map(|step| step.compute_units)
                    .max()
                    .unwrap(),
            );
        }
    }
    eprintln!("row416 retained oracle exposure: worlds=4, funded_rejections=8, unauthorized_marks=4, prefix_retries=4, exact_owner_payouts=20, peak_success_cu={peak_cu}");
}

fn reject(
    env: &mut V16Svm,
    tx: Transaction,
    index: u8,
    reason: PercolatorError,
    wrapper_prefixes: usize,
    spl_prefixes: usize,
) {
    tx.verify().unwrap();
    assert!(bincode::serialized_size(&tx).unwrap() <= 1_232);
    let payer = tx.message.account_keys[0];
    let budget = |index: usize| {
        let ix = &tx.message.instructions[index];
        assert_eq!(
            tx.message.account_keys[ix.program_id_index as usize],
            solana_sdk::compute_budget::id()
        );
        solana_program::borsh1::try_from_slice_unchecked::<ComputeBudgetInstruction>(&ix.data)
            .unwrap()
    };
    let ComputeBudgetInstruction::SetComputeUnitLimit(limit) = budget(1) else {
        panic!("CU limit")
    };
    let ComputeBudgetInstruction::SetComputeUnitPrice(price) = budget(2) else {
        panic!("CU price")
    };
    let fee = u64::from(tx.message.header.num_required_signatures)
        * FeeStructure::default().lamports_per_signature
        + (u128::from(limit) * u128::from(price)).div_ceil(1_000_000) as u64;
    let mut keys = tx.message.account_keys.clone();
    keys.extend([env.market, env.foreign_market, env.mint, env.vault]);
    for actor in &env.actors {
        keys.extend([actor.portfolio, actor.source_token, actor.destination_token]);
    }
    keys.sort_unstable();
    keys.dedup();
    let frame: Vec<_> = keys
        .into_iter()
        .map(|key| (key, env.svm.get_account(&key)))
        .collect();
    let error = env.land_retained(tx).expect_err("atomic rejection");
    let expected =
        TransactionError::InstructionError(index, InstructionError::Custom(reason as u32));
    assert!(error.contains(&format!("{expected:?}")), "{error}");
    for (program, count) in [
        (env.program_id, wrapper_prefixes),
        (spl_token::ID, spl_prefixes),
    ] {
        assert_eq!(
            error.matches(&format!("Program {program} success")).count(),
            count,
            "executed prefixes: {error}"
        );
    }
    for (key, mut before) in frame {
        if key == payer {
            before.as_mut().unwrap().lamports -= fee;
        }
        assert_eq!(env.svm.get_account(&key), before, "rollback account {key}");
    }
}

#[test]
fn v16_program_funded_oracle_return_rejects_retained_mark_and_preserves_value() {
    const ORACLE_A: usize = 2;
    const ORACLE_B: usize = 3;
    const PREFIX_OWNER: usize = 4;
    const PREFIX: u128 = 7;
    const LOTS: i128 = 10;
    const DELTA: u64 = INITIAL_PRICE / 10;
    let mut peak_cu = 0;
    let mut peak_preview_cu = 0;
    for asset in [0u16, 1] {
        for direction in [-1i128, 1] {
            let config = MarketConfig::default();
            let deposits = config.actor_deposits;
            let mut env = V16Svm::new([0x49; 32], config);
            env.begin_public_trace();
            env.configure_permissionless_resolve(2, 1).unwrap();
            for scope in [0u16, 1] {
                env.update_asset_authority_from_admin(
                    scope,
                    processor::ASSET_AUTH_ORACLE,
                    ORACLE_A,
                )
                .unwrap();
            }
            let peer = 1 - asset;
            env.trade_no_cpi(
                0,
                1,
                asset,
                direction * LOTS * percolator::POS_SCALE as i128,
                INITIAL_PRICE,
                0,
            )
            .unwrap();
            env.trade_no_cpi(
                ORACLE_A,
                ORACLE_B,
                peer,
                -direction * 6 * percolator::POS_SCALE as i128,
                INITIAL_PRICE,
                0,
            )
            .unwrap();
            env.warp_to_slot(2);
            let economy = env.primary_market_state();
            let profiles = [0, 1].map(|i| env.primary_profile(i));
            let sequences = [0, 1].map(|i| env.primary_control_sequences(i));
            let portfolios: Vec<_> = (0..5).map(|i| env.primary_portfolio_data(i)).collect();
            for scope in [0, 1] {
                assert!(economy.1.assets[scope].oi_eff_long_q > 0);
                assert!(economy.1.assets[scope].oi_eff_short_q > 0);
                assert_ne!(
                    profiles[scope].asset_admin,
                    profiles[scope].oracle_authority
                );
            }
            let epoch = sequences[asset as usize].authority_epoch;
            let mark = (INITIAL_PRICE as i128 + direction * DELTA as i128) as u64;
            let mark_instruction =
                |scope: u16, authority_epoch, mark_e6| ProgInstruction::PushAuthMark {
                    asset_index: scope,
                    market_id: economy.1.assets[scope as usize].market_id,
                    now_slot: 2,
                    mark_e6,
                    observation_sequence: sequences[scope as usize].oracle_observation + 1,
                    authority_epoch,
                };
            let retained = env.build_retained_market_control_for_actor(
                ORACLE_A,
                mark_instruction(asset, epoch, mark),
            );
            let sibling = env.build_retained_market_control_for_actor(
                ORACLE_A,
                mark_instruction(
                    peer,
                    sequences[peer as usize].authority_epoch,
                    INITIAL_PRICE,
                ),
            );
            let withdrawal = env.build_retained_withdrawal(PREFIX_OWNER, PREFIX);
            let bundle = env.bundle_retained_transactions(&[
                withdrawal.clone(),
                sibling.clone(),
                retained.clone(),
            ]);
            bundle.verify().unwrap();
            assert!(bincode::serialized_size(&bundle).unwrap() <= 1_232);
            let preview = env
                .svm
                .simulate_transaction(bundle.clone().into())
                .expect("the original mark, funded sibling observation and SPL payout are valid");
            peak_preview_cu = peak_preview_cu.max(preview.compute_units_consumed);
            assert_eq!(env.primary_market_state(), economy);

            for (step, from, to) in [(1, ORACLE_A, ORACLE_B), (2, ORACLE_B, ORACLE_A)] {
                env.update_asset_authority_between_actors(
                    asset,
                    processor::ASSET_AUTH_ORACLE,
                    from,
                    to,
                )
                .expect("the incumbent can transfer its oracle role over live exposure");
                let mut expected_profile = profiles[asset as usize];
                expected_profile.oracle_authority = env.actors[to].signer.pubkey().to_bytes();
                let mut expected_sequences = sequences[asset as usize];
                expected_sequences.authority_epoch += step;
                assert_eq!(env.primary_profile(asset as usize), expected_profile);
                assert_eq!(
                    env.primary_control_sequences(asset as usize),
                    expected_sequences
                );
                assert_eq!(env.primary_profile(peer as usize), profiles[peer as usize]);
                assert_eq!(
                    env.primary_control_sequences(peer as usize),
                    sequences[peer as usize]
                );
                assert_eq!(env.primary_market_state(), economy);
                assert_eq!(
                    (0..5)
                        .map(|i| env.primary_portfolio_data(i))
                        .collect::<Vec<_>>(),
                    portfolios
                );
            }
            assert_eq!(
                env.primary_profile(asset as usize),
                profiles[asset as usize]
            );
            reject(&mut env, bundle, 5, PercolatorError::EngineStale, 2, 1);
            reject(&mut env, retained, 3, PercolatorError::EngineStale, 0, 0);
            let revoked = env.build_retained_market_control_for_actor(
                ORACLE_B,
                mark_instruction(asset, epoch + 2, mark),
            );
            reject(&mut env, revoked, 3, PercolatorError::Unauthorized, 0, 0);

            env.land_retained(withdrawal)
                .expect("the original SPL prefix survives the stale-mark rollback");
            env.land_retained(sibling)
                .expect("the original funded sibling observation survives the selected ABA");
            let renewed = env.build_retained_market_control_for_actor(
                ORACLE_A,
                mark_instruction(asset, epoch + 2, mark),
            );
            env.land_retained(renewed)
                .expect("changing only the instruction's authority epoch renews consent");
            for scope in [0, 1] {
                let mut expected_profile = profiles[scope];
                expected_profile.last_good_oracle_slot = 2;
                expected_profile.mark_ewma_e6 = if scope == asset as usize {
                    mark
                } else {
                    INITIAL_PRICE
                };
                if scope == asset as usize {
                    expected_profile.mark_ewma_last_slot = 2;
                    expected_profile.oracle_target_price_e6 = mark;
                    expected_profile.funding_mark_pending_e6 = mark;
                    expected_profile.funding_mark_pending_slot = 2;
                }
                let mut expected_sequences = sequences[scope];
                expected_sequences.oracle_observation += 1;
                expected_sequences.authority_epoch += if scope == asset as usize { 2 } else { 0 };
                assert_eq!(env.primary_profile(scope), expected_profile);
                assert_eq!(env.primary_control_sequences(scope), expected_sequences);
            }

            let profit = (LOTS * DELTA as i128) as u128;
            for owner in [0, 1] {
                env.crank(owner, 2, crank_observations(asset)).unwrap();
            }
            assert_eq!(
                env.primary_market_state().1.assets[asset as usize].effective_price,
                mark
            );
            assert_eq!(
                env.primary_market_state().1.assets[peer as usize],
                economy.1.assets[peer as usize]
            );
            assert_eq!(env.primary_portfolio(0).pnl.get(), profit as i128);
            assert_eq!(env.primary_portfolio(1).capital.get(), deposits[1] - profit);
            assert_eq!(env.primary_portfolio(1).pnl.get(), 0);
            for owner in [ORACLE_A, ORACLE_B] {
                assert_eq!(env.primary_portfolio_data(owner), portfolios[owner]);
            }

            env.resolve_stale_permissionless(4).unwrap();
            let expected = [
                deposits[0] + profit,
                deposits[1] - profit,
                deposits[2],
                deposits[3],
                deposits[4],
            ];
            let mut remaining = deposits.iter().sum::<u128>() - PREFIX;
            // Clear both assets' stored positions before the positive claim is payable.
            for owner in [1, ORACLE_B, ORACLE_A, PREFIX_OWNER, 0] {
                // A nonzero source claim needs bounded terminal normalization before payout.
                for _ in 0..16 {
                    if u128::from(env.token_amount(env.actors[owner].destination_token))
                        == expected[owner]
                    {
                        break;
                    }
                    let before = (env.market_data(false), env.primary_portfolio_data(owner));
                    env.close_resolved_primary_signed(owner).unwrap();
                    assert_ne!(
                        (env.market_data(false), env.primary_portfolio_data(owner)),
                        before,
                        "every accepted terminal step must make public progress"
                    );
                }
                remaining -= expected[owner] - if owner == PREFIX_OWNER { PREFIX } else { 0 };
                assert_eq!(
                    u128::from(env.token_amount(env.actors[owner].destination_token)),
                    expected[owner]
                );
                assert_eq!(env.primary_market_state().1.vault, remaining);
                assert_eq!(u128::from(env.token_amount(env.vault)), remaining);
                assert_eq!(env.token_supply_observed(), env.initial_token_supply);
            }
            let settled = env.primary_market_state().1;
            assert_eq!(
                (settled.c_tot, settled.pnl_pos_tot, settled.vault),
                (0, 0, 0)
            );
            for scope in [0, 1] {
                assert_eq!(
                    (
                        settled.assets[scope].oi_eff_long_q,
                        settled.assets[scope].oi_eff_short_q
                    ),
                    (0, 0)
                );
            }
            assert_eq!(remaining, 0);
            assert_eq!(env.mint_supply() as u128, env.initial_token_supply);
            let trace = env.finish_public_trace();
            trace.validate_public_execution().unwrap();
            assert_eq!(trace.steps.iter().filter(|step| !step.succeeded).count(), 3);
            peak_cu = peak_cu.max(
                trace
                    .steps
                    .iter()
                    .filter_map(|step| step.compute_units)
                    .max()
                    .unwrap(),
            );
        }
    }
    eprintln!("row416 funded oracle return: worlds=4, stale_rejections=8, revoked_marks=4, SPL_rollbacks=4, sibling_retries=4, exact_owner_payouts=20, peak_success_cu={peak_cu}, peak_preview_cu={peak_preview_cu}");
}

#[test]
fn v16_program_retained_oracle_handoff_after_cold_admin_return_preserves_drain_exit() {
    const COLD: usize = 2;
    const INTERIM: usize = 4;
    const SUCCESSOR: usize = 3;
    const PREFIX: u128 = 7;
    let mut peak_cu = 0;
    for asset in [0u16, 1] {
        for direction in [-1i128, 1] {
            let config = MarketConfig::default();
            let deposits = config.actor_deposits;
            let mut env = V16Svm::new([0x47; 32], config);
            env.begin_public_trace();
            env.update_asset_authority_from_admin(asset, processor::ASSET_AUTH_ADMIN, COLD)
                .unwrap();
            let size = direction * 10 * percolator::POS_SCALE as i128;
            env.trade_no_cpi(0, 1, asset, size, INITIAL_PRICE, 0)
                .unwrap();
            let epoch = env
                .primary_control_sequences(asset as usize)
                .authority_epoch;
            let original_profile = env.primary_profile(asset as usize);
            let original_economy = env.primary_market_state();
            let peer = (1 - asset) as usize;
            let peer_profile = env.primary_profile(peer);
            let peer_sequences = env.primary_control_sequences(peer);
            assert!(original_economy.1.assets[asset as usize].oi_eff_long_q > 0);
            assert!(original_economy.1.assets[asset as usize].oi_eff_short_q > 0);

            let withdrawal = env.build_retained_withdrawal(INTERIM, PREFIX);
            let retained = env.build_retained_asset_authority_handoff_from_admin(
                asset,
                processor::ASSET_AUTH_ORACLE,
                SUCCESSOR,
            );
            let stale_bundle = env.bundle_retained_transactions(&[withdrawal.clone(), retained]);
            env.svm
                .simulate_transaction(stale_bundle.clone().into())
                .expect("incumbent consent and SPL prefix are valid over live exposure");

            for (from, to) in [(COLD, INTERIM), (INTERIM, COLD)] {
                env.update_asset_authority_between_actors(
                    asset,
                    processor::ASSET_AUTH_ADMIN,
                    from,
                    to,
                )
                .unwrap();
                assert_eq!(env.primary_market_state(), original_economy);
            }
            assert_eq!(env.primary_profile(asset as usize), original_profile);
            assert_eq!(
                env.primary_control_sequences(asset as usize)
                    .authority_epoch,
                epoch + 2
            );
            reject(
                &mut env,
                stale_bundle,
                4,
                PercolatorError::EngineStale,
                1,
                1,
            );

            let current = env.build_retained_asset_authority_handoff_from_admin(
                asset,
                processor::ASSET_AUTH_ORACLE,
                SUCCESSOR,
            );
            let increase = env.build_retained_no_cpi_trade(0, 1, asset, size, INITIAL_PRICE);
            let risk_bundle = env.bundle_retained_transactions(&[current.clone(), increase]);
            risk_bundle.verify().unwrap();
            assert!(bincode::serialized_size(&risk_bundle).unwrap() <= 1_232);
            env.svm
                .simulate_transaction(risk_bundle.clone().into())
                .expect("renewed oracle consent plus the same risk increase is valid in Active");
            let drain = env.build_retained_drain_only_asset(asset);
            env.land_retained(drain).unwrap();
            assert_eq!(
                env.primary_market_state().1.assets[asset as usize].lifecycle,
                AssetLifecycleV16::DrainOnly
            );
            assert_eq!(
                env.primary_control_sequences(asset as usize)
                    .authority_epoch,
                epoch + 2,
                "lifecycle admission, not epoch staleness, must reject the trade suffix"
            );
            let takeover = env.build_retained_asset_authority_handoff_between_actors(
                asset,
                processor::ASSET_AUTH_ORACLE,
                COLD,
                SUCCESSOR,
            );
            let takeover_bundle = env.bundle_retained_transactions(&[withdrawal.clone(), takeover]);
            reject(
                &mut env,
                takeover_bundle,
                4,
                PercolatorError::EngineLockActive,
                1,
                1,
            );
            reject(
                &mut env,
                risk_bundle,
                4,
                PercolatorError::EngineLockActive,
                1,
                0,
            );
            assert_eq!(env.primary_profile(asset as usize), original_profile);

            let before_handoff = env.primary_market_state();
            env.land_retained(current)
                .expect("the identical incumbent handoff remains usable after suffix rollback");
            assert_eq!(env.primary_market_state(), before_handoff);
            let mut expected_profile = original_profile;
            expected_profile.oracle_authority = env.actors[SUCCESSOR].signer.pubkey().to_bytes();
            assert_eq!(env.primary_profile(asset as usize), expected_profile);
            assert_eq!(
                env.primary_control_sequences(asset as usize)
                    .authority_epoch,
                epoch + 3
            );
            env.land_retained(withdrawal)
                .expect("the identical rolled-back SPL prefix remains usable");

            env.warp_to_slot(2);
            let old_mark = env.build_retained_auth_mark(asset, INITIAL_PRICE);
            reject(&mut env, old_mark, 3, PercolatorError::Unauthorized, 0, 0);
            let observation = env
                .primary_control_sequences(asset as usize)
                .oracle_observation;
            env.push_auth_mark_for_actor(SUCCESSOR, asset, u64::MAX, INITIAL_PRICE)
                .expect("consensual oracle succession preserves fresh observations in DrainOnly");
            assert_eq!(env.primary_profile(asset as usize).last_good_oracle_slot, 2);
            assert_eq!(
                env.primary_control_sequences(asset as usize)
                    .oracle_observation,
                observation + 1
            );
            assert_eq!(
                env.primary_market_state().1.assets[asset as usize].effective_price,
                INITIAL_PRICE
            );
            env.trade_no_cpi(0, 1, asset, -size, INITIAL_PRICE, 0)
                .expect("both independently funded owners retain a bounded live reduction");
            let flat = env.primary_market_state().1;
            assert_eq!(flat.mode, MarketModeV16::Live);
            assert_eq!(flat.assets[asset as usize].oi_eff_long_q, 0);
            assert_eq!(flat.assets[asset as usize].oi_eff_short_q, 0);
            let order = if direction < 0 {
                [0, 1, 2, 3, 4]
            } else {
                [1, 0, 2, 3, 4]
            };
            let mut remaining = deposits.iter().sum::<u128>() - PREFIX;
            for owner in order {
                let owed = deposits[owner] - if owner == INTERIM { PREFIX } else { 0 };
                env.withdraw_primary(owner, owed)
                    .expect("exact live principal exit");
                remaining -= owed;
                assert_eq!(
                    u128::from(env.token_amount(env.actors[owner].destination_token)),
                    deposits[owner]
                );
                let group = env.primary_market_state().1;
                assert_eq!(group.c_tot, remaining);
                assert_eq!(group.pnl_pos_tot, 0);
                assert_eq!(group.vault, remaining);
                assert_eq!(u128::from(env.token_amount(env.vault)), remaining);
                assert_eq!(env.token_supply_observed(), env.initial_token_supply);
            }
            assert_eq!(remaining, 0);
            assert_eq!(env.primary_profile(peer), peer_profile);
            assert_eq!(env.primary_control_sequences(peer), peer_sequences);
            assert_eq!(env.mint_supply() as u128, env.initial_token_supply);
            let trace = env.finish_public_trace();
            trace.validate_public_execution().unwrap();
            assert_eq!(trace.steps.iter().filter(|step| !step.succeeded).count(), 4);
            peak_cu = peak_cu.max(
                trace
                    .steps
                    .iter()
                    .filter_map(|step| step.compute_units)
                    .max()
                    .unwrap(),
            );
        }
    }
    eprintln!("row416 cold-admin return/drain exit: worlds=4, exact_rejections=16, handoff_rollbacks=4, SPL_rollbacks=8, live_reductions=4, exact_owner_exits=20, peak_success_cu={peak_cu}");
}

#[test]
fn v16_program_cold_oracle_handoff_waits_for_last_resolved_exposure() {
    fn close(env: &V16Svm, owner: usize) -> Transaction {
        let actor = &env.actors[owner];
        Transaction::new_signed_with_payer(
            &[
                ComputeBudgetInstruction::request_heap_frame(256 * 1024),
                ComputeBudgetInstruction::set_compute_unit_limit(1_400_000),
                ComputeBudgetInstruction::set_compute_unit_price(0),
                Instruction {
                    program_id: env.program_id,
                    accounts: vec![
                        AccountMeta::new_readonly(actor.signer.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(actor.portfolio, false),
                        AccountMeta::new(actor.destination_token, false),
                        AccountMeta::new(env.vault, false),
                        AccountMeta::new_readonly(env.vault_authority, false),
                        AccountMeta::new_readonly(spl_token::ID, false),
                    ],
                    data: ProgInstruction::CloseResolved {
                        fee_rate_per_slot: 0,
                    }
                    .encode(),
                },
            ],
            Some(&actor.signer.pubkey()),
            &[&actor.signer],
            env.svm.latest_blockhash(),
        )
    }

    const COLD: usize = 2;
    const SUCCESSOR: usize = 3;
    const BYSTANDER: usize = 4;
    let mut peak_cu = 0;
    let mut peak_release_cu = 0;
    for asset in [0u16, 1] {
        for first in [0usize, 1] {
            let config = MarketConfig::default();
            let deposits = config.actor_deposits;
            let mut env = V16Svm::new([0x48; 32], config);
            env.begin_public_trace();
            env.update_asset_authority_from_admin(asset, processor::ASSET_AUTH_ADMIN, COLD)
                .unwrap();
            env.trade_no_cpi(
                0,
                1,
                asset,
                10 * percolator::POS_SCALE as i128,
                INITIAL_PRICE,
                0,
            )
            .unwrap();
            let profile = env.primary_profile(asset as usize);
            let sequences = env.primary_control_sequences(asset as usize);
            let peer = (1 - asset) as usize;
            let peer_profile = env.primary_profile(peer);
            let peer_sequences = env.primary_control_sequences(peer);
            assert_ne!(
                profile.oracle_authority,
                env.actors[COLD].signer.pubkey().to_bytes()
            );
            let takeover = env.build_retained_asset_authority_handoff_between_actors(
                asset,
                processor::ASSET_AUTH_ORACLE,
                COLD,
                SUCCESSOR,
            );
            env.resolve_market().unwrap();
            assert_eq!(env.primary_market_state().1.mode, MarketModeV16::Resolved);
            assert_eq!(env.primary_control_sequences(asset as usize), sequences);
            assert_eq!(env.primary_profile(asset as usize), profile);

            // The first real terminal payout must roll back if management would
            // seize the oracle while the opposite side still has an owner.
            let first_close = close(&env, first);
            let bundle = env.bundle_retained_transactions(&[first_close.clone(), takeover.clone()]);
            reject(&mut env, bundle, 4, PercolatorError::EngineLockActive, 1, 1);
            env.land_retained(first_close)
                .expect("identical owner settlement survives the rejected management suffix");
            assert_eq!(
                u128::from(env.token_amount(env.actors[first].destination_token)),
                deposits[first]
            );
            let partial = env.primary_market_state().1;
            let selected = partial.assets[asset as usize];
            let size = 10 * percolator::POS_SCALE;
            assert_eq!(
                (selected.oi_eff_long_q, selected.oi_eff_short_q),
                if first == 0 { (0, size) } else { (size, 0) },
                "exercise each funded-oracle OI arm with its opposite side empty"
            );
            assert_eq!(
                (
                    selected.stored_pos_count_long,
                    selected.stored_pos_count_short
                ),
                if first == 0 { (0, 1) } else { (1, 0) }
            );
            assert_eq!(
                partial.c_tot,
                deposits.iter().sum::<u128>() - deposits[first]
            );
            assert_eq!(partial.pnl_pos_tot, 0);
            assert_eq!(partial.vault, partial.c_tot);
            assert_eq!(env.primary_control_sequences(asset as usize), sequences);

            // Closing an unrelated flat portfolio cannot release the incumbent's
            // remaining funded authority. This rejection starts with only one side.
            let bystander_close = close(&env, BYSTANDER);
            let bundle =
                env.bundle_retained_transactions(&[bystander_close.clone(), takeover.clone()]);
            reject(&mut env, bundle, 4, PercolatorError::EngineLockActive, 1, 1);
            assert_eq!(env.primary_profile(asset as usize), profile);
            env.land_retained(bystander_close).unwrap();

            let last = 1 - first;
            let last_close = close(&env, last);
            let release = env.bundle_retained_transactions(&[last_close.clone(), takeover.clone()]);
            release.verify().unwrap();
            assert!(bincode::serialized_size(&release).unwrap() <= 1_232);
            let simulated = env.svm.simulate_transaction(release.into()).expect(
                "the final exposed owner can exit before cold management in one transaction",
            );
            assert!(simulated.compute_units_consumed <= 1_400_000);
            peak_release_cu = peak_release_cu.max(simulated.compute_units_consumed);
            env.land_retained(last_close).unwrap();
            let flat = env.primary_market_state();
            let selected = flat.1.assets[asset as usize];
            assert_eq!((selected.oi_eff_long_q, selected.oi_eff_short_q), (0, 0));
            assert_eq!(
                (
                    selected.stored_pos_count_long,
                    selected.stored_pos_count_short
                ),
                (0, 0)
            );
            assert_eq!(flat.1.c_tot, deposits[COLD] + deposits[SUCCESSOR]);
            assert_eq!(flat.1.vault, flat.1.c_tot);
            assert_eq!(env.primary_control_sequences(asset as usize), sequences);
            env.land_retained(takeover)
                .expect("retained cold consent becomes admissible after the last exposure exits");
            assert_eq!(
                env.primary_market_state(),
                flat,
                "management moves no owner value"
            );
            let mut expected_profile = profile;
            expected_profile.oracle_authority = env.actors[SUCCESSOR].signer.pubkey().to_bytes();
            assert_eq!(env.primary_profile(asset as usize), expected_profile);
            let mut expected_sequences = sequences;
            expected_sequences.authority_epoch += 1;
            assert_eq!(
                env.primary_control_sequences(asset as usize),
                expected_sequences
            );

            for owner in [COLD, SUCCESSOR] {
                env.close_resolved_primary_signed(owner).unwrap();
            }
            for (owner, deposited) in deposits.into_iter().enumerate() {
                assert_eq!(
                    u128::from(env.token_amount(env.actors[owner].destination_token)),
                    deposited,
                    "owner {owner} receives exactly their own principal"
                );
            }
            let settled = env.primary_market_state().1;
            assert_eq!(
                (settled.c_tot, settled.pnl_pos_tot, settled.vault),
                (0, 0, 0)
            );
            assert_eq!(env.token_amount(env.vault), 0);
            assert_eq!(env.token_supply_observed(), env.initial_token_supply);
            assert_eq!(env.mint_supply() as u128, env.initial_token_supply);
            assert_eq!(env.primary_profile(peer), peer_profile);
            assert_eq!(env.primary_control_sequences(peer), peer_sequences);
            let trace = env.finish_public_trace();
            trace.validate_public_execution().unwrap();
            assert_eq!(trace.steps.iter().filter(|step| !step.succeeded).count(), 2);
            peak_cu = peak_cu.max(
                trace
                    .steps
                    .iter()
                    .filter_map(|step| step.compute_units)
                    .max()
                    .unwrap(),
            );
        }
    }
    eprintln!("row416 last resolved exposure: worlds=4, exact_rejections=8, SPL_rollbacks=8, empty_management_controls=4, exact_owner_payouts=20, peak_success_cu={peak_cu}, peak_simulated_release_cu={peak_release_cu}");
}

#[test]
fn v16_program_cold_oracle_handoff_waits_for_last_live_quantity_tick() {
    const COLD: usize = 2;
    const SUCCESSOR: usize = 3;
    const PREFIX_OWNER: usize = 4;
    const PREFIX: u128 = 7;
    let mut peak_cu = 0;
    for asset in [0u16, 1] {
        for direction in [-1i128, 1] {
            let config = MarketConfig::default();
            let deposits = config.actor_deposits;
            let mut env = V16Svm::new([0x49; 32], config);
            env.begin_public_trace();
            env.update_asset_authority_from_admin(asset, processor::ASSET_AUTH_ADMIN, COLD)
                .unwrap();
            let size = direction * 10 * percolator::POS_SCALE as i128;
            env.trade_no_cpi(0, 1, asset, size, INITIAL_PRICE, 0)
                .unwrap();
            let profile = env.primary_profile(asset as usize);
            let sequences = env.primary_control_sequences(asset as usize);
            let peer = (1 - asset) as usize;
            let peer_profile = env.primary_profile(peer);
            let peer_sequences = env.primary_control_sequences(peer);
            assert_ne!(
                profile.oracle_authority,
                env.actors[COLD].signer.pubkey().to_bytes()
            );
            let takeover = env.build_retained_asset_authority_handoff_between_actors(
                asset,
                processor::ASSET_AUTH_ORACLE,
                COLD,
                SUCCESSOR,
            );

            // One fixed-point quantity tick still funds the oracle role, even
            // though almost the entire original position has been removed.
            env.trade_no_cpi(0, 1, asset, -size + direction, INITIAL_PRICE, 0)
                .unwrap();
            let residual = env.primary_market_state().1;
            let selected = residual.assets[asset as usize];
            assert_eq!(residual.mode, MarketModeV16::Live);
            assert_eq!(selected.lifecycle, AssetLifecycleV16::Active);
            assert_eq!((selected.oi_eff_long_q, selected.oi_eff_short_q), (1, 1));
            assert_eq!(
                (
                    selected.stored_pos_count_long,
                    selected.stored_pos_count_short
                ),
                (1, 1)
            );
            assert_eq!(env.primary_profile(asset as usize), profile);
            assert_eq!(env.primary_control_sequences(asset as usize), sequences);
            let withdrawal = env.build_retained_withdrawal(PREFIX_OWNER, PREFIX);
            let bundle = env.bundle_retained_transactions(&[withdrawal.clone(), takeover.clone()]);
            reject(&mut env, bundle, 4, PercolatorError::EngineLockActive, 1, 1);

            // Keep the handoff's signed bytes and epoch: only the public final
            // reduction changes its admission, with the market still Live.
            env.trade_no_cpi(0, 1, asset, -direction, INITIAL_PRICE, 0)
                .expect("the final quantity tick remains reducible");
            let flat = env.primary_market_state();
            let selected = flat.1.assets[asset as usize];
            assert_eq!((selected.oi_eff_long_q, selected.oi_eff_short_q), (0, 0));
            assert_eq!(
                (
                    selected.stored_pos_count_long,
                    selected.stored_pos_count_short
                ),
                (0, 0)
            );
            assert_eq!(flat.1.mode, MarketModeV16::Live);
            assert_eq!(selected.lifecycle, AssetLifecycleV16::Active);
            assert_eq!(flat.1.c_tot, deposits.iter().sum::<u128>());
            assert_eq!(flat.1.pnl_pos_tot, 0);
            assert_eq!(flat.1.vault, flat.1.c_tot);
            assert_eq!(env.primary_control_sequences(asset as usize), sequences);
            env.land_retained(takeover)
                .expect("the original cold consent becomes valid at exactly zero exposure");
            assert_eq!(env.primary_market_state(), flat);
            let mut expected_profile = profile;
            expected_profile.oracle_authority = env.actors[SUCCESSOR].signer.pubkey().to_bytes();
            assert_eq!(env.primary_profile(asset as usize), expected_profile);
            let mut expected_sequences = sequences;
            expected_sequences.authority_epoch += 1;
            assert_eq!(
                env.primary_control_sequences(asset as usize),
                expected_sequences
            );
            env.land_retained(withdrawal)
                .expect("the rolled-back SPL prefix remains usable");
            assert_eq!(
                env.token_amount(env.actors[PREFIX_OWNER].destination_token),
                PREFIX as u64
            );

            let mut remaining = deposits.iter().sum::<u128>() - PREFIX;
            for (owner, deposited) in deposits.into_iter().enumerate() {
                let owed = deposited - if owner == PREFIX_OWNER { PREFIX } else { 0 };
                env.withdraw_primary(owner, owed)
                    .expect("exact live principal exit");
                remaining -= owed;
                assert_eq!(
                    u128::from(env.token_amount(env.actors[owner].destination_token)),
                    deposited
                );
                let group = env.primary_market_state().1;
                assert_eq!(
                    (group.c_tot, group.pnl_pos_tot, group.vault),
                    (remaining, 0, remaining)
                );
                assert_eq!(u128::from(env.token_amount(env.vault)), remaining);
                assert_eq!(env.token_supply_observed(), env.initial_token_supply);
            }
            assert_eq!(remaining, 0);
            assert_eq!(env.mint_supply() as u128, env.initial_token_supply);
            assert_eq!(env.primary_profile(peer), peer_profile);
            assert_eq!(env.primary_control_sequences(peer), peer_sequences);
            let trace = env.finish_public_trace();
            trace.validate_public_execution().unwrap();
            assert_eq!(trace.steps.iter().filter(|step| !step.succeeded).count(), 1);
            peak_cu = peak_cu.max(
                trace
                    .steps
                    .iter()
                    .filter_map(|step| step.compute_units)
                    .max()
                    .unwrap(),
            );
        }
    }
    eprintln!("row416 last live quantity tick: worlds=4, exact_SPL_rollbacks=4, empty_management_controls=4, exact_owner_exits=20, peak_success_cu={peak_cu}");
}

#[test]
fn v16_program_retained_oracle_handoff_recovery_rollback_preserves_booked_claim() {
    recovery_handoff_claim(false);
}

#[test]
fn v16_program_cold_oracle_handoff_waits_for_recovery_loss_cleanup() {
    recovery_handoff_claim(true);
}

fn recovery_handoff_claim(check_zero_oi_containment: bool) {
    const COLD: usize = 2;
    const SUCCESSOR: usize = 3;
    const PREFIX_OWNER: usize = 4;
    const PREFIX: u128 = 7;
    const LOTS: i128 = 10;
    const DELTA: u64 = INITIAL_PRICE / 20;
    const GAIN: u128 = LOTS as u128 * DELTA as u128;
    let mut peak_cu = 0;
    let mut peak_preview_cu = 0;
    for asset in [0u16, 1] {
        for direction in [-1i128, 1] {
            let config = MarketConfig::default();
            let deposits = config.actor_deposits;
            let mut env = V16Svm::new([0x4a; 32], config);
            env.begin_public_trace();
            env.configure_permissionless_resolve(100, 5).unwrap();
            env.update_asset_authority_from_admin(asset, processor::ASSET_AUTH_ADMIN, COLD)
                .unwrap();
            env.trade_no_cpi(
                0,
                1,
                asset,
                direction * LOTS * percolator::POS_SCALE as i128,
                INITIAL_PRICE,
                0,
            )
            .unwrap();
            for slot in [2, 3] {
                env.warp_to_slot(slot);
                let mark =
                    (INITIAL_PRICE as i128 + direction * (slot - 1) as i128 * DELTA as i128) as u64;
                let observation = env.build_retained_auth_mark(asset, mark);
                env.land_retained(observation).unwrap();
                env.crank(1, slot, crank_observations(asset)).unwrap();
                if slot == 2 {
                    env.crank(0, slot, crank_observations(asset)).unwrap();
                }
            }
            let winner = env.primary_portfolio(0);
            assert_eq!(winner.pnl.get(), GAIN as i128);
            assert_eq!(winner.capital.get(), deposits[0]);
            assert_eq!(
                env.primary_portfolio(1).capital.get(),
                deposits[1] - 2 * GAIN
            );
            let selected = env.primary_market_state().1.assets[asset as usize];
            assert_ne!(
                active_leg_for_asset(&winner, asset as usize).k_snap,
                if direction > 0 {
                    selected.k_long
                } else {
                    selected.k_short
                },
                "a second gain must remain unrefreshed for Recovery to forfeit"
            );
            let peer = (1 - asset) as usize;
            let peer_profile = env.primary_profile(peer);
            let peer_sequences = env.primary_control_sequences(peer);
            let sequences = env.primary_control_sequences(asset as usize);
            env.warp_to_slot(4);
            let withdrawal = env.build_retained_withdrawal(PREFIX_OWNER, PREFIX);
            let handoff = env.build_retained_asset_authority_handoff_from_admin(
                asset,
                processor::ASSET_AUTH_ORACLE,
                SUCCESSOR,
            );
            let mark = env.build_retained_market_control_for_actor(
                SUCCESSOR,
                ProgInstruction::PushAuthMark {
                    asset_index: asset,
                    market_id: selected.market_id,
                    now_slot: 4,
                    mark_e6: (INITIAL_PRICE as i128 + direction * 3 * DELTA as i128) as u64,
                    observation_sequence: sequences.oracle_observation + 1,
                    authority_epoch: sequences.authority_epoch + 1,
                },
            );
            let bundle =
                env.bundle_retained_transactions(&[withdrawal.clone(), handoff.clone(), mark]);
            bundle.verify().unwrap();
            assert!(bincode::serialized_size(&bundle).unwrap() <= 1_232);
            let preview = env.svm.simulate_transaction(bundle.clone().into()).expect(
                "the retained SPL payout, incumbent handoff and successor mark are valid in Active",
            );
            peak_preview_cu = peak_preview_cu.max(preview.compute_units_consumed);

            let shutdown = env.build_retained_shutdown_asset_for_actor(COLD, asset, 4);
            env.land_retained(shutdown).unwrap();
            let frozen = env.primary_market_state();
            assert_eq!(frozen.1.mode, MarketModeV16::Live);
            assert_eq!(
                frozen.1.assets[asset as usize].lifecycle,
                AssetLifecycleV16::Recovery
            );
            assert_eq!(env.primary_control_sequences(asset as usize), sequences);
            let profile = env.primary_profile(asset as usize);
            assert_eq!(profile.last_good_oracle_slot, 4);
            let portfolios: Vec<_> = (0..5).map(|i| env.primary_portfolio_data(i)).collect();
            reject(&mut env, bundle, 5, PercolatorError::EngineLockActive, 2, 1);
            let takeover = env.build_retained_asset_authority_handoff_between_actors(
                asset,
                processor::ASSET_AUTH_ORACLE,
                COLD,
                SUCCESSOR,
            );
            let takeover_bundle = env.bundle_retained_transactions(&[withdrawal.clone(), takeover]);
            reject(
                &mut env,
                takeover_bundle,
                4,
                PercolatorError::EngineLockActive,
                1,
                1,
            );

            env.land_retained(handoff).expect(
                "identical incumbent consent remains usable after the Recovery suffix rolls back",
            );
            assert_eq!(
                env.primary_market_state(),
                frozen,
                "succession moves no claim or loss"
            );
            assert_eq!(
                (0..5)
                    .map(|i| env.primary_portfolio_data(i))
                    .collect::<Vec<_>>(),
                portfolios
            );
            let mut expected_profile = profile;
            expected_profile.oracle_authority = env.actors[SUCCESSOR].signer.pubkey().to_bytes();
            assert_eq!(env.primary_profile(asset as usize), expected_profile);
            let mut expected_sequences = sequences;
            expected_sequences.authority_epoch += 1;
            assert_eq!(
                env.primary_control_sequences(asset as usize),
                expected_sequences
            );
            env.land_retained(withdrawal).unwrap();

            let check_claim = |env: &V16Svm| {
                assert_eq!(env.primary_portfolio(0).pnl.get(), GAIN as i128);
                assert_eq!(env.primary_portfolio(0).capital.get(), deposits[0]);
                assert_eq!(
                    env.primary_portfolio(1).capital.get(),
                    deposits[1] - 2 * GAIN
                );
                assert_eq!(env.primary_portfolio(1).pnl.get(), 0);
                let group = env.primary_market_state().1;
                assert_eq!(group.pnl_pos_tot, GAIN);
                assert_eq!(
                    group.source_claim_bound_total_num,
                    GAIN * percolator::BOUND_SCALE
                );
                assert_eq!(
                    group.c_tot,
                    deposits.iter().sum::<u128>() - 2 * GAIN - PREFIX
                );
                assert_eq!(group.vault, deposits.iter().sum::<u128>() - PREFIX);
                assert_eq!(u128::from(env.token_amount(env.vault)), group.vault);
                assert_eq!(env.primary_profile(asset as usize), expected_profile);
                assert_eq!(
                    env.primary_control_sequences(asset as usize),
                    expected_sequences
                );
            };
            check_claim(&env);
            let order = if check_zero_oi_containment || direction > 0 {
                [0, 1]
            } else {
                [1, 0]
            };
            for owner in order {
                env.forfeit_recovery_leg(owner, asset, u128::MAX).unwrap();
                check_claim(&env);
            }
            let flat = env.primary_market_state().1.assets[asset as usize];
            assert_eq!((flat.oi_eff_long_q, flat.oi_eff_short_q), (0, 0));
            let cleanup_handoff = if check_zero_oi_containment {
                // Both quantities are zero, but the first forfeited owner still
                // carries a stored loss obligation on its original side.
                assert_eq!(
                    (flat.stored_pos_count_long, flat.stored_pos_count_short),
                    if direction > 0 { (1, 0) } else { (0, 1) }
                );
                assert_eq!(
                    (
                        flat.pending_obligation_count_long,
                        flat.pending_obligation_count_short
                    ),
                    if direction > 0 { (1, 0) } else { (0, 1) }
                );
                let weight = LOTS as u128 * percolator::POS_SCALE;
                assert_eq!(
                    (flat.loss_weight_sum_long, flat.loss_weight_sum_short),
                    if direction > 0 {
                        (weight, 0)
                    } else {
                        (0, weight)
                    }
                );
                let takeover = env.build_retained_asset_authority_handoff_between_actors(
                    asset,
                    processor::ASSET_AUTH_ORACLE,
                    COLD,
                    0,
                );
                let withdrawal = env.build_retained_withdrawal(PREFIX_OWNER, PREFIX);
                let bundle = env.bundle_retained_transactions(&[withdrawal, takeover.clone()]);
                reject(&mut env, bundle, 4, PercolatorError::EngineLockActive, 1, 1);
                check_claim(&env);
                Some(takeover)
            } else {
                None
            };
            // Forfeiture can leave zero-position loss weight until its peer exits.
            for owner in order {
                for _ in 0..2 {
                    if !has_active_leg_for_asset(&env.primary_portfolio(owner), asset as usize) {
                        break;
                    }
                    env.crank(owner, 4, vec![]).unwrap();
                    check_claim(&env);
                }
                assert!(!has_active_leg_for_asset(
                    &env.primary_portfolio(owner),
                    asset as usize
                ));
            }
            if let Some(takeover) = cleanup_handoff {
                let clean = env.primary_market_state();
                let selected = clean.1.assets[asset as usize];
                assert_eq!(
                    (
                        selected.oi_eff_long_q,
                        selected.oi_eff_short_q,
                        selected.stored_pos_count_long,
                        selected.stored_pos_count_short,
                        selected.pending_obligation_count_long,
                        selected.pending_obligation_count_short,
                        selected.loss_weight_sum_long,
                        selected.loss_weight_sum_short
                    ),
                    (0, 0, 0, 0, 0, 0, 0, 0)
                );
                assert_eq!(clean.1.mode, MarketModeV16::Live);
                assert_eq!(selected.lifecycle, AssetLifecycleV16::Recovery);
                check_claim(&env);
                let portfolios: Vec<_> = (0..5).map(|i| env.primary_portfolio_data(i)).collect();
                env.land_retained(takeover).expect(
                    "the unchanged cold consent becomes admissible after public loss cleanup",
                );
                assert_eq!(env.primary_market_state(), clean);
                assert_eq!(
                    (0..5)
                        .map(|i| env.primary_portfolio_data(i))
                        .collect::<Vec<_>>(),
                    portfolios
                );
                let mut released_profile = expected_profile;
                released_profile.oracle_authority = env.actors[0].signer.pubkey().to_bytes();
                assert_eq!(env.primary_profile(asset as usize), released_profile);
                let mut released_sequences = expected_sequences;
                released_sequences.authority_epoch += 1;
                assert_eq!(
                    env.primary_control_sequences(asset as usize),
                    released_sequences
                );
            }
            env.resolve_market().unwrap();
            let expected = [
                deposits[0] + GAIN,
                deposits[1] - 2 * GAIN,
                deposits[2],
                deposits[3],
                deposits[4],
            ];
            let mut remaining = deposits.iter().sum::<u128>() - PREFIX;
            for owner in [1, COLD, SUCCESSOR, PREFIX_OWNER, 0] {
                for _ in 0..16 {
                    if u128::from(env.token_amount(env.actors[owner].destination_token))
                        == expected[owner]
                    {
                        break;
                    }
                    let before = (env.market_data(false), env.primary_portfolio_data(owner));
                    env.close_resolved_primary_signed(owner).unwrap();
                    assert_ne!(
                        (env.market_data(false), env.primary_portfolio_data(owner)),
                        before
                    );
                }
                remaining -= expected[owner] - if owner == PREFIX_OWNER { PREFIX } else { 0 };
                assert_eq!(
                    u128::from(env.token_amount(env.actors[owner].destination_token)),
                    expected[owner]
                );
                assert_eq!(env.primary_market_state().1.vault, remaining);
                assert_eq!(u128::from(env.token_amount(env.vault)), remaining);
                assert_eq!(env.token_supply_observed(), env.initial_token_supply);
            }
            let settled = env.primary_market_state().1;
            assert_eq!(
                (
                    settled.c_tot,
                    settled.pnl_pos_tot,
                    settled.source_claim_bound_total_num
                ),
                (0, 0, 0)
            );
            assert_eq!(
                remaining, GAIN,
                "the forfeited gain is not assigned to any owner"
            );
            assert_eq!(env.primary_profile(peer), peer_profile);
            assert_eq!(env.primary_control_sequences(peer), peer_sequences);
            assert_eq!(env.mint_supply() as u128, env.initial_token_supply);
            let trace = env.finish_public_trace();
            trace.validate_public_execution().unwrap();
            assert_eq!(
                trace.steps.iter().filter(|step| !step.succeeded).count(),
                if check_zero_oi_containment { 3 } else { 2 }
            );
            peak_cu = peak_cu.max(
                trace
                    .steps
                    .iter()
                    .filter_map(|step| step.compute_units)
                    .max()
                    .unwrap(),
            );
        }
    }
    if check_zero_oi_containment {
        eprintln!("row416 zero-OI Recovery containment: worlds=4, exact_rejections=12, zero_OI_SPL_rollbacks=4, unchanged_handoff_releases=4, exact_owner_payouts=20, peak_success_cu={peak_cu}, peak_active_preview_cu={peak_preview_cu}");
    } else {
        eprintln!("row416 retained oracle Recovery claim: worlds=4, exact_rejections=8, handoff_rollbacks=4, SPL_rollbacks=8, recovery_forfeits=8, exact_owner_payouts=20, peak_success_cu={peak_cu}, peak_active_preview_cu={peak_preview_cu}");
    }
}
