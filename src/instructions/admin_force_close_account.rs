//! Tag 21 — AdminForceCloseAccount. Admin force-closes an abandoned
//! account on a resolved market. Same engine entry point as
//! `force_close_resolved` (tag 30), but gated on `header.admin`
//! instead of the `force_close_delay_slots` cooldown.
//!
//! Wire format:
//!   `[21u8] [user_idx: u16]`   (3 bytes)
//!
//! Accounts (strict order, matches legacy):
//!   1. admin (Signer)
//!   2. slab (Account<PercolatorSlab>, mut)
//!   3. vault (UncheckedAccount, mut)
//!   4. owner_ata (UncheckedAccount, mut)  — destination for any payout
//!   5. vault_pda (UncheckedAccount)
//!   6. token_program (UncheckedAccount)
//!   7. clock (Sysvar<Clock>)             — present for ABI parity;
//!      not consulted (resolved markets are time-frozen at the
//!      engine's `resolved_slot`)

use crate::cpi;
use crate::errors::{map_risk_error, PercolatorError};
use crate::guards::{
    require_admin, require_initialized, require_no_reentrancy, slab_shape_guard,
};
use crate::processor::{check_idx, sync_account_fee};
use crate::state::{self, PercolatorSlab};
use crate::zc;
use anchor_lang_v2::prelude::*;
use solana_program_error::ProgramError;

#[derive(Accounts)]
pub struct AdminForceCloseAccount {
    pub admin: Signer,
    #[account(mut)]
    pub slab: Account<PercolatorSlab>,
    /// CHECK: validated against `MarketConfig.vault_pubkey` + PDA auth.
    #[account(mut)]
    pub vault: UncheckedAccount,
    /// CHECK: validated as the closed account's owner ATA when payout
    /// is nonzero.
    #[account(mut)]
    pub owner_ata: UncheckedAccount,
    /// CHECK: must equal the program-derived vault authority PDA.
    pub vault_pda: UncheckedAccount,
    /// CHECK: must be the SPL Token program.
    pub token_program: UncheckedAccount,
    /// Present for ABI parity with the legacy 7-account layout.
    /// Not consulted (the resolved-market path uses `resolved_slot`
    /// from engine state instead of `clock.slot`).
    pub _clock: Sysvar<Clock>,
}

pub fn handler(ctx: &mut Context<AdminForceCloseAccount>, user_idx: u16) -> Result<()> {
    cpi::verify_token_program(ctx.accounts.token_program.account())?;

    slab_shape_guard(&ctx.accounts.slab)?;
    let admin_addr = *ctx.accounts.admin.address();
    let slab_addr = *ctx.accounts.slab.account().address();
    let vault_view = *ctx.accounts.vault.account();
    let owner_ata_view = *ctx.accounts.owner_ata.account();
    let pda_view = *ctx.accounts.vault_pda.account();
    let token_program_view = *ctx.accounts.token_program.account();

    let data: &mut [u8] = state::slab_data_mut(&mut ctx.accounts.slab);
    require_no_reentrancy(data)?;
    require_initialized(data)?;

    let header = state::read_header(data);
    require_admin(header.admin, &admin_addr)?;

    if !zc::engine_ref(data)?.is_resolved() {
        return Err(ProgramError::InvalidAccountData.into());
    }

    let config = state::read_config(data);
    let mint = Address::from(config.collateral_mint);
    let auth = cpi::derive_vault_authority_with_bump(
        &crate::ID,
        &slab_addr,
        config.vault_authority_bump,
    )?;
    cpi::verify_vault(&vault_view, &auth, &mint, &Address::from(config.vault_pubkey))?;
    if pda_view.address() != &auth {
        return Err(ProgramError::InvalidArgument.into());
    }

    let engine = zc::engine_mut(data)?;
    let (price, resolved_slot) = engine.resolved_context();
    if price == 0 {
        return Err(ProgramError::InvalidAccountData.into());
    }
    check_idx(engine, user_idx)?;
    let owner_addr = Address::from(engine.accounts[user_idx as usize].owner);

    // Realize recurring maintenance fees to the resolved anchor BEFORE
    // force_close_resolved (engine doesn't sync the fee cursor itself).
    // No-op when maintenance_fee_per_slot == 0.
    sync_account_fee(engine, &config, user_idx, resolved_slot)?;

    // Engine v12.18.6+: slot arg removed — engine pulls resolved_slot
    // from its own state (§9.9).
    let amt_units = match engine
        .force_close_resolved_not_atomic(user_idx)
        .map_err(map_risk_error)?
    {
        percolator::ResolvedCloseResult::ProgressOnly => return Ok(()),
        percolator::ResolvedCloseResult::Closed(payout) => payout,
    };

    let amt_units_u64: u64 = amt_units
        .try_into()
        .map_err(|_| PercolatorError::EngineOverflow)?;

    // Only verify owner ATA when there's a nonzero payout.
    if amt_units_u64 > 0 {
        cpi::verify_token_account(&owner_ata_view, &owner_addr, &mint)?;
    }

    {
        let mut buf = state::read_risk_buffer(data);
        buf.remove(user_idx);
        state::write_risk_buffer(data, &buf);
    }

    let base_to_pay = crate::units::units_to_base_checked(amt_units_u64, config.unit_scale)
        .ok_or(PercolatorError::EngineOverflow)?;

    let slab_bytes = slab_addr.to_bytes();
    let bump_arr: [u8; 1] = [config.vault_authority_bump];
    let seeds: [&[u8]; 3] = [b"vault", &slab_bytes, &bump_arr];

    cpi::withdraw(
        &token_program_view,
        &vault_view,
        &owner_ata_view,
        &pda_view,
        base_to_pay,
        &seeds,
    )?;
    Ok(())
}
