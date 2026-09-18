//! INV-051 with INV-047/048/052: matched reductions convert both nonunit bases.
//!
//! Existing INV-051 partitions use unilateral RebalanceReduce, changing the passive
//! side's A each time. INV-050 reduces one scaled maker against a unit-index taker;
//! INV-086's dual-ADL worlds select liquidation/recovery quantities. None compares
//! aggregate/split/reversed matched fills with two distinct, fixed nonunit indices.
//! Here both retained bases must independently invert the same remaining effective
//! quantity; neither raw subtraction nor sharing one party's A is equivalent.
//! Fixed-price public histories also reconcile each owner's rounded fee and payout.
//! The two-asset selector additionally compares genuine batch clear/resize with
//! ordered singles, including an oversized effective close and a funded retry.

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

#[test]
fn v16_program_two_asset_nonunit_batch_clear_resize_matches_ordered_singles() {
    use support::fuzz_model::independent_health_certificate;

    const RESIDUAL: u128 = 11;
    let open = [3 * POS_SCALE + 7, 5 * POS_SCALE + 11];
    let first = [open[0] - open[0] / 3, open[1] - open[1] / 5];
    let effective = [first[0] - first[0] / 4, first[1] - first[1] / 3];
    let indices = std::array::from_fn::<_, 2, _>(|asset| {
        [
            mul_div_floor(ADL_ONE, effective[asset], first[asset]).unwrap(),
            mul_div_floor(ADL_ONE, first[asset], open[asset]).unwrap(),
        ]
    });
    let initial_raw = std::array::from_fn::<_, 2, _>(|asset| {
        [
            first[asset],
            mul_div_floor(effective[asset], ADL_ONE, indices[asset][1]).unwrap(),
        ]
    });
    let mut worlds = 0;
    let mut rejects = 0;
    let mut peak_cu = 0;
    let mut inverse_distinctions = 0;
    let mut wrong_asset_distinctions = 0;
    for direction in [-1i128, 1] {
        let signs = [direction, -direction];
        let mut common_endpoint = None;
        for order in [[0usize, 1], [1, 0]] {
            for route in ROUTES {
                let batch = matches!(route, TradeRoute::BatchNoCpi | TradeRoute::BatchCpi);
                let mut env = V16Svm::new(
                    [0x5b; 32],
                    MarketConfig {
                        initial_price: PRICE,
                        actor_deposits: [CAPITAL, CAPITAL, 0, 0, 0],
                        actor_token_balances: [CAPITAL as u64; PRIMARY_ACTOR_COUNT],
                        ..MarketConfig::default()
                    },
                );
                env.begin_public_trace();
                for asset in 0..2 {
                    env.trade_no_cpi(
                        0,
                        1,
                        asset as u16,
                        signs[asset] * open[asset] as i128,
                        PRICE,
                        0,
                    )
                    .unwrap();
                    env.rebalance_reduce(0, asset as u16, open[asset] - first[asset])
                        .unwrap();
                    env.rebalance_reduce(1, asset as u16, first[asset] - effective[asset])
                        .unwrap();
                }
                env.update_trade_fee_policy(FEE_BPS).unwrap();
                if matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi) {
                    env.ensure_primary_matcher_enabled(1).unwrap();
                }
                let staged = env.primary_market_state().1;
                let epochs = [0, 1].map(|asset| {
                    [
                        staged.assets[asset].epoch_long,
                        staged.assets[asset].epoch_short,
                    ]
                });
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
                let mut raw = initial_raw;
                let mut paid = 0;
                let check = |env: &V16Svm, remaining: [u128; 2], raw: [[u128; 2]; 2], paid| {
                    let group = env.primary_market_state().1;
                    assert_eq!(group.mode, MarketModeV16::Live);
                    for actor in 0..PRIMARY_ACTOR_COUNT {
                        let account = env.primary_portfolio(actor);
                        assert_eq!(
                            account.capital.get(),
                            if actor < 2 { CAPITAL - paid } else { 0 }
                        );
                        assert_eq!((account.pnl.get(), account.fee_credits.get()), (0, 0));
                        assert_eq!(
                            account.legs.iter().filter(|leg| leg.active != 0).count(),
                            if actor < 2 {
                                remaining.iter().filter(|q| **q != 0).count()
                            } else {
                                0
                            }
                        );
                    }
                    for asset in 0..2 {
                        let state = group.assets[asset];
                        let live = remaining[asset] != 0;
                        let expected_a = if !live {
                            [ADL_ONE; 2]
                        } else if signs[asset] > 0 {
                            indices[asset]
                        } else {
                            [indices[asset][1], indices[asset][0]]
                        };
                        assert_eq!([state.a_long, state.a_short], expected_a);
                        assert_eq!(
                            [state.epoch_long, state.epoch_short],
                            epochs[asset].map(|epoch| epoch + u64::from(!live))
                        );
                        assert_eq!(
                            [state.oi_eff_long_q, state.oi_eff_short_q],
                            [remaining[asset]; 2]
                        );
                        assert_eq!(
                            [state.mode_long, state.mode_short],
                            [if live {
                                SideModeV16::Normal
                            } else {
                                SideModeV16::ResetPending
                            }; 2]
                        );
                        assert_eq!(
                            [state.stored_pos_count_long, state.stored_pos_count_short],
                            [u64::from(live); 2]
                        );
                        assert_eq!(
                            [
                                state.pending_obligation_count_long,
                                state.pending_obligation_count_short
                            ],
                            [0; 2]
                        );
                        let mut weights = [0; 2];
                        for actor in 0..2 {
                            let account = env.primary_portfolio(actor);
                            assert_eq!(has_active_leg_for_asset(&account, asset), live);
                            if !live {
                                continue;
                            }
                            let leg = active_leg_for_asset(&account, asset);
                            let sign = if actor == 0 {
                                signs[asset]
                            } else {
                                -signs[asset]
                            };
                            assert_eq!(leg.basis_pos_q, sign * raw[asset][actor] as i128);
                            assert_eq!(leg.a_basis, ADL_ONE);
                            assert_eq!(leg.market_id, state.market_id);
                            assert_eq!(
                                leg.epoch_snap,
                                if sign > 0 {
                                    state.epoch_long
                                } else {
                                    state.epoch_short
                                }
                            );
                            assert_eq!(
                                leg.side,
                                if sign > 0 {
                                    SideV16::Long
                                } else {
                                    SideV16::Short
                                }
                            );
                            assert_eq!(
                                mul_div_ceil(raw[asset][actor], indices[asset][actor], ADL_ONE)
                                    .unwrap(),
                                remaining[asset]
                            );
                            let weight = mul_div_ceil(
                                raw[asset][actor],
                                percolator::SOCIAL_WEIGHT_SCALE,
                                ADL_ONE,
                            )
                            .unwrap();
                            assert_eq!(leg.loss_weight, weight);
                            weights[usize::from(sign < 0)] += weight;
                        }
                        assert_eq!(
                            [state.loss_weight_sum_long, state.loss_weight_sum_short],
                            weights
                        );
                    }
                    assert_eq!(
                        (group.vault, group.c_tot, group.insurance),
                        (2 * CAPITAL, 2 * (CAPITAL - paid), 2 * paid)
                    );
                    assert_eq!(env.token_amount(env.vault) as u128, group.vault);
                    assert_eq!(env.all_token_account_data(), custody);
                    assert_eq!(passive_keys.map(|key| env.svm.get_account(&key)), passive);
                    assert_public_stock_census("two-asset nonunit reduction", env).unwrap();
                    assert_public_encumbrance_census("two-asset nonunit reduction", env).unwrap();
                };
                check(&env, remaining, raw, paid);

                let send_batch = |env: &mut V16Svm, quantities: [u128; 2]| {
                    let group = env.primary_market_state().1;
                    if matches!(route, TradeRoute::BatchNoCpi) {
                        env.batch_trade_no_cpi(
                            0,
                            1,
                            order
                                .map(|asset| BatchTradeLeg {
                                    asset_index: asset as u16,
                                    market_id: group.assets[asset].market_id,
                                    size_q: -signs[asset] * quantities[asset] as i128,
                                    exec_price: PRICE,
                                    fee_bps: FEE_BPS,
                                })
                                .to_vec(),
                        )
                    } else {
                        env.batch_trade_cpi(
                            0,
                            1,
                            order
                                .map(|asset| BatchTradeCpiLeg {
                                    asset_index: asset as u16,
                                    market_id: group.assets[asset].market_id,
                                    size_q: -signs[asset] * quantities[asset] as i128,
                                    limit_price: PRICE,
                                    fee_bps: FEE_BPS,
                                })
                                .to_vec(),
                        )
                    }
                };
                let quantities = [effective[0], effective[1] - RESIDUAL];
                if batch {
                    // The second request is within both raw bases, but exceeds real exposure.
                    let bad_asset = order[1];
                    let mut bad = quantities;
                    bad[bad_asset] = effective[bad_asset] + 1;
                    assert!(initial_raw[bad_asset].iter().all(|q| *q > bad[bad_asset]));
                    let keys = env
                        .all_economic_account_lamports()
                        .into_iter()
                        .map(|(key, _)| key)
                        .collect::<Vec<_>>();
                    let before = keys
                        .iter()
                        .map(|key| env.svm.get_account(key))
                        .collect::<Vec<_>>();
                    let error = send_batch(&mut env, bad)
                        .expect_err("raw basis cannot authorize a flip after ADL");
                    let code = PercolatorError::EngineLockActive as u32;
                    assert!(
                        error.contains(&format!("Custom({code})")),
                        "{route:?} {order:?}: {error}"
                    );
                    assert_eq!(
                        keys.iter()
                            .map(|key| env.svm.get_account(key))
                            .collect::<Vec<_>>(),
                        before
                    );
                    check(&env, remaining, raw, paid);
                    rejects += 1;
                }
                let chunks = if batch {
                    vec![order.to_vec()]
                } else {
                    order.map(|asset| vec![asset]).to_vec()
                };
                for chunk in chunks {
                    let result = if batch {
                        send_batch(&mut env, quantities)
                    } else {
                        let asset = chunk[0];
                        execute_trade_route(
                            &mut env,
                            route,
                            0,
                            1,
                            asset as u16,
                            -signs[asset] * quantities[asset] as i128,
                            PRICE,
                            FEE_BPS,
                        )
                    }
                    .unwrap_or_else(|error| panic!("{route:?} {order:?}: {error}"));
                    peak_cu = peak_cu.max(result.compute_units);
                    assert_cu_within(
                        "two-asset nonunit clear/resize",
                        result.compute_units,
                        TRADE_CU_LIMIT,
                    );
                    for asset in chunk {
                        remaining[asset] -= quantities[asset];
                        for actor in 0..2 {
                            let next =
                                mul_div_floor(remaining[asset], ADL_ONE, indices[asset][actor])
                                    .unwrap();
                            inverse_distinctions +=
                                usize::from(raw[asset][actor] - quantities[asset] != next);
                            wrong_asset_distinctions += usize::from(
                                mul_div_floor(remaining[asset], ADL_ONE, indices[1 - asset][actor])
                                    .unwrap()
                                    != next,
                            );
                            raw[asset][actor] = next;
                        }
                        paid += mul_div_ceil(quantities[asset], FEE_BPS as u128, 10_000).unwrap();
                    }
                    check(&env, remaining, raw, paid);
                    let group = env.primary_market_state().1;
                    for actor in 0..2 {
                        let account = env.primary_portfolio(actor);
                        let cached = health_cert(&account);
                        let full =
                            independent_health_certificate("nonunit vector", &group, &account)
                                .unwrap();
                        // A reduction may retain a conservative certificate from before ADL.
                        assert!(cached.certified_equity <= full.certified_equity);
                        assert!(cached.certified_initial_req >= full.certified_initial_req);
                        assert!(cached.certified_maintenance_req >= full.certified_maintenance_req);
                        assert!(cached.certified_worst_case_loss >= full.certified_worst_case_loss);
                        assert!(cached.certified_liq_deficit >= full.certified_liq_deficit);
                        assert_eq!(cached.active_bitmap_at_cert, full.active_bitmap_at_cert);
                    }
                }
                let endpoint = (
                    [0, 1].map(|actor| active_leg_for_asset(&env.primary_portfolio(actor), 1)),
                    paid,
                );
                assert_eq!(*common_endpoint.get_or_insert(endpoint), endpoint,
                    "batch and both single-asset orders must preserve the same retained bases and fees");
                let result = execute_trade_route(
                    &mut env,
                    route,
                    0,
                    1,
                    1,
                    -signs[1] * RESIDUAL as i128,
                    PRICE,
                    FEE_BPS,
                )
                .unwrap();
                peak_cu = peak_cu.max(result.compute_units);
                assert_cu_within(
                    "nonunit vector final residual",
                    result.compute_units,
                    TRADE_CU_LIMIT,
                );
                paid += mul_div_ceil(RESIDUAL, FEE_BPS as u128, 10_000).unwrap();
                check(&env, [0; 2], [[0; 2]; 2], paid);
                for asset in order {
                    for side in [0, 1] {
                        let result = env.finalize_reset_side(asset as u16, side).unwrap();
                        peak_cu = peak_cu.max(result.compute_units);
                        assert_cu_within(
                            "nonunit vector reset",
                            result.compute_units,
                            CUSTODY_CU_LIMIT,
                        );
                    }
                    let state = env.primary_market_state().1.assets[asset];
                    assert_eq!([state.a_long, state.a_short], [ADL_ONE; 2]);
                    assert_eq!(
                        [state.mode_long, state.mode_short],
                        [SideModeV16::Normal; 2]
                    );
                }
                for actor in [0, 1] {
                    let destination = env.actors[actor].destination_token;
                    let before = env.token_amount(destination);
                    let result = env.withdraw_primary(actor, CAPITAL - paid).unwrap();
                    peak_cu = peak_cu.max(result.compute_units);
                    assert_cu_within(
                        "nonunit vector funded exit",
                        result.compute_units,
                        CUSTODY_CU_LIMIT,
                    );
                    assert_eq!(
                        env.token_amount(destination) - before,
                        (CAPITAL - paid) as u64
                    );
                    assert_eq!(env.primary_portfolio(actor).capital.get(), 0);
                }
                let group = env.primary_market_state().1;
                assert_eq!(
                    (group.vault, group.c_tot, group.insurance),
                    (2 * paid, 0, 2 * paid)
                );
                assert_eq!(env.token_amount(env.vault) as u128, 2 * paid);
                assert_eq!(env.token_supply_observed(), env.initial_token_supply);
                assert_public_stock_census("nonunit vector terminal custody", &env).unwrap();
                assert_eq!(passive_keys.map(|key| env.svm.get_account(&key)), passive);
                let trace = env.finish_public_trace();
                trace.validate_public_execution().unwrap();
                assert_eq!(trace.out_of_band_economic_mutations, 0);
                assert_eq!(
                    trace.steps.iter().filter(|step| !step.succeeded).count(),
                    usize::from(batch)
                );
                worlds += 1;
            }
        }
    }
    assert_eq!((worlds, rejects), (16, 8));
    assert!(inverse_distinctions > 0);
    assert_eq!(wrong_asset_distinctions, 32);
    println!("nonunit two-asset clear/resize: {worlds} worlds, {rejects} exact rollbacks, 64 resets, 32 payouts; raw subtraction/wrong-asset index distinctions={inverse_distinctions}/{wrong_asset_distinctions}; peak {peak_cu} CU");
}
