//! INV-027 - Protected principal seniority.
//!
//! Normative obligation: Existing junior claims and pending losses cannot consume fresh user
//! principal. Aggregate token conservation is necessary but not sufficient; attribution must show
//! that a new entrant pays only obligations created by that entrant's own episode.
//!
//! Evidence in this file (F over public I routes):
//! `v16_program_stale_cohort_route_matrix_preserves_historical_principal` creates a historical
//! source-backed winner and an uncrystallized loser entirely through public instructions. Every
//! single/batch CPI/no-CPI novation route must reject with exact SVM rollback while preserving the
//! original exposure. The winner can still reduce to zero through the owner route, the original
//! loser remains permissionlessly settleable in finite cranks, and the fresh entrant remains byte-
//! and token-identical. A separately funded control proves the same trade becomes admissible once
//! the complete cohort settles, so the safety gate is not a permanent exit lock.
//!
//! `v16_program_exact_kf_reversal_still_requires_cohort_settlement` returns K/F to the original
//! arithmetic value before the loser refreshes. The old leg still owns a generation membership,
//! unsafe novation rejects atomically, and the loser advances its epoch exactly once with zero PnL.
//!
//! `v16_program_haircut_conversion_retries_cannot_reuse_claim_or_backing` in the INV-031-owned
//! file closes the half-backed row without duplicating its expensive setup. Across every trade
//! route it creates a 2,000-atom claim backed by only 1,000 atoms, proves the exact terminally
//! withdrawable tranche equals the original loser's principal debit, and frames an unrelated
//! funded portfolio byte-for-byte before any replacement backing is added.
//!
//! Guarantee boundary: this certifies the stale K/F cohort and half-backed rows on the fixed
//! engine pin. Certificate-stale, pending-close, resolved-payout, insurance-withdrawal, and other
//! loss-stale rows still need one normalized public route-by-state matrix before INV-027 can be
//! promoted beyond partial coverage.
//! `v16_program_fully_backed_pnl_route_matrix_preserves_unrelated_principal` closes the Current
//! fully funded control row. It realizes the same directional profit through every trade family,
//! independently derives realizable support from the claim and backing stocks, and proves the
//! winner's token payout comes only from the losing episode while a fresh unrelated portfolio
//! remains byte- and token-identical. The underfunded final-leg row exposed a distinct crank
//! liveness counterexample and is owned by INV-071.
//!
//! Secondary coverage: INV-039. The same trace proves that novation cannot erase or transfer a
//! pre-existing cohort's pending loss obligation, while INV-027 owns principal attribution.

use super::*;
use crate::support::fuzz_model::execute_trade_route;
use crate::support::v16_svm::{MarketConfig, V16Svm};
use percolator::{BOUND_SCALE, CREDIT_RATE_SCALE, POS_SCALE};
use percolator_prog::ix::CrankObservationHint;

fn independent_source_support(
    group: &percolator_prog::state::MarketGroupV16,
    portfolio: &percolator_prog::state::PortfolioAccountV16,
    face: u128,
) -> u128 {
    let mut remaining_num = face
        .checked_mul(BOUND_SCALE)
        .expect("bounded test face scales");
    let mut support_num = 0u128;

    for source in portfolio
        .source_domains
        .iter()
        .filter(|source| source.is_occupied())
    {
        let domain = source.domain.get() as usize;
        let locked_num = source
            .source_claim_liened_num
            .get()
            .checked_add(source.source_claim_impaired_num.get())
            .expect("bounded source locks add");
        let lien_num = source
            .source_lien_effective_reserved
            .get()
            .checked_mul(BOUND_SCALE)
            .expect("bounded source lien scales")
            .min(remaining_num);
        support_num = support_num
            .checked_add(lien_num)
            .expect("bounded support adds");
        remaining_num -= lien_num;

        let claim_num = source
            .source_claim_bound_num
            .get()
            .checked_sub(locked_num)
            .expect("source locks do not exceed claim")
            .min(remaining_num);
        let credited_num = claim_num
            .checked_mul(group.source_credit[domain].credit_rate_num)
            .expect("bounded source credit multiplication")
            / CREDIT_RATE_SCALE;
        support_num = support_num
            .checked_add(credited_num)
            .expect("bounded credited support adds");
        remaining_num -= claim_num;
        if remaining_num == 0 {
            break;
        }
    }

    support_num / BOUND_SCALE
}

fn run_fully_backed_pnl_seniority_control(route: TradeRoute) {
    const WINNER: usize = 0;
    const LOSER: usize = 1;
    const FRESH_UNRELATED: usize = 2;
    const ASSET: u16 = 0;
    const SOURCE_DOMAIN: usize = 1;
    const OPEN_PRICE: u64 = 100;
    const WINNING_PRICE: u64 = 150;
    const SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const SETTLEMENT_SLOT: u64 = 11;

    let route_index = match route {
        TradeRoute::NoCpi => 0,
        TradeRoute::Cpi => 1,
        TradeRoute::BatchNoCpi => 2,
        TradeRoute::BatchCpi => 3,
    };
    let mut seed = [0x27; 32];
    seed[0] ^= route_index;
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: OPEN_PRICE,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [1_000, 1_000, 1_000, 1, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    let provider_tokens_before = env
        .token_amount(env.provider_source_token)
        .checked_add(env.token_amount(env.provider_destination_token))
        .expect("provider token total fits u64");
    let unrelated_account_before = env.primary_portfolio_data(FRESH_UNRELATED);
    let unrelated_source_before = env.token_amount(env.actors[FRESH_UNRELATED].source_token);
    let unrelated_destination_before =
        env.token_amount(env.actors[FRESH_UNRELATED].destination_token);
    let winner_destination_before = env.token_amount(env.actors[WINNER].destination_token);
    let loser_capital_before = env.primary_portfolio(LOSER).capital.get();
    env.begin_public_trace();

    execute_trade_route(&mut env, route, WINNER, LOSER, ASSET, SIZE_Q, OPEN_PRICE, 0)
        .expect("balanced pair opens");
    let oracle_accounts = env.primary_profile(ASSET as usize).oracle_leg_count;
    for (offset, mark) in (105u64..=WINNING_PRICE).step_by(5).enumerate() {
        let slot = 2u64
            .checked_add(u64::try_from(offset).expect("bounded mark-step index"))
            .expect("bounded mark slot");
        env.warp_to_slot(slot);
        env.push_auth_mark(ASSET, slot, mark)
            .expect("authenticated bounded winning mark lands");
        env.crank(
            4,
            slot,
            vec![CrankObservationHint {
                asset_index: ASSET,
                oracle_accounts,
            }],
        )
        .expect("empty keeper advances the bounded market mark");
    }
    execute_trade_route(
        &mut env,
        route,
        WINNER,
        LOSER,
        ASSET,
        -SIZE_Q,
        WINNING_PRICE,
        0,
    )
    .expect("balanced pair closes");

    let mut settlement_progress = 0usize;
    for round in 0..64u64 {
        let (_, group) = env.primary_market_state();
        if group.negative_pnl_account_count == 0 && !group.bankruptcy_hlock_active {
            break;
        }
        let now_slot = SETTLEMENT_SLOT
            .checked_add(round)
            .expect("bounded settlement slot");
        env.warp_to_slot(now_slot);
        for actor in [LOSER, WINNER, 4] {
            match env.crank(
                actor,
                now_slot,
                vec![CrankObservationHint {
                    asset_index: ASSET,
                    oracle_accounts,
                }],
            ) {
                Ok(_) => settlement_progress += 1,
                Err(error) if error.contains("Custom(21)") || error.contains("Custom(22)") => {}
                Err(error) => panic!(
                    "{route:?}: actor {actor} settlement returned an unexpected error at slot \
                     {now_slot}: {error}"
                ),
            }
        }
    }
    let (_, settled_group) = env.primary_market_state();
    assert_eq!(
        settled_group.negative_pnl_account_count,
        0,
        "{route:?}: negative account survived bounded public settlement; \
         progress={settlement_progress}, loser_capital={}, loser_pnl={}, bankruptcy_lock={}, \
         b_stale_accounts={}, loss_stale={}, current_slot={}",
        env.primary_portfolio(LOSER).capital.get(),
        env.primary_portfolio(LOSER).pnl.get(),
        settled_group.bankruptcy_hlock_active,
        settled_group.b_stale_account_count,
        settled_group.loss_stale_active,
        settled_group.current_slot
    );
    assert!(
        !settled_group.bankruptcy_hlock_active,
        "{route:?}: bankruptcy lock survived bounded public settlement"
    );

    let winner_before_conversion = env.primary_portfolio(WINNER);
    let released = u128::try_from(winner_before_conversion.pnl.get())
        .expect("winning route creates positive released PnL");
    let (_, group_before_conversion) = env.primary_market_state();
    let source = group_before_conversion.source_credit[SOURCE_DOMAIN];
    assert!(
        released > 0,
        "{route:?}: no released claim; winner_capital={}, winner_pnl={}, \
         loser_capital={}, loser_pnl={}, source={source:?}",
        winner_before_conversion.capital.get(),
        winner_before_conversion.pnl.get(),
        env.primary_portfolio(LOSER).capital.get(),
        env.primary_portfolio(LOSER).pnl.get()
    );
    let available_num = source
        .fresh_reserved_backing_num
        .checked_sub(source.valid_liened_backing_num)
        .and_then(|counterparty| {
            source
                .valid_liened_insurance_num
                .checked_add(source.impaired_liened_insurance_num)
                .and_then(|insurance_liened| {
                    source
                        .insurance_credit_reserved_num
                        .checked_sub(insurance_liened)
                        .and_then(|insurance| counterparty.checked_add(insurance))
                })
        })
        .expect("valid source encumbrance partition");
    let expected_rate = if source.positive_claim_bound_num == 0
        || available_num >= source.positive_claim_bound_num
    {
        CREDIT_RATE_SCALE
    } else {
        available_num
            .checked_mul(CREDIT_RATE_SCALE)
            .expect("bounded rate multiplication")
            / source.positive_claim_bound_num
    };
    assert_eq!(source.credit_rate_num, expected_rate);
    let expected_conversion = independent_source_support(
        &group_before_conversion,
        &winner_before_conversion,
        released,
    );
    assert!(expected_conversion > 0);
    let gross_profit = u128::from(WINNING_PRICE - OPEN_PRICE)
        .checked_mul(SIZE_Q.unsigned_abs())
        .expect("bounded gross profit multiplication")
        / POS_SCALE;
    assert_eq!(expected_conversion, gross_profit);

    let winner_capital_before = winner_before_conversion.capital.get();
    if let Err(error) = env.convert_released_pnl(WINNER, released) {
        panic!(
            "{route:?}: realizable source support did not convert: {error}; \
             market_mode={:?}, bankruptcy_lock={}, threshold_lock={}, loss_stale={}, \
             negative_accounts={}, stale_certs={}, b_stale_accounts={}, winner_stale={}, \
             winner_b_stale={}, winner_cert_valid={}, winner_active={:?}",
            group_before_conversion.mode,
            group_before_conversion.bankruptcy_hlock_active,
            group_before_conversion.threshold_stress_active,
            group_before_conversion.loss_stale_active,
            group_before_conversion.negative_pnl_account_count,
            group_before_conversion.stale_certificate_count,
            group_before_conversion.b_stale_account_count,
            winner_before_conversion.stale_state,
            winner_before_conversion.b_stale_state,
            winner_before_conversion.health_cert.valid,
            winner_before_conversion.active_bitmap
        );
    }
    let winner_after_conversion = env.primary_portfolio(WINNER);
    assert_eq!(
        winner_after_conversion
            .capital
            .get()
            .checked_sub(winner_capital_before)
            .expect("conversion cannot reduce capital"),
        expected_conversion
    );
    assert_eq!(winner_after_conversion.pnl.get(), 0);
    env.withdraw_primary(WINNER, expected_conversion)
        .expect("converted support is withdrawable");

    let winner_payout = env
        .token_amount(env.actors[WINNER].destination_token)
        .checked_sub(winner_destination_before)
        .expect("winner destination is monotonic");
    assert_eq!(
        winner_payout,
        u64::try_from(expected_conversion).expect("bounded payout fits u64")
    );
    let loser_capital_after = env.primary_portfolio(LOSER).capital.get();
    let loser_principal_debit = loser_capital_before
        .checked_sub(loser_capital_after)
        .expect("winning move cannot increase loser capital");
    assert_eq!(
        u128::from(winner_payout),
        loser_principal_debit,
        "{route:?}: payout escaped its losing-episode source"
    );
    let provider_tokens_after = env
        .token_amount(env.provider_source_token)
        .checked_add(env.token_amount(env.provider_destination_token))
        .expect("provider token total fits u64");
    assert_eq!(
        provider_tokens_after, provider_tokens_before,
        "provider funds were not part of this losing episode"
    );
    assert_eq!(
        env.primary_portfolio_data(FRESH_UNRELATED),
        unrelated_account_before,
        "{route:?}: unrelated portfolio bytes changed"
    );
    assert_eq!(
        env.token_amount(env.actors[FRESH_UNRELATED].source_token),
        unrelated_source_before
    );
    assert_eq!(
        env.token_amount(env.actors[FRESH_UNRELATED].destination_token),
        unrelated_destination_before
    );

    let (_, final_group) = env.primary_market_state();
    let capital_sum: u128 = (0..env.actors.len())
        .map(|actor| env.primary_portfolio(actor).capital.get())
        .sum();
    assert_eq!(final_group.c_tot, capital_sum);
    assert_eq!(final_group.vault, u128::from(env.token_amount(env.vault)));
    assert!(final_group.vault >= final_group.c_tot + final_group.insurance);
    assert_eq!(env.token_supply_observed(), supply_before);
    let trace = env.finish_public_trace();
    trace
        .validate_public_execution()
        .expect("protected-principal trace must be public and rollback-exact");
    assert_eq!(trace.out_of_band_economic_mutations, 0);
    assert!(trace
        .steps
        .iter()
        .filter(|step| !step.succeeded)
        .all(|step| {
            step.rejected_exact_writable_rollback == Some(true)
                && step.rejected_no_program_lamport_delta == Some(true)
                && step.token_deltas.iter().all(|(_, delta)| *delta == 0)
        }));
}

#[test]
fn v16_program_fully_backed_pnl_route_matrix_preserves_unrelated_principal() {
    for route in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ] {
        run_fully_backed_pnl_seniority_control(route);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: env_usize("PERCOLATOR_FUZZ_CASES", 4) as u32,
        max_shrink_iters: env_usize("PERCOLATOR_FUZZ_SHRINK_ITERS", 8) as u32,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                "proptest-regressions/inv_027_stale_cohort_seniority.txt",
            ),
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn v16_program_stale_cohort_route_matrix_preserves_historical_principal(
        seed in any::<[u8; 32]>()
    ) {
        let certifications = verify_stale_cohort_novation_guards(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert_eq!(certifications.len(), StaleCohortRoute::ALL.len());
        for (expected, certification) in StaleCohortRoute::ALL.into_iter().zip(&certifications) {
            prop_assert_eq!(certification.route, expected);
            prop_assert_eq!(certification.pre_stale_long_count, 0);
            prop_assert_eq!(certification.pre_stale_short_count, 1);
            prop_assert_eq!(certification.pre_negative_pnl_count, 0);
            prop_assert!(certification.unsafe_novation_rejected);
            prop_assert!(certification.rejection_exact_rollback);
            prop_assert_eq!(certification.rejected_winner_position_q, 10 * POS_SCALE as i128);
            prop_assert_eq!(certification.rejected_entrant_position_q, 0);
            prop_assert!(certification.owner_reduction_landed);
            prop_assert_eq!(certification.owner_position_after_reduction_q, 0);
            prop_assert!(certification.settlement_cranks > 0);
            prop_assert_eq!(certification.post_settlement_stale_long_count, 0);
            prop_assert_eq!(certification.post_settlement_stale_short_count, 0);
            prop_assert!(certification.entrant_untouched_by_historical_settlement);
            prop_assert!(certification.settled_cohort_retry_landed);
            prop_assert_eq!(certification.retry_winner_position_q, 0);
            prop_assert_eq!(certification.retry_entrant_position_q, 10 * POS_SCALE as i128);
            prop_assert!(certification.token_supply_conserved);
        }
    }

    #[test]
    fn v16_program_exact_kf_reversal_still_requires_cohort_settlement(
        seed in any::<[u8; 32]>()
    ) {
        let certification = verify_stale_cohort_exact_reversal(seed)
            .map_err(TestCaseError::fail)?;
        prop_assert!(certification.arithmetic_targets_reversed);
        prop_assert_eq!(certification.stale_short_count_before_settlement, 1);
        prop_assert_eq!(certification.stale_short_count_after_settlement, 0);
        prop_assert!(certification.loser_epoch_advanced);
        prop_assert!(certification.loser_pnl_zero);
        prop_assert!(certification.exact_rejection_rollback);
        prop_assert!(certification.token_supply_conserved);
    }
}
