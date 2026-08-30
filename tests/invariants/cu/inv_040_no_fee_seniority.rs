//! INV-040 - No fee seniority.
//!
//! Fees are junior to protected principal: an uncollectible protocol/trading
//! fee must be dropped or capped by available participant value, not charged
//! from another protected pool, minted into insurance, or allowed to block a
//! risk-reducing exit.
//!
//! The public route tests in this file put one side at an adverse maintenance
//! boundary, then execute a full exit whose quoted fee exceeds that side's
//! remaining capital. They check the successful exit, the actual insurance
//! delta, exact aggregate-capital conservation, and zero token-vault movement.
//! The source-complete census at the end composes those tests with the public
//! base-fee, backing-fee, maintenance, liquidation, resolved-close, Recovery,
//! and activation-fee evidence owned by adjacent invariants. It proves that the
//! wrapper has no independent protected-pool writer and delegates every
//! internal fee debit to the exact pinned engine. The engine pin owns the fee
//! cap, loss-before-fee ordering, and senior-stock function contracts; this
//! wrapper invariant owns admission, returned-amount attribution, custody, and
//! public route composition without duplicating those engine proofs.

use super::*;

fn assert_underfunded_exit_drops_only_uncollectible_fee(path: NoCpiReportedPricePath) {
    const MARK: u64 = 1_000_000;
    const SIZE_Q: i128 = POS_SCALE as i128;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: MARK,
        max_trading_fee_bps: 10_000,
        max_price_move_bps_per_slot: 10_000,
        max_accrual_dt_slots: 1,
        min_funding_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.configure_ewma_mark_with_cu(0, MARK, 1, 0);
    let (long_owner, long, short_owner, short) =
        funded_no_cpi_reported_price_pair(&mut env, MARK as u128);

    try_no_cpi_reported_price_trade_with_cu(
        &mut env,
        path,
        &long_owner,
        long,
        &short_owner,
        short,
        SIZE_Q,
        MARK,
        0,
    )
    .unwrap_or_else(|err| panic!("{path:?}: setup open failed: {err}"));

    env.svm.warp_to_slot(10);
    env.push_ewma_mark_with_cu(10, 1);
    env.svm.expire_blockhash();
    env.crank_steps_after_market_catchup(
        long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
        1,
    );
    env.svm.expire_blockhash();
    env.crank(
        short,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
    );

    env.svm.warp_to_slot(20);
    let (_, group_before) = env.market_state();
    let reported_exit_price = group_before.assets[0]
        .effective_price
        .checked_mul(2)
        .expect("one-slot upper price envelope");
    let requested_fee_per_side = reported_exit_price as u128;
    let long_before = env.portfolio_state(long);
    let short_before = env.portfolio_state(short);
    let aggregate_capital_before = long_before
        .capital
        .get()
        .checked_add(short_before.capital.get())
        .expect("aggregate capital before");
    assert!(
        0 < long_before.capital.get() && long_before.capital.get() < requested_fee_per_side,
        "{path:?}: setup must make one side's quoted fee partly uncollectible",
    );
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    env.svm.expire_blockhash();
    let exit = try_no_cpi_reported_price_trade_with_cu(
        &mut env,
        path,
        &long_owner,
        long,
        &short_owner,
        short,
        -SIZE_Q,
        reported_exit_price,
        0,
    );
    assert!(
        exit.is_ok(),
        "{path:?}: uncollectible fee must not DoS a risk-reducing full exit: {exit:?}",
    );

    let (_, group_after) = env.market_state();
    assert_eq!(group_after.assets[0].oi_eff_long_q, 0);
    assert_eq!(group_after.assets[0].oi_eff_short_q, 0);
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "{path:?}: fee collection is internal and must not mint or move vault tokens",
    );
    assert_eq!(
        group_after.vault, group_before.vault,
        "{path:?}: internal fee accounting must not change token-stock accounting",
    );

    let collected_fee = group_after.insurance - group_before.insurance;
    let quoted_two_sided_fee = requested_fee_per_side * 2;
    assert!(
        collected_fee < quoted_two_sided_fee,
        "{path:?}: the uncollectible part of the quoted fee must not be credited to insurance",
    );
    let aggregate_capital_after = env
        .portfolio_state(long)
        .capital
        .get()
        .checked_add(env.portfolio_state(short).capital.get())
        .expect("aggregate capital after");
    assert_eq!(
        aggregate_capital_before - aggregate_capital_after,
        collected_fee,
        "{path:?}: aggregate user capital may fall only by the actually collected fee",
    );
}

#[test]
fn v16_program_uncollectible_exit_fee_is_dropped_not_senioritized() {
    for path in [
        NoCpiReportedPricePath::Single,
        NoCpiReportedPricePath::Batch,
    ] {
        assert_underfunded_exit_drops_only_uncollectible_fee(path);
    }
}

fn assert_underfunded_cpi_exit_drops_only_uncollectible_fee(path: CpiEwmaTradePath) {
    const MARK: u64 = 1_000_000;
    const ADVERSE_MARK: u64 = 1_999_999;
    const SIZE_Q: i128 = POS_SCALE as i128;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: MARK,
        max_trading_fee_bps: 10_000,
        max_price_move_bps_per_slot: 10_000,
        max_accrual_dt_slots: 1,
        min_funding_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.configure_ewma_mark_with_cu(0, MARK, 1, 0);

    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp_owner = Keypair::new();
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, MARK as u128);
    env.deposit(&lp_owner, lp, MARK as u128);
    let (open_ctx, open_delegate, _) = env.init_matcher_context_with_passive_spread_authorized(
        matcher_program,
        &lp_owner,
        lp,
        0,
        9_000,
    );

    env.svm.expire_blockhash();
    env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &lp_owner,
        lp,
        matcher_program,
        open_ctx,
        open_delegate,
        0,
        -SIZE_Q,
        0,
    )
    .unwrap_or_else(|err| panic!("{path:?}: setup short open failed: {err}"));

    env.svm.warp_to_slot(10);
    env.push_ewma_mark_with_cu(10, ADVERSE_MARK);
    env.svm.expire_blockhash();
    env.crank_steps_after_market_catchup(
        taker,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
        1,
    );
    env.svm.expire_blockhash();
    env.crank(
        lp,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
    );

    let (exit_ctx, exit_delegate, _) = env.init_matcher_context_with_passive_spread_authorized(
        matcher_program,
        &lp_owner,
        lp,
        9_000,
        9_000,
    );
    env.svm.warp_to_slot(20);
    let (_, group_before) = env.market_state();
    let expected_matcher_price = group_before.assets[0]
        .effective_price
        .checked_mul(19)
        .expect("matcher ask numerator")
        / 10;
    let accepted_exit_price = oracle_v16::clamp_toward_engine_dt(
        group_before.assets[0].effective_price,
        expected_matcher_price,
        10_000,
        1,
    );
    assert_eq!(
        accepted_exit_price, expected_matcher_price,
        "{path:?}: wide matcher ask must remain inside the one-segment engine envelope"
    );
    let requested_fee_per_side = accepted_exit_price as u128;
    let taker_before = env.portfolio_state(taker);
    let lp_before = env.portfolio_state(lp);
    let aggregate_capital_before = taker_before
        .capital
        .get()
        .checked_add(lp_before.capital.get())
        .expect("aggregate capital before");
    assert!(
        0 < taker_before.capital.get() && taker_before.capital.get() < requested_fee_per_side,
        "{path:?}: setup must leave the adverse short unable to pay its quoted exit fee",
    );
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    env.svm.expire_blockhash();
    let exit = match path {
        CpiEwmaTradePath::Single => env.try_trade_cpi_with_cu_on_asset(
            &taker_owner,
            taker,
            &lp_owner,
            lp,
            matcher_program,
            exit_ctx,
            exit_delegate,
            0,
            SIZE_Q,
            0,
        ),
        CpiEwmaTradePath::Batch => env.send(
            env.batch_trade_cpi_ix(
                taker,
                lp,
                vec![BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: group_before.assets[0].market_id,
                    size_q: SIZE_Q,
                    fee_bps: 0,
                    limit_price: 0,
                }],
            ),
            vec![
                AccountMeta::new(taker_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker, false),
                AccountMeta::new(lp, false),
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new(exit_ctx, false),
                AccountMeta::new_readonly(exit_delegate, false),
            ],
            &[&taker_owner],
        ),
    };
    assert!(
        exit.is_ok(),
        "{path:?}: uncollectible CPI fee must not DoS a risk-reducing full exit: {exit:?}",
    );

    let (_, group_after) = env.market_state();
    assert_eq!(group_after.assets[0].oi_eff_long_q, 0);
    assert_eq!(group_after.assets[0].oi_eff_short_q, 0);
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "{path:?}: CPI exit fee collection is internal and must not move vault tokens",
    );
    assert_eq!(
        group_after.vault, group_before.vault,
        "{path:?}: CPI internal fee accounting must not change token-stock accounting",
    );

    let collected_fee = group_after.insurance - group_before.insurance;
    let quoted_two_sided_fee = requested_fee_per_side * 2;
    assert!(
        collected_fee < quoted_two_sided_fee,
        "{path:?}: the uncollectible CPI fee must not be credited to insurance",
    );
    let aggregate_capital_after = env
        .portfolio_state(taker)
        .capital
        .get()
        .checked_add(env.portfolio_state(lp).capital.get())
        .expect("aggregate capital after");
    assert_eq!(
        aggregate_capital_before - aggregate_capital_after,
        collected_fee,
        "{path:?}: CPI aggregate user capital may fall only by the actually collected fee",
    );
}

#[test]
fn v16_program_cpi_uncollectible_exit_fee_is_dropped_not_senioritized() {
    for path in [CpiEwmaTradePath::Single, CpiEwmaTradePath::Batch] {
        assert_underfunded_cpi_exit_drops_only_uncollectible_fee(path);
    }
}

// DoS-resistance: SyncMaintenanceFee is permissionless, so an attacker could try to grief a victim by
// spamming it to over-drain their capital, or by passing a far-future now_slot to charge future time.
// The fee is time-based (charged on real elapsed slots, last_sync advanced to now) and uses the
// AUTHENTICATED clock -> spamming in one slot charges only once, and a future now_slot charges nothing
// extra. The victim pays exactly the elapsed-time fee they already owe, no more.
#[test]
fn v16_attack_maintenance_fee_spam_cannot_overdrain() {
    let mut env =
        V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(1, 1_000, 1_000, 500, 100);
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    let cap0 = env.portfolio_state(p).capital.get();
    env.svm.warp_to_slot(50);
    // first sync at slot 50: charges the elapsed-time maintenance fee.
    env.sync_maintenance_fee_with_cu(p, None, 50);
    let cap1 = env.portfolio_state(p).capital.get();
    assert!(
        cap1 < cap0,
        "first sync charges the accrued maintenance fee"
    );
    // SPAM: repeated syncs in the same slot charge nothing more (idempotent -> no grief over-drain).
    for _ in 0..5 {
        env.svm.expire_blockhash();
        env.sync_maintenance_fee_with_cu(p, None, 50);
    }
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        cap1,
        "spamming sync in one slot must not over-charge"
    );
    // FUTURE now_slot lie: real clock is still 50, so no future time can be charged.
    env.svm.expire_blockhash();
    env.sync_maintenance_fee_with_cu(p, None, 50 + 1_000_000);
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        cap1,
        "a future now_slot cannot charge future maintenance time"
    );
    // real time advancing DOES accrue more (fee is genuinely time-based).
    env.svm.warp_to_slot(100);
    env.svm.expire_blockhash();
    env.sync_maintenance_fee_with_cu(p, None, 100);
    assert!(
        env.portfolio_state(p).capital.get() < cap1,
        "advancing real time accrues additional fee"
    );
}

#[derive(Clone, Copy)]
struct Inv040FeeIngress {
    owner: &'static str,
    method: &'static str,
    count: usize,
    fee_class: &'static str,
    witness: &'static str,
}

#[test]
fn v16_program_internal_fee_ingress_is_engine_owned_and_publicly_witnessed() {
    const ENGINE_PIN: &str = "ce590d9bd3e8f55eaf2e0321f36ef11fbc003d26";
    const ROWS: &[Inv040FeeIngress] = &[
        Inv040FeeIngress {
            owner: "collect_maintenance_fee_to_slot_before_value_debit_view",
            method: "sync_account_fee_to_slot_not_atomic",
            count: 1,
            fee_class: "maintenance-before-value-debit",
            witness: "v16_program_issue408_unsigned_matcher_cannot_spend_aged_maintenance_collateral",
        },
        Inv040FeeIngress {
            owner: "handle_trade_nocpi_zero_copy",
            method: "execute_trade_with_fee_loss_stale_scoped_not_atomic",
            count: 2,
            fee_class: "single-cpi-and-no-cpi-trade",
            witness: "v16_program_signed_direction_route_matrix_preserves_side_attribution_and_terminal_value",
        },
        Inv040FeeIngress {
            owner: "handle_batch_execute_zero_copy",
            method: "execute_batch_with_fee_loss_stale_scoped_not_atomic",
            count: 1,
            fee_class: "batch-cpi-and-no-cpi-trade",
            witness: "v16_program_mixed_direction_fee_allocation_matches_independent_side_ledger",
        },
        Inv040FeeIngress {
            owner: "handle_sync_maintenance_fee",
            method: "sync_account_fee_to_slot_not_atomic",
            count: 3,
            fee_class: "explicit-maintenance-and-reward",
            witness: "v16_bpf_sync_maintenance_fee_with_cranker_share_is_bounded",
        },
        Inv040FeeIngress {
            owner: "handle_close_resolved",
            method: "permissionless_auto_crank_not_atomic",
            count: 1,
            fee_class: "resolved-maintenance",
            witness: "v16_audit_resolved_maintenance_fee_insurance_stays_recoverable",
        },
        Inv040FeeIngress {
            owner: "handle_permissionless_crank_zero_copy",
            method: "permissionless_auto_crank_not_atomic",
            count: 2,
            fee_class: "liquidation-and-maintenance",
            witness: "v16_program_issue408_liquidation_reward_cannot_preempt_aged_maintenance_collateral",
        },
        Inv040FeeIngress {
            owner: "charge_account_backing_domain_fees_view",
            method: "charge_account_backing_fee_not_atomic",
            count: 1,
            fee_class: "source-backing-provider-and-insurance",
            witness: "v16_program_pr223_cpi_backing_fee_consent_fuzz",
        },
    ];
    const FEE_METHODS: &[&str] = &[
        "sync_account_fee_to_slot_not_atomic",
        "execute_trade_with_fee_loss_stale_scoped_not_atomic",
        "execute_batch_with_fee_loss_stale_scoped_not_atomic",
        "permissionless_auto_crank_not_atomic",
        "charge_account_backing_fee_not_atomic",
    ];

    let cargo = include_str!("../../../Cargo.toml");
    assert_eq!(
        cargo.matches(&format!("rev = \"{ENGINE_PIN}\"")).count(),
        2,
        "INV-040 engine proof composition must be re-audited on every engine pin change",
    );

    let production = include_str!("../../../src/v16_program.rs");
    let production = production
        .split("    #[cfg(test)]\n    mod tests")
        .next()
        .expect("production prefix exists");
    let processor = production
        .split_once("pub mod processor {")
        .map(|(_, processor)| processor)
        .expect("deployed processor module exists");

    for forbidden_write in [
        "header.capital =",
        "header.c_tot =",
        "header.insurance =",
        ".utilization_fee_earnings =",
    ] {
        assert!(
            !processor.contains(forbidden_write),
            "wrapper introduced an independent protected-pool writer: {forbidden_write}",
        );
    }
    for disabled_policy in [
        "backing_fee_base_rate_e9_per_slot",
        "backing_fee_slope_at_kink_e9_per_slot",
        "backing_fee_slope_above_kink_e9_per_slot",
    ] {
        assert!(
            !processor.contains(disabled_policy),
            "wrapper exposed an unrostered recurring backing-utilization fee policy: {disabled_policy}",
        );
    }

    let mut current_function = "<module>";
    let mut actual = std::collections::BTreeMap::<(String, String), usize>::new();
    for line in processor.lines() {
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
        for method in FEE_METHODS {
            let needle = format!(".{method}(");
            let count = line.matches(&needle).count();
            if count != 0 {
                *actual
                    .entry((current_function.to_string(), (*method).to_string()))
                    .or_default() += count;
            }
        }
    }

    let witness_sources = [
        include_str!("inv_027_protected_principal_seniority.rs"),
        include_str!("inv_036_fee_destination_and_policy_version_integrity.rs"),
        include_str!("inv_040_no_fee_seniority.rs"),
        include_str!("inv_061_deterministic_bounded_liquidation.rs"),
        include_str!("inv_067_terminal_payout_completeness_and_exact_once_settlement.rs"),
        include_str!("inv_071_crank_progress.rs"),
        include_str!("inv_073_no_permanent_user_lock.rs"),
        include_str!("inv_077_bounded_work_and_maximum_shape_compute.rs"),
        include_str!("inv_078_permissionless_recovery_coverage.rs"),
        include_str!("inv_088_global_summaries_are_not_account_local_proofs.rs"),
        include_str!("inv_089_activation_reactivation_and_initialization_equivalence.rs"),
        include_str!("../public_sbf/inv_036_fee_destination_and_policy_version_integrity.rs"),
        include_str!("../stateful/inv_036_fee_destination_and_policy_version_integrity.rs"),
    ];
    let mut expected = std::collections::BTreeMap::new();
    for row in ROWS {
        assert!(!row.fee_class.is_empty());
        assert!(
            witness_sources
                .iter()
                .any(|source| source.contains(&format!("fn {}", row.witness))),
            "{}.{} lacks executable public witness {}",
            row.owner,
            row.method,
            row.witness,
        );
        assert!(
            expected
                .insert((row.owner.to_string(), row.method.to_string()), row.count)
                .is_none(),
            "duplicate fee-ingress classification for {}.{}",
            row.owner,
            row.method,
        );
    }
    assert_eq!(
        actual, expected,
        "every wrapper ingress to an engine fee-bearing transition needs an INV-040 class and public witness",
    );
    let recovery_handler = processor
        .split_once("fn handle_force_close_abandoned_asset")
        .map(|(_, tail)| tail)
        .and_then(|tail| tail.split_once("fn matcher_tail_start_or_verify_lp_config"))
        .map(|(handler, _)| handler)
        .expect("Recovery force-close handler exists");
    assert_eq!(
        recovery_handler
            .matches(".force_close_recovery_pair_not_atomic(")
            .count(),
        1,
        "Recovery pair close must use the pinned zero-fee canonical transition",
    );
    assert!(
        !recovery_handler.contains(".execute_trade_with_fee_loss_stale_scoped_not_atomic("),
        "the wrapper must not reconstruct Recovery as an independently fee-bearing trade",
    );

    let activation_evidence =
        include_str!("../public_sbf/inv_036_fee_destination_and_policy_version_integrity.rs");
    assert!(activation_evidence
        .contains("fn v16_program_pr314_permissionless_activation_fee_requires_creator_consent"));
    assert!(processor.contains("permissionless_market_init_fee_for_asset("));
    assert!(processor.contains("fee > max_init_fee"));

    let transition_census =
        include_str!("inv_088_global_summaries_are_not_account_local_proofs.rs");
    assert!(transition_census.contains(
        "fn v16_program_every_wrapper_engine_transition_callsite_has_summary_disposition_and_witness"
    ));
}
