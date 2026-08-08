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
//! permissionless crank.

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
