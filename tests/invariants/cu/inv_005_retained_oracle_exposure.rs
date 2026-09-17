//! Row 416: empty-asset oracle consent must be rechecked after public exposure.
//! Rejection restores an SPL payout prefix; incumbent succession still permits
//! authenticated settlement and exact principal exits for every fixture owner.

use super::*;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction, fee::FeeStructure, instruction::InstructionError,
    transaction::TransactionError,
};

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

#[test]
fn v16_program_retained_oracle_handoff_after_cold_admin_return_preserves_drain_exit() {
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
