//! INV-035 - No global B pool; residuals remain local.
//!
//! Normative obligation: Bankruptcy residuals stay in the exact asset and opposing-side domain that created them.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_two_asset_bankruptcy_preserves_domain_local_settlement_and_exit` creates claims in
//! two source domains, books bankruptcy B in one asset, and independently recomputes both claim
//! deltas. It requires the unrelated claim to remain unchanged, the affected claim to absorb the
//! exact B loss, bounded public reductions to flatten the affected leg, and principal withdrawal
//! with conserved SPL supply.
//! `v16_program_ambiguous_multi_asset_deficit_recovers_without_last_asset_charge` exercises the
//! complementary public route boundary. A loss from asset 0 predates the final reduction of an
//! unrelated asset 1 leg, so the engine cannot safely infer one residual domain. Every trade route
//! must leave both B domains untouched, select terminal Recovery, and settle all funded portfolios
//! without losing SPL value.
//!
//! Guarantee boundary: this randomized public-route oracle certifies the exercised two-domain
//! topology. The deterministic TDD route lives in the public-SBF INV-035 file, while engine Kani
//! proves the domain-first partition kernel.

use super::*;
use crate::support::{
    fuzz_model::execute_trade_route,
    v16_svm::{MarketConfig, V16Svm},
};
use percolator::{active_bitmap_is_empty, MarketModeV16, POS_SCALE};
use percolator_prog::ix::CrankObservationHint;

fn inv_035_terminal(account: &percolator_prog::state::PortfolioAccountV16) -> bool {
    account.capital.get() == 0
        && account.pnl.get() == 0
        && active_bitmap_is_empty(account.active_bitmap.map(|word| word.get()))
}

fn verify_ambiguous_multi_asset_recovery(route: TradeRoute) -> Result<(), String> {
    const WINNER: usize = 0;
    const LOSER: usize = 1;
    const KEEPER: usize = 4;
    const LOSS_ASSET: u16 = 0;
    const LAST_CLOSED_ASSET: u16 = 1;
    const OPEN_PRICE: u64 = 100;
    const LOSS_PRICE: u64 = 150;
    const SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const LOSER_PRINCIPAL: u128 = 250;

    let route_tag = match route {
        TradeRoute::NoCpi => 0,
        TradeRoute::Cpi => 1,
        TradeRoute::BatchNoCpi => 2,
        TradeRoute::BatchCpi => 3,
    };
    let actor_deposits = [2_000, LOSER_PRINCIPAL, 1_000, 1, 1];
    let expected_payouts: u128 = actor_deposits.iter().sum();
    let mut seed = [0x35; 32];
    seed[0] ^= route_tag;
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: OPEN_PRICE,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits,
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    let destination_before: u128 = env
        .actors
        .iter()
        .map(|actor| env.token_amount(actor.destination_token) as u128)
        .sum();
    env.begin_public_trace();

    for asset in [LOSS_ASSET, LAST_CLOSED_ASSET] {
        execute_trade_route(&mut env, route, WINNER, LOSER, asset, SIZE_Q, OPEN_PRICE, 0)
            .map_err(|error| format!("{route:?}: open asset {asset}: {error}"))?;
    }

    let mut final_slot = 1;
    let oracle_accounts = env.primary_profile(LOSS_ASSET as usize).oracle_leg_count;
    for (offset, mark) in (105u64..=LOSS_PRICE).step_by(5).enumerate() {
        final_slot = 2 + u64::try_from(offset).expect("bounded mark step");
        env.warp_to_slot(final_slot);
        env.push_auth_mark(LOSS_ASSET, final_slot, mark)
            .map_err(|error| format!("{route:?}: publish loss mark {mark}: {error}"))?;
        env.crank(
            KEEPER,
            final_slot,
            vec![CrankObservationHint {
                asset_index: LOSS_ASSET,
                oracle_accounts,
            }],
        )
        .map_err(|error| format!("{route:?}: accrue loss asset: {error}"))?;
    }

    execute_trade_route(
        &mut env, route, WINNER, LOSER, LOSS_ASSET, -SIZE_Q, LOSS_PRICE, 0,
    )
    .map_err(|error| format!("{route:?}: close loss asset: {error}"))?;
    let after_loss_close = env.primary_portfolio(LOSER);
    let active_assets = after_loss_close
        .legs
        .iter()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .filter(|leg| leg.active)
        .map(|leg| leg.asset_index)
        .collect::<Vec<_>>();
    if after_loss_close.pnl.get() != -(LOSER_PRINCIPAL as i128)
        || active_assets != vec![LAST_CLOSED_ASSET as u32]
    {
        return Err(format!(
            "{route:?}: loss was not retained while unrelated leg remained: pnl={}, active={active_assets:?}",
            after_loss_close.pnl.get()
        ));
    }
    let interim_ledger = after_loss_close
        .close_progress
        .try_to_runtime()
        .map_err(|error| format!("{route:?}: decode interim ledger: {error:?}"))?;
    if interim_ledger.active || interim_ledger.residual_remaining != 0 {
        return Err(format!(
            "{route:?}: multi-asset intermediate close was misattributed: {interim_ledger:?}"
        ));
    }

    execute_trade_route(
        &mut env,
        route,
        WINNER,
        LOSER,
        LAST_CLOSED_ASSET,
        -SIZE_Q,
        OPEN_PRICE,
        0,
    )
    .map_err(|error| format!("{route:?}: close unrelated final asset: {error}"))?;
    let flat_loser = env.primary_portfolio(LOSER);
    let terminal_ledger = flat_loser
        .close_progress
        .try_to_runtime()
        .map_err(|error| format!("{route:?}: decode terminal ledger: {error:?}"))?;
    let (_, before_recovery) = env.primary_market_state();
    if !active_bitmap_is_empty(flat_loser.active_bitmap.map(|word| word.get()))
        || flat_loser.pnl.get() != -(LOSER_PRINCIPAL as i128)
        || terminal_ledger.active
        || terminal_ledger.residual_remaining != 0
        || before_recovery.assets[LOSS_ASSET as usize].b_long_num != 0
        || before_recovery.assets[LOSS_ASSET as usize].b_short_num != 0
        || before_recovery.assets[LAST_CLOSED_ASSET as usize].b_long_num != 0
        || before_recovery.assets[LAST_CLOSED_ASSET as usize].b_short_num != 0
    {
        return Err(format!(
            "{route:?}: ambiguous residual was charged to an inferred asset: pnl={}, \
             ledger={terminal_ledger:?}, loss_b=({}, {}), last_b=({}, {})",
            flat_loser.pnl.get(),
            before_recovery.assets[LOSS_ASSET as usize].b_long_num,
            before_recovery.assets[LOSS_ASSET as usize].b_short_num,
            before_recovery.assets[LAST_CLOSED_ASSET as usize].b_long_num,
            before_recovery.assets[LAST_CLOSED_ASSET as usize].b_short_num,
        ));
    }

    env.crank(LOSER, final_slot, vec![])
        .map_err(|error| format!("{route:?}: declare ambiguous recovery: {error}"))?;
    if env.primary_market_state().1.mode != MarketModeV16::Recovery {
        return Err(format!(
            "{route:?}: ambiguous residual did not enter Recovery"
        ));
    }
    env.crank(LOSER, final_slot, vec![])
        .map_err(|error| format!("{route:?}: finalize ambiguous recovery: {error}"))?;
    if env.primary_market_state().1.mode != MarketModeV16::Resolved {
        return Err(format!("{route:?}: Recovery did not finalize"));
    }

    for actor in [LOSER, WINNER, 2, 3, KEEPER] {
        for _ in 0..16 {
            if inv_035_terminal(&env.primary_portfolio(actor)) {
                break;
            }
            env.close_resolved_primary_signed(actor)
                .map_err(|error| format!("{route:?}: close actor {actor}: {error}"))?;
        }
        if !inv_035_terminal(&env.primary_portfolio(actor)) {
            return Err(format!("{route:?}: actor {actor} did not terminate"));
        }
    }

    let destination_after: u128 = env
        .actors
        .iter()
        .map(|actor| env.token_amount(actor.destination_token) as u128)
        .sum();
    if destination_after.checked_sub(destination_before) != Some(expected_payouts)
        || env.token_amount(env.vault) != 0
        || env.token_supply_observed() != supply_before
    {
        return Err(format!(
            "{route:?}: ambiguous-recovery payout mismatch: before={destination_before}, \
             after={destination_after}, expected={expected_payouts}, vault={}, supply={}",
            env.token_amount(env.vault),
            env.token_supply_observed()
        ));
    }

    let trace = env.finish_public_trace();
    if trace.out_of_band_economic_mutations != 0
        || trace
            .steps
            .iter()
            .filter(|step| !step.succeeded)
            .any(|step| {
                step.rejected_exact_writable_rollback != Some(true)
                    || step.rejected_no_program_lamport_delta != Some(true)
                    || step.token_deltas.iter().any(|(_, delta)| *delta != 0)
            })
    {
        return Err(format!("{route:?}: non-atomic public trace: {trace:?}"));
    }
    Ok(())
}

#[test]
fn v16_program_ambiguous_multi_asset_deficit_recovers_without_last_asset_charge() {
    for route in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ] {
        verify_ambiguous_multi_asset_recovery(route)
            .unwrap_or_else(|error| panic!("INV-035: {error}"));
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_035_cross_domain_b_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_two_asset_bankruptcy_preserves_domain_local_settlement_and_exit(
        seed in any::<[u8; 32]>()
    ) {
        let discovery = discover_cross_domain_b_violation(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert!(
            discovery.preserves_domain_locality_and_exit(),
            "domain-local B settlement or bounded exit failed: {:?}",
            discovery
        );
    }
}
