//! INV-069 - Terminal normalization and retirement.
//!
//! Normative obligation: economically empty accounts and assets can normalize
//! inert history and enter terminal/restarted states, while real obligations
//! such as provider receivables cannot be erased by cleanup.
//!
//! Evidence in this file (I/C): deployed LiteSVM public wrapper instructions cover
//! spent-only insurance history during asset restart, provider-receivable rejection with exact
//! rollback, retired-slot policy cleanup so unrelated batch trading remains live, and marketauth
//! terminal cleanup of an abandoned empty portfolio so CloseSlab can finish. The fixed-pin
//! terminal-blocker census composes those routes with the public expiry, reset-history,
//! pending-obligation, receipt, reservation, and zero-residue owners. It also source-locks the
//! wrapper to call the engine's proven retirement transition before local canonicalization.

use super::*;

fn terminal_spent_asset_env(with_provider_receivable: bool) -> (V16CuEnv, Keypair) {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    let admin = env.admin.insecure_clone();
    env.configure_auth_mark_with_cu(0, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    env.top_up_insurance_domain_with_authority(&admin, 2, 450);
    env.mutate_market(|_, group| {
        group.assets[1].lifecycle = AssetLifecycleV16::Recovery;
        group.insurance_domain_spent[2] = 450;
        group.insurance_domain_budget_remaining_total -= 450;
        group.insurance -= 450;
        group.c_tot += 450;
        if with_provider_receivable {
            let receivable = 250 * BOUND_SCALE;
            group.source_credit[3] = percolator::SourceCreditStateV16 {
                spent_backing_num: receivable,
                provider_receivable_num: receivable,
                ..percolator::SourceCreditStateV16::EMPTY
            };
            group.source_backing_buckets[3] = percolator::BackingBucketV16 {
                market_id: group.assets[1].market_id,
                consumed_liened_backing_num: receivable,
                expiry_slot: 1,
                status: BackingBucketStatusV16::Expired,
                ..percolator::BackingBucketV16::EMPTY
            };
        }
    });
    (env, admin)
}

#[test]
fn v16_program_spent_only_recovery_asset_can_restart_without_value_drift() {
    let (mut env, admin) = terminal_spent_asset_env(false);
    let before = env.market_state().1;
    let asset0_before = before.assets[0];
    let vault_before = before.vault;
    let c_tot_before = before.c_tot;
    let insurance_before = before.insurance;
    assert_eq!(before.assets[1].lifecycle, AssetLifecycleV16::Recovery);
    assert_eq!(before.insurance_domain_budget[2], 450);
    assert_eq!(before.insurance_domain_spent[2], 450);
    assert_eq!(before.insurance_domain_budget_remaining_total, 0);

    env.svm.warp_to_slot(3);
    let restart_cu = env
        .try_restart_asset_oracle_with_authority(&admin, 1, 3, 100)
        .expect("spent-only Recovery asset remains restartable");
    assert_cu_within(
        "spent-domain empty-asset RestartAssetOracle",
        restart_cu,
        CUSTODY_CU_LIMIT,
    );
    let after_restart = env.market_state().1;
    assert_eq!(after_restart.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(after_restart.assets[1].effective_price, 100);
    assert_eq!(after_restart.insurance_domain_budget[2], 0);
    assert_eq!(after_restart.insurance_domain_spent[2], 0);
    assert_eq!(after_restart.vault, vault_before);
    assert_eq!(after_restart.c_tot, c_tot_before);
    assert_eq!(after_restart.insurance, insurance_before);
    assert_eq!(after_restart.assets[0], asset0_before);
    assert_eq!(after_restart.vault as u64, env.token_amount(env.vault));
}

#[test]
fn v16_program_spent_cleanup_cannot_erase_provider_receivable() {
    let (mut env, admin) = terminal_spent_asset_env(true);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let before = env.market_state().1;
    assert_eq!(
        before.source_credit[3].provider_receivable_num,
        250 * BOUND_SCALE
    );
    assert_eq!(
        before.source_backing_buckets[3].consumed_liened_backing_num,
        250 * BOUND_SCALE
    );

    env.svm.warp_to_slot(3);
    let rejected = env.try_restart_asset_oracle_with_authority(&admin, 1, 3, 100);
    assert!(rejected.is_err(), "provider receivable must block restart");
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
}

#[test]
fn v16_program_retired_reused_asset_backing_fee_policy_cannot_stick_batch_gate() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.update_market_init_fee_policy_with_cu(1);

    let old_creator = Keypair::new();
    env.svm.warp_to_slot(1);
    env.activate_permissionless_asset_with_fee(
        &old_creator,
        1,
        1,
        100,
        old_creator.pubkey(),
        old_creator.pubkey(),
        old_creator.pubkey(),
        old_creator.pubkey(),
        1,
    );

    let update_policy =
        |env: &mut V16CuEnv, signer: &Keypair, fee_bps: u16| -> Result<u64, String> {
            env.svm.expire_blockhash();
            send_tx(
                &mut env.svm,
                env.program_id,
                &env.payer,
                ProgInstruction::UpdateBackingFeePolicy {
                    market_id: 0,
                    policy_sequence: u64::MAX,
                    domain: 2,
                    fee_bps,
                    insurance_share_bps: if fee_bps == 0 { 0 } else { 5_000 },
                    authority_epoch: 0,
                },
                vec![
                    AccountMeta::new(signer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                ],
                &[signer],
            )
        };

    update_policy(&mut env, &old_creator, 77).expect("old asset authority sets active policy");
    let (cfg_with_policy, _) = env.market_state();
    assert_eq!(cfg_with_policy.backing_trade_fee_policy_count, 1);

    env.svm.warp_to_slot(2);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_RETIRE,
        1,
        2,
        0,
    );
    let (retired_cfg, retired_group) = env.market_state();
    assert_eq!(
        retired_group.assets[1].lifecycle,
        AssetLifecycleV16::Retired
    );
    assert_eq!(
        retired_cfg.backing_trade_fee_policy_count, 0,
        "retired-slot backing fee must no longer hold the global batch gate"
    );
    assert!(
        update_policy(&mut env, &old_creator, 88).is_err(),
        "old authority must not set backing fees on an inactive retired slot"
    );
    assert_eq!(
        env.market_state().0.backing_trade_fee_policy_count,
        0,
        "rejected retired-slot policy update must not re-enable the batch gate"
    );

    let new_creator = Keypair::new();
    env.svm.warp_to_slot(3);
    env.activate_permissionless_asset_with_fee(
        &new_creator,
        1,
        3,
        250,
        new_creator.pubkey(),
        new_creator.pubkey(),
        new_creator.pubkey(),
        new_creator.pubkey(),
        1,
    );
    let (reused_cfg, reused_group) = env.market_state();
    assert_eq!(reused_group.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert_eq!(reused_cfg.backing_trade_fee_policy_count, 0);
    let reused_profile =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 1)
            .unwrap();
    assert_eq!(
        reused_profile.backing_trade_fee_bps_long, 0,
        "permissionless reuse installs a fresh profile without the old creator's backing fee"
    );

    let taker = Keypair::new();
    let lp = Keypair::new();
    let ta = env.create_portfolio(&taker);
    let la = env.create_portfolio(&lp);
    env.deposit(&taker, ta, 1_000_000);
    env.deposit(&lp, la, 1_000_000);
    let sz = (5 * POS_SCALE) as i128;
    env.svm.expire_blockhash();
    let batch = env.send(
        env.batch_trade_no_cpi_ix(
            ta,
            la,
            vec![BatchTradeLeg {
                asset_index: 0,
                market_id: first_generation_market_id(0),
                size_q: sz,
                exec_price: 100,
                fee_bps: 0,
            }],
        ),
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(lp.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ta, false),
            AccountMeta::new(la, false),
        ],
        &[&taker, &lp],
    );
    assert!(
        batch.is_ok(),
        "asset-0 batch trade must stay live after retiring/reusing an asset whose old policy was cleared: {batch:?}"
    );
}

#[test]
fn v16_program_abandoned_empty_portfolio_cannot_block_slab_close() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let attacker = Keypair::new();
    let abandoned = env.create_portfolio(&attacker);
    assert_eq!(env.portfolio_state(abandoned).capital.get(), 0);
    assert_eq!(env.market_state().1.materialized_portfolio_count, 1);

    env.resolve();
    let close_slab = |env: &mut V16CuEnv| -> Result<u64, String> {
        let dest = env.token_account(admin.pubkey(), 0);
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::CloseSlab { authority_epoch: 0 },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new(dest, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&admin],
        )
    };
    assert!(
        close_slab(&mut env).is_err(),
        "abandoned materialized portfolio blocks CloseSlab before cleanup"
    );

    let market_lamports_before = env.svm.get_account(&env.market).unwrap().lamports;
    let abandoned_lamports = env.svm.get_account(&abandoned).unwrap().lamports;
    env.svm.expire_blockhash();
    let cleanup = env.send(
        env.close_portfolio_ix(abandoned),
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(abandoned, false),
        ],
        &[&admin],
    );
    assert!(
        cleanup.is_ok(),
        "marketauth can close an abandoned empty portfolio after resolve: {cleanup:?}"
    );
    assert_eq!(env.market_state().1.materialized_portfolio_count, 0);
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().lamports,
        market_lamports_before + abandoned_lamports,
        "abandoned account rent moved to the market slab"
    );
    if let Some(closed) = env.svm.get_account(&abandoned) {
        assert_eq!(closed.lamports, 0, "abandoned portfolio rent swept");
        assert!(
            closed.data.is_empty() || !state::is_initialized(&closed.data),
            "abandoned portfolio dematerialized"
        );
    }

    assert!(
        close_slab(&mut env).is_ok(),
        "after abandoned empty cleanup the slab can be reclaimed"
    );
}

#[test]
fn v16_attack_restart_asset_oracle_rejects_backing_state_without_mutation() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    env.configure_permissionless_resolve_with_cu(100, 5);
    env.configure_auth_mark_with_cu(0, 100);
    env.top_up_backing_bucket(0, 500, 1_000);
    let funded = env.market_state().1;
    assert!(
        funded.source_backing_buckets[0].fresh_unliened_backing_num > 0,
        "test precondition: asset-0 backing bucket is funded"
    );

    env.svm.warp_to_slot(2);
    env.svm.expire_blockhash();
    env.try_shutdown_asset_with_authority(&admin, 0, 2)
        .expect("asset admin shuts down empty asset 0");
    let before_restart = env.svm.get_account(&env.market).unwrap();
    env.svm.expire_blockhash();
    let restart = env.try_restart_asset_oracle_with_authority(&admin, 0, 3, 150);
    assert!(
        restart.is_err(),
        "restart must not wipe live backing/source-credit/reservation state"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        before_restart,
        "rejected restart leaves the funded backing bucket and market bytes unchanged"
    );
    assert_eq!(
        env.market_state().1.source_backing_buckets[0].fresh_unliened_backing_num,
        funded.source_backing_buckets[0].fresh_unliened_backing_num,
        "funded backing bucket is still recoverable after rejected restart"
    );
    assert_eq!(
        env.market_state().1.assets[0].lifecycle,
        AssetLifecycleV16::Recovery
    );
}

// Coverage probe (Finding-G-adjacent): the RETIRE side of UpdateAssetLifecycle calls canonicalize_
// retired_asset_slot_view (v16_program 8928), which REJECTS unless the slot's insurance_domain_budget
// (long/short), spent, pending-loss-barrier, and backing utilization-earnings are ALL zero. If that
// guard were missing, RETIRE-ing an asset with a funded insurance domain budget would STRAND that
// value in a retired/reusable slot (withdrawable by nobody once retired, and inflating the aggregate
// vs. withdrawable) -> CloseSlab anti-strand check bricks. Existing tests cover the RESTART side
// (5817/5857); this covers the RETIRE side. Asserts: RETIRE rejects while funded (no mutation), and
// succeeds once the budget is drained (proving the budget is the sole blocker).
#[test]
fn v16_attack_retire_rejects_funded_insurance_domain_budget() {
    let mut env = V16CuEnv::new();
    env.update_market_init_fee_policy_with_cu(1);
    let attacker = Keypair::new();
    env.ensure_signer_account(attacker.pubkey());
    env.svm.warp_to_slot(1);
    // Permissionlessly append asset-1 with the attacker as all four domain authorities.
    env.activate_permissionless_asset_with_fee(
        &attacker,
        1,
        1,
        100,
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        1,
    );
    // Fund asset-1's long insurance domain (domain 2); attacker is its insurance_authority.
    env.top_up_insurance_domain_with_authority(&attacker, 2, 5_000);
    let (_, g_pre) = env.market_state();
    assert!(
        g_pre.insurance_domain_budget[2] > 0,
        "asset-1 domain funded (non-vacuous)"
    );

    let admin = env.admin.insecure_clone();
    let market = env.market;
    let admin_key = admin.pubkey();
    let market_id = env.asset_market_id(1);
    let retire_ix = |now_slot: u64| ProgInstruction::UpdateAssetLifecycle {
        action: percolator_prog::processor::ASSET_ACTION_RETIRE,
        asset_index: 1,
        market_id,
        authority_epoch: 0,
        now_slot,
        initial_price: 0,
        max_init_fee: u128::MAX,
        insurance_authority: admin_key.to_bytes(),
        insurance_operator: admin_key.to_bytes(),
        backing_bucket_authority: admin_key.to_bytes(),
        oracle_authority: admin_key.to_bytes(),
    };
    let retire_metas = || {
        vec![
            AccountMeta::new(admin_key, true),
            AccountMeta::new(market, false),
        ]
    };

    // marketauth RETIRE while the domain budget is funded -> must REJECT, no mutation.
    let market_before = env.svm.get_account(&env.market).unwrap();
    env.svm.warp_to_slot(3);
    env.svm.expire_blockhash();
    let retire = env.send(retire_ix(3), retire_metas(), &[&admin]);
    assert!(
        retire.is_err(),
        "RETIRE must reject while the asset's insurance domain budget is funded (would strand it)"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected RETIRE must not mutate the market"
    );
    assert_eq!(
        env.market_state().1.assets[1].lifecycle,
        AssetLifecycleV16::Active,
        "asset-1 stays Active after the rejected RETIRE"
    );

    // Drain the domain budget (the operator can), then RETIRE succeeds -> budget was the sole blocker.
    env.svm.expire_blockhash();
    env.try_withdraw_insurance_asset_with_authority(&attacker, 1, 5_000)
        .expect("operator drains asset-1's domain budget");
    env.svm.warp_to_slot(4);
    env.svm.expire_blockhash();
    let retire2 = env.send(retire_ix(4), retire_metas(), &[&admin]);
    assert!(
        retire2.is_ok(),
        "RETIRE succeeds once the domain budget is drained: {retire2:?}"
    );
    assert_eq!(
        env.market_state().1.assets[1].lifecycle,
        AssetLifecycleV16::Retired,
        "asset-1 retired after the budget is drained"
    );
}

// security.md sweep - RETIRE must also reject funded backing buckets (#22/#48): retiring an asset
// canonicalizes its slot for reuse. If fresh backing principal survived retirement, the provider's
// funds would become unreachable once the inactive slot can no longer be withdrawn from, and reuse
// could inherit stale backing bytes. This is distinct from the funded insurance-budget guard above.
#[test]
fn v16_attack_retire_rejects_funded_backing_bucket() {
    let mut env = V16CuEnv::new();
    env.activate_asset(1, 1, 100);
    env.top_up_backing_bucket(2, 700, 10_000);
    let (_, funded_group) = env.market_state();
    assert_eq!(
        funded_group.source_backing_buckets[2].fresh_unliened_backing_num,
        700 * BOUND_SCALE,
        "asset-1 backing bucket funded (non-vacuous)"
    );

    let admin = env.admin.insecure_clone();
    let market_before = env.svm.get_account(&env.market).unwrap();
    let market_id = env.asset_market_id(1);
    env.svm.warp_to_slot(3);
    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_RETIRE,
            asset_index: 1,
            market_id,
            authority_epoch: 0,
            now_slot: 3,
            initial_price: 0,
            max_init_fee: u128::MAX,
            insurance_authority: admin.pubkey().to_bytes(),
            insurance_operator: admin.pubkey().to_bytes(),
            backing_bucket_authority: admin.pubkey().to_bytes(),
            oracle_authority: admin.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        rejected.is_err(),
        "RETIRE must reject while the asset's backing bucket holds principal"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected backing-funded RETIRE must not mutate the market"
    );
    assert_eq!(
        env.market_state().1.assets[1].lifecycle,
        AssetLifecycleV16::Active,
        "asset-1 stays Active after the rejected RETIRE"
    );

    let backing_dest = env.token_account(admin.pubkey(), 0);
    env.withdraw_backing_bucket_to_admin_token_with_cu(backing_dest, 2, 700);
    assert_eq!(
        env.token_amount(backing_dest),
        700,
        "backing provider can recover the funded bucket"
    );

    env.svm.warp_to_slot(4);
    env.svm.expire_blockhash();
    let accepted = env.send(
        ProgInstruction::UpdateAssetLifecycle {
            action: percolator_prog::processor::ASSET_ACTION_RETIRE,
            asset_index: 1,
            market_id,
            authority_epoch: 0,
            now_slot: 4,
            initial_price: 0,
            max_init_fee: u128::MAX,
            insurance_authority: admin.pubkey().to_bytes(),
            insurance_operator: admin.pubkey().to_bytes(),
            backing_bucket_authority: admin.pubkey().to_bytes(),
            oracle_authority: admin.pubkey().to_bytes(),
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        accepted.is_ok(),
        "RETIRE succeeds once the backing bucket is drained: {accepted:?}"
    );
    let (_, retired_group) = env.market_state();
    assert_eq!(
        retired_group.assets[1].lifecycle,
        AssetLifecycleV16::Retired,
        "asset-1 retired after backing principal is drained"
    );
    assert_eq!(
        retired_group.source_backing_buckets[2].fresh_unliened_backing_num, 0,
        "retired slot carries no stale backing principal"
    );
}

// security.md sweep — permissionless append liveness (#44/#48): a stranger may legitimately grow
// the asset set, but that must not strand a flat user who deposited before the epoch change. The
// user has no asset-1 exposure, so a post-append full withdrawal must remain live and exact.
#[test]
fn v16_attack_permissionless_append_cannot_freeze_flat_withdrawal() {
    const FEE: u128 = 40;
    const DEPOSIT: u128 = 1_000;
    let mut env = V16CuEnv::new();
    env.update_market_init_fee_policy_with_cu(FEE);

    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, DEPOSIT);
    let (_, before) = env.market_state();
    assert_eq!(
        before.config.max_market_slots, 1,
        "starts as a one-asset market"
    );
    assert_eq!(env.portfolio_state(portfolio).capital.get(), DEPOSIT);

    let creator = Keypair::new();
    let cp = creator.pubkey();
    env.svm.warp_to_slot(1);
    env.activate_permissionless_asset_with_fee(&creator, 1, 1, 100, cp, cp, cp, cp, FEE);
    let (_, appended) = env.market_state();
    assert_eq!(
        appended.config.max_market_slots, 2,
        "permissionless append grew exactly one unrelated asset slot"
    );
    assert_eq!(appended.assets[1].lifecycle, AssetLifecycleV16::Active);
    assert!(
        appended.asset_set_epoch > before.asset_set_epoch,
        "asset-set epoch advanced across the append"
    );

    let (dest, _cu) = env.withdraw_with_cu(&owner, portfolio, DEPOSIT);
    let account = env.portfolio_state(portfolio);
    let (_, after_withdraw) = env.market_state();
    assert_eq!(
        env.token_amount(dest),
        DEPOSIT as u64,
        "full pre-append capital withdraws after the unrelated append"
    );
    assert_eq!(account.capital.get(), 0, "portfolio capital fully debited");
    assert!(
        percolator::active_bitmap_is_empty(active_bitmap(&account)),
        "flat account remains exposure-free"
    );
    assert_eq!(
        after_withdraw.c_tot, 0,
        "only the user's collateral left c_tot"
    );
    assert_eq!(
        after_withdraw.vault,
        appended.vault - DEPOSIT,
        "accounting vault debited exactly the withdrawal"
    );
    assert_eq!(
        after_withdraw.insurance, appended.insurance,
        "permissionless create fee insurance remains untouched"
    );
    assert_domain_budget_remaining_total_consistent(&after_withdraw, "post-append flat withdraw");
}

#[derive(Clone, Copy)]
struct Inv069TerminalBlockerClass {
    class: &'static str,
    engine_proofs: &'static [&'static str],
    public_witnesses: &'static [&'static str],
}

#[test]
fn v16_program_terminal_blocker_census_composes_engine_retirement_before_wrapper_cleanup() {
    const ENGINE_PIN: &str = "495a5590c97055bd71c6f94d849ff0298f243145";
    const CLASSES: &[Inv069TerminalBlockerClass] = &[
        Inv069TerminalBlockerClass {
            class: "live OI, stored legs, stale cohorts, side modes, and prior epochs",
            engine_proofs: &[
                "proof_v16_retire_nonempty_asset_rejects",
                "proof_v16_retire_empty_asset_is_value_neutral_and_epoch_scoped",
            ],
            public_witnesses: &[
                "v16_program_unilateral_zero_oi_reset_route_side_matrix_finalizes_permissionlessly",
            ],
        },
        Inv069TerminalBlockerClass {
            class: "pending loss, B, social-loss, and source-spent history",
            engine_proofs: &[
                "proof_v16_retire_empty_asset_is_value_neutral_and_epoch_scoped",
                "proof_v16_terminal_restart_cannot_erase_provider_receivable",
            ],
            public_witnesses: &[
                "v16_program_reset_carry_liquidation_matrix_preserves_progress",
                "v16_program_flat_negative_final_leg_route_matrix_reaches_terminal_payout",
            ],
        },
        Inv069TerminalBlockerClass {
            class: "fresh and expired backing labels plus provider receivables",
            engine_proofs: &[
                "proof_v16_retire_live_provider_receivable_rejects_without_mutation",
                "proof_v16_retire_normalizes_unreferenced_lapsed_backing",
                "proof_v16_retirement_backing_normalization_never_erases_obligations",
            ],
            public_witnesses: &[
                "v16_program_retirement_obligation_lattice_is_order_independent",
                "v16_program_retire_normalizes_unreferenced_lapsed_backing",
                "v16_program_asset0_recovery_matrix_preserves_provider_withdraw_and_restart_progress",
            ],
        },
        Inv069TerminalBlockerClass {
            class: "insurance budgets, spent history, and live reservations",
            engine_proofs: &[
                "proof_v16_public_terminal_insurance_retirement_rejects_every_live_reservation_class",
                "proof_v16_public_terminal_insurance_retirement_is_exact_and_fully_framed",
            ],
            public_witnesses: &[
                "v16_program_retirement_obligation_lattice_is_order_independent",
                "v16_program_recovery_resource_failure_lattice_preserves_public_exit",
            ],
        },
        Inv069TerminalBlockerClass {
            class: "resolved receipts, pending topups, materialized accounts, and slab residue",
            engine_proofs: &[
                "proof_v16_public_terminal_insurance_retirement_requires_resolved_ready_accounts",
            ],
            public_witnesses: &[
                "v16_program_receipt_conflict_seeded_frontier_is_exact_and_terminal",
                "v16_attack_marketauth_terminal_close_cannot_burn_pending_payout_topup",
                "v16_program_recovery_force_close_reaches_zero_residue_and_close_slab",
            ],
        },
        Inv069TerminalBlockerClass {
            class: "retired wrapper profile policy and authority residue",
            engine_proofs: &[
                "proof_v16_canonical_retired_asset_slot_preserves_identity_and_clears_local_ledgers",
            ],
            public_witnesses: &[
                "v16_program_retired_reused_asset_backing_fee_policy_cannot_stick_batch_gate",
            ],
        },
    ];

    let cargo = include_str!("../../../Cargo.toml");
    assert_eq!(
        cargo.matches(&format!("rev = \"{ENGINE_PIN}\"")).count(),
        2,
        "INV-069 composes exact engine retirement proofs and must reopen on a pin change",
    );

    let witness_sources = [
        include_str!("inv_061_deterministic_bounded_liquidation.rs"),
        include_str!("inv_069_terminal_normalization_and_retirement.rs"),
        include_str!("inv_070_zero_unattributed_terminal_residue_and_close_slab.rs"),
        include_str!("inv_073_no_permanent_user_lock.rs"),
        include_str!("../stateful/inv_063_backing_expiry_normalization.rs"),
        include_str!("../stateful/inv_065_reset_recovery_and_retired_state_isolation.rs"),
        include_str!("../stateful/inv_069_terminal_normalization_and_retirement.rs"),
        include_str!("../stateful/inv_071_crank_progress.rs"),
        include_str!("../stateful/inv_078_permissionless_recovery_coverage.rs"),
        include_str!("../stateful/inv_086_reference_model_and_deployed_transition_equivalence.rs"),
        include_str!("inv_063_backing_expiry_normalization.rs"),
    ];
    let mut classes = std::collections::BTreeSet::new();
    let mut proofs = std::collections::BTreeSet::new();
    for row in CLASSES {
        assert!(
            classes.insert(row.class),
            "duplicate terminal blocker class"
        );
        assert!(!row.engine_proofs.is_empty());
        assert!(!row.public_witnesses.is_empty());
        for proof in row.engine_proofs {
            assert!(proofs.insert(*proof) || proofs.contains(proof));
            assert!(proof.starts_with("proof_v16_"));
        }
        for witness in row.public_witnesses {
            assert!(
                witness_sources
                    .iter()
                    .any(|source| source.contains(&format!("fn {witness}"))),
                "terminal blocker class '{}' lacks public witness {witness}",
                row.class,
            );
        }
    }

    let production = include_str!("../../../src/v16_program.rs");
    let production = production
        .split("    #[cfg(test)]\n    mod tests")
        .next()
        .expect("production prefix exists");
    let canonicalizer = production
        .split_once("fn canonicalize_retired_asset_slot_view")
        .map(|(_, tail)| tail)
        .and_then(|tail| tail.split_once("fn handle_restart_asset_oracle"))
        .map(|(body, _)| body)
        .expect("retired-slot canonicalizer exists");
    for guard in [
        "AssetLifecycleV16::Retired",
        "asset.market_id == 0",
        "asset.retired_slot == 0",
        "insurance_domain_budget_long.get() != 0",
        "insurance_domain_budget_short.get() != 0",
        "insurance_domain_spent_long.get() != 0",
        "insurance_domain_spent_short.get() != 0",
        "pending_domain_loss_barrier_long.get() != 0",
        "pending_domain_loss_barrier_short.get() != 0",
        "backing_long.utilization_fee_earnings != 0",
        "backing_short.utilization_fee_earnings != 0",
    ] {
        assert!(
            canonicalizer.contains(guard),
            "wrapper retired-slot cleanup lost blocker guard: {guard}",
        );
    }

    let lifecycle_handler = production
        .split_once("fn handle_update_asset_lifecycle")
        .map(|(_, tail)| tail)
        .and_then(|tail| tail.split_once("fn handle_finalize_reset_side"))
        .map(|(body, _)| body)
        .expect("asset lifecycle handler exists");
    assert_eq!(
        lifecycle_handler
            .matches(".retire_empty_asset_not_atomic(")
            .count(),
        2,
        "both first retirement and idempotent recanonicalization need engine validation",
    );
    assert_eq!(
        lifecycle_handler
            .matches("canonicalize_retired_asset_slot_view(")
            .count(),
        2,
        "both retirement branches need wrapper-local canonicalization",
    );
    let mut tail = lifecycle_handler;
    for branch in 0..2 {
        let engine_offset = tail
            .find(".retire_empty_asset_not_atomic(")
            .unwrap_or_else(|| panic!("retirement branch {branch} lost engine validation"));
        tail = &tail[engine_offset + 1..];
        let canonical_offset = tail
            .find("canonicalize_retired_asset_slot_view(")
            .unwrap_or_else(|| panic!("retirement branch {branch} lost canonicalization"));
        let next_engine_offset = tail.find(".retire_empty_asset_not_atomic(");
        assert!(
            next_engine_offset.is_none() || canonical_offset < next_engine_offset.unwrap(),
            "branch {branch} canonicalized before the matching engine validation",
        );
        tail = &tail[canonical_offset + 1..];
    }

    let transition_census =
        include_str!("inv_088_global_summaries_are_not_account_local_proofs.rs");
    assert!(transition_census.contains(
        "owner: \"handle_update_asset_lifecycle\", method: \"retire_empty_asset_not_atomic\""
    ));
}
