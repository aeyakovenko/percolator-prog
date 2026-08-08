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
//! `v16_program_cross_domain_rounding_exit_matrix_discovers_funded_lock` independently constructs
//! two fractional source domains in both asset orders, reverses one source, and requires all six
//! public exit families plus a later honest crank to remain blocked before accepting a finding.
//! `v16_program_flat_source_lien_route_matrix_discovers_backed_claim_lock` flattens all exposure
//! while retaining a real source lien, then requires full/partial conversion, close, later honest
//! cranks, and CPI/no-CPI single/batch reopen-and-flatten escapes all to leave the backed PnL claim
//! uncollectible before accepting a finding.
//! `v16_program_reciprocal_cross_asset_cycle_cannot_mint_credit` runs all four trade families in
//! both close orders. Full recertification must net each portfolio's equal winner/loser legs before
//! any source claim becomes usable; neither the reciprocal exposures nor unattached backing may
//! admit a risk increase, and the complete cycle returns every user balance and source ledger.
//!
//! Guarantee boundary: the reversal matrix certifies the fixed source-lien unwind across all six
//! wrapper routes. The remaining tests in this file retain separate counterexamples for unrelated
//! cross-domain rounding and flat-lien findings.

use super::*;
use crate::support::fuzz_model::execute_trade_route;
use crate::support::v16_svm::{MarketConfig, V16Svm};
use percolator::POS_SCALE;

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
    fn v16_program_cross_domain_rounding_exit_matrix_discovers_funded_lock(
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
                discovery.is_persistent_funded_exit_lock(),
                "cross-domain rounding retained a public exit: {:?}",
                discovery
            );
        }
    }

    #[test]
    fn v16_program_flat_source_lien_route_matrix_discovers_backed_claim_lock(
        seed in any::<[u8; 32]>(),
        provider_withdrawal in prop::sample::select(vec![50u128]),
    ) {
        let discoveries = discover_flat_source_lien_claim_locks(seed, provider_withdrawal);
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
                discovery.is_persistent_backed_claim_lock(),
                "flat source lien retained a terminal claim route: {:?}",
                discovery
            );
        }
    }
}
