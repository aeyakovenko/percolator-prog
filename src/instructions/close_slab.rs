//! Tag 13 — CloseSlab. Admin teardown of a fully-resolved + drained
//! market. Drains any stranded vault tokens to admin, closes the
//! vault token account (recovering its rent), zeroes the slab data,
//! and transfers all slab lamports to admin.
//!
//! Wire format:
//!   `[13u8]`   (payload-less)
//!
//! Accounts (strict order, matches legacy):
//!   1. dest (Signer, mut)             — admin (header.admin)
//!   2. slab (Account<PercolatorSlab>, mut)
//!   3. vault (UncheckedAccount, mut)
//!   4. vault_auth (UncheckedAccount)  — vault PDA
//!   5. dest_ata (UncheckedAccount, mut)
//!   6. token_program (UncheckedAccount)
//!
//! Preconditions: market resolved, all engine vault/insurance balances
//! zero, no live accounts. CloseSlab is gated by `header.admin`;
//! operators who burn admin trap the slab rent — accepted cost of the
//! fully admin-free terminal state.

use crate::cpi;
use crate::errors::PercolatorError;
use crate::guards::{
    require_admin, require_initialized, require_no_reentrancy, slab_shape_guard,
};
use crate::state::{self, PercolatorSlab};
use crate::zc;
use anchor_lang_v2::prelude::*;
use solana_program_error::ProgramError;

#[derive(Accounts)]
pub struct CloseSlab {
    /// Admin (header.admin); receives drained tokens + slab rent.
    #[account(mut)]
    pub dest: Signer,
    #[account(mut)]
    pub slab: Account<PercolatorSlab>,
    /// CHECK: validated against `MarketConfig.vault_pubkey` + PDA auth.
    #[account(mut)]
    pub vault: crate::spl::TokenAccount,
    /// CHECK: framework-validated via `seeds` + `bump` constraint.
    #[account(seeds = [b"vault", slab], bump = slab.config.vault_authority_bump())]
    pub vault_auth: UncheckedAccount,
    /// CHECK: validated as admin's SPL token ATA when there are
    /// stranded tokens to drain.
    #[account(mut)]
    pub dest_ata: crate::spl::TokenAccount,
    pub token_program: Program<crate::spl::TokenProgram>,
}

pub fn handler(ctx: &mut Context<CloseSlab>) -> Result<()> {


    slab_shape_guard(&ctx.accounts.slab)?;
    let dest_addr = *ctx.accounts.dest.address();
    let slab_addr = *ctx.accounts.slab.account().address();
    let mut dest_view = *ctx.accounts.dest.account();
    let vault_view = *ctx.accounts.vault.account();
    let vault_auth_view = *ctx.accounts.vault_auth.account();
    let dest_ata_view = *ctx.accounts.dest_ata.account();
    let token_program_view = *ctx.accounts.token_program.account();
    let mut slab_view = *ctx.accounts.slab.account();

    {
        let data: &mut [u8] = state::slab_data_mut(&mut ctx.accounts.slab);
        require_no_reentrancy(data)?;
        require_initialized(data)?;

        // Lifecycle: must be resolved.
        if !zc::engine_ref(data)?.is_resolved() {
            return Err(ProgramError::InvalidAccountData.into());
        }

        let header = state::read_header(data);
        require_admin(header.admin, &dest_addr)?;

        let config = state::read_config(data);
        let mint = Address::from(config.collateral_mint);
        let auth = *vault_auth_view.address();
        cpi::verify_vault(&vault_view, &auth, &mint, &Address::from(config.vault_pubkey))?;

        // Engine accounting must be zero before we tear down.
        let engine = zc::engine_ref(data)?;
        if !engine.vault.is_zero() {
            return Err(PercolatorError::EngineInsufficientBalance.into());
        }
        if !engine.insurance_fund.balance.is_zero() {
            return Err(PercolatorError::EngineInsufficientBalance.into());
        }
        if engine.num_used_accounts != 0 {
            return Err(PercolatorError::EngineAccountNotFound.into());
        }

        // Drain any stranded vault tokens (unsolicited transfers, dust).
        let stranded = {
            let acc = pinocchio_token::state::Account::from_account_view(&vault_view)
                .map_err(|_| ProgramError::from(PercolatorError::InvalidVaultAta))?;
            acc.amount()
        };

        let slab_bytes = slab_addr.to_bytes();
        let bump_arr: [u8; 1] = [config.vault_authority_bump];
        let seeds: [&[u8]; 3] = [b"vault", &slab_bytes, &bump_arr];

        if stranded > 0 {
            cpi::verify_token_account(&dest_ata_view, &dest_addr, &mint)?;
            cpi::withdraw(
                &token_program_view,
                &vault_view,
                &dest_ata_view,
                &vault_auth_view,
                stranded,
                &seeds,
            )?;
        }

        // Close the vault token account, recovering its rent into dest.
        cpi::close_token_account(
            &token_program_view,
            &vault_view,
            &dest_view,
            &vault_auth_view,
            &seeds,
        )?;

        // Zero the slab data so the account can't be reused as a slab.
        for b in data.iter_mut() {
            *b = 0;
        }
    }

    // Sweep slab lamports to dest.
    let slab_lamports = slab_view.lamports();
    let dest_new = dest_view
        .lamports()
        .checked_add(slab_lamports)
        .ok_or(PercolatorError::EngineOverflow)?;
    slab_view.set_lamports(0);
    dest_view.set_lamports(dest_new);
    Ok(())
}
