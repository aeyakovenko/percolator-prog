//! INV-067 - Terminal payout completeness and exact-once settlement.
//!
//! Normative obligation: Each valid claim is paid, forfeited, or receipted exactly once without silent loss.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_reported_route_matrix_preserves_terminal_value_partition` compares terminal worlds
//! with and without a one-atom round trip through both reported-price routes. It drains every
//! public close/claim continuation to quiescence and requires unchanged victim payout while the
//! sole residual equals the coalition's one-atom rounding loss. Direct impact tests remain below. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: this certifies one source-haircut composition across all deployed trade
//! routes. It does not replace the broader claim-episode and bounded-reachability work in the
//! invariant roadmap.

use super::*;
use crate::support::v16_svm::{
    assert_closed_market_tombstone, MarketConfig, PublicTerminalClassification, V16Svm, TX_CU_LIMIT,
};
use percolator::{active_bitmap_is_empty, MarketModeV16, POS_SCALE};
use percolator_prog::{constants::HEADER_LEN, ix::CrankObservationHint, state};

fn inv067_resolved_portfolio_is_terminal(env: &V16Svm, actor: usize) -> bool {
    let group = env.primary_market_state().1;
    let account = env.primary_portfolio(actor);
    let Ok(receipt) = account.resolved_payout_receipt.try_to_runtime() else {
        return false;
    };
    let Ok(close) = account.close_progress.try_to_runtime() else {
        return false;
    };
    group.mode == MarketModeV16::Resolved
        && account.capital.get() == 0
        && account.pnl.get() == 0
        && account.reserved_pnl.get() == 0
        && account.fee_credits.get() == 0
        && account.cancel_deposit_escrow.get() == 0
        && active_bitmap_is_empty(state::portfolio_active_bitmap(&account))
        && account.stale_state == 0
        && account.b_stale_state == 0
        && account.rebalance_lock == 0
        && account.liquidation_lock == 0
        && account.last_fee_slot.get() == group.resolved_slot
        && account.health_cert.valid == 0
        && account
            .source_domains
            .iter()
            .all(|source| !source.is_occupied())
        && (!receipt.present || receipt.finalized)
        && (!close.active || (close.finalized && close.residual_remaining == 0))
}

fn inv067_drain_resolved_actor(env: &mut V16Svm, actor: usize, max_cu: &mut u64) {
    for step in 0..16 {
        if inv067_resolved_portfolio_is_terminal(env, actor) {
            return;
        }
        let before_market = env.market_data(false);
        let before_portfolio = env.primary_portfolio_data(actor);
        let success = env
            .close_resolved_primary_signed(actor)
            .unwrap_or_else(|error| panic!("resolved actor {actor} step {step}: {error}"));
        assert!(
            success.compute_units < TX_CU_LIMIT,
            "resolved actor {actor} step {step} consumed {} CU",
            success.compute_units
        );
        *max_cu = (*max_cu).max(success.compute_units);
        assert!(
            env.market_data(false) != before_market
                || env.primary_portfolio_data(actor) != before_portfolio,
            "resolved actor {actor} accepted a nonprogressing step {step}"
        );
    }
    panic!("resolved actor {actor} did not terminate in 16 public steps");
}

/// INV-067: a haircut receipt is not terminal while unrelated fresh backing can later expire into
/// the payout snapshot. This public trace deliberately leaves no second unreceipted claimant: an
/// unsigned close first pays the current haircut, authenticated time then expires unrelated
/// backing, and permissionless terminal calls must deliver that newly released value exactly once
/// instead of deleting the receipt and burning the value during `CloseSlab`.
#[test]
fn v16_program_late_unrelated_backing_cannot_outlive_and_erase_resolved_receipt() {
    const WINNER: usize = 0;
    const LOSER: usize = 1;
    const PROVIDER: usize = 2;
    const CLAIM_ASSET: u16 = 0;
    const UNRELATED_ASSET: u16 = 1;
    const UNRELATED_DOMAIN: u16 = 3;
    const INITIAL_PRICE: u64 = 100;
    const FINAL_PRICE: u64 = 150;
    const SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const UNRELATED_BACKING: u128 = 500;
    const RESOLVE_SLOT: u64 = 12;
    const EXPIRY_SLOT: u64 = 40;
    const EXPECTED_WINNER_PAYOUT: u64 = 1_750;

    let config = MarketConfig {
        initial_price: INITIAL_PRICE,
        maintenance_margin_bps: 1_000,
        initial_margin_bps: 1_000,
        max_price_move_bps_per_slot: 500,
        max_accrual_dt_slots: 1,
        min_funding_lifetime_slots: 1,
        actor_deposits: [1_000, 250, 0, 0, 0],
        ..MarketConfig::default()
    };
    let mut env = V16Svm::new([0x67; 32], config);
    let mint_supply_before = env.mint_supply();
    let mut max_cu = 0u64;

    env.configure_permissionless_resolve(100, 1)
        .expect("configure unsigned terminal progress");
    env.update_asset_authority_from_admin(
        UNRELATED_ASSET,
        percolator_prog::processor::ASSET_AUTH_BACKING_BUCKET,
        PROVIDER,
    )
    .expect("install unrelated backing provider");
    env.top_up_backing_bucket_for_actor(PROVIDER, UNRELATED_DOMAIN, UNRELATED_BACKING, EXPIRY_SLOT)
        .expect("fund unrelated future residual");
    env.trade_no_cpi(WINNER, LOSER, CLAIM_ASSET, SIZE_Q, INITIAL_PRICE, 0)
        .expect("open underfunded matched position");

    for (offset, mark) in (105..=FINAL_PRICE).step_by(5).enumerate() {
        let slot = 2 + u64::try_from(offset).expect("bounded mark sequence");
        env.warp_to_slot(slot);
        env.push_auth_mark(CLAIM_ASSET, slot, mark)
            .unwrap_or_else(|error| panic!("publish mark {mark}: {error}"));
        let oracle_accounts = env.primary_profile(CLAIM_ASSET as usize).oracle_leg_count;
        for actor in [LOSER, WINNER] {
            let success = env
                .crank(
                    actor,
                    slot,
                    vec![CrankObservationHint {
                        asset_index: CLAIM_ASSET,
                        oracle_accounts,
                    }],
                )
                .unwrap_or_else(|error| panic!("refresh actor {actor} at mark {mark}: {error}"));
            max_cu = max_cu.max(success.compute_units);
        }
    }
    env.trade_no_cpi(WINNER, LOSER, CLAIM_ASSET, -SIZE_Q, FINAL_PRICE, 0)
        .expect("flatten underfunded matched position");

    env.warp_to_slot(RESOLVE_SLOT);
    env.resolve_market().expect("resolve market");
    for actor in [LOSER, PROVIDER, 3, 4] {
        inv067_drain_resolved_actor(&mut env, actor, &mut max_cu);
        max_cu = max_cu.max(
            env.close_primary_portfolio(actor)
                .unwrap_or_else(|error| panic!("close terminal actor {actor}: {error}"))
                .compute_units,
        );
    }

    env.warp_to_slot(RESOLVE_SLOT + 2);
    let initial_close = env
        .close_resolved_primary(WINNER)
        .expect("unsigned winner close after configured delay");
    max_cu = max_cu.max(initial_close.compute_units);
    let pre_expiry_receipt = env
        .primary_portfolio(WINNER)
        .resolved_payout_receipt
        .try_to_runtime()
        .expect("decode pre-expiry receipt");
    let pre_expiry_payout = env.token_amount(env.actors[WINNER].destination_token);

    if pre_expiry_receipt.present {
        let market_before = env.market_data(false);
        let portfolio_before = env.primary_portfolio_data(WINNER);
        let destination_before = pre_expiry_payout;
        let vault_before = env.token_amount(env.vault);
        let error = env
            .crank(
                WINNER,
                RESOLVE_SLOT + 2,
                vec![CrankObservationHint {
                    asset_index: UNRELATED_ASSET,
                    oracle_accounts: 0,
                }],
            )
            .expect_err(
                "a backing hint before its committed expiry must not be a successful no-op",
            );
        assert!(
            error.contains("Custom(22)") || error.contains("custom program error: 0x16"),
            "unexpected pre-expiry crank error: {error}"
        );
        assert_eq!(env.market_data(false), market_before);
        assert_eq!(env.primary_portfolio_data(WINNER), portfolio_before);
        assert_eq!(
            env.token_amount(env.actors[WINNER].destination_token),
            destination_before
        );
        assert_eq!(env.token_amount(env.vault), vault_before);
    }

    env.warp_to_slot(EXPIRY_SLOT);
    if pre_expiry_receipt.present {
        for observations in [
            vec![],
            vec![CrankObservationHint {
                asset_index: CLAIM_ASSET,
                oracle_accounts: 0,
            }],
        ] {
            let market_before = env.market_data(false);
            let portfolio_before = env.primary_portfolio_data(WINNER);
            let destination_before = env.token_amount(env.actors[WINNER].destination_token);
            let vault_before = env.token_amount(env.vault);
            let error = env
                .crank(WINNER, EXPIRY_SLOT, observations)
                .expect_err("missing or unrelated backing discovery must reject atomically");
            assert!(
                error.contains("Custom(22)") || error.contains("custom program error: 0x16"),
                "unexpected unresolved-backing crank error: {error}"
            );
            assert_eq!(env.market_data(false), market_before);
            assert_eq!(env.primary_portfolio_data(WINNER), portfolio_before);
            assert_eq!(
                env.token_amount(env.actors[WINNER].destination_token),
                destination_before
            );
            assert_eq!(env.token_amount(env.vault), vault_before);
        }
        let backing_progress = env
            .crank(
                WINNER,
                EXPIRY_SLOT,
                vec![CrankObservationHint {
                    asset_index: UNRELATED_ASSET,
                    oracle_accounts: 0,
                }],
            )
            .expect("permissionless resolved crank expires hinted backing from committed state");
        assert!(backing_progress.compute_units < TX_CU_LIMIT);
        max_cu = max_cu.max(backing_progress.compute_units);
    }
    inv067_drain_resolved_actor(&mut env, WINNER, &mut max_cu);
    max_cu = max_cu.max(
        env.close_primary_portfolio(WINNER)
            .expect("close fully paid winner portfolio")
            .compute_units,
    );

    for step in 0..4 {
        if env
            .svm
            .get_account(&env.market)
            .is_some_and(|account| account.data.len() == HEADER_LEN)
        {
            break;
        }
        let success = env
            .close_primary_slab()
            .unwrap_or_else(|error| panic!("terminal slab step {step}: {error}"));
        assert!(
            success.compute_units < TX_CU_LIMIT,
            "terminal slab step {step} consumed {} CU",
            success.compute_units
        );
        max_cu = max_cu.max(success.compute_units);
    }

    let winner_payout = env.token_amount(env.actors[WINNER].destination_token);
    let mint_supply_after = env.mint_supply();
    eprintln!(
        "INV-067 late-backing trace: initial_close_cu={}, max_cu={max_cu}, pre_expiry_receipt={pre_expiry_receipt:?}, winner={pre_expiry_payout}->{winner_payout}, mint={mint_supply_before}->{mint_supply_after}",
        initial_close.compute_units,
    );
    assert!(
        pre_expiry_receipt.present,
        "the engine erased a haircut receipt while future backing could still raise its rate"
    );
    assert_eq!(winner_payout, EXPECTED_WINNER_PAYOUT);
    assert_eq!(mint_supply_after, mint_supply_before);
    assert_closed_market_tombstone(
        &env.svm
            .get_account(&env.market)
            .expect("terminal market tombstone"),
    );
}

#[test]
fn v16_program_fully_receipted_claimants_survive_two_unrelated_expiry_waves() {
    inv067_receipted_expiry_waves(false);
}

#[test]
fn v16_program_fully_paid_receipts_exit_before_late_excess_backing_cleanup() {
    // Unlike the haircut control, full face is terminal even with Fresh stock.
    inv067_receipted_expiry_waves(true);
}

fn inv067_receipted_expiry_waves(full_rate: bool) {
    use percolator::{BackingBucketStatusV16, ResolvedPayoutReceiptV16, BOUND_SCALE};
    use percolator_prog::ix::Instruction as ProgInstruction;
    use solana_sdk::{
        account::Account,
        compute_budget::ComputeBudgetInstruction,
        fee::FeeStructure,
        instruction::{AccountMeta, Instruction, InstructionError},
        message::Message,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        transaction::{Transaction, TransactionError},
    };

    const CLAIMANTS: [usize; 2] = [0, 4];
    // The debtor's 500 atoms realize pro rata before the receipt snapshot. Only
    // the remaining faces share later releases from unrelated backing.
    const GROSS: [u128; 2] = [14 * 50, 26 * 50];
    const REALIZED: [u128; 2] = [GROSS[0] * 500 / 2_000, GROSS[1] * 500 / 2_000];
    const FACES: [u128; 2] = [GROSS[0] - REALIZED[0], GROSS[1] - REALIZED[1]];
    const TOTAL_FACE: u128 = 2_000 - 500;
    const INITIAL_RESIDUAL: u128 = 0;

    fn frame(env: &V16Svm) -> Vec<(Pubkey, Option<Account>)> {
        let mut keys: Vec<_> = env
            .all_economic_account_lamports()
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        keys.extend(env.actors.iter().map(|actor| actor.signer.pubkey()));
        keys.extend([
            env.vault_authority,
            env.program_id,
            spl_token::ID,
            solana_sdk::system_program::ID,
            solana_sdk::compute_budget::ID,
            solana_sdk::sysvar::clock::ID,
        ]);
        keys.sort_unstable();
        keys.dedup();
        keys.into_iter()
            .map(|key| (key, env.svm.get_account(&key)))
            .collect()
    }

    fn send(
        env: &mut V16Svm,
        payer: &Keypair,
        instructions: &[Instruction],
        peak: &mut u64,
    ) -> Result<litesvm::types::TransactionMetadata, litesvm::types::FailedTransactionMetadata>
    {
        env.expire_blockhash();
        let mut all = vec![
            ComputeBudgetInstruction::request_heap_frame(256 * 1024),
            ComputeBudgetInstruction::set_compute_unit_limit(TX_CU_LIMIT as u32),
        ];
        all.extend_from_slice(instructions);
        let tx = Transaction::new_signed_with_payer(
            &all,
            Some(&payer.pubkey()),
            &[payer],
            env.svm.latest_blockhash(),
        );
        land(env, tx, peak)
    }

    fn land(
        env: &mut V16Svm,
        tx: Transaction,
        peak: &mut u64,
    ) -> Result<litesvm::types::TransactionMetadata, litesvm::types::FailedTransactionMetadata>
    {
        let payer = tx.message.account_keys[0];
        let mut price = 0u64;
        for ix in &tx.message.instructions {
            if tx.message.account_keys[ix.program_id_index as usize]
                == solana_sdk::compute_budget::ID
            {
                let budget: ComputeBudgetInstruction =
                    solana_sdk::borsh1::try_from_slice_unchecked(&ix.data).unwrap();
                match budget {
                    ComputeBudgetInstruction::SetComputeUnitPrice(value) => price = value,
                    ComputeBudgetInstruction::SetComputeUnitLimit(value) => {
                        assert_eq!(u64::from(value), TX_CU_LIMIT);
                    }
                    _ => {}
                }
            }
        }
        let fee = FeeStructure::default().lamports_per_signature * tx.signatures.len() as u64
            + (TX_CU_LIMIT * price).div_ceil(1_000_000);
        let before: Vec<_> = tx
            .message
            .account_keys
            .iter()
            .map(|&key| (key, env.svm.get_account(&key)))
            .collect();
        let result = env.svm.send_transaction(tx);
        let meta = match &result {
            Ok(meta) => meta,
            Err(error) => &error.meta,
        };
        *peak = (*peak).max(meta.compute_units_consumed);
        assert!(meta.compute_units_consumed < TX_CU_LIMIT);
        if result.is_err() {
            for (key, mut expected) in before {
                if key == payer {
                    expected.as_mut().unwrap().lamports -= fee;
                }
                assert_eq!(
                    env.svm.get_account(&key),
                    expected,
                    "rollback Account {key}"
                );
            }
        }
        result
    }

    let receipt = |env: &V16Svm, actor: usize| {
        env.primary_portfolio(actor)
            .resolved_payout_receipt
            .try_to_runtime()
            .unwrap()
    };
    let payout = |env: &V16Svm, actor: usize, instruction: ProgInstruction| Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new_readonly(env.actors[actor].signer.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.actors[actor].portfolio, false),
            AccountMeta::new(env.actors[actor].destination_token, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: instruction.encode(),
    };
    let mut peak_cu = 0;
    let mut rollbacks = 0;
    for early_side in 0..2 {
        for overdue in [false, true] {
            for order in [[0, 1], [1, 0]] {
                let mut env = V16Svm::new(
                    [0x74; 32],
                    MarketConfig {
                        initial_price: 100,
                        maintenance_margin_bps: 1_000,
                        initial_margin_bps: 1_000,
                        max_price_move_bps_per_slot: 500,
                        max_accrual_dt_slots: 1,
                        min_funding_lifetime_slots: 1,
                        actor_deposits: [1_000, 500, 0, 0, 1_000],
                        ..MarketConfig::default()
                    },
                );
                env.configure_permissionless_resolve(100, 1).unwrap();
                env.update_asset_authority_from_admin(
                    1,
                    percolator_prog::processor::ASSET_AUTH_BACKING_BUCKET,
                    2,
                )
                .unwrap();
                let expiries = if early_side == 0 { [40, 60] } else { [60, 40] };
                let first_side = if overdue { 0 } else { early_side };
                let backing = if full_rate {
                    let mut backing = [189; 2];
                    backing[first_side] = TOTAL_FACE;
                    backing
                } else {
                    [161, 189]
                };
                let backing_total = backing.iter().sum::<u128>();
                let provider_before = env.token_amount(env.actors[2].source_token);
                for side in 0..2 {
                    let topup = env.build_retained_backing_bucket_top_up_for_actor(
                        2,
                        2 + side as u16,
                        backing[side],
                        expiries[side],
                    );
                    env.land_retained(topup).unwrap();
                }
                for (actor, size) in [(0, 14), (4, 26)] {
                    env.trade_no_cpi(actor, 1, 0, size * POS_SCALE as i128, 100, 0)
                        .unwrap();
                }
                for (offset, mark) in (105..=150).step_by(5).enumerate() {
                    let slot = 2 + offset as u64;
                    env.warp_to_slot(slot);
                    env.push_auth_mark(0, slot, mark).unwrap();
                    for actor in [1, 0, 4] {
                        let oracle_accounts = env.primary_profile(0).oracle_leg_count;
                        let success = env
                            .crank(
                                actor,
                                slot,
                                vec![CrankObservationHint {
                                    asset_index: 0,
                                    oracle_accounts,
                                }],
                            )
                            .unwrap();
                        peak_cu = peak_cu.max(success.compute_units);
                    }
                }
                for (actor, size) in [(0, 14), (4, 26)] {
                    env.trade_no_cpi(actor, 1, 0, -size * POS_SCALE as i128, 150, 0)
                        .unwrap();
                }
                env.warp_to_slot(12);
                env.resolve_market().unwrap();
                for actor in [1, 2, 3] {
                    inv067_drain_resolved_actor(&mut env, actor, &mut peak_cu);
                    env.close_primary_portfolio(actor).unwrap();
                }
                env.warp_to_slot(14);
                for index in order {
                    let actor = CLAIMANTS[index];
                    for _ in 0..8 {
                        if receipt(&env, actor).present {
                            break;
                        }
                        peak_cu =
                            peak_cu.max(env.close_resolved_primary(actor).unwrap().compute_units);
                    }
                    assert!(receipt(&env, actor).present);
                }
                let original = CLAIMANTS.map(|actor| receipt(&env, actor));
                for index in 0..2 {
                    assert_eq!(
                        original[index],
                        ResolvedPayoutReceiptV16 {
                            present: true,
                            prior_bound_contribution_num: FACES[index] * BOUND_SCALE,
                            live_released_face_at_receipt: 0,
                            terminal_positive_claim_face: FACES[index],
                            paid_effective: 0,
                            finalized: false,
                        }
                    );
                }
                let identities = CLAIMANTS.map(|actor| {
                    (
                        env.primary_portfolio_id(actor),
                        env.primary_portfolio_position_epoch(actor),
                    )
                });
                let supply = env.mint_supply();
                let mut paid = [0; 2];
                let mut released = [false; 2];
                let mut residual = INITIAL_RESIDUAL;
                let check = |env: &V16Svm, paid: [u128; 2], released: [bool; 2], residual: u128| {
                    let group = env.primary_market_state().1;
                    let ledger = group.resolved_payout_ledger;
                    assert_eq!(group.c_tot, 0);
                    assert_eq!(group.insurance, 0);
                    assert_eq!(group.source_claim_bound_total_num, 0);
                    assert_eq!(group.materialized_portfolio_count, 2);
                    assert_eq!(ledger.snapshot_slot, 14);
                    assert_eq!(ledger.snapshot_residual, residual);
                    assert_eq!(ledger.terminal_claim_bound_unreceipted_num, 0);
                    assert_eq!(
                        ledger.terminal_claim_exact_receipts_num,
                        TOTAL_FACE * BOUND_SCALE
                    );
                    assert_eq!(
                        ledger.current_payout_rate_num,
                        residual.min(TOTAL_FACE) * BOUND_SCALE
                    );
                    assert_eq!(ledger.current_payout_rate_den, TOTAL_FACE * BOUND_SCALE);
                    assert!(!ledger.payout_halted && !ledger.finalized);
                    for side in 0..2 {
                        let bucket = group.source_backing_buckets[2 + side];
                        assert_eq!(bucket.expiry_slot, expiries[side]);
                        assert_eq!(
                            bucket.status,
                            if released[side] {
                                BackingBucketStatusV16::Expired
                            } else {
                                BackingBucketStatusV16::Fresh
                            }
                        );
                        assert_eq!(
                            group.source_credit[2 + side].fresh_reserved_backing_num,
                            if released[side] {
                                0
                            } else {
                                backing[side] * BOUND_SCALE
                            }
                        );
                        assert_eq!(bucket.consumed_liened_backing_num, 0);
                    }
                    for index in 0..2 {
                        let actor = CLAIMANTS[index];
                        let observed = receipt(env, actor);
                        if observed.present {
                            let expected = ResolvedPayoutReceiptV16 {
                                paid_effective: paid[index],
                                finalized: paid[index] == FACES[index],
                                ..original[index]
                            };
                            assert_eq!(observed, expected);
                            assert_eq!(observed.terminal_positive_claim_face, FACES[index]);
                        } else {
                            assert!(released.iter().all(|&done| done));
                            assert_eq!(
                                paid[index],
                                FACES[index] * residual.min(TOTAL_FACE) / TOTAL_FACE
                            );
                        }
                        assert_eq!(
                            (
                                env.primary_portfolio_id(actor),
                                env.primary_portfolio_position_epoch(actor)
                            ),
                            identities[index]
                        );
                        assert_eq!(
                            env.primary_portfolio(actor).owner,
                            env.actors[actor].signer.pubkey().to_bytes()
                        );
                        assert!(env
                            .primary_portfolio(actor)
                            .source_domains
                            .iter()
                            .all(|source| !source.is_occupied()));
                        assert!(active_bitmap_is_empty(state::portfolio_active_bitmap(
                            &env.primary_portfolio(actor)
                        )));
                        assert_eq!(
                            u128::from(env.token_amount(env.actors[actor].destination_token)),
                            1_000 + REALIZED[index] + paid[index]
                        );
                    }
                    assert_eq!(group.vault, backing_total - paid.iter().sum::<u128>());
                    assert_eq!(u128::from(env.token_amount(env.vault)), group.vault);
                    assert_eq!(
                        env.token_amount(env.actors[2].source_token),
                        provider_before - backing_total as u64
                    );
                    assert_eq!(env.mint_supply(), supply);
                    assert_eq!(env.token_supply_observed(), u128::from(supply));
                };
                check(&env, paid, released, residual);
                let payer = Keypair::new();
                env.svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
                // Retain the exact payout bytes before either release. Both portfolios
                // already have receipts, no source entries and no unreceipted bound.
                let claims = CLAIMANTS
                    .map(|actor| payout(&env, actor, ProgInstruction::ClaimResolvedPayoutTopup));
                for wave in 0..if full_rate { 1 } else { 2 } {
                    // When both sides are overdue, one hinted crank discovers the
                    // lower domain first, regardless of their expiry ordering.
                    let side = if overdue {
                        wave
                    } else if wave == 0 {
                        early_side
                    } else {
                        1 - early_side
                    };
                    let slot = if overdue { 61 } else { expiries[side] };
                    env.warp_to_slot(slot);
                    let crank = payout(
                        &env,
                        CLAIMANTS[order[wave]],
                        ProgInstruction::PermissionlessCrank {
                            now_slot: slot,
                            observations: vec![CrankObservationHint {
                                asset_index: 1,
                                oracle_accounts: 0,
                            }],
                        },
                    );
                    let bundle = [
                        crank,
                        claims[order[wave]].clone(),
                        claims[order[1 - wave]].clone(),
                    ];
                    if wave == 1 {
                        let before = frame(&env);
                        let mut aborted = bundle.to_vec();
                        aborted.push(Instruction {
                            program_id: solana_sdk::system_program::ID,
                            accounts: vec![],
                            data: vec![],
                        });
                        let failure = send(&mut env, &payer, &aborted, &mut peak_cu)
                            .expect_err("abort after second expiry and receipt payout prefix");
                        assert_eq!(
                            failure.err,
                            TransactionError::InstructionError(
                                5,
                                InstructionError::InvalidInstructionData
                            )
                        );
                        rollbacks += 1;
                        for (program, count) in [(env.program_id, 3), (spl_token::ID, 2)] {
                            assert_eq!(
                                failure
                                    .meta
                                    .logs
                                    .iter()
                                    .filter(|line| **line == format!("Program {program} success"))
                                    .count(),
                                count
                            );
                        }
                        assert_eq!(
                            frame(&env),
                            before,
                            "restore first-wave paid identities, stock and full custody Accounts"
                        );
                        check(&env, paid, released, residual);
                    }
                    let before = CLAIMANTS.map(|actor| receipt(&env, actor));
                    send(&mut env, &payer, &bundle[..1], &mut peak_cu).unwrap();
                    released[side] = true;
                    residual += backing[side];
                    assert_eq!(
                        CLAIMANTS.map(|actor| receipt(&env, actor)),
                        before,
                        "stock discovery alone must preserve receipt identity and paid counters"
                    );
                    check(&env, paid, released, residual);
                    for index in [order[wave], order[1 - wave]] {
                        let due =
                            FACES[index] * residual.min(TOTAL_FACE) / TOTAL_FACE - paid[index];
                        let before = frame(&env);
                        let meta =
                            send(&mut env, &payer, &[claims[index].clone()], &mut peak_cu).unwrap();
                        assert_eq!(
                            meta.logs
                                .iter()
                                .filter(
                                    |line| **line == format!("Program {} success", spl_token::ID)
                                )
                                .count(),
                            usize::from(due != 0)
                        );
                        paid[index] += due;
                        check(&env, paid, released, residual);
                        if due == 0 {
                            assert_eq!(frame(&env), before, "full face cannot be paid twice");
                        }
                    }
                    if wave == 0 {
                        let before = frame(&env);
                        if full_rate {
                            assert_eq!(residual, TOTAL_FACE);
                            assert!(CLAIMANTS
                                .iter()
                                .all(|&actor| receipt(&env, actor).finalized));
                        }
                        send(&mut env, &payer, &claims, &mut peak_cu).unwrap();
                        assert_eq!(
                            frame(&env),
                            before,
                            "zero-due retries preserve paid identity while another bucket remains"
                        );
                    }
                }
                let expected_paid = if full_rate { FACES } else { [122, 227] };
                let residue = backing_total - expected_paid.iter().sum::<u128>();
                assert_eq!(paid, expected_paid);
                assert_eq!(residue, if full_rate { 189 } else { 1 });
                assert_eq!(u128::from(env.token_amount(env.vault)), residue);
                for actor in CLAIMANTS {
                    inv067_drain_resolved_actor(&mut env, actor, &mut peak_cu);
                }
                for claim in &claims {
                    let before = frame(&env);
                    match send(&mut env, &payer, std::slice::from_ref(claim), &mut peak_cu) {
                        Ok(meta) => assert!(!meta
                            .logs
                            .iter()
                            .any(|line| *line == format!("Program {} success", spl_token::ID))),
                        Err(failure) => assert_eq!(
                            failure.err,
                            TransactionError::InstructionError(
                                2,
                                InstructionError::Custom(
                                    percolator_prog::error::PercolatorError::EngineNonProgress
                                        as u32
                                )
                            )
                        ),
                    }
                    assert_eq!(
                        frame(&env),
                        before,
                        "terminal replay cannot repay or rewrite a retired receipt"
                    );
                }
                for actor in CLAIMANTS {
                    let rent = env.account_lamports(env.actors[actor].portfolio);
                    let market_rent = env.account_lamports(env.market);
                    peak_cu =
                        peak_cu.max(env.close_primary_portfolio(actor).unwrap().compute_units);
                    assert_eq!(env.account_lamports(env.market), market_rent + rent);
                    assert!(env
                        .svm
                        .get_account(&env.actors[actor].portfolio)
                        .is_none_or(|account| account.lamports == 0 && account.data.is_empty()));
                }
                if full_rate {
                    let group = env.primary_market_state().1;
                    assert_eq!(group.materialized_portfolio_count, 0);
                    assert_eq!(
                        group
                            .source_credit
                            .iter()
                            .map(|source| source.fresh_reserved_backing_num)
                            .sum::<u128>(),
                        189 * BOUND_SCALE
                    );
                    assert_eq!(group.resolved_payout_ledger.snapshot_residual, TOTAL_FACE);
                    assert_eq!(
                        group
                            .resolved_payout_ledger
                            .terminal_claim_exact_receipts_num,
                        TOTAL_FACE * BOUND_SCALE
                    );
                    // Full payment releases the portfolios before the remaining
                    // bucket is normalized, but it cannot authorize premature burn.
                    if !overdue {
                        let mut locked = false;
                        for _ in 0..4 {
                            let tx = env.build_retained_close_primary_slab();
                            let before = frame(&env);
                            match land(&mut env, tx, &mut peak_cu) {
                                Ok(_) => assert_ne!(frame(&env), before),
                                Err(failure) => {
                                    assert_eq!(failure.err, TransactionError::InstructionError(
                                        3, InstructionError::Custom(
                                            percolator_prog::error::PercolatorError::EngineLockActive as u32)));
                                    assert_eq!(frame(&env), before);
                                    locked = true;
                                    break;
                                }
                            }
                        }
                        assert!(
                            locked,
                            "unexpired excess backing still prevents slab deletion"
                        );
                        assert_eq!(env.token_amount(env.vault), 189);
                        assert_eq!(env.mint_supply(), supply);
                    }
                    env.warp_to_slot(if overdue {
                        61
                    } else {
                        expiries[1 - first_side]
                    });
                }
                let ledger_before_slab = env.primary_market_state().1.resolved_payout_ledger;
                for _ in 0..8 {
                    if env.svm.get_account(&env.market).unwrap().data.len() == HEADER_LEN {
                        break;
                    }
                    if full_rate {
                        let close = env.build_retained_close_primary_slab();
                        let suffix = Transaction::new_unsigned(Message::new(
                            &[Instruction {
                                program_id: solana_sdk::system_program::ID,
                                accounts: vec![],
                                data: vec![],
                            }],
                            None,
                        ));
                        let aborted = env.bundle_retained_transactions(&[close.clone(), suffix]);
                        let before = frame(&env);
                        let failure = land(&mut env, aborted, &mut peak_cu)
                            .expect_err("rollback successful terminal scan or burn/close");
                        assert_eq!(
                            failure.err,
                            TransactionError::InstructionError(
                                4,
                                InstructionError::InvalidInstructionData
                            )
                        );
                        assert_eq!(
                            failure
                                .meta
                                .logs
                                .iter()
                                .filter(
                                    |line| **line == format!("Program {} success", env.program_id)
                                )
                                .count(),
                            1
                        );
                        assert_eq!(frame(&env), before);
                        rollbacks += 1;
                        let success = land(&mut env, close, &mut peak_cu).unwrap();
                        assert_ne!(frame(&env), before);
                        let closed =
                            env.svm.get_account(&env.market).unwrap().data.len() == HEADER_LEN;
                        for logs in [&failure.meta.logs, &success.logs] {
                            for instruction in ["Instruction: Burn", "Instruction: CloseAccount"] {
                                assert_eq!(
                                    logs.iter()
                                        .filter(|line| line.contains(instruction))
                                        .count(),
                                    usize::from(closed),
                                    "the aborted prefix reaches the same custody operations"
                                );
                            }
                        }
                        if !closed {
                            let group = env.primary_market_state().1;
                            let late_bucket = group.source_backing_buckets[2 + 1 - first_side];
                            assert_eq!(late_bucket.status, BackingBucketStatusV16::Expired);
                            assert_eq!(
                                group.source_credit[2 + 1 - first_side].fresh_reserved_backing_num,
                                0
                            );
                            let mut expected = ledger_before_slab;
                            expected.snapshot_residual += 189;
                            assert_eq!(group.resolved_payout_ledger, expected,
                                "late stock preserves the completed exact face mass and capped rate");
                            assert_eq!(group.vault, 189);
                            assert_eq!(env.token_amount(env.vault), 189);
                            assert_eq!(env.mint_supply(), supply);
                        }
                    } else {
                        peak_cu = peak_cu.max(env.close_primary_slab().unwrap().compute_units);
                    }
                }
                assert_closed_market_tombstone(&env.svm.get_account(&env.market).unwrap());
                assert_eq!(
                    env.mint_supply(),
                    supply - residue as u64,
                    "only input-derived excess plus floor remainder is burned"
                );
                assert_eq!(env.token_supply_observed(), u128::from(supply) - residue);
                assert_eq!(
                    CLAIMANTS.map(|actor| env.token_amount(env.actors[actor].destination_token)),
                    if full_rate {
                        [1_700, 2_300]
                    } else {
                        [1_297, 1_552]
                    }
                );
            }
        }
    }
    let positive_topups = if full_rate { 16 } else { 32 };
    eprintln!("INV-067 receipted expiry waves: full_rate={full_rate}, 8 worlds, {rollbacks} prefix rollbacks, {positive_topups} positive top-ups, 8 slab closures; peak_cu={peak_cu}");
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_067_terminal_dust_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_reported_route_matrix_preserves_terminal_value_partition(
        seed in any::<[u8; 32]>()
    ) {
        let discoveries = discover_terminal_dust_violations(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(discoveries.len(), ProspectiveAccrualRoute::ALL.len());
        for (expected, discovery) in ProspectiveAccrualRoute::ALL.into_iter().zip(&discoveries) {
            prop_assert_eq!(discovery.route, expected);
            prop_assert_eq!(discovery.attacker_loss, 1);
            prop_assert_eq!(discovery.victim_loss, 0);
            prop_assert_eq!(discovery.control_vault_remaining, 0);
            prop_assert_eq!(discovery.vault_remaining, discovery.attacker_loss);
            prop_assert_eq!(discovery.control_supply, discovery.dust_supply);
            prop_assert_eq!(
                discovery.terminal_classification,
                PublicTerminalClassification::BoundedExit
            );
        }
        let violations: Vec<_> = discoveries
            .iter()
            .filter(|discovery| discovery.is_violation())
            .map(|discovery| discovery.route)
            .collect();
        prop_assert!(violations.is_empty(), "terminal claim erasure returned: {violations:?}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/v16_program_stateful_fuzz.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_terminal_source_haircut_preserves_victim_claim_fuzz(
        (seed, route) in terminal_dust_payout_protection_strategy()
    ) {
        let result = verify_terminal_dust_payout_protection(seed, route);
        prop_assert!(
            result.is_ok(),
            "terminal source-haircut protection failed for {:?}, seed {:?}: {}",
            route,
            seed,
            result.unwrap_err()
        );
    }
}
