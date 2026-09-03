//! INV-044 - No phantom value from indices, certificates, or labels.
//!
//! Normative obligation: derived labels and crank classifications cannot create
//! token stock, withdrawable value, health, or senior capital by themselves.
//!
//! Evidence in this file (I/C): a third party permissionlessly asks the public
//! crank route to make B-settlement progress on a flat solvent account. The
//! engine must return non-progress rather than a successful no-op, and the account's capital,
//! vault, `c_tot`, insurance, and owner withdrawability must remain exact.
//! A second public lifecycle creates source-attributed PnL on one asset and an
//! offsetting loss on another, then permutes both permissionless account-crank
//! order and the portfolio slots in which those legs reside. Every order must
//! converge to identical claims, certified equity, source stock, and terminal
//! SPL withdrawal. This catches derived-claim burn that depends on either keeper
//! or account-local leg order.
//! Additional no-phantom-value coverage lives in INV-025, INV-026, INV-069, and
//! INV-070.
//! `v16_program_derived_value_class_roster_is_source_complete` partitions every current derived
//! value surface into ten classes and binds each class to exact-pin engine proofs plus public
//! value/encumbrance witnesses. The caller-field, wrapper-persisted-field, and wrapper-to-engine
//! transition inventories are themselves executable dependencies, so a new input, mirror, or
//! transition cannot silently inherit this closure.

use super::*;

#[test]
fn v16_program_permissionless_settle_b_on_healthy_account_is_safe_noop() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);

    let (_, group_before) = env.market_state();
    let capital_before = env.portfolio_state(portfolio).capital.get();
    assert_eq!(capital_before, 1_000);

    env.svm.warp_to_slot(5);
    let _ = env.send_crank_if_actionable(
        ProgInstruction::PermissionlessCrank {
            now_slot: 5,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[],
    );

    let (_, group_after) = env.market_state();
    assert_eq!(env.portfolio_state(portfolio).capital.get(), capital_before);
    assert_eq!(group_after.vault, group_before.vault);
    assert_eq!(group_after.c_tot, group_before.c_tot);
    assert_eq!(group_after.insurance, group_before.insurance);

    let (dest, _) = env.withdraw_with_cu(&owner, portfolio, 1_000);
    assert_eq!(
        env.token_amount(dest),
        1_000,
        "derived crank labels cannot trap an otherwise withdrawable account"
    );
}

#[derive(Debug, PartialEq, Eq)]
struct CrossDomainSettlementOutcome {
    winner_capital: u128,
    winner_pnl: i128,
    winner_source_claim_num: u128,
    winner_source_stock: Vec<(u32, u128, u128, u128, u128)>,
    winner_certified_equity: i128,
    counterparty_capital: u128,
    counterparty_pnl: i128,
    counterparty_source_stock: Vec<(u32, u128, u128, u128, u128)>,
    source_claim_num: u128,
    source_funded_or_consumed_num: u128,
    pre_close_source_stock: Vec<(usize, u128, u128, u128, u128, u128, u128)>,
    winner_withdrawal: u64,
    counterparty_withdrawal: u64,
    resolved_winner_payout: u128,
    resolved_counterparty_payout: u128,
    terminal_vault: u128,
    terminal_c_tot: u128,
    terminal_vault_tokens: u64,
    terminal_source_stock: Vec<(usize, u128, u128, u128, u128, u128, u128)>,
}

fn inv044_account_source_stock(
    account: &percolator::PortfolioAccountV16Account,
) -> Vec<(u32, u128, u128, u128, u128)> {
    account
        .source_domains
        .iter()
        .filter(|source| source.is_occupied())
        .map(|source| {
            (
                source.domain.get(),
                source.source_claim_bound_num.get(),
                source.source_claim_liened_num.get(),
                source.source_lien_effective_reserved.get(),
                source.source_lien_counterparty_backing_num.get(),
            )
        })
        .collect()
}

fn inv044_source_stock(
    group: &state::MarketGroupV16,
) -> Vec<(usize, u128, u128, u128, u128, u128, u128)> {
    group
        .source_credit
        .iter()
        .zip(group.source_backing_buckets.iter())
        .enumerate()
        .filter_map(|(domain, (source, bucket))| {
            let row = (
                domain,
                source.positive_claim_bound_num,
                source.fresh_reserved_backing_num,
                source.spent_backing_num,
                source.provider_receivable_num,
                bucket.fresh_unliened_backing_num,
                bucket.consumed_liened_backing_num,
            );
            (row.1 != 0 || row.2 != 0 || row.3 != 0 || row.4 != 0 || row.5 != 0 || row.6 != 0)
                .then_some(row)
        })
        .collect()
}

fn inv044_cross_domain_settlement_outcome(
    settle_counterparty_first: bool,
    open_positive_asset_first: bool,
) -> CrossDomainSettlementOutcome {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 5_000, 10_000, 1_000);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);

    let owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let observer_owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let counterparty = env.create_portfolio(&counterparty_owner);
    let observer = env.create_portfolio(&observer_owner);
    env.deposit(&owner, portfolio, 3_150);
    env.deposit(&counterparty_owner, counterparty, 5_000);
    const POSITIVE_SOURCE_DOMAIN: usize = 1;
    let open_order = if open_positive_asset_first {
        [0u16, 1u16]
    } else {
        [1u16, 0u16]
    };
    for asset_index in open_order {
        let size_q = if asset_index == 0 { 20 } else { 10 };
        env.trade_asset_with_cu(
            asset_index,
            &owner,
            portfolio,
            &counterparty_owner,
            counterparty,
            size_q * POS_SCALE as i128,
            100,
            0,
        );
    }

    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, 110);
    env.push_auth_mark_for_asset_as_admin(1, 2, 95);
    for _ in 0..4 {
        if env
            .crank_if_actionable(
                observer,
                ProgInstruction::PermissionlessCrank {
                    now_slot: 2,
                    observations: crank_observations_for_assets(&[0, 1]),
                },
            )
            .is_none()
        {
            break;
        }
    }
    let settlement_order = if settle_counterparty_first {
        [counterparty, portfolio, counterparty, portfolio, portfolio]
    } else {
        [portfolio, counterparty, portfolio, counterparty, portfolio]
    };
    for actor in settlement_order {
        for _ in 0..4 {
            if env
                .crank_if_actionable(
                    actor,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 2,
                        observations: crank_observations_for_assets(&[0, 1]),
                    },
                )
                .is_none()
            {
                break;
            }
        }
    }

    let winner = env.portfolio_state(portfolio);
    let loser = env.portfolio_state(counterparty);
    let (_, group) = env.market_state();
    let winner_source_claim_num = winner
        .source_domains
        .iter()
        .filter(|source| {
            source.is_occupied() && source.domain.get() == POSITIVE_SOURCE_DOMAIN as u32
        })
        .map(|source| source.source_claim_bound_num.get())
        .sum::<u128>();
    let before_close = CrossDomainSettlementOutcome {
        winner_capital: winner.capital.get(),
        winner_pnl: winner.pnl.get(),
        winner_source_claim_num,
        winner_source_stock: inv044_account_source_stock(&winner),
        winner_certified_equity: health_cert(&winner).certified_equity,
        counterparty_capital: loser.capital.get(),
        counterparty_pnl: loser.pnl.get(),
        counterparty_source_stock: inv044_account_source_stock(&loser),
        source_claim_num: group.source_credit[POSITIVE_SOURCE_DOMAIN].positive_claim_bound_num,
        source_funded_or_consumed_num: group.source_credit[POSITIVE_SOURCE_DOMAIN]
            .fresh_reserved_backing_num
            .checked_add(group.source_credit[POSITIVE_SOURCE_DOMAIN].spent_backing_num)
            .unwrap(),
        pre_close_source_stock: inv044_source_stock(&group),
        winner_withdrawal: 0,
        counterparty_withdrawal: 0,
        resolved_winner_payout: 0,
        resolved_counterparty_payout: 0,
        terminal_vault: 0,
        terminal_c_tot: 0,
        terminal_vault_tokens: 0,
        terminal_source_stock: Vec::new(),
    };

    env.trade_asset_with_cu(
        0,
        &owner,
        portfolio,
        &counterparty_owner,
        counterparty,
        -(20 * POS_SCALE as i128),
        110,
        0,
    );
    env.trade_asset_with_cu(
        1,
        &owner,
        portfolio,
        &counterparty_owner,
        counterparty,
        -(10 * POS_SCALE as i128),
        95,
        0,
    );
    for _ in 0..8 {
        let mut progressed = false;
        for actor in [portfolio, counterparty] {
            progressed |= env
                .crank_if_actionable(
                    actor,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 2,
                        observations: Vec::new(),
                    },
                )
                .is_some();
        }
        if !progressed {
            break;
        }
    }
    let released = env.portfolio_state(portfolio).pnl.get().max(0) as u128;
    if released != 0 {
        env.convert_released_pnl_with_cu(&owner, portfolio, released);
    }
    let winner_capital = env.portfolio_state(portfolio).capital.get();
    let (winner_destination, _) = env.withdraw_with_cu(&owner, portfolio, winner_capital);
    let counterparty_capital = env.portfolio_state(counterparty).capital.get();
    let (counterparty_destination, _) =
        env.withdraw_with_cu(&counterparty_owner, counterparty, counterparty_capital);
    env.resolve();
    let resolved_payouts = drain_resolved_cohort(
        &mut env,
        &[(&owner, portfolio), (&counterparty_owner, counterparty)],
        "INV-044 cross-domain terminal attribution",
    );
    let (_, terminal_group) = env.market_state();
    CrossDomainSettlementOutcome {
        winner_withdrawal: env.token_amount(winner_destination),
        counterparty_withdrawal: env.token_amount(counterparty_destination),
        resolved_winner_payout: resolved_payouts[0],
        resolved_counterparty_payout: resolved_payouts[1],
        terminal_vault: terminal_group.vault,
        terminal_c_tot: terminal_group.c_tot,
        terminal_vault_tokens: env.token_amount(env.vault),
        terminal_source_stock: inv044_source_stock(&terminal_group),
        ..before_close
    }
}

#[test]
fn v16_program_cross_domain_settlement_is_crank_and_leg_slot_order_independent() {
    let positive_slot_winner_first = inv044_cross_domain_settlement_outcome(false, true);
    let positive_slot_counterparty_first = inv044_cross_domain_settlement_outcome(true, true);
    let negative_slot_winner_first = inv044_cross_domain_settlement_outcome(false, false);
    let negative_slot_counterparty_first = inv044_cross_domain_settlement_outcome(true, false);

    assert_eq!(
        positive_slot_winner_first, positive_slot_counterparty_first,
        "permissionless crank order cannot burn a user's source-attributed claim",
    );
    assert_eq!(
        positive_slot_winner_first, negative_slot_winner_first,
        "portfolio leg-slot order cannot change source-attributed value",
    );
    assert_eq!(
        positive_slot_winner_first, negative_slot_counterparty_first,
        "account and leg-slot orders cannot compose into value drift",
    );
    assert_eq!(positive_slot_winner_first.winner_capital, 3_100);
    assert_eq!(positive_slot_winner_first.winner_pnl, 200);
    assert_eq!(
        positive_slot_winner_first.winner_source_claim_num,
        200 * BOUND_SCALE,
    );
    assert_eq!(positive_slot_winner_first.winner_certified_equity, 3_300);
    assert_eq!(positive_slot_winner_first.counterparty_capital, 4_800);
    assert_eq!(positive_slot_winner_first.counterparty_pnl, 50);
    assert_eq!(
        positive_slot_winner_first.source_claim_num,
        200 * BOUND_SCALE,
    );
    assert_eq!(
        positive_slot_winner_first.source_funded_or_consumed_num,
        200 * BOUND_SCALE,
    );
    assert_eq!(positive_slot_winner_first.winner_withdrawal, 3_300);
    assert_eq!(positive_slot_winner_first.counterparty_withdrawal, 4_800);
    assert_eq!(positive_slot_winner_first.resolved_winner_payout, 0);
    assert_eq!(positive_slot_winner_first.resolved_counterparty_payout, 50);
    assert_eq!(positive_slot_winner_first.terminal_vault, 0);
    assert_eq!(positive_slot_winner_first.terminal_c_tot, 0);
    assert_eq!(positive_slot_winner_first.terminal_vault_tokens, 0);
}

// security.md sweep — deposit with parked pnl (#32/#33): depositing while holding junior (parked) pnl
// must credit capital exactly and leave the pnl and its residual backing untouched. No double-count,
// no disturbance of the junior pnl, conservation holds.
#[test]
fn v16_attack_deposit_with_parked_pnl_clean() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let lo_owner = Keypair::new();
    let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new();
    let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, 1_000_000);
    env.deposit(&sh_owner, sh, 1_000_000);
    env.trade_asset_with_cu(
        0,
        &lo_owner,
        lo,
        &sh_owner,
        sh,
        (10_000 * POS_SCALE) as i128,
        100,
        0,
    );
    // price up -> long accrues parked pnl; settle.
    env.svm.warp_to_slot(10);
    env.push_auth_mark_with_cu(10, 110);
    env.crank_steps_after_market_catchup(
        sh,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
        1,
    );
    env.crank(
        lo,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
    );
    env.svm.warp_to_slot(11);
    for p in [sh, lo] {
        env.crank_if_actionable(
            p,
            ProgInstruction::PermissionlessCrank {
                now_slot: 11,
                observations: crank_observations(0),
            },
        );
    }
    let a0 = env.portfolio_state(lo);
    assert!(a0.pnl.get() > 0, "long has parked pnl (non-vacuous)");
    let (_, g0) = env.market_state();
    let resid0 = g0.vault as i128 - g0.c_tot as i128 - g0.insurance as i128;

    // deposit MORE while holding the parked pnl.
    env.svm.expire_blockhash();
    env.deposit(&lo_owner, lo, 500_000);
    let a1 = env.portfolio_state(lo);
    let (_, g1) = env.market_state();
    assert_eq!(
        a1.capital.get(),
        a0.capital.get() + 500_000,
        "capital credited exactly by the deposit"
    );
    assert_eq!(
        a1.pnl.get(),
        a0.pnl.get(),
        "parked pnl UNCHANGED by the deposit (no double-count/disturbance)"
    );
    assert_eq!(
        g1.vault,
        g0.vault + 500_000,
        "vault grew by exactly the deposit"
    );
    assert_eq!(
        g1.c_tot,
        g0.c_tot + 500_000,
        "c_tot grew by exactly the deposit"
    );
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting vault == real vault balance"
    );
    // the parked pnl is still backed by (at least) the same residual.
    let resid1 = g1.vault as i128 - g1.c_tot as i128 - g1.insurance as i128;
    assert_eq!(
        resid1, resid0,
        "residual backing of the junior pnl unchanged by the deposit"
    );
    assert!(
        resid1 >= a1.pnl.get().max(0),
        "junior pnl still backed by residual"
    );
    assert!(g1.vault >= g1.c_tot + g1.insurance, "senior conservation");
}

#[derive(Clone, Copy)]
struct Inv044DerivedValueClass {
    class: &'static str,
    engine_proofs: &'static [&'static str],
    public_witnesses: &'static [(&'static str, &'static str)],
}

fn inv044_source_defines_test(source: &str, function: &str) -> bool {
    let marker = format!("fn {function}");
    source.lines().any(|line| {
        line.trim()
            .strip_prefix(&marker)
            .is_some_and(|tail| tail.trim_start().starts_with('('))
    })
}

#[test]
fn v16_program_derived_value_class_roster_is_source_complete() {
    const ENGINE_PIN: &str = "495a5590c97055bd71c6f94d849ff0298f243145";
    const CLASSES: &[Inv044DerivedValueClass] = &[
        Inv044DerivedValueClass {
            class: "ADL A indices and effective quantity",
            engine_proofs: &[
                "proof_v16_adl_scaled_accrual_kernel_matches_every_valid_factor",
                "proof_v16_adl_scaled_accrual_is_cross_side_zero_sum_at_every_factor_quantum",
                "proof_v16_adl_kf_settlement_is_account_partition_invariant_and_zero_sum",
            ],
            public_witnesses: &[
                (
                    "tests/invariants/stateful/inv_053_full_health_recertification_equivalence.rs",
                    "v16_program_incremental_trade_certificate_matches_full_refresh_with_nonunit_adl",
                ),
                (
                    "tests/invariants/cu/inv_051_canonical_adl_effective_quantity.rs",
                    "v16_program_liquidation_adl_effective_exit_matrix_preserves_bounded_cleanup",
                ),
            ],
        },
        Inv044DerivedValueClass {
            class: "K and F accrual indices and cohort snapshots",
            engine_proofs: &[
                "proof_v16_kf_accrual_marks_each_stored_cohort_exactly_once",
                "proof_v16_kf_settlement_disposes_one_cohort_member_without_underflow",
                "proof_v16_canonical_accrual_path_is_partition_invariant",
            ],
            public_witnesses: &[
                (
                    "tests/invariants/cu/inv_044_no_phantom_value_from_indices_certificates_or_labels.rs",
                    "v16_program_cross_domain_settlement_is_crank_and_leg_slot_order_independent",
                ),
                (
                    "tests/invariants/cu/inv_024_attributed_quote_value_conservation.rs",
                    "v16_attack_funding_and_fee_combined_conserve",
                ),
            ],
        },
        Inv044DerivedValueClass {
            class: "B index, stale flags, and pending settlement labels",
            engine_proofs: &[
                "proof_v16_b_settlement_atom_budget_clears_public_scale_gap",
                "proof_v16_auto_crank_b_settlement_pending_is_exact_and_fail_closed",
            ],
            public_witnesses: &[
                (
                    "tests/invariants/cu/inv_044_no_phantom_value_from_indices_certificates_or_labels.rs",
                    "v16_program_permissionless_settle_b_on_healthy_account_is_safe_noop",
                ),
                (
                    "tests/invariants/cu/inv_071_crank_progress.rs",
                    "v16_program_public_b_prerequisite_preserves_later_liquidation_progress",
                ),
            ],
        },
        Inv044DerivedValueClass {
            class: "health certificates, active bitmap, and epoch keys",
            engine_proofs: &[
                "proof_v16_raw_oracle_target_change_invalidates_all_prior_certificates",
                "proof_v16_final_batch_margin_gate_accepts_only_final_certified_im",
                "proof_v16_health_cert_capital_debit_preserves_im_or_rejects",
            ],
            public_witnesses: &[
                (
                    "tests/invariants/stateful/inv_053_full_health_recertification_equivalence.rs",
                    "v16_program_incremental_trade_certificate_equals_public_full_refresh",
                ),
                (
                    "tests/invariants/cu/inv_054_certificate_epoch_completeness.rs",
                    "v16_attack_convert_released_pnl_requires_current_cert_and_public_refresh",
                ),
            ],
        },
        Inv044DerivedValueClass {
            class: "claim bounds, source reservations, and encumbrance labels",
            engine_proofs: &[
                "proof_v16_available_source_support_excludes_liened_and_encumbered_amounts",
                "proof_v16_capital_backed_loss_reservation_is_value_neutral_and_capital_capped",
            ],
            public_witnesses: &[
                (
                    "tests/invariants/cu/inv_026_reservation_and_encumbrance_conservation_is_separate_from_token_value.rs",
                    "v16_program_source_credit_reservation_labels_do_not_free_backing_value",
                ),
                (
                    "tests/invariants/cu/inv_028_source_domain_realizability_cap.rs",
                    "v16_program_source_realizability_cap_composition_is_source_complete",
                ),
            ],
        },
        Inv044DerivedValueClass {
            class: "counterparty and insurance lien lifecycle labels",
            engine_proofs: &[
                "proof_v16_public_counterparty_lien_create_moves_fresh_to_valid_without_value_movement",
                "proof_v16_public_counterparty_lien_release_restores_unliened_backing_without_value_movement",
                "proof_v16_public_counterparty_lien_consume_creates_receivable_without_value_movement",
                "proof_v16_public_counterparty_lien_impair_moves_valid_to_impaired_without_value_movement",
            ],
            public_witnesses: &[
                (
                    "tests/invariants/cu/inv_032_exact_counterparty_lien_lifecycle.rs",
                    "v16_program_counterparty_lien_lifecycle_composition_is_source_complete",
                ),
                (
                    "tests/invariants/cu/inv_033_insurance_backed_lien_single_classification.rs",
                    "v16_program_public_source_lien_classification_never_double_counts_insurance",
                ),
                (
                    "tests/invariants/cu/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs",
                    "v16_program_single_use_lifecycle_composition_is_source_complete",
                ),
            ],
        },
        Inv044DerivedValueClass {
            class: "soft credit, realizability rates, and durable-value admission",
            engine_proofs: &[
                "proof_v16_source_credit_rate_never_exceeds_available_backing_ratio",
                "proof_v16_underbacked_source_credit_cannot_satisfy_im_lien_requirements",
                "proof_v16_counterparty_source_credit_support_does_not_debit_vault_or_insurance",
            ],
            public_witnesses: &[
                (
                    "tests/invariants/cu/inv_028_source_domain_realizability_cap.rs",
                    "v16_attack_convert_released_pnl_cannot_mint_from_unbacked_pnl",
                ),
                (
                    "tests/invariants/stateful/inv_028_source_domain_realizability_cap.rs",
                    "v16_program_reciprocal_cross_asset_cycle_cannot_mint_credit",
                ),
                (
                    "tests/invariants/cu/inv_027_protected_principal_seniority.rs",
                    "v16_program_loss_stale_economic_routes_have_a_complete_seniority_disposition",
                ),
            ],
        },
        Inv044DerivedValueClass {
            class: "backing, lifecycle, retirement, and wrapper policy tags",
            engine_proofs: &[
                "proof_v16_counterparty_backing_expiry_reclassifies_principal_and_impairs_lien",
                "proof_v16_lapsed_source_backing_preempts_flat_lien_normalization",
                "proof_v16_canonical_retired_asset_slot_preserves_identity_and_clears_local_ledgers",
            ],
            public_witnesses: &[
                (
                    "tests/invariants/cu/inv_063_backing_expiry_normalization.rs",
                    "v16_program_backing_expiry_consumer_composition_is_source_complete",
                ),
                (
                    "tests/invariants/cu/inv_069_terminal_normalization_and_retirement.rs",
                    "v16_program_terminal_blocker_census_composes_engine_retirement_before_wrapper_cleanup",
                ),
                (
                    "tests/invariants/cu/inv_087_no_phantom_controls_or_dead_security_fields.rs",
                    "v16_program_all_wrapper_owned_persisted_structs_have_complete_field_rosters",
                ),
            ],
        },
        Inv044DerivedValueClass {
            class: "global stocks, external token value, and terminal residue",
            engine_proofs: &[
                "proof_v16_inductive_settle_negative_pnl_preserves_senior_solvency",
                "proof_v16_public_terminal_insurance_retirement_is_exact_and_fully_framed",
            ],
            public_witnesses: &[
                (
                    "tests/invariants/stateful/inv_025_exact_stock_reconciliation.rs",
                    "v16_program_public_value_lifecycle_reconciles_every_materialized_stock_census",
                ),
                (
                    "tests/invariants/cu/inv_025_exact_stock_reconciliation.rs",
                    "v16_program_value_routes_reconcile_vault_capital_insurance_and_backing_stocks",
                ),
                (
                    "tests/invariants/cu/inv_070_zero_unattributed_terminal_residue_and_close_slab.rs",
                    "v16_program_terminal_stock_and_close_slab_composition_is_source_complete",
                ),
            ],
        },
        Inv044DerivedValueClass {
            class: "caller inputs, wrapper mirrors, summaries, and transition ownership",
            engine_proofs: &[],
            public_witnesses: &[
                (
                    "tests/invariants/cu/inv_023_caller_input_confinement_for_derived_safety_state.rs",
                    "v16_program_caller_input_roster_owns_every_production_field",
                ),
                (
                    "tests/invariants/cu/inv_087_no_phantom_controls_or_dead_security_fields.rs",
                    "v16_program_every_wrapper_persisted_security_field_has_a_named_mutation_witness",
                ),
                (
                    "tests/invariants/cu/inv_088_global_summaries_are_not_account_local_proofs.rs",
                    "v16_program_every_wrapper_engine_transition_callsite_has_summary_disposition_and_witness",
                ),
            ],
        },
    ];

    let cargo = include_str!("../../../Cargo.toml");
    let lock = include_str!("../../../Cargo.lock");
    assert_eq!(
        cargo.matches(&format!("rev = \"{ENGINE_PIN}\"")).count(),
        2,
        "INV-044 composes exact engine proofs and must reopen on a pin change",
    );
    assert!(
        lock.contains(&format!("rev={ENGINE_PIN}#{ENGINE_PIN}")),
        "Cargo.lock must resolve the same certified engine revision",
    );

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut classes = std::collections::BTreeSet::new();
    let mut proofs = std::collections::BTreeSet::new();
    let mut source_cache = std::collections::BTreeMap::<&str, String>::new();
    for row in CLASSES {
        assert!(classes.insert(row.class), "duplicate derived-value class");
        assert!(!row.public_witnesses.is_empty());
        for proof in row.engine_proofs {
            assert!(proofs.insert(*proof), "duplicate engine proof {proof}");
            assert!(proof.starts_with("proof_v16_"));
        }
        for (path, witness) in row.public_witnesses {
            let source = source_cache.entry(path).or_insert_with(|| {
                std::fs::read_to_string(root.join(path))
                    .unwrap_or_else(|error| panic!("read {path}: {error}"))
            });
            assert!(
                inv044_source_defines_test(source, witness),
                "derived-value class '{}' lacks executable witness {path}#{witness}",
                row.class,
            );
        }
    }
    assert_eq!(classes.len(), 10, "derived-value class roster drift");
    assert_eq!(proofs.len(), 25, "derived-value proof roster drift");
}
