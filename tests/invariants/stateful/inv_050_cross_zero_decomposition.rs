//! INV-050 - Cross-zero decomposition under pending-loss epochs.
//!
//! Normative obligation: a pending domain-loss barrier may block new or flipped exposure, but it
//! must not block an exact same-side reduction and must release through bounded permissionless
//! work. The finding-blind matrix creates a real bankruptcy close and its zero-basis pending-loss
//! obligation without mutating program-owned bytes. While that close owns the domain barrier, it
//! runs the same cross-zero and exact-exit requests through all four trade routes for both long and
//! short barrier domains. Each flip must reject with the shared runner's exact full-state rollback
//! oracle; each exact reduction must clear both auxiliary positions and effective OI without
//! rewriting the close. The retained cure then cancels the close, and bounded public cranks must
//! release the obligation.
//!
//! Evidence: public LiteSVM stateful composition plus route metamorphism and independent raw-leg,
//! effective-OI, custody, encumbrance, stock, rollback, and liveness oracles after every step.
//! After the barrier releases, the pair's remaining capital plus junior positive-PnL face must
//! equal its original principal exactly, and both traders must withdraw all remaining senior
//! capital. Clearing OI while dropping value or silently converting a junior claim into senior
//! capital is therefore not accepted.
//!
//! Guarantee boundary: one real pending-loss episode is reached for every route/orientation cell.
//! Multiple simultaneous barriers, cross-asset barrier ordering, and full-width cross-zero
//! quantities remain owned by INV-039, INV-052, INV-074, and the remaining INV-050 boundary
//! campaign.

use crate::support::{
    fuzz_model::{
        assert_public_stock_census, execute_trade_route, run_pending_barrier_cross_zero_probe,
        TradeRoute,
    },
    v16_svm::{MarketConfig, V16Svm, INITIAL_PRICE, PRIMARY_ACTOR_COUNT, TX_CU_LIMIT},
};
use percolator::{SideModeV16, SideV16, POS_SCALE};

#[test]
fn v16_program_pending_loss_barrier_rejects_flips_but_preserves_all_route_exits() {
    let evidence = run_pending_barrier_cross_zero_probe()
        .expect("pending-loss barrier cross-zero matrix must preserve bounded owner exits");
    assert_eq!(evidence.world_count, 8, "{evidence:?}");
    assert_eq!(evidence.route_worlds, [2; 4], "{evidence:?}");
    assert_eq!(evidence.long_barrier_worlds, 4, "{evidence:?}");
    assert_eq!(evidence.short_barrier_worlds, 4, "{evidence:?}");
    assert_eq!(
        evidence.rejected_cross_zero_worlds, evidence.world_count,
        "{evidence:?}"
    );
    assert_eq!(
        evidence.exact_exit_worlds, evidence.world_count,
        "{evidence:?}"
    );
    assert_eq!(
        evidence.released_barrier_worlds, evidence.world_count,
        "{evidence:?}"
    );
    assert_eq!(
        evidence.senior_capital_exit_worlds, evidence.world_count,
        "{evidence:?}"
    );
}

#[test]
fn v16_program_one_sided_cross_zero_reconciles_three_owner_open_interest() {
    fn assert_book(env: &V16Svm, expected: [i128; PRIMARY_ACTOR_COUNT], expected_oi: u128) {
        let (_, group) = env.primary_market_state();
        let asset = &group.assets[0];
        let mut observed_oi = [0u128; 2];
        let mut observed_count = [0u64; 2];
        assert_eq!(expected.iter().sum::<i128>(), 0);
        for (actor, expected_q) in expected.into_iter().enumerate() {
            let account = env.primary_portfolio(actor);
            let active_legs = account
                .legs
                .iter()
                .map(|encoded| encoded.try_to_runtime().expect("decode public leg"))
                .filter(|leg| leg.active)
                .collect::<Vec<_>>();
            assert_eq!(
                active_legs.len(),
                usize::from(expected_q != 0),
                "actor {actor}"
            );
            assert_eq!(account.pnl.get(), 0, "actor {actor}");
            for leg in active_legs {
                assert_eq!(leg.asset_index, 0, "actor {actor}");
                assert_eq!(leg.market_id, asset.market_id, "actor {actor}");
                assert_eq!(leg.basis_pos_q, expected_q, "actor {actor}");
                let (side, current_a, epoch, mode) = match leg.side {
                    SideV16::Long => (0, asset.a_long, asset.epoch_long, asset.mode_long),
                    SideV16::Short => (1, asset.a_short, asset.epoch_short, asset.mode_short),
                };
                assert_eq!(side, usize::from(expected_q < 0), "actor {actor}");
                assert_eq!(mode, SideModeV16::Normal, "actor {actor}");
                assert_eq!(leg.epoch_snap, epoch, "actor {actor}");
                assert_ne!(current_a, 0, "actor {actor}");
                // No ADL or reset occurs here, so retained basis is exactly effective quantity.
                assert_eq!(leg.a_basis, current_a, "actor {actor}");
                observed_oi[side] += leg.basis_pos_q.unsigned_abs();
                observed_count[side] += 1;
            }
        }
        assert_eq!(observed_oi, [expected_oi; 2]);
        assert_eq!([asset.oi_eff_long_q, asset.oi_eff_short_q], observed_oi);
        assert_eq!(
            [asset.stored_pos_count_long, asset.stored_pos_count_short],
            observed_count
        );
        assert_eq!(
            [
                asset.pending_obligation_count_long,
                asset.pending_obligation_count_short,
            ],
            [0; 2]
        );
        assert_public_stock_census("INV-050 three-owner cross-zero", env)
            .expect("public stocks must reconcile at every trade prefix");
    }

    let mut worlds = 0;
    for route in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ] {
        for direction in [-1i128, 1] {
            for crossing_maker in [false, true] {
                let mut env = V16Svm::new([0x50; 32], MarketConfig::default());
                let token_frame = env.all_token_account_data();
                let foreign_market = env.market_data(true);
                let (_, initial_group) = env.primary_market_state();
                assert_book(&env, [0; PRIMARY_ACTOR_COUNT], 0);
                env.begin_public_trace();

                // The nine-lot fill flips only actor 0: OI falls by seven lots, not nine.
                let history = [
                    (0, 1, 7, [7, -7, 0, 0, 0], 7),
                    (2, 1, 3, [7, -10, 3, 0, 0], 10),
                    (0, 1, -9, [-2, -1, 3, 0, 0], 3),
                    (2, 0, -2, [0, -1, 1, 0, 0], 1),
                    (2, 1, -1, [0, 0, 0, 0, 0], 0),
                ];
                for (step, (mut taker, mut maker, lots, positions, oi_lots)) in
                    history.into_iter().enumerate()
                {
                    let mut size_q = lots * direction * POS_SCALE as i128;
                    if step == 2 && crossing_maker {
                        std::mem::swap(&mut taker, &mut maker);
                        size_q = -size_q;
                    }
                    let before_portfolios = env.all_primary_portfolio_data();
                    let result = execute_trade_route(
                        &mut env,
                        route,
                        taker,
                        maker,
                        0,
                        size_q,
                        INITIAL_PRICE,
                        0,
                    )
                    .unwrap_or_else(|error| {
                        panic!(
                            "{route:?} direction={direction} crossing_maker={crossing_maker} step={step}: {error}"
                        )
                    });
                    assert!(result.compute_units <= TX_CU_LIMIT);
                    assert_book(
                        &env,
                        positions.map(|lots| lots * direction * POS_SCALE as i128),
                        oi_lots * POS_SCALE,
                    );
                    for (actor, before) in before_portfolios.iter().enumerate() {
                        if actor != taker && actor != maker {
                            assert_eq!(*before, env.primary_portfolio_data(actor), "actor {actor}");
                        }
                    }
                    assert_eq!(env.all_token_account_data(), token_frame);
                    assert_eq!(env.market_data(true), foreign_market);
                    let (_, group) = env.primary_market_state();
                    assert_eq!(group.assets[1..], initial_group.assets[1..]);
                }
                let trace = env.finish_public_trace();
                trace
                    .validate_public_execution()
                    .expect("all positions must originate in public wrapper trades");
                assert!(trace.steps.len() >= history.len());
                worlds += 1;
            }
        }
    }
    assert_eq!(worlds, 16);
}
