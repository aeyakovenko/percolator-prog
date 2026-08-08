//! INV-003 - Portfolio incarnation binding.
//!
//! Normative obligation: a retained portfolio-specific request must bind the
//! program-assigned portfolio incarnation, not only the portfolio pubkey. Closing
//! and recreating a portfolio at the same address must not revive prior consent
//! or accounting identity.
//!
//! Evidence in this file (I): public LiteSVM lifecycle coverage for portfolio
//! close/reinit. The test creates, closes, and recreates through public routes and
//! asserts the wrapper assigns a new monotonic `portfolio_id` while failed reinit
//! attempts do not consume an incarnation.

use super::*;

// security.md sweep — account reuse / sentinel re-materialization (#44/#48): after ClosePortfolio,
// reusing the SAME account address (re-init) must yield a CLEAN portfolio — no stale capital, pnl,
// A reward distributor snapshots portfolio-local monotonic counters. The account pubkey alone is
// not a stable identity because ClosePortfolio permits that address to be initialized again. Every
// successful incarnation must receive a new market-assigned ID, while failed initialization and
// unrelated wrapper-tail updates must not consume or rewrite IDs.
#[test]
fn v16_portfolio_incarnation_id_separates_close_and_reuse() {
    let mut env = V16CuEnv::new();
    assert_eq!(
        env.market_state().0.next_portfolio_id,
        1,
        "a new market starts the program-owned portfolio sequence at one"
    );

    let first_owner = Keypair::new();
    let first_account = Keypair::new();
    let first = first_account.pubkey();
    env.ensure_signer_account(first_owner.pubkey());
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &first_account,
        env.portfolio_account_len,
        env.program_id,
    );
    env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(first_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(first, false),
        ],
        &[&first_owner],
    )
    .expect("initialize first portfolio through the public instruction");
    let first_id = env.portfolio_id(first);
    assert_eq!(first_id, 1);
    assert_eq!(env.market_state().0.next_portfolio_id, 2);

    let second_owner = Keypair::new();
    let second = env.create_portfolio(&second_owner);
    assert_eq!(env.portfolio_id(second), 2);
    assert_eq!(env.market_state().0.next_portfolio_id, 3);

    // SetMatcherConfig owns the adjacent wrapper tail. It must not overwrite portfolio identity.
    env.set_matcher_config(
        Pubkey::default(),
        &first_owner,
        first,
        Pubkey::default(),
        Pubkey::default(),
        0,
    );
    assert_eq!(env.portfolio_id(first), first_id);

    // Reinitializing a live account rejects atomically and does not burn an ID.
    let attacker = Keypair::new();
    env.ensure_signer_account(attacker.pubkey());
    let market_before_rejected_init = env.svm.get_account(&env.market).unwrap();
    let first_before_rejected_init = env.svm.get_account(&first).unwrap();
    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(first, false),
        ],
        &[&attacker],
    );
    assert!(rejected.is_err(), "a live portfolio cannot be reincarnated");
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_rejected_init,
        "a rejected initialization does not consume a portfolio ID"
    );
    assert_eq!(
        env.svm.get_account(&first).unwrap(),
        first_before_rejected_init,
        "a rejected initialization leaves the live incarnation unchanged"
    );

    env.close_portfolio_with_cu(&first_owner, first);
    assert_eq!(
        env.market_state().0.next_portfolio_id,
        3,
        "closing a portfolio does not rewind or advance the sequence"
    );
    // Re-fund the exact same address through the System Program, then initialize it through the
    // public Percolator instruction. LiteSVM retains the closed account's program owner while its
    // lamports and data are zero, so a transfer models its next incarnation without state injection.
    env.svm.expire_blockhash();
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        system_instruction::transfer(&env.payer.pubkey(), &first, 1_000_000_000),
        &[],
    )
    .expect("re-fund closed portfolio through the System Program");
    let replacement_owner = Keypair::new();
    env.ensure_signer_account(replacement_owner.pubkey());
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(replacement_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(first, false),
        ],
        &[&replacement_owner],
    )
    .expect("reinitialize closed portfolio address");

    let replacement_id = env.portfolio_id(first);
    assert_eq!(replacement_id, 3);
    assert_ne!(
        replacement_id, first_id,
        "a stale (market, portfolio, portfolio_id) snapshot cannot name the replacement account"
    );
    assert_eq!(env.market_state().0.next_portfolio_id, 4);
    let replacement = env.portfolio_state(first);
    assert_eq!(replacement.residual_crystallized_loss_atoms_total.get(), 0);
    assert_eq!(replacement.residual_spent_principal_atoms_total.get(), 0);
    assert_eq!(replacement.residual_received_atoms_total.get(), 0);
}
