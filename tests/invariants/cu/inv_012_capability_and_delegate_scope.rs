//! INV-012 - capability and delegate scope.
//!
//! Normative obligation: a matcher capability is portfolio-local, owner-controlled, and valid
//! only while the configured matcher has observed every position mutation. A mutation outside
//! that matcher must invalidate the capability; a fill through the configured matcher may retain
//! it for the participating LP only.
//!
//! Evidence in this file (SVM/CU plus a production-source roster): owner/delegate and
//! tuple-substitution tests cover the original
//! authorization boundary. The issue-406 matrix reaches partial permissionless liquidation,
//! force-close plus asset retirement/reuse, direct/batch no-CPI fills, and direct/batch CPI fills
//! through public instructions. It observes the real external matcher inventory, proves stale
//! unsigned fills reject with exact rollback, and proves fresh owner authorization restores the
//! same fill. Direct and batch controls distinguish the synchronized LP from a taker's unrelated
//! matcher and assert that invalidation preserves the signed fee cap.
//!
//! INV-016 exhausts the complete delegate PDA seed tuple. INV-002/003/004 independently compose
//! asset generation, portfolio incarnation, and position episode checks around both CPI routes.
//! The capability authorizes only CPI trade matching, so a separate operation set is structurally
//! inapplicable; per-leg asset scope is carried by the generation-bound trade request.
//!
//! Guarantee boundary: both retained CPI routes bind the persisted matcher-config incarnation.
//! The deployed capability still has no expiry, which remains explicit in AUDIT-012.

use super::*;

fn issue406_matcher_inventory(data: &[u8]) -> i128 {
    i128::from_le_bytes(data[160..176].try_into().unwrap())
}

fn inv012_function_body<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start = source
        .find(start)
        .unwrap_or_else(|| panic!("missing production function {start}"));
    let tail = &source[start..];
    let end = tail
        .find(end)
        .unwrap_or_else(|| panic!("missing production successor {end}"));
    &tail[..end]
}

#[test]
fn v16_program_matcher_capability_route_roster_binds_every_current_scope() {
    let source = include_str!("../../../src/v16_program.rs");
    let single = inv012_function_body(
        source,
        "fn handle_trade_cpi<'a>(",
        "fn handle_set_matcher_config<'a>(",
    );
    let batch = inv012_function_body(
        source,
        "fn handle_batch_trade_cpi<'a>(",
        "fn handle_permissionless_crank<'a>(",
    );
    let config = inv012_function_body(
        source,
        "fn handle_set_matcher_config<'a>(",
        "fn invoke_matcher_batch<'a>(",
    );
    let matcher_guard = inv012_function_body(
        source,
        "fn matcher_tail_start_or_verify_lp_config<'a>(",
        "fn validate_matcher_tail<'a>(",
    );

    for (route, body) in [("TradeCpi", single), ("BatchTradeCpi", batch)] {
        assert_eq!(
            body.matches("expect_portfolio_position_binding(").count(),
            2,
            "{route} must bind both portfolio incarnations and position episodes"
        );
        assert!(body.contains("derive_matcher_delegate("));
        assert!(body.contains("matcher_tail_start_or_verify_lp_config("));
        assert!(body.contains("account_b_matcher_sequence,"));
        assert!(body.contains("account_a_header.portfolio_account_id"));
        assert!(body.contains("account_b_header.portfolio_account_id"));
    }
    assert!(single.contains("market_id != expected_market_id"));
    assert!(batch.contains("AssetGenerationMismatch"));
    assert!(batch.contains("leg.market_id != *market_id"));
    assert!(config.contains("portfolio_id != current_portfolio_id"));
    assert!(config.contains("expected_sequence != current_sequence"));
    assert!(config.contains("derive_matcher_delegate("));
    assert!(matcher_guard.contains("read_portfolio_matcher_sequence(&account_b_data)?"));
    assert!(matcher_guard.contains("!= expected_matcher_sequence"));
    assert!(matcher_guard.contains("PercolatorError::EngineStale"));
    assert!(matcher_guard.contains("read_portfolio_matcher_config(&account_b_data)?"));

    assert_eq!(
        source
            .matches("matcher_tail_start_or_verify_lp_config(")
            .count(),
        2,
        "only the two current CPI trade routes may consume matcher capability"
    );
    assert_eq!(
        source.matches("authorizes_matcher_tuple(").count(),
        2,
        "the typed predicate must have one definition and one production consumer"
    );
}

fn issue406_signed_trade_invalidates_both_matchers(batch: bool) {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher SBF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let owner_a = Keypair::new();
    let owner_b = Keypair::new();
    let account_a = env.create_portfolio(&owner_a);
    let account_b = env.create_portfolio(&owner_b);
    env.deposit(&owner_a, account_a, 1_000_000);
    env.deposit(&owner_b, account_b, 1_000_000);
    env.init_auth_matcher_context(matcher_program, &owner_a, account_a);
    env.init_auth_matcher_context(matcher_program, &owner_b, account_b);
    assert_eq!(env.portfolio_matcher_config(account_a).enabled(), 1);
    assert_eq!(env.portfolio_matcher_config(account_b).enabled(), 1);

    if batch {
        env.send(
            env.batch_trade_no_cpi_ix(
                account_a,
                account_b,
                vec![BatchTradeLeg {
                    asset_index: 0,
                    market_id: env.asset_market_id(0),
                    size_q: POS_SCALE as i128,
                    exec_price: 100,
                    fee_bps: 0,
                }],
            ),
            vec![
                AccountMeta::new(owner_a.pubkey(), true),
                AccountMeta::new(owner_b.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(account_a, false),
                AccountMeta::new(account_b, false),
            ],
            &[&owner_a, &owner_b],
        )
        .expect("signed batch trade");
    } else {
        env.trade_asset_with_cu(
            0,
            &owner_a,
            account_a,
            &owner_b,
            account_b,
            POS_SCALE as i128,
            100,
            0,
        );
    }

    for account in [account_a, account_b] {
        let config = env.portfolio_matcher_config(account);
        assert_eq!(
            config.enabled(),
            0,
            "a signed trade outside either configured matcher invalidates that matcher"
        );
        assert_eq!(config.position_epoch(), 1);
        assert_eq!(config.trade_fee_cap_bps(), 10_000);
    }
}

#[test]
fn v16_program_issue406_signed_trade_routes_invalidate_both_matcher_capabilities() {
    issue406_signed_trade_invalidates_both_matchers(false);
    issue406_signed_trade_invalidates_both_matchers(true);
}

fn issue406_matcher_trade_preserves_only_participating_lp(batch: bool) {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher SBF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&lp_owner, lp, 1_000_000);
    env.init_auth_matcher_context(matcher_program, &taker_owner, taker);
    let (ctx, delegate, _) = env.init_auth_matcher_context(matcher_program, &lp_owner, lp);

    let execute = |env: &mut V16CuEnv, size_q: i128| {
        if batch {
            env.send(
                env.batch_trade_cpi_ix(
                    taker,
                    lp,
                    vec![BatchTradeCpiLeg {
                        asset_index: 0,
                        market_id: env.asset_market_id(0),
                        size_q,
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
                    AccountMeta::new(ctx, false),
                    AccountMeta::new_readonly(delegate, false),
                ],
                &[&taker_owner],
            )
            .expect("batch matcher fill");
        } else {
            env.trade_cpi_with_cu_on_asset(
                &taker_owner,
                taker,
                &lp_owner,
                lp,
                matcher_program,
                ctx,
                delegate,
                0,
                size_q,
                0,
            );
        }
    };

    execute(&mut env, POS_SCALE as i128);
    assert_eq!(
        env.portfolio_matcher_config(taker).enabled(),
        0,
        "the taker's unrelated matcher did not participate and must be invalidated"
    );
    assert_eq!(
        env.portfolio_matcher_config(lp).enabled(),
        1,
        "the LP matcher remains synchronized after its own fill"
    );
    execute(&mut env, -(POS_SCALE as i128));
    assert_eq!(env.portfolio_matcher_config(lp).enabled(), 1);
    assert!(!has_active_leg_for_asset(&env.portfolio_state(lp), 0));
}

#[test]
fn v16_program_issue406_matcher_trade_routes_preserve_only_participating_lp_capability() {
    issue406_matcher_trade_preserves_only_participating_lp(false);
    issue406_matcher_trade_preserves_only_participating_lp(true);
}

#[test]
fn v16_program_issue406_liquidation_invalidates_stale_lp_matcher_capability() {
    const PRICE: u64 = 1_000_000;
    const MAX_INV_Q: u128 = 2 * POS_SCALE;
    const CROSS_REQUEST_Q: u128 = 2 * MAX_INV_Q;

    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.update_liquidation_fee_policy_with_cu(0);
    env.configure_auth_mark_with_cu(0, PRICE);

    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let attack_taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, 100_000_000);
    env.deposit(&taker_owner, attack_taker, 100_000_000);
    env.deposit(&lp_owner, lp, 200_000);

    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read official matcher SBF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let mut init = encode_matcher_init_passive(CROSS_REQUEST_Q);
    init[50..66].copy_from_slice(&MAX_INV_Q.to_le_bytes());
    let (ctx, delegate, _) =
        env.init_matcher_context_with_data_authorized(matcher_program, &lp_owner, lp, init);

    env.trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &lp_owner,
        lp,
        matcher_program,
        ctx,
        delegate,
        0,
        MAX_INV_Q as i128,
        0,
    );
    assert_eq!(
        issue406_matcher_inventory(&env.svm.get_account(&ctx).unwrap().data),
        -(MAX_INV_Q as i128)
    );
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(lp), 0).basis_pos_q,
        -(MAX_INV_Q as i128)
    );

    let before_liquidation_q = active_leg_for_asset(&env.portfolio_state(lp), 0)
        .basis_pos_q
        .unsigned_abs();
    for slot in 1..=30u64 {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_with_cu(slot, 1_060_000);
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(lp, false),
            ],
            &[],
        );
    }
    let after_liquidation_q = active_leg_for_asset(&env.portfolio_state(lp), 0)
        .basis_pos_q
        .unsigned_abs();
    assert!(
        after_liquidation_q < before_liquidation_q && after_liquidation_q != 0,
        "setup requires a public partial liquidation: before={before_liquidation_q}, after={after_liquidation_q}"
    );
    assert_eq!(
        issue406_matcher_inventory(&env.svm.get_account(&ctx).unwrap().data),
        -(MAX_INV_Q as i128),
        "permissionless liquidation cannot reconcile the external matcher inventory"
    );
    assert_eq!(
        env.portfolio_matcher_config(lp).enabled(),
        0,
        "an out-of-matcher liquidation must invalidate the stale LP capability"
    );

    env.deposit(&lp_owner, lp, 5_000_000);
    env.crank_steps(
        lp,
        ProgInstruction::PermissionlessCrank {
            now_slot: 30,
            observations: crank_observations(0),
        },
        4,
    );
    env.crank_steps(
        taker,
        ProgInstruction::PermissionlessCrank {
            now_slot: 30,
            observations: crank_observations(0),
        },
        4,
    );

    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&attack_taker).unwrap();
    let lp_before = env.svm.get_account(&lp).unwrap();
    let ctx_before = env.svm.get_account(&ctx).unwrap();
    env.svm.expire_blockhash();
    let stale_fill = env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        attack_taker,
        &lp_owner,
        lp,
        matcher_program,
        ctx,
        delegate,
        0,
        -(CROSS_REQUEST_Q as i128),
        0,
    );
    assert!(
        stale_fill.is_err(),
        "an unsigned taker must not use stale matcher inventory to exceed the LP's cap"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&attack_taker).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before);
    assert_eq!(env.svm.get_account(&ctx).unwrap(), ctx_before);

    // The successful pre-liquidation fill above is the positive capability control. A fresh LP
    // signature can deliberately restore this exact capability, but quantity ADL now blocks all
    // ordinary position mutations until the side is normalized. Do not use a post-ADL trade as
    // the capability-liveness oracle: that would require reopening the basis-reissue bug owned by
    // INV-050/INV-061.
    env.set_matcher_config(matcher_program, &lp_owner, lp, ctx, delegate, 1);
    assert_eq!(env.portfolio_matcher_config(lp).enabled(), 1);
    assert_eq!(
        issue406_matcher_inventory(&env.svm.get_account(&ctx).unwrap().data),
        -(MAX_INV_Q as i128),
        "reauthorization changes wrapper capability state, not external matcher inventory"
    );
}

#[test]
fn v16_program_issue406_force_close_and_reuse_require_fresh_lp_matcher_capability() {
    const ASSET: u16 = 1;
    const POSITION_Q: u128 = POS_SCALE;
    const SHUTDOWN_SLOT: u64 = 3;
    const FORCE_CLOSE_SLOT: u64 = 8;
    const REUSE_SLOT: u64 = 9;

    let mut env = V16CuEnv::new();
    let old_creator = Keypair::new();
    let new_insurance = Keypair::new();
    let new_operator = Keypair::new();
    let new_backing = Keypair::new();
    let new_oracle = Keypair::new();
    let cranker = Keypair::new();
    env.configure_permissionless_resolve_with_cu(100, FORCE_CLOSE_SLOT - SHUTDOWN_SLOT);
    env.update_market_init_fee_policy_with_cu(1);
    env.svm.warp_to_slot(1);
    env.activate_permissionless_asset_with_fee(
        &old_creator,
        ASSET,
        1,
        100,
        old_creator.pubkey(),
        old_creator.pubkey(),
        old_creator.pubkey(),
        old_creator.pubkey(),
        1,
    );
    env.configure_auth_mark_for_asset_with_authority(ASSET, &old_creator, 1, 100);

    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, 10_000);
    env.deposit(&lp_owner, lp, 10_000);
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read official matcher SBF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let mut init = encode_matcher_init_passive(POSITION_Q);
    init[50..66].copy_from_slice(&POSITION_Q.to_le_bytes());
    let (ctx, delegate, _) =
        env.init_matcher_context_with_data_authorized(matcher_program, &lp_owner, lp, init);
    env.trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &lp_owner,
        lp,
        matcher_program,
        ctx,
        delegate,
        ASSET,
        POSITION_Q as i128,
        0,
    );
    assert_eq!(env.portfolio_matcher_config(lp).enabled(), 1);
    let matcher_after_fill = env.svm.get_account(&ctx).unwrap();

    env.svm.warp_to_slot(SHUTDOWN_SLOT);
    env.update_asset_lifecycle_as_admin_with_cu(
        processor::ASSET_ACTION_SHUTDOWN,
        ASSET,
        SHUTDOWN_SLOT,
        0,
    );
    env.svm.warp_to_slot(FORCE_CLOSE_SLOT);
    env.force_close_abandoned_asset_with_cu(
        &cranker,
        taker,
        lp,
        ASSET,
        FORCE_CLOSE_SLOT,
        POSITION_Q,
    );
    assert!(!has_active_leg_for_asset(
        &env.portfolio_state(taker),
        ASSET as usize
    ));
    assert!(!has_active_leg_for_asset(
        &env.portfolio_state(lp),
        ASSET as usize
    ));
    assert_eq!(env.svm.get_account(&ctx).unwrap(), matcher_after_fill);
    assert_eq!(
        env.portfolio_matcher_config(lp).enabled(),
        0,
        "force-close must invalidate matcher inventory that it cannot update"
    );

    env.update_asset_lifecycle_as_admin_with_cu(
        processor::ASSET_ACTION_RETIRE,
        ASSET,
        FORCE_CLOSE_SLOT,
        0,
    );
    env.svm.warp_to_slot(REUSE_SLOT);
    env.activate_asset_with_authorities(
        ASSET,
        REUSE_SLOT,
        250,
        new_insurance.pubkey(),
        new_operator.pubkey(),
        new_backing.pubkey(),
        new_oracle.pubkey(),
    );

    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker).unwrap();
    let lp_before = env.svm.get_account(&lp).unwrap();
    let ctx_before = env.svm.get_account(&ctx).unwrap();
    env.svm.expire_blockhash();
    let stale_reuse_fill = env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &lp_owner,
        lp,
        matcher_program,
        ctx,
        delegate,
        ASSET,
        -(POSITION_Q as i128),
        0,
    );
    assert!(
        stale_reuse_fill.is_err(),
        "asset reuse must not revive a force-close-desynchronized matcher capability"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before);
    assert_eq!(env.svm.get_account(&ctx).unwrap(), ctx_before);

    env.set_matcher_config(matcher_program, &lp_owner, lp, ctx, delegate, 1);
    env.trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &lp_owner,
        lp,
        matcher_program,
        ctx,
        delegate,
        ASSET,
        -(POSITION_Q as i128),
        0,
    );
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(lp), ASSET as usize).basis_pos_q,
        POSITION_Q as i128,
        "a fresh LP signature restores the replacement-asset matcher route"
    );
}

#[test]
fn v16_program_disabled_lp_matcher_config_blocks_all_cpi_fills() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&lp_owner, lp, 1_000_000);
    let (ctx, delegate, _) = env.init_auth_matcher_context(matcher_program, &lp_owner, lp);

    env.set_matcher_config(matcher_program, &lp_owner, lp, ctx, delegate, 0);
    assert_eq!(env.portfolio_matcher_config(lp).enabled(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker).unwrap();
    let lp_before = env.svm.get_account(&lp).unwrap();
    let ctx_before = env.svm.get_account(&ctx).unwrap();

    env.svm.expire_blockhash();
    let single = env.send(
        env.trade_cpi_ix(taker, lp, 0, (5 * POS_SCALE) as i128, 100, 0),
        vec![
            AccountMeta::new(taker_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker, false),
            AccountMeta::new(lp, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ],
        &[&taker_owner],
    );
    assert!(single.is_err(), "disabled LP tuple must block TradeCpi");
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before);
    assert_eq!(env.svm.get_account(&ctx).unwrap(), ctx_before);

    env.svm.expire_blockhash();
    let batch = env.send(
        env.batch_trade_cpi_ix(
            taker,
            lp,
            vec![BatchTradeCpiLeg {
                asset_index: 0,
                market_id: first_generation_market_id(0),
                size_q: (5 * POS_SCALE) as i128,
                fee_bps: 100,
                limit_price: 0,
            }],
        ),
        vec![
            AccountMeta::new(taker_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker, false),
            AccountMeta::new(lp, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ],
        &[&taker_owner],
    );
    assert!(batch.is_err(), "disabled LP tuple must block BatchTradeCpi");
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before);
    assert_eq!(env.svm.get_account(&ctx).unwrap(), ctx_before);

    env.set_matcher_config(matcher_program, &lp_owner, lp, ctx, delegate, 1);
    env.svm.expire_blockhash();
    env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &lp_owner,
        lp,
        matcher_program,
        ctx,
        delegate,
        0,
        (5 * POS_SCALE) as i128,
        100,
    )
    .expect("LP owner can re-enable the exact matcher tuple");
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(taker), 0).basis_pos_q,
        (5 * POS_SCALE) as i128
    );
}

#[test]
fn v16_program_non_owner_cannot_revoke_lp_matcher_capability() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let attacker = Keypair::new();
    env.ensure_signer_account(attacker.pubkey());
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&lp_owner, lp, 1_000_000);
    let (ctx, delegate, _) = env.init_auth_matcher_context(matcher_program, &lp_owner, lp);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let lp_before = env.svm.get_account(&lp).unwrap();
    let ctx_before = env.svm.get_account(&ctx).unwrap();
    let portfolio_id = env.portfolio_id(lp);
    let expected_sequence = env.portfolio_matcher_sequence(lp);

    env.svm.expire_blockhash();
    let revoke = env.send(
        ProgInstruction::SetMatcherConfig {
            portfolio_id,
            expected_sequence,
            enabled: 0,
            trade_fee_cap_bps: 0,
        },
        vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new_readonly(env.market, false),
            AccountMeta::new(lp, false),
        ],
        &[&attacker],
    );
    assert!(
        revoke.is_err(),
        "a non-owner signer must not change an LP matcher capability"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before);
    assert_eq!(env.svm.get_account(&ctx).unwrap(), ctx_before);
    assert_eq!(env.portfolio_matcher_config(lp).enabled(), 1);

    env.svm.expire_blockhash();
    env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &lp_owner,
        lp,
        matcher_program,
        ctx,
        delegate,
        0,
        (5 * POS_SCALE) as i128,
        100,
    )
    .expect("failed non-owner revoke must not DoS authorized matcher fills");
}

#[test]
fn v16_program_tradecpi_requires_exact_lp_authorized_matcher_tuple() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    let honest = Pubkey::new_unique();
    let auth_matcher_bytes =
        std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(honest, &auth_matcher_bytes);
    let hostile = Pubkey::new_unique();
    env.svm.add_program(
        hostile,
        &std::fs::read(hostile_matcher_program_path()).unwrap(),
    );
    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&lp_owner, lp, 1_000_000);

    let (honest_ctx, honest_delegate, _) = env.init_auth_matcher_context(honest, &lp_owner, lp);
    let hostile_ctx = Pubkey::new_unique();
    let hostile_delegate = matcher_delegate_key(
        &env.program_id,
        &env.market,
        &lp,
        &lp_owner.pubkey(),
        &hostile,
        &hostile_ctx,
    );
    env.svm
        .set_account(
            hostile_delegate,
            Account {
                lamports: 1_000_000_000,
                data: vec![],
                owner: Pubkey::default(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let mut hostile_ctx_data = vec![0u8; MATCHER_CONTEXT_LEN];
    hostile_ctx_data[0] = 9;
    env.svm
        .set_account(
            hostile_ctx,
            Account {
                lamports: 1_000_000_000,
                data: hostile_ctx_data,
                owner: hostile,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let send = |env: &mut V16CuEnv,
                matcher_program: Pubkey,
                matcher_context: Pubkey,
                matcher_delegate: Pubkey| {
        env.svm.expire_blockhash();
        env.send(
            env.trade_cpi_ix(taker, lp, 0, (5 * POS_SCALE) as i128, 100, 0),
            vec![
                AccountMeta::new(taker_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker, false),
                AccountMeta::new(lp, false),
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new(matcher_context, false),
                AccountMeta::new_readonly(matcher_delegate, false),
            ],
            &[&taker_owner],
        )
    };

    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker).unwrap();
    let lp_before = env.svm.get_account(&lp).unwrap();
    let ctx_before = env.svm.get_account(&hostile_ctx).unwrap();
    let replay = send(&mut env, hostile, hostile_ctx, hostile_delegate);
    assert!(
        replay.is_err(),
        "LP capability for one tuple must not authorize different TradeCpi args"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before);
    assert_eq!(env.svm.get_account(&hostile_ctx).unwrap(), ctx_before);
    assert_eq!(env.market_state().1.assets[0].oi_eff_long_q, 0);
    assert_eq!(env.portfolio_state(taker).legs[0].basis_pos_q.get(), 0);
    assert_eq!(env.portfolio_state(lp).legs[0].basis_pos_q.get(), 0);

    send(&mut env, honest, honest_ctx, honest_delegate)
        .expect("the exact LP-authorized matcher tuple still fills");
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(taker), 0).basis_pos_q,
        (5 * POS_SCALE) as i128
    );
}

#[test]
fn v16_program_batch_tradecpi_requires_exact_lp_authorized_matcher_tuple() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    let honest = Pubkey::new_unique();
    let auth_matcher_bytes =
        std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(honest, &auth_matcher_bytes);
    let hostile = Pubkey::new_unique();
    env.svm.add_program(
        hostile,
        &std::fs::read(hostile_matcher_program_path()).unwrap(),
    );
    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&lp_owner, lp, 1_000_000);

    let (honest_ctx, honest_delegate, _) = env.init_auth_matcher_context(honest, &lp_owner, lp);
    let hostile_ctx = Pubkey::new_unique();
    let hostile_delegate = matcher_delegate_key(
        &env.program_id,
        &env.market,
        &lp,
        &lp_owner.pubkey(),
        &hostile,
        &hostile_ctx,
    );
    env.svm
        .set_account(
            hostile_delegate,
            Account {
                lamports: 1_000_000_000,
                data: vec![],
                owner: Pubkey::default(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let mut hostile_ctx_data = vec![0u8; MATCHER_CONTEXT_LEN];
    hostile_ctx_data[0] = 9;
    env.svm
        .set_account(
            hostile_ctx,
            Account {
                lamports: 1_000_000_000,
                data: hostile_ctx_data,
                owner: hostile,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker).unwrap();
    let lp_before = env.svm.get_account(&lp).unwrap();
    let ctx_before = env.svm.get_account(&hostile_ctx).unwrap();
    let sz = (5 * POS_SCALE) as i128;
    env.svm.expire_blockhash();
    let rejected = env.send(
        env.batch_trade_cpi_ix(
            taker,
            lp,
            vec![
                BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: first_generation_market_id(0),
                    size_q: sz,
                    fee_bps: 100,
                    limit_price: 0,
                },
                BatchTradeCpiLeg {
                    asset_index: 1,
                    market_id: first_generation_market_id(1),
                    size_q: -sz,
                    fee_bps: 100,
                    limit_price: 0,
                },
            ],
        ),
        vec![
            AccountMeta::new(taker_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker, false),
            AccountMeta::new(lp, false),
            AccountMeta::new_readonly(hostile, false),
            AccountMeta::new(hostile_ctx, false),
            AccountMeta::new_readonly(hostile_delegate, false),
        ],
        &[&taker_owner],
    );
    assert!(
        rejected.is_err(),
        "LP capability for one tuple must not authorize different BatchTradeCpi args"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before);
    assert_eq!(env.svm.get_account(&hostile_ctx).unwrap(), ctx_before);
    assert_eq!(env.market_state().1.assets[0].oi_eff_long_q, 0);
    assert_eq!(env.market_state().1.assets[1].oi_eff_long_q, 0);

    env.svm.expire_blockhash();
    env.send(
        env.batch_trade_cpi_ix(
            taker,
            lp,
            vec![BatchTradeCpiLeg {
                asset_index: 0,
                market_id: first_generation_market_id(0),
                size_q: sz,
                fee_bps: 100,
                limit_price: 0,
            }],
        ),
        vec![
            AccountMeta::new(taker_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker, false),
            AccountMeta::new(lp, false),
            AccountMeta::new_readonly(honest, false),
            AccountMeta::new(honest_ctx, false),
            AccountMeta::new_readonly(honest_delegate, false),
        ],
        &[&taker_owner],
    )
    .expect("the exact LP-configured matcher tuple still batch-fills");
}

#[test]
fn v16_attack_cross_lp_cannot_overwrite_lp_matcher_config() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new();
    let victim_owner = Keypair::new();
    let attacker_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let victim_lp = env.create_portfolio(&victim_owner);
    let attacker_lp = env.create_portfolio(&attacker_owner);
    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&victim_owner, victim_lp, 1_000_000);
    env.deposit(&attacker_owner, attacker_lp, 1_000_000);
    let (victim_ctx, victim_delegate, _) =
        env.init_auth_matcher_context(matcher_program, &victim_owner, victim_lp);
    let (attacker_ctx, attacker_delegate, _) =
        env.init_auth_matcher_context(matcher_program, &attacker_owner, attacker_lp);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let victim_before = env.svm.get_account(&victim_lp).unwrap();
    let attacker_before = env.svm.get_account(&attacker_lp).unwrap();
    let portfolio_id = env.portfolio_id(victim_lp);
    let expected_sequence = env.portfolio_matcher_sequence(victim_lp);

    env.svm.expire_blockhash();
    let overwrite = env.send(
        ProgInstruction::SetMatcherConfig {
            portfolio_id,
            expected_sequence,
            enabled: 0,
            trade_fee_cap_bps: 0,
        },
        vec![
            AccountMeta::new(attacker_owner.pubkey(), true),
            AccountMeta::new_readonly(env.market, false),
            AccountMeta::new(victim_lp, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new_readonly(attacker_ctx, false),
            AccountMeta::new_readonly(attacker_delegate, false),
        ],
        &[&attacker_owner],
    );
    assert!(
        overwrite.is_err(),
        "one LP must not overwrite another LP's matcher config with its own tuple"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&victim_lp).unwrap(), victim_before);
    assert_eq!(env.svm.get_account(&attacker_lp).unwrap(), attacker_before);

    env.svm.expire_blockhash();
    let victim_fill = env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &victim_owner,
        victim_lp,
        matcher_program,
        victim_ctx,
        victim_delegate,
        0,
        (5 * POS_SCALE) as i128,
        100,
    );
    assert!(
        victim_fill.is_ok(),
        "failed cross-LP overwrite must not DoS the victim LP's authorized matcher fills: {victim_fill:?}"
    );
}

#[test]
fn v16_attack_set_lp_matcher_config_cannot_target_protocol_accounts() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let lp_owner = Keypair::new();
    let lp = env.create_portfolio(&lp_owner);
    let ctx = Pubkey::new_unique();
    let delegate = matcher_delegate_key(
        &env.program_id,
        &env.market,
        &lp,
        &lp_owner.pubkey(),
        &matcher_program,
        &ctx,
    );
    env.try_init_auth_matcher_context_with_delegate(matcher_program, &lp_owner, lp, ctx, delegate)
        .expect("init auth matcher context without setting percolator auth");
    let portfolio_id = env.portfolio_id(lp);
    let expected_sequence = env.portfolio_matcher_sequence(lp);

    let send_with_lp_account = |env: &mut V16CuEnv, lp_account: Pubkey| {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::SetMatcherConfig {
                portfolio_id,
                expected_sequence,
                enabled: 1,
                trade_fee_cap_bps: 10_000,
            },
            vec![
                AccountMeta::new(lp_owner.pubkey(), true),
                AccountMeta::new_readonly(env.market, false),
                AccountMeta::new(lp_account, false),
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new_readonly(ctx, false),
                AccountMeta::new_readonly(delegate, false),
            ],
            &[&lp_owner],
        )
    };

    let market = env.market;
    let market_before = env.svm.get_account(&market).unwrap();
    let lp_before = env.svm.get_account(&lp).unwrap();
    let market_alias = send_with_lp_account(&mut env, market);
    assert!(
        market_alias.is_err(),
        "SetMatcherConfig must not treat the market as an LP account"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before);

    env.set_matcher_config(matcher_program, &lp_owner, lp, ctx, delegate, 1);
    let auth_state = env.portfolio_matcher_config(lp);
    assert_eq!(
        auth_state.enabled(),
        1,
        "a real LP account stores the matcher program/context config"
    );
    assert_eq!(auth_state.matcher_program, matcher_program.to_bytes());
    assert_eq!(auth_state.matcher_context, ctx.to_bytes());
    assert_eq!(auth_state.matcher_delegate, delegate.to_bytes());
}

#[test]
fn v16_attack_permissionless_lp_cpi_rejects_wrong_delegate_owner_or_account_binding() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let wrong_lp_owner = Keypair::new();
    env.ensure_signer_account(wrong_lp_owner.pubkey());
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    let other_lp_same_owner = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&lp_owner, lp, 1_000_000);
    env.deposit(&lp_owner, other_lp_same_owner, 1_000_000);
    let (ctx, _delegate, _) = env.init_auth_matcher_context(matcher_program, &lp_owner, lp);
    let (other_ctx, other_delegate, _) =
        env.init_auth_matcher_context(matcher_program, &lp_owner, other_lp_same_owner);
    let wrong_owner_delegate = matcher_delegate_key(
        &env.program_id,
        &env.market,
        &lp,
        &wrong_lp_owner.pubkey(),
        &matcher_program,
        &ctx,
    );
    env.svm
        .set_account(
            wrong_owner_delegate,
            Account {
                lamports: 1_000_000_000,
                data: vec![],
                owner: Pubkey::default(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let sz = (5 * POS_SCALE) as i128;

    env.svm.expire_blockhash();
    let wrong_delegate_single = env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &wrong_lp_owner,
        lp,
        matcher_program,
        ctx,
        wrong_owner_delegate,
        0,
        sz,
        100,
    );
    assert!(
        wrong_delegate_single.is_err(),
        "single TradeCpi must reject a delegate derived from the wrong LP owner"
    );

    env.svm.expire_blockhash();
    let wrong_account_single = env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &lp_owner,
        lp,
        matcher_program,
        other_ctx,
        other_delegate,
        0,
        sz,
        100,
    );
    assert!(
        wrong_account_single.is_err(),
        "single TradeCpi must reject a delegate/context bound to a different LP portfolio"
    );

    let batch_ix = env.batch_trade_cpi_ix(
        taker,
        lp,
        vec![BatchTradeCpiLeg {
            asset_index: 0,
            market_id: first_generation_market_id((0) as u16),
            size_q: sz,
            fee_bps: 100,
            limit_price: 0,
        }],
    );
    let taker_owner_key = taker_owner.pubkey();
    let market = env.market;
    let metas = |context: Pubkey, del: Pubkey| {
        vec![
            AccountMeta::new(taker_owner_key, true),
            AccountMeta::new(market, false),
            AccountMeta::new(taker, false),
            AccountMeta::new(lp, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new(context, false),
            AccountMeta::new_readonly(del, false),
        ]
    };

    env.svm.expire_blockhash();
    let wrong_delegate_batch = env.send(
        batch_ix.clone(),
        metas(ctx, wrong_owner_delegate),
        &[&taker_owner],
    );
    assert!(
        wrong_delegate_batch.is_err(),
        "BatchTradeCpi must reject a delegate derived from the wrong LP owner"
    );

    env.svm.expire_blockhash();
    let wrong_account_batch = env.send(batch_ix, metas(other_ctx, other_delegate), &[&taker_owner]);
    assert!(
        wrong_account_batch.is_err(),
        "BatchTradeCpi must reject a delegate/context bound to a different LP portfolio"
    );

    let group = env.market_state().1;
    assert_eq!(
        group.assets[0].oi_eff_long_q, 0,
        "no OI created by rejected CPI attempts"
    );
    assert_eq!(
        env.portfolio_state(taker).legs[0].basis_pos_q.get(),
        0,
        "taker untouched"
    );
    assert_eq!(
        env.portfolio_state(lp).legs[0].basis_pos_q.get(),
        0,
        "LP untouched"
    );
}

#[test]
fn v16_attack_nocpi_trades_still_require_lp_owner_signature() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&lp_owner, lp, 1_000_000);

    env.svm.expire_blockhash();
    let single = env.send(
        env.trade_no_cpi_ix(taker, lp, 0, (10 * POS_SCALE) as i128, 100, 0),
        vec![
            AccountMeta::new(taker_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker, false),
            AccountMeta::new(lp, false),
        ],
        &[&taker_owner],
    );
    assert!(
        single.is_err(),
        "TradeNoCpi without the LP owner signature must reject"
    );

    env.svm.expire_blockhash();
    let batch = env.send(
        env.batch_trade_no_cpi_ix(
            taker,
            lp,
            vec![
                BatchTradeLeg {
                    asset_index: 0,
                    market_id: first_generation_market_id((0) as u16),
                    size_q: (5 * POS_SCALE) as i128,
                    exec_price: 100,
                    fee_bps: 0,
                },
                BatchTradeLeg {
                    asset_index: 1,
                    market_id: first_generation_market_id((1) as u16),
                    size_q: -(5 * POS_SCALE as i128),
                    exec_price: 100,
                    fee_bps: 0,
                },
            ],
        ),
        vec![
            AccountMeta::new(taker_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker, false),
            AccountMeta::new(lp, false),
        ],
        &[&taker_owner],
    );
    assert!(
        batch.is_err(),
        "BatchTradeNoCpi without the LP owner signature must reject"
    );

    let group = env.market_state().1;
    assert_eq!(group.assets[0].oi_eff_long_q, 0);
    assert_eq!(group.assets[1].oi_eff_long_q, 0);
    assert_eq!(env.portfolio_state(taker).legs[0].basis_pos_q.get(), 0);
    assert_eq!(env.portfolio_state(lp).legs[0].basis_pos_q.get(), 0);
}

// full-interface sweep / issue: removing the LP signer from TradeCpi is only safe if Percolator
// verifies that the LP owner explicitly authorized this matcher program/context. A hostile matcher can
// otherwise return a perfectly well-formed oracle-priced fill and force a victim LP portfolio into a
// position. This is the single-fill reproducer: no LP signature and no Percolator matcher config
// must reject before any position is opened.
#[test]
fn v16_attack_tradecpi_rejects_unapproved_unsigned_lp_matcher() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    let hostile = Pubkey::new_unique();
    env.svm.add_program(
        hostile,
        &std::fs::read(hostile_matcher_program_path()).unwrap(),
    );
    let taker = Keypair::new();
    let lp = Keypair::new();
    let ta = env.create_portfolio(&taker);
    let la = env.create_portfolio(&lp);
    env.deposit(&taker, ta, 1_000_000);
    env.deposit(&lp, la, 1_000_000);

    let ctx = Pubkey::new_unique();
    let delegate = matcher_delegate_key(
        &env.program_id,
        &env.market,
        &la,
        &lp.pubkey(),
        &hostile,
        &ctx,
    );
    env.svm
        .set_account(
            delegate,
            Account {
                lamports: 1_000_000_000,
                data: vec![],
                owner: Pubkey::default(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let mut ctx_data = vec![0u8; MATCHER_CONTEXT_LEN];
    ctx_data[0] = 9; // hostile fixture faithful mode: returns a valid fill.
    env.svm
        .set_account(
            ctx,
            Account {
                lamports: 1_000_000_000,
                data: ctx_data,
                owner: hostile,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&ta).unwrap();
    let lp_before = env.svm.get_account(&la).unwrap();
    let ctx_before = env.svm.get_account(&ctx).unwrap();

    env.svm.expire_blockhash();
    let r = env.send(
        env.trade_cpi_ix(ta, la, 0, (5 * POS_SCALE) as i128, 100, 0),
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ta, false),
            AccountMeta::new(la, false),
            AccountMeta::new_readonly(hostile, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ],
        &[&taker],
    );
    assert!(
        r.is_err(),
        "unauthorized matcher must not be able to force an unsigned LP TradeCpi fill"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&ta).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&la).unwrap(), lp_before);
    assert_eq!(env.svm.get_account(&ctx).unwrap(), ctx_before);
    assert!(!has_active_leg_for_asset(
        &state::read_portfolio(&env.svm.get_account(&la).unwrap().data).unwrap(),
        0
    ));
}

// Same config boundary for the batched matcher CPI path. A hostile matcher that emits valid
// return-data for every leg is still unapproved for the LP unless the LP stored that matcher tuple
// on its portfolio.
#[test]
fn v16_attack_batch_tradecpi_rejects_unapproved_unsigned_lp_matcher() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
    let hostile = Pubkey::new_unique();
    env.svm.add_program(
        hostile,
        &std::fs::read(hostile_matcher_program_path()).unwrap(),
    );
    let taker = Keypair::new();
    let lp = Keypair::new();
    let ta = env.create_portfolio(&taker);
    let la = env.create_portfolio(&lp);
    env.deposit(&taker, ta, 1_000_000);
    env.deposit(&lp, la, 1_000_000);

    let ctx = Pubkey::new_unique();
    let delegate = matcher_delegate_key(
        &env.program_id,
        &env.market,
        &la,
        &lp.pubkey(),
        &hostile,
        &ctx,
    );
    env.svm
        .set_account(
            delegate,
            Account {
                lamports: 1_000_000_000,
                data: vec![],
                owner: Pubkey::default(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let mut ctx_data = vec![0u8; MATCHER_CONTEXT_LEN];
    ctx_data[0] = 9; // faithful batch replies.
    env.svm
        .set_account(
            ctx,
            Account {
                lamports: 1_000_000_000,
                data: ctx_data,
                owner: hostile,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&ta).unwrap();
    let lp_before = env.svm.get_account(&la).unwrap();
    let ctx_before = env.svm.get_account(&ctx).unwrap();
    let sz = (5 * POS_SCALE) as i128;

    env.svm.expire_blockhash();
    let r = env.send(
        env.batch_trade_cpi_ix(
            ta,
            la,
            vec![
                BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: first_generation_market_id((0) as u16),
                    size_q: sz,
                    fee_bps: 100,
                    limit_price: 0,
                },
                BatchTradeCpiLeg {
                    asset_index: 1,
                    market_id: first_generation_market_id((1) as u16),
                    size_q: -sz,
                    fee_bps: 100,
                    limit_price: 0,
                },
            ],
        ),
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ta, false),
            AccountMeta::new(la, false),
            AccountMeta::new_readonly(hostile, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ],
        &[&taker],
    );
    assert!(
        r.is_err(),
        "unauthorized matcher must not be able to force an unsigned LP BatchTradeCpi fill"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&ta).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&la).unwrap(), lp_before);
    assert_eq!(env.svm.get_account(&ctx).unwrap(), ctx_before);
    let lp_state = state::read_portfolio(&env.svm.get_account(&la).unwrap().data).unwrap();
    assert!(!has_active_leg_for_asset(&lp_state, 0));
    assert!(!has_active_leg_for_asset(&lp_state, 1));
}

#[test]
fn v16_bpf_tradecpi_permissionless_lp_fill_does_not_need_lp_owner_signature() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&lp_owner, lp, 1_000_000);
    let (matcher_ctx, matcher_delegate, init_cu) =
        env.init_auth_matcher_context_via_system_create(matcher_program, &lp_owner, lp);

    env.svm.expire_blockhash();
    let cu = env
        .try_trade_cpi_with_cu_on_asset(
            &taker_owner,
            taker,
            &lp_owner,
            lp,
            matcher_program,
            matcher_ctx,
            matcher_delegate,
            0,
            (10 * POS_SCALE) as i128,
            100,
        )
        .expect("matcher CPI fill succeeds with only the taker signing");
    println!("v16 permissionless LP matcher system-init CU: {init_cu}, TradeCpi CU: {cu}");

    let taker_state = env.portfolio_state(taker);
    let lp_state = env.portfolio_state(lp);
    assert_eq!(active_leg_for_asset(&taker_state, 0).side, SideV16::Long);
    assert_eq!(active_leg_for_asset(&lp_state, 0).side, SideV16::Short);
    assert_eq!(
        active_leg_for_asset(&taker_state, 0).basis_pos_q,
        (10 * POS_SCALE) as i128
    );
}
