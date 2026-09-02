//! INV-013 - Destructive-consent scope.
//!
//! Normative obligation: close, forfeit, recovery, liquidation, and reduction
//! consent must only apply to the exact market/asset/portfolio/position/claim
//! episode it was signed for. A later episode at the same visible pubkeys must
//! reject stale destructive authority before any mutation.
//!
//! Evidence in this file (I/C): the reduction route is exercised through the
//! deployed LiteSVM wrapper only. The test opens exposure, records the owner's
//! old position episode, closes and reopens through public trades, then submits
//! the old `RebalanceReduce` request. The stale request must reject with exact
//! market, portfolio, counterparty, and custody rollback; a current-episode
//! request must still reduce exposure, proving this is not a blanket user-exit
//! DoS.
//!
//! Guarantee boundary: this is the public SVM owner for reduction destructive consent.
//! INV-004's finding-blind episode matrix additionally owns Recovery forfeit, released-PnL
//! conversion, and close/cure episodes. INV-002 owns asset-generation shutdown/resolve scope;
//! INV-005 owns every configured-authority incarnation, and INV-001/007 permanently retire a
//! closed market address. Permissionless liquidation, abandoned-asset close, reset finalization,
//! and terminal payout derive their action from current state and carry no retained user consent.

use super::*;

fn inv013_braced_body<'a>(source: &'a str, marker: &str) -> &'a str {
    let start = source
        .find(marker)
        .unwrap_or_else(|| panic!("missing source marker {marker}"));
    let open = source[start..]
        .find('{')
        .map(|offset| start + offset)
        .unwrap_or_else(|| panic!("missing opening brace for {marker}"));
    let mut depth = 0usize;
    for (offset, byte) in source[open..].bytes().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &source[start..=open + offset];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated source item {marker}")
}

#[test]
fn v16_program_destructive_consent_composition_is_source_complete() {
    let source = include_str!("../../../src/v16_program.rs");
    let instruction_enum = inv013_braced_body(source, "pub enum Instruction {");
    let close_slab = inv013_braced_body(instruction_enum, "CloseSlab {");
    assert!(close_slab.contains("authority_epoch: u64"));

    let dispatch = inv013_braced_body(source, "pub fn process_instruction<'a>(");
    assert!(dispatch.contains(
        "Instruction::CloseSlab { authority_epoch } => {\n                handle_close_slab(program_id, accounts, authority_epoch)"
    ));
    let handler = inv013_braced_body(source, "fn handle_close_slab<'a>(");
    for guard in [
        "expect_signer(admin_dest)?;",
        "expect_writable(market_ai)?;",
        "expect_owner(market_ai, program_id)?;",
        "expect_live_authority(&cfg.marketauth, admin_dest.key)?;",
        "require_authority_epoch_view(&group, 0, expected_authority_epoch)?;",
        "market_ai.realloc(constants::HEADER_LEN, false)?;",
        "state::write_closed_market_tombstone",
    ] {
        assert!(handler.contains(guard), "CloseSlab lost guard {guard}");
    }

    let position_evidence = include_str!("inv_004_position_episode_binding.rs");
    assert!(position_evidence.contains(
        "fn v16_program_retained_position_binding_and_writer_rosters_are_source_complete("
    ));
    let asset_evidence = include_str!("inv_002_asset_generation_binding.rs");
    assert!(asset_evidence
        .contains("fn v16_program_asset_generation_field_and_guard_roster_is_source_complete("));
    let authority_evidence = include_str!("inv_005_authority_incarnation_binding.rs");
    assert!(authority_evidence
        .contains("fn v16_program_configured_authority_route_dispositions_are_source_complete("));
    assert!(authority_evidence.contains("let expected_open = std::collections::BTreeSet::new();"));
    let transaction_domain_evidence =
        include_str!("../public_sbf/inv_006_program_chain_message_type_and_version_binding.rs");
    assert!(transaction_domain_evidence
        .contains("fn deployed_wrapper_has_no_detached_signature_interpreter("));
    let account_evidence = include_str!("../public_sbf/inv_007_no_aba_reuse.rs");
    assert!(
        account_evidence.contains("fn v16_wrapper_account_incarnation_census_is_source_complete(")
    );
}

#[test]
fn v16_program_stale_rebalance_reduce_episode_rejects_atomically_after_reopen() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let counterparty_owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let counterparty = env.create_portfolio(&counterparty_owner);
    env.deposit(&owner, portfolio, 1_000_000);
    env.deposit(&counterparty_owner, counterparty, 1_000_000);

    let size_q = POS_SCALE as i128;
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        0,
        &owner,
        portfolio,
        &counterparty_owner,
        counterparty,
        size_q,
        100,
        0,
    );
    let stale_epoch = env.portfolio_position_epoch(portfolio);
    let stale_portfolio_id = env.portfolio_id(portfolio);
    assert!(
        env.portfolio_state(portfolio).legs[0].basis_pos_q.get() > 0,
        "setup opened a positive position"
    );

    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        0,
        &owner,
        portfolio,
        &counterparty_owner,
        counterparty,
        -size_q,
        100,
        0,
    );
    assert!(
        !has_active_leg_for_asset(&env.portfolio_state(portfolio), 0),
        "public close clears the first episode"
    );

    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        0,
        &owner,
        portfolio,
        &counterparty_owner,
        counterparty,
        size_q,
        100,
        0,
    );
    let current_epoch = env.portfolio_position_epoch(portfolio);
    assert!(
        current_epoch > stale_epoch,
        "reopen creates a later position episode"
    );
    assert_eq!(
        env.portfolio_id(portfolio),
        stale_portfolio_id,
        "episode changed without replacing the portfolio incarnation"
    );

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let counterparty_before = env.svm.get_account(&counterparty).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let stale = env.send(
        ProgInstruction::RebalanceReduce {
            portfolio_id: stale_portfolio_id,
            position_epoch: stale_epoch,
            asset_index: 0,
            reduce_q: POS_SCALE / 4,
        },
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&owner],
    );
    assert!(
        stale.is_err(),
        "stale destructive reduction from a prior position episode must reject"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "stale reduction rejection leaves market exposure unchanged"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "stale reduction rejection leaves the reopened owner leg intact"
    );
    assert_eq!(
        env.svm.get_account(&counterparty).unwrap(),
        counterparty_before,
        "stale reduction rejection leaves the counterparty untouched"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "stale reduction rejection moves no custody"
    );

    env.svm.expire_blockhash();
    let current = env.send(
        ProgInstruction::RebalanceReduce {
            portfolio_id: stale_portfolio_id,
            position_epoch: current_epoch,
            asset_index: 0,
            reduce_q: POS_SCALE / 4,
        },
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&owner],
    );
    assert!(
        current.is_ok(),
        "current destructive reduction remains live: {current:?}"
    );
    assert!(
        env.portfolio_state(portfolio).legs[0]
            .basis_pos_q
            .get()
            .unsigned_abs()
            < POS_SCALE,
        "current-episode reduction changed the reopened exposure"
    );
}

// ForfeitRecoveryLeg owner-gating + input guard (sibling of v16_attack_rebalance_reduce_owner_gated, which
// was tested while ForfeitRecoveryLeg was not). handle_forfeit_recovery_leg uses with_one_portfolio_view
// (owner_must_sign=true), so a non-owner forfeiting a victim's recovery leg -- which would force the victim
// to realize a loss -- must reject before any engine mutation. Also guards the b_delta_budget==0 reject.
#[test]
fn v16_attack_forfeit_recovery_leg_owner_gated_and_zero_budget_rejected() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 5_000, 10_000, 1_000);
    let la = Keypair::new();
    let pa = env.create_portfolio(&la);
    let lb = Keypair::new();
    let pb = env.create_portfolio(&lb);
    env.deposit(&la, pa, 1_000_000);
    env.deposit(&lb, pb, 1_000_000);
    env.trade_asset_with_cu(0, &la, pa, &lb, pb, POS_SCALE as i128, 100, 0);
    let basis0 = env.portfolio_state(pa).legs[0].basis_pos_q.get();
    assert!(basis0 != 0, "la opened a position");
    let (_, g0) = env.market_state();

    // ATTACK: a non-owner forfeits la's recovery leg -> reject (owner mismatch, before engine).
    let mallory = Keypair::new();
    env.ensure_signer_account(mallory.pubkey());
    env.svm.expire_blockhash();
    let r_grief = env.send(
        ProgInstruction::ForfeitRecoveryLeg {
            portfolio_id: env.portfolio_id(pa),
            position_epoch: env.portfolio_position_epoch(pa),
            asset_index: 0,
            b_delta_budget: 1_000,
        },
        vec![
            AccountMeta::new(mallory.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(pa, false),
        ],
        &[&mallory],
    );
    assert!(
        r_grief.is_err(),
        "non-owner forfeit of a victim's recovery leg must reject"
    );
    assert_eq!(
        env.portfolio_state(pa).legs[0].basis_pos_q.get(),
        basis0,
        "victim's position untouched by rejected griefing forfeit"
    );
    assert_eq!(
        env.market_state().1.vault,
        g0.vault,
        "vault unchanged by rejected griefing forfeit"
    );

    // INPUT GUARD: b_delta_budget == 0 rejected (checked before with_one_portfolio_view).
    env.svm.expire_blockhash();
    let r_zero = env.send(
        ProgInstruction::ForfeitRecoveryLeg {
            portfolio_id: env.portfolio_id(pa),
            position_epoch: env.portfolio_position_epoch(pa),
            asset_index: 0,
            b_delta_budget: 0,
        },
        vec![
            AccountMeta::new(la.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(pa, false),
        ],
        &[&la],
    );
    assert!(r_zero.is_err(), "b_delta_budget == 0 must be rejected");
}
