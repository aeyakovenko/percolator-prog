//! INV-051 with INV-047/048/052: matched reductions convert both nonunit bases.
//!
//! Existing INV-051 partitions use unilateral RebalanceReduce, changing the passive
//! side's A each time. INV-050 reduces one scaled maker against a unit-index taker;
//! INV-086's dual-ADL worlds select liquidation/recovery quantities. None compares
//! aggregate/split/reversed matched fills with two distinct, fixed nonunit indices.
//! Here both retained bases must independently invert the same remaining effective
//! quantity; neither raw subtraction nor sharing one party's A is equivalent.
//! Fixed-price public histories also reconcile each owner's rounded fee and payout.

use super::*;
use support::{
    fuzz_model::{
        assert_public_encumbrance_census, assert_public_stock_census, execute_trade_route,
        TradeRoute,
    },
    reference_math::{mul_div_ceil, mul_div_floor},
    v16_svm::{MarketConfig, V16Svm, PRIMARY_ACTOR_COUNT},
};

const PRICE: u64 = POS_SCALE as u64;
const CAPITAL: u128 = 10 * POS_SCALE;
const FEE_BPS: u64 = 137;
const ROUTES: [TradeRoute; 4] = [
    TradeRoute::NoCpi,
    TradeRoute::Cpi,
    TradeRoute::BatchNoCpi,
    TradeRoute::BatchCpi,
];

fn check_book(env: &V16Svm, direction: i128, remaining: u128, fee: u128) {
    let (_, group) = env.primary_market_state();
    let asset = group.assets[0];
    let mut oi = [0; 2];
    let mut counts = [0; 2];
    let mut weights = [0; 2];
    assert_eq!(group.mode, MarketModeV16::Live);
    for actor in 0..PRIMARY_ACTOR_COUNT {
        let portfolio = env.primary_portfolio(actor);
        assert_eq!(portfolio.pnl.get(), 0);
        assert_eq!(portfolio.fee_credits.get(), 0);
        assert_eq!(
            portfolio.capital.get(),
            if actor < 2 { CAPITAL - fee } else { 0 }
        );
        let legs: Vec<_> = portfolio
            .legs
            .iter()
            .map(|encoded| encoded.try_to_runtime().unwrap())
            .filter(|leg| leg.active)
            .collect();
        assert_eq!(legs.len(), usize::from(actor < 2 && remaining != 0));
        for leg in legs {
            assert_eq!(leg.asset_index, 0);
            assert_eq!(leg.market_id, asset.market_id);
            let sign = if actor == 0 { direction } else { -direction };
            assert_eq!(leg.basis_pos_q.signum(), sign);
            let side = usize::from(sign < 0);
            let (a, epoch) = if side == 0 {
                (asset.a_long, asset.epoch_long)
            } else {
                (asset.a_short, asset.epoch_short)
            };
            assert_eq!(
                leg.side,
                if side == 0 {
                    SideV16::Long
                } else {
                    SideV16::Short
                }
            );
            assert_eq!(leg.epoch_snap, epoch);
            let effective = mul_div_ceil(leg.basis_pos_q.unsigned_abs(), a, leg.a_basis).unwrap();
            assert_eq!(effective, remaining, "actor {actor}");
            oi[side] += effective;
            counts[side] += 1;
            let weight = mul_div_ceil(
                leg.basis_pos_q.unsigned_abs(),
                percolator::SOCIAL_WEIGHT_SCALE,
                leg.a_basis,
            )
            .unwrap();
            assert_eq!(leg.loss_weight, weight);
            weights[side] += weight;
        }
    }
    assert_eq!(oi, [remaining; 2]);
    assert_eq!([asset.oi_eff_long_q, asset.oi_eff_short_q], oi);
    assert_eq!(
        [asset.stored_pos_count_long, asset.stored_pos_count_short],
        counts
    );
    assert_eq!(
        [asset.loss_weight_sum_long, asset.loss_weight_sum_short],
        weights
    );
    assert_eq!(
        [
            asset.pending_obligation_count_long,
            asset.pending_obligation_count_short
        ],
        [0; 2]
    );
    assert_eq!(group.vault, 2 * CAPITAL);
    assert_eq!(group.c_tot, 2 * (CAPITAL - fee));
    assert_eq!(group.insurance, 2 * fee);
    assert_eq!(env.token_amount(env.vault) as u128, group.vault);
    assert_public_stock_census("dual-ADL matched partition", env).unwrap();
    assert_public_encumbrance_census("dual-ADL matched partition", env).unwrap();
}

#[test]
fn v16_program_dual_adl_matched_partitions_preserve_both_bases_and_fee_adjusted_exits() {
    let mut worlds = 0;
    let mut fills = 0;
    let mut raw_subtraction_differences = 0;
    let mut shared_index_differences = 0;
    let mut inverse_ceil_differences = 0;
    let mut peak_cu = 0;
    for open in [31, 3 * POS_SCALE + 7] {
        let first = open - open / 3;
        let effective = first - first / 4;
        // Actor 0 reduces first; actor 1 then reduces its already scaled exposure.
        let owner_a = [
            mul_div_floor(ADL_ONE, effective, first).unwrap(),
            mul_div_floor(ADL_ONE, first, open).unwrap(),
        ];
        assert_ne!(owner_a[0], owner_a[1]);
        let start_raw = [
            first,
            mul_div_floor(effective, ADL_ONE, owner_a[1]).unwrap(),
        ];
        for direction in [-1i128, 1] {
            let mut common_basis = None;
            let mut route_endpoints = [None, None, None];
            for route_offset in 0..ROUTES.len() {
                for (partition, parts) in [
                    vec![effective - 1],
                    vec![1, effective - 2],
                    vec![effective - 2, 1],
                ]
                .into_iter()
                .enumerate()
                {
                    let mut env = V16Svm::new(
                        [0x51; 32],
                        MarketConfig {
                            initial_price: PRICE,
                            actor_deposits: [CAPITAL, CAPITAL, 0, 0, 0],
                            actor_token_balances: [CAPITAL as u64; PRIMARY_ACTOR_COUNT],
                            ..MarketConfig::default()
                        },
                    );
                    env.begin_public_trace();
                    env.trade_no_cpi(0, 1, 0, direction * open as i128, PRICE, 0)
                        .unwrap();
                    check_book(&env, direction, open, 0);
                    env.rebalance_reduce(0, 0, open / 3).unwrap();
                    check_book(&env, direction, first, 0);
                    env.rebalance_reduce(1, 0, first / 4).unwrap();
                    check_book(&env, direction, effective, 0);
                    let staged = env.primary_market_state().1;
                    let staged_a = [staged.assets[0].a_long, staged.assets[0].a_short];
                    assert_eq!(
                        staged_a,
                        if direction == 1 {
                            owner_a
                        } else {
                            [owner_a[1], owner_a[0]]
                        }
                    );
                    let initial_legs =
                        [0, 1].map(|actor| active_leg_for_asset(&env.primary_portfolio(actor), 0));
                    for actor in 0..2 {
                        assert_eq!(
                            initial_legs[actor].basis_pos_q.unsigned_abs(),
                            start_raw[actor]
                        );
                        assert_eq!(initial_legs[actor].a_basis, ADL_ONE);
                        assert!(owner_a[actor] > 0 && owner_a[actor] < ADL_ONE);
                        assert!(start_raw[actor] > effective);
                    }
                    env.update_trade_fee_policy(FEE_BPS).unwrap();
                    let custody = env.all_token_account_data();
                    let passive_keys = [
                        env.foreign_market,
                        env.foreign_actor.portfolio,
                        env.backing_domain_ledger,
                        env.mint,
                        env.actors[2].portfolio,
                        env.actors[3].portfolio,
                        env.actors[4].portfolio,
                    ];
                    let passive = passive_keys.map(|key| env.svm.get_account(&key));
                    let mut remaining = effective;
                    let mut paid = 0;
                    let mut last_raw = start_raw;
                    for (step, q) in parts.iter().copied().chain([1]).enumerate() {
                        let route = ROUTES[(route_offset + step) % ROUTES.len()];
                        remaining -= q;
                        let expected_raw =
                            owner_a.map(|a| mul_div_floor(remaining, ADL_ONE, a).unwrap());
                        for actor in 0..2 {
                            raw_subtraction_differences +=
                                usize::from(last_raw[actor] - q != expected_raw[actor]);
                            shared_index_differences += usize::from(
                                mul_div_floor(remaining, ADL_ONE, owner_a[1 - actor]).unwrap()
                                    != expected_raw[actor],
                            );
                            inverse_ceil_differences += usize::from(
                                mul_div_ceil(remaining, ADL_ONE, owner_a[actor]).unwrap()
                                    != expected_raw[actor],
                            );
                        }
                        let result = execute_trade_route(
                            &mut env, route, 0, 1, 0, -direction * q as i128, PRICE, FEE_BPS,
                        ).unwrap_or_else(|error| panic!("open={open} direction={direction} partition={partition} route={route:?} q={q}: {error}"));
                        assert_cu_within(
                            "dual-ADL matched reduction",
                            result.compute_units,
                            TRADE_CU_LIMIT,
                        );
                        peak_cu = peak_cu.max(result.compute_units);
                        paid += mul_div_ceil(q, FEE_BPS as u128, 10_000).unwrap();
                        check_book(&env, direction, remaining, paid);
                        assert_eq!(env.all_token_account_data(), custody);
                        assert_eq!(passive_keys.map(|key| env.svm.get_account(&key)), passive);
                        if remaining != 0 {
                            let group = env.primary_market_state().1;
                            assert_eq!([group.assets[0].a_long, group.assets[0].a_short], staged_a);
                            let legs = [0, 1].map(|actor| {
                                active_leg_for_asset(&env.primary_portfolio(actor), 0)
                            });
                            for actor in 0..2 {
                                assert_eq!(
                                    legs[actor].basis_pos_q.unsigned_abs(),
                                    expected_raw[actor]
                                );
                                assert_eq!(legs[actor].a_basis, initial_legs[actor].a_basis);
                            }
                            if remaining == 1 {
                                // Exact position equality; only the independently bounded fees differ.
                                assert_eq!(*common_basis.get_or_insert(legs), legs);
                                let endpoint = (legs, paid);
                                assert_eq!(
                                    *route_endpoints[partition].get_or_insert(endpoint),
                                    endpoint
                                );
                            }
                        }
                        last_raw = expected_raw;
                        fills += 1;
                    }
                    let aggregate_fee =
                        mul_div_ceil(effective - 1, FEE_BPS as u128, 10_000).unwrap() + 1;
                    assert!(
                        paid >= aggregate_fee && paid <= aggregate_fee + parts.len() as u128 - 1
                    );
                    let closed = env.primary_market_state().1.assets[0];
                    assert_eq!(
                        [closed.mode_long, closed.mode_short],
                        [SideModeV16::ResetPending; 2]
                    );
                    for side in [0, 1] {
                        let result = env.finalize_reset_side(0, side).unwrap();
                        assert_cu_within(
                            "dual-ADL matched reset",
                            result.compute_units,
                            CUSTODY_CU_LIMIT,
                        );
                        peak_cu = peak_cu.max(result.compute_units);
                    }
                    let reset = env.primary_market_state().1.assets[0];
                    assert_eq!([reset.a_long, reset.a_short], [ADL_ONE; 2]);
                    assert_eq!(
                        [reset.mode_long, reset.mode_short],
                        [SideModeV16::Normal; 2]
                    );
                    for actor in [0, 1] {
                        let destination = env.actors[actor].destination_token;
                        let before = env.token_amount(destination);
                        let result = env.withdraw_primary(actor, CAPITAL - paid).unwrap();
                        assert_cu_within(
                            "dual-ADL matched payout",
                            result.compute_units,
                            CUSTODY_CU_LIMIT,
                        );
                        peak_cu = peak_cu.max(result.compute_units);
                        assert_eq!(
                            env.token_amount(destination) - before,
                            (CAPITAL - paid) as u64
                        );
                        assert_eq!(env.primary_portfolio(actor).capital.get(), 0);
                        assert_public_stock_census("dual-ADL matched payout", &env).unwrap();
                    }
                    let final_group = env.primary_market_state().1;
                    assert_eq!(
                        (final_group.c_tot, final_group.insurance, final_group.vault),
                        (0, 2 * paid, 2 * paid)
                    );
                    assert_eq!(env.token_amount(env.vault) as u128, 2 * paid);
                    assert_eq!(env.token_supply_observed(), env.initial_token_supply);
                    assert_eq!(passive_keys.map(|key| env.svm.get_account(&key)), passive);
                    let trace = env.finish_public_trace();
                    trace.validate_public_execution().unwrap();
                    assert_eq!(trace.out_of_band_economic_mutations, 0);
                    assert!(trace.steps.iter().all(|step| step.succeeded));
                    worlds += 1;
                }
            }
        }
    }
    assert_eq!(worlds, 48);
    assert_eq!(fills, 128);
    assert!(raw_subtraction_differences > 0);
    assert!(shared_index_differences > 0);
    assert!(inverse_ceil_differences > 0);
    println!("dual-ADL matched partitions: {worlds} worlds, {fills} fills, {} resets, {} payouts; raw/shared-index/inverse-ceil distinctions={raw_subtraction_differences}/{shared_index_differences}/{inverse_ceil_differences}; peak {peak_cu} CU", 2 * worlds, 2 * worlds);
}
