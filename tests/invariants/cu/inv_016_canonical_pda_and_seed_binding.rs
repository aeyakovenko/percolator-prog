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
//! account bytes. Market-address tombstones and portfolio incarnation/episode checks compose with
//! these stateless addresses: a same-pubkey portfolio recreation derives the same matcher delegate,
//! but its capability bytes are zero and cannot authorize CPI until the new owner state explicitly
//! grants it again.

use super::*;

fn inv016_source_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let start = source
        .find(start)
        .unwrap_or_else(|| panic!("missing production source boundary {start}"));
    let tail = &source[start..];
    let end = tail
        .find(end)
        .unwrap_or_else(|| panic!("missing production source successor {end}"));
    &tail[..end]
}

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
    WithdrawResolvedInsuranceAsset,
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
    PdaRoute::WithdrawResolvedInsuranceAsset,
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
            | PdaRoute::WithdrawResolvedInsuranceAsset
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

fn noncanonical_vault_token_address(vault_authority: Pubkey, mint: Pubkey) -> Pubkey {
    let associated_token_program =
        solana_sdk::pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
    let (_, canonical_bump) = Pubkey::find_program_address(
        &[
            vault_authority.as_ref(),
            spl_token::ID.as_ref(),
            mint.as_ref(),
        ],
        &associated_token_program,
    );
    for bump in 0..=u8::MAX {
        if bump == canonical_bump {
            continue;
        }
        let bump_seed = [bump];
        if let Ok(key) = Pubkey::create_program_address(
            &[
                vault_authority.as_ref(),
                spl_token::ID.as_ref(),
                mint.as_ref(),
                &bump_seed,
            ],
            &associated_token_program,
        ) {
            return key;
        }
    }
    panic!("could not find a noncanonical bump for the canonical vault ATA seed tuple");
}

#[derive(Clone, Copy, Debug)]
enum MatcherDelegateFault {
    WrongBump,
    CrossRole,
    CrossMarket,
    CrossPortfolio,
    CrossOwner,
    CrossMatcherProgram,
    CrossMatcherContext,
    ReorderedProgramAndContext,
    OmittedContext,
}

const MATCHER_DELEGATE_FAULTS: [MatcherDelegateFault; 9] = [
    MatcherDelegateFault::WrongBump,
    MatcherDelegateFault::CrossRole,
    MatcherDelegateFault::CrossMarket,
    MatcherDelegateFault::CrossPortfolio,
    MatcherDelegateFault::CrossOwner,
    MatcherDelegateFault::CrossMatcherProgram,
    MatcherDelegateFault::CrossMatcherContext,
    MatcherDelegateFault::ReorderedProgramAndContext,
    MatcherDelegateFault::OmittedContext,
];

fn noncanonical_matcher_delegate(
    program_id: Pubkey,
    market: Pubkey,
    maker: Pubkey,
    maker_owner: Pubkey,
    matcher_program: Pubkey,
    matcher_context: Pubkey,
) -> Pubkey {
    let (_, canonical_bump) = Pubkey::find_program_address(
        &[
            b"matcher",
            market.as_ref(),
            maker.as_ref(),
            maker_owner.as_ref(),
            matcher_program.as_ref(),
            matcher_context.as_ref(),
        ],
        &program_id,
    );
    for bump in 0..=u8::MAX {
        if bump == canonical_bump {
            continue;
        }
        let bump_seed = [bump];
        if let Ok(key) = Pubkey::create_program_address(
            &[
                b"matcher",
                market.as_ref(),
                maker.as_ref(),
                maker_owner.as_ref(),
                matcher_program.as_ref(),
                matcher_context.as_ref(),
                &bump_seed,
            ],
            &program_id,
        ) {
            return key;
        }
    }
    panic!("could not find a noncanonical bump for the matcher-delegate seed tuple");
}

fn matcher_delegate_fault_key(
    fault: MatcherDelegateFault,
    program_id: Pubkey,
    market: Pubkey,
    maker: Pubkey,
    maker_owner: Pubkey,
    matcher_program: Pubkey,
    matcher_context: Pubkey,
) -> Pubkey {
    match fault {
        MatcherDelegateFault::WrongBump => noncanonical_matcher_delegate(
            program_id,
            market,
            maker,
            maker_owner,
            matcher_program,
            matcher_context,
        ),
        MatcherDelegateFault::CrossRole => {
            Pubkey::find_program_address(&[b"vault", market.as_ref()], &program_id).0
        }
        MatcherDelegateFault::CrossMarket => matcher_delegate_key(
            &program_id,
            &Pubkey::new_unique(),
            &maker,
            &maker_owner,
            &matcher_program,
            &matcher_context,
        ),
        MatcherDelegateFault::CrossPortfolio => matcher_delegate_key(
            &program_id,
            &market,
            &Pubkey::new_unique(),
            &maker_owner,
            &matcher_program,
            &matcher_context,
        ),
        MatcherDelegateFault::CrossOwner => matcher_delegate_key(
            &program_id,
            &market,
            &maker,
            &Pubkey::new_unique(),
            &matcher_program,
            &matcher_context,
        ),
        MatcherDelegateFault::CrossMatcherProgram => matcher_delegate_key(
            &program_id,
            &market,
            &maker,
            &maker_owner,
            &Pubkey::new_unique(),
            &matcher_context,
        ),
        MatcherDelegateFault::CrossMatcherContext => matcher_delegate_key(
            &program_id,
            &market,
            &maker,
            &maker_owner,
            &matcher_program,
            &Pubkey::new_unique(),
        ),
        MatcherDelegateFault::ReorderedProgramAndContext => {
            Pubkey::find_program_address(
                &[
                    b"matcher",
                    market.as_ref(),
                    maker.as_ref(),
                    maker_owner.as_ref(),
                    matcher_context.as_ref(),
                    matcher_program.as_ref(),
                ],
                &program_id,
            )
            .0
        }
        MatcherDelegateFault::OmittedContext => {
            Pubkey::find_program_address(
                &[
                    b"matcher",
                    market.as_ref(),
                    maker.as_ref(),
                    maker_owner.as_ref(),
                    matcher_program.as_ref(),
                ],
                &program_id,
            )
            .0
        }
    }
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
                authority_epoch: 0,
                market_id: 0,
                intent_id: 0,
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
                authority_epoch: 0,
                market_id: 0,
                intent_id: 0,
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
                authority_epoch: 0,
                market_id: 0,
                intent_id: 0,
                domain: 0,
                backing_fee_bps: 0,
                insurance_share_bps: 0,
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
                intent_id: u64::MAX,
                authority_epoch: 0,
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
            let market_id = env.asset_market_id(0);
            let ix = ProgInstruction::WithdrawBackingBucket {
                domain: 0,
                market_id,
                authority_epoch: 0,
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
        PdaRoute::WithdrawResolvedInsuranceAsset => {
            let admin = env.admin.insecure_clone();
            env.top_up_insurance(1_000);
            env.resolve();
            let dest = env.token_account(admin.pubkey(), 0);
            let substitutions = pda_substitution_accounts(&mut env, fault, None);
            let vault = substitute_vault(slot, env.vault, substitutions);
            let vault_authority =
                substitute_vault_authority(slot, env.vault_authority, substitutions);
            let ix = env.withdraw_insurance_asset_instruction(admin.pubkey(), 0, 100);
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
            let ix = ProgInstruction::CloseSlab { authority_epoch: 0 };
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
            let ix = ProgInstruction::SwapSecondaryForPrimary {
                amount: 100,
                authority_epoch: env.control_sequences(0).authority_epoch,
            };
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
    let noncanonical_vault = noncanonical_vault_token_address(env.vault_authority, env.mint);
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
fn v16_bpf_auth_matcher_init_binds_every_delegate_seed_and_canonical_bump() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let lp_owner = Keypair::new();
    let lp = env.create_portfolio(&lp_owner);

    for fault in MATCHER_DELEGATE_FAULTS {
        let bad_ctx = Keypair::new();
        system_create_account_for_test(
            &mut env.svm,
            &env.payer,
            &bad_ctx,
            MATCHER_CONTEXT_LEN,
            matcher_program,
        );
        let bad_delegate = matcher_delegate_fault_key(
            fault,
            env.program_id,
            env.market,
            lp,
            lp_owner.pubkey(),
            matcher_program,
            bad_ctx.pubkey(),
        );
        let before_ctx = env.svm.get_account(&bad_ctx.pubkey()).unwrap();
        let before_market = env.svm.get_account(&env.market).unwrap();
        let before_lp = env.svm.get_account(&lp).unwrap();
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
            "auth matcher init accepted {fault:?} delegate {bad_delegate}"
        );
        assert_eq!(
            env.svm.get_account(&bad_ctx.pubkey()).unwrap(),
            before_ctx,
            "{fault:?} must not initialize or otherwise mutate the matcher context"
        );
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            before_market,
            "{fault:?} must frame the market"
        );
        assert_eq!(
            env.svm.get_account(&lp).unwrap(),
            before_lp,
            "{fault:?} must frame the LP portfolio"
        );
    }

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

#[test]
fn v16_program_reused_matcher_delegate_cannot_revive_closed_portfolio_capability() {
    let mut env = V16CuEnv::new();
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);

    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp = env.create_portfolio(&lp_owner);
    let old_portfolio_id = env.portfolio_id(lp);
    let (context, old_delegate, _) = env.init_auth_matcher_context(matcher_program, &lp_owner, lp);
    assert_eq!(env.portfolio_matcher_config(lp).enabled(), 1);

    // The portfolio is economically empty, so close and publicly recreate its exact address under
    // the same owner. Every matcher-delegate seed is therefore identical across incarnations.
    env.close_portfolio_with_cu(&lp_owner, lp);
    env.svm.expire_blockhash();
    send_raw_tx(
        &mut env.svm,
        &env.payer,
        system_instruction::transfer(&env.payer.pubkey(), &lp, 1_000_000_000),
        &[],
    )
    .expect("System Program re-funds the closed LP address");
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::InitPortfolio,
        vec![
            AccountMeta::new(lp_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(lp, false),
        ],
        &[&lp_owner],
    )
    .expect("wrapper recreates the LP at the same pubkey");

    let new_portfolio_id = env.portfolio_id(lp);
    assert_ne!(new_portfolio_id, old_portfolio_id);
    let reused_delegate = matcher_delegate_key(
        &env.program_id,
        &env.market,
        &lp,
        &lp_owner.pubkey(),
        &matcher_program,
        &context,
    );
    assert_eq!(
        reused_delegate, old_delegate,
        "a stateless PDA intentionally repeats when every seed repeats"
    );
    assert_eq!(
        env.portfolio_matcher_config(lp),
        state::PortfolioMatcherConfigV16::default(),
        "the replacement portfolio must not inherit the prior capability"
    );

    env.deposit(&taker_owner, taker, 1_000_000);
    env.deposit(&lp_owner, lp, 1_000_000);
    let market_before = env.svm.get_account(&env.market).unwrap();
    let taker_before = env.svm.get_account(&taker).unwrap();
    let lp_before = env.svm.get_account(&lp).unwrap();
    let context_before = env.svm.get_account(&context).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();
    env.svm.expire_blockhash();
    let stale_capability = env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &lp_owner,
        lp,
        matcher_program,
        context,
        reused_delegate,
        0,
        POS_SCALE as i128,
        100,
    );
    assert!(
        stale_capability.is_err(),
        "reused PDA alone must not revive the closed portfolio's matcher authority"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&taker).unwrap(), taker_before);
    assert_eq!(env.svm.get_account(&lp).unwrap(), lp_before);
    assert_eq!(env.svm.get_account(&context).unwrap(), context_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    env.set_matcher_config(matcher_program, &lp_owner, lp, context, reused_delegate, 1);
    let open_cu = env
        .try_trade_cpi_with_cu_on_asset(
            &taker_owner,
            taker,
            &lp_owner,
            lp,
            matcher_program,
            context,
            reused_delegate,
            0,
            POS_SCALE as i128,
            100,
        )
        .expect("fresh replacement-portfolio authorization restores CPI liveness");
    assert_cu_within(
        "fresh capability after delegate reuse",
        open_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );
    let close_cu = env
        .try_trade_cpi_with_cu_on_asset(
            &taker_owner,
            taker,
            &lp_owner,
            lp,
            matcher_program,
            context,
            reused_delegate,
            0,
            -(POS_SCALE as i128),
            100,
        )
        .expect("fresh capability retains the ordinary inverse exit");
    assert_cu_within(
        "fresh capability inverse exit after delegate reuse",
        close_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );
    assert!(!has_active_leg_for_asset(&env.portfolio_state(taker), 0));
    assert!(!has_active_leg_for_asset(&env.portfolio_state(lp), 0));
    assert_eq!(env.market_state().1.assets[0].oi_eff_long_q, 0);
    assert_eq!(env.market_state().1.assets[0].oi_eff_short_q, 0);
    assert_eq!(
        env.market_state().1.vault as u64,
        env.token_amount(env.vault)
    );
}

#[test]
fn v16_program_pda_and_token_move_callsite_roster_is_source_complete() {
    let source = include_str!("../../../src/v16_program.rs");
    assert_eq!(
        source.matches("fn derive_vault_authority").count(),
        1,
        "vault-authority derivation must have one canonical implementation"
    );
    assert_eq!(
        source.matches("fn derive_matcher_delegate").count(),
        1,
        "matcher-delegate derivation must have one canonical implementation"
    );
    assert_eq!(
        source.matches("fn canonical_vault_address").count(),
        1,
        "vault ATA derivation must have one canonical implementation"
    );

    let mut token_move_handlers = Vec::new();
    let mut direct_vault_derivation_handlers = Vec::new();
    let mut matcher_derivation_handlers = Vec::new();
    for segment in source.split("\n    fn handle_").skip(1) {
        let name_end = segment
            .find(|character: char| character == '<' || character == '(')
            .expect("handler name terminator");
        let name = &segment[..name_end];
        let moves_tokens =
            segment.contains("transfer_tokens(") || segment.contains("transfer_tokens_signed(");
        if moves_tokens {
            assert!(
                segment.contains("verify_token_program(token_program)?;"),
                "token-moving handler {name} has no exact token-program gate"
            );
            assert!(
                segment.contains("verify_vault_token_account(")
                    || segment.contains("verify_withdrawable_token_accounts(")
                    || segment.contains("verify_domain_withdrawal_preflight("),
                "token-moving handler {name} has no canonical vault-address guard"
            );
            token_move_handlers.push(name);
        }
        if segment.matches("derive_vault_authority(").count()
            > segment.matches("fn derive_vault_authority").count()
        {
            direct_vault_derivation_handlers.push(name);
        }
        if segment.matches("derive_matcher_delegate(").count()
            > segment.matches("fn derive_matcher_delegate").count()
        {
            matcher_derivation_handlers.push(name);
        }
    }
    token_move_handlers.sort_unstable();
    token_move_handlers.dedup();
    direct_vault_derivation_handlers.sort_unstable();
    direct_vault_derivation_handlers.dedup();
    matcher_derivation_handlers.sort_unstable();
    matcher_derivation_handlers.dedup();

    let mut expected_token_move_handlers = vec![
        "claim_resolved_payout_topup",
        "close_resolved",
        "close_slab",
        "cure_and_cancel_close",
        "deposit",
        "swap_secondary_for_primary",
        "top_up_backing_bucket",
        "top_up_insurance",
        "update_asset_lifecycle",
        "withdraw",
        "withdraw_backing_bucket",
        "withdraw_backing_bucket_earnings",
        "withdraw_insurance_asset",
    ];
    expected_token_move_handlers.sort_unstable();
    assert_eq!(
        token_move_handlers, expected_token_move_handlers,
        "every production token-moving handler needs an explicit INV-016 owner"
    );

    let mut expected_direct_vault_derivations = vec![
        "claim_resolved_payout_topup",
        "close_resolved",
        "close_slab",
        "cure_and_cancel_close",
        "deposit",
        "swap_secondary_for_primary",
        "top_up_backing_bucket",
        "top_up_insurance",
        "update_asset_lifecycle",
        "update_base_unit_mints",
        "withdraw",
        "withdraw_insurance_asset",
    ];
    expected_direct_vault_derivations.sort_unstable();
    assert_eq!(
        direct_vault_derivation_handlers, expected_direct_vault_derivations,
        "vault-derivation callsite roster changed without INV-016 review"
    );

    let mut expected_matcher_derivations =
        vec!["batch_trade_cpi", "set_matcher_config", "trade_cpi"];
    expected_matcher_derivations.sort_unstable();
    assert_eq!(
        matcher_derivation_handlers, expected_matcher_derivations,
        "matcher-derivation callsite roster changed without INV-016 review"
    );
}

#[test]
fn v16_program_stateless_pda_incarnation_composition_is_source_complete() {
    let source = include_str!("../../../src/v16_program.rs");
    assert_eq!(
        source.matches("Pubkey::find_program_address(").count(),
        3,
        "a new PDA class needs explicit seed, incarnation, and close/recreate coverage"
    );

    let vault = inv016_source_between(
        source,
        "fn derive_vault_authority(",
        "/// The SPL Associated Token Account program",
    );
    assert!(vault.contains("&[b\"vault\", market_key.as_ref()]"));
    assert!(vault.contains("program_id"));
    let vault_ata = inv016_source_between(source, "fn canonical_vault_address(", "fn expect_key(");
    for seed in [
        "vault_authority.as_ref()",
        "spl_token::ID.as_ref()",
        "mint.as_ref()",
        "&ASSOCIATED_TOKEN_PROGRAM_ID",
    ] {
        assert!(
            vault_ata.contains(seed),
            "canonical vault ATA lost seed {seed}"
        );
    }

    let market_init = inv016_source_between(
        source,
        "pub fn init_market_account_zero_copy(",
        "pub fn read_market(data:",
    );
    let initialized_guard = market_init
        .find("if is_initialized(data)")
        .expect("market initialization rejects every initialized account kind");
    let length_guard = market_init
        .find("if data.len() < MIN_MARKET_ACCOUNT_LEN")
        .expect("fresh market length guard remains present");
    assert!(
        initialized_guard < length_guard,
        "typed market tombstones must reject before fresh-market length admission"
    );
    assert!(source.contains("market_ai.realloc(constants::HEADER_LEN, false)?"));
    assert!(source
        .contains("state::write_closed_market_tombstone(&mut market_ai.try_borrow_mut_data()?)?"));

    let portfolio_init = inv016_source_between(
        source,
        "pub fn init_portfolio_account_zero_copy(",
        "pub fn read_portfolio(data:",
    );
    let zero = portfolio_init
        .find("for b in data.iter_mut()")
        .expect("replacement portfolio storage is zeroed");
    let write_id = portfolio_init
        .find("write_portfolio_id(data, portfolio_id)")
        .expect("replacement portfolio receives a fresh program-assigned id");
    assert!(zero < write_id);

    let binding = inv016_source_between(
        source,
        "fn expect_portfolio_position_binding(",
        "fn reject_missing_pending_liquidation_observations_view(",
    );
    assert!(binding.contains("expect_portfolio_id(data, expected_portfolio_id)?"));
    assert!(binding.contains("state::read_portfolio_position_epoch(data)?"));

    let market_aba = include_str!("../public_sbf/inv_007_no_aba_reuse.rs");
    assert!(market_aba
        .contains("v16_program_whole_market_recreate_aba_matrix_is_public_and_nonvacuous"));
    let portfolio_aba = include_str!("../public_sbf/inv_003_portfolio_incarnation_binding.rs");
    assert!(portfolio_aba
        .contains("v16_program_all_retained_portfolio_intents_reject_after_same_pubkey_recreate"));
    let matcher_transport = include_str!("inv_019_cpi_invocation_and_return_data_binding.rs");
    assert!(matcher_transport
        .contains("v16_program_matcher_cpi_identity_incarnation_census_is_source_complete"));
    assert!(matcher_transport
        .contains("v16_stateful_matcher_context_incarnations_bind_single_and_batch_cpi"));
}
