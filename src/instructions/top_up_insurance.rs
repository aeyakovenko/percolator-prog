//! Tag 9 — TopUpInsurance. User deposits collateral directly into the
//! insurance fund (gated by `insurance_withdraw_deposits_only` flag).
//!
//! Wire format:
//!   `[9u8] [amount: u64]`   (9 bytes)
//!
//! Accounts (strict order, matches legacy):
//!   1. user (Signer)
//!   2. slab (Account<SlabHeader>, mut)
//!   3. user_ata (UncheckedAccount, mut)
//!   4. vault (UncheckedAccount, mut)
//!   5. token_program (UncheckedAccount)
//!   6. clock (Sysvar<Clock>)

use crate::cpi;
use crate::errors::{map_risk_error, PercolatorError};
use crate::guards::{require_initialized, require_no_reentrancy, slab_shape_guard};
use crate::oracle;
use crate::processor::{check_no_oracle_live_envelope, insurance_withdraw_deposits_only};
use crate::state::{self, SlabHeader};
use crate::zc;
use anchor_lang_v2::prelude::*;
use solana_program_error::ProgramError;

#[derive(Accounts)]
pub struct TopUpInsurance {
    pub user: Signer,
    #[account(mut)]
    pub slab: Account<SlabHeader>,
    /// CHECK: validated as user's SPL token ATA in the handler.
    #[account(mut)]
    pub user_ata: UncheckedAccount,
    /// CHECK: validated against `MarketConfig.vault_pubkey` + program-
    /// derived vault authority in the handler.
    #[account(mut)]
    pub vault: UncheckedAccount,
    /// CHECK: must be the SPL Token program (`pinocchio_token::ID`).
    pub token_program: UncheckedAccount,
    pub clock: Sysvar<Clock>,
}

pub fn handler(ctx: &mut Context<TopUpInsurance>, amount: u64) -> Result<()> {
    cpi::verify_token_program(ctx.accounts.token_program.account())?;
    if amount == 0 {
        return Err(ProgramError::InvalidArgument.into());
    }

    slab_shape_guard(&ctx.accounts.slab)?;
    let clock = *ctx.accounts.clock;
    let user_addr = *ctx.accounts.user.address();
    let slab_addr = *ctx.accounts.slab.account().address();
    let user_ata_view = *ctx.accounts.user_ata.account();
    let vault_view = *ctx.accounts.vault.account();
    let token_program_view = *ctx.accounts.token_program.account();
    let user_view = *ctx.accounts.user.account();

    let data: &mut [u8] = state::slab_data_mut(&mut ctx.accounts.slab);
    require_no_reentrancy(data)?;
    require_initialized(data)?;

    if zc::engine_ref(data)?.is_resolved() {
        return Err(ProgramError::InvalidAccountData.into());
    }

    let mut config = state::read_config(data);
    insurance_withdraw_deposits_only(&config)?;

    let mint = Address::from(config.collateral_mint);
    let vault_pubkey = Address::from(config.vault_pubkey);
    let auth = cpi::derive_vault_authority_with_bump(
        &crate::ID,
        &slab_addr,
        config.vault_authority_bump,
    )?;
    cpi::verify_vault(&vault_view, &auth, &mint, &vault_pubkey)?;
    cpi::verify_token_account(&user_ata_view, &user_addr, &mint)?;

    if oracle::permissionless_stale_matured(&config, clock.slot) {
        return Err(PercolatorError::OracleStale.into());
    }
    check_no_oracle_live_envelope(zc::engine_ref(data)?, clock.slot)?;

    // Reject misaligned deposits — dust would be silently donated.
    let (_units_check, dust_check) = crate::units::base_to_units(amount, config.unit_scale);
    if dust_check != 0 {
        return Err(ProgramError::InvalidArgument.into());
    }

    let (units, _dust) = crate::units::base_to_units(amount, config.unit_scale);
    let new_deposit_remaining = config
        .insurance_withdraw_deposit_remaining
        .checked_add(units)
        .ok_or(PercolatorError::EngineOverflow)?;

    cpi::deposit(
        &token_program_view,
        &user_ata_view,
        &vault_view,
        &user_view,
        amount,
    )?;

    let engine = zc::engine_mut(data)?;
    engine
        .top_up_insurance_fund(units as u128, clock.slot)
        .map_err(map_risk_error)?;
    config.insurance_withdraw_deposit_remaining = new_deposit_remaining;
    state::write_config(data, &config);
    Ok(())
}
