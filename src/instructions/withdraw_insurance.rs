//! Tag 20 — WithdrawInsurance. Unbounded insurance-fund withdrawal
//! gated by `insurance_authority`. Resolved markets only.
//!
//! Wire format:
//!   `[20u8]`   (payload-less)
//!
//! Accounts (strict order, matches legacy):
//!   1. admin (Signer)             — must equal header.insurance_authority
//!   2. slab (Account<PercolatorSlab>, mut)
//!   3. admin_ata (UncheckedAccount, mut)
//!   4. vault (UncheckedAccount, mut)
//!   5. token_program (UncheckedAccount)
//!   6. vault_pda (UncheckedAccount)  — must equal program-derived auth
//!
//! Behavior summary (matches legacy `Instruction::WithdrawInsurance`):
//! resolved-market only, all accounts must already be closed
//! (`engine.num_used_accounts == 0`), then call
//! `engine.withdraw_resolved_insurance_not_atomic` and forward the
//! payout via vault-PDA-signed `cpi::withdraw`.

use crate::cpi;
use crate::errors::{map_risk_error, PercolatorError};
use crate::guards::{
    require_admin, require_initialized, require_no_reentrancy, slab_shape_guard,
};
use crate::state::{self, PercolatorSlab};
use crate::zc;
use anchor_lang_v2::prelude::*;
use solana_program_error::ProgramError;

#[derive(Accounts)]
pub struct WithdrawInsurance {
    pub admin: Signer,
    #[account(mut)]
    pub slab: Account<PercolatorSlab>,
    /// CHECK: validated as admin's SPL token ATA in the handler.
    #[account(mut)]
    pub admin_ata: crate::spl::TokenAccount,
    /// CHECK: validated against `MarketConfig.vault_pubkey` + program-
    /// derived vault authority in the handler.
    #[account(mut)]
    pub vault: crate::spl::TokenAccount,
    pub token_program: Program<crate::spl::TokenProgram>,
    /// Program-derived vault authority. Validated via `seeds` + `bump`
    /// constraint and signed via `signer_seeds` for the SPL Token CPI.
    /// CHECK: framework-validated.
    #[account(seeds = [b"vault", slab], bump = slab.config.vault_authority_bump())]
    pub vault_pda: UncheckedAccount,
}

pub fn handler(ctx: &mut Context<WithdrawInsurance>) -> Result<()> {


    slab_shape_guard(&ctx.accounts.slab)?;
    let admin_addr = *ctx.accounts.admin.address();
    let slab_addr = *ctx.accounts.slab.account().address();
    let admin_ata_view = *ctx.accounts.admin_ata.account();
    let vault_view = *ctx.accounts.vault.account();
    let token_program_view = *ctx.accounts.token_program.account();
    let vault_pda_view = *ctx.accounts.vault_pda.account();

    let data: &mut [u8] = state::slab_data_mut(&mut ctx.accounts.slab);
    require_no_reentrancy(data)?;
    require_initialized(data)?;

    let header = state::read_header(data);
    require_admin(header.insurance_authority, &admin_addr)?;

    if !zc::engine_ref(data)?.is_resolved() {
        return Err(ProgramError::InvalidAccountData.into());
    }

    let config = state::read_config(data);
    let mint = Address::from(config.collateral_mint);
    let vault_pubkey = Address::from(config.vault_pubkey);
    let auth = *vault_pda_view.address();
    cpi::verify_vault(&vault_view, &auth, &mint, &vault_pubkey)?;
    cpi::verify_token_account(&admin_ata_view, &admin_addr, &mint)?;

    let payout_units = {
        let engine = zc::engine_mut(data)?;
        // Require all user accounts to be fully closed first. Stale
        // positions (epoch-mismatched) report effective_pos_q == 0
        // but still hold capital that must settle before sweep.
        if engine.num_used_accounts != 0 {
            return Err(ProgramError::InvalidAccountData.into());
        }
        engine
            .withdraw_resolved_insurance_not_atomic()
            .map_err(map_risk_error)?
    };
    if payout_units == 0 {
        return Ok(()); // Nothing to withdraw.
    }

    let units_u64: u64 = payout_units
        .try_into()
        .map_err(|_| PercolatorError::EngineOverflow)?;
    let base_amount = crate::units::units_to_base_checked(units_u64, config.unit_scale)
        .ok_or(PercolatorError::EngineOverflow)?;

    // Vault PDA seeds: [b"vault", slab_key, &[bump]].
    let slab_bytes = slab_addr.to_bytes();
    let bump_arr: [u8; 1] = [config.vault_authority_bump];
    let seeds: [&[u8]; 3] = [b"vault", &slab_bytes, &bump_arr];

    cpi::withdraw(
        &token_program_view,
        &vault_view,
        &admin_ata_view,
        &vault_pda_view,
        base_amount,
        &seeds,
    )?;
    Ok(())
}
