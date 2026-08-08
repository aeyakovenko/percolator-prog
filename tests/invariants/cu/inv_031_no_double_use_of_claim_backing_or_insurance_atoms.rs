//! INV-031 - no double use of claim, backing, or insurance atoms.
//!
//! Normative obligation: one backing, claim, insurance, or collateral atom can
//! support only one withdrawal, payout, risk increase, or residual cure at a
//! time. These public-route LiteSVM regressions exercise source backing,
//! insurance, dual-mint rail, and PnL conversion paths to prove retrying or
//! reclassifying the same atom cannot create a second spend.

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
        ProgInstruction::WithdrawInsurance { amount: 99 },
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
        ProgInstruction::WithdrawInsurance { amount: 1 },
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
        ProgInstruction::WithdrawInsurance { amount: 1 },
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
