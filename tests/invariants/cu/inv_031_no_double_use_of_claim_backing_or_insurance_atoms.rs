//! INV-031 - no double use of claim, backing, or insurance atoms.
//!
//! Normative obligation: one backing, claim, insurance, or collateral atom can
//! support only one withdrawal, payout, risk increase, or residual cure at a
//! time. These public-route LiteSVM regressions exercise source backing,
//! insurance, dual-mint rail, and PnL conversion paths to prove retrying or
//! reclassifying the same atom cannot create a second spend.
//! The liquidation-to-withdrawal history below distinguishes spent insurance
//! from its still-recorded domain budget: a residual cure cannot also fund an
//! operator payout, while the unspent remainder and a fresh top-up stay usable.
//!
//! The composition census at the end closes the current wrapper surface without
//! duplicating engine internals. INV-026/031 own every reachable successful
//! counterparty-lien class, INV-024/025 own value and stock, INV-080 owns complete
//! error propagation into SVM rollback, INV-033 owns public unreachability plus
//! pinned engine proofs for insurance-backed liens, and INV-088 reopens on any
//! new wrapper-to-engine transition.

use super::*;

// security.md sweep — base-unit deposit/withdraw mint routing (#5 / README L122): deposits accept ONLY
// the primary base-unit mint, but a holder may withdraw in EITHER the primary or the secondary mint.
#[test]
fn v16_attack_deposit_primary_only_withdraw_either() {
    let mut env = V16CuEnv::new();
    let market = env.market;
    let primary = env.mint;
    let vault_authority = env.vault_authority;
    let secondary = env.create_mint();
    env.update_base_unit_mints_with_cu(primary, secondary);

    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000); // PRIMARY deposit works.

    // SECONDARY deposit must reject (deposits are primary-only).
    let sec_src = env.token_account_for_mint(secondary, owner.pubkey(), 500);
    let sec_vault = canonical_vault_ata(vault_authority, secondary);
    env.svm
        .set_account(
            sec_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(secondary, vault_authority, 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    let r_dep = env.send(
        env.deposit_ix(p, 500),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(sec_src, false),
            AccountMeta::new(sec_vault, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    assert!(
        r_dep.is_err(),
        "depositing the secondary mint must reject (primary-only)"
    );
    assert_eq!(
        env.token_amount(sec_src),
        500,
        "rejected secondary deposit pulled nothing"
    );

    // Withdraw in PRIMARY works.
    let (pd, _) = env.withdraw_with_cu(&owner, p, 400);
    assert_eq!(env.token_amount(pd), 400, "primary withdrawal delivered");

    // Withdraw in SECONDARY works too (fund the secondary reserve, withdraw to a secondary dest).
    env.svm
        .set_account(
            sec_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(secondary, vault_authority, 300),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let sec_dest = env.token_account_for_mint(secondary, owner.pubkey(), 0);
    env.svm.expire_blockhash();
    let r_wd = env.send(
        env.withdraw_ix(p, 300),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(market, false),
            AccountMeta::new(p, false),
            AccountMeta::new(sec_dest, false),
            AccountMeta::new(sec_vault, false),
            AccountMeta::new_readonly(vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    assert!(
        r_wd.is_ok(),
        "withdrawing in the secondary mint must succeed: {r_wd:?}"
    );
    assert_eq!(
        env.token_amount(sec_dest),
        300,
        "secondary withdrawal delivered 1:1"
    );
}

// security.md sweep — dual-mint shared credit (#33/#44): primary and secondary withdrawals spend the
// same portfolio capital. A user must not withdraw a primary deposit once from the primary vault and
// then again from a funded secondary reserve.
#[test]
fn v16_attack_dual_mint_shared_credit_no_double_withdraw() {
    let mut env = V16CuEnv::new();
    let secondary = env.create_mint();
    env.update_base_unit_mints_with_cu(env.mint, secondary);
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);

    let (primary_dest, _) = env.withdraw_with_cu(&owner, portfolio, 1_000);
    assert_eq!(env.token_amount(primary_dest), 1_000);
    assert_eq!(
        env.portfolio_state(portfolio).capital.get(),
        0,
        "primary withdrawal exhausted the shared credit"
    );

    let secondary_vault = canonical_vault_ata(env.vault_authority, secondary);
    env.svm
        .set_account(
            secondary_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(secondary, env.vault_authority, 1_000),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let secondary_dest = env.token_account_for_mint(secondary, owner.pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    let secondary_vault_before = env.svm.get_account(&secondary_vault).unwrap();
    let secondary_dest_before = env.svm.get_account(&secondary_dest).unwrap();

    env.svm.expire_blockhash();
    let double_withdraw = env.send(
        env.withdraw_ix(portfolio, 1),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(secondary_dest, false),
            AccountMeta::new(secondary_vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    assert!(
        double_withdraw.is_err(),
        "secondary reserve must not pay a second withdrawal after primary credit is exhausted"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&portfolio).unwrap(), portfolio_before);
    assert_eq!(
        env.svm.get_account(&secondary_vault).unwrap(),
        secondary_vault_before,
        "rejected double-withdraw leaves secondary reserve untouched"
    );
    assert_eq!(
        env.svm.get_account(&secondary_dest).unwrap(),
        secondary_dest_before,
        "rejected double-withdraw pays no secondary tokens"
    );

    env.deposit(&owner, portfolio, 1);
    env.svm.expire_blockhash();
    let legitimate_secondary = env.send(
        env.withdraw_ix(portfolio, 1),
        vec![
            AccountMeta::new(owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
            AccountMeta::new(secondary_dest, false),
            AccountMeta::new(secondary_vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&owner],
    );
    assert!(
        legitimate_secondary.is_ok(),
        "fresh shared credit can still withdraw through the secondary reserve: {legitimate_secondary:?}"
    );
    assert_eq!(env.token_amount(secondary_dest), 1);
}

// security.md sweep — dual-mint insurance budget (#33/#44): the domain insurance budget is shared
// across primary and secondary payout rails. It must not pay once from the primary vault and then again
// from an independently funded secondary reserve.
#[test]
fn v16_attack_dual_mint_domain_insurance_no_double_withdraw() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let secondary = env.create_mint();
    env.update_base_unit_mints_with_cu(env.mint, secondary);
    env.top_up_insurance_domain_with_authority(&admin, 0, 100);
    let (primary_dest, _) = env
        .try_withdraw_insurance_asset_with_authority(&admin, 0, 100)
        .expect("primary insurance withdrawal exhausts the budget");
    assert_eq!(env.token_amount(primary_dest), 100);
    assert_eq!(
        env.market_state().1.insurance_domain_budget[0],
        0,
        "first withdrawal exhausted the shared insurance budget"
    );

    let secondary_vault = canonical_vault_ata(env.vault_authority, secondary);
    env.svm
        .set_account(
            secondary_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(secondary, env.vault_authority, 100),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let secondary_dest = env.token_account_for_mint(secondary, admin.pubkey(), 0);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let secondary_vault_before = env.svm.get_account(&secondary_vault).unwrap();
    let secondary_dest_before = env.svm.get_account(&secondary_dest).unwrap();

    env.svm.expire_blockhash();
    let double_withdraw = env.send(
        ProgInstruction::WithdrawInsuranceAsset {
            market_id: 0,
            authority_epoch: 0,
            asset_index: 0,
            amount: 1,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(secondary_dest, false),
            AccountMeta::new(secondary_vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        double_withdraw.is_err(),
        "secondary reserve must not pay after the insurance budget is exhausted"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(
        env.svm.get_account(&secondary_vault).unwrap(),
        secondary_vault_before,
        "rejected double-withdraw leaves secondary reserve untouched"
    );
    assert_eq!(
        env.svm.get_account(&secondary_dest).unwrap(),
        secondary_dest_before,
        "rejected double-withdraw pays no secondary insurance"
    );

    env.top_up_insurance_domain_with_authority(&admin, 0, 1);
    env.svm.expire_blockhash();
    let legitimate_secondary = env.send(
        ProgInstruction::WithdrawInsuranceAsset {
            market_id: 0,
            authority_epoch: 0,
            asset_index: 0,
            amount: 1,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(secondary_dest, false),
            AccountMeta::new(secondary_vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        legitimate_secondary.is_ok(),
        "fresh insurance budget can still pay through the secondary reserve: {legitimate_secondary:?}"
    );
    assert_eq!(env.token_amount(secondary_dest), 1);
}

// Terminal insurance is one shared market stock even when custody is split across the primary and
// configured secondary collateral rails. A resolved market must not pay the same terminal insurance
// atoms once from the primary vault and again from an independently funded secondary reserve.
#[test]
fn v16_program_dual_mint_terminal_insurance_no_double_withdraw() {
    let mut env = V16CuEnv::new();
    let admin = env.admin.insecure_clone();
    let secondary = env.create_mint();
    env.update_base_unit_mints_with_cu(env.mint, secondary);
    env.top_up_insurance(100);
    env.resolve();

    let secondary_vault = canonical_vault_ata(env.vault_authority, secondary);
    env.set_token_account_amount(env.vault, env.mint, env.vault_authority, 99);
    env.svm
        .set_account(
            secondary_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(secondary, env.vault_authority, 1),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let (_, split_group) = env.market_state();
    assert_eq!(split_group.insurance, 100);
    assert_eq!(split_group.vault, 100);
    assert_eq!(
        env.token_amount(env.vault) + env.token_amount(secondary_vault),
        100,
        "balanced fixture splits terminal custody across the two configured rails"
    );

    let primary_dest = env.token_account_for_mint(env.mint, admin.pubkey(), 0);
    env.svm.expire_blockhash();
    let primary = env.send(
        env.withdraw_insurance_asset_instruction(admin.pubkey(), 0, 99),
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(primary_dest, false),
            AccountMeta::new(env.vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        primary.is_ok(),
        "primary terminal insurance withdrawal should consume the primary rail: {primary:?}"
    );
    assert_eq!(env.token_amount(primary_dest), 99);
    assert_eq!(env.token_amount(env.vault), 0);
    assert_eq!(env.token_amount(secondary_vault), 1);
    let (_, after_primary) = env.market_state();
    assert_eq!(after_primary.insurance, 1);
    assert_eq!(after_primary.vault, 1);

    let secondary_dest = env.token_account_for_mint(secondary, admin.pubkey(), 0);
    env.svm.expire_blockhash();
    let secondary_withdraw = env.send(
        env.withdraw_insurance_asset_instruction(admin.pubkey(), 0, 1),
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(secondary_dest, false),
            AccountMeta::new(secondary_vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        secondary_withdraw.is_ok(),
        "secondary terminal insurance withdrawal should consume only the remaining shared stock: {secondary_withdraw:?}"
    );
    assert_eq!(env.token_amount(secondary_dest), 1);
    assert_eq!(env.token_amount(secondary_vault), 0);
    let (_, exhausted) = env.market_state();
    assert_eq!(exhausted.insurance, 0);
    assert_eq!(exhausted.vault, 0);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let secondary_vault_before = env.svm.get_account(&secondary_vault).unwrap();
    let secondary_dest_before = env.svm.get_account(&secondary_dest).unwrap();
    env.svm.expire_blockhash();
    let double_withdraw = env.send(
        env.withdraw_insurance_asset_instruction(admin.pubkey(), 0, 1),
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(secondary_dest, false),
            AccountMeta::new(secondary_vault, false),
            AccountMeta::new_readonly(env.vault_authority, false),
            AccountMeta::new_readonly(spl_token::ID, false),
        ],
        &[&admin],
    );
    assert!(
        double_withdraw.is_err(),
        "terminal insurance cannot be withdrawn again through the other collateral rail"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(
        env.svm.get_account(&secondary_vault).unwrap(),
        secondary_vault_before,
        "rejected terminal double-withdraw leaves the secondary vault untouched"
    );
    assert_eq!(
        env.svm.get_account(&secondary_dest).unwrap(),
        secondary_dest_before,
        "rejected terminal double-withdraw pays no additional secondary tokens"
    );
}

#[test]
fn v16_program_liquidation_spent_insurance_cannot_be_withdrawn_again() {
    const INSURANCE: u128 = 125;
    const OTHER_INSURANCE: u128 = 137;
    const SHORT_CAPITAL: u128 = 100;
    const LONG_CAPITAL: u128 = 10_000;
    const LOSS: u128 = 10 * (120 - 100);
    const SPENT: u128 = LOSS - SHORT_CAPITAL;
    const REMAINING: u128 = INSURANCE - SPENT;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    let admin = env.admin.insecure_clone();
    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, LONG_CAPITAL);
    env.deposit(&short_owner, short, SHORT_CAPITAL);
    // All top-ups use this finite source; no account is rewritten to refill insurance.
    let source = env.token_account(admin.pubkey(), (INSURANCE + OTHER_INSURANCE + 1) as u64);
    let destination = env.token_account(admin.pubkey(), 0);
    let top_up = |env: &mut V16CuEnv, domain, amount| {
        env.svm.expire_blockhash();
        env.send(
            ProgInstruction::TopUpInsuranceDomain {
                market_id: 0,
                authority_epoch: 0,
                intent_id: 0,
                domain,
                amount,
            },
            vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&admin],
        )
        .expect("public insurance top-up from the finite source")
    };
    assert_cu_within(
        "INV-031 insurance funding",
        top_up(&mut env, 0, INSURANCE),
        CUSTODY_CU_LIMIT,
    );
    assert_cu_within(
        "INV-031 unrelated asset insurance funding",
        top_up(&mut env, 2, OTHER_INSURANCE),
        CUSTODY_CU_LIMIT,
    );
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, 100);
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        10 * POS_SCALE as i128,
        100,
        0,
    );
    let funded_vault = LONG_CAPITAL + SHORT_CAPITAL + INSURANCE + OTHER_INSURANCE;
    assert_eq!(u128::from(env.token_amount(env.vault)), funded_vault);
    assert_eq!(env.token_amount(source), 1);

    let mut max_crank_cu = 0;
    for now_slot in 2..=5 {
        env.svm.warp_to_slot(now_slot);
        env.push_auth_mark_with_cu(now_slot, 120);
        let cu = env.crank(
            long,
            ProgInstruction::PermissionlessCrank {
                now_slot,
                observations: crank_observations(0),
            },
        );
        assert_cu_within("INV-031 public mark advance", cu, CRANK_CU_LIMIT);
        max_crank_cu = max_crank_cu.max(cu);
    }
    assert_eq!(env.market_state().1.assets[0].effective_price, 120);
    for _ in 0..8 {
        for portfolio in [long, short] {
            if let Some(cu) = env.crank_if_actionable(
                portfolio,
                ProgInstruction::PermissionlessCrank {
                    now_slot: 5,
                    observations: crank_observations(0),
                },
            ) {
                assert_cu_within("INV-031 insurance-consuming crank", cu, CRANK_CU_LIMIT);
                max_crank_cu = max_crank_cu.max(cu);
            }
        }
        if close_progress(&env.portfolio_state(short)).finalized {
            break;
        }
    }
    let short_after = env.portfolio_state(short);
    let close = close_progress(&short_after);
    assert!(
        close.finalized,
        "insurance-covered liquidation must finish within eight rounds"
    );
    assert!(percolator::active_bitmap_is_empty(active_bitmap(
        &short_after
    )));
    assert_eq!((short_after.capital.get(), short_after.pnl.get()), (0, 0));
    assert_eq!(close.insurance_spent, SPENT);
    assert_eq!(close.residual_remaining, 0);
    crate::support::fuzz_model::verify_close_residual_partition("INV-031 insurance spend", &close)
        .expect("the residual cure must account for its insurance atoms exactly once");

    let after_liquidation = env.market_state().1;
    assert_eq!(after_liquidation.insurance_domain_budget[0], INSURANCE);
    assert_eq!(after_liquidation.insurance_domain_spent[0], SPENT);
    assert_eq!(after_liquidation.insurance_domain_budget[1], 0);
    assert_eq!(after_liquidation.insurance, REMAINING + OTHER_INSURANCE);
    assert_eq!(after_liquidation.c_tot, LONG_CAPITAL);
    assert_eq!(after_liquidation.vault, funded_vault);
    assert_eq!(u128::from(env.token_amount(env.vault)), funded_vault);

    let withdraw = |env: &mut V16CuEnv, amount| {
        env.svm.expire_blockhash();
        let ix = Instruction {
            program_id: env.program_id,
            accounts: vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(destination, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(env.vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            data: env
                .withdraw_insurance_asset_instruction(admin.pubkey(), 0, amount)
                .encode(),
        };
        let tx = Transaction::new_signed_with_payer(
            &[
                heap_ix(),
                ComputeBudgetInstruction::set_compute_unit_limit(CUSTODY_CU_LIMIT as u32),
                ix,
            ],
            Some(&env.payer.pubkey()),
            &[&env.payer, &admin],
            env.svm.latest_blockhash(),
        );
        env.svm.send_transaction(tx)
    };
    let protected_keys = [long, short, env.mint];
    let protected_before = protected_keys.map(|key| env.svm.get_account(&key).unwrap());
    let rollback_keys = [env.market, env.vault, source, destination, admin.pubkey()];
    let mut max_withdraw_cu = 0;
    let mut paid = 0u128;
    // After each boundary rejection, the exact available amount succeeds in the same live market.
    // Gross domain budget, aggregate insurance, and SPL custody can all pay the rejected request;
    // only this asset's budget minus the prior liquidation spend makes it unavailable.
    for (amount, accepted) in [(REMAINING + 1, false), (REMAINING, true), (1, false)] {
        assert!(u128::from(env.token_amount(env.vault)) >= amount);
        assert!(env.market_state().1.insurance >= amount);
        assert!(env.market_state().1.insurance_domain_budget[0] >= amount);
        let before = rollback_keys.map(|key| env.svm.get_account(&key).unwrap());
        let result = withdraw(&mut env, amount);
        let cu = if accepted {
            paid += amount;
            result
                .expect("the unspent insurance remainder must stay withdrawable")
                .compute_units_consumed
        } else {
            let error = result.expect_err("liquidation-spent insurance must not fund a second use");
            assert_eq!(
                error.err,
                solana_sdk::transaction::TransactionError::InstructionError(
                    2,
                    solana_sdk::instruction::InstructionError::Custom(
                        PercolatorError::EngineLockActive as u32,
                    ),
                ),
                "the remaining insurance allowance must reject, not custody or compute exhaustion",
            );
            assert_eq!(
                rollback_keys.map(|key| env.svm.get_account(&key).unwrap()),
                before
            );
            error.meta.compute_units_consumed
        };
        assert_cu_within(
            "INV-031 liquidation-to-withdrawal boundary",
            cu,
            CUSTODY_CU_LIMIT,
        );
        max_withdraw_cu = max_withdraw_cu.max(cu);
        let group = env.market_state().1;
        assert_eq!(group.insurance_domain_budget[0], INSURANCE - paid);
        assert_eq!(group.insurance_domain_spent[0], SPENT);
        assert_eq!(
            &group.insurance_domain_budget[1..4],
            &[0, OTHER_INSURANCE, 0]
        );
        assert_eq!(&group.insurance_domain_spent[1..4], &[0, 0, 0]);
        assert_eq!(group.insurance, REMAINING + OTHER_INSURANCE - paid);
        assert_eq!(
            group.insurance_domain_budget_remaining_total,
            REMAINING + OTHER_INSURANCE - paid
        );
        assert_domain_budget_remaining_total_consistent(&group, "INV-031 once-only insurance");
        assert_eq!(group.c_tot, LONG_CAPITAL);
        assert_eq!(group.vault, funded_vault - paid);
        assert_eq!(u128::from(env.token_amount(env.vault)), group.vault);
        assert_eq!(u128::from(env.token_amount(destination)), paid);
        assert_eq!(env.token_amount(source), 1);
        assert_eq!(
            protected_keys.map(|key| env.svm.get_account(&key).unwrap()),
            protected_before
        );
    }

    let refill_cu = top_up(&mut env, 0, 1);
    assert_cu_within(
        "INV-031 fresh insurance refill",
        refill_cu,
        CUSTODY_CU_LIMIT,
    );
    let refilled = env.market_state().1;
    assert_eq!(refilled.insurance_domain_budget[0], SPENT + 1);
    assert_eq!(refilled.insurance_domain_spent[0], SPENT);
    assert_eq!(refilled.insurance, OTHER_INSURANCE + 1);
    assert_eq!(
        refilled.insurance_domain_budget_remaining_total,
        OTHER_INSURANCE + 1
    );
    assert_eq!(refilled.vault, funded_vault - REMAINING + 1);
    assert_eq!(u128::from(env.token_amount(env.vault)), refilled.vault);
    assert_eq!(env.token_amount(source), 0);
    let fresh_cu = withdraw(&mut env, 1)
        .expect("only the freshly transferred atom may fund another insurance withdrawal")
        .compute_units_consumed;
    assert_cu_within(
        "INV-031 fresh insurance withdrawal",
        fresh_cu,
        CUSTODY_CU_LIMIT,
    );
    let final_group = env.market_state().1;
    assert_eq!(final_group.insurance_domain_budget[0], SPENT);
    assert_eq!(final_group.insurance_domain_spent[0], SPENT);
    assert_eq!(
        &final_group.insurance_domain_budget[1..4],
        &[0, OTHER_INSURANCE, 0]
    );
    assert_eq!(&final_group.insurance_domain_spent[1..4], &[0, 0, 0]);
    assert_eq!(final_group.insurance, OTHER_INSURANCE);
    assert_eq!(
        final_group.insurance_domain_budget_remaining_total,
        OTHER_INSURANCE
    );
    assert_domain_budget_remaining_total_consistent(&final_group, "INV-031 after refill payout");
    assert_eq!(final_group.c_tot, LONG_CAPITAL);
    assert_eq!(final_group.vault, LONG_CAPITAL + LOSS + OTHER_INSURANCE);
    assert_eq!(u128::from(env.token_amount(env.vault)), final_group.vault);
    assert_eq!(u128::from(env.token_amount(destination)), REMAINING + 1);
    assert_eq!(
        SPENT + u128::from(env.token_amount(destination)),
        INSURANCE + 1
    );
    assert_eq!(
        protected_keys.map(|key| env.svm.get_account(&key).unwrap()),
        protected_before
    );
    println!(
        "INV-031: insurance cure={SPENT}, payout={}, max crank CU={max_crank_cu}, max withdrawal CU={}",
        REMAINING + 1,
        max_withdraw_cu.max(fresh_cu),
    );
}

#[test]
fn v16_program_single_use_lifecycle_composition_is_source_complete() {
    crate::assert_certified_engine_pin("INV-031 single-use composition");

    let public_source =
        include_str!("../public_sbf/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs");
    assert!(public_source.contains("fn v16_program_cross_domain_backing_is_consumed_once"));

    let stateful_source =
        include_str!("../stateful/inv_031_no_double_use_of_claim_backing_or_insurance_atoms.rs");
    for witness in [
        "v16_program_live_source_lien_route_pairs_preserve_single_backing_ownership",
        "v16_program_two_accounts_cannot_reserve_the_same_source_backing_atoms",
        "v16_program_haircut_conversion_retries_cannot_reuse_claim_or_backing",
    ] {
        assert!(
            stateful_source.contains(&format!("fn {witness}")),
            "missing INV-031 public ownership witness {witness}",
        );
    }

    let lifecycle_source =
        include_str!("../stateful/inv_026_reservation_and_encumbrance_conservation.rs");
    assert!(lifecycle_source.contains(
        "fn v16_program_counterparty_encumbrance_lifecycle_is_exact_across_routes_sides_and_terminal_modes"
    ));
    let insurance_source = include_str!("inv_033_insurance_backed_lien_single_classification.rs");
    assert!(insurance_source.contains(
        "fn v16_program_public_source_lien_classification_never_double_counts_insurance"
    ));
    let rollback_source = include_str!("inv_080_error_propagation_and_exact_rollback.rs");
    assert!(rollback_source
        .contains("fn v16_program_explicit_engine_error_dispositions_are_source_complete"));
    assert!(rollback_source
        .contains("fn v16_program_dispatch_and_entrypoints_preserve_every_handler_error"));
    let transition_source =
        include_str!("inv_088_global_summaries_are_not_account_local_proofs.rs");
    assert!(transition_source.contains(
        "fn v16_program_every_wrapper_engine_transition_callsite_has_summary_disposition_and_witness"
    ));

    let value_proof = include_str!("../kani/inv_024_attributed_quote_value_conservation.rs");
    assert!(
        value_proof.contains("fn kani_inv024_engine_flow_validator_equals_wrapper_value_equation")
    );
    let stock_proof = include_str!("../kani/inv_025_exact_stock_reconciliation.rs");
    assert!(
        stock_proof.contains("fn kani_inv025_engine_partition_composes_with_wrapper_spl_custody")
    );
    let residual_source = include_str!("../stateful/inv_037_exact_residual_partition.rs");
    assert!(residual_source
        .contains("fn inv037_public_cure_preserves_exact_partition_across_routes_and_sides"));
}
