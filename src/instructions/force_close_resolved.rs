//! Tag 30 — ForceCloseResolved. Permissionless force-close on
//! resolved markets after the configured `force_close_delay_slots`
//! cooldown elapses. Mirrors AdminForceCloseAccount but trades the
//! admin signature for a wall-clock delay.
//!
//! Wire format:
//!   `[30u8] [user_idx: u16]`   (3 bytes)
//!
//! Accounts (strict order, matches legacy):
//!   1. slab (Account<PercolatorSlab>, mut)
//!   2. vault (UncheckedAccount, mut)
//!   3. owner_ata (UncheckedAccount, mut)
//!   4. vault_pda (UncheckedAccount)
//!   5. token_program (UncheckedAccount)
//!   6. clock (Sysvar<Clock>)             — used for the cooldown gate

use crate::cpi;
use crate::errors::{map_risk_error, PercolatorError};
use crate::guards::{require_initialized, require_no_reentrancy, slab_shape_guard};
use crate::processor::{check_idx, sync_account_fee};
use crate::state::{self, PercolatorSlab};
use crate::zc;
use anchor_lang_v2::prelude::*;
use solana_program_error::ProgramError;

#[derive(Accounts)]
pub struct ForceCloseResolved {
    #[account(mut)]
    pub slab: Account<PercolatorSlab>,
    /// CHECK: validated against `MarketConfig.vault_pubkey` + PDA auth.
    #[account(mut)]
    pub vault: crate::spl::TokenAccount,
    /// CHECK: validated as the closed account's owner ATA when payout
    /// is nonzero.
    #[account(mut)]
    pub owner_ata: crate::spl::TokenAccount,
    /// CHECK: framework-validated via `seeds` + `bump` constraint.
    #[account(seeds = [b"vault", slab], bump = slab.config.vault_authority_bump())]
    pub vault_pda: UncheckedAccount,
    pub token_program: Program<crate::spl::TokenProgram>,
    pub clock: Sysvar<Clock>,
}

pub fn handler(ctx: &mut Context<ForceCloseResolved>, user_idx: u16) -> Result<()> {


    slab_shape_guard(&ctx.accounts.slab)?;
    let slab_addr = *ctx.accounts.slab.account().address();
    let vault_view = *ctx.accounts.vault.account();
    let owner_ata_view = *ctx.accounts.owner_ata.account();
    let pda_view = *ctx.accounts.vault_pda.account();
    let token_program_view = *ctx.accounts.token_program.account();
    let clock = *ctx.accounts.clock;

    let data: &mut [u8] = state::slab_data_mut(&mut ctx.accounts.slab);
    require_no_reentrancy(data)?;
    require_initialized(data)?;

    let resolved_slot = {
        let eng = zc::engine_ref(data)?;
        if !eng.is_resolved() {
            return Err(ProgramError::InvalidAccountData.into());
        }
        eng.resolved_context().1
    };

    let config = state::read_config(data);
    if config.force_close_delay_slots == 0 {
        return Err(PercolatorError::InvalidConfigParam.into());
    }
    if clock.slot.saturating_sub(resolved_slot) < config.force_close_delay_slots {
        return Err(ProgramError::InvalidAccountData.into());
    }

    let mint = Address::from(config.collateral_mint);
    let auth = *pda_view.address();
    cpi::verify_vault(&vault_view, &auth, &mint, &Address::from(config.vault_pubkey))?;

    let engine = zc::engine_mut(data)?;
    let (price, resolved_slot) = engine.resolved_context();
    if price == 0 {
        return Err(ProgramError::InvalidAccountData.into());
    }
    check_idx(engine, user_idx)?;
    let owner_addr = Address::from(engine.accounts[user_idx as usize].owner);

    // Realize recurring maintenance fees to the resolved anchor BEFORE
    // force_close_resolved. No-op when maintenance_fee_per_slot == 0.
    sync_account_fee(engine, &config, user_idx, resolved_slot)?;

    // Engine v12.18.6+: engine pulls resolved_slot from its own state.
    let amt_units = match engine
        .force_close_resolved_not_atomic(user_idx)
        .map_err(map_risk_error)?
    {
        percolator::ResolvedCloseResult::ProgressOnly => return Ok(()),
        percolator::ResolvedCloseResult::Closed(payout) => payout,
    };

    if amt_units > 0 {
        cpi::verify_token_account(&owner_ata_view, &owner_addr, &mint)?;
    }

    let amt_units_u64: u64 = amt_units
        .try_into()
        .map_err(|_| PercolatorError::EngineOverflow)?;

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
