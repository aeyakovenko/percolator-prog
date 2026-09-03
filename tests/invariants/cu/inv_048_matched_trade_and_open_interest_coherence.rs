//! INV-048 - Matched trade and open-interest coherence.
//!
//! Normative obligation: every matched public trade preserves signed quantity
//! and keeps stored market open interest coherent with the complete set of
//! active portfolio legs.
//!
//! This file is the primary directed CU/SBF owner for that guarantee. It executes
//! the four public trade routes from fresh state, then independently scans the two
//! affected portfolios and compares observed long/short basis with the maintained
//! O(1) market counters. The stateful INV-086 owner extends the oracle beyond fresh
//! state: its independent transition ledger tracks exact effective OI across
//! matched trades, retained trades, liquidation, rebalance, reset cleanup, and
//! forfeit without treating ADL-retained raw basis as effective OI.
//! `v16_program_position_mutation_composition_is_source_complete` closes the
//! induction boundary for the current wrapper surface. It pins the engine
//! revision whose attach, resize, pending-obligation, and clear kernels are
//! contract checked; inventories every wrapper callsite that can mutate a
//! position; rejects direct wrapper writes to either OI lane; and requires a
//! public census witness for every route class. A new engine pin, transition
//! callsite, or wrapper-owned OI write reopens the invariant.

use super::*;

#[derive(Clone, Copy, Debug)]
enum MatchedTradeRoute {
    TradeNoCpi,
    BatchTradeNoCpi,
    TradeCpi,
    BatchTradeCpi,
}

impl MatchedTradeRoute {
    const ALL: [Self; 4] = [
        Self::TradeNoCpi,
        Self::BatchTradeNoCpi,
        Self::TradeCpi,
        Self::BatchTradeCpi,
    ];
}

fn observed_oi_for_asset(portfolios: &[PortfolioAccountV16], asset_index: usize) -> (u128, u128) {
    let mut long = 0u128;
    let mut short = 0u128;
    for portfolio in portfolios {
        for leg in portfolio
            .legs
            .iter()
            .filter_map(|leg| leg.try_to_runtime().ok())
        {
            if !leg.active || leg.asset_index as usize != asset_index {
                continue;
            }
            if leg.basis_pos_q > 0 {
                long = long
                    .checked_add(leg.basis_pos_q as u128)
                    .expect("observed long OI overflow");
            } else if leg.basis_pos_q < 0 {
                short = short
                    .checked_add(leg.basis_pos_q.unsigned_abs())
                    .expect("observed short OI overflow");
            }
        }
    }
    (long, short)
}

fn assert_matched_oi_equals_portfolio_scan(
    env: &V16CuEnv,
    asset_index: usize,
    portfolios: &[Pubkey],
    expected_q: u128,
) {
    let states: Vec<_> = portfolios
        .iter()
        .map(|portfolio| env.portfolio_state(*portfolio))
        .collect();
    let (observed_long, observed_short) = observed_oi_for_asset(&states, asset_index);
    let group = env.market_state().1;
    let asset = group.assets[asset_index];
    assert_eq!(
        observed_long, expected_q,
        "portfolio scan long OI must equal expected route size",
    );
    assert_eq!(
        observed_short, expected_q,
        "portfolio scan short OI must equal expected route size",
    );
    assert_eq!(
        asset.oi_eff_long_q, observed_long,
        "stored long OI must equal complete active-leg scan",
    );
    assert_eq!(
        asset.oi_eff_short_q, observed_short,
        "stored short OI must equal complete active-leg scan",
    );
    assert_eq!(
        asset.oi_eff_long_q, asset.oi_eff_short_q,
        "live matched trade OI must remain balanced",
    );
}

fn run_matched_trade_route(route: MatchedTradeRoute) {
    const PRICE: u64 = 100;
    const SIZE_Q: i128 = 3 * POS_SCALE as i128;

    let mut env = V16CuEnv::new();
    env.configure_auth_mark_for_asset_as_admin(0, 1, PRICE);
    let taker = Keypair::new();
    let lp = Keypair::new();
    let taker_account = env.create_portfolio(&taker);
    let lp_account = env.create_portfolio(&lp);
    env.deposit(&taker, taker_account, 1_000_000);
    env.deposit(&lp, lp_account, 1_000_000);

    let matcher = if matches!(
        route,
        MatchedTradeRoute::TradeCpi | MatchedTradeRoute::BatchTradeCpi
    ) {
        let matcher_program = Pubkey::new_unique();
        let matcher_bytes =
            std::fs::read(auth_matcher_program_path()).expect("read auth matcher SBF");
        env.svm.add_program(matcher_program, &matcher_bytes);
        let (ctx, delegate, _) = env.init_auth_matcher_context(matcher_program, &lp, lp_account);
        Some((matcher_program, ctx, delegate))
    } else {
        None
    };

    match route {
        MatchedTradeRoute::TradeNoCpi => {
            env.trade_asset_with_cu(0, &taker, taker_account, &lp, lp_account, SIZE_Q, PRICE, 0);
        }
        MatchedTradeRoute::BatchTradeNoCpi => {
            env.send(
                env.batch_trade_no_cpi_ix(
                    taker_account,
                    lp_account,
                    vec![BatchTradeLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q: SIZE_Q,
                        exec_price: PRICE,
                        fee_bps: 0,
                    }],
                ),
                vec![
                    AccountMeta::new(taker.pubkey(), true),
                    AccountMeta::new(lp.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(taker_account, false),
                    AccountMeta::new(lp_account, false),
                ],
                &[&taker, &lp],
            )
            .expect("BatchTradeNoCpi matched open");
        }
        MatchedTradeRoute::TradeCpi => {
            let (matcher_program, ctx, delegate) = matcher.unwrap();
            env.trade_cpi_with_cu_on_asset(
                &taker,
                taker_account,
                &lp,
                lp_account,
                matcher_program,
                ctx,
                delegate,
                0,
                SIZE_Q,
                0,
            );
        }
        MatchedTradeRoute::BatchTradeCpi => {
            let (matcher_program, ctx, delegate) = matcher.unwrap();
            env.send(
                env.batch_trade_cpi_ix(
                    taker_account,
                    lp_account,
                    vec![BatchTradeCpiLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q: SIZE_Q,
                        fee_bps: 0,
                        limit_price: 0,
                    }],
                ),
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
            )
            .expect("BatchTradeCpi matched open");
        }
    }

    assert_matched_oi_equals_portfolio_scan(&env, 0, &[taker_account, lp_account], SIZE_Q as u128);
}

#[test]
fn v16_program_all_trade_routes_keep_oi_equal_to_active_leg_scan() {
    for route in MatchedTradeRoute::ALL {
        run_matched_trade_route(route);
    }
}

#[derive(Clone, Copy)]
struct Inv048PositionMutationRoute {
    owner: &'static str,
    method: &'static str,
    count: usize,
    disposition: &'static str,
    witnesses: &'static [(&'static str, &'static str)],
}

fn inv048_source_defines_test(source: &str, function: &str) -> bool {
    let marker = format!("fn {function}");
    source.lines().any(|line| {
        line.trim()
            .strip_prefix(&marker)
            .is_some_and(|tail| tail.trim_start().starts_with('('))
    })
}

#[derive(Clone, Copy)]
struct Inv048ObligationOwner {
    category: &'static str,
    census_field: &'static str,
    path: &'static str,
    witness: &'static str,
}

#[test]
fn v16_program_typed_matched_book_obligation_oracle_is_source_complete() {
    const OBLIGATIONS: &[Inv048ObligationOwner] = &[
        Inv048ObligationOwner {
            category: "effective exposure",
            census_field: "effective_q",
            path: "tests/invariants/cu/inv_048_matched_trade_and_open_interest_coherence.rs",
            witness: "v16_program_all_trade_routes_keep_oi_equal_to_active_leg_scan",
        },
        Inv048ObligationOwner {
            category: "ADL-reduced raw residue",
            census_field: "adl_reduced_raw_q",
            path: "tests/invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs",
            witness: "v16_program_adl_reduction_clamp_matrix_matches_public_terminal_routes",
        },
        Inv048ObligationOwner {
            category: "reset-epoch raw residue",
            census_field: "reset_residue_raw_q",
            path: "tests/invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs",
            witness: "v16_program_reset_pending_seeded_frontier_is_exact_and_terminal",
        },
        Inv048ObligationOwner {
            category: "Recovery effective exposure",
            census_field: "recovery_effective_q",
            path: "tests/invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs",
            witness: "v16_program_adl_force_close_clamp_matrix_matches_recovery_terminal_routes",
        },
        Inv048ObligationOwner {
            category: "pending loss obligation count",
            census_field: "pending_obligation_count",
            path: "tests/invariants/stateful/inv_071_crank_progress.rs",
            witness: "v16_program_cured_close_releases_counterparty_obligation",
        },
        Inv048ObligationOwner {
            category: "pending loss weight",
            census_field: "loss_weight_num",
            path: "tests/invariants/stateful/inv_071_crank_progress.rs",
            witness: "v16_program_cured_close_releases_counterparty_obligation",
        },
        Inv048ObligationOwner {
            category: "active close residual",
            census_field: "active_close_residual_num",
            path: "tests/invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs",
            witness: "v16_program_active_close_seeded_frontier_preserves_episode_and_bounded_owner_exit",
        },
        Inv048ObligationOwner {
            category: "active close booked B",
            census_field: "active_close_b_loss_booked_num",
            path: "tests/invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs",
            witness: "v16_program_active_close_seeded_frontier_preserves_episode_and_bounded_owner_exit",
        },
        Inv048ObligationOwner {
            category: "booked B",
            census_field: "b_index_num",
            path: "tests/invariants/cu/inv_038_rounding_and_ratio_conservation.rs",
            witness: "v16_program_social_loss_aggregate_and_chunked_routes_converge_exactly",
        },
        Inv048ObligationOwner {
            category: "social-loss remainder",
            census_field: "social_loss_remainder_num",
            path: "tests/invariants/cu/inv_038_rounding_and_ratio_conservation.rs",
            witness: "v16_program_social_loss_booking_and_settlement_preserve_exact_remainders",
        },
        Inv048ObligationOwner {
            category: "social-loss dust",
            census_field: "social_loss_dust_num",
            path: "tests/invariants/cu/inv_038_rounding_and_ratio_conservation.rs",
            witness: "v16_program_public_odd_atom_partitions_conserve_every_atom",
        },
        Inv048ObligationOwner {
            category: "explicit unallocated loss",
            census_field: "explicit_unallocated_loss_num",
            path: "tests/invariants/stateful/inv_035_no_global_b_pool_residuals_remain_local.rs",
            witness: "v16_program_ambiguous_multi_asset_deficit_order_matrix_avoids_domain_guess",
        },
        Inv048ObligationOwner {
            category: "terminal unmatched effective exposure",
            census_field: "terminal_unmatched_effective_q",
            path: "tests/invariants/stateful/inv_061_deterministic_bounded_liquidation.rs",
            witness: "v16_program_resolved_adl_close_order_matrix_preserves_funded_exits",
        },
    ];

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let model = include_str!("../../support/fuzz_model.rs");
    assert!(model.contains("struct MatchedBookObligationCensus"));
    assert!(model.contains("fn matched_book_obligation_census("));
    assert!(model.contains("matched_book_obligations: [MatchedBookObligationCensus; ASSET_COUNT]"));
    assert!(
        !model.contains("protocol_positions"),
        "an untyped balancing ghost can conceal a missing counterparty"
    );
    let mut categories = std::collections::BTreeSet::new();
    let mut fields = std::collections::BTreeSet::new();
    for obligation in OBLIGATIONS {
        assert!(
            categories.insert(obligation.category),
            "duplicate matched-book obligation category {}",
            obligation.category
        );
        assert!(
            fields.insert(obligation.census_field),
            "duplicate matched-book census field {}",
            obligation.census_field
        );
        assert!(
            model.contains(&format!("{}:", obligation.census_field)),
            "matched-book census lacks typed field {}",
            obligation.census_field
        );
        let source = std::fs::read_to_string(root.join(obligation.path))
            .unwrap_or_else(|error| panic!("read {}: {error}", obligation.path));
        assert!(
            inv048_source_defines_test(&source, obligation.witness),
            "{} lacks executable lifecycle owner {}#{}",
            obligation.category,
            obligation.path,
            obligation.witness,
        );
    }
    assert_eq!(categories.len(), 13, "matched-book obligation roster drift");
    assert!(model.contains("effective + ADL residue + reset residue"));
    assert!(model.contains("effective OI is not the sum of independently decoded effective legs"));
}

#[test]
fn v16_program_position_mutation_composition_is_source_complete() {
    const ENGINE_PIN: &str = "495a5590c97055bd71c6f94d849ff0298f243145";
    const ENGINE_CONTRACTS: &[&str] = &[
        "contract_check_kernel_attach_leg",
        "contract_check_kernel_resize_leg_same_side",
        "contract_check_kernel_retain_leg_as_pending_obligation",
        "contract_check_kernel_clear_leg",
        "contract_check_kernel_classify_position_delta",
        "contract_check_kernel_reduce_position_delta",
        "composition_attach_body_frame_division_stubbed",
        "composition_clear_leg_body_frame",
        "composition_attach_value_conservation_under_axiom",
        "composition_clear_leg_value_conservation",
        "proof_v16_signed_trade_request_maps_to_opposite_account_deltas",
        "proof_v16_view_trade_position_delta_preserves_oi_symmetry",
        "proof_v16_trade_reductions_are_funded_only_by_preexisting_side_oi",
        "proof_v16_wrapper_shape_distinct_asset_batch_projection_preserves_oi_and_outcome",
        "proof_v16_live_market_shape_rejects_long_short_oi_mismatch",
        "proof_v16_full_drain_reset_then_prior_epoch_clear_is_total_and_exact",
    ];
    const ROUTES: &[Inv048PositionMutationRoute] = &[
        Inv048PositionMutationRoute {
            owner: "handle_trade_nocpi_zero_copy",
            method: "execute_trade_with_fee_loss_stale_scoped_not_atomic",
            count: 2,
            disposition: "single CPI and no-CPI trades apply exact opposite deltas through canonical position kernels",
            witnesses: &[(
                "tests/invariants/cu/inv_048_matched_trade_and_open_interest_coherence.rs",
                "v16_program_all_trade_routes_keep_oi_equal_to_active_leg_scan",
            )],
        },
        Inv048PositionMutationRoute {
            owner: "handle_batch_execute_zero_copy",
            method: "execute_batch_with_fee_loss_stale_scoped_not_atomic",
            count: 1,
            disposition: "both batch transports share one engine transition and per-asset OI projection",
            witnesses: &[(
                "tests/invariants/cu/inv_048_matched_trade_and_open_interest_coherence.rs",
                "v16_program_all_trade_routes_keep_oi_equal_to_active_leg_scan",
            )],
        },
        Inv048PositionMutationRoute {
            owner: "handle_permissionless_crank_zero_copy",
            method: "permissionless_auto_crank_not_atomic",
            count: 3,
            disposition: "all live, expired-close, and Recovery crank dispatches use canonical effective quantity",
            witnesses: &[
                (
                    "tests/invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs",
                    "v16_program_scaled_liquidation_matches_independent_selector_model",
                ),
                (
                    "tests/invariants/stateful/inv_061_deterministic_bounded_liquidation.rs",
                    "v16_program_multi_asset_adl_liquidation_is_order_local_and_exit_complete",
                ),
            ],
        },
        Inv048PositionMutationRoute {
            owner: "handle_close_resolved",
            method: "permissionless_auto_crank_not_atomic",
            count: 1,
            disposition: "resolved close dispatches bounded position cleanup before payout",
            witnesses: &[(
                "tests/invariants/stateful/inv_061_deterministic_bounded_liquidation.rs",
                "v16_program_resolved_adl_close_order_matrix_preserves_funded_exits",
            )],
        },
        Inv048PositionMutationRoute {
            owner: "handle_force_close_abandoned_asset",
            method: "forfeit_recovery_leg_not_atomic",
            count: 2,
            disposition: "pair fallback removes each side by its canonical effective quantity",
            witnesses: &[(
                "tests/invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs",
                "v16_program_dual_adl_force_close_clamps_stale_and_raw_work",
            )],
        },
        Inv048PositionMutationRoute {
            owner: "handle_force_close_abandoned_asset",
            method: "force_close_recovery_pair_not_atomic",
            count: 1,
            disposition: "atomic pair force-close preserves both OI lanes",
            witnesses: &[(
                "tests/invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs",
                "v16_program_adl_force_close_clamp_matrix_matches_recovery_terminal_routes",
            )],
        },
        Inv048PositionMutationRoute {
            owner: "handle_forfeit_recovery_leg",
            method: "forfeit_recovery_leg_not_atomic",
            count: 1,
            disposition: "owner Recovery forfeit removes only the bound episode's effective quantity",
            witnesses: &[(
                "tests/invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs",
                "v16_program_dual_adl_recovery_forfeit_matches_effective_oi_model",
            )],
        },
        Inv048PositionMutationRoute {
            owner: "handle_rebalance_reduce",
            method: "rebalance_reduce_position_not_atomic",
            count: 1,
            disposition: "owner unilateral reduction uses the same canonical position delta",
            witnesses: &[(
                "tests/invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs",
                "v16_program_adl_reduction_clamp_matrix_matches_public_terminal_routes",
            )],
        },
    ];

    let cargo = include_str!("../../../Cargo.toml");
    let lock = include_str!("../../../Cargo.lock");
    assert_eq!(
        cargo.matches(&format!("rev = \"{ENGINE_PIN}\"")).count(),
        2,
        "INV-048 engine induction must be reviewed on every pin change",
    );
    assert!(
        lock.contains(&format!("rev={ENGINE_PIN}#{ENGINE_PIN}")),
        "Cargo.lock must resolve the same OI-certified engine revision",
    );

    let mut contracts = std::collections::BTreeSet::new();
    for contract in ENGINE_CONTRACTS {
        assert!(
            contracts.insert(*contract),
            "duplicate engine proof {contract}"
        );
        assert!(
            contract.starts_with("contract_check_")
                || contract.starts_with("composition_")
                || contract.starts_with("proof_v16_"),
            "unclassified OI proof {contract}",
        );
    }
    assert_eq!(contracts.len(), 16, "OI engine-proof roster drift");

    let production = include_str!("../../../src/v16_program.rs");
    let production = production
        .split("    #[cfg(test)]\n    mod tests")
        .next()
        .expect("production prefix exists");
    for (forbidden, comparison) in [
        (".oi_eff_long_q =", ".oi_eff_long_q =="),
        (".oi_eff_short_q =", ".oi_eff_short_q =="),
    ] {
        let direct_writes: Vec<_> = production
            .lines()
            .filter(|line| line.contains(forbidden) && !line.contains(comparison))
            .collect();
        assert!(
            direct_writes.is_empty(),
            "the wrapper must not directly mutate engine OI via {forbidden}: {direct_writes:?}",
        );
    }

    let methods: std::collections::BTreeSet<_> = ROUTES.iter().map(|row| row.method).collect();
    let mut current_function = "<module>";
    let mut actual = std::collections::BTreeMap::<(String, String), usize>::new();
    for line in production.lines() {
        let trimmed = line.trim_start();
        if let Some(fn_offset) = trimmed.find("fn ") {
            let prefix = &trimmed[..fn_offset];
            if prefix.is_empty() || prefix.starts_with("pub") {
                let rest = &trimmed[fn_offset + 3..];
                let end = rest
                    .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                    .unwrap_or(rest.len());
                current_function = &rest[..end];
            }
        }
        for method in &methods {
            let marker = format!(".{method}(");
            let count = line.matches(&marker).count();
            if count != 0 {
                *actual
                    .entry((current_function.to_string(), (*method).to_string()))
                    .or_default() += count;
            }
        }
    }

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut source_cache = std::collections::BTreeMap::<&str, String>::new();
    let mut expected = std::collections::BTreeMap::new();
    for route in ROUTES {
        assert!(!route.disposition.is_empty());
        assert!(!route.witnesses.is_empty());
        for (path, witness) in route.witnesses {
            let source = source_cache.entry(path).or_insert_with(|| {
                std::fs::read_to_string(root.join(path))
                    .unwrap_or_else(|error| panic!("read {path}: {error}"))
            });
            assert!(
                inv048_source_defines_test(source, witness),
                "{}.{} lacks executable OI witness {path}#{witness}",
                route.owner,
                route.method,
            );
        }
        assert!(
            expected
                .insert(
                    (route.owner.to_string(), route.method.to_string()),
                    route.count,
                )
                .is_none(),
            "duplicate OI transition class {}.{}",
            route.owner,
            route.method,
        );
    }
    assert_eq!(
        actual, expected,
        "every wrapper position mutation needs an inductive OI disposition and public census",
    );

    let transition_census =
        include_str!("inv_088_global_summaries_are_not_account_local_proofs.rs");
    assert!(transition_census.contains(
        "fn v16_program_every_wrapper_engine_transition_callsite_has_summary_disposition_and_witness"
    ));
}
