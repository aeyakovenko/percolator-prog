//! INV-016 - Canonical PDA and seed binding.
//!
//! Public custody routes must bind the vault token account and vault authority to the canonical PDA
//! seeds for the supplied market. A token account owned by the right PDA but located at a
//! noncanonical address, or a withdrawal authority account from another seed, must reject without
//! moving SPL tokens or changing wrapper state.
//!
//! Guarantee boundary: this covers the wrapper's public custody PDA boundary. Clients submit account
//! keys on these routes, not seed vectors or bump bytes, so reordered or omitted seed components have
//! no independent instruction encoding. At the byte/API boundary those attempts are executable only
//! as substituted account keys, which the matrix below exercises without mutating program-owned
//! account bytes.

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

#[derive(Clone, Debug, PartialEq, Eq)]
struct AccountSetSnapshot(Vec<(Pubkey, Option<Account>)>);

fn account_set_snapshot(env: &V16CuEnv, accounts: &[AccountMeta]) -> AccountSetSnapshot {
    let mut keys = Vec::new();
    for meta in accounts {
        if !keys.contains(&meta.pubkey) {
            keys.push(meta.pubkey);
        }
    }
    AccountSetSnapshot(
        keys.into_iter()
            .map(|key| (key, env.svm.get_account(&key)))
            .collect(),
    )
}

fn assert_public_pda_rejects_without_mutation(
    env: &mut V16CuEnv,
    label: &str,
    ix: ProgInstruction,
    accounts: Vec<AccountMeta>,
    extra_signers: &[&Keypair],
) {
    let before = account_set_snapshot(env, &accounts);
    env.svm.expire_blockhash();
    let err = match env.send(ix, accounts.clone(), extra_signers) {
        Ok(_) => panic!("{label} unexpectedly accepted PDA substitution"),
        Err(err) => err,
    };
    assert!(
        err.contains("InstructionError")
            || err.contains("Custom")
            || err.contains("InvalidArgument"),
        "{label} should reject inside the public instruction boundary, got {err}"
    );
    assert_eq!(
        account_set_snapshot(env, &accounts),
        before,
        "{label} must roll back every public account exactly"
    );
}

#[derive(Clone, Copy, Debug)]
enum PdaFault {
    WrongBumpKey,
    CrossRoleKey,
    CrossMarketKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PdaSlot {
    VaultToken,
    SecondaryVaultToken,
    VaultAuthority,
}

#[derive(Clone, Copy, Debug)]
enum PdaRoute {
    Deposit,
    TopUpInsurance,
    TopUpInsuranceDomain,
    TopUpBackingBucket,
    Withdraw,
    WithdrawInsuranceAsset,
    WithdrawBackingBucket,
    WithdrawInsurance,
    CloseResolved,
    CloseSlab,
    SwapSecondaryForPrimary,
}

const PDA_FAULTS: [PdaFault; 3] = [
    PdaFault::WrongBumpKey,
    PdaFault::CrossRoleKey,
    PdaFault::CrossMarketKey,
];
const PDA_ROUTES: [PdaRoute; 11] = [
    PdaRoute::Deposit,
    PdaRoute::TopUpInsurance,
    PdaRoute::TopUpInsuranceDomain,
    PdaRoute::TopUpBackingBucket,
    PdaRoute::Withdraw,
    PdaRoute::WithdrawInsuranceAsset,
    PdaRoute::WithdrawBackingBucket,
    PdaRoute::WithdrawInsurance,
    PdaRoute::CloseResolved,
    PdaRoute::CloseSlab,
    PdaRoute::SwapSecondaryForPrimary,
];
const VAULT_TOKEN_ONLY: &[PdaSlot] = &[PdaSlot::VaultToken];
const VAULT_TOKEN_AND_AUTHORITY: &[PdaSlot] = &[PdaSlot::VaultToken, PdaSlot::VaultAuthority];
const SWAP_PDA_SLOTS: &[PdaSlot] = &[
    PdaSlot::VaultToken,
    PdaSlot::SecondaryVaultToken,
    PdaSlot::VaultAuthority,
];

impl PdaRoute {
    fn pda_slots(self) -> &'static [PdaSlot] {
        match self {
            PdaRoute::Deposit
            | PdaRoute::TopUpInsurance
            | PdaRoute::TopUpInsuranceDomain
            | PdaRoute::TopUpBackingBucket => VAULT_TOKEN_ONLY,
            PdaRoute::SwapSecondaryForPrimary => SWAP_PDA_SLOTS,
            PdaRoute::Withdraw
            | PdaRoute::WithdrawInsuranceAsset
            | PdaRoute::WithdrawBackingBucket
            | PdaRoute::WithdrawInsurance
            | PdaRoute::CloseResolved
            | PdaRoute::CloseSlab => VAULT_TOKEN_AND_AUTHORITY,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct PdaSubstitutionAccounts {
    vault_authority: Pubkey,
    vault_token: Pubkey,
    secondary_vault_token: Option<Pubkey>,
}

fn noncanonical_vault_authority(program_id: Pubkey, market: Pubkey) -> Pubkey {
    let (_, canonical_bump) =
        Pubkey::find_program_address(&[b"vault", market.as_ref()], &program_id);
    for bump in 0..=u8::MAX {
        if bump == canonical_bump {
            continue;
        }
        if let Ok(key) =
            Pubkey::create_program_address(&[b"vault", market.as_ref(), &[bump]], &program_id)
        {
            return key;
        }
    }
    panic!("could not find a noncanonical bump for the vault-authority seed tuple");
}

fn seed_system_account(env: &mut V16CuEnv, key: Pubkey) {
    if env.svm.get_account(&key).is_none() {
        env.svm
            .set_account(
                key,
                Account {
                    lamports: 1_000_000_000,
                    data: Vec::new(),
                    owner: solana_sdk::system_program::ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
    }
}

fn seed_token_account_for_owner(env: &mut V16CuEnv, key: Pubkey, mint: Pubkey, owner: Pubkey) {
    env.svm
        .set_account(
            key,
            Account {
                lamports: 1_000_000_000,
                data: make_token_data(mint, owner, 1_000),
                owner: spl_token::ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
}

fn pda_substitution_accounts(
    env: &mut V16CuEnv,
    fault: PdaFault,
    secondary_mint: Option<Pubkey>,
) -> PdaSubstitutionAccounts {
    let vault_authority = match fault {
        PdaFault::WrongBumpKey => noncanonical_vault_authority(env.program_id, env.market),
        PdaFault::CrossRoleKey => matcher_delegate_key(
            &env.program_id,
            &env.market,
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
        ),
        PdaFault::CrossMarketKey => {
            // The vault-authority PDA is stateless; these public routes receive only the account key.
            // A different market seed is therefore represented completely by the derived key.
            let foreign_market = Pubkey::new_unique();
            Pubkey::find_program_address(&[b"vault", foreign_market.as_ref()], &env.program_id).0
        }
    };
    seed_system_account(env, vault_authority);

    let vault_token = canonical_vault_ata(vault_authority, env.mint);
    seed_token_account_for_owner(env, vault_token, env.mint, vault_authority);
    let secondary_vault_token = secondary_mint.map(|mint| {
        let vault = canonical_vault_ata(vault_authority, mint);
        seed_token_account_for_owner(env, vault, mint, vault_authority);
        vault
    });

    PdaSubstitutionAccounts {
        vault_authority,
        vault_token,
        secondary_vault_token,
    }
}

fn substitute_vault(
    slot: PdaSlot,
    canonical: Pubkey,
    substitutions: PdaSubstitutionAccounts,
) -> Pubkey {
    if slot == PdaSlot::VaultToken {
        substitutions.vault_token
    } else {
        canonical
    }
}

fn substitute_secondary_vault(
    slot: PdaSlot,
    canonical: Pubkey,
    substitutions: PdaSubstitutionAccounts,
) -> Pubkey {
    if slot == PdaSlot::SecondaryVaultToken {
        substitutions
            .secondary_vault_token
            .expect("secondary substitution requested")
    } else {
        canonical
    }
}

fn substitute_vault_authority(
    slot: PdaSlot,
    canonical: Pubkey,
    substitutions: PdaSubstitutionAccounts,
) -> Pubkey {
    if slot == PdaSlot::VaultAuthority {
        substitutions.vault_authority
    } else {
        canonical
    }
}

fn exercise_public_pda_substitution(route: PdaRoute, slot: PdaSlot, fault: PdaFault) {
    let label = format!("{route:?}/{slot:?}/{fault:?}");
    let mut env = V16CuEnv::new();

    match route {
        PdaRoute::Deposit => {
            let owner = Keypair::new();
            let portfolio = env.create_portfolio(&owner);
            let source = env.token_account(owner.pubkey(), 1_000);
            let substitutions = pda_substitution_accounts(&mut env, fault, None);
            let vault = substitute_vault(slot, env.vault, substitutions);
            let ix = env.deposit_ix(portfolio, 100);
            let accounts = vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(source, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ];

            assert_public_pda_rejects_without_mutation(&mut env, &label, ix, accounts, &[&owner]);
        }
        PdaRoute::TopUpInsurance => {
            let admin = env.admin.insecure_clone();
            let source = env.token_account(admin.pubkey(), 1_000);
            let substitutions = pda_substitution_accounts(&mut env, fault, None);
            let vault = substitute_vault(slot, env.vault, substitutions);
            let ix = ProgInstruction::TopUpInsurance {
                market_id: 0,
                amount: 100,
            };
            let accounts = vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ];

            assert_public_pda_rejects_without_mutation(&mut env, &label, ix, accounts, &[&admin]);
        }
        PdaRoute::TopUpInsuranceDomain => {
            let authority = env.admin.insecure_clone();
            let source = env.token_account(authority.pubkey(), 1_000);
            let substitutions = pda_substitution_accounts(&mut env, fault, None);
            let vault = substitute_vault(slot, env.vault, substitutions);
            let ix = ProgInstruction::TopUpInsuranceDomain {
                market_id: 0,
                domain: 0,
                amount: 100,
            };
            let accounts = vec![
                AccountMeta::new(authority.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ];

            assert_public_pda_rejects_without_mutation(
                &mut env,
                &label,
                ix,
                accounts,
                &[&authority],
            );
        }
        PdaRoute::TopUpBackingBucket => {
            let admin = env.admin.insecure_clone();
            let source = env.token_account(admin.pubkey(), 1_000);
            let substitutions = pda_substitution_accounts(&mut env, fault, None);
            let vault = substitute_vault(slot, env.vault, substitutions);
            let ix = ProgInstruction::TopUpBackingBucket {
                market_id: 0,
                domain: 0,
                amount: 100,
                expiry_slot: 10,
            };
            let accounts = vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(source, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ];

            assert_public_pda_rejects_without_mutation(&mut env, &label, ix, accounts, &[&admin]);
        }
        PdaRoute::Withdraw => {
            let owner = Keypair::new();
            let portfolio = env.create_portfolio(&owner);
            env.deposit(&owner, portfolio, 1_000);
            let dest = env.token_account(owner.pubkey(), 0);
            let substitutions = pda_substitution_accounts(&mut env, fault, None);
            let vault = substitute_vault(slot, env.vault, substitutions);
            let vault_authority =
                substitute_vault_authority(slot, env.vault_authority, substitutions);
            let ix = env.withdraw_ix(portfolio, 100);
            let accounts = vec![
                AccountMeta::new(owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ];

            assert_public_pda_rejects_without_mutation(&mut env, &label, ix, accounts, &[&owner]);
        }
        PdaRoute::WithdrawInsuranceAsset => {
            let admin = env.admin.insecure_clone();
            env.top_up_insurance_domain_with_authority(&admin, 0, 1_000);
            let dest = env.token_account(admin.pubkey(), 0);
            let substitutions = pda_substitution_accounts(&mut env, fault, None);
            let vault = substitute_vault(slot, env.vault, substitutions);
            let vault_authority =
                substitute_vault_authority(slot, env.vault_authority, substitutions);
            let ix = ProgInstruction::WithdrawInsuranceAsset {
                market_id: 0,
                asset_index: 0,
                amount: 100,
            };
            let accounts = vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ];

            assert_public_pda_rejects_without_mutation(&mut env, &label, ix, accounts, &[&admin]);
        }
        PdaRoute::WithdrawBackingBucket => {
            let admin = env.admin.insecure_clone();
            env.top_up_backing_bucket(0, 1_000, 10);
            let dest = env.token_account(admin.pubkey(), 0);
            let substitutions = pda_substitution_accounts(&mut env, fault, None);
            let vault = substitute_vault(slot, env.vault, substitutions);
            let vault_authority =
                substitute_vault_authority(slot, env.vault_authority, substitutions);
            let ix = ProgInstruction::WithdrawBackingBucket {
                domain: 0,
                amount: 100,
            };
            let accounts = vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ];

            assert_public_pda_rejects_without_mutation(&mut env, &label, ix, accounts, &[&admin]);
        }
        PdaRoute::WithdrawInsurance => {
            let admin = env.admin.insecure_clone();
            env.top_up_insurance(1_000);
            env.resolve();
            let dest = env.token_account(admin.pubkey(), 0);
            let substitutions = pda_substitution_accounts(&mut env, fault, None);
            let vault = substitute_vault(slot, env.vault, substitutions);
            let vault_authority =
                substitute_vault_authority(slot, env.vault_authority, substitutions);
            let ix = ProgInstruction::WithdrawInsurance { amount: 100 };
            let accounts = vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ];

            assert_public_pda_rejects_without_mutation(&mut env, &label, ix, accounts, &[&admin]);
        }
        PdaRoute::CloseResolved => {
            let owner = Keypair::new();
            let portfolio = env.create_portfolio(&owner);
            env.deposit(&owner, portfolio, 1_000);
            env.resolve();
            let dest = env.token_account(owner.pubkey(), 0);
            let substitutions = pda_substitution_accounts(&mut env, fault, None);
            let vault = substitute_vault(slot, env.vault, substitutions);
            let vault_authority =
                substitute_vault_authority(slot, env.vault_authority, substitutions);
            let ix = ProgInstruction::CloseResolved {
                fee_rate_per_slot: 0,
            };
            let accounts = vec![
                AccountMeta::new_readonly(owner.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(portfolio, false),
                AccountMeta::new(dest, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ];

            assert_public_pda_rejects_without_mutation(&mut env, &label, ix, accounts, &[]);
        }
        PdaRoute::CloseSlab => {
            let admin = env.admin.insecure_clone();
            env.resolve();
            let dest = env.token_account(admin.pubkey(), 0);
            let substitutions = pda_substitution_accounts(&mut env, fault, None);
            let vault = substitute_vault(slot, env.vault, substitutions);
            let vault_authority =
                substitute_vault_authority(slot, env.vault_authority, substitutions);
            let ix = ProgInstruction::CloseSlab;
            let accounts = vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(vault_authority, false),
                AccountMeta::new(dest, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ];

            assert_public_pda_rejects_without_mutation(&mut env, &label, ix, accounts, &[&admin]);
        }
        PdaRoute::SwapSecondaryForPrimary => {
            let admin = env.admin.insecure_clone();
            let secondary_mint = env.create_mint();
            let secondary_vault = canonical_vault_ata(env.vault_authority, secondary_mint);
            let current_vault_authority = env.vault_authority;
            seed_token_account_for_owner(
                &mut env,
                secondary_vault,
                secondary_mint,
                current_vault_authority,
            );
            env.update_base_unit_mints_with_cu(env.mint, secondary_mint);
            let primary_source = env.token_account(admin.pubkey(), 100);
            let secondary_dest = env.token_account_for_mint(secondary_mint, admin.pubkey(), 0);
            let substitutions = pda_substitution_accounts(&mut env, fault, Some(secondary_mint));
            let primary_vault = substitute_vault(slot, env.vault, substitutions);
            let secondary_vault = substitute_secondary_vault(slot, secondary_vault, substitutions);
            let vault_authority =
                substitute_vault_authority(slot, env.vault_authority, substitutions);
            let ix = ProgInstruction::SwapSecondaryForPrimary { amount: 100 };
            let accounts = vec![
                AccountMeta::new(admin.pubkey(), true),
                AccountMeta::new_readonly(env.market, false),
                AccountMeta::new(primary_source, false),
                AccountMeta::new(primary_vault, false),
                AccountMeta::new(secondary_dest, false),
                AccountMeta::new(secondary_vault, false),
                AccountMeta::new_readonly(vault_authority, false),
                AccountMeta::new_readonly(spl_token::ID, false),
            ];

            assert_public_pda_rejects_without_mutation(&mut env, &label, ix, accounts, &[&admin]);
        }
    }
}

#[test]
fn v16_program_public_pda_substitution_matrix_rejects_without_mutation() {
    for route in PDA_ROUTES {
        for slot in route.pda_slots() {
            for fault in PDA_FAULTS {
                exercise_public_pda_substitution(route, *slot, fault);
            }
        }
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
