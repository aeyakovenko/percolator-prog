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
