//! INV-038 - rounding and ratio conservation.
//!
//! Normative obligation: every rounded allocation preserves
//! `input = sum(outputs) + explicit residue`, with residue assigned only to a
//! non-user-value class. These public-route LiteSVM tests cover dust trade fees,
//! split subatom execution, batch reconstruction, funding direction/caps, and
//! backing-fee splits so rounding cannot mint principal, insurance, backing, or
//! withdrawable PnL. The social-loss matrix also compares one aggregate public
//! booking/settlement with the same residual carried through one-atom calls;
//! both schedules must converge to an identical user-value, asset, ledger, and
//! SPL-custody frame.

use super::*;

// This module now owns deployed public evidence for exact social-loss B booking, per-account B
// settlement, and zero-OI carry normalization. Each route checks the persisted quotient/remainder
// partition rather than inferring correctness from a balanced final vault alone.

// The public bankruptcy route has two independent rounded allocations. Booking converts collateral
// atoms into a side-local B-index delta and persists the division remainder on the market. Settling
// that B delta converts it back into an account loss and persists the second remainder on the leg.
// The close ledger and the first account settlement expose atom outputs independently; the terminal
// combined forfeit is additionally bound by its persisted leg carry and exact SPL custody. Together
// the identities detect a dropped, duplicated, or misattributed sub-atom carry in deployed code.
#[test]
fn v16_program_social_loss_booking_and_settlement_preserve_exact_remainders() {
    const ASSET: usize = 1;

    let PublicActiveCloseFixture {
        mut env,
        loss_owner,
        loss,
        asset1_counterparty_owner,
        asset1_counterparty,
        ..
    } = public_asset1_bankrupt_close_fixture();

    let before_market = env.market_state().1;
    let before_asset = before_market.assets[ASSET];
    let before_close = close_progress(&env.portfolio_state(loss));
    assert!(before_close.active && before_close.residual_remaining > 0);
    assert!(before_asset.loss_weight_sum_long > 0);

    env.svm.expire_blockhash();
    let booking_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 0,
                observations: vec![],
            },
            vec![
                AccountMeta::new_readonly(env.payer.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(loss, false),
            ],
            &[],
        )
        .expect("public close continuation must book a social-loss chunk");
    assert_cu_within(
        "INV-038 exact social-loss booking partition",
        booking_cu,
        CRANK_CU_LIMIT,
    );

    let after_booking_market = env.market_state().1;
    let after_booking_asset = after_booking_market.assets[ASSET];
    let after_close = close_progress(&env.portfolio_state(loss));
    let booked_atoms = after_close
        .b_loss_booked
        .checked_sub(before_close.b_loss_booked)
        .expect("booked-loss ledger must be monotonic");
    let booked_delta_b = after_booking_asset
        .b_long_num
        .checked_sub(before_asset.b_long_num)
        .expect("loss-side B index must be monotonic");
    assert!(booked_atoms > 0 && booked_delta_b > 0);
    assert_eq!(
        after_booking_asset.loss_weight_sum_long, before_asset.loss_weight_sum_long,
        "booking must use one stable loss-weight denominator"
    );
    let booking_numerator = booked_atoms
        .checked_mul(percolator::SOCIAL_LOSS_DEN)
        .and_then(|value| value.checked_add(before_asset.social_loss_remainder_long_num))
        .expect("bounded booking numerator");
    let booking_partition = booked_delta_b
        .checked_mul(before_asset.loss_weight_sum_long)
        .and_then(|value| value.checked_add(after_booking_asset.social_loss_remainder_long_num))
        .expect("bounded booking quotient/remainder partition");
    assert_eq!(
        booking_partition, booking_numerator,
        "booked atoms must equal B-index allocation plus the persisted market remainder"
    );
    assert!(
        after_booking_asset.social_loss_remainder_long_num < before_asset.loss_weight_sum_long,
        "market booking remainder must be strictly below its denominator"
    );

    let before_account = env.portfolio_state(asset1_counterparty);
    let before_leg = active_leg_for_asset(&before_account, ASSET);
    let target_b = after_booking_asset.b_long_num;
    assert!(target_b > before_leg.b_snap);
    let settlement_cu = env.forfeit_recovery_leg_with_cu(
        &asset1_counterparty_owner,
        asset1_counterparty,
        ASSET as u16,
        1,
    );
    assert_cu_within(
        "INV-038 exact account B settlement partition",
        settlement_cu,
        CUSTODY_CU_LIMIT,
    );

    let after_account = env.portfolio_state(asset1_counterparty);
    let after_leg = active_leg_for_asset(&after_account, ASSET);
    let settled_delta_b = after_leg
        .b_snap
        .checked_sub(before_leg.b_snap)
        .expect("account B snapshot must be monotonic");
    let settled_atoms = before_account
        .pnl
        .get()
        .checked_sub(after_account.pnl.get())
        .and_then(|value| u128::try_from(value).ok())
        .expect("public settlement must debit a nonnegative atom amount");
    assert!(settled_delta_b > 0 && settled_atoms > 0);
    assert_eq!(after_leg.loss_weight, before_leg.loss_weight);
    let settlement_numerator = before_leg
        .loss_weight
        .checked_mul(settled_delta_b)
        .and_then(|value| value.checked_add(before_leg.b_rem))
        .expect("bounded settlement numerator");
    let settlement_partition = settled_atoms
        .checked_mul(percolator::SOCIAL_LOSS_DEN)
        .and_then(|value| value.checked_add(after_leg.b_rem))
        .expect("bounded settlement quotient/remainder partition");
    assert_eq!(
        settlement_partition, settlement_numerator,
        "account loss plus persisted leg remainder must exactly reconstruct the B allocation"
    );
    assert!(
        after_leg.b_rem < percolator::SOCIAL_LOSS_DEN,
        "account settlement remainder must be strictly below its denominator"
    );
    assert!(
        after_leg.b_snap <= target_b,
        "bounded settlement cannot consume beyond the committed B target"
    );

    // The next owner-signed recovery step consumes the final committed B delta. Auto-crank dispatch
    // is intentionally one action per call, so a later call detaches the now-current recovery leg.
    let remaining_delta_b = target_b
        .checked_sub(after_leg.b_snap)
        .expect("settled leg cannot pass the committed B target");
    assert!(
        remaining_delta_b > 0,
        "fixture must retain one final B chunk"
    );
    let final_settlement_numerator = after_leg
        .loss_weight
        .checked_mul(remaining_delta_b)
        .and_then(|value| value.checked_add(after_leg.b_rem))
        .expect("bounded final settlement numerator");
    let final_settlement_loss = final_settlement_numerator / percolator::SOCIAL_LOSS_DEN;
    let final_leg_remainder = final_settlement_numerator % percolator::SOCIAL_LOSS_DEN;
    assert!(final_settlement_loss > 0);

    let final_settlement_cu = env.forfeit_recovery_leg_with_cu(
        &asset1_counterparty_owner,
        asset1_counterparty,
        ASSET as u16,
        1,
    );
    assert_cu_within(
        "INV-038 exact final account B settlement partition",
        final_settlement_cu,
        CUSTODY_CU_LIMIT,
    );
    let current_account = env.portfolio_state(asset1_counterparty);
    let current_leg = active_leg_for_asset(&current_account, ASSET);
    assert_eq!(current_leg.b_snap, target_b);
    assert_eq!(current_leg.b_rem, final_leg_remainder);
    assert_eq!(
        final_settlement_loss
            .checked_mul(percolator::SOCIAL_LOSS_DEN)
            .and_then(|value| value.checked_add(current_leg.b_rem)),
        Some(final_settlement_numerator),
        "the terminal combined forfeit must persist the exact final B quotient/remainder"
    );
    assert_eq!(current_leg.basis_pos_q, 0);
    assert_eq!(current_account.pnl.get(), 0);

    // The opposite bankrupt episode owns the domain barrier until its close ledger finalizes and
    // its owner forfeits the remaining Recovery basis. Releasing the surviving obligation before
    // this point would misattribute its carry.
    for step in 0..8 {
        if close_progress(&env.portfolio_state(loss)).finalized {
            break;
        }
        env.svm.expire_blockhash();
        let cu = env
            .send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: 0,
                    observations: crank_observations(ASSET as u16),
                },
                vec![
                    AccountMeta::new_readonly(env.payer.pubkey(), false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(loss, false),
                ],
                &[],
            )
            .unwrap_or_else(|error| {
                panic!("permissionless close-finalization crank {step} must progress: {error}")
            });
        assert_cu_within("INV-038 domain-barrier close progress", cu, CRANK_CU_LIMIT);
    }
    assert!(
        close_progress(&env.portfolio_state(loss)).finalized,
        "bounded public cranks must finalize the zero-residual close ledger"
    );
    if has_active_leg_for_asset(&env.portfolio_state(loss), ASSET) {
        let owner_forfeit_cu = env.forfeit_recovery_leg_with_cu(&loss_owner, loss, ASSET as u16, 1);
        assert_cu_within(
            "INV-038 finalized bankrupt owner forfeit",
            owner_forfeit_cu,
            CUSTODY_CU_LIMIT,
        );
    }
    for step in 0..8 {
        if !has_active_leg_for_asset(&env.portfolio_state(loss), ASSET) {
            break;
        }
        env.svm.expire_blockhash();
        let cu = env
            .send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: 0,
                    observations: crank_observations(ASSET as u16),
                },
                vec![
                    AccountMeta::new_readonly(env.payer.pubkey(), false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(loss, false),
                ],
                &[],
            )
            .unwrap_or_else(|error| {
                panic!("permissionless bankrupt-obligation crank {step} must progress: {error}")
            });
        assert_cu_within("INV-038 bankrupt-obligation release", cu, CRANK_CU_LIMIT);
    }
    assert!(
        !has_active_leg_for_asset(&env.portfolio_state(loss), ASSET),
        "bounded public cranks must clear the completed bankrupt episode"
    );

    // Detaching moves the leg's persisted sub-atom carry, plus any side-level booking remainder
    // that becomes economically exhausted at zero OI, into dust/explicit loss exactly once.
    let mut carry_normalization_checked = false;
    for step in 0..8 {
        let before_detach_account = env.portfolio_state(asset1_counterparty);
        let before_detach_leg = active_leg_for_asset(&before_detach_account, ASSET);
        let before_detach_asset = env.market_state().1.assets[ASSET];
        env.svm.expire_blockhash();
        let detach_cu = env
            .send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: 0,
                    observations: crank_observations(ASSET as u16),
                },
                vec![
                    AccountMeta::new_readonly(env.payer.pubkey(), false),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(asset1_counterparty, false),
                ],
                &[],
            )
            .unwrap_or_else(|error| {
                panic!("permissionless residue crank {step} must progress: {error}")
            });
        assert_cu_within(
            "INV-038 exact social-loss carry normalization",
            detach_cu,
            CRANK_CU_LIMIT,
        );
        let after_detach_account = env.portfolio_state(asset1_counterparty);
        if has_active_leg_for_asset(&after_detach_account, ASSET) {
            continue;
        }

        let after_detach_asset = env.market_state().1.assets[ASSET];
        let consumed_market_remainder = before_detach_asset
            .social_loss_remainder_long_num
            .checked_sub(after_detach_asset.social_loss_remainder_long_num)
            .expect("zero-OI normalization cannot increase the booking remainder");
        assert!(
            after_detach_asset.social_loss_remainder_long_num == 0
                || after_detach_asset.social_loss_remainder_long_num
                    == before_detach_asset.social_loss_remainder_long_num,
            "booking remainder must either stay live or be consumed in full"
        );
        let carry_numerator = before_detach_asset
            .social_loss_dust_long_num
            .checked_add(before_detach_leg.b_rem)
            .and_then(|value| value.checked_add(consumed_market_remainder))
            .expect("bounded terminal carry numerator");
        let expected_explicit_increment = carry_numerator / percolator::SOCIAL_LOSS_DEN;
        let expected_dust = carry_numerator % percolator::SOCIAL_LOSS_DEN;
        assert_eq!(
            after_detach_asset.social_loss_dust_long_num, expected_dust,
            "detached leg and exhausted booking carries must remain as exact side-local dust"
        );
        assert_eq!(
            after_detach_asset.explicit_unallocated_loss_long,
            before_detach_asset
                .explicit_unallocated_loss_long
                .saturating_add(expected_explicit_increment),
            "each full carry atom must be classified exactly once as explicit side-local loss"
        );
        carry_normalization_checked = true;
        break;
    }
    assert!(
        carry_normalization_checked,
        "bounded public cranks must reach the exact carry-normalization transition"
    );
    assert_eq!(
        env.market_state().1.vault as u64,
        env.token_amount(env.vault),
        "bookkeeping-only rounding transitions cannot change SPL custody"
    );
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SocialLossPartitionFrame {
    asset0: percolator::AssetStateV16,
    asset1: percolator::AssetStateV16,
    market_mode: MarketModeV16,
    bankruptcy_hlock_active: bool,
    vault: u128,
    c_tot: u128,
    insurance: u128,
    loss_capital: u128,
    loss_pnl: i128,
    loss_active_bitmap: percolator::V16ActiveBitmap,
    loss_close: CloseProgressLedgerV16,
    counterparty_capital: u128,
    counterparty_pnl: i128,
    counterparty_active_bitmap: percolator::V16ActiveBitmap,
    counterparty_close: CloseProgressLedgerV16,
    spl_vault: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SocialLossPartitionOutcome {
    frame: SocialLossPartitionFrame,
    initial_residual: u128,
    booked_atoms: u128,
    settled_atoms: u128,
    booking_calls: usize,
    settlement_calls: usize,
    cleanup_calls: usize,
    max_compute_units: u64,
}

fn inv038_public_crank(
    env: &mut V16CuEnv,
    portfolio: Pubkey,
    observations: Vec<CrankObservationHint>,
    label: &str,
) -> u64 {
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations,
        },
        vec![
            AccountMeta::new_readonly(env.payer.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[],
    )
    .unwrap_or_else(|error| panic!("{label}: {error}"))
}

fn inv038_assert_social_loss_booking_partition(
    before_close: CloseProgressLedgerV16,
    after_close: CloseProgressLedgerV16,
    before_asset: percolator::AssetStateV16,
    after_asset: percolator::AssetStateV16,
    label: &str,
) -> u128 {
    let booked = after_close
        .b_loss_booked
        .checked_sub(before_close.b_loss_booked)
        .expect("INV-038 booked-loss ledger must be monotonic");
    let delta_b = after_asset
        .b_long_num
        .checked_sub(before_asset.b_long_num)
        .expect("INV-038 B index must be monotonic");
    assert!(booked > 0 && delta_b > 0, "{label}");
    assert_eq!(
        after_asset.loss_weight_sum_long, before_asset.loss_weight_sum_long,
        "{label}: booking must retain one denominator"
    );
    assert_eq!(
        delta_b
            .checked_mul(before_asset.loss_weight_sum_long)
            .and_then(|value| value.checked_add(after_asset.social_loss_remainder_long_num)),
        booked
            .checked_mul(percolator::SOCIAL_LOSS_DEN)
            .and_then(|value| value.checked_add(before_asset.social_loss_remainder_long_num)),
        "{label}: booking must preserve its exact carried remainder"
    );
    assert!(after_asset.social_loss_remainder_long_num < before_asset.loss_weight_sum_long);
    booked
}

fn run_social_loss_partition_schedule(public_b_chunk_atoms: u128) -> SocialLossPartitionOutcome {
    const ASSET: usize = 1;
    const MAX_STEPS: usize = 16;

    let PublicActiveCloseFixture {
        mut env,
        loss_owner,
        loss,
        asset1_counterparty_owner,
        asset1_counterparty,
        ..
    } = public_asset1_bankrupt_close_fixture_before_close_with_b_chunk_atoms(public_b_chunk_atoms);

    let before_start_close = close_progress(&env.portfolio_state(loss));
    let before_start_asset = env.market_state().1.assets[ASSET];
    assert!(!before_start_close.active);
    let start_cu =
        env.forfeit_recovery_leg_with_cu(&loss_owner, loss, ASSET as u16, public_b_chunk_atoms);
    assert_cu_within(
        "INV-038 partitioned close start",
        start_cu,
        CUSTODY_CU_LIMIT,
    );
    let after_start_close = close_progress(&env.portfolio_state(loss));
    let after_start_asset = env.market_state().1.assets[ASSET];
    assert!(after_start_close.active);
    let first_booked = inv038_assert_social_loss_booking_partition(
        before_start_close,
        after_start_close,
        before_start_asset,
        after_start_asset,
        "INV-038 close-start booking",
    );
    let initial_residual = first_booked
        .checked_add(after_start_close.residual_remaining)
        .expect("INV-038 initial residual overflow");
    let mut booked_atoms = first_booked;
    let mut booking_calls = 1usize;
    let mut settlement_calls = 0usize;
    let mut cleanup_calls = 0usize;
    let mut max_compute_units = start_cu;

    for step in 0..MAX_STEPS {
        let before_close = close_progress(&env.portfolio_state(loss));
        if before_close.residual_remaining == 0 {
            break;
        }
        let before_asset = env.market_state().1.assets[ASSET];
        let cu = inv038_public_crank(
            &mut env,
            loss,
            Vec::new(),
            &format!("INV-038 social-loss booking step {step}"),
        );
        assert_cu_within(
            "INV-038 partitioned social-loss booking",
            cu,
            CRANK_CU_LIMIT,
        );
        max_compute_units = max_compute_units.max(cu);

        let after_close = close_progress(&env.portfolio_state(loss));
        let after_asset = env.market_state().1.assets[ASSET];
        let booked = inv038_assert_social_loss_booking_partition(
            before_close,
            after_close,
            before_asset,
            after_asset,
            &format!("INV-038 booking step {step}"),
        );
        assert_eq!(
            before_close.residual_remaining - after_close.residual_remaining,
            booked
        );
        booked_atoms = booked_atoms.checked_add(booked).unwrap();
        booking_calls += 1;
    }
    assert_eq!(
        close_progress(&env.portfolio_state(loss)).residual_remaining,
        0
    );
    assert_eq!(booked_atoms, initial_residual);

    let target_b = env.market_state().1.assets[ASSET].b_long_num;
    let mut settled_atoms = 0u128;
    for step in 0..MAX_STEPS {
        let before_account = env.portfolio_state(asset1_counterparty);
        if !has_active_leg_for_asset(&before_account, ASSET) {
            break;
        }
        let before_leg = active_leg_for_asset(&before_account, ASSET);
        if before_leg.b_snap == target_b && before_leg.basis_pos_q == 0 {
            break;
        }
        let cu = env.forfeit_recovery_leg_with_cu(
            &asset1_counterparty_owner,
            asset1_counterparty,
            ASSET as u16,
            public_b_chunk_atoms,
        );
        assert_cu_within(
            "INV-038 partitioned account B settlement",
            cu,
            CUSTODY_CU_LIMIT,
        );
        max_compute_units = max_compute_units.max(cu);
        settlement_calls += 1;

        let after_account = env.portfolio_state(asset1_counterparty);
        let before_value = i128::try_from(before_account.capital.get())
            .ok()
            .and_then(|capital| capital.checked_add(before_account.pnl.get()))
            .expect("INV-038 before-account value overflow");
        let after_value = i128::try_from(after_account.capital.get())
            .ok()
            .and_then(|capital| capital.checked_add(after_account.pnl.get()))
            .expect("INV-038 after-account value overflow");
        let settled = before_value
            .checked_sub(after_value)
            .and_then(|value| u128::try_from(value).ok())
            .expect("INV-038 B settlement must debit a nonnegative amount");
        let after_leg = has_active_leg_for_asset(&after_account, ASSET)
            .then(|| active_leg_for_asset(&after_account, ASSET));
        let after_b_snap = after_leg.map_or(target_b, |leg| leg.b_snap);
        let delta_b = after_b_snap
            .checked_sub(before_leg.b_snap)
            .expect("INV-038 account B snapshot must be monotonic");
        assert!(
            delta_b > 0 && settled > 0,
            "INV-038 settlement step {step} cap={public_b_chunk_atoms}: delta_b={delta_b}, settled={settled}, before={}/{}, after={}/{}, target={target_b}, before_snap={}, detached={}",
            before_account.capital.get(),
            before_account.pnl.get(),
            after_account.capital.get(),
            after_account.pnl.get(),
            before_leg.b_snap,
            after_leg.is_none(),
        );
        let numerator = before_leg
            .loss_weight
            .checked_mul(delta_b)
            .and_then(|value| value.checked_add(before_leg.b_rem))
            .expect("INV-038 settlement numerator overflow");
        assert_eq!(settled, numerator / percolator::SOCIAL_LOSS_DEN);
        let expected_remainder = numerator % percolator::SOCIAL_LOSS_DEN;
        if let Some(after_leg) = after_leg {
            assert_eq!(after_leg.loss_weight, before_leg.loss_weight);
            assert_eq!(after_leg.b_rem, expected_remainder);
            assert!(after_leg.b_snap <= target_b);
        } else {
            assert_eq!(after_b_snap, target_b);
        }
        settled_atoms = settled_atoms.checked_add(settled).unwrap();
    }
    if has_active_leg_for_asset(&env.portfolio_state(asset1_counterparty), ASSET) {
        let settled_leg = active_leg_for_asset(&env.portfolio_state(asset1_counterparty), ASSET);
        assert_eq!(settled_leg.b_snap, target_b);
        assert_eq!(settled_leg.basis_pos_q, 0);
    }

    for step in 0..MAX_STEPS {
        if close_progress(&env.portfolio_state(loss)).finalized {
            break;
        }
        let cu = inv038_public_crank(
            &mut env,
            loss,
            crank_observations(ASSET as u16),
            &format!("INV-038 close cleanup step {step}"),
        );
        assert_cu_within("INV-038 partitioned close cleanup", cu, CRANK_CU_LIMIT);
        max_compute_units = max_compute_units.max(cu);
        cleanup_calls += 1;
    }
    assert!(close_progress(&env.portfolio_state(loss)).finalized);
    if has_active_leg_for_asset(&env.portfolio_state(loss), ASSET) {
        let cu =
            env.forfeit_recovery_leg_with_cu(&loss_owner, loss, ASSET as u16, public_b_chunk_atoms);
        assert_cu_within("INV-038 bankrupt owner cleanup", cu, CUSTODY_CU_LIMIT);
        max_compute_units = max_compute_units.max(cu);
        cleanup_calls += 1;
    }
    for (label, portfolio) in [
        ("bankrupt obligation", loss),
        ("counterparty obligation", asset1_counterparty),
    ] {
        for step in 0..MAX_STEPS {
            if !has_active_leg_for_asset(&env.portfolio_state(portfolio), ASSET) {
                break;
            }
            let cu = inv038_public_crank(
                &mut env,
                portfolio,
                crank_observations(ASSET as u16),
                &format!("INV-038 {label} cleanup step {step}"),
            );
            assert_cu_within("INV-038 partitioned obligation cleanup", cu, CRANK_CU_LIMIT);
            max_compute_units = max_compute_units.max(cu);
            cleanup_calls += 1;
        }
        assert!(
            !has_active_leg_for_asset(&env.portfolio_state(portfolio), ASSET),
            "INV-038 {label} must clear in bounded public work"
        );
    }

    let group = env.market_state().1;
    let loss_state = env.portfolio_state(loss);
    let counterparty_state = env.portfolio_state(asset1_counterparty);
    assert_eq!(group.vault as u64, env.token_amount(env.vault));
    SocialLossPartitionOutcome {
        frame: SocialLossPartitionFrame {
            asset0: group.assets[0],
            asset1: group.assets[ASSET],
            market_mode: group.mode,
            bankruptcy_hlock_active: group.bankruptcy_hlock_active,
            vault: group.vault,
            c_tot: group.c_tot,
            insurance: group.insurance,
            loss_capital: loss_state.capital.get(),
            loss_pnl: loss_state.pnl.get(),
            loss_active_bitmap: active_bitmap(&loss_state),
            loss_close: close_progress(&loss_state),
            counterparty_capital: counterparty_state.capital.get(),
            counterparty_pnl: counterparty_state.pnl.get(),
            counterparty_active_bitmap: active_bitmap(&counterparty_state),
            counterparty_close: close_progress(&counterparty_state),
            spl_vault: env.token_amount(env.vault),
        },
        initial_residual,
        booked_atoms,
        settled_atoms,
        booking_calls,
        settlement_calls,
        cleanup_calls,
        max_compute_units,
    }
}

#[test]
fn v16_program_social_loss_aggregate_and_chunked_routes_converge_exactly() {
    let aggregate = run_social_loss_partition_schedule(percolator::MAX_VAULT_TVL);
    let chunked = run_social_loss_partition_schedule(1);

    assert_eq!(aggregate.initial_residual, chunked.initial_residual);
    assert_eq!(aggregate.booked_atoms, chunked.booked_atoms);
    assert_eq!(aggregate.settled_atoms, chunked.settled_atoms);
    assert_eq!(aggregate.frame, chunked.frame);
    assert_eq!(aggregate.booking_calls, 1);
    assert!(chunked.booking_calls > aggregate.booking_calls);
    assert_eq!(aggregate.settlement_calls, 1);
    assert!(chunked.settlement_calls > aggregate.settlement_calls);
    assert!(aggregate.cleanup_calls > 0 && chunked.cleanup_calls > 0);
    assert!(aggregate.max_compute_units < support::v16_svm::TX_CU_LIMIT);
    assert!(chunked.max_compute_units < support::v16_svm::TX_CU_LIMIT);
}

#[test]
fn v16_program_truncating_arithmetic_surface_has_a_semantic_owner() {
    #[derive(Clone, Copy)]
    struct RoundingOwner {
        function: &'static str,
        operations: usize,
        class: &'static str,
        evidence: &'static str,
    }

    const ROWS: &[RoundingOwner] = &[
        RoundingOwner {
            function: "accrue_asset_to_not_atomic",
            operations: 1,
            class: "ENGINE",
            evidence: "INV-085",
        },
        RoundingOwner {
            function: "accumulate_fee_to_domain_budget_credits",
            operations: 2,
            class: "EXACT_PARTITION",
            evidence: "v16_program_public_odd_atom_partitions_conserve_every_atom",
        },
        RoundingOwner {
            function: "asset_index",
            operations: 1,
            class: "STRUCTURAL",
            evidence: "INV-034",
        },
        RoundingOwner {
            function: "backing_domain_parts_view",
            operations: 2,
            class: "STRUCTURAL",
            evidence: "INV-034",
        },
        RoundingOwner {
            function: "backing_fee_policy_for_domain_view",
            operations: 2,
            class: "STRUCTURAL",
            evidence: "INV-036",
        },
        RoundingOwner {
            function: "backing_unavailable_principal_atoms",
            operations: 1,
            class: "CUMULATIVE_FLOOR",
            evidence: "v16_bpf_backing_residual_reward_counter_covers_all_trade_paths",
        },
        RoundingOwner {
            function: "ceil_div_u128",
            operations: 2,
            class: "POLICY",
            evidence: "v16_program_canonical_arithmetic_matches_bigint_on_full_width_boundaries",
        },
        RoundingOwner {
            function: "clamp_toward_engine_dt",
            operations: 1,
            class: "ORACLE",
            evidence:
                "v16_program_pr365_fractional_cap_reaches_target_and_preserves_terminal_payouts",
        },
        RoundingOwner {
            function: "collected_fee_supported_mark",
            operations: 2,
            class: "POLICY",
            evidence: "v16_program_policy_arithmetic_matches_independent_full_width_corpus",
        },
        RoundingOwner {
            function: "compose_price_e6",
            operations: 2,
            class: "ORACLE",
            evidence: "v16_program_composite_epoch_coherence_crosses_all_providers_and_transforms",
        },
        RoundingOwner {
            function: "credit_fee_to_domain_budget_view",
            operations: 1,
            class: "STRUCTURAL",
            evidence: "INV-036",
        },
        RoundingOwner {
            function: "credit_market_insurance_budget_view",
            operations: 1,
            class: "EXACT_PARTITION",
            evidence: "v16_attack_fee_redirect_split_lands_correctly",
        },
        RoundingOwner {
            function: "deposit_market_zero_insurance_view",
            operations: 1,
            class: "EXACT_PARTITION",
            evidence: "v16_program_public_odd_atom_partitions_conserve_every_atom",
        },
        RoundingOwner {
            function: "domain_authorities_from_view",
            operations: 1,
            class: "STRUCTURAL",
            evidence: "INV-034",
        },
        RoundingOwner {
            function: "ensure_source_credit_full_rate_for_domain_view",
            operations: 2,
            class: "STRUCTURAL",
            evidence: "INV-030",
        },
        RoundingOwner {
            function: "ewma_effective_alpha_bps",
            operations: 1,
            class: "POLICY",
            evidence: "v16_program_policy_arithmetic_matches_independent_full_width_corpus",
        },
        RoundingOwner {
            function: "ewma_update",
            operations: 3,
            class: "POLICY",
            evidence: "v16_program_policy_arithmetic_matches_independent_full_width_corpus",
        },
        RoundingOwner {
            function: "fee_bps_for_two_sided_fee_paid",
            operations: 1,
            class: "POLICY",
            evidence: "v16_program_policy_arithmetic_matches_independent_full_width_corpus",
        },
        RoundingOwner {
            function: "handle_top_up_backing_bucket",
            operations: 8,
            class: "STRUCTURAL",
            evidence: "INV-002",
        },
        RoundingOwner {
            function: "handle_update_backing_fee_policy",
            operations: 3,
            class: "STRUCTURAL",
            evidence: "INV-036",
        },
        RoundingOwner {
            function: "handle_withdraw_backing_bucket",
            operations: 2,
            class: "STRUCTURAL",
            evidence: "INV-032",
        },
        RoundingOwner {
            function: "handle_withdraw_backing_bucket_earnings",
            operations: 2,
            class: "STRUCTURAL",
            evidence: "INV-036",
        },
        RoundingOwner {
            function: "hybrid_trade_fee_quote_view",
            operations: 1,
            class: "POLICY",
            evidence: "v16_program_policy_arithmetic_matches_independent_full_width_corpus",
        },
        RoundingOwner {
            function: "live_domain_withdraw_health_or_shutdown_view",
            operations: 2,
            class: "STRUCTURAL",
            evidence: "INV-064",
        },
        RoundingOwner {
            function: "mul_div_u128_by_u64",
            operations: 4,
            class: "POLICY",
            evidence: "v16_program_canonical_arithmetic_matches_bigint_on_full_width_boundaries",
        },
        RoundingOwner {
            function: "permissionless_market_init_fee_for_asset",
            operations: 1,
            class: "POLICY",
            evidence: "v16_attack_permissionless_create_fee_funds_asset0_insurance",
        },
        RoundingOwner {
            function: "premium_funding_rate_e9",
            operations: 1,
            class: "POLICY",
            evidence: "v16_program_policy_arithmetic_matches_independent_full_width_corpus",
        },
        RoundingOwner {
            function: "price_move_bps_ceil",
            operations: 1,
            class: "POLICY",
            evidence: "v16_program_policy_arithmetic_matches_independent_full_width_corpus",
        },
        RoundingOwner {
            function: "read_pyth_price_e6_from_bytes",
            operations: 1,
            class: "ORACLE",
            evidence: "v16_program_composite_epoch_coherence_crosses_all_providers_and_transforms",
        },
        RoundingOwner {
            function: "reject_lapsed_source_backing_for_conversion_view",
            operations: 2,
            class: "STRUCTURAL",
            evidence: "INV-063",
        },
        RoundingOwner {
            function: "require_domain_accepts_live_topup_view",
            operations: 1,
            class: "STRUCTURAL",
            evidence: "INV-034",
        },
        RoundingOwner {
            function: "scale_decimal_to_e6",
            operations: 1,
            class: "ORACLE",
            evidence: "v16_program_composite_epoch_coherence_crosses_all_providers_and_transforms",
        },
        RoundingOwner {
            function: "trade_fee_budgeted_amounts_with_mark_externality_view",
            operations: 1,
            class: "EXACT_PARTITION",
            evidence:
                "v16_program_pr225_mark_movement_fee_is_nonwithdrawable_and_terminally_burned",
        },
        RoundingOwner {
            function: "validate_switchboard_observation_e6",
            operations: 1,
            class: "ORACLE",
            evidence: "v16_program_composite_epoch_coherence_crosses_all_providers_and_transforms",
        },
        RoundingOwner {
            function: "verify_domain_withdrawal_preflight",
            operations: 1,
            class: "STRUCTURAL",
            evidence: "INV-032",
        },
        RoundingOwner {
            function: "write_market_wire",
            operations: 2,
            class: "AGGREGATE_CEIL",
            evidence:
                "v16_program_value_routes_reconcile_vault_capital_insurance_and_backing_stocks",
        },
    ];

    let production = include_str!("../../../src/v16_program.rs");
    let production = production
        .split("    #[cfg(test)]\n    mod tests")
        .next()
        .expect("production prefix exists");
    let mut current_function = "<module>";
    let mut actual = std::collections::BTreeMap::<String, usize>::new();
    for line in production.lines() {
        let code = line.split("//").next().unwrap_or(line);
        let trimmed = code.trim_start();
        if let Some(fn_offset) = trimmed.find("fn ") {
            let prefix = &trimmed[..fn_offset];
            if prefix.is_empty() || prefix.starts_with("pub") || prefix.starts_with("unsafe") {
                let rest = &trimmed[fn_offset + 3..];
                let end = rest
                    .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                    .unwrap_or(rest.len());
                current_function = &rest[..end];
            }
        }
        let truncating_operations = code.matches(" / ").count()
            + code.matches(" % ").count()
            + code.matches(".checked_div(").count();
        if truncating_operations != 0 {
            *actual.entry(current_function.to_owned()).or_default() += truncating_operations;
        }
    }

    let witness_sources = [
        include_str!("inv_020_authenticated_clock_slot_and_oracle_provenance.rs"),
        include_str!("inv_025_exact_stock_reconciliation.rs"),
        include_str!("inv_036_fee_destination_and_policy_version_integrity.rs"),
        include_str!("inv_037_exact_residual_partition.rs"),
        include_str!("inv_038_rounding_and_ratio_conservation.rs"),
        include_str!("inv_085_proven_arithmetic_equals_deployed_arithmetic.rs"),
        include_str!("../public_sbf/inv_038_rounding_and_ratio_conservation.rs"),
        include_str!("../public_sbf/inv_045_no_free_mark_movement.rs"),
    ];
    let mut expected = std::collections::BTreeMap::new();
    for row in ROWS {
        assert!(
            expected
                .insert(row.function.to_owned(), row.operations)
                .is_none(),
            "duplicate truncating-arithmetic owner {}",
            row.function
        );
        match row.class {
            "ENGINE" | "STRUCTURAL" => assert!(row.evidence.starts_with("INV-")),
            "AGGREGATE_CEIL" | "ORACLE" | "POLICY" | "EXACT_PARTITION" | "CUMULATIVE_FLOOR" => {
                assert!(
                    witness_sources
                        .iter()
                        .any(|source| source.contains(&format!("fn {}", row.evidence))),
                    "{} lacks executable semantic rounding evidence {}",
                    row.function,
                    row.evidence
                )
            }
            other => panic!("unknown truncating-arithmetic ownership class {other}"),
        }
    }
    assert_eq!(
        actual, expected,
        "every production division/modulo site needs one semantic residue owner"
    );
}

#[test]
fn v16_program_public_odd_atom_partitions_conserve_every_atom() {
    const ODD_ACTIVATION_FEE: u128 = 41;
    let mut activation = V16CuEnv::new();
    activation.update_market_init_fee_policy_with_cu(ODD_ACTIVATION_FEE);
    activation.svm.warp_to_slot(1);
    let before = activation.market_state().1;
    let creator = Keypair::new();
    let authority = Keypair::new();
    let market_authority = activation.admin.pubkey();
    let (_, activation_cu) = activation.activate_permissionless_asset_with_fee(
        &creator,
        1,
        1,
        100,
        authority.pubkey(),
        authority.pubkey(),
        authority.pubkey(),
        market_authority,
        ODD_ACTIVATION_FEE,
    );
    assert_cu_within(
        "INV-038 odd activation-fee partition",
        activation_cu,
        CUSTODY_CU_LIMIT,
    );
    let after = activation.market_state().1;
    let activation_long = after.insurance_domain_budget[0] - before.insurance_domain_budget[0];
    let activation_short = after.insurance_domain_budget[1] - before.insurance_domain_budget[1];
    assert_eq!((activation_long, activation_short), (20, 21));
    assert_eq!(activation_long + activation_short, ODD_ACTIVATION_FEE);
    assert_eq!(after.insurance - before.insurance, ODD_ACTIVATION_FEE);
    assert_eq!(after.vault - before.vault, ODD_ACTIVATION_FEE);
    assert_eq!(
        after.vault as u64,
        activation.token_amount(activation.vault)
    );

    const PRICE: u64 = 100;
    const FEE_BPS: u64 = 100;
    assert_eq!(
        percolator_prog::policy_v16::batch_leg_fee(POS_SCALE, PRICE, FEE_BPS),
        Some(1),
        "each side must contribute one odd redirect atom"
    );
    let mut batch = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    batch.configure_auth_mark_for_asset_as_admin(1, 1, PRICE);
    batch.update_fee_redirect_policy_with_cu(10_000);
    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker = batch.create_portfolio(&taker_owner);
    let lp = batch.create_portfolio(&lp_owner);
    batch.deposit(&taker_owner, taker, 1_000_000);
    batch.deposit(&lp_owner, lp, 1_000_000);
    let before = batch.market_state().1;
    batch.svm.expire_blockhash();
    let batch_cu = batch
        .send(
            batch.batch_trade_no_cpi_ix(
                taker,
                lp,
                vec![BatchTradeLeg {
                    asset_index: 1,
                    market_id: first_generation_market_id(1),
                    size_q: POS_SCALE as i128,
                    exec_price: PRICE,
                    fee_bps: FEE_BPS,
                }],
            ),
            vec![
                AccountMeta::new(taker_owner.pubkey(), true),
                AccountMeta::new(lp_owner.pubkey(), true),
                AccountMeta::new(batch.market, false),
                AccountMeta::new(taker, false),
                AccountMeta::new(lp, false),
            ],
            &[&taker_owner, &lp_owner],
        )
        .expect("odd-atom batch redirect must execute");
    assert_cu_within(
        "INV-038 odd batch fee-redirect partition",
        batch_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );
    let after = batch.market_state().1;
    let deltas: Vec<u128> = (0..4)
        .map(|domain| {
            after.insurance_domain_budget[domain] - before.insurance_domain_budget[domain]
        })
        .collect();
    assert_eq!(deltas, vec![0, 2, 0, 0]);
    assert_eq!(deltas.iter().sum::<u128>(), 2);
    assert_eq!(after.insurance - before.insurance, 2);
    assert_eq!(after.vault, before.vault);
    assert_eq!(after.vault as u64, batch.token_amount(batch.vault));
}

// security.md sweep — rounding asymmetry (#37 dust): trade fees must round UP (ceil, protocol favor)
// so dust-notional trades are never free and repeated churn never leaks value to the trader. Attacker
// success = a fee that floors to 0 (free trade) or insurance that fails to grow on a fee'd dust trade.
#[test]
fn v16_attack_trade_fee_rounds_up_no_free_dust_trades() {
    let mut env = V16CuEnv::new(); // max_trading_fee_bps = 10_000
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    let ins = |env: &V16CuEnv| env.market_state().1.insurance;
    // dust notional with the smallest nonzero fee: notional = size*price/POS_SCALE.
    // size = POS_SCALE/100 @ price 100 => notional = 1; fee_bps=1 => true fee = 0.0001 -> must ceil to >=1.
    let dust_size = (POS_SCALE / 100) as i128;
    let mut prev_ins = ins(&env);
    let mut opened: i128 = 0;
    for i in 0..5 {
        env.svm.expire_blockhash();
        let r = env.try_trade_asset_with_cu(0, &la, pa, &lb, pb, dust_size, 100, 1);
        if r.is_err() {
            break;
        } // if dust trade is rejected outright, that's also safe (no free trade)
        opened += dust_size;
        let now = ins(&env);
        assert!(
            now > prev_ins,
            "dust trade #{} charged a nonzero fee (insurance grew {} -> {})",
            i,
            prev_ins,
            now
        );
        prev_ins = now;
        // conservation after each dust trade.
        let (_, g) = env.market_state();
        assert_eq!(g.vault, 2_000_000, "no value created by dust trade");
        assert_eq!(g.vault, g.c_tot + g.insurance, "exact conservation");
    }
    assert!(opened > 0, "at least one dust trade executed (non-vacuous)");
    // close the accumulated dust position; conservation still exact, insurance only grew.
    if opened > 0 {
        env.svm.expire_blockhash();
        env.try_trade_asset_with_cu(0, &la, pa, &lb, pb, -opened, 100, 0)
            .expect("close the accumulated dust position");
        assert_eq!(
            env.portfolio_state(pa).legs[0].basis_pos_q.get(),
            0,
            "the dust round trip must actually close the long leg"
        );
        assert_eq!(
            env.portfolio_state(pb).legs[0].basis_pos_q.get(),
            0,
            "the dust round trip must actually close the short leg"
        );
    }
    let (_, g) = env.market_state();
    assert_eq!(g.vault, 2_000_000, "vault conserved across dust churn");
    assert_eq!(
        g.vault,
        g.c_tot + g.insurance,
        "exact conservation after close"
    );
    assert!(
        g.insurance >= prev_ins,
        "insurance never decreased (fees are protocol-favorable)"
    );
}

// security.md sweep — fee-splitting dust below one atom: a caller can split a real position into
// fills whose floor notional is zero. The public TradeNoCpi path must still charge nonzero fees for
// nonzero risk-changing fills, otherwise repeated slices grow OI without paying the configured fee.
#[test]
fn v16_attack_subatom_fee_splits_cannot_accumulate_free_position() {
    let mut env = V16CuEnv::new(); // max_trading_fee_bps = 10_000
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);

    let sub_atom_size = (POS_SCALE / 100 - 1) as i128;
    assert!(sub_atom_size > 0, "probe size is nonzero");
    let mut opened = 0i128;
    let mut prev_insurance = env.market_state().1.insurance;
    for i in 0..5 {
        env.svm.expire_blockhash();
        env.try_trade_asset_with_cu(0, &la, pa, &lb, pb, sub_atom_size, 100, 1)
            .unwrap_or_else(|e| panic!("sub-atom public trade #{i} must execute, got {e}"));
        opened += sub_atom_size;
        let (_, group) = env.market_state();
        assert!(
            group.insurance > prev_insurance,
            "accepted sub-atom trade #{i} grew OI by {sub_atom_size}q but paid no fee"
        );
        assert_eq!(group.vault, 2_000_000, "no value created by sub-atom fill");
        assert_eq!(
            group.vault,
            group.c_tot + group.insurance,
            "exact conservation"
        );
        prev_insurance = group.insurance;
    }

    let taker = env.portfolio_state(pa);
    let lp = env.portfolio_state(pb);
    assert!(
        opened > (POS_SCALE / 100) as i128,
        "accepted sub-atom fills accumulate into a real position"
    );
    assert_eq!(active_leg_for_asset(&taker, 0).basis_pos_q, opened);
    assert_eq!(active_leg_for_asset(&lp, 0).basis_pos_q, -opened);
}

// Same fee invariant through BatchTradeNoCpi. This also exercises the wrapper's per-leg fee
// reconstruction: a stale floor-notional mirror computes zero and fails to match the fixed engine's
// aggregate fee outcome for a sub-atom leg.
#[test]
fn v16_attack_batch_subatom_fee_reconstruction_uses_ceil_notional() {
    let mut env = V16CuEnv::new(); // max_trading_fee_bps = 10_000
    let taker = Keypair::new();
    let lp = Keypair::new();
    let taker_account = env.create_portfolio(&taker);
    let lp_account = env.create_portfolio(&lp);
    env.deposit(&taker, taker_account, 1_000_000);
    env.deposit(&lp, lp_account, 1_000_000);

    let sub_atom_size = (POS_SCALE / 100 - 1) as i128;
    let before_insurance = env.market_state().1.insurance;
    env.send(
        env.batch_trade_no_cpi_ix(
            taker_account,
            lp_account,
            vec![BatchTradeLeg {
                asset_index: 0,
                market_id: first_generation_market_id((0) as u16),
                size_q: sub_atom_size,
                exec_price: 100,
                fee_bps: 1,
            }],
        ),
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(lp.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker_account, false),
            AccountMeta::new(lp_account, false),
        ],
        &[&taker, &lp],
    )
    .expect("sub-atom BatchTradeNoCpi must execute with matching fee reconstruction");

    let (_, group) = env.market_state();
    let expected_fee_per_side =
        percolator_prog::policy_v16::batch_leg_fee(sub_atom_size.unsigned_abs(), 100, 1)
            .expect("canonical batch fee arithmetic");
    assert_eq!(
        group.insurance - before_insurance,
        expected_fee_per_side * 2,
        "deployed batch accounting must charge the canonical ceil fee to both sides"
    );
    assert!(
        expected_fee_per_side > 0,
        "the rounding edge is non-vacuous"
    );
    assert_eq!(group.vault, 2_000_000, "no value created by sub-atom batch");
    assert_eq!(
        group.vault,
        group.c_tot + group.insurance,
        "exact conservation"
    );
    let taker_after = env.portfolio_state(taker_account);
    let lp_after = env.portfolio_state(lp_account);
    assert_eq!(
        active_leg_for_asset(&taker_after, 0).basis_pos_q,
        sub_atom_size
    );
    assert_eq!(
        active_leg_for_asset(&lp_after, 0).basis_pos_q,
        -sub_atom_size
    );
}

// security.md sweep — funding cap precision (#19 DoS): an extreme mark premium must be clamped to
// max_abs_funding_e9_per_slot. If funding scaled with the raw premium, a tiny mark push could drain
// a counterparty arbitrarily fast. Decisive check: a 2x-index premium and a 1000x-index premium must
// accrue IDENTICAL funding (both pinned to the cap), and value stays conserved.
#[test]
fn v16_attack_extreme_premium_funding_is_capped() {
    const INITIAL_PRICE: u64 = 1_000_000;
    const DEPOSIT: u128 = 100_000_000;
    fn run_scenario(mark_mult: u64) -> (i128, u128, u128) {
        let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
            initial_price: INITIAL_PRICE,
            max_price_move_bps_per_slot: 1_000,
            max_accrual_dt_slots: 1,
            max_abs_funding_e9_per_slot: 1_000,
            min_funding_lifetime_slots: 1,
            ..V16CuMarketParams::default()
        });
        env.svm.warp_to_slot(0);
        env.configure_ewma_mark_with_cu(0, INITIAL_PRICE, 1, 0);
        let lo_owner = Keypair::new();
        let lo = env.create_portfolio(&lo_owner);
        let sh_owner = Keypair::new();
        let sh = env.create_portfolio(&sh_owner);
        env.deposit(&lo_owner, lo, DEPOSIT);
        env.deposit(&sh_owner, sh, DEPOSIT);
        env.trade_with_cu(
            &lo_owner,
            lo,
            &sh_owner,
            sh,
            POS_SCALE as i128,
            INITIAL_PRICE,
            0,
        );
        env.svm.warp_to_slot(1);
        env.push_ewma_mark_with_cu(1, INITIAL_PRICE.saturating_mul(mark_mult)); // premium
        for slot in 1..=4u64 {
            env.svm.warp_to_slot(slot);
            for p in [lo, sh] {
                let _ = env.send_crank_if_actionable(
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: crank_observations(0),
                    },
                    vec![
                        AccountMeta::new(env.payer.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(p, false),
                    ],
                    &[],
                );
            }
        }
        let (_, g) = env.market_state();
        (g.assets[0].f_long_num, g.vault, g.c_tot + g.insurance)
    }
    let (f2, vault2, senior2) = run_scenario(2); // 2x index premium
    let (f1000, vault1000, senior1000) = run_scenario(1000); // 1000x index premium (capped at clamp)
    assert!(f2 != 0, "funding actually accrued (non-vacuous)");
    // CRUX: extreme premium yields the SAME funding as the moderate one — both pinned to the cap.
    assert_eq!(
        f2, f1000,
        "extreme premium funding is clamped to the cap (identical to moderate)"
    );
    // conservation in both runs.
    assert_eq!(vault2, 2 * DEPOSIT, "scenario 2x: vault conserved");
    assert_eq!(vault1000, 2 * DEPOSIT, "scenario 1000x: vault conserved");
    assert!(
        vault2 >= senior2 && vault1000 >= senior1000,
        "senior conservation in both"
    );
}

// security.md sweep — funding direction symmetry (#33/#9): with the mark BELOW the index (opposite of
// batch 19), funding must flow the other way (shorts pay longs) and still be value-conserving. Probes
// the negative-premium branch of premium_funding_rate_e9.
#[test]
fn v16_attack_funding_direction_mark_below_index_conserves() {
    const INITIAL_PRICE: u64 = 1_000_000;
    const DEPOSIT: u128 = 10_000_000;
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: INITIAL_PRICE,
        max_price_move_bps_per_slot: 1_000,
        max_accrual_dt_slots: 1,
        max_abs_funding_e9_per_slot: 1_000,
        min_funding_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(0);
    env.configure_ewma_mark_with_cu(0, INITIAL_PRICE, 1, 0);
    let lo_owner = Keypair::new();
    let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new();
    let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, DEPOSIT);
    env.deposit(&sh_owner, sh, DEPOSIT);
    env.trade_with_cu(
        &lo_owner,
        lo,
        &sh_owner,
        sh,
        POS_SCALE as i128,
        INITIAL_PRICE,
        0,
    );
    // push the mark BELOW the index, then let funding accrue (no re-push).
    env.svm.warp_to_slot(1);
    env.push_ewma_mark_with_cu(1, INITIAL_PRICE / 2);
    for slot in 1..=5u64 {
        env.svm.warp_to_slot(slot);
        for p in [lo, sh] {
            let _ = env.send_crank_if_actionable(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(p, false),
                ],
                &[],
            );
        }
    }
    let a = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    let b = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
    let (_, g) = env.market_state();
    assert!(
        g.assets[0].f_long_num != 0 || g.assets[0].f_short_num != 0,
        "funding accrued (non-vacuous)"
    );
    // mark < index => OPPOSITE direction from batch 19 (where longs were charged): longs are credited.
    assert!(
        g.assets[0].f_long_num > 0 && g.assets[0].f_short_num < 0,
        "mark < index => shorts pay longs (f_long>0, f_short<0)"
    );
    // value conservation (same widened invariant as batch 19).
    assert_eq!(g.vault, 2 * DEPOSIT, "no tokens minted/burned");
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    let residual = g.vault as i128 - g.c_tot as i128 - g.insurance as i128;
    assert!(
        residual >= a.pnl.get().max(0) + b.pnl.get().max(0),
        "positive pnl backed by residual"
    );
    let total_equity =
        (a.capital.get() as i128 + a.pnl.get()) + (b.capital.get() as i128 + b.pnl.get());
    assert!(
        total_equity + g.insurance as i128 <= g.vault as i128,
        "no over-distribution"
    );
}

// Product spec — force-shutdown with a timeout so traders can exit (no rug): marketauth can
// shut down any asset (ASSET_ACTION_SHUTDOWN -> RECOVERY with a frozen mark), but the permissionless
// force-close (which winds the asset down) is gated behind force_close_delay_slots. So there is a
// window after shutdown during which the asset is NOT yet force-closed — traders can exit — and only
// after the delay can the wind-down proceed. Asserts: shutdown -> RECOVERY; force-close REJECTS before
// lifecycle sweep — explicit DrainOnly is a public UpdateAssetLifecycle action, distinct from
// shutdown/recovery. It must be marketauth-gated, reject malformed slot/price args, block new risk,
// security.md sweep — §6.2 backing-yield fee split (#5): when a risk-increasing trade GROWS an
// account's source-credit IM lien that draws a fresh counterparty backing bucket, the wrapper charges
// a backing-domain trade fee = fee_bps * Δbacking and splits it three ways: insurance_share to the
// market insurance pool (asset-0), the SAME amount earmarked into that domain's insurance budget, and
// the remainder to the backing provider's bucket earnings. Verify the split conserves on real BPF
// state: charged == insurance_pool_delta + provider_delta, and the domain budget mirrors the insurance
// share. The lien is formed organically (cross-margin positive PnL used as IM), not injected.
#[test]
fn v16_attack_backing_fee_split_conserves() {
    const INITIAL_PRICE: u64 = 100;
    const ASSET0_SIZE_Q: i128 = 200 * POS_SCALE as i128;
    const ASSET1_SIZE_Q: i128 = 100 * POS_SCALE as i128;
    const SAFE_INCREASE_Q: i128 = 20 * POS_SCALE as i128;
    const DEPOSIT: u128 = 3_130;
    const WINNING_DOMAIN: usize = 1;
    const FEE_BPS: u16 = 5_000; // 10% of consumed backing
    const INSURANCE_SHARE_BPS: u16 = 2_500; // 25% of the fee to insurance, 75% to provider
    const EXPECTED_SOURCE_CLAIM: u128 = 1_000;
    const EXPECTED_GROSS_SOURCE_PNL: i128 = 1_000;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(4, 1_000, 1_000, 500);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, INITIAL_PRICE);
    env.configure_auth_mark_for_asset_as_admin(1, 1, INITIAL_PRICE);
    env.update_backing_fee_policy_with_cu(WINNING_DOMAIN as u16, FEE_BPS, INSURANCE_SHARE_BPS);
    // Reconfigure the asset-0 oracle after setting the policy. Asset 0 carries both market-wide
    // config and a stored per-asset profile; the fee policy must survive this path because the
    // backing-fee collector reads the stored profile.
    env.svm.expire_blockhash();
    env.configure_auth_mark_for_asset_as_admin(0, 1, INITIAL_PRICE);

    let cross_owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let cross_account = env.create_portfolio(&cross_owner);
    let counterparty_account = env.create_portfolio(&counterparty_owner);
    env.deposit(&cross_owner, cross_account, DEPOSIT);
    env.deposit(&counterparty_owner, counterparty_account, 10_000);
    env.top_up_backing_bucket(WINNING_DOMAIN as u16, 1_500, 10);

    // Build cross_account's source-backed positive PnL on asset0 (a long that wins as the mark rises).
    env.trade_asset_with_cu(
        0,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        ASSET0_SIZE_Q,
        INITIAL_PRICE,
        0,
    );
    env.trade_asset_with_cu(
        1,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        ASSET1_SIZE_Q,
        INITIAL_PRICE,
        0,
    );
    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, 105);
    env.push_auth_mark_for_asset_as_admin(1, 2, 95);
    for (portfolio, asset_index) in [
        (counterparty_account, 0),
        (cross_account, 0),
        (counterparty_account, 1),
    ] {
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations_for_assets(&[asset_index, 1 - asset_index]),
            },
        );
    }
    // Force capital low so the parked positive-PnL claim is NEEDED as IM (triggers the source lien).
    env.force_portfolio_capital_for_benchmark(cross_account, 2_600);
    assert_eq!(
        env.portfolio_state(cross_account).pnl.get(),
        EXPECTED_GROSS_SOURCE_PNL,
        "setup must retain gross source-attributed PnL after complete refresh"
    );

    // Trim the bucket to the exact watermark, then refill generously so the risk increase liens against
    // fresh counterparty backing.
    let (_, g0) = env.market_state();
    assert_eq!(
        g0.source_credit[WINNING_DOMAIN].positive_claim_bound_num,
        EXPECTED_SOURCE_CLAIM * BOUND_SCALE,
        "the source-domain claim must preserve gross attributable positive PnL",
    );
    let surplus = (g0.source_credit[WINNING_DOMAIN].fresh_reserved_backing_num
        - g0.source_credit[WINNING_DOMAIN].positive_claim_bound_num)
        / BOUND_SCALE;
    if surplus > 0 {
        let dest = env.token_account(env.admin.pubkey(), 0);
        env.withdraw_backing_bucket_to_admin_token_with_cu(dest, WINNING_DOMAIN as u16, surplus);
    }
    env.top_up_backing_bucket(WINNING_DOMAIN as u16, 50_000, 10);
    env.deposit(&cross_owner, cross_account, 500);
    env.deposit(&counterparty_owner, counterparty_account, 500);
    env.svm.warp_to_slot(3);

    // Snapshot the three sinks + the payers before the lien-growing trade.
    let (_, gb) = env.market_state();
    let insurance_before = gb.insurance;
    let budget_before = gb.insurance_domain_budget[WINNING_DOMAIN];
    let provider_before = gb.source_backing_buckets[WINNING_DOMAIN].utilization_fee_earnings;
    let ctot_before = gb.c_tot;
    let cap_cross_before = env.portfolio_state(cross_account).capital.get();
    let cap_cp_before = env.portfolio_state(counterparty_account).capital.get();
    let lien_before: u128 = env
        .portfolio_state(cross_account)
        .source_domains
        .iter()
        .map(|slot| slot.source_lien_counterparty_backing_num.get())
        .sum();

    // Risk-increasing trade: grows the IM source-credit lien -> draws fresh backing -> charges the fee.
    let r = env.try_trade_asset_with_backing_fee_cap_with_cu(
        1,
        &cross_owner,
        cross_account,
        &counterparty_owner,
        counterparty_account,
        SAFE_INCREASE_Q,
        95,
        0,
        FEE_BPS,
    );
    assert!(r.is_ok(), "backed risk increase must succeed: {r:?}");

    let (_, ga) = env.market_state();
    let insurance_delta = ga.insurance - insurance_before;
    let budget_delta = ga.insurance_domain_budget[WINNING_DOMAIN] - budget_before;
    let provider_delta =
        ga.source_backing_buckets[WINNING_DOMAIN].utilization_fee_earnings - provider_before;
    let cap_cross_after = env.portfolio_state(cross_account).capital.get();
    let cap_cp_after = env.portfolio_state(counterparty_account).capital.get();
    let lien_after: u128 = env
        .portfolio_state(cross_account)
        .source_domains
        .iter()
        .map(|slot| slot.source_lien_counterparty_backing_num.get())
        .sum();
    let charged = (cap_cross_before - cap_cross_after) + (cap_cp_before - cap_cp_after);

    // The trade must actually grow the counterparty-backing lien (else the fee is vacuous).
    assert!(
        lien_after > lien_before,
        "trade must grow the counterparty-backing lien (before={lien_before} after={lien_after})"
    );
    assert!(charged > 0, "a positive backing fee must be charged");
    // Route 1+2+3: charged splits exactly into insurance pool + provider earnings, no atoms created/lost.
    assert_eq!(insurance_delta + provider_delta, charged, "fee must split with no leakage: ins={insurance_delta} prov={provider_delta} charged={charged}");
    // Insurance share is floor(charged * share_bps): asset-0 pool and the per-domain budget move together.
    let expected_insurance = charged * INSURANCE_SHARE_BPS as u128 / 10_000;
    assert_eq!(
        insurance_delta, expected_insurance,
        "insurance pool gets floor(fee*share)"
    );
    assert_eq!(
        budget_delta, insurance_delta,
        "per-domain insurance budget mirrors the insurance share"
    );
    // c_tot drops by exactly the fee debited from collateral (insurance + provider leave c_tot).
    assert_eq!(
        ctot_before - ga.c_tot,
        charged,
        "c_tot decreases by exactly the charged fee"
    );
    assert_domain_budget_remaining_total_consistent(&ga, "backing fee insurance share");
}

#[test]
fn v16_bpf_permissionless_crank_computes_funding_from_internal_mark_premium() {
    const INITIAL_PRICE: u64 = 1_000_000;
    const DEPOSIT: u128 = 10_000_000;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: INITIAL_PRICE,
        max_price_move_bps_per_slot: 1_000,
        max_accrual_dt_slots: 1,
        max_abs_funding_e9_per_slot: 1_000,
        min_funding_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(0);
    env.configure_ewma_mark_with_cu(0, INITIAL_PRICE, 1, 0);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, DEPOSIT);
    env.deposit(&short_owner, short_account, DEPOSIT);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        INITIAL_PRICE,
        0,
    );

    env.svm.warp_to_slot(1);
    env.push_ewma_mark_with_cu(1, INITIAL_PRICE * 2);
    env.crank(
        long_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
    );
    env.crank(
        short_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
    );
    let (cfg_after_first, group_after_first) = env.market_state();
    assert_eq!(cfg_after_first.mark_ewma_e6, 1_500_000);
    assert_eq!(group_after_first.assets[0].effective_price, 1_100_000);
    assert_eq!(
        group_after_first.funding_epoch, 0,
        "a newly pushed mark must not retroactively charge funding before its slot"
    );
    assert_eq!(group_after_first.assets[0].f_long_num, 0);
    assert_eq!(group_after_first.assets[0].f_short_num, 0);

    env.svm.warp_to_slot(2);
    let funding_cu = env.crank(
        long_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
    );
    assert_cu_within(
        "permissionless computed funding crank",
        funding_cu,
        CRANK_CU_LIMIT,
    );
    let (_, funded_group) = env.market_state();
    assert_eq!(funded_group.funding_epoch, 1);
    assert_eq!(
        funded_group.assets[0].effective_price, 1_200_000,
        "the two-slot movement envelope is linear from the stable trajectory anchor"
    );
    assert_eq!(funded_group.assets[0].f_long_num, -(ADL_ONE as i128));
    assert_eq!(funded_group.assets[0].f_short_num, ADL_ONE as i128);
}

#[test]
fn v16_bpf_existing_funding_ledger_refreshes_and_converts_between_sides() {
    const INITIAL_PRICE: u64 = 1_000_000;
    const FUNDING_RATE_E9: i128 = 1_000;
    const DEPOSIT: u128 = 2_000_000;

    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, DEPOSIT);
    env.deposit(&short_owner, short_account, DEPOSIT);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        INITIAL_PRICE,
        0,
    );

    env.mutate_market(|_, group| {
        let out = group
            .accrue_asset_to_not_atomic(0, 1, INITIAL_PRICE, FUNDING_RATE_E9, true)
            .unwrap();
        assert!(out.funding_active);
        group.assets[0].raw_oracle_target_price = INITIAL_PRICE;
    });
    env.svm.warp_to_slot(1);
    let (_, funded_group) = env.market_state();
    assert_eq!(funded_group.funding_epoch, 1);
    assert_eq!(funded_group.assets[0].f_long_num, -(ADL_ONE as i128));
    assert_eq!(funded_group.assets[0].f_short_num, ADL_ONE as i128);

    let long_refresh_cu = env.crank(
        long_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
    );
    assert_cu_within(
        "funding smoke long loss refresh",
        long_refresh_cu,
        CRANK_CU_LIMIT,
    );
    let long_after = env.portfolio_state(long_account);
    assert_eq!(long_after.pnl.get(), 0);
    assert_eq!(long_after.capital.get(), DEPOSIT - 1);

    let short_refresh_cu = env.crank(
        short_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
    );
    assert_cu_within(
        "funding smoke short gain refresh",
        short_refresh_cu,
        CRANK_CU_LIMIT,
    );
    let short_after = env.portfolio_state(short_account);
    assert_eq!(short_after.pnl.get(), 1);
    assert_eq!(short_after.capital.get(), DEPOSIT);

    let close_cu = env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        -(POS_SCALE as i128),
        INITIAL_PRICE,
        0,
    );
    assert_cu_within(
        "funding smoke close funded position",
        close_cu,
        TRADE_CU_LIMIT,
    );
    let long_flat = env.portfolio_state(long_account);
    let short_flat = env.portfolio_state(short_account);
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &long_flat
    )));
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &short_flat
    )));
    assert_eq!(long_flat.capital.get(), DEPOSIT - 1);
    assert_eq!(short_flat.pnl.get(), 1);

    let convert_cu = env.convert_released_pnl_with_cu(&short_owner, short_account, 1);
    assert_cu_within(
        "funding smoke convert released pnl",
        convert_cu,
        CUSTODY_CU_LIMIT,
    );
    let short_after_convert = env.portfolio_state(short_account);
    assert_eq!(short_after_convert.pnl.get(), 0);
    assert_eq!(short_after_convert.capital.get(), DEPOSIT + 1);

    let (_, group) = env.market_state();
    assert_eq!(group.c_tot, DEPOSIT * 2);
    assert_eq!(group.vault, DEPOSIT * 2);
}

// regression (security.md sweep): premium-funding + price-move settlement value-conservation.
// Balanced long/short with a persistent mark premium so funding accrues across slots. Probe whether
// funding/price settlement creates or destroys net VAULT value, breaks senior conservation, or
// leaves the winner unbacked. (Initial probe fired on a too-narrow Σ(capital+pnl)==deposits invariant
// — funding fees accrue to insurance and §6.2 warmup holds an in-vault residual; widened below.)
#[test]
fn v16_regression_premium_funding_settlement_conserves_vault() {
    const INITIAL_PRICE: u64 = 1_000_000;
    const DEPOSIT: u128 = 10_000_000;
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: INITIAL_PRICE,
        max_price_move_bps_per_slot: 1_000,
        max_accrual_dt_slots: 1,
        max_abs_funding_e9_per_slot: 1_000,
        min_funding_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(0);
    env.configure_ewma_mark_with_cu(0, INITIAL_PRICE, 1, 0);
    let lo_owner = Keypair::new();
    let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new();
    let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, DEPOSIT);
    env.deposit(&sh_owner, sh, DEPOSIT);
    env.trade_with_cu(
        &lo_owner,
        lo,
        &sh_owner,
        sh,
        POS_SCALE as i128,
        INITIAL_PRICE,
        0,
    );
    // Push the mark premium ONCE at slot 1 (anti-retroactivity: it won't charge funding that slot),
    // then crank subsequent slots WITHOUT re-pushing so the established premium accrues funding.
    env.svm.warp_to_slot(1);
    env.push_ewma_mark_with_cu(1, INITIAL_PRICE * 2);
    let crank_both = |env: &mut V16CuEnv, slot: u64| {
        for acct in [lo, sh] {
            let _ = env.send_crank_if_actionable(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(acct, false),
                ],
                &[],
            );
        }
    };
    crank_both(&mut env, 1);
    for slot in 2..=5u64 {
        env.svm.warp_to_slot(slot);
        crank_both(&mut env, slot);
    }
    let a = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    let b = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
    let (_, g) = env.market_state();
    // funding must actually have accrued (non-vacuous): the ledger moved off zero.
    assert!(
        g.assets[0].f_long_num != 0 || g.assets[0].f_short_num != 0,
        "funding actually accrued"
    );
    // Correct (widened) conservation invariant. NOTE: the account-level sum Σ(capital+pnl) is NOT
    // == deposits here, because funding fees legitimately accrue to INSURANCE and the §6.2 warmup
    // holds a RESIDUAL buffer in-vault before crediting the winner. The real guarantees are:
    //   1) no tokens minted/burned: the vault still holds exactly the deposited amount,
    //   2) senior conservation: vault >= c_tot + insurance,
    //   3) winner backed: positive pnl <= residual,
    //   4) no over-distribution: Σ(capital+pnl) + insurance <= vault.
    assert_eq!(
        g.vault,
        2 * DEPOSIT,
        "no tokens minted or burned: vault == total deposited"
    );
    assert!(
        g.vault >= g.c_tot + g.insurance,
        "senior conservation under funding"
    );
    let residual = g.vault as i128 - g.c_tot as i128 - g.insurance as i128;
    assert!(
        residual >= a.pnl.get().max(0) + b.pnl.get().max(0),
        "positive pnl backed by residual"
    );
    let total_equity =
        (a.capital.get() as i128 + a.pnl.get()) + (b.capital.get() as i128 + b.pnl.get());
    assert!(
        total_equity + g.insurance as i128 <= g.vault as i128,
        "no value over-distributed beyond the vault"
    );
    assert!(
        g.assets[0].f_long_num < 0 && g.assets[0].f_short_num > 0,
        "longs pay shorts under mark premium"
    );
}
