//! Tag 3 — DepositCollateral. Owner deposits collateral into their
//! engine account. Live markets only. The deposit path does NOT
//! accrue funding (per spec §10.2: pure capital transfer); the
//! engine's `check_live_accrual_envelope` inside `deposit_not_atomic`
//! is the only staleness gate.
//!
//! Wire format:
//!   `[3u8] [user_idx: u16] [amount: u64]`   (11 bytes)
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
use crate::processor::{check_idx, check_no_oracle_live_envelope, sync_account_fee_bounded_to_market};
use crate::state::{self, SlabHeader};
use crate::zc;
use anchor_lang_v2::prelude::*;
use solana_program_error::ProgramError;

#[derive(Accounts)]
pub struct DepositCollateral {
    pub user: Signer,
    #[account(mut)]
    pub slab: Account<SlabHeader>,
    /// CHECK: validated as user's SPL token ATA in the handler.
    #[account(mut)]
    pub user_ata: UncheckedAccount,
    /// CHECK: validated against `MarketConfig.vault_pubkey` + PDA auth.
    #[account(mut)]
    pub vault: UncheckedAccount,
    /// CHECK: must be the SPL Token program.
    pub token_program: UncheckedAccount,
    pub clock: Sysvar<Clock>,
}

pub fn handler(
    ctx: &mut Context<DepositCollateral>,
    user_idx: u16,
    amount: u64,
) -> Result<()> {
    cpi::verify_token_program(ctx.accounts.token_program.account())?;
    if amount == 0 {
        return Err(ProgramError::InvalidArgument.into());
    }

    slab_shape_guard(&ctx.accounts.slab)?;
    let user_addr = *ctx.accounts.user.address();
    let slab_addr = *ctx.accounts.slab.account().address();
    let user_ata_view = *ctx.accounts.user_ata.account();
    let vault_view = *ctx.accounts.vault.account();
    let token_program_view = *ctx.accounts.token_program.account();
    let user_view = *ctx.accounts.user.account();
    let clock = *ctx.accounts.clock;

    let data: &mut [u8] = state::slab_data_mut(&mut ctx.accounts.slab);
    require_no_reentrancy(data)?;
    require_initialized(data)?;

    if zc::engine_ref(data)?.is_resolved() {
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

    // TVL:insurance cap (admin opt-in). Enforced BEFORE the SPL
    // transfer so rejected deposits don't move funds.
    if config.tvl_insurance_cap_mult > 0 {
        let (capital_units_sim, _) = crate::units::base_to_units(amount, config.unit_scale);
        let engine_r = zc::engine_ref(data)?;
        let ins = engine_r.insurance_fund.balance.get();
        let c_tot_new = engine_r.c_tot.get().saturating_add(capital_units_sim as u128);
        let cap = ins.saturating_mul(config.tvl_insurance_cap_mult as u128);
        if c_tot_new > cap {
            return Err(PercolatorError::DepositCapExceeded.into());
        }
    }

    // Transfer base tokens to vault.
    cpi::deposit(&token_program_view, &user_ata_view, &vault_view, &user_view, amount)?;

    let (units, _dust) = crate::units::base_to_units(amount, config.unit_scale);

    let engine = zc::engine_mut(data)?;
    check_idx(engine, user_idx)?;
    let owner = engine.accounts[user_idx as usize].owner;
    if !crate::policy::owner_ok(owner, user_addr.to_bytes()) {
        return Err(PercolatorError::EngineUnauthorized.into());
    }

    // No-oracle path: bounded-to-market fee sync (§10.7).
    sync_account_fee_bounded_to_market(engine, &config, user_idx, clock.slot)?;
    engine
        .deposit_not_atomic(user_idx, units as u128, clock.slot)
        .map_err(map_risk_error)?;
    Ok(())
}
