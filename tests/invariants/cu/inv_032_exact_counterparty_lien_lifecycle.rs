//! INV-032 - Exact counterparty-lien lifecycle.
//!
//! Normative obligation: Lien creation, consumption, release, impairment, and recovery occur exactly once.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_attack_force_close_source_backed_accounts_does_not_grow_source_liens`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: reachable successful create/grow/impair/consume/release
//! classes are independently censused after public transitions across all trade
//! routes, source sides, Resolved, Recovery, expiry, and conversion retry. Every
//! engine error propagates into SVM rollback under INV-080. The insurance-backed
//! lifecycle is not exposed by this wrapper and is pin-bound to INV-033's engine
//! contracts. A new transition, public insurance reservation, engine pin, or
//! missing lifecycle witness reopens this current-surface closure.

use super::*;

#[test]
fn v16_attack_force_close_source_backed_accounts_does_not_grow_source_liens() {
    const INITIAL_PRICE: u64 = 100;
    const ASSET0_SIZE_Q: i128 = 200 * POS_SCALE as i128;
    const ASSET1_SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const WINNING_DOMAIN: usize = 1;
    const DELAY: u64 = 5;
    const SHUTDOWN_SLOT: u64 = 10;
    const FORCE_SLOT: u64 = SHUTDOWN_SLOT + DELAY + 1;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(4, 1_000, 1_000, 500);
    env.configure_permissionless_resolve_with_cu(1_000, DELAY);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, INITIAL_PRICE);
    env.configure_auth_mark_for_asset_as_admin(1, 1, INITIAL_PRICE);
    env.update_backing_fee_policy_with_cu(WINNING_DOMAIN as u16, 5_000, 2_500);
    env.svm.expire_blockhash();
    env.configure_auth_mark_for_asset_as_admin(0, 1, INITIAL_PRICE);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 10_000);
    env.deposit(&short_owner, short, 10_000);
    env.top_up_backing_bucket(WINNING_DOMAIN as u16, 2_500, 10_000);

    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        ASSET0_SIZE_Q,
        INITIAL_PRICE,
        0,
    );
    env.trade_asset_with_cu(
        1,
        &long_owner,
        long,
        &short_owner,
        short,
        ASSET1_SIZE_Q,
        INITIAL_PRICE,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_for_asset_as_admin(0, 2, 105);
    env.crank(
        long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
    );
    assert!(
        state::portfolio_source_domain(&env.portfolio_state(long), WINNING_DOMAIN)
            .source_claim_bound_num
            .get()
            != 0,
        "setup must create source-backed positive PnL through public trades and crank"
    );

    env.svm.warp_to_slot(SHUTDOWN_SLOT);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
        1,
        SHUTDOWN_SLOT,
        0,
    );

    let (_, before_group) = env.market_state();
    let provider_before =
        before_group.source_backing_buckets[WINNING_DOMAIN].utilization_fee_earnings;
    let insurance_before = before_group.insurance;
    let long_lien_before: u128 = env
        .portfolio_state(long)
        .source_domains
        .iter()
        .map(|slot| slot.source_lien_counterparty_backing_num.get())
        .sum();
    let short_lien_before: u128 = env
        .portfolio_state(short)
        .source_domains
        .iter()
        .map(|slot| slot.source_lien_counterparty_backing_num.get())
        .sum();

    env.svm.warp_to_slot(FORCE_SLOT);
    env.svm.expire_blockhash();
    let cranker = Keypair::new();
    let cu = env
        .try_force_close_abandoned_asset_with_cu(
            &cranker,
            long,
            short,
            1,
            FORCE_SLOT,
            ASSET1_SIZE_Q.unsigned_abs(),
        )
        .expect("source-backed recovery force-close remains live");
    assert_cu_within(
        "source-backed ForceCloseAbandonedAsset",
        cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );

    let (_, after_group) = env.market_state();
    assert_eq!(
        after_group.source_backing_buckets[WINNING_DOMAIN].utilization_fee_earnings,
        provider_before,
        "force-close must not create an uncharged provider-fee delta"
    );
    assert_eq!(
        after_group.insurance, insurance_before,
        "force-close must not create an uncharged insurance-fee delta"
    );
    let long_lien_after: u128 = env
        .portfolio_state(long)
        .source_domains
        .iter()
        .map(|slot| slot.source_lien_counterparty_backing_num.get())
        .sum();
    let short_lien_after: u128 = env
        .portfolio_state(short)
        .source_domains
        .iter()
        .map(|slot| slot.source_lien_counterparty_backing_num.get())
        .sum();
    assert_eq!(
        long_lien_after, long_lien_before,
        "permissionless force-close must not grow the source-backed long lien"
    );
    assert_eq!(
        short_lien_after, short_lien_before,
        "permissionless force-close must not grow the source-backed short lien"
    );
    assert!(
        !has_active_leg_for_asset(&env.portfolio_state(long), 1),
        "force-close closes the abandoned long leg"
    );
    assert!(
        !has_active_leg_for_asset(&env.portfolio_state(short), 1),
        "force-close closes the abandoned short leg"
    );
    assert_domain_budget_remaining_total_consistent(
        &after_group,
        "source-backed force-close no fee bypass",
    );
}

#[test]
fn v16_program_counterparty_lien_lifecycle_composition_is_source_complete() {
    crate::assert_certified_engine_pin("INV-032 lien-lifecycle composition");

    let lifecycle_source =
        include_str!("../stateful/inv_026_reservation_and_encumbrance_conservation.rs");
    assert!(lifecycle_source.contains(
        "fn v16_program_counterparty_encumbrance_lifecycle_is_exact_across_routes_sides_and_terminal_modes"
    ));
    let impairment_source =
        include_str!("../stateful/inv_030_credit_rate_determinism_and_fail_closed_behavior.rs");
    assert!(impairment_source
        .contains("fn v16_program_liened_backing_expiry_route_matrix_preserves_owner_reduction"));
    let expiry_source = include_str!("inv_028_source_domain_realizability_cap.rs");
    assert!(expiry_source
        .contains("fn v16_program_expired_source_lien_route_matrix_preserves_bounded_owner_exit"));
    assert!(expiry_source
        .contains("fn v16_program_shared_expiry_progress_matrix_preserves_terminal_progress"));
    let retry_source =
        include_str!("../stateful/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs");
    assert!(retry_source
        .contains("fn v16_program_haircut_conversion_retries_cannot_reuse_claim_or_backing"));

    let insurance_source = include_str!("inv_033_insurance_backed_lien_single_classification.rs");
    assert!(insurance_source.contains(
        "fn v16_program_public_source_lien_classification_never_double_counts_insurance"
    ));
    let rollback_source = include_str!("inv_080_error_propagation_and_exact_rollback.rs");
    assert!(rollback_source
        .contains("fn v16_program_explicit_engine_error_dispositions_are_source_complete"));
    assert!(rollback_source
        .contains("fn v16_program_dispatch_and_entrypoints_preserve_every_handler_error"));
    let transition_source =
        include_str!("inv_088_global_summaries_are_not_account_local_proofs.rs");
    assert!(transition_source.contains(
        "fn v16_program_every_wrapper_engine_transition_callsite_has_summary_disposition_and_witness"
    ));
}
