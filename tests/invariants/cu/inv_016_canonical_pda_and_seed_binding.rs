//! INV-016 - Canonical PDA and seed binding.
//!
//! Public custody routes must bind the vault token account and vault authority to the canonical PDA
//! seeds for the supplied market. A token account owned by the right PDA but located at a
//! noncanonical address, or a withdrawal authority account from another seed, must reject without
//! moving SPL tokens or changing wrapper state.
//!
//! Guarantee boundary: this covers the wrapper's custody PDA boundary. It does not attempt to prove
//! every PDA used by every auxiliary route.

use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
struct CustodySnapshot {
    market: Account,
    portfolio: Account,
    source_or_dest: Account,
    canonical_vault: Account,
    substituted: Account,
}

fn custody_snapshot(
    env: &V16CuEnv,
    portfolio: Pubkey,
    token: Pubkey,
    substituted: Pubkey,
) -> CustodySnapshot {
    CustodySnapshot {
        market: env.svm.get_account(&env.market).unwrap(),
        portfolio: env.svm.get_account(&portfolio).unwrap(),
        source_or_dest: env.svm.get_account(&token).unwrap(),
        canonical_vault: env.svm.get_account(&env.vault).unwrap(),
        substituted: env.svm.get_account(&substituted).unwrap(),
    }
}

#[test]
fn v16_program_deposit_rejects_noncanonical_vault_address_without_mutation() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    let source = env.token_account(owner.pubkey(), 1_000);
    let noncanonical_vault = Pubkey::new_unique();
    env.svm
        .set_account(
            noncanonical_vault,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(env.mint, env.vault_authority, 0),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let before = custody_snapshot(&env, portfolio, source, noncanonical_vault);

    env.svm.expire_blockhash();
    let err = env
        .send(
            env.deposit_ix(portfolio, 100),
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(source, false),
                AccountMeta::new(noncanonical_vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&owner],
        )
        .expect_err("deposit must reject a noncanonical vault token account");
    assert!(
        err.contains("Custom") || err.contains("InstructionError"),
        "noncanonical vault rejection should surface as an instruction error, got {err}"
    );
    assert_eq!(
        custody_snapshot(&env, portfolio, source, noncanonical_vault),
        before,
        "noncanonical vault deposit rejection must roll back exactly"
    );
}

#[test]
fn v16_program_withdraw_rejects_wrong_vault_authority_pda_without_mutation() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000);
    let dest = env.token_account(owner.pubkey(), 0);
    let wrong_authority = Pubkey::new_unique();
    env.svm
        .set_account(
            wrong_authority,
            Account {
                lamports: 1_000_000_000,
                data: Vec::new(),
                owner: solana_sdk::system_program::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let before = custody_snapshot(&env, portfolio, dest, wrong_authority);

    env.svm.expire_blockhash();
    let err = env
        .send(
            env.withdraw_ix(portfolio, 100),
            vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(env.vault, false),
                AccountMeta::new_readonly(wrong_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ],
            &[&owner],
        )
        .expect_err("withdraw must reject a noncanonical vault authority account");
    assert!(
        err.contains("InvalidArgument")
            || err.contains("InstructionError")
            || err.contains("Custom"),
        "wrong vault-authority PDA should reject at the wrapper boundary, got {err}"
    );
    assert_eq!(
        custody_snapshot(&env, portfolio, dest, wrong_authority),
        before,
        "wrong vault-authority withdrawal rejection must roll back exactly"
    );
}

#[test]
fn v16_bpf_auth_matcher_init_rejects_wrong_pda_accepts_right_pda() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let lp_owner = Keypair::new();
    let lp = env.create_portfolio(&lp_owner);

    let bad_ctx = Keypair::new();
    let bad_delegate = Pubkey::new_unique();
    system_create_account_for_test(
        &mut env.svm,
        &env.payer,
        &bad_ctx,
        MATCHER_CONTEXT_LEN,
        matcher_program,
    );
    let bad = send_raw_tx(
        &mut env.svm,
        &env.payer,
        Instruction {
            program_id: matcher_program,
            accounts: vec![
                AccountMeta::new_readonly(lp_owner.pubkey(), true),
                AccountMeta::new_readonly(bad_delegate, false),
                AccountMeta::new(bad_ctx.pubkey(), false),
                AccountMeta::new_readonly(env.program_id, false),
                AccountMeta::new_readonly(env.market, false),
                AccountMeta::new_readonly(lp, false),
            ],
            data: vec![2],
        },
        &[&lp_owner],
    );
    assert!(
        bad.is_err(),
        "auth matcher init must reject a delegate PDA with the wrong seeds"
    );
    assert_eq!(
        env.svm.get_account(&bad_ctx.pubkey()).unwrap().data[64],
        0,
        "failed init must not mark the matcher context initialized"
    );

    let (ctx, delegate, _) =
        env.init_auth_matcher_context_via_system_create(matcher_program, &lp_owner, lp);
    let data = env.svm.get_account(&ctx).unwrap().data;
    assert_eq!(
        data[64], 1,
        "valid init marks the matcher context initialized"
    );
    assert_eq!(
        &data[65..97],
        delegate.as_ref(),
        "valid init stores the LP-account-bound delegate PDA"
    );
}
