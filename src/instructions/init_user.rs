//! Tag 1 — InitUser. Owner-signed account materialization.
//! Spec §10.2: deposit is the canonical materialization path — pure
//! capital transfer, MUST NOT accrue and MUST NOT mutate side state.
//! `fee_payment` is split: `new_account_fee` → insurance,
//! remainder → capital.
//!
//! Wire format:
//!   `[1u8] [fee_payment: u64]`   (9 bytes)
//!
//! Accounts (strict order, matches legacy):
//!   1. user (Signer)
//!   2. slab (Account<PercolatorSlab>, mut)
//!   3. user_ata (UncheckedAccount, mut)
//!   4. vault (UncheckedAccount, mut)
//!   5. token_program (UncheckedAccount)
//!   6. clock (Sysvar<Clock>)

use crate::cpi;
use crate::errors::{map_risk_error, PercolatorError};
use crate::guards::{require_initialized, require_no_reentrancy, slab_shape_guard};
use crate::oracle;
use crate::processor::{check_no_oracle_live_envelope, prepare_lazy_free_head};
use crate::state::{self, PercolatorSlab};
use crate::zc;
use anchor_lang_v2::prelude::*;
use solana_program_error::ProgramError;

#[derive(Accounts)]
pub struct InitUser {
    pub user: Signer,
    #[account(mut)]
    pub slab: Account<PercolatorSlab>,
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

pub fn handler(ctx: &mut Context<InitUser>, fee_payment: u64) -> Result<()> {
    cpi::verify_token_program(ctx.accounts.token_program.account())?;

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
    let (_units_check, dust_check) = crate::units::base_to_units(fee_payment, config.unit_scale);
    if dust_check != 0 {
        return Err(ProgramError::InvalidArgument.into());
    }

    // Split: `new_account_fee` → insurance (wrapper-charged); remainder → capital.
    let fee_base: u64 = config
        .new_account_fee
        .try_into()
        .map_err(|_| PercolatorError::EngineOverflow)?;
    if fee_payment <= fee_base {
        return Err(PercolatorError::EngineInsufficientBalance.into());
    }
    let capital_base = fee_payment - fee_base;

    // TVL:insurance cap (admin opt-in). InitUser must respect the same
    // cap as DepositCollateral — `fee_payment` is otherwise an unbounded
    // bypass. Simulate post-fee: fee → insurance ceiling, capital → c_tot.
    if config.tvl_insurance_cap_mult > 0 {
        let (capital_units_sim, _) =
            crate::units::base_to_units(capital_base, config.unit_scale);
        let (fee_units_sim, _) = crate::units::base_to_units(fee_base, config.unit_scale);
        let engine_r = zc::engine_ref(data)?;
        let ins_new = engine_r
            .insurance_fund
            .balance
            .get()
            .saturating_add(fee_units_sim as u128);
        let c_tot_new = engine_r
            .c_tot
            .get()
            .saturating_add(capital_units_sim as u128);
        let cap = ins_new.saturating_mul(config.tvl_insurance_cap_mult as u128);
        if c_tot_new > cap {
            return Err(PercolatorError::DepositCapExceeded.into());
        }
    }

    // Transfer the full fee_payment to vault; split downstream in the engine.
    cpi::deposit(
        &token_program_view,
        &user_ata_view,
        &vault_view,
        &user_view,
        fee_payment,
    )?;

    let (capital_units, _) = crate::units::base_to_units(capital_base, config.unit_scale);
    let (fee_units, _) = crate::units::base_to_units(fee_base, config.unit_scale);
    if capital_units == 0 {
        return Err(PercolatorError::EngineInsufficientBalance.into());
    }

    let engine = zc::engine_mut(data)?;
    let idx = prepare_lazy_free_head(engine)?;
    engine
        .deposit_not_atomic(idx, capital_units as u128, clock.slot)
        .map_err(map_risk_error)?;
    engine
        .set_owner(idx, user_addr.to_bytes())
        .map_err(map_risk_error)?;
    if fee_units > 0 {
        engine
            .top_up_insurance_fund(fee_units as u128, clock.slot)
            .map_err(map_risk_error)?;
    }
    let generation =
        state::next_mat_counter(data).ok_or(PercolatorError::EngineOverflow)?;
    state::write_account_generation(data, idx, generation);
    Ok(())
}
