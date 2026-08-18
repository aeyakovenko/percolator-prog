//! INV-021 - account creation, reallocation, close, rent, and lamport safety.
//!
//! These tests exercise the public InitPortfolio and ClosePortfolio routes with real
//! LiteSVM accounts. The invariant is that successful realloc/close/reuse transitions preserve
//! market accounting and route rent only through the market slab, while rejected owner,
//! reinit, foreign-account, alias, and non-rent-exempt paths roll back every program byte,
//! lamport, and SPL-token effect exactly. The issue-404 regression creates the hostile account
//! through the System Program in the same transaction; no program-owned state is injected.

use super::*;

fn inv021_init_portfolio_ix(env: &V16CuEnv, owner: Pubkey, portfolio: Pubkey) -> Instruction {
    Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(owner, true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        data: ProgInstruction::InitPortfolio.encode(),
    }
}

#[test]
fn v16_program_issue404_zero_lamport_system_create_cannot_leave_phantom_portfolio() {
    let mut env = V16CuEnv::new();
    let ghost = Keypair::new();
    let market_before = env.svm.get_account(&env.market).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let count_before = env.market_state().1.materialized_portfolio_count;
    let create = system_instruction::create_account(
        &env.payer.pubkey(),
        &ghost.pubkey(),
        0,
        env.portfolio_account_len as u64,
        &env.program_id,
    );
    let init = inv021_init_portfolio_ix(&env, env.payer.pubkey(), ghost.pubkey());

    env.svm.expire_blockhash();
    let rejected = send_raw_ixs(
        &mut env.svm,
        &env.payer,
        vec![heap_ix(), cu_ix(), create, init],
        &[&ghost],
    );
    assert!(
        rejected
            .as_ref()
            .is_err_and(|error| error.contains("Custom(31)")),
        "zero-lamport System CreateAccount + InitPortfolio must reject before registration"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected transient init must roll back the market registration exactly"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected transient init cannot affect custody"
    );
    assert_eq!(
        env.market_state().1.materialized_portfolio_count,
        count_before
    );
    assert!(
        env.svm.get_account(&ghost.pubkey()).is_none(),
        "the failed transaction must roll back the System Program creation"
    );
}

#[test]
fn v16_program_issue404_final_size_rent_boundary_rejects_then_remains_live() {
    let mut env = V16CuEnv::new();
    let rent = env.svm.get_sysvar::<solana_sdk::rent::Rent>();
    let small_len = env.portfolio_account_len / 3;
    let required_len = env.portfolio_account_len;
    let small_rent = rent.minimum_balance(small_len);
    let required_rent = rent.minimum_balance(required_len);
    assert!(
        small_rent < required_rent,
        "fixture must become underfunded only after canonical reallocation"
    );

    let underfunded = Keypair::new();
    let market_before = env.svm.get_account(&env.market).unwrap();
    let create_underfunded = system_instruction::create_account(
        &env.payer.pubkey(),
        &underfunded.pubkey(),
        small_rent,
        small_len as u64,
        &env.program_id,
    );
    let init_underfunded = inv021_init_portfolio_ix(&env, env.payer.pubkey(), underfunded.pubkey());
    env.svm.expire_blockhash();
    let rejected = send_raw_ixs(
        &mut env.svm,
        &env.payer,
        vec![heap_ix(), cu_ix(), create_underfunded, init_underfunded],
        &[&underfunded],
    );
    assert!(
        rejected
            .as_ref()
            .is_err_and(|error| error.contains("Custom(31)")),
        "rent-exempt-at-old-size account must reject after underfunded canonical realloc: {rejected:?}"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert!(env.svm.get_account(&underfunded.pubkey()).is_none());

    let funded = Keypair::new();
    let create_funded = system_instruction::create_account(
        &env.payer.pubkey(),
        &funded.pubkey(),
        required_rent,
        required_len as u64,
        &env.program_id,
    );
    let init_funded = inv021_init_portfolio_ix(&env, env.payer.pubkey(), funded.pubkey());
    env.svm.expire_blockhash();
    let init_cu = send_raw_ixs(
        &mut env.svm,
        &env.payer,
        vec![heap_ix(), cu_ix(), create_funded, init_funded],
        &[&funded],
    )
    .expect("exact-final-rent account remains initializable");
    assert_cu_within(
        "INV-021 exact-rent InitPortfolio",
        init_cu,
        CUSTODY_CU_LIMIT,
    );
    let funded_account = env.svm.get_account(&funded.pubkey()).unwrap();
    assert_eq!(funded_account.lamports, required_rent);
    assert!(rent.is_exempt(funded_account.lamports, funded_account.data.len()));
    assert_eq!(env.market_state().1.materialized_portfolio_count, 1);

    env.svm.expire_blockhash();
    let close_cu = env
        .send(
            env.close_portfolio_ix(funded.pubkey()),
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(funded.pubkey(), false),
            ],
            &[],
        )
        .expect("exact-rent empty portfolio remains closeable");
    assert_cu_within(
        "INV-021 exact-rent ClosePortfolio",
        close_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(env.market_state().1.materialized_portfolio_count, 0);
}

#[test]
fn v16_program_issue404_atomic_close_reinit_rolls_back_without_phantom_count() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    assert_eq!(env.market_state().1.materialized_portfolio_count, 1);

    let close = Instruction {
        program_id: env.program_id,
        accounts: vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        data: env.close_portfolio_ix(portfolio).encode(),
    };
    let reinit = inv021_init_portfolio_ix(&env, owner.pubkey(), portfolio);
    env.svm.expire_blockhash();
    let rejected = send_raw_ixs(
        &mut env.svm,
        &env.payer,
        vec![heap_ix(), cu_ix(), close, reinit],
        &[&owner],
    );
    assert!(
        rejected
            .as_ref()
            .is_err_and(|error| error.contains("Custom(31)")),
        "atomic close/reinit must reject at the zero-lamport reinit: {rejected:?}"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&portfolio).unwrap(), portfolio_before);
    assert_eq!(env.market_state().1.materialized_portfolio_count, 1);

    let close_cu = env.close_portfolio_with_cu(&owner, portfolio);
    assert_cu_within(
        "INV-021 post-rollback ClosePortfolio",
        close_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(env.market_state().1.materialized_portfolio_count, 0);
}

#[test]
fn v16_program_undersized_init_grows_account_then_close_sweeps_rent_exactly() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    env.ensure_signer_account(owner.pubkey());

    let small_len = env.portfolio_account_len / 3;
    let portfolio = env.program_account(small_len);
    let pre_init_lamports = env.svm.get_account(&portfolio).unwrap().lamports;
    assert_eq!(
        env.market_state().1.materialized_portfolio_count,
        0,
        "control starts with no materialized portfolios",
    );

    env.svm.expire_blockhash();
    let init_cu = env
        .send(
            ProgInstruction::InitPortfolio,
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
            ],
            &[&owner],
        )
        .expect("public InitPortfolio grows the undersized account");
    assert_cu_within(
        "INV-021 undersized InitPortfolio",
        init_cu,
        CUSTODY_CU_LIMIT,
    );
    assert!(
        env.svm.get_account(&portfolio).unwrap().data.len() >= env.portfolio_account_len,
        "InitPortfolio grows to the canonical portfolio size",
    );
    assert_eq!(
        env.market_state().1.materialized_portfolio_count,
        1,
        "successful creation materializes exactly one portfolio",
    );

    env.deposit(&owner, portfolio, 1_000);
    let dest = env.withdraw(&owner, portfolio, 1_000);
    assert_eq!(
        env.token_amount(dest),
        1_000,
        "grown account remains a usable portfolio before close",
    );
    assert_eq!(
        env.portfolio_state(portfolio).capital.get(),
        0,
        "portfolio is economically empty before close",
    );

    let market_lamports_before_close = env.svm.get_account(&env.market).unwrap().lamports;
    let portfolio_lamports_before_close = env.svm.get_account(&portfolio).unwrap().lamports;
    assert_eq!(
        portfolio_lamports_before_close, pre_init_lamports,
        "InitPortfolio did not consume or redirect the account rent",
    );
    let close_cu = env.close_portfolio_with_cu(&owner, portfolio);
    assert_cu_within(
        "INV-021 ClosePortfolio after realloc",
        close_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_eq!(
        env.market_state().1.materialized_portfolio_count,
        0,
        "close deregisters the single materialized portfolio",
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().lamports,
        market_lamports_before_close + portfolio_lamports_before_close,
        "ClosePortfolio routes rent only into the market slab",
    );
    if let Some(closed) = env.svm.get_account(&portfolio) {
        assert_eq!(closed.lamports, 0, "closed portfolio rent is swept");
        assert!(
            closed.data.is_empty() || !state::is_initialized(&closed.data),
            "closed portfolio is dematerialized",
        );
    }
}

#[test]
fn v16_program_funded_close_rejects_exact_rollback_and_remains_withdrawable() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 400_000);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.token_amount(env.vault);
    let capital_before = env.portfolio_state(portfolio).capital.get();
    assert!(capital_before > 0, "control portfolio is funded");

    env.svm.expire_blockhash();
    let rejected = env.send(
        env.close_portfolio_ix(portfolio),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&owner],
    );
    assert!(
        rejected.is_err(),
        "ClosePortfolio must reject while user capital remains",
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "funded-close reject rolls back market bytes and lamports",
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "funded-close reject rolls back portfolio bytes and lamports",
    );
    assert_eq!(
        env.token_amount(env.vault),
        vault_before,
        "funded-close reject leaves custody tokens untouched",
    );

    let dest = env.withdraw(&owner, portfolio, 400_000);
    assert_eq!(
        env.token_amount(dest),
        400_000,
        "owner can still withdraw after the rejected close",
    );
}

#[test]
fn v16_program_uninitialized_close_rejects_exact_rollback() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    env.ensure_signer_account(owner.pubkey());
    let raw = env.program_account(env.portfolio_account_len);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let raw_before = env.svm.get_account(&raw).unwrap();
    assert_eq!(
        env.market_state().1.materialized_portfolio_count,
        0,
        "control starts with no materialized portfolios",
    );

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::ClosePortfolio {
            portfolio_id: 0,
            expected_sequence: 0,
            position_epoch: 0,
        },
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(raw, false),
        ],
        &[&owner],
    );
    assert!(
        rejected.is_err(),
        "ClosePortfolio must reject a raw never-initialized account",
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "raw-close reject does not move rent or counters",
    );
    assert_eq!(
        env.svm.get_account(&raw).unwrap(),
        raw_before,
        "raw-close reject leaves supplied account untouched",
    );
}

#[test]
fn v16_program_stale_undersized_init_rejects_exact_realloc_rollback() {
    let mut env = V16CuEnv::new();
    env.configure_permissionless_resolve_with_cu(5, 5);
    env.configure_auth_mark_with_cu(0, 100);

    env.svm.warp_to_slot(3);
    env.push_auth_mark_with_cu(3, 100);
    env.svm.warp_to_slot(40);

    let creator = Keypair::new();
    env.ensure_signer_account(creator.pubkey());
    let small = env.program_account(env.portfolio_account_len / 2);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let small_before = env.svm.get_account(&small).unwrap();

    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(small, false),
        ],
        &[&creator],
    );
    assert!(
        rejected.is_err(),
        "stale InitPortfolio must reject after the permissionless resolve window",
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "stale InitPortfolio reject rolls back market bytes and lamports",
    );
    assert_eq!(
        env.svm.get_account(&small).unwrap(),
        small_before,
        "stale InitPortfolio reject rolls back pre-validation realloc",
    );
    assert_eq!(
        env.market_state().1.materialized_portfolio_count,
        0,
        "failed stale init cannot create a terminal CloseSlab blocker",
    );
}

// not permissionlessly creatable), so a parasitic zero-activity asset earns ZERO. This guards the fix.
#[test]
fn v16_attack_bug113_maintenance_fee_siphon_to_parasitic_asset() {
    // capacity 1 (asset-1 is appended at index == configured_slots, growing to 2), maintenance fee 58/slot.
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 10_000, 10_000, 10_000, 58,
    );
    env.update_market_init_fee_policy_with_cu(1); // permissionless create enabled (nonzero fee)

    // Honest depositor H on the real market (asset 0).
    let h_owner = Keypair::new();
    let h = env.create_portfolio(&h_owner);
    env.deposit(&h_owner, h, 100_000_000);

    // Attacker permissionlessly appends a do-nothing asset 1 with ITSELF as insurance_operator.
    let attacker = Keypair::new();
    env.ensure_signer_account(attacker.pubkey());
    env.svm.warp_to_slot(1);
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
    let (_, g_pre) = env.market_state();
    assert_eq!(
        g_pre.assets[1].lifecycle,
        AssetLifecycleV16::Active,
        "parasite asset-1 active"
    );
    // asset-1 domains (2 = long, 3 = short) start empty: it has no positions and was never funded.
    assert_eq!(g_pre.insurance_domain_budget[2], 0);
    assert_eq!(g_pre.insurance_domain_budget[3], 0);

    // Charge H's maintenance fee (58 * 10 slots = 580).
    env.svm.warp_to_slot(10);
    let h_cap_before = env.portfolio_state(h).capital.get();
    env.svm.expire_blockhash();
    env.sync_maintenance_fee_with_cu(h, None, 10);
    let fee_paid = h_cap_before - env.portfolio_state(h).capital.get();
    assert!(
        fee_paid > 0,
        "H actually paid a maintenance fee (non-vacuous)"
    );

    // SECURITY PROPERTY: the parasitic zero-activity asset-1 must have captured NOTHING of H's fee.
    let (_, g) = env.market_state();
    let parasite_share = g.insurance_domain_budget[2] + g.insurance_domain_budget[3];
    assert_eq!(
        parasite_share, 0,
        "BUG #113: parasitic asset-1 captured {parasite_share} of H's {fee_paid} maintenance fee"
    );

    // Positive side of the fix: the whole retained fee (no cranker here) landed in ASSET-0's insurance
    // domains (0 = long, 1 = short), and the domain-budget aggregate stays consistent with group.insurance
    // (a desync here would be a "weird state" — engine fee charge + wrapper credit double-counting).
    let asset0_before = g_pre.insurance_domain_budget[0] + g_pre.insurance_domain_budget[1];
    let asset0_after = g.insurance_domain_budget[0] + g.insurance_domain_budget[1];
    assert_eq!(
        asset0_after - asset0_before,
        fee_paid,
        "the full retained maintenance fee must land in asset-0 insurance"
    );
    assert_domain_budget_remaining_total_consistent(
        &g,
        "after #113-fix maintenance fee to asset-0",
    );

    // And the attacker must not be able to withdraw any of it as that asset's insurance_operator.
    env.svm.expire_blockhash();
    let siphon = env.try_withdraw_insurance_asset_with_authority(&attacker, 1, 1);
    assert!(
        siphon.is_err(),
        "BUG #113: attacker siphoned honest maintenance fees via WithdrawInsuranceAsset(asset 1)"
    );
}

#[test]
fn v16_bpf_underfunded_flat_sync_sweeps_remaining_capital_once() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 10_000, 10_000, 10_000, 40,
    );
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_portfolio = env.create_portfolio(&long_owner);
    let short_portfolio = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_portfolio, 1);
    env.deposit(&short_owner, short_portfolio, 10_000);

    env.svm.warp_to_slot(10);
    let market_lamports_before_close = env.svm.get_account(&env.market).unwrap().lamports;
    let long_lamports_before_close = env.svm.get_account(&long_portfolio).unwrap().lamports;
    env.sync_maintenance_fee_with_cu(long_portfolio, None, 10);
    let (_, group_after_flat_sync) = env.market_state();
    assert_eq!(
        group_after_flat_sync.insurance, 1,
        "underfunded flat sync sweeps the remaining capital into insurance"
    );
    assert_eq!(group_after_flat_sync.materialized_portfolio_count, 1);
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().lamports,
        market_lamports_before_close + long_lamports_before_close,
        "dust-closed portfolio rent should move into the market slab"
    );
    if let Some(closed_long_account) = env.svm.get_account(&long_portfolio) {
        assert_eq!(closed_long_account.lamports, 0);
        assert!(
            closed_long_account.data.is_empty()
                || !state::is_initialized(&closed_long_account.data)
        );
    }

    let fresh_long_portfolio = env.create_portfolio(&long_owner);
    env.deposit(&long_owner, fresh_long_portfolio, 1_000);
    env.trade_with_cu(
        &long_owner,
        fresh_long_portfolio,
        &short_owner,
        short_portfolio,
        POS_SCALE as i128,
        100,
        0,
    );
    env.crank(
        fresh_long_portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
    );
    let (_, before_nonflat_sync) = env.market_state();
    assert_eq!(before_nonflat_sync.assets[0].slot_last, 1);
    let insurance_before_nonflat_sync = before_nonflat_sync.insurance;

    let fresh_long_lamports_before_sync =
        env.svm.get_account(&fresh_long_portfolio).unwrap().lamports;
    env.sync_maintenance_fee_with_cu(fresh_long_portfolio, None, 11);
    let (_, group_after_nonflat_sync) = env.market_state();
    let long_after_nonflat_sync = env.portfolio_state(fresh_long_portfolio);
    assert_eq!(long_after_nonflat_sync.capital.get(), 1_000);
    assert_eq!(long_after_nonflat_sync.last_fee_slot.get(), 10);
    assert_eq!(
        env.svm
            .get_account(&fresh_long_portfolio)
            .expect("non-flat portfolio should remain allocated")
            .lamports,
        fresh_long_lamports_before_sync
    );
    assert_eq!(
        group_after_nonflat_sync.insurance, insurance_before_nonflat_sync,
        "later deposits are not charged for an already-swept empty interval"
    );
}

#[test]
fn v16_attack_underfunded_sync_with_cranker_reward_still_closes_payer() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 10_000, 10_000, 10_000, 40,
    );
    env.update_maintenance_fee_policy_with_cu(5_000);
    let payer_owner = Keypair::new();
    let cranker_owner = Keypair::new();
    let payer_portfolio = env.create_portfolio(&payer_owner);
    let cranker_portfolio = env.create_portfolio(&cranker_owner);
    env.deposit(&payer_owner, payer_portfolio, 3);

    env.svm.warp_to_slot(10);
    let market_lamports_before_close = env.svm.get_account(&env.market).unwrap().lamports;
    let payer_lamports_before_close = env.svm.get_account(&payer_portfolio).unwrap().lamports;
    let cranker_cap_before = env.portfolio_state(cranker_portfolio).capital.get();
    let sync_cu = env.sync_maintenance_fee_with_cu(payer_portfolio, Some(cranker_portfolio), 10);
    println!("v16 underfunded SyncMaintenanceFee with cranker reward CU: {sync_cu}");
    assert_cu_within(
        "underfunded SyncMaintenanceFee with cranker reward",
        sync_cu,
        CUSTODY_CU_LIMIT,
    );

    let (_, group_after_sync) = env.market_state();
    assert_eq!(
        group_after_sync.insurance, 2,
        "underfunded sync retains the non-cranker share in insurance"
    );
    assert_eq!(
        env.portfolio_state(cranker_portfolio).capital.get(),
        cranker_cap_before + 1,
        "cranker receives only the configured share of the swept dust fee"
    );
    assert_eq!(
        group_after_sync.materialized_portfolio_count, 1,
        "dust payer is still closed even when a cranker reward is paid"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().lamports,
        market_lamports_before_close + payer_lamports_before_close,
        "dust-closed payer rent moves into the market slab"
    );
    if let Some(closed_payer_account) = env.svm.get_account(&payer_portfolio) {
        assert_eq!(closed_payer_account.lamports, 0);
        assert!(
            closed_payer_account.data.is_empty()
                || !state::is_initialized(&closed_payer_account.data)
        );
    }
}

#[test]
fn v16_bpf_nonflat_fee_sync_settles_hidden_loss_before_sweeping_fee() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 10_000, 10_000, 10_000, 100,
    );
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_portfolio = env.create_portfolio(&long_owner);
    let short_portfolio = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_portfolio, 100);
    env.deposit(&short_owner, short_portfolio, 1_000);
    env.trade_with_cu(
        &long_owner,
        long_portfolio,
        &short_owner,
        short_portfolio,
        POS_SCALE as i128,
        100,
        0,
    );

    let long_before_move = env.portfolio_state(long_portfolio);
    assert_eq!(long_before_move.capital.get(), 100);
    assert_eq!(long_before_move.pnl.get(), 0);

    env.mutate_market(|_, group| {
        group.accrue_asset_to_not_atomic(0, 1, 50, 0, true).unwrap();
        group.assets[0].raw_oracle_target_price = 50;
    });
    env.svm.warp_to_slot(1);

    let long_with_hidden_loss = env.portfolio_state(long_portfolio);
    assert_eq!(
        long_with_hidden_loss.capital.get(),
        100,
        "the price move should be hidden until the account is touched"
    );
    assert_eq!(long_with_hidden_loss.pnl.get(), 0);
    let (_, group_with_hidden_loss) = env.market_state();
    assert_eq!(group_with_hidden_loss.insurance, 0);
    assert_eq!(group_with_hidden_loss.c_tot, 1_100);

    let sync_cu = env.sync_maintenance_fee_with_cu(long_portfolio, None, 1);
    println!("v16 SyncMaintenanceFee nonflat hidden-loss CU: {sync_cu}");
    assert_cu_within(
        "SyncMaintenanceFee nonflat hidden-loss regression",
        sync_cu,
        CUSTODY_CU_LIMIT,
    );

    let long_after_sync = env.portfolio_state(long_portfolio);
    let (_, group_after_sync) = env.market_state();
    assert_eq!(long_after_sync.capital.get(), 0);
    assert_eq!(long_after_sync.pnl.get(), 0);
    assert_eq!(long_after_sync.last_fee_slot.get(), 1);
    assert_eq!(
        group_after_sync.insurance, 50,
        "only capital remaining after the hidden loss is settled can be swept as fee"
    );
    assert_eq!(group_after_sync.c_tot, 1_000);
}

#[test]
fn v16_bpf_fee_sync_rejects_reused_market_slot_stale_leg_without_mutation() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 10_000, 10_000, 10_000, 1,
    );
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_portfolio = env.create_portfolio(&long_owner);
    let short_portfolio = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_portfolio, 1_000);
    env.deposit(&short_owner, short_portfolio, 1_000);
    env.trade_with_cu(
        &long_owner,
        long_portfolio,
        &short_owner,
        short_portfolio,
        POS_SCALE as i128,
        100,
        0,
    );

    let old_market_id = env.market_state().1.assets[0].market_id;
    env.mutate_market(|_, group| {
        group
            .accrue_asset_to_not_atomic(0, 1, 100, 0, true)
            .unwrap();
        group.assets[0].market_id = old_market_id + 1;
        group.next_market_id = group.next_market_id.max(old_market_id + 2);
    });
    env.svm.warp_to_slot(1);

    let market_before = env.svm.get_account(&env.market).unwrap().data;
    let long_before = env.svm.get_account(&long_portfolio).unwrap().data;
    let err = env
        .try_sync_maintenance_fee_with_cu(long_portfolio, None, 1)
        .expect_err("stale market id leg must fail closed");
    println!("v16 SyncMaintenanceFee stale reused-market-id rejection: {err}");

    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        market_before,
        "failed sync must not mutate the reused market slot"
    );
    assert_eq!(
        env.svm.get_account(&long_portfolio).unwrap().data,
        long_before,
        "failed sync must not mutate the stale portfolio"
    );
}

#[test]
fn v16_bpf_close_portfolio_sweeps_rent_to_market_slab() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);
    env.withdraw(&owner, portfolio, 1_000);

    let market_lamports_before_close = env.svm.get_account(&env.market).unwrap().lamports;
    let portfolio_lamports_before_close = env.svm.get_account(&portfolio).unwrap().lamports;
    let close_cu = env.close_portfolio_with_cu(&owner, portfolio);
    assert_cu_within("close portfolio rent sweep", close_cu, CUSTODY_CU_LIMIT);

    let (_, group) = env.market_state();
    assert_eq!(group.materialized_portfolio_count, 0);
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().lamports,
        market_lamports_before_close + portfolio_lamports_before_close,
        "ClosePortfolio should move closed account rent into the market slab"
    );
    if let Some(closed_account) = env.svm.get_account(&portfolio) {
        assert_eq!(closed_account.lamports, 0);
        assert!(closed_account.data.is_empty() || !state::is_initialized(&closed_account.data));
    }
}

#[test]
fn v16_program_non_owner_cannot_close_flat_portfolio() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let mallory = Keypair::new();
    env.ensure_signer_account(mallory.pubkey());

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    assert_eq!(
        env.market_state().1.materialized_portfolio_count,
        1,
        "portfolio is materialized before the close attempt"
    );

    env.svm.expire_blockhash();
    let bad_close = env.send(
        env.close_portfolio_ix(portfolio),
        vec![
            AccountMeta::new(mallory.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&mallory],
    );
    assert!(
        bad_close.is_err(),
        "a non-owner must not be able to dematerialize a flat victim portfolio"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "non-owner close must not decrement counters or receive rent into the market slab"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "non-owner close must leave the victim portfolio account intact"
    );

    env.close_portfolio_with_cu(&owner, portfolio);
    assert_eq!(
        env.market_state().1.materialized_portfolio_count,
        0,
        "the real owner can still close the flat portfolio"
    );
}

// position, or resolved receipt may carry over. Attacker success = inheriting stale value/claims.
#[test]
fn v16_program_portfolio_reuse_after_close_is_clean() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000);
    // flatten then close.
    env.svm.expire_blockhash();
    env.withdraw(&owner, p, 1_000);
    env.close_portfolio_with_cu(&owner, p);

    // Adversarial twist: re-fund the SAME address with the OLD (possibly stale) bytes still present,
    // simulating a reuse where the closed account's data was not zeroed. Re-init must overwrite it.
    let stale = env
        .svm
        .get_account(&p)
        .map(|a| a.data.clone())
        .unwrap_or_else(|| vec![0u8; env.portfolio_account_len]);
    env.svm
        .set_account(
            p,
            Account {
                lamports: 1_000_000_000,
                data: stale, // whatever close left behind
                owner: env.program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let new_owner = Keypair::new();
    env.ensure_signer_account(new_owner.pubkey());
    env.svm.expire_blockhash();
    let r = env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(new_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(p, false),
        ],
        &[&new_owner],
    );
    // Either re-init is rejected (account still considered live) OR it succeeds with a CLEAN slate.
    if r.is_ok() {
        let a = state::read_portfolio(&env.svm.get_account(&p).unwrap().data).unwrap();
        assert_eq!(
            a.capital.get(),
            0,
            "reused portfolio starts with zero capital (no stale value)"
        );
        assert_eq!(a.pnl.get(), 0, "no stale pnl carried over");
        assert!(
            !resolved_receipt(&a).present,
            "no stale resolved receipt carried over"
        );
        assert!(
            percolator::active_bitmap_is_empty(active_bitmap(&a)),
            "no stale positions"
        );
        // and a fresh deposit credits exactly the deposited amount.
        env.svm.expire_blockhash();
        env.deposit(&new_owner, p, 500);
        let a2 = state::read_portfolio(&env.svm.get_account(&p).unwrap().data).unwrap();
        assert_eq!(
            a2.capital.get(),
            500,
            "fresh deposit credits exactly 500 (no stale base)"
        );
    }
    // conservation intact regardless.
    let (_, g) = env.market_state();
    assert_eq!(
        g.vault,
        g.c_tot + g.insurance,
        "conservation after close+reuse"
    );
}

// account). No corrupting an account it doesn't own.
#[test]
fn v16_program_init_portfolio_foreign_account_rejected() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    env.ensure_signer_account(owner.pubkey());
    // an account owned by SPL Token (a foreign program), sized like a portfolio.
    let foreign = Pubkey::new_unique();
    env.svm
        .set_account(
            foreign,
            Account {
                lamports: 1_000_000_000,
                data: vec![0u8; env.portfolio_account_len],
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    let r = env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(foreign, false),
        ],
        &[&owner],
    );
    assert!(
        r.is_err(),
        "InitPortfolio on a foreign-owned account must reject"
    );
    // the foreign account is unchanged (still spl-token-owned, not a portfolio).
    let acc = env.svm.get_account(&foreign).unwrap();
    assert_eq!(
        acc.owner,
        spl_token::ID,
        "foreign account ownership unchanged (not hijacked)"
    );
    // a proper program-owned account still initializes fine.
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000);
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        1_000,
        "proper portfolio works"
    );
}

// otherwise a user could rewrite the market into a portfolio and strand every account/vault.
#[test]
fn v16_program_init_portfolio_cannot_use_market_as_portfolio_account() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let funded_portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, funded_portfolio, 500_000);

    let attacker = Keypair::new();
    env.ensure_signer_account(attacker.pubkey());
    let market_before = env.svm.get_account(&env.market).unwrap();
    let funded_before = env.svm.get_account(&funded_portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    env.svm.expire_blockhash();
    let alias_init = env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(env.market, false),
        ],
        &[&attacker],
    );
    assert!(
        alias_init.is_err(),
        "InitPortfolio must reject the market account as the portfolio target"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected alias init must leave market bytes and lamports unchanged"
    );
    assert_eq!(
        env.svm.get_account(&funded_portfolio).unwrap(),
        funded_before,
        "rejected alias init must not touch existing portfolios"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected alias init must not move or orphan vault custody"
    );

    let normal_owner = Keypair::new();
    env.ensure_signer_account(normal_owner.pubkey());
    let normal_portfolio = Pubkey::new_unique();
    env.svm
        .set_account(
            normal_portfolio,
            Account {
                lamports: 1_000_000_000,
                data: vec![0u8; env.portfolio_account_len],
                owner: env.program_id,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    let normal_init = env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(normal_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(normal_portfolio, false),
        ],
        &[&normal_owner],
    );
    assert!(
        normal_init.is_ok(),
        "valid InitPortfolio must still work after rejected alias attempt: {normal_init:?}"
    );
    let initialized = env.portfolio_state(normal_portfolio);
    assert_eq!(initialized.owner, normal_owner.pubkey().to_bytes());
    assert_eq!(
        initialized.provenance_header.market_group_id,
        env.market.to_bytes()
    );
    assert_eq!(initialized.capital.get(), 0);
}

// must reject it, leaving the victim's portfolio byte-identical.
#[test]
fn v16_program_init_portfolio_cannot_reinit_funded_victim() {
    let mut env = V16CuEnv::new();
    let victim = Keypair::new();
    let vp = env.create_portfolio(&victim);
    env.deposit(&victim, vp, 500_000);
    let before = env.svm.get_account(&vp).unwrap().data.clone();
    let v_owner = env.portfolio_state(vp).owner;
    let v_cap = env.portfolio_state(vp).capital.get();
    assert!(v_cap > 0);
    // attacker tries to re-initialize the victim's portfolio, claiming ownership.
    let attacker = Keypair::new();
    env.ensure_signer_account(attacker.pubkey());
    env.svm.expire_blockhash();
    let r = env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(vp, false),
        ],
        &[&attacker],
    );
    assert!(
        r.is_err(),
        "re-init of an initialized portfolio must reject (AlreadyInitialized)"
    );
    // victim's portfolio is byte-identical: capital, owner, and raw bytes intact.
    assert_eq!(
        env.svm.get_account(&vp).unwrap().data,
        before,
        "victim portfolio bytes unchanged"
    );
    assert_eq!(
        env.portfolio_state(vp).owner,
        v_owner,
        "ownership not reassigned"
    );
    assert_eq!(
        env.portfolio_state(vp).capital.get(),
        v_cap,
        "capital not reset"
    );
}

// leave a phantom count behind and permanently block CloseSlab on an otherwise empty market.
#[test]
fn v16_program_empty_portfolio_reinit_cannot_inflate_materialized_count() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    assert_eq!(env.portfolio_state(portfolio).capital.get(), 0);
    assert_eq!(
        env.market_state().1.materialized_portfolio_count,
        1,
        "one empty initialized portfolio is materialized"
    );

    let attacker = Keypair::new();
    env.ensure_signer_account(attacker.pubkey());
    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    env.svm.expire_blockhash();
    let reinit = env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&attacker],
    );
    assert!(
        reinit.is_err(),
        "empty initialized portfolio reinit must reject"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected empty reinit must not increment the materialized count"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "rejected empty reinit must not reassign or rewrite the portfolio"
    );

    env.svm.expire_blockhash();
    env.send(
        env.close_portfolio_ix(portfolio),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[&owner],
    )
    .expect("owner closes the one real empty portfolio");
    assert_eq!(
        env.market_state().1.materialized_portfolio_count,
        0,
        "one close clears the one real materialized account"
    );

    env.resolve();
    env.close_slab_with_cu();
}

// security.md sweep - public maintenance sync must not bypass the live ClosePortfolio owner gate.
// A flat but initialized portfolio is user-owned state; ClosePortfolio rejects a non-owner in live
// mode, so the unsigned SyncMaintenanceFee path must not dematerialize it as a side effect.
#[test]
fn v16_attack_sync_maintenance_cannot_close_empty_live_victim_portfolio() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    assert_eq!(
        env.market_state().1.materialized_portfolio_count,
        1,
        "empty initialized portfolio is materialized before the attack"
    );

    env.svm.expire_blockhash();
    let sync = env.send(
        ProgInstruction::SyncMaintenanceFee { now_slot: 0 },
        vec![
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[],
    );
    assert!(
        sync.is_ok(),
        "public SyncMaintenanceFee may refresh an empty live portfolio but must not close it: {sync:?}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "empty maintenance sync must not decrement materialized count or absorb rent"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "empty maintenance sync must leave the victim portfolio alive"
    );

    env.close_portfolio_with_cu(&owner, portfolio);
    assert_eq!(
        env.market_state().1.materialized_portfolio_count,
        0,
        "owner ClosePortfolio remains the live dematerialization path"
    );
}

// SOL-010 (reinitialization): InitMarket targets the shared market account. Reinitializing a funded
// live market would reset c_tot/insurance/assets while the SPL vault still holds user tokens, stranding
// all portfolios. The market header guard must reject even when the current market authority signs.
#[test]
fn v16_attack_init_market_cannot_reinitialize_funded_market() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 500_000);
    env.top_up_insurance_domain_with_authority(&admin, 0, 100);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    let params = V16CuMarketParams::default();

    env.svm.expire_blockhash();
    let reinit = env.send(
        ProgInstruction::InitMarket {
            max_portfolio_assets: params.max_portfolio_assets,
            h_min: params.h_min,
            h_max: params.h_max,
            initial_price: params.initial_price,
            min_nonzero_mm_req: params.min_nonzero_mm_req,
            min_nonzero_im_req: params.min_nonzero_im_req,
            maintenance_margin_bps: params.maintenance_margin_bps,
            initial_margin_bps: params.initial_margin_bps,
            max_trading_fee_bps: params.max_trading_fee_bps,
            trade_fee_base_bps: params.trade_fee_base_bps,
            liquidation_fee_bps: params.liquidation_fee_bps,
            liquidation_fee_cap: params.liquidation_fee_cap,
            min_liquidation_abs: params.min_liquidation_abs,
            max_price_move_bps_per_slot: params.max_price_move_bps_per_slot,
            max_accrual_dt_slots: params.max_accrual_dt_slots,
            max_abs_funding_e9_per_slot: params.max_abs_funding_e9_per_slot,
            min_funding_lifetime_slots: params.min_funding_lifetime_slots,
            max_account_b_settlement_chunks: params.max_account_b_settlement_chunks,
            max_bankrupt_close_chunks: params.max_bankrupt_close_chunks,
            max_bankrupt_close_lifetime_slots: params.max_bankrupt_close_lifetime_slots,
            public_b_chunk_atoms: params.public_b_chunk_atoms,
            maintenance_fee_per_slot: params.maintenance_fee_per_slot,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new_readonly(env.mint, false),
        ],
        &[&admin],
    );
    assert!(
        reinit.is_err(),
        "InitMarket on an initialized market must reject, even when signed by market authority"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected market reinit must leave all market bytes and lamports unchanged"
    );
    assert_eq!(
        env.svm.get_account(&portfolio).unwrap(),
        portfolio_before,
        "rejected market reinit must not rewrite funded portfolio state"
    );
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "rejected market reinit must not move or orphan vault custody"
    );
}

// SOL-010 (reinitialization): even before user funds arrive, a hostile signer must not be able to
// re-run InitMarket on the already initialized slab and seize marketauth or swap the collateral mint.
#[test]
fn v16_attack_init_market_cannot_reinitialize_empty_market_or_seize_authority() {
    let mut env = V16CuEnv::new();
    let attacker = Keypair::new();
    env.ensure_signer_account(attacker.pubkey());
    let attacker_mint = env.create_mint();
    let market_before = env.svm.get_account(&env.market).unwrap();
    let params = V16CuMarketParams {
        initial_price: 777,
        ..V16CuMarketParams::default()
    };

    env.svm.expire_blockhash();
    let reinit = env.send(
        ProgInstruction::InitMarket {
            max_portfolio_assets: params.max_portfolio_assets,
            h_min: params.h_min,
            h_max: params.h_max,
            initial_price: params.initial_price,
            min_nonzero_mm_req: params.min_nonzero_mm_req,
            min_nonzero_im_req: params.min_nonzero_im_req,
            maintenance_margin_bps: params.maintenance_margin_bps,
            initial_margin_bps: params.initial_margin_bps,
            max_trading_fee_bps: params.max_trading_fee_bps,
            trade_fee_base_bps: params.trade_fee_base_bps,
            liquidation_fee_bps: params.liquidation_fee_bps,
            liquidation_fee_cap: params.liquidation_fee_cap,
            min_liquidation_abs: params.min_liquidation_abs,
            max_price_move_bps_per_slot: params.max_price_move_bps_per_slot,
            max_accrual_dt_slots: params.max_accrual_dt_slots,
            max_abs_funding_e9_per_slot: params.max_abs_funding_e9_per_slot,
            min_funding_lifetime_slots: params.min_funding_lifetime_slots,
            max_account_b_settlement_chunks: params.max_account_b_settlement_chunks,
            max_bankrupt_close_chunks: params.max_bankrupt_close_chunks,
            max_bankrupt_close_lifetime_slots: params.max_bankrupt_close_lifetime_slots,
            public_b_chunk_atoms: params.public_b_chunk_atoms,
            maintenance_fee_per_slot: params.maintenance_fee_per_slot,
        },
        vec![
            AccountMeta::new(attacker.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new_readonly(attacker_mint, false),
        ],
        &[&attacker],
    );
    assert!(
        reinit.is_err(),
        "non-authority InitMarket reinit must not seize an empty initialized market"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected empty-market reinit leaves the initialized slab byte-for-byte unchanged"
    );
    let cfg = env.market_state().0;
    assert_eq!(
        cfg.marketauth,
        env.admin.pubkey().to_bytes(),
        "market authority stayed with the original initializer"
    );
    assert_eq!(
        cfg.collateral_mint,
        env.mint.to_bytes(),
        "attacker could not swap the market collateral mint"
    );

    env.update_fee_redirect_policy_with_cu(1_234);
    assert_eq!(
        env.market_state().0.fee_redirect_to_market_0_bps,
        1_234,
        "market remains usable by the real authority after rejected reinit"
    );
}
