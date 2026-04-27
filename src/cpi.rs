//! SPL Token CPI helpers + token-account / vault verifiers.
//!
//! Two transfer flavors mirror the legacy `mod collateral`:
//!   - `deposit` — user signs the source ATA debit (`invoke`).
//!   - `withdraw` — vault PDA signs (`invoke_signed` with `[b"vault",
//!     slab_key, &[bump]]`).
//!
//! Account-shape verifiers replace the legacy `verify_vault*` /
//! `verify_token_*` helpers that were deferred during the
//! `processor.rs` port. They read SPL-token account data via
//! `pinocchio_token::state::Account`, which validates owner-program +
//! length on `from_account_view` and exposes typed accessors for
//! mint/owner/amount/state.
//!
//! The matcher-forward CPI helper (`forward_matcher`) is intentionally
//! parked at the bottom — TradeCpi's call site wires it up in a later
//! commit.

#![allow(dead_code)]

use crate::errors::PercolatorError;
use anchor_lang_v2::prelude::*;
use pinocchio::account::AccountView;
use pinocchio::address::Address;
use pinocchio_token::instructions::Transfer;
use pinocchio_token::state::{Account as TokenAccount, AccountState};
use solana_program_error::ProgramError;

// ── Token-program guard ─────────────────────────────────────────────────────

/// Reject if the supplied account isn't the canonical SPL Token program.
/// Matches legacy `verify_token_program` semantics.
pub fn verify_token_program(view: &AccountView) -> Result<()> {
    if view.address() != &pinocchio_token::ID {
        return Err(PercolatorError::InvalidTokenProgram.into());
    }
    if !view.executable() {
        return Err(PercolatorError::InvalidTokenProgram.into());
    }
    Ok(())
}

// ── Token account verifiers ─────────────────────────────────────────────────

fn load_token_account_view<'a>(
    view: &'a AccountView,
) -> Result<pinocchio::account::Ref<'a, TokenAccount>> {
    let acc = TokenAccount::from_account_view(view)
        .map_err(|_| ProgramError::from(PercolatorError::InvalidTokenAccount))?;
    if acc.state() != AccountState::Initialized {
        return Err(PercolatorError::InvalidTokenAccount.into());
    }
    Ok(acc)
}

/// Validate a generic SPL token account: owner-program is the SPL Token
/// program, length matches `Account::LEN`, state is Initialized, and
/// the account's `mint` + `owner` fields match the expected values.
pub fn verify_token_account(
    view: &AccountView,
    expected_owner: &Address,
    expected_mint: &Address,
) -> Result<()> {
    let acc = load_token_account_view(view)?;
    if acc.mint() != expected_mint {
        return Err(PercolatorError::InvalidTokenAccount.into());
    }
    if acc.owner() != expected_owner {
        return Err(PercolatorError::InvalidTokenAccount.into());
    }
    Ok(())
}

/// Validate the vault token account: it must be owned by the program-
/// derived vault authority, hold the configured mint, and equal the
/// vault pubkey stored in `MarketConfig`.
pub fn verify_vault(
    view: &AccountView,
    expected_authority: &Address,
    expected_mint: &Address,
    expected_vault_pubkey: &Address,
) -> Result<()> {
    if view.address() != expected_vault_pubkey {
        return Err(PercolatorError::InvalidVaultAta.into());
    }
    let acc = load_token_account_view(view).map_err(|_| {
        ProgramError::from(PercolatorError::InvalidVaultAta)
    })?;
    if acc.mint() != expected_mint {
        return Err(PercolatorError::InvalidVaultAta.into());
    }
    if acc.owner() != expected_authority {
        return Err(PercolatorError::InvalidVaultAta.into());
    }
    Ok(())
}

/// Same as `verify_vault` plus a balance == 0 invariant. Used by
/// `CloseSlab` to refuse teardown if any user funds remain.
pub fn verify_vault_empty(
    view: &AccountView,
    expected_authority: &Address,
    expected_mint: &Address,
    expected_vault_pubkey: &Address,
) -> Result<()> {
    if view.address() != expected_vault_pubkey {
        return Err(PercolatorError::InvalidVaultAta.into());
    }
    let acc = load_token_account_view(view).map_err(|_| {
        ProgramError::from(PercolatorError::InvalidVaultAta)
    })?;
    if acc.mint() != expected_mint {
        return Err(PercolatorError::InvalidVaultAta.into());
    }
    if acc.owner() != expected_authority {
        return Err(PercolatorError::InvalidVaultAta.into());
    }
    if acc.amount() != 0 {
        return Err(PercolatorError::InvalidVaultAta.into());
    }
    Ok(())
}

// ── Vault PDA derivation ────────────────────────────────────────────────────

/// Derive the canonical vault authority for a slab. Seed scheme is
/// `[b"vault", slab_key]`.
pub fn derive_vault_authority(program_id: &Address, slab_key: &Address) -> (Address, u8) {
    Address::find_program_address(&[b"vault", slab_key.as_ref()], program_id)
}

/// Same as `derive_vault_authority` but uses the bump cached in
/// `MarketConfig.vault_authority_bump` so the on-chain path doesn't
/// pay for `find_program_address` (~1300 CU saving).
pub fn derive_vault_authority_with_bump(
    program_id: &Address,
    slab_key: &Address,
    bump: u8,
) -> Result<Address> {
    Address::create_program_address(&[b"vault", slab_key.as_ref(), &[bump]], program_id)
        .map_err(|_| ProgramError::InvalidSeeds.into())
}

// ── SPL token transfers ─────────────────────────────────────────────────────

/// User → vault deposit. Caller signs the source debit; no PDA seeds.
/// `amount == 0` is a no-op (matches legacy).
pub fn deposit(
    _token_program: &AccountView,
    source: &AccountView,
    dest: &AccountView,
    authority: &AccountView,
    amount: u64,
) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    Transfer::<&AccountView> {
        from: source,
        to: dest,
        authority,
        multisig_signers: &[][..],
        amount,
    }
    .invoke()
    .map_err(Into::into)
}

/// Vault → user withdrawal. Authority is the program-derived vault
/// PDA; `signer_seeds` must derive that authority. Layout matches
/// legacy `collateral::withdraw`: caller passes the full
/// `[seed_a, seed_b, &[bump]]` seed bundle.
pub fn withdraw(
    _token_program: &AccountView,
    source: &AccountView,
    dest: &AccountView,
    authority: &AccountView,
    amount: u64,
    signer_seeds: &[&[u8]],
) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    use pinocchio::cpi;
    let seeds: alloc::vec::Vec<cpi::Seed> = signer_seeds
        .iter()
        .map(|b| cpi::Seed::from(*b))
        .collect();
    let signer = cpi::Signer::from(seeds.as_slice());
    Transfer::<&AccountView> {
        from: source,
        to: dest,
        authority,
        multisig_signers: &[][..],
        amount,
    }
    .invoke_signed(core::slice::from_ref(&signer))
    .map_err(Into::into)
}
