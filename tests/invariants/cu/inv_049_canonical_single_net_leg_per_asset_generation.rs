//! INV-049 - Canonical single net leg per asset generation.
//!
//! Normative obligation: each portfolio has at most one active canonical net
//! leg for the current asset generation. Same-asset trades must resize or cross
//! that leg; they must not create hidden duplicate current-generation legs.
//!
//! This file is the primary CU/SBF owner for that guarantee. It repeats
//! same-asset fills through all four public trade routes and checks the exact
//! active-leg census after increase, reduction, and cross-zero transitions. A
//! source-complete composition guard additionally proves the wrapper has no direct
//! leg writer or position-transfer ingress and binds every current structural
//! engine callsite to public trade, ADL, reset, Recovery, and resolved-close
//! witnesses. Malformed program-owned bytes are intentionally excluded here: they
//! are not publicly constructible and the exact engine validator already proves
//! duplicate-leg rejection.

use super::*;

#[derive(Clone, Copy, Debug)]
enum NetLegRoute {
    TradeNoCpi,
    BatchTradeNoCpi,
    TradeCpi,
    BatchTradeCpi,
}

impl NetLegRoute {
    const ALL: [Self; 4] = [
        Self::TradeNoCpi,
        Self::BatchTradeNoCpi,
        Self::TradeCpi,
        Self::BatchTradeCpi,
    ];
}

fn active_leg_census_for_asset(portfolio: &PortfolioAccountV16, asset_index: usize) -> Vec<i128> {
    portfolio
        .legs
        .iter()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .filter(|leg| leg.active && leg.asset_index as usize == asset_index)
        .map(|leg| leg.basis_pos_q)
        .collect()
}

fn assert_single_net_leg(
    env: &V16CuEnv,
    portfolio: Pubkey,
    asset_index: usize,
    expected_basis_q: i128,
) {
    let state = env.portfolio_state(portfolio);
    let legs = active_leg_census_for_asset(&state, asset_index);
    assert_eq!(
        legs,
        vec![expected_basis_q],
        "portfolio must have exactly one active net leg for asset {asset_index}",
    );
    assert_eq!(
        active_leg_for_asset(&state, asset_index).basis_pos_q,
        expected_basis_q,
        "canonical helper must observe the same net leg",
    );
}

fn apply_netting_trade(
    env: &mut V16CuEnv,
    route: NetLegRoute,
    taker: &Keypair,
    taker_account: Pubkey,
    lp: &Keypair,
    lp_account: Pubkey,
    matcher: Option<(Pubkey, Pubkey, Pubkey)>,
    size_q: i128,
) {
    const PRICE: u64 = 100;
    match route {
        NetLegRoute::TradeNoCpi => {
            env.trade_asset_with_cu(0, taker, taker_account, lp, lp_account, size_q, PRICE, 0);
        }
        NetLegRoute::BatchTradeNoCpi => {
            env.send(
                env.batch_trade_no_cpi_ix(
                    taker_account,
                    lp_account,
                    vec![BatchTradeLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q,
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
                &[taker, lp],
            )
            .expect("BatchTradeNoCpi same-asset netting trade");
        }
        NetLegRoute::TradeCpi => {
            let (matcher_program, ctx, delegate) = matcher.expect("CPI matcher");
            env.trade_cpi_with_cu_on_asset(
                taker,
                taker_account,
                lp,
                lp_account,
                matcher_program,
                ctx,
                delegate,
                0,
                size_q,
                0,
            );
        }
        NetLegRoute::BatchTradeCpi => {
            let (matcher_program, ctx, delegate) = matcher.expect("CPI matcher");
            env.send(
                env.batch_trade_cpi_ix(
                    taker_account,
                    lp_account,
                    vec![BatchTradeCpiLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q,
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
                &[taker],
            )
            .expect("BatchTradeCpi same-asset netting trade");
        }
    }
}

fn run_single_net_leg_route(route: NetLegRoute) {
    const PRICE: u64 = 100;
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_for_asset_as_admin(0, 1, PRICE);
    let taker = Keypair::new();
    let lp = Keypair::new();
    let taker_account = env.create_portfolio(&taker);
    let lp_account = env.create_portfolio(&lp);
    env.deposit(&taker, taker_account, 1_000_000);
    env.deposit(&lp, lp_account, 1_000_000);

    let matcher = if matches!(route, NetLegRoute::TradeCpi | NetLegRoute::BatchTradeCpi) {
        let matcher_program = Pubkey::new_unique();
        let matcher_bytes =
            std::fs::read(auth_matcher_program_path()).expect("read auth matcher SBF");
        env.svm.add_program(matcher_program, &matcher_bytes);
        let (ctx, delegate, _) = env.init_auth_matcher_context(matcher_program, &lp, lp_account);
        Some((matcher_program, ctx, delegate))
    } else {
        None
    };

    apply_netting_trade(
        &mut env,
        route,
        &taker,
        taker_account,
        &lp,
        lp_account,
        matcher,
        2 * POS_SCALE as i128,
    );
    assert_single_net_leg(&env, taker_account, 0, 2 * POS_SCALE as i128);
    assert_single_net_leg(&env, lp_account, 0, -(2 * POS_SCALE as i128));

    apply_netting_trade(
        &mut env,
        route,
        &taker,
        taker_account,
        &lp,
        lp_account,
        matcher,
        POS_SCALE as i128,
    );
    assert_single_net_leg(&env, taker_account, 0, 3 * POS_SCALE as i128);
    assert_single_net_leg(&env, lp_account, 0, -(3 * POS_SCALE as i128));

    apply_netting_trade(
        &mut env,
        route,
        &taker,
        taker_account,
        &lp,
        lp_account,
        matcher,
        -(2 * POS_SCALE as i128),
    );
    assert_single_net_leg(&env, taker_account, 0, POS_SCALE as i128);
    assert_single_net_leg(&env, lp_account, 0, -(POS_SCALE as i128));

    apply_netting_trade(
        &mut env,
        route,
        &taker,
        taker_account,
        &lp,
        lp_account,
        matcher,
        -(3 * POS_SCALE as i128),
    );
    assert_single_net_leg(&env, taker_account, 0, -(2 * POS_SCALE as i128));
    assert_single_net_leg(&env, lp_account, 0, 2 * POS_SCALE as i128);

    let group = env.market_state().1;
    assert_eq!(group.assets[0].oi_eff_long_q, 2 * POS_SCALE);
    assert_eq!(group.assets[0].oi_eff_short_q, 2 * POS_SCALE);
}

#[test]
fn v16_program_same_asset_trades_resize_one_canonical_net_leg() {
    for route in NetLegRoute::ALL {
        run_single_net_leg_route(route);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Inv049LegWriterCallsite<'a> {
    owner: &'a str,
    method: &'a str,
    count: usize,
}

#[test]
fn v16_program_leg_writer_surface_is_engine_owned_and_source_complete() {
    use std::collections::BTreeMap;

    const PRODUCTION_SOURCE: &str = include_str!("../../../src/v16_program.rs");
    const ENGINE_TRANSITION_ROSTER: &str =
        include_str!("inv_088_global_summaries_are_not_account_local_proofs.rs");
    const STATEFUL_VALIDITY: &str =
        include_str!("../stateful/inv_081_success_state_validity_over_complete_public_routes.rs");
    const ADL_EVIDENCE: &str = include_str!("inv_051_canonical_adl_effective_quantity.rs");
    const RESET_EVIDENCE: &str =
        include_str!("inv_065_reset_recovery_and_retired_state_isolation.rs");

    let production = PRODUCTION_SOURCE
        .split("    #[cfg(test)]\n    mod tests")
        .next()
        .expect("production prefix exists");

    let direct_leg_reads = production
        .lines()
        .filter(|line| line.contains(".header.legs["))
        .collect::<Vec<_>>();
    assert_eq!(
        direct_leg_reads.len(),
        5,
        "a new direct wrapper leg access needs structural-writer review"
    );
    assert!(
        direct_leg_reads
            .iter()
            .all(|line| line.trim_start().starts_with("let leg =")),
        "the wrapper may inspect canonical legs but must not write them directly"
    );
    assert_eq!(
        production.matches("state::write_portfolio(").count(),
        0,
        "production must mutate the zero-copy portfolio only through typed engine transitions"
    );
    for forbidden_ingress in [
        "TransferPosition",
        "TransferLeg",
        "ImportPosition",
        "DeserializePortfolio",
    ] {
        assert!(
            !production.contains(forbidden_ingress),
            "new leg ingress {forbidden_ingress} requires a public canonicalization matrix"
        );
    }

    const STRUCTURAL_ROWS: &[Inv049LegWriterCallsite<'_>] = &[
        Inv049LegWriterCallsite {
            owner: "handle_trade_nocpi_zero_copy",
            method: "execute_trade_with_fee_loss_stale_scoped_not_atomic",
            count: 2,
        },
        Inv049LegWriterCallsite {
            owner: "handle_batch_execute_zero_copy",
            method: "execute_batch_with_fee_loss_stale_scoped_not_atomic",
            count: 1,
        },
        Inv049LegWriterCallsite {
            owner: "handle_force_close_abandoned_asset",
            method: "forfeit_recovery_leg_not_atomic",
            count: 2,
        },
        Inv049LegWriterCallsite {
            owner: "handle_force_close_abandoned_asset",
            method: "force_close_recovery_pair_not_atomic",
            count: 1,
        },
        Inv049LegWriterCallsite {
            owner: "handle_forfeit_recovery_leg",
            method: "forfeit_recovery_leg_not_atomic",
            count: 1,
        },
        Inv049LegWriterCallsite {
            owner: "handle_rebalance_reduce",
            method: "rebalance_reduce_position_not_atomic",
            count: 1,
        },
        Inv049LegWriterCallsite {
            owner: "handle_close_resolved",
            method: "permissionless_auto_crank_not_atomic",
            count: 1,
        },
        Inv049LegWriterCallsite {
            owner: "handle_permissionless_crank_zero_copy",
            method: "permissionless_auto_crank_not_atomic",
            count: 2,
        },
    ];

    let mut current_function = "<module>";
    let mut actual = BTreeMap::<(&str, &str), usize>::new();
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
        for row in STRUCTURAL_ROWS {
            if current_function == row.owner && line.contains(&format!(".{}(", row.method)) {
                *actual.entry((row.owner, row.method)).or_default() += 1;
            }
        }
    }
    let expected = STRUCTURAL_ROWS
        .iter()
        .map(|row| ((row.owner, row.method), row.count))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(actual, expected, "canonical leg-writer callsites drifted");

    assert_eq!(
        ENGINE_TRANSITION_ROSTER
            .matches("Inv088EngineCallsite { owner:")
            .count(),
        50,
        "a new wrapper-to-engine transition reopens the structural-leg classification"
    );
    assert!(ENGINE_TRANSITION_ROSTER.contains(
        "fn v16_program_every_wrapper_engine_transition_callsite_has_summary_disposition_and_witness("
    ));
    for (source, witness) in [
        (
            STATEFUL_VALIDITY,
            "v16_program_extended_public_action_alphabet_runs_through_shared_oracles",
        ),
        (
            STATEFUL_VALIDITY,
            "v16_program_recovery_exit_restart_and_fresh_generation_trade_compose",
        ),
        (
            ADL_EVIDENCE,
            "v16_program_liquidation_adl_effective_exit_matrix_preserves_bounded_cleanup",
        ),
        (
            RESET_EVIDENCE,
            "v16_program_reset_pending_rejects_fresh_counterparty_and_completes_recovery",
        ),
    ] {
        assert!(
            source.contains(&format!("fn {witness}(")),
            "canonical-leg composition witness {witness} is missing"
        );
    }

    crate::assert_certified_engine_pin("INV-049 engine validator and leg-kernel contracts");
}
