//! INV-028 - Source-domain realizability cap.
//!
//! Normative obligation: Source-backed credit cannot survive beyond its realizable backing, and
//! reconciliation of a vanished claim cannot permanently lock funded user exposure.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_source_lien_reversal_exit_matrix_preserves_bounded_exit` generates a positive
//! source-backed claim, liens it through a public risk increase, reverses the authenticated mark,
//! and tests canonical crank, unilateral reduction, and all four trade routes from independent
//! worlds. Every route must unwind the vanished claim and reduce exposure in bounded calls; any
//! rejected attempt must preserve exact SVM rollback while real capital and custody remain.
//! `v16_program_cross_domain_rounding_exit_matrix_preserves_bounded_exit` independently constructs
//! two fractional source domains in both asset orders, reverses one source, and requires a bounded
//! public exit while preserving exact rollback for every rejected prefix.
//! `v16_program_flat_source_lien_route_matrix_preserves_bounded_claim_exit` flattens all exposure
//! while retaining a real source lien, proves conversion and close reject atomically before the
//! lien is released, traverses each CPI/no-CPI single/batch trade family from an independent world,
//! and then requires the sole public crank to release the obsolete encumbrance before complete
//! conversion, withdrawal, and portfolio close.
//! `v16_program_reciprocal_cross_asset_cycle_cannot_mint_credit` runs all four trade families in
//! both close orders. Full recertification must net each portfolio's equal winner/loser legs before
//! any source claim becomes usable; neither the reciprocal exposures nor unattached backing may
//! admit a risk increase, and the complete cycle returns every user balance and source ledger.
//! `v16_program_lien_backed_admission_preserves_new_domain_settlement_and_exit` carries a
//! retained claim through cross-asset risk admission and later favorable settlement in both
//! account orders. The new domain must obtain its own backing while the original reservation
//! remains attributed, then both claims must reach bounded release, conversion and payout.
//!
//! Guarantee boundary: the reversal matrix certifies the fixed source-lien unwind across all six
//! wrapper routes. Cross-domain fractional support and flat backed claims both have public progress
//! regressions with exact rollback and terminal custody reconciliation.

use super::*;
use crate::support::fuzz_model::{
    assert_public_encumbrance_census, assert_public_stock_census, execute_trade_route,
};
use crate::support::v16_svm::{MarketConfig, V16Svm};
use percolator::{BOUND_SCALE, POS_SCALE};
use percolator_prog::ix::CrankObservationHint;

#[test]
fn v16_program_lien_backed_admission_preserves_new_domain_settlement_and_exit() {
    const CAPITAL: u128 = 201;
    const PEER_CAPITAL: u128 = 1_000;
    const PROVIDER_BACKING: u128 = 100;
    const OLD_GAIN: u128 = 20 * 5;
    const NEW_UNITS: i128 = 21;
    const NEW_GAIN: u128 = NEW_UNITS as u128;
    const ADMISSION_LIEN: u128 = NEW_UNITS as u128 * 100 / 10 - CAPITAL;
    const OLD_DOMAIN: usize = 1;
    const NEW_DOMAIN: usize = 3;

    for winner_first in [true, false] {
        let mut seed = [0x28; 32];
        seed[0] ^= u8::from(winner_first);
        let mut env = V16Svm::new(
            seed,
            MarketConfig {
                initial_price: 100,
                h_max: 4,
                maintenance_margin_bps: 1_000,
                initial_margin_bps: 1_000,
                max_price_move_bps_per_slot: 500,
                max_accrual_dt_slots: 1,
                min_funding_lifetime_slots: 1,
                actor_deposits: [CAPITAL, PEER_CAPITAL, 1, 1, 1],
                actor_token_balances: [CAPITAL as u64, PEER_CAPITAL as u64, 1, 1, 1],
                ..MarketConfig::default()
            },
        );
        let supply = env.token_supply_observed();
        let provider_before = env.token_amount(env.provider_source_token)
            + env.token_amount(env.provider_destination_token);
        let unrelated = (2..env.actors.len())
            .map(|actor| env.primary_portfolio_data(actor))
            .collect::<Vec<_>>();
        let census = |env: &V16Svm| {
            assert_public_stock_census("INV-028 admission/latent settlement", env).unwrap();
            assert_public_encumbrance_census("INV-028 admission/latent settlement", env).unwrap();
            assert_eq!(env.token_supply_observed(), supply);
            assert_eq!(
                (2..env.actors.len())
                    .map(|actor| env.primary_portfolio_data(actor))
                    .collect::<Vec<_>>(),
                unrelated
            );
        };
        let source = |env: &V16Svm, domain: usize| {
            env.primary_portfolio(0)
                .source_domains
                .into_iter()
                .find(|source| source.is_occupied() && source.domain.get() as usize == domain)
                .expect("expected independently attributed source")
        };
        env.begin_public_trace();
        env.top_up_backing_bucket(OLD_DOMAIN as u16, PROVIDER_BACKING, 100)
            .expect("initialize the historical source's fresh reservation bucket");
        census(&env);
        let vault = env.token_amount(env.vault);
        execute_trade_route(
            &mut env,
            TradeRoute::NoCpi,
            0,
            1,
            0,
            20 * POS_SCALE as i128,
            100,
            0,
        )
        .expect("capital funds the initial position");
        census(&env);
        env.warp_to_slot(2);
        env.push_auth_mark(0, 2, 105).expect("first favorable mark");
        census(&env);
        for actor in [1, 0] {
            let rank = |env: &V16Svm| {
                let account = env.primary_portfolio(actor);
                u128::from(2 - env.primary_market_state().1.assets[0].slot_last)
                    + if actor == 0 {
                        account.pnl.get().abs_diff(OLD_GAIN as i128)
                    } else {
                        account.capital.get().abs_diff(PEER_CAPITAL - OLD_GAIN)
                    }
            };
            for _ in 0..4 {
                let before = rank(&env);
                if before == 0 {
                    break;
                }
                env.crank(
                    actor,
                    2,
                    vec![CrankObservationHint {
                        asset_index: 0,
                        oracle_accounts: 0,
                    }],
                )
                .expect("settle historical claim and backing");
                assert!(rank(&env) < before);
                census(&env);
            }
            assert_eq!(rank(&env), 0);
        }
        execute_trade_route(
            &mut env,
            TradeRoute::NoCpi,
            0,
            1,
            0,
            -20 * POS_SCALE as i128,
            105,
            0,
        )
        .expect("retain the historical claim after flattening its asset");
        census(&env);
        assert_eq!(env.primary_portfolio(0).pnl.get(), OLD_GAIN as i128);
        assert_eq!(
            source(&env, OLD_DOMAIN).source_claim_bound_num.get(),
            OLD_GAIN * BOUND_SCALE
        );
        assert_eq!(source(&env, OLD_DOMAIN).source_claim_liened_num.get(), 0);
        assert_eq!(
            env.primary_portfolio(1).capital.get(),
            PEER_CAPITAL - OLD_GAIN
        );

        // This admission needs claim credit: its 210-atom IM exceeds 201 atoms of capital.
        assert!(NEW_UNITS as u128 * 100 / 10 > CAPITAL);
        execute_trade_route(
            &mut env,
            TradeRoute::NoCpi,
            0,
            1,
            1,
            NEW_UNITS * POS_SCALE as i128,
            100,
            0,
        )
        .expect("the old source backs admission of a distinct asset");
        census(&env);
        let admitted = source(&env, OLD_DOMAIN);
        let lien = admitted.source_lien_counterparty_backing_num.get();
        assert_eq!(lien, ADMISSION_LIEN * BOUND_SCALE);
        assert!(ADMISSION_LIEN > 0 && ADMISSION_LIEN <= OLD_GAIN);
        assert_eq!(
            admitted.source_claim_bound_num.get(),
            OLD_GAIN * BOUND_SCALE
        );
        assert_eq!(admitted.source_claim_liened_num.get(), lien);
        assert_eq!(env.primary_portfolio(0).capital.get(), CAPITAL);
        assert_eq!(
            env.primary_portfolio(0)
                .source_domains
                .iter()
                .filter(|s| s.is_occupied())
                .count(),
            1
        );
        let old_credit = env.primary_market_state().1.source_credit[OLD_DOMAIN];
        let old_bucket = env.primary_market_state().1.source_backing_buckets[OLD_DOMAIN];
        assert_eq!(old_credit.valid_liened_backing_num, lien);
        assert_eq!(old_bucket.valid_liened_backing_num, lien);
        assert_eq!(old_credit.credit_rate_num, percolator::CREDIT_RATE_SCALE);
        assert!(old_bucket.fresh_unliened_backing_num >= OLD_GAIN * BOUND_SCALE);
        assert_eq!(
            old_bucket.fresh_unliened_backing_num + lien,
            (OLD_GAIN + PROVIDER_BACKING) * BOUND_SCALE
        );
        let admitted_exposure = |env: &V16Svm| {
            for actor in [0, 1] {
                let account = env.primary_portfolio(actor);
                let legs = account
                    .legs
                    .iter()
                    .filter(|leg| leg.active != 0)
                    .collect::<Vec<_>>();
                assert_eq!(legs.len(), 1);
                assert_eq!(legs[0].asset_index.get(), 1);
                assert_eq!(
                    legs[0].basis_pos_q.get(),
                    if actor == 0 { NEW_UNITS } else { -NEW_UNITS } * POS_SCALE as i128
                );
                let mut resources = account
                    .source_domains
                    .iter()
                    .filter(|s| s.is_occupied())
                    .map(|s| s.domain.get())
                    .collect::<std::collections::BTreeSet<_>>();
                for leg in legs {
                    resources.extend([2 * leg.asset_index.get(), 2 * leg.asset_index.get() + 1]);
                }
                assert_eq!(
                    resources.into_iter().collect::<Vec<_>>(),
                    if actor == 0 {
                        vec![1, 2, 3]
                    } else {
                        vec![2, 3]
                    }
                );
            }
        };
        admitted_exposure(&env);

        env.warp_to_slot(3);
        env.push_auth_mark(1, 3, 101)
            .expect("admitted asset develops a new gain");
        census(&env);
        let order = if winner_first { [0, 1] } else { [1, 0] };
        let mut settled = [false; 2];
        let mut settlement_calls = 0;
        for actor in order {
            let expected_capital = if actor == 0 {
                CAPITAL
            } else {
                PEER_CAPITAL - OLD_GAIN - NEW_GAIN
            };
            let expected_pnl = if actor == 0 {
                (OLD_GAIN + NEW_GAIN) as i128
            } else {
                0
            };
            let rank = |env: &V16Svm| {
                let account = env.primary_portfolio(actor);
                u128::from(3 - env.primary_market_state().1.assets[1].slot_last)
                    + account.capital.get().abs_diff(expected_capital)
                    + account.pnl.get().abs_diff(expected_pnl)
            };
            for _ in 0..4 {
                let before = rank(&env);
                if before == 0 {
                    break;
                }
                let peer_before = env.primary_portfolio_data(1 - actor);
                env.crank(
                    actor,
                    3,
                    vec![CrankObservationHint {
                        asset_index: 1,
                        oracle_accounts: 0,
                    }],
                )
                .expect("admission retains a bounded settlement path");
                settlement_calls += 1;
                assert!(
                    rank(&env) < before,
                    "each crank settles value or consumes accrual backlog"
                );
                assert_eq!(env.primary_portfolio_data(1 - actor), peer_before);
                census(&env);
                admitted_exposure(&env);
                assert_eq!(source(&env, OLD_DOMAIN), admitted);
                assert_eq!(
                    env.primary_market_state().1.source_credit[OLD_DOMAIN],
                    old_credit
                );
                assert_eq!(
                    env.primary_market_state().1.source_backing_buckets[OLD_DOMAIN],
                    old_bucket
                );
                assert_eq!(env.token_amount(env.vault), vault);
            }
            assert_eq!(
                rank(&env),
                0,
                "input-derived settlement endpoint must be reached"
            );
            settled[actor] = true;
            let group = env.primary_market_state().1;
            let claim = if settled[0] {
                NEW_GAIN * BOUND_SCALE
            } else {
                0
            };
            let backing = if settled[1] {
                NEW_GAIN * BOUND_SCALE
            } else {
                0
            };
            let new_credit = group.source_credit[NEW_DOMAIN];
            assert_eq!(new_credit.positive_claim_bound_num, claim);
            assert_eq!(new_credit.exact_positive_claim_num, claim);
            assert_eq!(new_credit.fresh_reserved_backing_num, backing);
            assert_eq!(
                group.source_backing_buckets[NEW_DOMAIN].fresh_unliened_backing_num,
                backing
            );
            assert_eq!(new_credit.valid_liened_backing_num, 0);
            assert_eq!(new_credit.impaired_liened_backing_num, 0);
            assert!(
                claim * new_credit.credit_rate_num / percolator::CREDIT_RATE_SCALE <= backing,
                "new credit cannot borrow the other domain's reserved atoms"
            );
            if settled[0] {
                let new_source = source(&env, NEW_DOMAIN);
                assert_eq!(new_source.source_claim_bound_num.get(), claim);
                assert_eq!(new_source.source_claim_liened_num.get(), 0);
            }
        }
        assert!(settlement_calls > 0 && settlement_calls <= 8);
        assert_eq!(
            env.primary_portfolio(0)
                .source_domains
                .iter()
                .filter(|s| s.is_occupied())
                .count(),
            2
        );

        execute_trade_route(
            &mut env,
            TradeRoute::NoCpi,
            0,
            1,
            1,
            -NEW_UNITS * POS_SCALE as i128,
            101,
            0,
        )
        .expect("settled admitted risk remains closeable");
        census(&env);
        for _ in 0..4 {
            if source(&env, OLD_DOMAIN).source_claim_liened_num.get() == 0 {
                break;
            }
            let before = source(&env, OLD_DOMAIN).source_claim_liened_num.get();
            env.crank(0, 3, vec![])
                .expect("flat owner releases obsolete source reservation");
            assert!(source(&env, OLD_DOMAIN).source_claim_liened_num.get() < before);
            census(&env);
        }
        assert_eq!(source(&env, OLD_DOMAIN).source_claim_liened_num.get(), 0);
        assert_eq!(
            env.primary_market_state().1.source_backing_buckets[OLD_DOMAIN]
                .fresh_unliened_backing_num,
            (OLD_GAIN + PROVIDER_BACKING) * BOUND_SCALE
        );

        let snapshot = |env: &V16Svm| {
            (
                env.market_data(false),
                env.all_primary_portfolio_data(),
                env.all_token_account_data(),
                env.all_matcher_context_data(),
                env.all_economic_account_lamports(),
            )
        };
        let before = snapshot(&env);
        let error = env
            .convert_released_pnl(0, OLD_GAIN + NEW_GAIN - 1)
            .expect_err("one-atom short cap must roll back both source consumptions");
        assert!(
            error.contains("Custom(21)"),
            "unexpected conversion rejection: {error}"
        );
        assert_eq!(snapshot(&env), before);
        census(&env);
        env.convert_released_pnl(0, OLD_GAIN + NEW_GAIN)
            .expect("both independently backed claims convert");
        census(&env);
        assert_eq!(env.token_amount(env.vault), vault);
        for (actor, payout) in [
            (0, CAPITAL + OLD_GAIN + NEW_GAIN),
            (1, PEER_CAPITAL - OLD_GAIN - NEW_GAIN),
        ] {
            let account = env.primary_portfolio(actor);
            assert!(percolator::active_bitmap_is_empty(
                account.active_bitmap.map(|w| w.get())
            ));
            assert_eq!(account.capital.get(), payout);
            assert_eq!((account.pnl.get(), account.reserved_pnl.get()), (0, 0));
            assert!(account.source_domains.iter().all(|s| !s.is_occupied()));
            env.withdraw_primary(actor, payout)
                .expect("complete funded owner payout");
            assert_eq!(
                env.token_amount(env.actors[actor].destination_token) as u128,
                payout
            );
            census(&env);
            env.close_primary_portfolio(actor)
                .expect("delete the fully exited portfolio");
            census(&env);
        }
        env.withdraw_backing_bucket(OLD_DOMAIN as u16, PROVIDER_BACKING)
            .expect("return the unused provider contribution");
        census(&env);
        assert_eq!(
            env.token_amount(env.provider_source_token)
                + env.token_amount(env.provider_destination_token),
            provider_before
        );
        let terminal = env.primary_market_state().1;
        assert_eq!(
            (terminal.c_tot, terminal.vault, terminal.insurance),
            (3, 3, 0)
        );
        assert_eq!(terminal.materialized_portfolio_count, 3);
        assert!(terminal
            .assets
            .iter()
            .all(|asset| asset.oi_eff_long_q == 0 && asset.oi_eff_short_q == 0));
        for domain in [OLD_DOMAIN, NEW_DOMAIN] {
            let source = terminal.source_credit[domain];
            assert_eq!(source.positive_claim_bound_num, 0);
            assert_eq!(source.fresh_reserved_backing_num, 0);
            assert_eq!(source.valid_liened_backing_num, 0);
            assert_eq!(source.impaired_liened_backing_num, 0);
        }
        let trace = env.finish_public_trace();
        trace
            .validate_public_execution()
            .expect("public history and exact rejected rollback");
        assert_eq!(trace.out_of_band_economic_mutations, 0);
        assert_eq!(trace.steps.iter().filter(|step| !step.succeeded).count(), 1);
        let max_cu = trace
            .steps
            .iter()
            .filter_map(|step| step.compute_units)
            .max()
            .unwrap();
        assert!(max_cu < 1_375_000);
        println!("INV-028 lien-backed new domain: winner_first={winner_first}, lien_atoms={}, settlement_calls={settlement_calls}, max_cu={max_cu}", lien / BOUND_SCALE);
    }
}

fn run_reciprocal_cross_asset_credit_cycle(route: TradeRoute, close_asset_one_first: bool) {
    const PRICE: u64 = 100;
    const MOVED_PRICE: u64 = 105;
    const SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const RISK_INCREASE_Q: i128 = 85 * POS_SCALE as i128;
    const BACKING: u128 = 100;

    let route_index = match route {
        TradeRoute::NoCpi => 0,
        TradeRoute::Cpi => 1,
        TradeRoute::BatchNoCpi => 2,
        TradeRoute::BatchCpi => 3,
    };
    let mut seed = [0x28; 32];
    seed[0] ^= route_index;
    seed[1] ^= u8::from(close_asset_one_first);

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [1_000, 1_000, 100_000, 100_000, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    let vault_before = env.token_amount(env.vault);
    let provider_tokens_before = env
        .token_amount(env.provider_source_token)
        .checked_add(env.token_amount(env.provider_destination_token))
        .expect("provider token total fits u64");
    let capital_before = (0..env.actors.len())
        .map(|actor| env.primary_portfolio(actor).capital.get())
        .collect::<Vec<_>>();
    env.begin_public_trace();

    execute_trade_route(&mut env, route, 0, 1, 0, SIZE_Q, PRICE, 0)
        .expect("actor 0 opens the asset-0 winning leg");
    execute_trade_route(&mut env, route, 1, 0, 1, SIZE_Q, PRICE, 0)
        .expect("actor 1 opens the asset-1 winning leg");
    env.warp_to_slot(2);
    for asset in [0u16, 1] {
        env.push_auth_mark(asset, 2, MOVED_PRICE)
            .expect("authenticated mark advances");
    }

    let close_asset = |env: &mut V16Svm, asset: u16| {
        if asset == 0 {
            execute_trade_route(env, route, 0, 1, 0, -SIZE_Q, MOVED_PRICE, 0)
        } else {
            execute_trade_route(env, route, 1, 0, 1, -SIZE_Q, MOVED_PRICE, 0)
        }
    };
    let (first_asset, second_asset) = if close_asset_one_first {
        (1u16, 0u16)
    } else {
        (0u16, 1u16)
    };
    close_asset(&mut env, first_asset).expect("first reciprocal leg closes");

    for actor in [0usize, 1] {
        let portfolio = env.primary_portfolio(actor);
        assert_eq!(
            portfolio.pnl.get(),
            0,
            "{route:?}/{first_asset}: full recertification must net reciprocal PnL"
        );
        assert!(
            portfolio
                .source_domains
                .iter()
                .all(|source| !source.is_occupied()),
            "{route:?}/{first_asset}: reciprocal exposure minted a source claim"
        );
    }

    let market_before = env.market_data(false);
    let actor_0_before = env.primary_portfolio_data(0);
    let actor_2_before = env.primary_portfolio_data(2);
    let custody_before = env.token_amount(env.vault);
    let unbacked = execute_trade_route(
        &mut env,
        route,
        0,
        2,
        first_asset,
        RISK_INCREASE_Q,
        MOVED_PRICE,
        0,
    );
    assert!(
        unbacked.is_err(),
        "{route:?}/{first_asset}: reciprocal legs admitted unbacked risk"
    );
    assert_eq!(env.market_data(false), market_before);
    assert_eq!(env.primary_portfolio_data(0), actor_0_before);
    assert_eq!(env.primary_portfolio_data(2), actor_2_before);
    assert_eq!(env.token_amount(env.vault), custody_before);

    env.top_up_backing_bucket(1, BACKING, 10)
        .expect("external source backing arrives");
    let market_after_backing = env.market_data(false);
    let actor_0_after_backing = env.primary_portfolio_data(0);
    let actor_2_after_backing = env.primary_portfolio_data(2);
    let custody_after_backing = env.token_amount(env.vault);
    let unattached_backing = execute_trade_route(
        &mut env,
        route,
        0,
        2,
        first_asset,
        RISK_INCREASE_Q,
        MOVED_PRICE,
        0,
    );
    assert!(
        unattached_backing.is_err(),
        "{route:?}/{first_asset}: backing without a claim created user credit"
    );
    assert_eq!(env.market_data(false), market_after_backing);
    assert_eq!(env.primary_portfolio_data(0), actor_0_after_backing);
    assert_eq!(env.primary_portfolio_data(2), actor_2_after_backing);
    assert_eq!(env.token_amount(env.vault), custody_after_backing);

    close_asset(&mut env, second_asset).expect("second reciprocal leg closes");
    env.withdraw_backing_bucket(1, BACKING)
        .expect("unused backing remains withdrawable");

    let (_, group) = env.primary_market_state();
    for actor in 0..env.actors.len() {
        let portfolio = env.primary_portfolio(actor);
        assert_eq!(portfolio.capital.get(), capital_before[actor]);
        assert_eq!(portfolio.pnl.get(), 0);
        assert!(portfolio
            .source_domains
            .iter()
            .all(|source| !source.is_occupied()));
    }
    assert!(group.source_credit.iter().all(|source| {
        source.positive_claim_bound_num == 0
            && source.fresh_reserved_backing_num == 0
            && source.valid_liened_backing_num == 0
            && source.impaired_liened_backing_num == 0
    }));
    assert_eq!(env.token_supply_observed(), supply_before);
    assert_eq!(env.token_amount(env.vault), vault_before);
    assert_eq!(group.vault, u128::from(vault_before));
    assert_eq!(
        env.token_amount(env.provider_source_token)
            .checked_add(env.token_amount(env.provider_destination_token))
            .expect("provider token total fits u64"),
        provider_tokens_before
    );

    let trace = env.finish_public_trace();
    trace
        .validate_public_execution()
        .expect("source-domain trace must be public and rollback-exact");
    assert_eq!(trace.out_of_band_economic_mutations, 0);
    let rejected = trace
        .steps
        .iter()
        .filter(|step| !step.succeeded)
        .collect::<Vec<_>>();
    assert_eq!(rejected.len(), 2);
    assert!(rejected.iter().all(|step| {
        step.rejected_exact_writable_rollback == Some(true)
            && step.rejected_no_program_lamport_delta == Some(true)
            && step.token_deltas.iter().all(|(_, delta)| *delta == 0)
    }));
}

#[test]
fn v16_program_reciprocal_cross_asset_cycle_cannot_mint_credit() {
    for route in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ] {
        for close_asset_one_first in [false, true] {
            run_reciprocal_cross_asset_credit_cycle(route, close_asset_one_first);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 8) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 64) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_028_source_lien_reversal_discovery.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_source_lien_reversal_exit_matrix_preserves_bounded_exit(
        seed in any::<[u8; 32]>(),
        // These sizes all pass public admission and create a nonzero source-credit lien. A 10%
        // increase is correctly rejected as LockActive during setup, before the reversal state
        // this property is intended to exercise.
        increase_divisor in prop::sample::select(vec![20u8, 25, 40]),
    ) {
        let discoveries = discover_source_lien_reversal_exit_locks(seed, increase_divisor);
        prop_assert!(
            discoveries.is_ok(),
            "source-lien reversal matrix failed for divisor {increase_divisor}: {}",
            discoveries.unwrap_err()
        );
        let discoveries = discoveries.unwrap();
        prop_assert_eq!(
            discoveries.len(),
            SourceLienReversalExitRoute::ALL.len(),
            "every public exit route needs an independent reversal world"
        );
        let violations = discoveries
            .iter()
            .filter(|discovery| !discovery.preserves_bounded_funded_exit())
            .collect::<Vec<_>>();
        prop_assert!(
            violations.is_empty(),
            "source-lien reversal failed to preserve bounded public exits: {violations:#?}"
        );
    }

    #[test]
    fn v16_program_cross_domain_rounding_exit_matrix_preserves_bounded_exit(
        seed in any::<[u8; 32]>(),
    ) {
        let discoveries = discover_cross_domain_rounding_exit_locks(seed);
        prop_assert!(
            discoveries.is_ok(),
            "cross-domain rounding matrix setup failed: {}",
            discoveries.unwrap_err()
        );
        let discoveries = discoveries.unwrap();
        prop_assert_eq!(
            discoveries.len(),
            CrossDomainRoundingOrder::ALL.len(),
            "both asset orders need independent public worlds"
        );
        for discovery in discoveries {
            prop_assert!(
                discovery.preserves_bounded_funded_exit(),
                "cross-domain rounding failed to retain a bounded public exit: {:?}",
                discovery
            );
        }
    }

    #[test]
    fn v16_program_flat_source_lien_route_matrix_preserves_bounded_claim_exit(
        seed in any::<[u8; 32]>(),
        provider_withdrawal in prop::sample::select(vec![50u128]),
    ) {
        let discoveries = discover_flat_source_lien_bounded_exits(seed, provider_withdrawal);
        prop_assert!(
            discoveries.is_ok(),
            "flat source-lien setup failed: {}",
            discoveries.unwrap_err()
        );
        let discoveries = discoveries.unwrap();
        prop_assert_eq!(
            discoveries.len(),
            FlatSourceLienEscapeRoute::ALL.len(),
            "every trade family needs an independent flat-lien escape world"
        );
        for discovery in discoveries {
            prop_assert!(
                discovery.preserves_bounded_backed_claim_exit(),
                "flat source lien lost its bounded terminal claim route: {:?}",
                discovery
            );
        }
    }
}
