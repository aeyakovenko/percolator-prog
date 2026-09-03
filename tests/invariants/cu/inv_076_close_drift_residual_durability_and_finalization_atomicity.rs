//! INV-076 - Close drift, residual durability, and finalization atomicity.
//!
//! Normative obligation: close-progress state and optional cure deposits are
//! atomic. A rejected continuation must not cancel the ledger, free exposure,
//! credit capital, move custody, or block the later terminal path.
//!
//! Evidence in this file (I/C): stale-market and public-created close-ledger
//! continuations reject atomically. A public bankrupt close reached through
//! trade, mark accrual, shutdown, and ForfeitRecoveryLeg rejects a zero-deposit
//! cure with exact rollback, then still reaches terminal progress through the
//! permissionless crank. A two-asset ordering trace advances the global market
//! slot through unrelated authenticated accrual while the close asset remains
//! frozen, then proves the local residual still books without global Recovery,
//! custody movement, foreign-account mutation, or loss of unrelated user exits.
//! The public bankruptcy-escalation matrix in INV-071 additionally reaches the
//! open-risk liquidation-to-Recovery boundary and, after normalizing the same
//! call's authenticated accrual clock, requires the complete decoded market to
//! change only in terminal mode/reason while the target portfolio stays
//! byte-identical. That frames OI, basis, counters, barriers, insurance, and
//! custody across the engine's commit-on-Recovery disposition without duplicating
//! the engine transition in this wrapper-owned file. The source-complete
//! composition gate at the end of this file closes the remaining fallible-phase
//! question by combining the exact-pin engine success contracts with INV-080's
//! complete wrapper error propagation and the SVM rollback boundary.

use super::*;

#[test]
fn v16_program_cure_and_cancel_close_rejects_when_resolve_matured_atomically() {
    let mut env = V16CuEnv::new();
    env.configure_permissionless_resolve_with_cu(5, 5);
    env.configure_auth_mark_with_cu(0, 100);

    env.svm.warp_to_slot(3);
    env.push_auth_mark_with_cu(3, 100);

    let fresh_owner = Keypair::new();
    let fresh = env.create_portfolio(&fresh_owner);
    env.deposit(&fresh_owner, fresh, 100);
    env.seed_cancellable_close_progress(fresh);
    let fresh_source = env.token_account_for_mint(env.mint, fresh_owner.pubkey(), 20);
    env.svm.warp_to_slot(4);
    env.cure_and_cancel_close_with_cu(&fresh_owner, fresh, fresh_source, 20);
    let fresh_after = env.portfolio_state(fresh);
    assert!(close_progress(&fresh_after).canceled);
    assert_eq!(fresh_after.capital.get(), 120);
    assert_eq!(env.token_amount(fresh_source), 0);

    let stale_owner = Keypair::new();
    let stale = env.create_portfolio(&stale_owner);
    env.deposit(&stale_owner, stale, 100);
    env.seed_cancellable_close_progress(stale);
    let stale_source = env.token_account_for_mint(env.mint, stale_owner.pubkey(), 20);

    env.svm.warp_to_slot(40);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&stale).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let source_before = env.svm.get_account(&stale_source).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::CureAndCancelClose {
            portfolio_id: env.portfolio_id(stale),
            position_epoch: env.portfolio_position_epoch(stale),
            optional_deposit: 20,
        },
        vec![
            AccountMeta::new(stale_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(stale, false),
            AccountMeta::new(stale_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&stale_owner],
    );
    assert!(
        rejected.is_err(),
        "stale cure must reject before committing finalization state"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&stale).unwrap(), portfolio_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    assert_eq!(env.svm.get_account(&stale_source).unwrap(), source_before);

    env.svm.expire_blockhash();
    let resolve = env.send(
        ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(
        resolve.is_ok(),
        "permissionless resolve remains live after rejected stale cure: {resolve:?}"
    );
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
}

#[test]
fn v16_program_public_close_zero_cure_rejects_atomically_and_terminal_progress_remains() {
    let PublicActiveCloseFixture {
        mut env,
        loss_owner,
        loss,
        ..
    } = public_asset1_bankrupt_close_fixture();
    let ledger = close_progress(&env.portfolio_state(loss));
    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&loss).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::CureAndCancelClose {
            portfolio_id: env.portfolio_id(loss),
            position_epoch: env.portfolio_position_epoch(loss),
            optional_deposit: 0,
        },
        vec![
            AccountMeta::new(loss_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(loss, false),
        ],
        &[&loss_owner],
    );
    assert!(
        rejected.is_err(),
        "a public close with residual remaining cannot be cured for free"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&loss).unwrap(), portfolio_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    assert_eq!(
        close_progress(&env.portfolio_state(loss)).residual_remaining,
        ledger.residual_remaining,
        "rejected zero-cure must not consume or forgive residual"
    );

    env.svm.warp_to_slot(ledger.max_close_slot + 1);
    env.svm.expire_blockhash();
    let cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 0,
                observations: vec![],
            },
            vec![
                AccountMeta::new_readonly(env.payer.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(loss, false),
            ],
            &[],
        )
        .expect("rejected zero-cure must not block terminal close progress");
    assert_cu_within("INV-076 public close terminal progress", cu, CRANK_CU_LIMIT);
    assert!(
        matches!(
            env.market_state().1.mode,
            MarketModeV16::Recovery | MarketModeV16::Resolved
        ),
        "expired public close must enter a terminal progress mode"
    );
}

#[test]
fn v16_program_unrelated_asset_slot_drift_preserves_local_close_progress_and_live_scope() {
    let PublicActiveCloseFixture {
        mut env,
        loss,
        asset1_counterparty,
        live_counterparty_owner,
        live_counterparty,
        live_peer_owner,
        live_peer,
        ..
    } = public_asset1_bankrupt_close_fixture();
    let portfolio_before = env.portfolio_state(loss);
    let ledger_before = close_progress(&portfolio_before);
    assert!(
        has_active_leg_for_asset(&portfolio_before, ledger_before.asset_index as usize),
        "the close-snapshot drift guard applies only while its loss leg remains active"
    );
    let drift_slot = ledger_before
        .drift_reference_slot
        .checked_add(1)
        .expect("fixture close reference has one-slot headroom");
    assert!(
        drift_slot <= ledger_before.max_close_slot,
        "this probe must hit snapshot drift before the close-expiry branch"
    );

    // Commit an authenticated observation for the unrelated healthy asset. The
    // wrapper advances the engine's market slot only through real market work;
    // wall-clock movement by itself is intentionally insufficient.
    env.svm.warp_to_slot(drift_slot);
    env.push_auth_mark_with_cu(drift_slot, 100);
    env.crank(
        live_counterparty,
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: crank_observations(0),
        },
    );
    let (_, group_before) = env.market_state();
    assert_eq!(group_before.mode, MarketModeV16::Live);
    assert!(
        group_before.current_slot > ledger_before.drift_reference_slot,
        "the unrelated accrual must advance the global market slot past the close anchor"
    );
    assert!(
        group_before.current_slot <= ledger_before.max_close_slot,
        "the setup must remain before close expiry"
    );
    assert_eq!(
        close_progress(&env.portfolio_state(loss)).residual_remaining,
        ledger_before.residual_remaining,
        "unrelated accrual must not itself advance the close ledger"
    );
    assert_eq!(
        group_before.assets[ledger_before.asset_index as usize].slot_last,
        ledger_before.drift_reference_slot,
        "the originating asset snapshot remains current despite unrelated accrual"
    );

    let loss_before = env.svm.get_account(&loss).unwrap();
    let counterparty_before = env.svm.get_account(&asset1_counterparty).unwrap();
    let live_counterparty_before = env.svm.get_account(&live_counterparty).unwrap();
    let live_peer_before = env.svm.get_account(&live_peer).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    env.svm.expire_blockhash();
    let progress_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 0,
                observations: vec![],
            },
            vec![
                AccountMeta::new_readonly(env.payer.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(loss, false),
            ],
            &[],
        )
        .expect("unrelated accrual must not block the local close continuation");
    assert_cu_within(
        "INV-076 asset-local close continuation",
        progress_cu,
        CRANK_CU_LIMIT,
    );

    let (_, progressed) = env.market_state();
    let ledger_after = close_progress(&env.portfolio_state(loss));
    assert_eq!(
        progressed.mode,
        MarketModeV16::Live,
        "an unrelated asset must not turn a local close into global Recovery"
    );
    assert!(
        ledger_after.residual_remaining < ledger_before.residual_remaining,
        "the honest close crank must strictly lower the residual rank"
    );
    assert_eq!(progressed.vault, group_before.vault);
    assert_eq!(progressed.c_tot, group_before.c_tot);
    assert_eq!(progressed.insurance, group_before.insurance);
    assert_ne!(env.svm.get_account(&loss).unwrap(), loss_before);
    assert_eq!(
        env.svm.get_account(&asset1_counterparty).unwrap(),
        counterparty_before
    );
    assert_eq!(
        env.svm.get_account(&live_counterparty).unwrap(),
        live_counterparty_before
    );
    assert_eq!(env.svm.get_account(&live_peer).unwrap(), live_peer_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    let exit_cu = env
        .try_trade_asset_with_cu(
            0,
            &live_counterparty_owner,
            live_counterparty,
            &live_peer_owner,
            live_peer,
            -(POS_SCALE as i128),
            100,
            0,
        )
        .expect("local close progress must preserve unrelated users' live exit");
    assert_cu_within(
        "INV-076 unrelated live exit after local close progress",
        exit_cu,
        TRADE_CU_LIMIT,
    );
    let (_, exited) = env.market_state();
    assert_eq!(exited.mode, MarketModeV16::Live);
    assert_eq!(exited.assets[0].oi_eff_long_q, 0);
    assert_eq!(exited.assets[0].oi_eff_short_q, 0);
}

#[derive(Clone, Copy)]
struct Inv076CloseClass {
    class: &'static str,
    engine_proofs: &'static [&'static str],
    public_witnesses: &'static [(&'static str, &'static str)],
}

fn inv076_source_defines_function(source: &str, function: &str) -> bool {
    let marker = format!("fn {function}");
    source.lines().any(|line| {
        line.trim()
            .strip_prefix(&marker)
            .is_some_and(|tail| tail.trim_start().starts_with('('))
    })
}

#[test]
fn v16_program_close_finalization_composition_is_source_complete() {
    const ENGINE_PIN: &str = "495a5590c97055bd71c6f94d849ff0298f243145";
    const CLASSES: &[Inv076CloseClass] = &[
        Inv076CloseClass {
            class: "close creation identity exclusion and exit barrier",
            engine_proofs: &[
                "proof_v16_close_begin_takes_barrier_and_stamps_immutable_identity",
                "proof_v16_close_begin_rejects_occupied_domain_before_mutation",
                "proof_v16_close_begin_rejects_account_with_active_close",
                "proof_v16_withdraw_rejects_while_close_active",
            ],
            public_witnesses: &[
                (
                    "tests/invariants/stateful/inv_074_scope_locality.rs",
                    "v16_program_two_asset_closes_advance_without_crossing_scope",
                ),
                (
                    "tests/invariants/stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs",
                    "v16_program_active_close_seeded_frontier_preserves_episode_and_bounded_owner_exit",
                ),
            ],
        },
        Inv076CloseClass {
            class: "immutable drift anchor asset freshness and expiry",
            engine_proofs: &[
                "contract_check_kernel_open_close_snapshot_is_stale",
                "proof_v16_expired_close_progress_declares_recovery_without_value_mutation",
            ],
            public_witnesses: &[
                (
                    "tests/invariants/stateful/inv_076_close_drift_residual_durability_and_finalization_atomicity.rs",
                    "v16_program_same_asset_price_and_funding_drift_preserves_close_and_owner_exit",
                ),
                (
                    "tests/invariants/cu/inv_076_close_drift_residual_durability_and_finalization_atomicity.rs",
                    "v16_program_unrelated_asset_slot_drift_preserves_local_close_progress_and_live_scope",
                ),
            ],
        },
        Inv076CloseClass {
            class: "principal support insurance and senior-stock attribution",
            engine_proofs: &[
                "proof_v16_negative_pnl_settlement_consumes_principal_before_residual",
                "contract_check_flow_insurance_to_close_insurance_spent",
                "contract_check_flow_insurance_to_close_rejects_vault_movement",
                "proof_v16_residual_excludes_senior_backing_provider_earnings",
                "proof_v16_residual_excludes_recoverable_counterparty_backing_principal",
            ],
            public_witnesses: &[
                (
                    "tests/invariants/cu/inv_037_exact_residual_partition.rs",
                    "v16_program_insurance_covered_liquidation_close_ledger_partitions_exactly",
                ),
                (
                    "tests/invariants/cu/inv_027_protected_principal_seniority.rs",
                    "v16_attack_leveraged_bad_debt_socialized_not_printed",
                ),
            ],
        },
        Inv076CloseClass {
            class: "B booking residual partition and strict ledger advance",
            engine_proofs: &[
                "closure_kernel_advance_close_ledger_rank_witness",
                "contract_check_bresidual_chunk_conservation",
                "contract_check_kernel_bresidual_step",
                "closure_close_ledger_absorbs_booking_outcome",
                "proof_v16_close_progress_ledger_residual_equation_is_enforced",
                "proof_v16_live_residual_booking_to_loss_bearing_side_is_bounded_and_exact",
                "proof_v16_resolved_residual_booking_without_loss_bearing_side_is_explicit_only",
            ],
            public_witnesses: &[
                (
                    "tests/invariants/cu/inv_071_crank_progress.rs",
                    "v16_program_public_pending_close_preempts_b_stale_then_exposes_b_progress",
                ),
                (
                    "tests/invariants/stateful/inv_071_crank_progress.rs",
                    "v16_program_pending_close_residual_is_part_of_the_public_crank_rank",
                ),
            ],
        },
        Inv076CloseClass {
            class: "effective exposure retention and obligation release",
            engine_proofs: &[
                "contract_check_kernel_retain_leg_as_pending_obligation",
                "contract_check_kernel_recovery_pending_obligation_release_allowed",
                "proof_v16_unilateral_close_capacity_is_safe_effective_progress",
                "proof_v16_close_cancel_shape_rejects_dropped_residual",
            ],
            public_witnesses: &[
                (
                    "tests/invariants/cu/inv_071_crank_progress.rs",
                    "v16_program_bankruptcy_escalation_matrix_commits_recovery_and_resolves",
                ),
                (
                    "tests/invariants/cu/inv_061_deterministic_bounded_liquidation.rs",
                    "v16_program_liquidation_composition_is_source_complete",
                ),
            ],
        },
        Inv076CloseClass {
            class: "cure cancellation and finalized-ledger inertness",
            engine_proofs: &[
                "contract_check_flow_close_cure_to_account_capital",
                "proof_v16_cure_and_cancel_close_rejects_without_active_close",
                "proof_v16_withdraw_allowed_after_canceled_close",
                "proof_v16_finalized_zero_residual_close_is_inert_for_dematerialization",
                "proof_v16_finalized_zero_residual_close_is_inert_for_flat_withdraw",
                "proof_v16_finalized_zero_residual_close_is_inert_for_next_begin",
            ],
            public_witnesses: &[
                (
                    "tests/invariants/cu/inv_076_close_drift_residual_durability_and_finalization_atomicity.rs",
                    "v16_program_cure_and_cancel_close_rejects_when_resolve_matured_atomically",
                ),
                (
                    "tests/invariants/cu/inv_076_close_drift_residual_durability_and_finalization_atomicity.rs",
                    "v16_program_public_close_zero_cure_rejects_atomically_and_terminal_progress_remains",
                ),
            ],
        },
        Inv076CloseClass {
            class: "durability preflight and explicit terminal Recovery",
            engine_proofs: &[
                "proof_v16_liquidation_preflight_accepts_only_fully_durable_residual",
                "proof_v16_liquidation_preflight_routes_insufficient_residual_capacity_to_recovery",
                "proof_v16_liquidation_error_commits_only_fully_declared_recovery",
                "contract_check_kernel_forfeit_residual_step",
            ],
            public_witnesses: &[
                (
                    "tests/invariants/stateful/inv_071_crank_progress.rs",
                    "v16_program_unattributed_multi_asset_loss_reaches_liquidation_and_terminal_payout",
                ),
                (
                    "tests/invariants/cu/inv_070_zero_unattributed_terminal_residue_and_close_slab.rs",
                    "v16_program_recovery_force_close_reaches_zero_residue_and_close_slab",
                ),
            ],
        },
        Inv076CloseClass {
            class: "bounded selector dispatch error propagation and SVM rollback",
            engine_proofs: &[
                "proof_v16_auto_crank_pending_close_priority_is_total",
                "proof_v16_seq_double_crank_is_monotone_and_value_flat",
            ],
            public_witnesses: &[
                (
                    "tests/invariants/cu/inv_071_crank_progress.rs",
                    "v16_program_crank_progress_and_recovery_composition_is_source_complete",
                ),
                (
                    "tests/invariants/cu/inv_077_bounded_work_and_maximum_shape_compute.rs",
                    "v16_program_max_shape_resolved_close_order_matrix_is_bounded_and_fair",
                ),
                (
                    "tests/invariants/cu/inv_080_error_propagation_and_exact_rollback.rs",
                    "v16_program_explicit_engine_error_dispositions_are_source_complete",
                ),
                (
                    "tests/invariants/cu/inv_080_error_propagation_and_exact_rollback.rs",
                    "v16_program_dispatch_and_entrypoints_preserve_every_handler_error",
                ),
            ],
        },
    ];

    let cargo = include_str!("../../../Cargo.toml");
    let lock = include_str!("../../../Cargo.lock");
    assert_eq!(
        cargo.matches(&format!("rev = \"{ENGINE_PIN}\"")).count(),
        2,
        "INV-076 composition must be reviewed on every engine pin change",
    );
    assert!(
        lock.contains(&format!("rev={ENGINE_PIN}#{ENGINE_PIN}")),
        "Cargo.lock must resolve the close-certified engine revision",
    );

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut classes = std::collections::BTreeSet::new();
    let mut proofs = std::collections::BTreeSet::new();
    let mut witnesses = std::collections::BTreeSet::new();
    let mut source_cache = std::collections::BTreeMap::<&str, String>::new();
    for row in CLASSES {
        assert!(classes.insert(row.class), "duplicate close class");
        assert!(!row.engine_proofs.is_empty());
        assert!(!row.public_witnesses.is_empty());
        for proof in row.engine_proofs {
            assert!(proofs.insert(*proof), "duplicate engine proof {proof}");
            assert!(
                proof.starts_with("proof_v16_")
                    || proof.starts_with("contract_check_")
                    || proof.starts_with("closure_"),
                "unclassified close proof {proof}",
            );
        }
        for (path, witness) in row.public_witnesses {
            assert!(witnesses.insert(*witness), "duplicate witness {witness}");
            let source = source_cache.entry(path).or_insert_with(|| {
                std::fs::read_to_string(root.join(path))
                    .unwrap_or_else(|error| panic!("read {path}: {error}"))
            });
            assert!(
                inv076_source_defines_function(source, witness),
                "close class '{}' lacks executable witness {path}#{witness}",
                row.class,
            );
        }
    }
    assert_eq!(classes.len(), 8, "close composition class roster drift");
    assert_eq!(proofs.len(), 34, "close engine-proof roster drift");
    assert_eq!(witnesses.len(), 18, "close public-witness roster drift");

    // The engine is intentionally `_not_atomic`: wrapper propagation plus SVM
    // transaction rollback is the atomicity boundary. Lock both close ingresses
    // directly so this composition cannot silently inherit INV-080 after a
    // handler starts swallowing or translating a close failure into success.
    let production = include_str!("../../../src/v16_program.rs");
    assert!(production.contains(
        ".cure_and_cancel_close_not_atomic(&mut portfolio, optional_deposit)\n                .map_err(map_v16_error)?;"
    ));
    assert!(production.contains("let result = match group.permissionless_auto_crank_not_atomic("));
    assert!(production.contains("Err(err) => return Err(map_v16_error(err)),"));
}
