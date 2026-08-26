//! INV-080 - Error propagation and exact rollback.
//!
//! Normative obligation: Every engine error aborts the instruction and commits no persistent bytes, tokens, or lamports.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): a source-complete engine-error
//! disposition and dispatcher/entrypoint composition roster; multi-instruction aborts before later
//! SPL and matcher-CPI consumers; partial oracle error retry safety; and stale-window, raw-account
//! realloc, terminal-top-up, and token-CPI error paths that would otherwise mutate persistent
//! economic state. These tests exercise the deployed public wrapper with real SBF/LiteSVM account
//! construction and assert exact rollback plus retry liveness.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_program_explicit_engine_error_dispositions_are_source_complete() {
    struct ErrorDisposition {
        source_arm: &'static str,
        disposition: &'static str,
        witness: &'static str,
    }

    const ROWS: &[ErrorDisposition] = &[
        ErrorDisposition {
            source_arm: "Err(V16Error::LockActive) => false,",
            disposition: "optional deregistration keeps the live user account",
            witness: "v16_attack_sync_maintenance_cannot_close_empty_live_victim_portfolio",
        },
        ErrorDisposition {
            source_arm: "Err(V16Error::NonProgress) if market_accrual_performed => None,",
            disposition: "the wrapper already committed authenticated market progress",
            witness: "v16_attack_stale_liquidation_budget_observation_crank_progresses_without_reward_or_value",
        },
        ErrorDisposition {
            source_arm: "Err(V16Error::NonProgress) => {",
            disposition: "an unaccompanied fixed point returns an instruction error",
            witness: "v16_regression_crank_idempotent_at_settlement_fixed_point",
        },
    ];

    let production = include_str!("../../../src/v16_program.rs");
    let production = production
        .split("    #[cfg(test)]\n    mod tests")
        .next()
        .expect("production prefix exists");
    let actual = production
        .lines()
        .map(str::trim)
        .filter(|line| line.contains("Err(V16Error::") && line.contains("=>"))
        .collect::<Vec<_>>();
    let expected = ROWS.iter().map(|row| row.source_arm).collect::<Vec<_>>();
    assert_eq!(
        actual, expected,
        "every explicit engine-error arm needs a reviewed INV-080 disposition"
    );
    assert_eq!(
        production
            .matches("Err(err) => return Err(map_v16_error(err)),")
            .count(),
        2,
        "both explicit engine catch-all arms must continue to propagate"
    );
    assert_eq!(
        production.matches("map_err(map_v16_error)").count(),
        138,
        "engine-result mapping drift requires an INV-080 disposition review"
    );
    assert!(
        !production.contains("Err(_) =>"),
        "the wrapper must not silently discard an unclassified error"
    );

    let witnesses = [
        include_str!("inv_021_account_creation_reallocation_close_rent_and_lamport_safety.rs"),
        include_str!("inv_071_crank_progress.rs"),
    ];
    for row in ROWS {
        assert!(!row.disposition.is_empty());
        assert!(
            witnesses
                .iter()
                .any(|source| source.contains(&format!("fn {}", row.witness))),
            "engine-error disposition lacks public witness {}",
            row.witness
        );
    }
}

#[test]
fn v16_program_dispatch_and_entrypoints_preserve_every_handler_error() {
    let production = include_str!("../../../src/v16_program.rs");
    let dispatcher = production
        .split("    pub fn process_instruction<'a>(\n")
        .nth(1)
        .expect("processor dispatcher exists")
        .split("    #[inline(never)]\n    fn handle_init_market")
        .next()
        .expect("dispatcher ends before the first handler");
    assert!(
        dispatcher.contains("match Instruction::decode(instruction_data)?"),
        "decode errors must propagate before dispatch"
    );

    let mut handlers = std::collections::BTreeSet::new();
    let mut remaining = dispatcher;
    while let Some(start) = remaining.find("handle_") {
        remaining = &remaining[start..];
        let end = remaining
            .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
            .unwrap_or(remaining.len());
        handlers.insert(&remaining[..end]);
        remaining = &remaining[end..];
    }
    assert_eq!(
        dispatcher.matches("handle_").count(),
        50,
        "all 50 public variants must return a handler result"
    );
    for (shared_handler, variant_count) in [
        ("handle_top_up_insurance(", 2),
        ("handle_update_market_authority_policy(", 4),
        ("handle_configure_managed_mark(", 2),
        ("handle_push_managed_mark(", 2),
    ] {
        assert_eq!(
            dispatcher.matches(shared_handler).count(),
            variant_count,
            "shared handler family must preserve every variant's direct result"
        );
    }
    assert_eq!(
        handlers.len(),
        44,
        "dispatcher implementation count drift requires a shared-handler review"
    );
    assert_eq!(
        dispatcher.matches("Instruction::").count(),
        51,
        "the dispatcher must contain decode plus exactly 50 variant arms"
    );
    for forbidden in ["Ok(())", ".ok()", "is_err()", "unwrap_or", "let _ ="] {
        assert!(
            !dispatcher.contains(forbidden),
            "dispatcher must not coerce a handler result through {forbidden}"
        );
    }

    let standard_entrypoint = production
        .split("#[cfg(all(not(feature = \"no-entrypoint\"), not(feature = \"anchor-v2\")))]")
        .nth(1)
        .expect("standard entrypoint exists")
        .split("#[cfg(all(not(feature = \"no-entrypoint\"), feature = \"anchor-v2\"))]")
        .next()
        .expect("standard entrypoint has a bounded source region");
    assert!(standard_entrypoint.contains(
        "match process_instruction(&program_id, &accounts, &instruction_data) {\n            Ok(()) => SUCCESS,\n            Err(error) => error.into(),\n        }"
    ));

    let anchor_entrypoint = production
        .split("#[cfg(all(not(feature = \"no-entrypoint\"), feature = \"anchor-v2\"))]")
        .nth(1)
        .expect("anchor-v2 entrypoint exists");
    assert!(anchor_entrypoint.contains(
        "process_with_legacy_account_infos(&program_id, accounts, instruction_data)\n            .map_err(map_legacy_error)"
    ));
    assert!(
        !anchor_entrypoint.contains("unwrap_or(Ok(()))"),
        "the compatibility entrypoint must not turn an adapter error into success"
    );
}

#[test]
fn v16_engine_error_aborts_before_later_valid_instruction_can_commit() {
    use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

    let mut env = V16CuEnv::new();
    let withdrawing_owner = Keypair::new();
    let depositing_owner = Keypair::new();
    let withdrawing_portfolio = env.create_portfolio(&withdrawing_owner);
    let depositing_portfolio = env.create_portfolio(&depositing_owner);
    env.deposit(&withdrawing_owner, withdrawing_portfolio, 10);

    let withdraw_destination = env.token_account(withdrawing_owner.pubkey(), 0);
    let deposit_source = env.token_account(depositing_owner.pubkey(), 7);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let withdrawing_before = env.svm.get_account(&withdrawing_portfolio).unwrap();
    let depositing_before = env.svm.get_account(&depositing_portfolio).unwrap();
    let withdraw_destination_before = env.svm.get_account(&withdraw_destination).unwrap();
    let deposit_source_before = env.svm.get_account(&deposit_source).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    let over_withdraw = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(withdrawing_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(withdrawing_portfolio, false),
            AccountMeta::new(withdraw_destination, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: env.withdraw_ix(withdrawing_portfolio, 11).encode(),
    };
    let valid_later_deposit = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(depositing_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(depositing_portfolio, false),
            AccountMeta::new(deposit_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: env.deposit_ix(depositing_portfolio, 7).encode(),
    };
    env.svm.expire_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &[heap_ix(), cu_ix(), over_withdraw, valid_later_deposit],
        Some(&env.payer.pubkey()),
        &[&env.payer, &withdrawing_owner, &depositing_owner],
        env.svm.latest_blockhash(),
    );
    let error = env
        .svm
        .send_transaction(tx)
        .expect_err("the engine over-withdraw error must abort the transaction");
    match error.err {
        TransactionError::InstructionError(2, InstructionError::Custom(code)) => {
            assert_ne!(code, 0, "engine error must map to a nonzero program error")
        }
        other => {
            panic!("engine rejection must propagate from the first program instruction: {other:?}")
        }
    }

    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(
        env.svm.get_account(&withdrawing_portfolio).unwrap(),
        withdrawing_before
    );
    assert_eq!(
        env.svm.get_account(&depositing_portfolio).unwrap(),
        depositing_before
    );
    assert_eq!(
        env.svm.get_account(&withdraw_destination).unwrap(),
        withdraw_destination_before
    );
    assert_eq!(
        env.svm.get_account(&deposit_source).unwrap(),
        deposit_source_before
    );
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    env.svm.expire_blockhash();
    env.send(
        env.deposit_ix(depositing_portfolio, 7),
        vec![
            AccountMeta::new(depositing_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(depositing_portfolio, false),
            AccountMeta::new(deposit_source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&depositing_owner],
    )
    .expect("the later deposit is independently valid after the aborted transaction");
    assert_eq!(env.portfolio_state(depositing_portfolio).capital.get(), 7);
    assert_eq!(env.token_amount(deposit_source), 0);
}

#[test]
fn v16_engine_error_aborts_before_later_cpi_return_consumer_can_execute() {
    use solana_sdk::{instruction::InstructionError, transaction::TransactionError};

    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);

    let withdrawing_owner = Keypair::new();
    let withdrawing_portfolio = env.create_portfolio(&withdrawing_owner);
    env.deposit(&withdrawing_owner, withdrawing_portfolio, 10);
    let withdraw_destination = env.token_account(withdrawing_owner.pubkey(), 0);

    let taker = Keypair::new();
    let lp = Keypair::new();
    let taker_portfolio = env.create_portfolio(&taker);
    let lp_portfolio = env.create_portfolio(&lp);
    env.deposit(&taker, taker_portfolio, 1_000_000);
    env.deposit(&lp, lp_portfolio, 1_000_000);
    let matcher_program = Pubkey::new_unique();
    env.svm.add_program(
        matcher_program,
        &std::fs::read(auth_matcher_program_path()).expect("read auth matcher SBF"),
    );
    let (matcher_context, matcher_delegate, _) =
        env.init_auth_matcher_context(matcher_program, &lp, lp_portfolio);

    let over_withdraw = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(withdrawing_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(withdrawing_portfolio, false),
            AccountMeta::new(withdraw_destination, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        data: env.withdraw_ix(withdrawing_portfolio, 11).encode(),
    };
    let trade = env.trade_cpi_ix(taker_portfolio, lp_portfolio, 0, POS_SCALE as i128, 0, 0);
    let trade_accounts = vec![
        AccountMeta::new(taker.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(taker_portfolio, false),
        AccountMeta::new(lp_portfolio, false),
        AccountMeta::new_readonly(matcher_program, false),
        AccountMeta::new(matcher_context, false),
        AccountMeta::new_readonly(matcher_delegate, false),
    ];
    let later_cpi_trade = Instruction {
        program_id: env.program_id,
        accounts: trade_accounts.clone(),
        data: trade.clone().encode(),
    };

    let market_before = env.svm.get_account(&env.market).unwrap();
    let withdrawing_before = env.svm.get_account(&withdrawing_portfolio).unwrap();
    let taker_before = env.svm.get_account(&taker_portfolio).unwrap();
    let lp_before = env.svm.get_account(&lp_portfolio).unwrap();
    let context_before = env.svm.get_account(&matcher_context).unwrap();
    let destination_before = env.svm.get_account(&withdraw_destination).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    env.svm.expire_blockhash();
    let tx = Transaction::new_signed_with_payer(
        &[heap_ix(), cu_ix(), over_withdraw, later_cpi_trade],
        Some(&env.payer.pubkey()),
        &[&env.payer, &withdrawing_owner, &taker],
        env.svm.latest_blockhash(),
    );
    let error = env
        .svm
        .send_transaction(tx)
        .expect_err("the first engine error must abort before the CPI consumer");
    assert!(
        matches!(
            &error.err,
            TransactionError::InstructionError(2, InstructionError::Custom(code)) if *code != 0
        ),
        "the over-withdraw must remain the transaction's first program error: {:?}",
        error.err
    );
    for (key, before) in [
        (env.market, market_before),
        (withdrawing_portfolio, withdrawing_before),
        (taker_portfolio, taker_before),
        (lp_portfolio, lp_before),
        (matcher_context, context_before),
        (withdraw_destination, destination_before),
        (env.vault, vault_before),
    ] {
        assert_eq!(
            env.svm.get_account(&key).unwrap(),
            before,
            "aborted transaction must roll back account {key}"
        );
    }

    env.svm.expire_blockhash();
    env.send(trade, trade_accounts, &[&taker])
        .expect("the CPI trade remains independently executable after rollback");
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(taker_portfolio), 0).basis_pos_q,
        POS_SCALE as i128
    );
}

#[test]
fn v16_attack_hybrid_soft_stale_partial_oracle_error_does_not_poison_retry() {
    let mut env = V16CuEnv::new();
    set_test_clock(&mut env, 1, 100);

    let feeds = [[0xc1u8; 32], [0xc2u8; 32], [0xc3u8; 32]];
    let initial_leg0 = env.set_pyth_price(&feeds[0], 4_000_000_000, -6, 100);
    let initial_leg1 = env.set_pyth_price(&feeds[1], 150_000_000, -6, 100);
    let initial_leg2 = env.set_pyth_price(&feeds[2], 200_000_000, -6, 100);
    env.configure_three_leg_hybrid_with_cu(feeds, initial_leg0, initial_leg1, initial_leg2, 1, 100);

    let keeper = Keypair::new();
    let keeper_portfolio = env.create_portfolio(&keeper);
    set_test_clock(&mut env, 2, 101);
    let fresh_leg0 = env.set_pyth_price(&feeds[0], 4_200_000_000, -6, 101);
    let fresh_leg1 = env.set_pyth_price(&feeds[1], 150_000_000, -6, 101);
    let fresh_leg2 = env.set_pyth_price(&feeds[2], 200_000_000, -6, 101);
    env.crank_with_oracle_tail(
        keeper_portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
        &[fresh_leg0, fresh_leg1, fresh_leg2],
    );
    let (fresh_cfg, fresh_group) = env.market_state();
    assert_eq!(fresh_cfg.last_good_oracle_slot, 2);
    assert_eq!(fresh_group.assets[0].effective_price, 140_000);

    set_test_clock(&mut env, 6, 170);
    let mixed_fresh_leg0 = env.set_pyth_price(&feeds[0], 4_350_000_000, -6, 170);
    env.svm.expire_blockhash();
    let fallback = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 6,
            observations: crank_observations_with_accounts(0, 3),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(keeper_portfolio, false),
            AccountMeta::new_readonly(mixed_fresh_leg0, false),
            AccountMeta::new_readonly(fresh_leg1, false),
            AccountMeta::new_readonly(fresh_leg2, false),
        ],
        &[],
    );
    let fallback_cu = fallback.expect("soft-stale mixed oracle tail falls back without DoS");
    assert_cu_within(
        "HybridMark soft-stale mixed-tail fallback",
        fallback_cu,
        CRANK_CU_LIMIT,
    );
    let (fallback_cfg, fallback_group) = env.market_state();
    assert_eq!(
        fallback_cfg.last_good_oracle_slot, 2,
        "mixed-tail fallback must not claim a fresh external oracle slot"
    );
    assert_eq!(
        fallback_group.assets[0].effective_price, 140_000,
        "fallback uses the committed EWMA mark, not a partially composed external price"
    );

    set_test_clock(&mut env, 7, 171);
    let retry_leg0 = env.set_pyth_price(&feeds[0], 4_500_000_000, -6, 171);
    let retry_leg1 = env.set_pyth_price(&feeds[1], 150_000_000, -6, 171);
    let retry_leg2 = env.set_pyth_price(&feeds[2], 200_000_000, -6, 171);
    let retry_cu = env.crank_with_oracle_tail(
        keeper_portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 7,
            observations: crank_observations(0),
        },
        &[retry_leg0, retry_leg1, retry_leg2],
    );
    assert_cu_within(
        "HybridMark post-mixed-tail fresh retry",
        retry_cu,
        CRANK_CU_LIMIT,
    );
    let (retry_cfg, retry_group) = env.market_state();
    assert_eq!(retry_cfg.last_good_oracle_slot, 7);
    assert_eq!(retry_cfg.mark_ewma_last_slot, 7);
    assert_eq!(retry_cfg.mark_ewma_e6, 150_000);
    assert_eq!(retry_group.assets[0].effective_price, 150_000);
    assert_eq!(retry_group.assets[0].raw_oracle_target_price, 150_000);
}

// accounts expanded/zero-filled while the market is supposed to be frozen.
#[test]
fn v16_program_stale_init_portfolio_rolls_back_undersized_realloc() {
    let mut env = V16CuEnv::new();
    env.configure_permissionless_resolve_with_cu(5, 5);
    env.configure_auth_mark_with_cu(0, 100);

    env.svm.warp_to_slot(3);
    env.push_auth_mark_with_cu(3, 100);
    env.svm.warp_to_slot(40);

    let attacker = Keypair::new();
    env.ensure_signer_account(attacker.pubkey());
    let small_len = env.portfolio_account_len / 2;
    let stale_portfolio = env.program_account(small_len);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let stale_portfolio_before = env.svm.get_account(&stale_portfolio).unwrap();
    assert_eq!(
        stale_portfolio_before.data.len(),
        small_len,
        "test setup supplies an undersized uninitialized program-owned account"
    );

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(stale_portfolio, false),
        ],
        &[&attacker],
    );
    assert!(
        rejected.is_err(),
        "stale InitPortfolio on an undersized account must reject"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected undersized stale init leaves market accounting unchanged"
    );
    assert_eq!(
        env.svm.get_account(&stale_portfolio).unwrap(),
        stale_portfolio_before,
        "rejected undersized stale init rolls back the pre-stale realloc"
    );
    assert_eq!(
        env.svm.get_account(&stale_portfolio).unwrap().data.len(),
        small_len,
        "failed stale init does not leave a public account expansion behind"
    );
    assert_eq!(
        env.market_state().1.materialized_portfolio_count,
        0,
        "no undersized stale account can materialize and block slab reclaim"
    );

    env.svm.expire_blockhash();
    let resolve = env.send(
        ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(
        resolve.is_ok(),
        "permissionless resolve remains live after rejected undersized init: {resolve:?}"
    );
    env.close_slab_with_cu();
}

// debit capital, credit insurance, or collect a reward.
#[test]
fn v16_program_stale_maintenance_rolls_back_legacy_cranker_reallocs() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 10_000, 10_000, 10_000, 58,
    );
    env.configure_permissionless_resolve_with_cu(5, 5);
    env.configure_auth_mark_with_cu(0, 100);
    env.update_maintenance_fee_policy_with_cu(4_000);

    let fresh_owner = Keypair::new();
    let fresh_cranker_owner = Keypair::new();
    let fresh_payer = env.create_portfolio(&fresh_owner);
    let fresh_cranker = env.create_portfolio(&fresh_cranker_owner);
    env.deposit(&fresh_owner, fresh_payer, 100_000_000);

    let stale_owner = Keypair::new();
    let stale_cranker_owner = Keypair::new();
    let stale_payer = env.create_portfolio(&stale_owner);
    let stale_cranker = env.create_portfolio(&stale_cranker_owner);
    env.deposit(&stale_owner, stale_payer, 100_000_000);

    env.svm.warp_to_slot(3);
    env.push_auth_mark_with_cu(3, 100);

    // Non-vacuous control: while fresh, legacy payer+cranker accounts grow and the cranker earns.
    env.svm.warp_to_slot(4);
    for portfolio in [fresh_payer, fresh_cranker] {
        let mut legacy = env.svm.get_account(&portfolio).unwrap();
        legacy.data.truncate(PORTFOLIO_ENGINE_ACCOUNT_LEN);
        env.svm.set_account(portfolio, legacy).unwrap();
    }
    let fresh_cu = env.sync_maintenance_fee_with_cu(fresh_payer, Some(fresh_cranker), 4);
    assert_cu_within(
        "fresh legacy SyncMaintenanceFee",
        fresh_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        env.svm.get_account(&fresh_payer).unwrap().data.len(),
        env.portfolio_account_len,
        "fresh maintenance sync grows the legacy charged portfolio"
    );
    assert_eq!(
        env.svm.get_account(&fresh_cranker).unwrap().data.len(),
        env.portfolio_account_len,
        "fresh maintenance sync grows the legacy cranker portfolio"
    );
    assert_eq!(env.portfolio_state(fresh_payer).last_fee_slot.get(), 4);
    assert!(
        env.portfolio_state(fresh_cranker).capital.get() > 0,
        "fresh maintenance sync credits the cranker reward"
    );

    for portfolio in [stale_payer, stale_cranker] {
        let mut legacy = env.svm.get_account(&portfolio).unwrap();
        legacy.data.truncate(PORTFOLIO_ENGINE_ACCOUNT_LEN);
        env.svm.set_account(portfolio, legacy).unwrap();
    }
    env.svm.warp_to_slot(40);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let payer_before = env.svm.get_account(&stale_payer).unwrap();
    let cranker_before = env.svm.get_account(&stale_cranker).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::SyncMaintenanceFee { now_slot: 0 },
        vec![
            AccountMeta::new(env.market, false),
            AccountMeta::new(stale_payer, false),
            AccountMeta::new(stale_cranker, false),
        ],
        &[],
    );
    assert!(
        rejected.is_err(),
        "legacy cranker maintenance sync must reject once the market is resolve-matured"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected stale maintenance sync leaves insurance and budgets unchanged"
    );
    assert_eq!(
        env.svm.get_account(&stale_payer).unwrap(),
        payer_before,
        "rejected stale maintenance sync rolls back charged-account realloc/debit"
    );
    assert_eq!(
        env.svm.get_account(&stale_cranker).unwrap(),
        cranker_before,
        "rejected stale maintenance sync rolls back cranker realloc/reward"
    );
    assert_eq!(
        env.svm.get_account(&stale_payer).unwrap().data.len(),
        PORTFOLIO_ENGINE_ACCOUNT_LEN,
        "failed stale maintenance sync leaves charged account legacy-sized"
    );
    assert_eq!(
        env.svm.get_account(&stale_cranker).unwrap().data.len(),
        PORTFOLIO_ENGINE_ACCOUNT_LEN,
        "failed stale maintenance sync leaves cranker account legacy-sized"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected stale maintenance sync moves no custody"
    );

    env.svm.expire_blockhash();
    let resolve = env.send(
        ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(
        resolve.is_ok(),
        "permissionless resolve remains live after rejected stale legacy maintenance sync: {resolve:?}"
    );
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
}

// expanded, cancel the close ledger, credit capital, or move SPL tokens.
#[test]
fn v16_program_stale_cure_rolls_back_legacy_realloc_and_transfer() {
    let mut env = V16CuEnv::new();
    env.configure_permissionless_resolve_with_cu(5, 5);
    env.configure_auth_mark_with_cu(0, 100);

    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 100);
    env.seed_cancellable_close_progress(portfolio);
    let source = env.token_account_for_mint(env.mint, owner.pubkey(), 20);
    let portfolio_id = env.portfolio_id(portfolio);
    let position_epoch = env.portfolio_position_epoch(portfolio);

    let mut legacy = env.svm.get_account(&portfolio).unwrap();
    legacy.data.truncate(PORTFOLIO_ENGINE_ACCOUNT_LEN);
    env.svm.set_account(portfolio, legacy).unwrap();
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap().data.len(),
        PORTFOLIO_ENGINE_ACCOUNT_LEN,
        "test setup simulates a legacy portfolio with active close-progress"
    );

    env.svm.warp_to_slot(3);
    env.push_auth_mark_with_cu(3, 100);
    env.svm.warp_to_slot(40);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let source_before = env.svm.get_account(&source).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::CureAndCancelClose {
            portfolio_id,
            position_epoch,
            optional_deposit: 20,
        },
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    assert!(
        rejected.is_err(),
        "legacy CureAndCancelClose must reject once the market is resolve-matured"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected stale legacy cure leaves close-progress accounting unchanged"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "rejected stale legacy cure rolls back the pre-stale realloc and close ledger"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap().data.len(),
        PORTFOLIO_ENGINE_ACCOUNT_LEN,
        "failed stale cure does not leave a public legacy realloc behind"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected stale legacy cure moves no vault custody"
    );
    assert_eq!(
        env.svm.get_account(&source).unwrap(),
        source_before,
        "rejected stale legacy cure pulls no optional-deposit tokens"
    );

    env.svm.expire_blockhash();
    let resolve = env.send(
        ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(
        resolve.is_ok(),
        "permissionless resolve remains live after rejected stale legacy cure: {resolve:?}"
    );
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
}

// roll back every protocol account plus matcher-context writes; the same path remains live while fresh.
#[test]
fn v16_program_batch_tradecpi_rejects_stale_resolve_matured_atomically() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_permissionless_resolve_with_cu(5, 5);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
    env.configure_auth_mark_for_asset_as_admin(1, 1, 100);

    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker = Keypair::new();
    let lp = Keypair::new();
    let taker_portfolio = env.create_portfolio(&taker);
    let lp_portfolio = env.create_portfolio(&lp);
    env.deposit(&taker, taker_portfolio, 1_000_000);
    env.deposit(&lp, lp_portfolio, 1_000_000);
    let (ctx, delegate, _) =
        env.init_matcher_context_authorized(matcher_program, &lp, lp_portfolio);
    let sz = (2 * POS_SCALE) as i128;
    let legs = vec![
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
    ];
    let accounts = |env: &V16CuEnv| {
        vec![
            AccountMeta::new(taker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(taker_portfolio, false),
            AccountMeta::new(lp_portfolio, false),
            AccountMeta::new_readonly(matcher_program, false),
            AccountMeta::new(ctx, false),
            AccountMeta::new_readonly(delegate, false),
        ]
    };

    env.svm.warp_to_slot(4);
    env.svm.expire_blockhash();
    let fresh = env.send(
        env.batch_trade_cpi_ix(taker_portfolio, lp_portfolio, legs.clone()),
        accounts(&env),
        &[&taker],
    );
    assert!(
        fresh.is_ok(),
        "fresh-oracle BatchTradeCpi should still fill before stale maturity: {fresh:?}"
    );
    let fresh_taker = env.portfolio_state(taker_portfolio);
    assert!(
        has_active_leg_for_asset(&fresh_taker, 0) && has_active_leg_for_asset(&fresh_taker, 1),
        "fresh control fills both batch legs"
    );

    env.svm.warp_to_slot(40);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker_portfolio).unwrap();
    let lp_before = env.svm.get_account(&lp_portfolio).unwrap();
    let ctx_before = env.svm.get_account(&ctx).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        env.batch_trade_cpi_ix(taker_portfolio, lp_portfolio, legs),
        accounts(&env),
        &[&taker],
    );
    assert!(
        rejected.is_err(),
        "BatchTradeCpi must reject once the market is stale-resolve matured"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "stale BatchTradeCpi leaves market bytes unchanged"
    );
    assert_eq!(
        env.svm.get_account(&taker_portfolio).unwrap(),
        taker_before,
        "stale BatchTradeCpi leaves taker portfolio unchanged"
    );
    assert_eq!(
        env.svm.get_account(&lp_portfolio).unwrap(),
        lp_before,
        "stale BatchTradeCpi leaves LP portfolio unchanged"
    );
    assert_eq!(
        env.svm.get_account(&ctx).unwrap(),
        ctx_before,
        "stale BatchTradeCpi rolls back matcher context writes"
    );

    env.svm.expire_blockhash();
    let resolve = env.send(
        ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(
        resolve.is_ok(),
        "permissionless resolve still succeeds after the rejected stale batch: {resolve:?}"
    );
    assert_eq!(env.market_state().1.mode, MarketModeV16::Resolved);
}

// expanded/zero-filled, burn the pending receipt, or move vault custody.
#[test]
fn v16_program_claim_resolved_topup_bad_dest_rolls_back_legacy_realloc() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    {
        let mut market_account = env.svm.get_account(&env.market).expect("market account");
        let mut portfolio_account = env.svm.get_account(&portfolio).expect("portfolio account");
        let (cfg, mut group) = state::read_market(&market_account.data).unwrap();
        let mut account = state::read_portfolio(&portfolio_account.data).unwrap();
        group.mode = MarketModeV16::Resolved;
        group.resolved_slot = 1;
        group.current_slot = 1;
        group.vault = 60;
        group.payout_snapshot_captured = true;
        group.payout_snapshot = 100;
        group.resolved_payout_ledger = ResolvedPayoutLedgerV16 {
            snapshot_residual: 100,
            terminal_claim_exact_receipts_num: 100 * BOUND_SCALE,
            terminal_claim_bound_unreceipted_num: 0,
            current_payout_rate_num: 100 * BOUND_SCALE,
            current_payout_rate_den: 100 * BOUND_SCALE,
            snapshot_slot: 1,
            payout_halted: false,
            finalized: false,
        };
        account.resolved_payout_receipt =
            percolator::ResolvedPayoutReceiptV16Account::from_runtime(&ResolvedPayoutReceiptV16 {
                present: true,
                prior_bound_contribution_num: 100 * BOUND_SCALE,
                live_released_face_at_receipt: 0,
                terminal_positive_claim_face: 100,
                paid_effective: 40,
                finalized: false,
            });
        state::write_market(&mut market_account.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut portfolio_account.data, &account).unwrap();
        env.svm.set_account(env.market, market_account).unwrap();
        env.svm.set_account(portfolio, portfolio_account).unwrap();
    }
    env.set_token_account_amount(env.vault, env.mint, env.vault_authority, 60);

    let mut legacy = env.svm.get_account(&portfolio).unwrap();
    legacy.data.truncate(PORTFOLIO_ENGINE_ACCOUNT_LEN);
    env.svm.set_account(portfolio, legacy).unwrap();
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap().data.len(),
        PORTFOLIO_ENGINE_ACCOUNT_LEN,
        "test setup simulates a legacy portfolio with a pending top-up receipt"
    );

    let attacker = Keypair::new();
    let bad_dest = env.token_account_for_mint(env.mint, attacker.pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let bad_dest_before = env.svm.get_account(&bad_dest).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::ClaimResolvedPayoutTopup,
        vec![
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(bad_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        rejected.is_err(),
        "unsigned top-up must reject a destination not owned by the portfolio owner"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected bad-dest top-up leaves terminal payout accounting unchanged"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "rejected bad-dest top-up rolls back the legacy realloc and receipt mutation"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected bad-dest top-up moves no vault custody"
    );
    assert_eq!(
        env.svm.get_account(&bad_dest).unwrap(),
        bad_dest_before,
        "bad destination receives no tokens"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap().data.len(),
        PORTFOLIO_ENGINE_ACCOUNT_LEN,
        "failed unsigned top-up does not leave a public realloc behind"
    );

    let good_dest = env.token_account_for_mint(env.mint, owner.pubkey(), 0);
    let topup_cu = env.claim_resolved_payout_topup_with_cu(owner.pubkey(), portfolio, good_dest);
    assert_cu_within(
        "ClaimResolvedPayoutTopup legacy retry",
        topup_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap().data.len(),
        env.portfolio_account_len,
        "valid top-up grows legacy storage exactly once"
    );
    assert_eq!(
        env.token_amount(good_dest),
        60,
        "same legacy receipt remains claimable after the rejected bad-dest attempt"
    );
    let account = env.portfolio_state(portfolio);
    assert_eq!(resolved_receipt(&account).paid_effective, 100);
    assert!(resolved_receipt(&account).finalized);
}

// account must still reject atomically, with no persistent resize or market mutation.
#[test]
fn v16_program_crank_raw_program_portfolio_realloc_rolls_back() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    let raw = Pubkey::new_unique();
    env.svm
        .set_account(
            raw,
            Account {
                lamports: 1_000_000_000,
                data: vec![0u8; 8],
                owner: env.program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let raw_before = env.svm.get_account(&raw).unwrap();
    let market_before = env.svm.get_account(&env.market).unwrap();

    env.svm.warp_to_slot(1);
    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(raw, false),
        ],
        &[],
    );
    assert!(
        rejected.is_err(),
        "raw program-owned account must not become a portfolio through crank pre-realloc"
    );
    assert_eq!(
        env.svm.get_account(&raw).unwrap(),
        raw_before,
        "rejected crank rolls back the raw account realloc and zero-fill"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected raw-account crank leaves market bytes unchanged"
    );

    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.svm.expire_blockhash();
    let ok = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[],
    );
    assert!(
        ok.is_ok(),
        "valid portfolio crank remains live after rejected raw-account attempt: {ok:?}"
    );
}

// security.md sweep - permissionless CloseResolved legacy rollback (#5/#26/#44/#48):
// After the owner exit window, CloseResolved is intentionally permissionless
// when the caller names the portfolio owner, but it mutates payout state before
// validating the destination token account. A bad destination must roll back
// both the legacy realloc and the post-engine payout mutation.
#[test]
fn v16_attack_permissionless_close_resolved_bad_dest_rolls_back_legacy_realloc() {
    let mut env = V16CuEnv::new();
    const EXIT_DELAY: u64 = 5;
    env.configure_permissionless_resolve_with_cu(100, EXIT_DELAY);

    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);
    env.resolve();
    env.svm.warp_to_slot(EXIT_DELAY + 1);

    let mut legacy = env.svm.get_account(&portfolio).unwrap();
    legacy.data.truncate(PORTFOLIO_ENGINE_ACCOUNT_LEN);
    env.svm.set_account(portfolio, legacy).unwrap();
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap().data.len(),
        PORTFOLIO_ENGINE_ACCOUNT_LEN,
        "test setup simulates a resolved legacy portfolio"
    );

    let attacker = Keypair::new();
    let attacker_dest = env.token_account(attacker.pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let attacker_dest_before = env.svm.get_account(&attacker_dest).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        vec![
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(attacker_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );
    assert!(
        rejected.is_err(),
        "permissionless CloseResolved must reject an attacker-owned destination"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected legacy CloseResolved leaves resolved market accounting unchanged"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "rejected legacy CloseResolved rolls back realloc and payout-state mutation"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap().data.len(),
        PORTFOLIO_ENGINE_ACCOUNT_LEN,
        "failed CloseResolved does not leave a public legacy realloc behind"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected legacy CloseResolved moves no vault custody"
    );
    assert_eq!(
        env.svm.get_account(&attacker_dest).unwrap(),
        attacker_dest_before,
        "attacker destination receives no payout"
    );

    let owner_dest = env.token_account(owner.pubkey(), 0);
    env.svm.expire_blockhash();
    let accepted = env
        .send(
            ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            },
            vec![
                AccountMeta::new_readonly(owner.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(owner_dest, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[],
        )
        .expect("permissionless CloseResolved still works for the owner destination");
    assert_cu_within(
        "permissionless legacy CloseResolved retry",
        accepted,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(env.token_amount(owner_dest), 1_000);
    assert_eq!(env.market_state().1.vault, 0);
    assert_eq!(env.portfolio_state(portfolio).capital.get(), 0);
}

// CU/DoS hardening: PermissionlessCrank must prove the target portfolio belongs to this market before
// it parses any supplied hybrid-oracle tail. The valid-target control reaches bogus oracle parsing;
// the foreign-market portfolio must fail as a provenance error first.
#[test]
fn v16_attack_crank_target_portfolio_rejects_before_oracle_tail_parse() {
    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    set_test_clock(&mut env, 1, 100);
    let feed = [0xc7u8; 32];
    let initial_oracle = env.set_pyth_price_with_conf(&feed, 1_000_000, -6, 0, 100);
    env.try_configure_hybrid_asset_with_conf_filter_cu(
        0,
        1,
        0,
        [feed, [0u8; 32], [0u8; 32]],
        &[initial_oracle],
        1,
        100,
        0,
        0,
        10,
        0,
    )
    .expect("configure hybrid oracle");

    let owner = Keypair::new();
    let valid_portfolio = env.create_portfolio(&owner);
    let foreign_owner = Keypair::new();
    let foreign_portfolio = Pubkey::new_unique();
    let mut foreign_data = vec![0u8; env.portfolio_account_len];
    state::init_portfolio_account_zero_copy(
        &mut foreign_data,
        Pubkey::new_unique().to_bytes(),
        foreign_portfolio.to_bytes(),
        foreign_owner.pubkey().to_bytes(),
        0,
        1,
        1,
    )
    .unwrap();
    env.svm
        .set_account(
            foreign_portfolio,
            Account {
                lamports: 1_000_000_000,
                data: foreign_data,
                owner: env.program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let bogus_oracle = env.program_account(8);

    let send = |env: &mut V16CuEnv, portfolio: Pubkey| {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new_readonly(bogus_oracle, false),
            ],
            &[],
        )
    };

    set_test_clock(&mut env, 2, 101);
    let valid_err = send(&mut env, valid_portfolio)
        .expect_err("valid target portfolio should reach bogus oracle parsing");
    assert!(
        !valid_err.contains("Custom(16)"),
        "valid target must not trip the provenance preflight: {valid_err}"
    );

    let market_before = env.svm.get_account(&env.market).unwrap();
    let foreign_before = env.svm.get_account(&foreign_portfolio).unwrap();
    let invalid_err = send(&mut env, foreign_portfolio)
        .expect_err("foreign target portfolio must reject before oracle parsing");
    assert!(
        invalid_err.contains("Custom(16)"),
        "foreign target must fail as EngineProvenanceMismatch before bogus oracle parsing, got {invalid_err}"
    );
    assert!(
        !invalid_err.contains("Custom(29)") && !invalid_err.contains("IllegalOwner"),
        "foreign target must not reach the bogus oracle parser: {invalid_err}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "pre-oracle target rejection leaves market bytes unchanged"
    );
    assert_eq!(
        env.svm.get_account(&foreign_portfolio).unwrap(),
        foreign_before,
        "pre-oracle target rejection leaves foreign portfolio bytes unchanged"
    );
}

// CU/DoS hardening: when liquidation rewards are enabled, a wrong-owner reward tail must reject
// before the crank parses a supplied external oracle tail. The authorized control below reaches the
// bogus oracle and fails there; the wrong-owner attempt must fail as Unauthorized first.
#[test]
fn v16_attack_liquidation_reward_wrong_owner_rejects_before_oracle_tail_parse() {
    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.update_liquidation_fee_policy_with_cu(5_000);
    set_test_clock(&mut env, 1, 100);
    let feed = [0xb1u8; 32];
    let initial_oracle = env.set_pyth_price_with_conf(&feed, 1_000_000, -6, 0, 100);
    env.try_configure_hybrid_asset_with_conf_filter_cu(
        0,
        1,
        0,
        [feed, [0u8; 32], [0u8; 32]],
        &[initial_oracle],
        1,
        100,
        0,
        0,
        10,
        0,
    )
    .expect("configure hybrid oracle");

    let victim_owner = Keypair::new();
    let victim = env.create_portfolio(&victim_owner);
    let reward_owner = Keypair::new();
    let reward = env.create_portfolio(&reward_owner);
    let wrong_owner = Keypair::new();
    env.ensure_signer_account(wrong_owner.pubkey());
    let bogus_oracle = env.program_account(8);

    set_test_clock(&mut env, 2, 101);
    let send = |env: &mut V16CuEnv, signer: &Keypair| {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(signer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(victim, false),
                AccountMeta::new_readonly(bogus_oracle, false),
                AccountMeta::new(reward, false),
            ],
            &[signer],
        )
    };

    let authorized_oracle_err = send(&mut env, &reward_owner)
        .expect_err("authorized reward owner should reach bogus oracle parsing");
    assert!(
        !authorized_oracle_err.contains("Custom(8)"),
        "authorized reward owner must not trip the reward-owner gate: {authorized_oracle_err}"
    );

    let market_before = env.svm.get_account(&env.market).unwrap();
    let victim_before = env.svm.get_account(&victim).unwrap();
    let reward_before = env.svm.get_account(&reward).unwrap();
    let wrong_owner_err = send(&mut env, &wrong_owner)
        .expect_err("wrong reward owner must reject before oracle parsing");
    assert!(
        wrong_owner_err.contains("Custom(8)"),
        "wrong reward owner must fail as Unauthorized before bogus oracle parsing, got {wrong_owner_err}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "wrong-owner preflight leaves market bytes unchanged"
    );
    assert_eq!(
        env.svm.get_account(&victim).unwrap(),
        victim_before,
        "wrong-owner preflight leaves victim bytes unchanged"
    );
    assert_eq!(
        env.svm.get_account(&reward).unwrap(),
        reward_before,
        "wrong-owner preflight leaves reward bytes unchanged"
    );
}

// [from pr114]
// full-interface sweep: a liquidation crank can carry both a hybrid-oracle tail and an optional
// program-owned cranker reward tail. A malformed reward tail must not let the valid oracle update
// partially persist before the later reward-account validation fails.
#[test]
fn v16_attack_hybrid_liquidation_bad_reward_tail_rolls_back_oracle_update() {
    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.update_liquidation_fee_policy_with_cu(5_000);
    set_test_clock(&mut env, 1, 100);
    let feed = [0xa9u8; 32];
    let initial_oracle = env.set_pyth_price_with_conf(&feed, 1_000_000, -6, 0, 100);
    env.try_configure_hybrid_asset_with_conf_filter_cu(
        0,
        1,
        0,
        [feed, [0u8; 32], [0u8; 32]],
        &[initial_oracle],
        1,
        100,
        0,
        0,
        10,
        0,
    )
    .expect("configure hybrid oracle");

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 100_000_000);
    env.deposit(&short_owner, short, 100_000);
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        1_000_000,
        0,
    );

    set_test_clock(&mut env, 2, 101);
    let fresh_oracle = env.set_pyth_price_with_conf(&feed, 2_000_000, -6, 0, 101);
    let malformed_reward = env.program_account(env.portfolio_account_len);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let short_before = env.svm.get_account(&short).unwrap();
    let malformed_before = env.svm.get_account(&malformed_reward).unwrap();
    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations_with_accounts(0, 1),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short, false),
            AccountMeta::new_readonly(fresh_oracle, false),
            AccountMeta::new(malformed_reward, false),
        ],
        &[],
    );
    assert!(
        rejected.is_err(),
        "malformed program-owned reward tail must reject even with a valid hybrid oracle tail"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected mixed-tail crank must not persist the fresh oracle target or liquidation state"
    );
    assert_eq!(
        env.svm.get_account(&short).unwrap(),
        short_before,
        "rejected mixed-tail crank must not mutate the liquidated portfolio"
    );
    assert_eq!(
        env.svm.get_account(&malformed_reward).unwrap(),
        malformed_before,
        "rejected mixed-tail crank must not mutate the malformed reward account"
    );

    let payer_owner = env.payer.insecure_clone();
    let valid_reward = env.create_portfolio(&payer_owner);
    env.svm.expire_blockhash();
    let accepted_observation = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations_with_accounts(0, 1),
        },
        vec![
            AccountMeta::new(payer_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short, false),
            AccountMeta::new_readonly(fresh_oracle, false),
            AccountMeta::new(valid_reward, false),
        ],
        &[&payer_owner],
    );
    assert!(
        accepted_observation.is_ok(),
        "same hybrid oracle observation succeeds with a valid reward portfolio: {accepted_observation:?}"
    );
    let (cfg, group) = env.market_state();
    assert_eq!(
        cfg.last_good_oracle_slot, 2,
        "valid control persists the fresh hybrid oracle update"
    );
    assert_eq!(group.assets[0].raw_oracle_target_price, 2_000_000);
}

// security.md sweep - liquidation reward legacy realloc rollback (#5/#6/#35/#44):
// the reward tail is detected before the crank path refreshes oracle/profile state, and a legacy-sized
// reward portfolio is grown before the later owner/provenance validation. A wrong signer must roll back
// that pre-validation realloc as well as the oracle write, liquidation mutation, and reward credit.
#[test]
fn v16_attack_liquidation_wrong_owner_rolls_back_legacy_reward_realloc() {
    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.update_liquidation_fee_policy_with_cu(5_000);
    env.configure_auth_mark_with_cu(0, 1_000_000);
    let lo = Keypair::new();
    let l = env.create_portfolio(&lo);
    let so = Keypair::new();
    let s = env.create_portfolio(&so);
    let co = Keypair::new();
    let c = env.create_portfolio(&co);
    env.deposit(&lo, l, 100_000_000);
    env.deposit(&so, s, 100_000);
    env.trade_asset_with_cu(0, &lo, l, &so, s, POS_SCALE as i128, 1_000_000, 0);
    for slot in 1..=30u64 {
        env.svm.warp_to_slot(slot);
        let _ = env.push_auth_mark_with_cu(slot, 2_000_000);
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(s, false),
            ],
            &[],
        );
    }
    assert!(
        health_cert(&env.portfolio_state(s)).certified_liq_deficit != 0,
        "short is liquidatable before probing the legacy reward rollback"
    );

    let cranker_cap_before = env.portfolio_state(c).capital.get();
    let mut legacy_cranker = env.svm.get_account(&c).unwrap();
    legacy_cranker.data.truncate(PORTFOLIO_ENGINE_ACCOUNT_LEN);
    env.svm.set_account(c, legacy_cranker).unwrap();
    let cranker_before = env.svm.get_account(&c).unwrap();
    assert_eq!(
        cranker_before.data.len(),
        PORTFOLIO_ENGINE_ACCOUNT_LEN,
        "test setup uses a legacy reward portfolio"
    );

    let wrong_owner = Keypair::new();
    env.ensure_signer_account(wrong_owner.pubkey());
    let market_before = env.svm.get_account(&env.market).unwrap();
    let short_before = env.svm.get_account(&s).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 30,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(wrong_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(s, false),
            AccountMeta::new(c, false),
        ],
        &[&wrong_owner],
    );
    assert!(
        rejected.is_err(),
        "wrong signer must not grow or credit another user's legacy reward portfolio"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "wrong-owner legacy reward rejection rolls back the pre-validation oracle/profile write"
    );
    assert_eq!(
        env.svm.get_account(&s).unwrap(),
        short_before,
        "wrong-owner legacy reward rejection leaves the liquidated portfolio byte-identical"
    );
    assert_eq!(
        env.svm.get_account(&c).unwrap(),
        cranker_before,
        "wrong-owner legacy reward rejection rolls back reward-account realloc and bytes"
    );
    assert_eq!(
        env.svm.get_account(&c).unwrap().data.len(),
        PORTFOLIO_ENGINE_ACCOUNT_LEN,
        "failed liquidation leaves the reward account legacy-sized"
    );

    env.svm.expire_blockhash();
    let accepted = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 30,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(co.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(s, false),
            AccountMeta::new(c, false),
        ],
        &[&co],
    );
    assert!(
        accepted.is_ok(),
        "authorized owner can still grow the legacy reward account and claim the liquidation reward: {accepted:?}"
    );
    assert_eq!(
        env.svm.get_account(&c).unwrap().data.len(),
        env.portfolio_account_len,
        "successful retry grows the legacy reward portfolio"
    );
    assert!(
        env.portfolio_state(c).capital.get() > cranker_cap_before,
        "authorized retry credits a real cranker reward"
    );
}

#[test]
fn v16_bpf_failed_deposit_spl_transfer_rolls_back_engine_credit() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let source = Pubkey::new_unique();
    env.svm
        .set_account(
            source,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, owner.pubkey(), 100),
                owner: Pubkey::new_unique(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let source_before = env.svm.get_account(&source).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let result = env.send(
        env.deposit_ix(portfolio, 100),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );

    assert!(
        result.is_err(),
        "deposit must fail when the token CPI cannot debit the source account"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&portfolio).unwrap(), portfolio_before);
    assert_eq!(env.svm.get_account(&source).unwrap(), source_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    let (_, group) = env.market_state();
    let account = env.portfolio_state(portfolio);
    assert_eq!(group.vault, 0);
    assert_eq!(group.c_tot, 0);
    assert_eq!(account.capital.get(), 0);
}

#[test]
fn v16_bpf_failed_insurance_topup_transfer_preserves_same_intent_retry() {
    let mut env = V16CuEnv::new();
    let ledger = env.insurance_ledger_account();
    let market_id = env.asset_market_id(0);
    let sequences_before = env.control_sequences(0);
    let intent_id = next_control_sequence(sequences_before.insurance_top_up);
    let source = Pubkey::new_unique();
    env.svm
        .set_account(
            source,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, env.admin.pubkey(), 100),
                owner: Pubkey::new_unique(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let ledger_before = env.svm.get_account(&ledger).unwrap();
    let source_before = env.svm.get_account(&source).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let result = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpInsurance {
            intent_id,
            market_id,
            amount: 100,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        &[&env.admin],
    );

    assert!(
        result.is_err(),
        "insurance top-up must fail when the transfer CPI cannot debit the source"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&ledger).unwrap(), ledger_before);
    assert_eq!(env.svm.get_account(&source).unwrap(), source_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    assert_eq!(env.control_sequences(0), sequences_before);
    let (_, group) = env.market_state();
    assert_eq!(group.insurance, 0);
    assert_eq!(group.vault, 0);

    let mut repaired_source = env.svm.get_account(&source).unwrap();
    repaired_source.owner = spl_token::ID;
    env.svm.set_account(source, repaired_source).unwrap();
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpInsurance {
            intent_id,
            market_id,
            amount: 100,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        &[&env.admin],
    )
    .expect("the same insurance intent remains live after rolled-back CPI failure");
    assert_eq!(env.control_sequences(0).insurance_top_up, intent_id);
    let (_, group) = env.market_state();
    assert_eq!(group.insurance, 100);
    assert_eq!(group.vault, 100);
    assert_eq!(env.token_amount(source), 0);
    assert_eq!(env.token_amount(env.vault), 100);
}

#[test]
fn v16_bpf_failed_domain_insurance_topup_transfer_preserves_same_intent_retry() {
    let mut env = V16CuEnv::new();
    let ledger = env.insurance_ledger_account();
    let market_id = env.asset_market_id(0);
    let sequences_before = env.control_sequences(0);
    let intent_id = next_control_sequence(sequences_before.insurance_top_up);
    let source = Pubkey::new_unique();
    env.svm
        .set_account(
            source,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, env.admin.pubkey(), 100),
                owner: Pubkey::new_unique(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let ledger_before = env.svm.get_account(&ledger).unwrap();
    let source_before = env.svm.get_account(&source).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let result = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpInsuranceDomain {
            intent_id,
            market_id,
            domain: 1,
            amount: 100,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        &[&env.admin],
    );

    assert!(
        result.is_err(),
        "domain insurance top-up must fail when the transfer CPI cannot debit the source"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&ledger).unwrap(), ledger_before);
    assert_eq!(env.svm.get_account(&source).unwrap(), source_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    assert_eq!(env.control_sequences(0), sequences_before);
    let (_, group) = env.market_state();
    assert_eq!(group.insurance_domain_budget[1], 0);
    assert_eq!(group.insurance_domain_budget_remaining_total, 0);
    assert_eq!(group.insurance, 0);
    assert_eq!(group.vault, 0);

    let mut repaired_source = env.svm.get_account(&source).unwrap();
    repaired_source.owner = spl_token::ID;
    env.svm.set_account(source, repaired_source).unwrap();
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpInsuranceDomain {
            intent_id,
            market_id,
            domain: 1,
            amount: 100,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        &[&env.admin],
    )
    .expect("the same domain-insurance intent remains live after rolled-back CPI failure");
    assert_eq!(env.control_sequences(0).insurance_top_up, intent_id);
    let (_, group) = env.market_state();
    assert_eq!(group.insurance_domain_budget[1], 100);
    assert_eq!(group.insurance_domain_budget_remaining_total, 100);
    assert_eq!(group.insurance, 100);
    assert_eq!(group.vault, 100);
    assert_eq!(env.token_amount(source), 0);
    assert_eq!(env.token_amount(env.vault), 100);
}

#[test]
fn v16_bpf_failed_backing_topup_transfer_preserves_same_intent_retry() {
    let mut env = V16CuEnv::new();
    let ledger = env.backing_domain_ledger_account();
    let market_id = env.asset_market_id(0);
    let sequences_before = env.control_sequences(0);
    let intent_id = next_control_sequence(sequences_before.backing_top_up);
    let source = Pubkey::new_unique();
    env.svm
        .set_account(
            source,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, env.admin.pubkey(), 100),
                owner: Pubkey::new_unique(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let ledger_before = env.svm.get_account(&ledger).unwrap();
    let source_before = env.svm.get_account(&source).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let result = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpBackingBucket {
            intent_id,
            market_id,
            domain: 1,
            amount: 100,
            expiry_slot: 10,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        &[&env.admin],
    );

    assert!(
        result.is_err(),
        "backing top-up must fail when the transfer CPI cannot debit the source"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&ledger).unwrap(), ledger_before);
    assert_eq!(env.svm.get_account(&source).unwrap(), source_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    assert_eq!(env.control_sequences(0), sequences_before);
    let (_, group) = env.market_state();
    assert_eq!(group.vault, 0);
    assert_eq!(
        group.source_backing_buckets[1].fresh_unliened_backing_num,
        0
    );
    assert_eq!(group.source_credit[1].fresh_reserved_backing_num, 0);

    let mut repaired_source = env.svm.get_account(&source).unwrap();
    repaired_source.owner = spl_token::ID;
    env.svm.set_account(source, repaired_source).unwrap();
    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::TopUpBackingBucket {
            intent_id,
            market_id,
            domain: 1,
            amount: 100,
            expiry_slot: 10,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(source, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        &[&env.admin],
    )
    .expect("the same backing intent remains live after rolled-back CPI failure");
    assert_eq!(env.control_sequences(0).backing_top_up, intent_id);
    let (_, group) = env.market_state();
    assert_eq!(group.vault, 100);
    assert_eq!(
        group.source_backing_buckets[1].fresh_unliened_backing_num,
        100 * BOUND_SCALE
    );
    assert_eq!(
        group.source_credit[1].fresh_reserved_backing_num,
        100 * BOUND_SCALE
    );
    assert_eq!(env.token_amount(source), 0);
    assert_eq!(env.token_amount(env.vault), 100);
}

#[test]
fn v16_bpf_failed_withdraw_spl_transfer_rolls_back_engine_debit() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 100);
    let dest = env.token_account(owner.pubkey(), 0);
    let mut corrupted_vault = env.svm.get_account(&env.vault).unwrap();
    corrupted_vault.owner = Pubkey::new_unique();
    env.svm.set_account(env.vault, corrupted_vault).unwrap();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let dest_before = env.svm.get_account(&dest).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let result = env.send(
        env.withdraw_ix(portfolio, 40),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );

    assert!(
        result.is_err(),
        "withdraw must fail when the token CPI cannot debit the vault account"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&portfolio).unwrap(), portfolio_before);
    assert_eq!(env.svm.get_account(&dest).unwrap(), dest_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    let (_, group) = env.market_state();
    let account = env.portfolio_state(portfolio);
    assert_eq!(group.vault, 100);
    assert_eq!(group.c_tot, 100);
    assert_eq!(account.capital.get(), 100);
    assert_eq!(env.token_amount(dest), 0);
}

#[test]
fn v16_bpf_overwithdraw_engine_error_rolls_back_public_route() {
    let mut env = V16CuEnv::new();
    let thin_owner = Keypair::new();
    let rich_owner = Keypair::new();
    let thin_portfolio = env.create_portfolio(&thin_owner);
    let rich_portfolio = env.create_portfolio(&rich_owner);
    env.deposit(&thin_owner, thin_portfolio, 100);
    env.deposit(&rich_owner, rich_portfolio, 10_000);
    let dest = env.token_account(thin_owner.pubkey(), 0);
    let attempted_withdraw = 1_000;
    assert!(
        env.token_amount(env.vault) >= attempted_withdraw,
        "setup keeps the vault precheck satisfied so the rejection comes from engine account admission"
    );

    let market_before = env.svm.get_account(&env.market).unwrap();
    let thin_before = env.svm.get_account(&thin_portfolio).unwrap();
    let rich_before = env.svm.get_account(&rich_portfolio).unwrap();
    let dest_before = env.svm.get_account(&dest).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let rejected = env.send(
        env.withdraw_ix(thin_portfolio, attempted_withdraw as u128),
        vec![
            AccountMeta::new(thin_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(thin_portfolio, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&thin_owner],
    );

    assert!(
        rejected.is_err(),
        "over-withdraw must propagate the engine's nonzero return as an instruction error"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "engine-rejected over-withdraw leaves global vault/c_tot accounting unchanged"
    );
    assert_eq!(
        env.svm.get_account(&thin_portfolio).unwrap(),
        thin_before,
        "engine-rejected over-withdraw leaves the charged portfolio unchanged"
    );
    assert_eq!(
        env.svm.get_account(&rich_portfolio).unwrap(),
        rich_before,
        "engine-rejected over-withdraw cannot borrow backing from an unrelated depositor"
    );
    assert_eq!(
        env.svm.get_account(&dest).unwrap(),
        dest_before,
        "engine-rejected over-withdraw transfers no tokens to the user"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "engine-rejected over-withdraw leaves vault custody unchanged"
    );
    assert_eq!(env.portfolio_state(thin_portfolio).capital.get(), 100);
    assert_eq!(env.portfolio_state(rich_portfolio).capital.get(), 10_000);
    assert_eq!(env.token_amount(dest), 0);
    assert_eq!(env.token_amount(env.vault), 10_100);

    let retry_cu = env
        .send(
            env.withdraw_ix(thin_portfolio, 40),
            vec![
                AccountMeta::new(thin_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(thin_portfolio, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&thin_owner],
        )
        .expect("valid smaller withdraw remains live after the engine-rejected attempt");
    assert_cu_within("over-withdraw rollback retry", retry_cu, CUSTODY_CU_LIMIT);
    let (_, group) = env.market_state();
    assert_eq!(group.vault, 10_060);
    assert_eq!(group.c_tot, 10_060);
    assert_eq!(env.portfolio_state(thin_portfolio).capital.get(), 60);
    assert_eq!(env.portfolio_state(rich_portfolio).capital.get(), 10_000);
    assert_eq!(env.token_amount(dest), 40);
    assert_eq!(env.token_amount(env.vault), 10_060);
}

#[test]
fn v16_bpf_tradenocpi_rejects_invalid_final_market_shape() {
    let mut env = V16CuEnv::new();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 1_000_000);

    env.mutate_market(|_, group| {
        group.insurance_domain_budget[0] = group.insurance.saturating_add(1);
    });
    let before_market = env.svm.get_account(&env.market).unwrap().data;

    let result = env.try_trade_asset_with_cu(
        0,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        100,
        0,
    );

    assert!(
        result.is_err(),
        "TradeNoCpi must reject instead of persisting an invalid market shape"
    );
    let after_market = env.svm.get_account(&env.market).unwrap().data;
    assert_eq!(
        after_market, before_market,
        "failed TradeNoCpi must roll back market data"
    );
}

// Public auto-crank liveness sweep: a current, solvent, under-margin account must make progress in one
// public instruction without observations. The engine chooses a proper partial close and restores health;
// Public auto-crank ordering sweep: a stale budgeted liquidation tx may land after
// the account's selected step has changed to a committed-state refresh. In that
// case the tx may make refresh progress and apply a valid unrelated oracle update,
// Public multi-observation differential: keepers may submit authenticated
// observations in any order. Reordering the same set must not change either
// LoF/DoS sweep: committed-state refresh is valid only when the selected
// engine asset has no pending wrapper-side mark. If a public keeper can omit
// the selected asset observation while another asset made the account stale,
// the engine fallback would accrue the selected asset at its old committed
// price and consume the slot, making the pending mark impossible to apply in
// that slot. That lets out-of-order keepers starve mark/funding progress by
#[test]
fn v16_bpf_no_cranker_liquidation_rejects_invalid_final_market_shape() {
    let mut env = V16CuEnv::new();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 250);
    env.configure_ewma_mark_with_cu(0, 100, 1, 0);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(1);
    env.push_ewma_mark_with_cu(1, 300);
    env.crank(
        short_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
    );
    assert_ne!(
        health_cert(&env.portfolio_state(short_account)).certified_liq_deficit,
        0,
        "the public refresh step must make the next selected action a liquidation"
    );
    env.mutate_market(|_, group| {
        group.insurance_domain_budget[0] = group.insurance.saturating_add(1);
    });
    let before_market = env.svm.get_account(&env.market).unwrap().data;
    let before_short = env.svm.get_account(&short_account).unwrap().data;

    let result = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short_account, false),
        ],
        &[],
    );

    assert!(
        result.is_err(),
        "no-cranker liquidation must reject instead of persisting an invalid market shape"
    );
    let after_market = env.svm.get_account(&env.market).unwrap().data;
    let after_short = env.svm.get_account(&short_account).unwrap().data;
    assert_eq!(
        after_market, before_market,
        "failed no-cranker liquidation must roll back market data"
    );
    assert_eq!(
        after_short, before_short,
        "failed no-cranker liquidation must roll back portfolio data"
    );
}

#[test]
fn v16_bpf_cranker_reward_liquidation_rejects_invalid_shape_without_paying_reward() {
    let mut env = V16CuEnv::new();
    env.update_liquidation_fee_policy_with_cu(10_000);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let cranker_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    let cranker_account = env.create_portfolio(&cranker_owner);
    env.deposit(&long_owner, long_account, 1_000_000);
    env.deposit(&short_owner, short_account, 250);
    env.configure_ewma_mark_with_cu(0, 100, 1, 0);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(1);
    env.push_ewma_mark_with_cu(1, 300);
    env.crank(
        short_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
    );
    assert_ne!(
        health_cert(&env.portfolio_state(short_account)).certified_liq_deficit,
        0,
        "the public refresh step must make the next selected action a liquidation"
    );
    env.mutate_market(|_, group| {
        group.config.liquidation_fee_bps = 10_000;
        group.config.liquidation_fee_cap = 1;
        group.insurance_domain_budget[0] = group.insurance.saturating_add(1_000_000);
    });
    let before_market = env.svm.get_account(&env.market).unwrap().data;
    let before_short = env.svm.get_account(&short_account).unwrap().data;
    let before_cranker = env.svm.get_account(&cranker_account).unwrap().data;

    let result = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(cranker_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short_account, false),
            AccountMeta::new(cranker_account, false),
        ],
        &[&cranker_owner],
    );

    assert!(
        result.is_err(),
        "cranker-reward liquidation must reject instead of persisting an invalid market shape"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before_market,
        "failed cranker-reward liquidation must roll back market data"
    );
    assert_eq!(
        env.svm.get_account(&short_account).unwrap().data,
        before_short,
        "failed cranker-reward liquidation must roll back liquidated portfolio data"
    );
    assert_eq!(
        env.svm.get_account(&cranker_account).unwrap().data,
        before_cranker,
        "failed cranker-reward liquidation must not pay the cranker portfolio"
    );
}

#[test]
fn v16_bpf_failed_close_resolved_transfer_rolls_back_payout_state() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);
    env.resolve();
    let dest = env.token_account(owner.pubkey(), 0);
    let mut corrupted_vault = env.svm.get_account(&env.vault).unwrap();
    corrupted_vault.owner = Pubkey::new_unique();
    env.svm.set_account(env.vault, corrupted_vault).unwrap();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let dest_before = env.svm.get_account(&dest).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let result = env.send(
        ProgInstruction::CloseResolved {
            fee_rate_per_slot: 0,
        },
        vec![
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );

    assert!(
        result.is_err(),
        "close-resolved must fail when the payout transfer CPI fails"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&portfolio).unwrap(), portfolio_before);
    assert_eq!(env.svm.get_account(&dest).unwrap(), dest_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    let (_, group) = env.market_state();
    let account = env.portfolio_state(portfolio);
    assert_eq!(group.vault, 1_000);
    assert_eq!(group.c_tot, 1_000);
    assert_eq!(account.capital.get(), 1_000);
    assert!(
        !resolved_receipt(&account).present,
        "failed payout must not persist a paid/finalized receipt"
    );
    assert_eq!(env.token_amount(dest), 0);
}

#[test]
fn v16_bpf_failed_claim_resolved_topup_rolls_back_receipt_and_ledger() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    {
        let mut market_account = env.svm.get_account(&env.market).expect("market account");
        let mut portfolio_account = env.svm.get_account(&portfolio).expect("portfolio account");
        let (cfg, mut group) = state::read_market(&market_account.data).unwrap();
        let mut account = state::read_portfolio(&portfolio_account.data).unwrap();
        group.mode = MarketModeV16::Resolved;
        group.resolved_slot = 1;
        group.current_slot = 1;
        group.vault = 60;
        group.payout_snapshot_captured = true;
        group.payout_snapshot = 100;
        group.resolved_payout_ledger = ResolvedPayoutLedgerV16 {
            snapshot_residual: 100,
            terminal_claim_exact_receipts_num: 100 * BOUND_SCALE,
            terminal_claim_bound_unreceipted_num: 0,
            current_payout_rate_num: 100 * BOUND_SCALE,
            current_payout_rate_den: 100 * BOUND_SCALE,
            snapshot_slot: 1,
            payout_halted: false,
            finalized: false,
        };
        account.resolved_payout_receipt =
            percolator::ResolvedPayoutReceiptV16Account::from_runtime(&ResolvedPayoutReceiptV16 {
                present: true,
                prior_bound_contribution_num: 100 * BOUND_SCALE,
                live_released_face_at_receipt: 0,
                terminal_positive_claim_face: 100,
                paid_effective: 40,
                finalized: false,
            });
        state::write_market(&mut market_account.data, &cfg, &group).unwrap();
        state::write_portfolio(&mut portfolio_account.data, &account).unwrap();
        env.svm.set_account(env.market, market_account).unwrap();
        env.svm.set_account(portfolio, portfolio_account).unwrap();
    }
    env.set_token_account_amount(env.vault, env.mint, env.vault_authority, 60);
    let clean_vault = env.svm.get_account(&env.vault).unwrap();

    let dest = env.token_account_for_mint(env.mint, owner.pubkey(), 0);
    let mut corrupted_vault = clean_vault.clone();
    corrupted_vault.owner = Pubkey::new_unique();
    env.svm.set_account(env.vault, corrupted_vault).unwrap();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let dest_before = env.svm.get_account(&dest).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let result = env.send(
        ProgInstruction::ClaimResolvedPayoutTopup,
        vec![
            AccountMeta::new_readonly(owner.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[],
    );

    assert!(
        result.is_err(),
        "resolved payout top-up must fail when post-mutation vault validation fails"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&portfolio).unwrap(), portfolio_before);
    assert_eq!(env.svm.get_account(&dest).unwrap(), dest_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    let (_, group) = env.market_state();
    let account = env.portfolio_state(portfolio);
    assert_eq!(group.vault, 60);
    assert_eq!(group.resolved_payout_ledger.snapshot_residual, 100);
    assert_eq!(
        group
            .resolved_payout_ledger
            .terminal_claim_exact_receipts_num,
        100 * BOUND_SCALE
    );
    assert_eq!(resolved_receipt(&account).paid_effective, 40);
    assert!(
        !resolved_receipt(&account).finalized,
        "failed top-up must leave the pending receipt claimable"
    );
    assert_eq!(env.token_amount(dest), 0);

    env.svm.set_account(env.vault, clean_vault).unwrap();
    env.svm.expire_blockhash();
    let cu = env.claim_resolved_payout_topup_with_cu(owner.pubkey(), portfolio, dest);
    assert_cu_within(
        "ClaimResolvedPayoutTopup rollback retry",
        cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(env.token_amount(dest), 60);
    let (_, group) = env.market_state();
    let account = env.portfolio_state(portfolio);
    assert_eq!(group.vault, 0);
    assert_eq!(resolved_receipt(&account).paid_effective, 100);
    assert!(resolved_receipt(&account).finalized);
}

#[test]
fn v16_bpf_failed_terminal_insurance_withdraw_rolls_back_market_and_ledger() {
    let mut env = V16CuEnv::new();
    env.top_up_insurance(100);
    env.resolve();
    let ledger = env.insurance_ledger_account();
    let dest = env.token_account(env.admin.pubkey(), 0);
    let mut corrupted_vault = env.svm.get_account(&env.vault).unwrap();
    corrupted_vault.owner = Pubkey::new_unique();
    env.svm.set_account(env.vault, corrupted_vault).unwrap();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let ledger_before = env.svm.get_account(&ledger).unwrap();
    let dest_before = env.svm.get_account(&dest).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let result = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawInsurance { amount: 40 },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        &[&env.admin],
    );

    assert!(
        result.is_err(),
        "terminal insurance withdraw must fail when the transfer CPI fails"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&ledger).unwrap(), ledger_before);
    assert_eq!(env.svm.get_account(&dest).unwrap(), dest_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    let (_, group) = env.market_state();
    assert_eq!(group.insurance, 100);
    assert_eq!(group.vault, 100);
    assert_eq!(env.token_amount(dest), 0);
}

#[test]
fn v16_bpf_failed_backing_withdraw_transfer_rolls_back_bucket_and_ledger() {
    let mut env = V16CuEnv::new();
    let ledger = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger, 1, 100, 10);
    let dest = env.token_account(env.admin.pubkey(), 0);
    let mut corrupted_vault = env.svm.get_account(&env.vault).unwrap();
    corrupted_vault.owner = Pubkey::new_unique();
    env.svm.set_account(env.vault, corrupted_vault).unwrap();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let ledger_before = env.svm.get_account(&ledger).unwrap();
    let dest_before = env.svm.get_account(&dest).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let market_id = env.asset_market_id(0);
    let result = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawBackingBucket {
            domain: 1,
            market_id,
            amount: 40,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
            AccountMeta::new(ledger, false),
        ],
        &[&env.admin],
    );

    assert!(
        result.is_err(),
        "backing withdraw must fail when the transfer CPI cannot debit the vault"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&ledger).unwrap(), ledger_before);
    assert_eq!(env.svm.get_account(&dest).unwrap(), dest_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    let (_, group) = env.market_state();
    assert_eq!(group.vault, 100);
    assert_eq!(
        group.source_backing_buckets[1].fresh_unliened_backing_num,
        100 * BOUND_SCALE
    );
    assert_eq!(
        group.source_credit[1].fresh_reserved_backing_num,
        100 * BOUND_SCALE
    );
    assert_eq!(env.token_amount(dest), 0);
}

#[test]
fn v16_bpf_failed_backing_earnings_withdraw_rolls_back_bucket_and_ledger() {
    let mut env = V16CuEnv::new();
    let ledger = env.backing_domain_ledger_account();
    env.top_up_backing_bucket_with_ledger_with_cu(ledger, 1, 100, 10);
    env.mutate_market(|_, group| {
        group.source_backing_buckets[1].utilization_fee_earnings = 30;
        group.vault += 30;
    });
    env.set_token_account_amount(env.vault, env.mint, env.vault_authority, 130);
    env.sync_backing_domain_ledger_with_cu(ledger, 1);

    let dest = env.token_account(env.admin.pubkey(), 0);
    let mut corrupted_vault = env.svm.get_account(&env.vault).unwrap();
    corrupted_vault.owner = Pubkey::new_unique();
    env.svm.set_account(env.vault, corrupted_vault).unwrap();

    let market_before = env.svm.get_account(&env.market).unwrap();
    let ledger_before = env.svm.get_account(&ledger).unwrap();
    let dest_before = env.svm.get_account(&dest).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let market_id = env.asset_market_id(0);
    let result = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::WithdrawBackingBucketEarnings {
            domain: 1,
            market_id,
            amount: 10,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(ledger, false),
            AccountMeta::new(dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&env.admin],
    );

    assert!(
        result.is_err(),
        "backing earnings withdraw must fail when the transfer CPI cannot debit the vault"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&ledger).unwrap(), ledger_before);
    assert_eq!(env.svm.get_account(&dest).unwrap(), dest_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    let (_, group) = env.market_state();
    assert_eq!(group.vault, 130, "vault accounting rolled back");
    assert_eq!(
        group.source_backing_buckets[1].utilization_fee_earnings, 30,
        "earnings counter rolled back"
    );
    let ledger_state =
        state::read_backing_domain_ledger(&env.svm.get_account(&ledger).unwrap().data).unwrap();
    assert_eq!(
        ledger_state.last_observed_bucket_earnings_atoms, 30,
        "ledger observed earnings rolled back"
    );
    assert_eq!(
        ledger_state.total_earnings_withdrawn_atoms, 0,
        "ledger withdraw total rolled back"
    );
    assert_eq!(env.token_amount(dest), 0);
}
