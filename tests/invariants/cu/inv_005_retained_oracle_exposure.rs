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
