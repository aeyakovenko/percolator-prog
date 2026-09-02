//! INV-035 - No global B pool; residuals remain local.
//!
//! Normative obligation: a bankruptcy residual is booked only to the exact
//! `(asset, opposing_side)` domain that generated the exposure. No wrapper
//! route may select a different B domain or make an unrelated source claim
//! absorb that loss.
//!
//! Evidence in this file (source composition over P/F/I/M evidence): the
//! wrapper is bound to the exact engine revision that owns the B arithmetic,
//! every wrapper ingress capable of reaching residual booking is enumerated,
//! and the wrapper has no direct B writer. Each route class requires named,
//! engine-owned proof contracts and executable public LiteSVM witnesses. The
//! witnesses include exact two-domain B/source-claim deltas, all trade
//! transports, pending-close and resolved continuations, both Recovery exits,
//! and a 48-world three-asset order product.
//!
//! Guarantee boundary: engine arithmetic stays engine-owned. Changing the
//! engine pin, adding a B-capable wrapper call, or adding a direct wrapper B
//! writer invalidates this composition and requires an explicit new row.

#[derive(Clone, Copy)]
struct Inv035ResidualRoute {
    class: &'static str,
    engine_proofs: &'static [&'static str],
    public_witnesses: &'static [(&'static str, &'static str)],
}

fn inv035_source_defines_function(source: &str, function: &str) -> bool {
    let marker = format!("fn {function}");
    source.lines().any(|line| {
        line.trim()
            .strip_prefix(&marker)
            .is_some_and(|tail| tail.trim_start().starts_with('('))
    })
}

#[test]
fn v16_program_domain_local_b_composition_is_source_complete() {
    const ENGINE_PIN: &str = "495a5590c97055bd71c6f94d849ff0298f243145";
    const ROUTES: &[Inv035ResidualRoute] = &[
        Inv035ResidualRoute {
            class: "single and batch trade terminal residual attribution",
            engine_proofs: &[
                "contract_check_bresidual_chunk_conservation",
                "contract_check_kernel_bresidual_step",
                "proof_v16_live_residual_booking_to_loss_bearing_side_is_bounded_and_exact",
            ],
            public_witnesses: &[
                (
                    "tests/invariants/stateful/inv_035_no_global_b_pool_residuals_remain_local.rs",
                    "v16_program_ambiguous_multi_asset_deficit_order_matrix_avoids_domain_guess",
                ),
                (
                    "tests/invariants/public_sbf/inv_035_no_global_b_pool_residuals_remain_local.rs",
                    "v16_program_pr281_b_settlement_stays_domain_local_and_owner_can_exit",
                ),
            ],
        },
        Inv035ResidualRoute {
            class: "live liquidation and pending-close continuation",
            engine_proofs: &[
                "closure_kernel_advance_close_ledger_rank_witness",
                "closure_close_ledger_absorbs_booking_outcome",
                "proof_v16_auto_crank_pending_close_priority_is_total",
                "proof_v16_close_progress_ledger_residual_equation_is_enforced",
                "proof_v16_liquidation_preflight_accepts_only_fully_durable_residual",
            ],
            public_witnesses: &[
                (
                    "tests/invariants/cu/inv_071_crank_progress.rs",
                    "v16_program_public_pending_close_preempts_b_stale_then_exposes_b_progress",
                ),
                (
                    "tests/invariants/cu/inv_037_exact_residual_partition.rs",
                    "v16_program_insurance_covered_liquidation_close_ledger_partitions_exactly",
                ),
            ],
        },
        Inv035ResidualRoute {
            class: "resolved attributable and ambiguous bankruptcy",
            engine_proofs: &[
                "proof_v16_resolved_two_active_legs_are_unattributed_for_bankruptcy",
                "proof_v16_resolved_unattributed_bad_debt_clears_without_recovery",
                "proof_v16_resolved_residual_booking_without_loss_bearing_side_is_explicit_only",
            ],
            public_witnesses: &[
                (
                    "tests/invariants/stateful/inv_071_crank_progress.rs",
                    "v16_program_pending_close_residual_is_part_of_the_public_crank_rank",
                ),
                (
                    "tests/invariants/cu/inv_067_terminal_payout_completeness_and_exact_once_settlement.rs",
                    "v16_program_terminal_bankruptcy_residual_matrix_preserves_provider_value",
                ),
            ],
        },
        Inv035ResidualRoute {
            class: "Recovery owner forfeit and permissionless pair close",
            engine_proofs: &[
                "contract_check_kernel_forfeit_residual_step",
                "proof_v16_liquidation_preflight_routes_insufficient_residual_capacity_to_recovery",
            ],
            public_witnesses: &[
                (
                    "tests/invariants/stateful/inv_081_success_state_validity_over_complete_public_routes.rs",
                    "v16_program_owner_recovery_forfeit_strictly_reduces_each_position_episode",
                ),
                (
                    "tests/invariants/stateful/inv_081_success_state_validity_over_complete_public_routes.rs",
                    "v16_program_abandoned_asset_force_close_strictly_reduces_public_exposure",
                ),
            ],
        },
        Inv035ResidualRoute {
            class: "source-domain burn and larger multi-asset topology",
            engine_proofs: &[
                "proof_v16_source_claim_burn_partition_is_domain_first_conservative_and_isolated",
            ],
            public_witnesses: &[
                (
                    "tests/invariants/stateful/inv_035_no_global_b_pool_residuals_remain_local.rs",
                    "v16_program_two_asset_bankruptcy_preserves_domain_local_settlement_and_exit",
                ),
                (
                    "tests/invariants/stateful/inv_035_no_global_b_pool_residuals_remain_local.rs",
                    "v16_program_three_asset_locked_loss_liquidation_is_domain_neutral_and_order_independent",
                ),
            ],
        },
    ];

    let cargo = include_str!("../../../Cargo.toml");
    let lock = include_str!("../../../Cargo.lock");
    assert_eq!(
        cargo.matches(&format!("rev = \"{ENGINE_PIN}\"")).count(),
        2,
        "INV-035 composition must be reviewed on every engine-pin change",
    );
    assert!(
        lock.contains(&format!("rev={ENGINE_PIN}#{ENGINE_PIN}")),
        "Cargo.lock must resolve the domain-local-B-certified engine revision",
    );

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut classes = std::collections::BTreeSet::new();
    let mut proofs = std::collections::BTreeSet::new();
    let mut witnesses = std::collections::BTreeSet::new();
    let mut source_cache = std::collections::BTreeMap::<&str, String>::new();
    for route in ROUTES {
        assert!(
            classes.insert(route.class),
            "duplicate residual route class"
        );
        assert!(!route.engine_proofs.is_empty());
        assert!(!route.public_witnesses.is_empty());
        for proof in route.engine_proofs {
            assert!(proofs.insert(*proof), "duplicate engine proof {proof}");
            assert!(
                proof.starts_with("proof_v16_")
                    || proof.starts_with("contract_check_")
                    || proof.starts_with("closure_"),
                "unclassified residual proof {proof}",
            );
        }
        for (path, witness) in route.public_witnesses {
            assert!(witnesses.insert(*witness), "duplicate witness {witness}");
            let source = source_cache.entry(path).or_insert_with(|| {
                std::fs::read_to_string(root.join(path))
                    .unwrap_or_else(|error| panic!("read {path}: {error}"))
            });
            assert!(
                inv035_source_defines_function(source, witness),
                "residual class '{}' lacks executable witness {path}#{witness}",
                route.class,
            );
        }
    }
    assert_eq!(classes.len(), 5, "residual route class roster drift");
    assert_eq!(proofs.len(), 14, "residual engine-proof roster drift");
    assert_eq!(witnesses.len(), 10, "residual public-witness roster drift");

    let production = include_str!("../../../src/v16_program.rs");
    let b_capable_ingresses = [
        ("execute_trade_with_fee_loss_stale_scoped_not_atomic", 2),
        ("execute_batch_with_fee_loss_stale_scoped_not_atomic", 1),
        ("permissionless_auto_crank_not_atomic", 4),
        ("forfeit_recovery_leg_not_atomic", 3),
        ("force_close_recovery_pair_not_atomic", 1),
    ];
    for (method, expected_count) in b_capable_ingresses {
        assert_eq!(
            production.matches(method).count(),
            expected_count,
            "B-capable wrapper ingress {method} changed without an INV-035 route review",
        );
    }

    for line in production
        .lines()
        .filter(|line| line.contains("b_long_num") || line.contains("b_short_num"))
    {
        assert!(
            line.contains("== 0") || line.contains(".get() != 0"),
            "the wrapper must not write engine-owned B state: {line}",
        );
    }

    let transition_roster =
        include_str!("inv_088_global_summaries_are_not_account_local_proofs.rs");
    for required_row in [
        "owner: \"handle_trade_nocpi_zero_copy\", method: \"execute_trade_with_fee_loss_stale_scoped_not_atomic\", count: 2",
        "owner: \"handle_batch_execute_zero_copy\", method: \"execute_batch_with_fee_loss_stale_scoped_not_atomic\", count: 1",
        "owner: \"handle_force_close_abandoned_asset\", method: \"forfeit_recovery_leg_not_atomic\", count: 2",
        "owner: \"handle_force_close_abandoned_asset\", method: \"force_close_recovery_pair_not_atomic\", count: 1",
        "owner: \"handle_forfeit_recovery_leg\", method: \"forfeit_recovery_leg_not_atomic\", count: 1",
        "owner: \"handle_close_resolved\", method: \"permissionless_auto_crank_not_atomic\", count: 1",
        "owner: \"handle_permissionless_crank_zero_copy\", method: \"permissionless_auto_crank_not_atomic\", count: 3",
    ] {
        assert!(
            transition_roster.contains(required_row),
            "B-capable ingress lacks an INV-088 source-derived transition row: {required_row}",
        );
    }
}
