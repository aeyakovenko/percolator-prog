//! Tag 23 — WithdrawInsuranceLimited. BOUNDED live insurance
//! withdrawal gated by `insurance_operator` (NOT `insurance_authority`).
//! The auth split is structural — an operator with only this kind of
//! authority cannot bypass the per-call cap by routing to tag 20.
//!
//! Wire format:
//!   `[23u8] [amount: u64]`   (9 bytes)
//!
//! Accounts (strict order, matches legacy):
//!   1. operator (Signer)          — must equal header.insurance_operator
//!   2. slab (Account<PercolatorSlab>, mut)
//!   3. operator_ata (UncheckedAccount, mut)
//!   4. vault (UncheckedAccount, mut)
//!   5. token_program (UncheckedAccount)
//!   6. vault_pda (UncheckedAccount)
//!   7. clock (Sysvar<Clock>)
//!
//! Bounds:
//!   - per-call amount ≤ max(MIN_WITHDRAW_FLOOR_UNITS, bps_cap), clamped
//!     to insurance balance, where
//!       bps_cap = insurance × insurance_withdraw_max_bps / 10_000
//!   - cooldown: clock.slot - last_withdraw_slot ≥ cooldown_slots
//!   - if `insurance_withdraw_deposits_only`, additionally clamped to
//!     `insurance_withdraw_deposit_remaining`.
//!
//! The 10-unit floor (anti-Zeno) lets the fund drain to zero even when
//! `max_bps × insurance < 10`.
//!
//! Live markets only. Resolved markets use tag 20 (which also folds
//! the terminal-surplus sweep into the payout — not done here).

use crate::cpi;
use crate::errors::{map_risk_error, PercolatorError};
use crate::guards::{
    require_admin, require_initialized, require_no_reentrancy, slab_shape_guard,
};
use crate::oracle;
use crate::processor::{insurance_withdraw_deposits_only, reject_any_target_lag};
use crate::state::{self, PercolatorSlab};
use crate::zc;
use anchor_lang_v2::prelude::*;
use solana_program_error::ProgramError;

const MIN_WITHDRAW_FLOOR_UNITS: u128 = 10;

#[derive(Accounts)]
pub struct WithdrawInsuranceLimited {
    pub operator: Signer,
    #[account(mut)]
    pub slab: Account<PercolatorSlab>,
    /// CHECK: validated as operator's SPL token ATA in the handler.
    #[account(mut)]
    pub operator_ata: UncheckedAccount,
    /// CHECK: validated against `MarketConfig.vault_pubkey` + PDA auth.
    #[account(mut)]
    pub vault: UncheckedAccount,
    /// CHECK: must be the SPL Token program.
    pub token_program: UncheckedAccount,
    /// CHECK: must equal the program-derived vault authority PDA.
    pub vault_pda: UncheckedAccount,
    pub clock: Sysvar<Clock>,
}

pub fn handler(ctx: &mut Context<WithdrawInsuranceLimited>, amount: u64) -> Result<()> {
    cpi::verify_token_program(ctx.accounts.token_program.account())?;

    slab_shape_guard(&ctx.accounts.slab)?;
    let operator_addr = *ctx.accounts.operator.address();
    let slab_addr = *ctx.accounts.slab.account().address();
    let operator_ata_view = *ctx.accounts.operator_ata.account();
    let vault_view = *ctx.accounts.vault.account();
    let token_program_view = *ctx.accounts.token_program.account();
    let vault_pda_view = *ctx.accounts.vault_pda.account();
    let clock = *ctx.accounts.clock;

    let data: &mut [u8] = state::slab_data_mut(&mut ctx.accounts.slab);
    require_no_reentrancy(data)?;
    require_initialized(data)?;

    // Live markets only.
    if zc::engine_ref(data)?.is_resolved() {
        return Err(ProgramError::InvalidAccountData.into());
    }

    let header = state::read_header(data);
    require_admin(header.insurance_operator, &operator_addr)?;

    let mut config = state::read_config(data);

    if oracle::permissionless_stale_matured(&config, clock.slot) {
        return Err(PercolatorError::OracleStale.into());
    }

    // Feature-disabled gate.
    if config.insurance_withdraw_max_bps == 0 {
        return Err(PercolatorError::InvalidConfigParam.into());
    }
    let deposits_only = insurance_withdraw_deposits_only(&config)?;
    {
        let engine = zc::engine_ref(data)?;
        reject_any_target_lag(&config, engine)?;
        let oi_any = engine.oi_eff_long_q != 0 || engine.oi_eff_short_q != 0;
        if oi_any && engine.last_market_slot != clock.slot {
            return Err(PercolatorError::CatchupRequired.into());
        }
    }

    // Cooldown.
    let last = config.last_insurance_withdraw_slot;
    if last != 0
        && clock.slot.saturating_sub(last) < config.insurance_withdraw_cooldown_slots
    {
        return Err(PercolatorError::InsuranceWithdrawCooldown.into());
    }

    let (amount_units, dust) = crate::units::base_to_units(amount, config.unit_scale);
    if dust != 0 || amount_units == 0 {
        return Err(ProgramError::InvalidArgument.into());
    }

    // Per-call cap: bps × insurance / 10_000, floor lifted to
    // MIN_WITHDRAW_FLOOR_UNITS, clamped to insurance.
    let ins = zc::engine_ref(data)?.insurance_fund.balance.get();
    if ins == 0 {
        return Err(PercolatorError::EngineInsufficientBalance.into());
    }
    let bps_cap = ins.saturating_mul(config.insurance_withdraw_max_bps as u128) / 10_000;
    let cap = core::cmp::max(bps_cap, MIN_WITHDRAW_FLOOR_UNITS);
    let mut cap = core::cmp::min(cap, ins);
    if deposits_only {
        cap = core::cmp::min(cap, config.insurance_withdraw_deposit_remaining as u128);
    }
    if (amount_units as u128) > cap {
        return Err(PercolatorError::InsuranceWithdrawCapExceeded.into());
    }

    // Vault + ATA verifiers.
    let mint = Address::from(config.collateral_mint);
    let vault_pubkey = Address::from(config.vault_pubkey);
    let auth = cpi::derive_vault_authority_with_bump(
        &crate::ID,
        &slab_addr,
        config.vault_authority_bump,
    )?;
    cpi::verify_vault(&vault_view, &auth, &mint, &vault_pubkey)?;
    cpi::verify_token_account(&operator_ata_view, &operator_addr, &mint)?;
    if vault_pda_view.address() != &auth {
        return Err(ProgramError::InvalidArgument.into());
    }

    // Commit state changes BEFORE the CPI (SPL Token can't re-enter
    // this program; CPI failure reverts atomically).
    {
        let engine = zc::engine_mut(data)?;
        engine
            .withdraw_live_insurance_not_atomic(amount_units as u128, clock.slot)
            .map_err(map_risk_error)?;
    }
    config.last_insurance_withdraw_slot = clock.slot;
    if deposits_only {
        config.insurance_withdraw_deposit_remaining = config
            .insurance_withdraw_deposit_remaining
            .checked_sub(amount_units)
            .ok_or(PercolatorError::InsuranceWithdrawCapExceeded)?;
    } else {
        config.insurance_withdraw_deposit_remaining = config
            .insurance_withdraw_deposit_remaining
            .saturating_sub(amount_units);
    }
    state::write_config(data, &config);

    // PDA-signed transfer.
    let slab_bytes = slab_addr.to_bytes();
    let bump_arr: [u8; 1] = [config.vault_authority_bump];
    let seeds: [&[u8]; 3] = [b"vault", &slab_bytes, &bump_arr];

    cpi::withdraw(
        &token_program_view,
        &vault_view,
        &operator_ata_view,
        &vault_pda_view,
        amount,
        &seeds,
    )?;
    Ok(())
}
