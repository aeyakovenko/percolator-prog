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

    env.warp_to_slot(EXPIRY_SLOT);
    if pre_expiry_receipt.present {
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
