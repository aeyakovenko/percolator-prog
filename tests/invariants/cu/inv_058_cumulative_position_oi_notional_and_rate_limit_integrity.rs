//! INV-058 - Cumulative position, OI, notional, and rate-limit integrity.
//!
//! Normative obligation: cumulative caps and post-transition effective state
//! are enforced at boundaries and cannot be bypassed by splitting, oversized
//! values, or route choice. Arithmetic at zero, max, and max-plus-one must fail
//! closed without truncation.
//!
//! Evidence in this file (I/C): public LiteSVM wrapper tests cover the total
//! vault TVL cap across deposit and privileged top-up routes, amount values
//! larger than the SPL-token `u64` transport can represent, owner reduction
//! over the current exposure clamping to flat rather than opening opposite-side
//! risk, and batch/CPI active-leg caps rejecting atomically before partial
//! state mutation or hostile matcher CPI. The cumulative position/OI matrix
//! exhausts all sixteen first-fill/final-fill transport pairs, reaches the
//! shared account/side-OI cap by splitting `max - 1` and one atom, rejects one
//! more atom through every transport with a complete economic snapshot, and
//! then closes the exact-maximum position through a public route.
//! A same-pair history table compares direct open, reduction, cross-zero, and
//! split close/reopen near the cap, then reuses released capacity across routes
//! while reconciling post-state OI, positions, and cached risk notional.
//! The recreation witness moves one counterparty's exposure into a third account,
//! closes/reinitializes the now-flat address, and checks fresh cap admission in
//! both account roles. Real two-asset batches frame an otherwise valid leg on
//! rejection; split refills and cross-zero reuse only post-transition headroom.
//! A distinct-owner-pair regression isolates aggregate side-OI enforcement from
//! account-local position caps by filling the side cap across two pairs, then
//! rejecting one additional public trade with exact rollback. These fixed-price
//! witnesses assert unit ADL indices and do not exercise rate limits.
//!
//! Guarantee boundary: `MAX_TRADE_SIZE_Q`, `MAX_POSITION_ABS_Q`, and
//! `MAX_OI_SIDE_Q` are currently one shared bound, while the exact maximum
//! position/price product is `MAX_ACCOUNT_NOTIONAL`. Assertions below make a
//! future divergence reopen this matrix. INV-050 owns cross-zero and scalar
//! max/max+1 route boundaries, INV-009/011/052/059 own cumulative signed
//! quantity/fee partitions, INV-045 owns elapsed price/funding-rate limits,
//! INV-083 owns every public configuration boundary, and INV-085 owns the
//! deployed full-width notional arithmetic. There is no public position
//! transfer route; INV-049 source-locks the complete position-writer surface.

use super::*;

#[path = "inv_058_liquidation_lifecycle.rs"]
mod liquidation_lifecycle;

use support::{
    fuzz_model::{
        assert_public_encumbrance_census, assert_public_stock_census, execute_trade_route,
        TradeRoute,
    },
    v16_svm::{MarketConfig, V16Svm, PRIMARY_ACTOR_COUNT, TX_CU_LIMIT},
};

const INV_058_TRADE_ROUTES: [TradeRoute; 4] = [
    TradeRoute::NoCpi,
    TradeRoute::Cpi,
    TradeRoute::BatchNoCpi,
    TradeRoute::BatchCpi,
];

#[derive(Debug, PartialEq, Eq)]
struct Inv058EconomicSnapshot {
    market: Vec<u8>,
    foreign_market: Vec<u8>,
    portfolios: Vec<Vec<u8>>,
    foreign_portfolio: Vec<u8>,
    backing_ledger: Vec<u8>,
    token_accounts: Vec<(Pubkey, Vec<u8>)>,
    matcher_contexts: Vec<Vec<u8>>,
    economic_lamports: Vec<(Pubkey, u64)>,
    token_supply: u128,
}

fn inv_058_economic_snapshot(env: &V16Svm) -> Inv058EconomicSnapshot {
    Inv058EconomicSnapshot {
        market: env.market_data(false),
        foreign_market: env.market_data(true),
        portfolios: env.all_primary_portfolio_data(),
        foreign_portfolio: env.foreign_portfolio_data(),
        backing_ledger: env.backing_domain_ledger_data(),
        token_accounts: env.all_token_account_data(),
        matcher_contexts: env.all_matcher_context_data(),
        economic_lamports: env.all_economic_account_lamports(),
        token_supply: env.token_supply_observed(),
    }
}

fn inv_058_max_position_config() -> MarketConfig {
    const CAPITAL: u128 = 20_000_000_000;
    const TOKEN_BALANCE: u64 = CAPITAL as u64;
    MarketConfig {
        initial_price: 100,
        actor_deposits: [CAPITAL; PRIMARY_ACTOR_COUNT],
        actor_token_balances: [TOKEN_BALANCE; PRIMARY_ACTOR_COUNT],
        ..MarketConfig::default()
    }
}

#[test]
fn v16_program_split_fills_cannot_cross_position_or_side_oi_cap_on_any_route_pair() {
    const TAKER: usize = 0;
    const MAKER: usize = 1;
    const ASSET: u16 = 0;
    const PRICE: u64 = 100;

    assert_eq!(
        percolator::MAX_TRADE_SIZE_Q,
        percolator::MAX_POSITION_ABS_Q,
        "the scalar and cumulative account-position ceilings changed; expand this matrix"
    );
    assert_eq!(
        percolator::MAX_POSITION_ABS_Q,
        percolator::MAX_OI_SIDE_Q,
        "the account and side-OI ceilings changed; add the newly distinct partition"
    );
    let maximum_leg_notional = percolator::MAX_POSITION_ABS_Q
        .checked_mul(u128::from(percolator::MAX_ORACLE_PRICE))
        .expect("published maximum quantity/price product fits u128")
        / POS_SCALE;
    assert_eq!(
        maximum_leg_notional,
        percolator::MAX_ACCOUNT_NOTIONAL,
        "the liquidation/config notional domain changed; add a distinct public boundary"
    );

    let max_q = i128::try_from(percolator::MAX_POSITION_ABS_Q).expect("position cap fits i128");
    for (first_index, first_route) in INV_058_TRADE_ROUTES.into_iter().enumerate() {
        for (final_index, final_route) in INV_058_TRADE_ROUTES.into_iter().enumerate() {
            let mut seed = [0x58; 32];
            seed[0] = first_index as u8;
            seed[1] = final_index as u8;
            let mut env = V16Svm::new(seed, inv_058_max_position_config());

            let first = execute_trade_route(
                &mut env,
                first_route,
                TAKER,
                MAKER,
                ASSET,
                max_q - 1,
                PRICE,
                0,
            )
            .unwrap_or_else(|error| {
                panic!("{first_route:?}->{final_route:?} max-1 fill failed: {error}")
            });
            assert!(first.compute_units < TX_CU_LIMIT);

            let final_fill =
                execute_trade_route(&mut env, final_route, TAKER, MAKER, ASSET, 1, PRICE, 0)
                    .unwrap_or_else(|error| {
                        panic!(
                            "{first_route:?}->{final_route:?} final one-atom fill failed: {error}"
                        )
                    });
            assert!(final_fill.compute_units < TX_CU_LIMIT);

            let (_, capped_group) = env.primary_market_state();
            assert_eq!(
                capped_group.assets[ASSET as usize].oi_eff_long_q,
                percolator::MAX_OI_SIDE_Q
            );
            assert_eq!(
                capped_group.assets[ASSET as usize].oi_eff_short_q,
                percolator::MAX_OI_SIDE_Q
            );
            assert_eq!(
                active_leg_for_asset(&env.primary_portfolio(TAKER), ASSET as usize).basis_pos_q,
                max_q
            );
            assert_eq!(
                active_leg_for_asset(&env.primary_portfolio(MAKER), ASSET as usize).basis_pos_q,
                -max_q
            );

            for reject_route in INV_058_TRADE_ROUTES {
                if matches!(reject_route, TradeRoute::Cpi | TradeRoute::BatchCpi) {
                    env.ensure_primary_matcher_enabled(MAKER)
                        .expect("prepare public CPI capability before rollback snapshot");
                }
                let before = inv_058_economic_snapshot(&env);
                let error =
                    execute_trade_route(&mut env, reject_route, TAKER, MAKER, ASSET, 1, PRICE, 0)
                        .expect_err("one atom beyond the cumulative position/OI cap must reject");
                assert!(
                    error.contains("Custom(18)") || error.contains("custom program error: 0x12"),
                    "{first_route:?}->{final_route:?} over-cap {reject_route:?} returned {error}"
                );
                assert_eq!(
                    inv_058_economic_snapshot(&env),
                    before,
                    "{first_route:?}->{final_route:?} over-cap {reject_route:?} did not roll back exactly"
                );
            }

            let close =
                execute_trade_route(&mut env, final_route, TAKER, MAKER, ASSET, -max_q, PRICE, 0)
                    .unwrap_or_else(|error| {
                        panic!("{first_route:?}->{final_route:?} exact-max exit failed: {error}")
                    });
            assert!(close.compute_units < TX_CU_LIMIT);
            let (_, terminal_group) = env.primary_market_state();
            assert_eq!(terminal_group.assets[ASSET as usize].oi_eff_long_q, 0);
            assert_eq!(terminal_group.assets[ASSET as usize].oi_eff_short_q, 0);
            assert!(!has_active_leg_for_asset(
                &env.primary_portfolio(TAKER),
                ASSET as usize
            ));
            assert!(!has_active_leg_for_asset(
                &env.primary_portfolio(MAKER),
                ASSET as usize
            ));
            assert_public_stock_census("INV-058 split-cap terminal", &env)
                .expect("split-cap route preserves independent stock reconciliation");
            assert_public_encumbrance_census("INV-058 split-cap terminal", &env)
                .expect("split-cap route preserves independent encumbrance reconciliation");
        }
    }
}

#[test]
fn v16_program_distinct_owner_pairs_cannot_cross_shared_side_oi_cap() {
    const ASSET: u16 = 0;
    const PRICE: u64 = 100;
    const EXTRA_TAKER: usize = 4;

    fn enable_matcher_if_needed(env: &mut V16Svm, route: TradeRoute, maker: usize) {
        if matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi) {
            env.ensure_primary_matcher_enabled(maker)
                .expect("prepare public CPI capability");
        }
    }

    fn assert_positions(env: &V16Svm, expected: [i128; PRIMARY_ACTOR_COUNT]) {
        let (_, group) = env.primary_market_state();
        let asset = &group.assets[ASSET as usize];
        let mut recomputed = [0u128; 2];
        for (actor, expected_q) in expected.into_iter().enumerate() {
            let account = env.primary_portfolio(actor);
            let actual = if expected_q == 0 {
                assert!(
                    !has_active_leg_for_asset(&account, ASSET as usize),
                    "actor {actor} unexpectedly has active exposure"
                );
                0
            } else {
                let leg = active_leg_for_asset(&account, ASSET as usize);
                assert_eq!(leg.market_id, asset.market_id);
                assert_eq!(leg.a_basis, ADL_ONE);
                assert_eq!(leg.basis_pos_q, expected_q, "actor {actor} position");
                leg.basis_pos_q
            };
            assert!(actual.unsigned_abs() <= percolator::MAX_POSITION_ABS_Q);
            if actual > 0 {
                recomputed[0] = recomputed[0].checked_add(actual.unsigned_abs()).unwrap();
            } else if actual < 0 {
                recomputed[1] = recomputed[1].checked_add(actual.unsigned_abs()).unwrap();
            }
        }
        assert_eq!(
            [asset.oi_eff_long_q, asset.oi_eff_short_q],
            recomputed,
            "maintained side OI must equal the sum of distinct owner-pair exposure"
        );
        assert_eq!(recomputed, [percolator::MAX_OI_SIDE_Q; 2]);
        assert_public_stock_census("INV-058 side-OI cap", env)
            .expect("side-OI cap route preserves stock reconciliation");
        assert_public_encumbrance_census("INV-058 side-OI cap", env)
            .expect("side-OI cap route preserves encumbrance reconciliation");
    }

    assert!(
        PRIMARY_ACTOR_COUNT > EXTRA_TAKER,
        "side-OI split test needs two pairs plus one extra taker"
    );
    let max = i128::try_from(percolator::MAX_OI_SIDE_Q).unwrap();
    let first = max / 2;
    let second = max - first;
    assert!(first > 0 && second > 0 && first < max && second < max);

    for (fill_index, fill_route) in INV_058_TRADE_ROUTES.into_iter().enumerate() {
        let mut seed = [0x58; 32];
        seed[0] = 0x42;
        seed[1] = fill_index as u8;
        let mut env = V16Svm::new(seed, inv_058_max_position_config());
        env.begin_public_trace();

        enable_matcher_if_needed(&mut env, fill_route, 1);
        execute_trade_route(&mut env, fill_route, 0, 1, ASSET, first, PRICE, 0)
            .unwrap_or_else(|error| panic!("{fill_route:?} first side-OI fill failed: {error}"));
        enable_matcher_if_needed(&mut env, fill_route, 3);
        execute_trade_route(&mut env, fill_route, 2, 3, ASSET, second, PRICE, 0)
            .unwrap_or_else(|error| panic!("{fill_route:?} second side-OI fill failed: {error}"));

        let expected = [first, -first, second, -second, 0];
        assert_positions(&env, expected);
        assert!(
            expected
                .into_iter()
                .all(|q| q.unsigned_abs() < percolator::MAX_POSITION_ABS_Q),
            "each populated account must remain below its own cap so rejection isolates side OI"
        );

        for reject_route in INV_058_TRADE_ROUTES {
            enable_matcher_if_needed(&mut env, reject_route, 1);
            let before = inv_058_economic_snapshot(&env);
            let error =
                execute_trade_route(&mut env, reject_route, EXTRA_TAKER, 1, ASSET, 1, PRICE, 0)
                    .expect_err("one more atom must reject at the shared side-OI cap");
            let code = PercolatorError::EngineInvalidLeg as u32;
            assert!(
                error.contains(&format!("Custom({code})")),
                "{fill_route:?}->{reject_route:?} returned {error}"
            );
            assert_eq!(
                inv_058_economic_snapshot(&env),
                before,
                "{fill_route:?}->{reject_route:?} side-OI cap rollback"
            );
        }

        let trace = env.finish_public_trace();
        trace
            .validate_public_execution()
            .expect("distinct-pair side-OI cap uses public instructions only");
    }
}

#[test]
fn v16_program_post_transition_caps_match_across_reduction_and_cross_zero_histories() {
    const PRICE: u64 = 100;

    fn assert_limits(env: &V16Svm, expected_q: i128) -> ([u128; 2], [u128; 2]) {
        let (_, group) = env.primary_market_state();
        let asset = &group.assets[0];
        assert_eq!(asset.effective_price, PRICE);
        assert_eq!(asset.raw_oracle_target_price, PRICE);
        let mut recomputed_oi = [0u128; 2];
        let mut notionals = [0u128; 2];
        for actor in 0..PRIMARY_ACTOR_COUNT {
            let account = env.primary_portfolio(actor);
            let expected = match actor {
                0 => expected_q,
                1 => -expected_q,
                _ => 0,
            };
            assert_eq!(
                percolator::active_bitmap_count_ones(active_bitmap(&account)),
                u32::from(expected != 0),
                "actor {actor} active count"
            );
            let mut notional = 0;
            for encoded in &account.legs {
                let leg = encoded.try_to_runtime().expect("decode public leg");
                if !leg.active {
                    continue;
                }
                assert_eq!(leg.asset_index, 0);
                assert_eq!(leg.market_id, asset.market_id);
                assert_eq!(leg.basis_pos_q, expected, "actor {actor} position");
                let (side, current_a, epoch, mode) = match leg.side {
                    SideV16::Long => (0, asset.a_long, asset.epoch_long, asset.mode_long),
                    SideV16::Short => (1, asset.a_short, asset.epoch_short, asset.mode_short),
                };
                assert_eq!(side, usize::from(expected < 0));
                assert_eq!(mode, SideModeV16::Normal);
                assert_eq!(leg.epoch_snap, epoch);
                // These fixed-price bilateral histories have no ADL or reset.
                assert_eq!(current_a, ADL_ONE);
                assert_eq!(leg.a_basis, current_a);
                let abs_q = leg.basis_pos_q.unsigned_abs();
                assert!(abs_q <= percolator::MAX_POSITION_ABS_Q);
                recomputed_oi[side] = recomputed_oi[side].checked_add(abs_q).unwrap();
                let product = abs_q.checked_mul(u128::from(PRICE)).unwrap();
                notional += product / POS_SCALE + u128::from(product % POS_SCALE != 0);
            }
            assert!(notional <= percolator::MAX_ACCOUNT_NOTIONAL);
            if actor < 2 {
                let cert = health_cert(&account);
                assert!(cert.valid);
                assert_eq!(
                    cert.certified_worst_case_loss, notional,
                    "actor {actor} notional"
                );
                notionals[actor] = cert.certified_worst_case_loss;
            }
        }
        let maintained_oi = [asset.oi_eff_long_q, asset.oi_eff_short_q];
        assert_eq!(maintained_oi, recomputed_oi);
        assert_eq!(recomputed_oi, [expected_q.unsigned_abs(); 2]);
        assert!(maintained_oi
            .into_iter()
            .all(|q| q <= percolator::MAX_OI_SIDE_Q));
        assert_public_stock_census("INV-058 post-transition limits", env)
            .expect("stocks reconcile after every accepted transition");
        (maintained_oi, notionals)
    }

    let max_q = i128::try_from(percolator::MAX_POSITION_ABS_Q).unwrap();
    let unit_q = i128::try_from(POS_SCALE).unwrap();
    assert!(max_q > unit_q + 1);
    let histories = [
        ("direct", vec![(TradeRoute::NoCpi, -(max_q - unit_q))]),
        (
            "reduce",
            vec![(TradeRoute::BatchCpi, -max_q), (TradeRoute::NoCpi, unit_q)],
        ),
        (
            "cross-zero",
            vec![(TradeRoute::NoCpi, unit_q), (TradeRoute::BatchCpi, -max_q)],
        ),
        (
            "split-close-reopen",
            vec![
                (TradeRoute::NoCpi, unit_q),
                (TradeRoute::Cpi, -unit_q),
                (TradeRoute::BatchNoCpi, -(max_q - unit_q)),
            ],
        ),
    ];
    for direction in [-1i128, 1] {
        let mut reference_outcomes = None;
        for (history, prefix) in &histories {
            let mut env = V16Svm::new([0x58; 32], inv_058_max_position_config());
            env.begin_public_trace();
            let mut expected_q = 0i128;
            for &(route, size_q) in prefix {
                let result =
                    execute_trade_route(&mut env, route, 0, 1, 0, direction * size_q, PRICE, 0)
                        .unwrap_or_else(|error| {
                            panic!("{history} direction={direction} {route:?}: {error}")
                        });
                assert!(result.compute_units < TX_CU_LIMIT);
                expected_q += direction * size_q;
                assert_limits(&env, expected_q);
            }
            assert_eq!(expected_q, -direction * (max_q - unit_q));
            let mut outcomes = vec![assert_limits(&env, expected_q)];

            // Fill the released headroom, reduce it again, and retry at the same cap.
            for (route, size_q, accepted) in [
                (TradeRoute::Cpi, -(unit_q + 1), false),
                (TradeRoute::BatchNoCpi, -unit_q, true),
                (TradeRoute::BatchCpi, -1, false),
                (TradeRoute::Cpi, unit_q, true),
                (TradeRoute::NoCpi, -(unit_q + 1), false),
                (TradeRoute::BatchCpi, -unit_q, true),
                (TradeRoute::NoCpi, max_q, true),
            ] {
                assert!(size_q.unsigned_abs() <= percolator::MAX_TRADE_SIZE_Q);
                if matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi) {
                    env.ensure_primary_matcher_enabled(1)
                        .expect("prepare CPI capability before rollback snapshot");
                }
                let before = inv_058_economic_snapshot(&env);
                let result =
                    execute_trade_route(&mut env, route, 0, 1, 0, direction * size_q, PRICE, 0);
                let label = format!("{history} direction={direction} {route:?} size={size_q}");
                if accepted {
                    let success = result.unwrap_or_else(|error| panic!("{label}: {error}"));
                    assert!(success.compute_units < TX_CU_LIMIT, "{label}");
                    expected_q += direction * size_q;
                } else {
                    assert_eq!(
                        (expected_q + direction * size_q).unsigned_abs(),
                        percolator::MAX_POSITION_ABS_Q + 1
                    );
                    let error = result.expect_err("cumulative cap+1 must reject");
                    assert!(
                        error.contains("Custom(18)")
                            || error.contains("custom program error: 0x12"),
                        "{label}: {error}"
                    );
                    assert_eq!(inv_058_economic_snapshot(&env), before, "{label} rollback");
                }
                outcomes.push(assert_limits(&env, expected_q));
            }
            assert_eq!(expected_q, 0);
            if let Some(reference) = &reference_outcomes {
                assert_eq!(
                    &outcomes, reference,
                    "{history} post-transition limits differ"
                );
            } else {
                reference_outcomes = Some(outcomes);
            }
            env.finish_public_trace()
                .validate_public_execution()
                .expect("all position transitions must execute through public instructions");
        }
    }
}

#[test]
fn v16_program_recreated_counterparty_preserves_post_transition_cumulative_limits() {
    const PRICE: u64 = 100;
    const CAPITAL: u128 = 20_000_000_000;

    struct Ledger {
        positions: [[i128; 2]; PRIMARY_ACTOR_COUNT],
        capital: [u128; PRIMARY_ACTOR_COUNT],
        max_trade_cu: u64,
    }

    impl Ledger {
        fn check(&self, env: &V16Svm) {
            let (_, group) = env.primary_market_state();
            let mut oi = [[0u128; 2]; 2];
            for actor in 0..PRIMARY_ACTOR_COUNT {
                let account = env.primary_portfolio(actor);
                assert_eq!(account.capital.get(), self.capital[actor]);
                assert_eq!(account.pnl.get(), 0, "fixed-price, zero-fee history");
                let expected = self.positions[actor];
                let mut decoded = [0i128; 2];
                for encoded in &account.legs {
                    let leg = encoded.try_to_runtime().expect("decode public leg");
                    if !leg.active {
                        continue;
                    }
                    let index = leg.asset_index as usize;
                    assert!(index < 2, "no unmodeled asset exposure");
                    assert_eq!(decoded[index], 0, "one canonical net leg per asset");
                    let asset = &group.assets[index];
                    let (a, epoch, mode) = match leg.side {
                        SideV16::Long => (asset.a_long, asset.epoch_long, asset.mode_long),
                        SideV16::Short => (asset.a_short, asset.epoch_short, asset.mode_short),
                    };
                    // No ADL/reset is induced here: assert, rather than assume, basis == effective.
                    assert_eq!(a, ADL_ONE);
                    assert_eq!(leg.a_basis, a);
                    assert_eq!(leg.epoch_snap, epoch);
                    assert_eq!(mode, SideModeV16::Normal);
                    assert_eq!(leg.market_id, asset.market_id);
                    assert_eq!(
                        leg.side,
                        if expected[index] > 0 {
                            SideV16::Long
                        } else {
                            SideV16::Short
                        }
                    );
                    decoded[index] = leg.basis_pos_q;
                }
                assert_eq!(decoded, expected, "actor {actor} effective quantities");
                assert_eq!(
                    percolator::active_bitmap_count_ones(active_bitmap(&account)),
                    expected.into_iter().filter(|q| *q != 0).count() as u32
                );
                let mut notional = 0u128;
                for (index, q) in expected.into_iter().enumerate() {
                    let abs_q = q.unsigned_abs();
                    assert!(abs_q <= percolator::MAX_POSITION_ABS_Q);
                    let side = usize::from(q < 0);
                    oi[index][side] = oi[index][side].checked_add(abs_q).unwrap();
                    let product = abs_q.checked_mul(u128::from(PRICE)).unwrap();
                    notional = notional
                        .checked_add(product / POS_SCALE + u128::from(product % POS_SCALE != 0))
                        .unwrap();
                }
                assert!(notional <= percolator::MAX_ACCOUNT_NOTIONAL);
                let cert = health_cert(&account);
                if expected != [0; 2] {
                    assert!(cert.valid, "exposed actor {actor} has a certificate");
                }
                if cert.valid {
                    assert_eq!(
                        cert.certified_worst_case_loss, notional,
                        "actor {actor} ceil notional"
                    );
                }
            }
            for (index, asset) in group.assets.iter().enumerate() {
                let expected = oi.get(index).copied().unwrap_or([0; 2]);
                assert_eq!([asset.oi_eff_long_q, asset.oi_eff_short_q], expected);
                assert_eq!(expected[0], expected[1]);
                assert!(expected.into_iter().all(|q| q <= percolator::MAX_OI_SIDE_Q));
                if index < 2 {
                    assert_eq!(asset.effective_price, PRICE);
                    assert_eq!(asset.raw_oracle_target_price, PRICE);
                }
            }
            let capital: u128 = self.capital.iter().sum();
            assert_eq!(group.c_tot, capital);
            assert_eq!(group.vault, capital);
            assert_eq!(group.insurance, 0);
            assert_public_stock_census("INV-058 recreated cap", env).unwrap();
            assert_public_encumbrance_census("INV-058 recreated cap", env).unwrap();
        }

        fn trade(
            &mut self,
            env: &mut V16Svm,
            route: TradeRoute,
            taker: usize,
            maker: usize,
            legs: &[(u16, i128)],
            accepted: bool,
        ) {
            if matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi) {
                env.ensure_primary_matcher_enabled(maker)
                    .expect("fresh public matcher grant before rollback frame");
            }
            let mut next = self.positions;
            for &(asset, size) in legs {
                assert!(size != 0 && size.unsigned_abs() <= percolator::MAX_TRADE_SIZE_Q);
                next[taker][asset as usize] =
                    next[taker][asset as usize].checked_add(size).unwrap();
                next[maker][asset as usize] =
                    next[maker][asset as usize].checked_sub(size).unwrap();
            }
            let projected_max = next
                .iter()
                .flatten()
                .map(|q| q.unsigned_abs())
                .max()
                .unwrap();
            if accepted {
                assert!(projected_max <= percolator::MAX_POSITION_ABS_Q);
            } else {
                assert_eq!(projected_max, percolator::MAX_POSITION_ABS_Q + 1);
            }
            let before = inv_058_economic_snapshot(env);
            let (_, group) = env.primary_market_state();
            let result = match route {
                TradeRoute::NoCpi | TradeRoute::Cpi => {
                    assert_eq!(legs.len(), 1);
                    execute_trade_route(env, route, taker, maker, legs[0].0, legs[0].1, PRICE, 0)
                }
                TradeRoute::BatchNoCpi => env.batch_trade_no_cpi(
                    taker,
                    maker,
                    legs.iter()
                        .map(|&(asset_index, size_q)| BatchTradeLeg {
                            asset_index,
                            market_id: group.assets[asset_index as usize].market_id,
                            size_q,
                            exec_price: PRICE,
                            fee_bps: 0,
                        })
                        .collect(),
                ),
                TradeRoute::BatchCpi => env.batch_trade_cpi(
                    taker,
                    maker,
                    legs.iter()
                        .map(|&(asset_index, size_q)| BatchTradeCpiLeg {
                            asset_index,
                            market_id: group.assets[asset_index as usize].market_id,
                            size_q,
                            fee_bps: 0,
                            limit_price: PRICE,
                        })
                        .collect(),
                ),
            };
            let label = format!("{route:?} taker={taker} maker={maker} legs={legs:?}");
            if accepted {
                let success = result.unwrap_or_else(|error| panic!("{label}: {error}"));
                self.max_trade_cu = self.max_trade_cu.max(success.compute_units);
                assert!(success.compute_units < TX_CU_LIMIT, "{label}");
                self.positions = next;
                assert_eq!(
                    env.all_token_account_data(),
                    before.token_accounts,
                    "{label} custody"
                );
            } else {
                let error = result.expect_err("freshly authorized cap+1 must reject");
                let code = PercolatorError::EngineInvalidLeg as u32;
                assert!(
                    error.contains(&format!("Custom({code})")),
                    "{label}: {error}"
                );
                assert_eq!(
                    inv_058_economic_snapshot(env),
                    before,
                    "{label} exact rollback"
                );
            }
            self.check(env);
        }
    }

    fn boundary_legs(route: TradeRoute, size: i128) -> Vec<(u16, i128)> {
        if matches!(route, TradeRoute::BatchNoCpi | TradeRoute::BatchCpi) {
            // Distinct, valid asset first: a later cap rejection must also frame this leg.
            vec![(1, size.signum()), (0, size)]
        } else {
            vec![(0, size)]
        }
    }

    let max = i128::try_from(percolator::MAX_POSITION_ABS_Q).unwrap();
    assert_eq!(percolator::MAX_POSITION_ABS_Q, percolator::MAX_OI_SIDE_Q);
    assert!(max > 4);
    for direction in [-1i128, 1] {
        for (route_index, route) in INV_058_TRADE_ROUTES.into_iter().enumerate() {
            let other_route = INV_058_TRADE_ROUTES[(route_index + 1) % 4];
            let mut config = inv_058_max_position_config();
            config.actor_token_balances[1] = u64::try_from(2 * CAPITAL).unwrap();
            let mut env = V16Svm::new([0x58; 32], config);
            env.begin_public_trace();
            let mut ledger = Ledger {
                positions: [[0; 2]; PRIMARY_ACTOR_COUNT],
                capital: [CAPITAL; PRIMARY_ACTOR_COUNT],
                max_trade_cu: 0,
            };
            ledger.check(&env);

            // Fill the shared ceiling while moving each counterleg fragment to actor 2.
            // Actor 0 stays capped during recreation; actor 1 cannot reset that exposure.
            for size in [direction * (max - 1), direction] {
                ledger.trade(&mut env, route, 0, 1, &[(0, size)], true);
                ledger.trade(&mut env, other_route, 1, 2, &[(0, size)], true);
            }
            assert_eq!(ledger.positions[0][0], direction * max);
            assert_eq!(ledger.positions[1], [0; 2]);
            let old_id = env.primary_portfolio_id(1);
            let address = env.actors[1].portfolio;
            let destination_before = env.token_amount(env.actors[1].destination_token);
            env.withdraw_primary(1, CAPITAL)
                .expect("withdraw flat original incarnation");
            ledger.capital[1] = 0;
            ledger.check(&env);
            assert_eq!(
                env.token_amount(env.actors[1].destination_token) - destination_before,
                CAPITAL as u64
            );
            let survivors = [env.primary_portfolio_data(0), env.primary_portfolio_data(2)];
            env.close_primary_portfolio(1)
                .expect("close original incarnation");
            assert_eq!(env.svm.get_account(&address).unwrap().lamports, 0);
            env.fund_closed_primary_portfolio(1, 1_000_000_000)
                .expect("System Program rent funding");
            env.reinitialize_primary_portfolio(1)
                .expect("public same-address initialization");
            assert_eq!(env.actors[1].portfolio, address);
            assert!(env.primary_portfolio_id(1) > old_id);
            assert_eq!(
                [env.primary_portfolio_data(0), env.primary_portfolio_data(2)],
                survivors
            );
            ledger.check(&env);
            env.deposit_primary(1, CAPITAL)
                .expect("fund replacement with existing SPL tokens");
            ledger.capital[1] = CAPITAL;
            ledger.check(&env);

            // Fresh identities, epochs and matcher grants exclude stale-consent rejection.
            // Both account roles reject even though the recreated account itself is flat.
            for probe_route in INV_058_TRADE_ROUTES {
                for (taker, maker, size) in [(0, 1, direction), (1, 0, -direction)] {
                    ledger.trade(
                        &mut env,
                        probe_route,
                        taker,
                        maker,
                        &boundary_legs(probe_route, size),
                        false,
                    );
                }
            }

            // Two released atoms are reusable exactly once, including a real two-asset batch.
            ledger.trade(&mut env, other_route, 0, 2, &[(0, -2 * direction)], true);
            ledger.trade(
                &mut env,
                route,
                0,
                1,
                &boundary_legs(route, direction),
                true,
            );
            ledger.trade(&mut env, other_route, 0, 1, &[(0, direction)], true);
            ledger.trade(
                &mut env,
                route,
                0,
                1,
                &boundary_legs(route, direction),
                false,
            );

            // Consolidate, cross zero without flattening first, and refill the opposite ceiling.
            ledger.trade(
                &mut env,
                other_route,
                0,
                2,
                &[(0, -direction * (max - 2))],
                true,
            );
            assert_eq!(ledger.positions[0][0], 2 * direction);
            ledger.trade(&mut env, route, 0, 1, &[(0, -4 * direction)], true);
            assert_eq!(ledger.positions[0][0], -2 * direction);
            ledger.trade(
                &mut env,
                other_route,
                0,
                1,
                &[(0, -direction * (max - 2))],
                true,
            );
            assert_eq!(
                ledger.positions[1][0],
                direction * max,
                "replacement reaches its own cap"
            );
            ledger.trade(
                &mut env,
                route,
                0,
                1,
                &boundary_legs(route, -direction),
                false,
            );
            ledger.trade(&mut env, route, 0, 1, &[(0, direction * max)], true);
            let auxiliary = ledger.positions[0][1];
            if auxiliary != 0 {
                ledger.trade(&mut env, other_route, 0, 1, &[(1, -auxiliary)], true);
            }
            assert_eq!(ledger.positions, [[0; 2]; PRIMARY_ACTOR_COUNT]);
            let trace = env.finish_public_trace();
            trace
                .validate_public_execution()
                .expect("public instructions only, complete rollback");
            let rejects = trace.steps.iter().filter(|step| !step.succeeded).count();
            assert_eq!(rejects, 10);
            let max_public_cu = trace
                .steps
                .iter()
                .filter_map(|step| step.compute_units)
                .max()
                .unwrap();
            println!("INV-058 recreation direction={direction} route={route:?}: steps={} rejects={rejects} max_trade_cu={} max_public_cu={max_public_cu}", trace.steps.len(), ledger.max_trade_cu);
        }
    }
}

#[test]
fn v16_program_cumulative_tvl_cap_enforced_and_withdrawable() {
    let mut env = V16CuEnv::new();
    let a = Keypair::new();
    let pa = env.create_portfolio(&a);
    let b = Keypair::new();
    let pb = env.create_portfolio(&b);
    env.deposit(&a, pa, percolator::MAX_VAULT_TVL);
    assert_eq!(env.market_state().1.vault, percolator::MAX_VAULT_TVL);

    let src = env.token_account_for_mint(env.mint, b.pubkey(), 100);
    env.svm.expire_blockhash();
    let over_cap = env.send(
        env.deposit_ix(pb, 100),
        vec![
            AccountMeta::new(b.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(pb, false),
            AccountMeta::new(src, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&b],
    );
    assert!(
        over_cap.is_err(),
        "deposit pushing the vault over MAX_VAULT_TVL must reject"
    );
    assert_eq!(env.portfolio_state(pb).capital.get(), 0);
    assert_eq!(env.market_state().1.vault, percolator::MAX_VAULT_TVL);
    assert_eq!(env.token_amount(src), 100);

    let (dest, _) = env.withdraw_with_cu(&a, pa, 1_000_000);
    assert_eq!(
        env.token_amount(dest),
        1_000_000,
        "funds remain withdrawable from a capped vault"
    );
}

#[test]
fn v16_program_deposit_withdraw_amount_over_u64_max_rejects_no_truncation() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);

    let over = u128::from(u64::MAX) + 1;
    let (_, group_before) = env.market_state();
    let capital_before = env.portfolio_state(portfolio).capital.get();

    let src = env.token_account(owner.pubkey(), 1_000);
    env.svm.expire_blockhash();
    let deposit = env.send(
        env.deposit_ix(portfolio, over),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(src, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    let deposit_err = deposit.expect_err("over-u64 deposit must reject");
    assert!(deposit_err.contains("Custom(9)"));
    assert_eq!(env.token_amount(src), 1_000);
    assert_eq!(env.portfolio_state(portfolio).capital.get(), capital_before);
    assert_eq!(env.market_state().1.c_tot, group_before.c_tot);

    let dest = env.token_account(owner.pubkey(), 0);
    env.svm.expire_blockhash();
    let withdraw = env.send(
        env.withdraw_ix(portfolio, over),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    let withdraw_err = withdraw.expect_err("over-u64 withdraw must reject");
    assert!(withdraw_err.contains("Custom(9)"));
    assert_eq!(env.token_amount(dest), 0);
    assert_eq!(env.portfolio_state(portfolio).capital.get(), capital_before);
}

#[test]
fn v16_program_rebalance_reduce_overshoot_clamps_to_flat_no_flip() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 5_000, 10_000, 1_000);
    let long_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short_owner = Keypair::new();
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 1_000_000);
    env.deposit(&short_owner, short, 1_000_000);
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        100,
        0,
    );
    let basis_before = env.portfolio_state(long).legs[0].basis_pos_q.get();
    assert!(basis_before > 0);

    env.svm.expire_blockhash();
    let reduce = env.send(
        ProgInstruction::RebalanceReduce {
            portfolio_id: env.portfolio_id(long),
            position_epoch: env.portfolio_position_epoch(long),
            asset_index: 0,
            reduce_q: 3 * POS_SCALE,
        },
        vec![
            AccountMeta::new(long_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(long, false),
        ],
        &[&long_owner],
    );
    assert!(
        reduce.is_ok(),
        "oversized owner reduce should succeed by clamping: {reduce:?}"
    );

    let basis_after = env.portfolio_state(long).legs[0].basis_pos_q.get();
    assert_eq!(basis_after, 0, "over-reduce clamps exactly to flat");
    assert!(basis_after >= 0, "reduce must not open opposite-side risk");
    let (_, group) = env.market_state();
    assert_eq!(
        group.assets[0].oi_eff_long_q, group.assets[0].oi_eff_short_q,
        "OI remains balanced after clamped reduce"
    );
    assert!(group.vault >= group.c_tot + group.insurance);
}

fn fill_to_one_below_tvl_cap(env: &mut V16CuEnv) {
    let depositor = Keypair::new();
    let portfolio = env.create_portfolio(&depositor);
    env.deposit(&depositor, portfolio, percolator::MAX_VAULT_TVL - 1);
    assert_eq!(env.market_state().1.vault, percolator::MAX_VAULT_TVL - 1);
    assert_eq!(
        env.token_amount(env.vault) as u128,
        percolator::MAX_VAULT_TVL - 1
    );
}

#[test]
fn v16_program_topups_cannot_bypass_cumulative_tvl_cap() {
    {
        let mut env = V16CuEnv::new();
        fill_to_one_below_tvl_cap(&mut env);
        let admin = env.admin.insecure_clone();
        let source = env.token_account(admin.pubkey(), 2);
        let ledger = env.insurance_ledger_account();
        let market_before = env.svm.get_account(&env.market).unwrap();
        let ledger_before = env.svm.get_account(&ledger).unwrap();
        let source_before = env.svm.get_account(&source).unwrap();
        let vault_before = env.svm.get_account(&env.vault).unwrap();

        env.svm.expire_blockhash();
        let result = env.send(
            ProgInstruction::TopUpInsurance {
                authority_epoch: 0,
                intent_id: 0,
                market_id: 0,
                amount: 2,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(ledger, false),
            ],
            &[&admin],
        );
        assert!(result.is_err());
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&ledger).unwrap(), ledger_before);
        assert_eq!(env.svm.get_account(&source).unwrap(), source_before);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

        let ok_source = env.token_account(env.admin.pubkey(), 1);
        env.svm.expire_blockhash();
        env.top_up_insurance_from_admin_token_with_cu(ok_source, 1);
        let (_, group) = env.market_state();
        assert_eq!(group.vault, percolator::MAX_VAULT_TVL);
        assert_eq!(group.insurance, 1);
        assert_eq!(env.token_amount(ok_source), 0);
        assert_eq!(
            env.token_amount(env.vault) as u128,
            percolator::MAX_VAULT_TVL
        );
    }

    {
        let mut env = V16CuEnv::new();
        fill_to_one_below_tvl_cap(&mut env);
        let admin = env.admin.insecure_clone();
        let source = env.token_account(admin.pubkey(), 2);
        let ledger = env.insurance_ledger_account();
        let market_before = env.svm.get_account(&env.market).unwrap();
        let ledger_before = env.svm.get_account(&ledger).unwrap();
        let source_before = env.svm.get_account(&source).unwrap();
        let vault_before = env.svm.get_account(&env.vault).unwrap();

        env.svm.expire_blockhash();
        let result = env.send(
            ProgInstruction::TopUpInsuranceDomain {
                authority_epoch: 0,
                intent_id: 0,
                market_id: 0,
                domain: 0,
                amount: 2,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(ledger, false),
            ],
            &[&admin],
        );
        assert!(result.is_err());
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&ledger).unwrap(), ledger_before);
        assert_eq!(env.svm.get_account(&source).unwrap(), source_before);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

        let ok_source = env.token_account(admin.pubkey(), 1);
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::TopUpInsuranceDomain {
                authority_epoch: 0,
                intent_id: 0,
                market_id: 0,
                domain: 0,
                amount: 1,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(ok_source, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&admin],
        )
        .expect("one-atom domain insurance top-up reaches the exact cap");
        let (_, group) = env.market_state();
        assert_eq!(group.vault, percolator::MAX_VAULT_TVL);
        assert_eq!(group.insurance, 1);
        assert_eq!(group.insurance_domain_budget[0], 1);
        assert_eq!(group.insurance_domain_budget_remaining_total, 1);
        assert_eq!(env.token_amount(ok_source), 0);
        assert_eq!(
            env.token_amount(env.vault) as u128,
            percolator::MAX_VAULT_TVL
        );
    }

    {
        let mut env = V16CuEnv::new();
        fill_to_one_below_tvl_cap(&mut env);
        let admin = env.admin.insecure_clone();
        let source = env.token_account(admin.pubkey(), 2);
        let ledger = env.backing_domain_ledger_account();
        let market_before = env.svm.get_account(&env.market).unwrap();
        let ledger_before = env.svm.get_account(&ledger).unwrap();
        let source_before = env.svm.get_account(&source).unwrap();
        let vault_before = env.svm.get_account(&env.vault).unwrap();

        env.svm.expire_blockhash();
        let result = env.send(
            ProgInstruction::TopUpBackingBucket {
                authority_epoch: 0,
                intent_id: 0,
                market_id: 0,
                domain: 1,
                backing_fee_bps: 0,
                insurance_share_bps: 0,
                amount: 2,
                expiry_slot: 10_000,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
                AccountMeta::new(ledger, false),
            ],
            &[&admin],
        );
        assert!(result.is_err());
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&ledger).unwrap(), ledger_before);
        assert_eq!(env.svm.get_account(&source).unwrap(), source_before);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

        let ok_source = env.token_account(env.admin.pubkey(), 1);
        env.svm.expire_blockhash();
        env.top_up_backing_bucket_from_admin_token_with_cu(ok_source, 1, 1, 10_000);
        let (_, group) = env.market_state();
        assert_eq!(group.vault, percolator::MAX_VAULT_TVL);
        assert_eq!(
            group.source_backing_buckets[1].fresh_unliened_backing_num,
            BOUND_SCALE
        );
        assert_eq!(
            group.source_credit[1].fresh_reserved_backing_num,
            BOUND_SCALE
        );
        assert_eq!(env.token_amount(ok_source), 0);
        assert_eq!(
            env.token_amount(env.vault) as u128,
            percolator::MAX_VAULT_TVL
        );
    }
}

fn setup_batch_cap_env(cap: u16) -> (V16CuEnv, Keypair, Pubkey, Keypair, Pubkey) {
    let mut env = V16CuEnv::new_with_init_params_and_market_capacity(
        V16CuMarketParams {
            max_portfolio_assets: cap,
            maintenance_margin_bps: 10_000,
            initial_margin_bps: 10_000,
            max_price_move_bps_per_slot: 10_000,
            ..V16CuMarketParams::default()
        },
        70,
    );
    assert_eq!(env.market_state().1.config.max_market_slots, cap as u32);
    env.activate_asset(cap, 20, 100);
    let (_, group) = env.market_state();
    assert_eq!(group.config.max_market_slots, u32::from(cap + 1));
    assert_eq!(group.config.max_portfolio_assets, cap);

    let taker = Keypair::new();
    let lp = Keypair::new();
    let taker_account = env.create_portfolio(&taker);
    let lp_account = env.create_portfolio(&lp);
    env.deposit(&taker, taker_account, 100_000_000);
    env.deposit(&lp, lp_account, 100_000_000);
    (env, taker, taker_account, lp, lp_account)
}

fn batch_nocpi_legs(count: u16) -> Vec<BatchTradeLeg> {
    (0..count)
        .map(|asset_index| BatchTradeLeg {
            asset_index,
            market_id: first_generation_market_id(asset_index),
            size_q: POS_SCALE as i128,
            exec_price: 100,
            fee_bps: 0,
        })
        .collect()
}

fn batch_cpi_legs(count: u16) -> Vec<BatchTradeCpiLeg> {
    (0..count)
        .map(|asset_index| BatchTradeCpiLeg {
            asset_index,
            market_id: first_generation_market_id(asset_index),
            size_q: POS_SCALE as i128,
            fee_bps: 0,
            limit_price: 0,
        })
        .collect()
}

#[test]
fn v16_program_batch_over_portfolio_leg_cap_rejects_atomically() {
    const CAP: u16 = percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS;
    const OVER: u16 = CAP + 1;

    {
        let (mut env, taker, taker_account, lp, lp_account) = setup_batch_cap_env(CAP);
        let market_before = env.svm.get_account(&env.market).unwrap();
        let taker_before = env.svm.get_account(&taker_account).unwrap();
        let lp_before = env.svm.get_account(&lp_account).unwrap();

        env.svm.expire_blockhash();
        let rejected = env.send(
            env.batch_trade_no_cpi_ix(taker_account, lp_account, batch_nocpi_legs(OVER)),
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(lp.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker_account, false),
                AccountMeta::new(lp_account, false),
            ],
            &[&taker, &lp],
        );
        assert!(rejected.is_err());
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&taker_account).unwrap(), taker_before);
        assert_eq!(env.svm.get_account(&lp_account).unwrap(), lp_before);

        env.svm.expire_blockhash();
        let ok = env.send(
            env.batch_trade_no_cpi_ix(taker_account, lp_account, batch_nocpi_legs(CAP)),
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(lp.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker_account, false),
                AccountMeta::new(lp_account, false),
            ],
            &[&taker, &lp],
        );
        assert!(ok.is_ok(), "exact-cap BatchTradeNoCpi must execute: {ok:?}");
        assert_eq!(
            percolator::active_bitmap_count_ones(active_bitmap(
                &env.portfolio_state(taker_account)
            )),
            u32::from(CAP)
        );
    }

    {
        let (mut env, taker, taker_account, lp, lp_account) = setup_batch_cap_env(CAP);
        let matcher_program = Pubkey::new_unique();
        let matcher_bytes =
            std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
        env.svm.add_program(matcher_program, &matcher_bytes);
        let (ctx, delegate, _) = env.init_auth_matcher_context(matcher_program, &lp, lp_account);
        let market_before = env.svm.get_account(&env.market).unwrap();
        let taker_before = env.svm.get_account(&taker_account).unwrap();
        let lp_before = env.svm.get_account(&lp_account).unwrap();
        let ctx_before = env.svm.get_account(&ctx).unwrap();

        env.svm.expire_blockhash();
        let rejected = env.send(
            env.batch_trade_cpi_ix(taker_account, lp_account, batch_cpi_legs(OVER)),
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker_account, false),
                AccountMeta::new(lp_account, false),
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new(ctx, false),
                AccountMeta::new_readonly(delegate, false),
            ],
            &[&taker],
        );
        assert!(rejected.is_err());
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&taker_account).unwrap(), taker_before);
        assert_eq!(env.svm.get_account(&lp_account).unwrap(), lp_before);
        assert_eq!(env.svm.get_account(&ctx).unwrap(), ctx_before);

        env.svm.expire_blockhash();
        let ok = env.send(
            env.batch_trade_cpi_ix(taker_account, lp_account, batch_cpi_legs(CAP)),
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker_account, false),
                AccountMeta::new(lp_account, false),
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new(ctx, false),
                AccountMeta::new_readonly(delegate, false),
            ],
            &[&taker],
        );
        assert!(ok.is_ok(), "exact-cap BatchTradeCpi must execute: {ok:?}");
        assert_eq!(
            percolator::active_bitmap_count_ones(active_bitmap(
                &env.portfolio_state(taker_account)
            )),
            u32::from(CAP)
        );
    }
}

#[test]
fn v16_program_batch_tradecpi_configured_leg_cap_rejects_before_hostile_matcher_cpi() {
    const CAP: u16 = 2;
    const OVER: u16 = CAP + 1;

    let (mut env, taker, taker_account, lp, lp_account) = setup_batch_cap_env(CAP);
    let hostile = Pubkey::new_unique();
    env.svm.add_program(
        hostile,
        &std::fs::read(hostile_matcher_program_path()).unwrap(),
    );
    let ctx = Pubkey::new_unique();
    let delegate = matcher_delegate_key(
        &env.program_id,
        &env.market,
        &lp_account,
        &lp.pubkey(),
        &hostile,
        &ctx,
    );
    env.svm
        .set_account(
            delegate,
            Account {
                lamports: 1_000_000_000,
                data: vec![],
                owner: Pubkey::default(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm
        .set_account(
            ctx,
            Account {
                lamports: 1_000_000_000,
                data: vec![0u8; MATCHER_CONTEXT_LEN],
                owner: hostile,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.set_matcher_config(hostile, &lp, lp_account, ctx, delegate, 1);

    let send = |env: &mut V16CuEnv, count: u16| {
        let mut data = vec![0u8; MATCHER_CONTEXT_LEN];
        data[0] = 0;
        env.svm
            .set_account(
                ctx,
                Account {
                    lamports: 1_000_000_000,
                    data,
                    owner: hostile,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        env.svm.expire_blockhash();
        env.send(
            env.batch_trade_cpi_ix(taker_account, lp_account, batch_cpi_legs(count)),
            vec![
                AccountMeta::new(taker.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker_account, false),
                AccountMeta::new(lp_account, false),
                AccountMeta::new_readonly(hostile, false),
                AccountMeta::new(ctx, false),
                AccountMeta::new_readonly(delegate, false),
            ],
            &[&taker],
        )
    };

    let exact_cap_err =
        send(&mut env, CAP).expect_err("exact-cap hostile batch reaches matcher validation");
    assert!(exact_cap_err.contains("InvalidAccountData"));
    assert!(!exact_cap_err.contains("Custom(9)"));

    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker_account).unwrap();
    let lp_before = env.svm.get_account(&lp_account).unwrap();
    let ctx_before = env.svm.get_account(&ctx).unwrap();
    let over_cap_err =
        send(&mut env, OVER).expect_err("over-cap BatchTradeCpi rejects before matcher CPI");
    assert!(over_cap_err.contains("Custom(9)"));
    assert!(!over_cap_err.contains("InvalidAccountData"));
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker_account).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp_account).unwrap(), lp_before);
    assert_eq!(env.svm.get_account(&ctx).unwrap(), ctx_before);
}
