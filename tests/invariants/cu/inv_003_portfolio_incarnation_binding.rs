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
//! attempts do not consume an incarnation. A source-bound completeness roster
//! additionally proves that every instruction field carrying a portfolio ID is
//! forwarded by dispatch and consumed by a production incarnation guard.

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

fn source_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start = source
        .find(start)
        .unwrap_or_else(|| panic!("missing source boundary {start:?}"));
    let tail = &source[start..];
    let end = tail
        .find(end)
        .unwrap_or_else(|| panic!("missing source boundary {end:?}"));
    &tail[..end]
}

fn handler_source<'a>(source: &'a str, name: &str) -> &'a str {
    let marker = format!("\n    fn handle_{name}");
    let start = source
        .find(&marker)
        .unwrap_or_else(|| panic!("missing production handler {name}"));
    let tail = &source[start + 1..];
    let end = tail.find("\n    fn handle_").unwrap_or(tail.len());
    &tail[..end]
}

fn assert_dispatch_forwards(
    dispatch: &str,
    variant: &str,
    handler: &str,
    portfolio_fields: &[&str],
) {
    let marker = format!("Instruction::{variant}");
    let start = dispatch
        .find(&marker)
        .unwrap_or_else(|| panic!("{variant}: missing production dispatch arm"));
    let tail = &dispatch[start..];
    let end = tail
        .find("\n            Instruction::")
        .unwrap_or(tail.len());
    let arm = &tail[..end];
    assert!(
        arm.contains(&format!("handle_{handler}(")),
        "{variant}: dispatch does not call handle_{handler}"
    );
    for field in portfolio_fields {
        assert!(
            arm.matches(field).count() >= 2,
            "{variant}: {field} must occur in both the decoded pattern and handler arguments"
        );
    }
}

#[test]
fn v16_program_retained_portfolio_binding_roster_is_source_complete() {
    let source = include_str!("../../../src/v16_program.rs");
    let instruction_enum =
        source_between(source, "pub enum Instruction {", "\n    impl Instruction {");
    let dispatch = source_between(
        source,
        "pub fn process_instruction<'a>(",
        "\n    #[inline(never)]\n    fn handle_init_market",
    );

    // Eight single-account requests and four two-account trade requests are the complete set of
    // encoded portfolio-incarnation fields. A new field-bearing instruction changes this count and
    // must be assigned to INV-003 before CI can pass.
    assert_eq!(
        instruction_enum.matches("portfolio_id: u64").count(),
        16,
        "portfolio-ID-bearing instruction roster changed without INV-003 review"
    );

    let routes: [(&str, &str, &[&str]); 12] = [
        ("Deposit", "deposit", &["portfolio_id"]),
        ("Withdraw", "withdraw", &["portfolio_id"]),
        (
            "TradeNoCpi",
            "trade_nocpi",
            &["account_a_portfolio_id", "account_b_portfolio_id"],
        ),
        (
            "TradeCpi",
            "trade_cpi",
            &["account_a_portfolio_id", "account_b_portfolio_id"],
        ),
        (
            "BatchTradeNoCpi",
            "batch_trade_nocpi",
            &["account_a_portfolio_id", "account_b_portfolio_id"],
        ),
        (
            "BatchTradeCpi",
            "batch_trade_cpi",
            &["account_a_portfolio_id", "account_b_portfolio_id"],
        ),
        ("SetMatcherConfig", "set_matcher_config", &["portfolio_id"]),
        ("ClosePortfolio", "close_portfolio", &["portfolio_id"]),
        (
            "ConvertReleasedPnl",
            "convert_released_pnl",
            &["portfolio_id"],
        ),
        (
            "CureAndCancelClose",
            "cure_and_cancel_close",
            &["portfolio_id"],
        ),
        (
            "ForfeitRecoveryLeg",
            "forfeit_recovery_leg",
            &["portfolio_id"],
        ),
        ("RebalanceReduce", "rebalance_reduce", &["portfolio_id"]),
    ];
    for (variant, handler, fields) in routes {
        let variant_marker = format!("{variant} {{");
        assert!(
            instruction_enum.contains(&variant_marker),
            "{variant}: missing from the portfolio-ID instruction roster"
        );
        assert_dispatch_forwards(dispatch, variant, handler, fields);
    }

    let deposit = handler_source(source, "deposit");
    let withdraw = handler_source(source, "withdraw");
    for (name, handler) in [("Deposit", deposit), ("Withdraw", withdraw)] {
        assert!(
            handler.contains("expect_portfolio_id(&portfolio_data, expected_portfolio_id)?;"),
            "{name}: decoded portfolio ID is not consumed before mutation"
        );
    }

    let trade_core = handler_source(source, "trade_nocpi_zero_copy");
    let batch_core = handler_source(source, "batch_execute_zero_copy");
    for (name, handler) in [("single trade", trade_core), ("batch trade", batch_core)] {
        assert!(
            handler.contains("expect_portfolio_id(&account_a_data, account_a_portfolio_id)?;")
                && handler
                    .contains("expect_portfolio_id(&account_b_data, account_b_portfolio_id)?;"),
            "{name}: both portfolio incarnations must be consumed by the shared mutation core"
        );
    }
    for name in ["trade_cpi", "batch_trade_cpi"] {
        let handler = handler_source(source, name);
        assert!(
            handler.contains("expect_portfolio_id(&data, account_a_portfolio_id)?;")
                && handler.contains("expect_portfolio_id(&data, account_b_portfolio_id)?;"),
            "{name}: both IDs must be checked before invoking the external matcher"
        );
    }

    let matcher = handler_source(source, "set_matcher_config");
    assert!(
        matcher.contains("portfolio_id != current_portfolio_id"),
        "SetMatcherConfig: current incarnation is not compared with signed consent"
    );
    let close = handler_source(source, "close_portfolio");
    assert!(
        close.contains("state::portfolio_close_binding_matches(")
            && close.contains("expected_portfolio_id,"),
        "ClosePortfolio: the composite close binding omits portfolio incarnation"
    );
    let convert = handler_source(source, "convert_released_pnl");
    assert!(
        convert.contains("Some((expected_portfolio_id, expected_position_epoch))"),
        "ConvertReleasedPnl: portfolio and position bindings are not composed"
    );
    for name in ["forfeit_recovery_leg", "rebalance_reduce"] {
        let handler = handler_source(source, name);
        assert!(
            handler.contains("Some((expected_portfolio_id, expected_position_epoch))"),
            "{name}: portfolio and position incarnations are not composed"
        );
    }

    let cure = handler_source(source, "cure_and_cancel_close");
    assert!(cure.contains("expect_signer(owner)?;"));
    assert!(
        cure.contains("state::portfolio_position_binding_matches(")
            && cure.contains("expected_portfolio_id")
            && cure.contains("expected_position_epoch"),
        "CureAndCancelClose: current incarnation and episode are not checked before mutation"
    );
}
