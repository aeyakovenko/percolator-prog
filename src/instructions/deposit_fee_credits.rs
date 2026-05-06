//! Tag 27 — DepositFeeCredits. Direct fee-debt repayment (§10.3.1).
//! Owner only. Reads outstanding debt BEFORE the SPL transfer to
//! reject overpayment — without this, excess tokens become stranded
//! vault surplus with no withdrawal path for the user.
//!
//! Wire format:
//!   `[27u8] [user_idx: u16] [amount: u64]`   (11 bytes)
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
use crate::processor::{check_idx, check_no_oracle_live_envelope, sync_account_fee_bounded_to_market};
use crate::state::{self, PercolatorSlab};
use crate::zc;
use anchor_lang_v2::prelude::*;
use solana_program_error::ProgramError;

#[derive(Accounts)]
pub struct DepositFeeCredits {
    pub user: Signer,
    #[account(mut)]
    pub slab: Account<PercolatorSlab>,
    /// CHECK: validated as user's SPL token ATA in the handler.
    #[account(mut)]
    pub user_ata: crate::spl::TokenAccount,
    /// CHECK: validated against `MarketConfig.vault_pubkey` + PDA auth.
    #[account(mut)]
    pub vault: crate::spl::TokenAccount,
    pub token_program: Program<crate::spl::TokenProgram>,
    pub clock: Sysvar<Clock>,
}

pub fn handler(
    ctx: &mut Context<DepositFeeCredits>,
    user_idx: u16,
    amount: u64,
) -> Result<()> {


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

    let cfg = state::read_config(data);
    let mint = Address::from(cfg.collateral_mint);
    let auth = cpi::derive_vault_authority_with_bump(
        &crate::ID,
        &slab_addr,
        cfg.vault_authority_bump,
    )?;
    cpi::verify_vault(&vault_view, &auth, &mint, &Address::from(cfg.vault_pubkey))?;
    cpi::verify_token_account(&user_ata_view, &user_addr, &mint)?;

    if oracle::permissionless_stale_matured(&cfg, clock.slot) {
        return Err(PercolatorError::OracleStale.into());
    }

    // Phase 1: sync latent maintenance fees, read post-sync debt.
    // Done BEFORE SPL transfer so a user whose realized debt is zero
    // but who has nonzero latent fees doesn't get rejected as
    // overpayment when they try to legitimately repay.
    let debt_units: u128 = {
        let engine = zc::engine_mut(data)?;
        check_idx(engine, user_idx)?;
        let owner = engine.accounts[user_idx as usize].owner;
        if !crate::policy::owner_ok(owner, user_addr.to_bytes()) {
            return Err(PercolatorError::EngineUnauthorized.into());
        }
        check_no_oracle_live_envelope(engine, clock.slot)?;
        sync_account_fee_bounded_to_market(engine, &cfg, user_idx, clock.slot)?;
        let fc = engine.accounts[user_idx as usize].fee_credits.get();
        if fc > 0 || fc == i128::MIN {
            return Err(PercolatorError::EngineCorruptState.into());
        }
        if fc < 0 {
            fc.unsigned_abs()
        } else {
            0u128
        }
    }; // engine borrow drops here.

    // Phase 2: reject zero, misaligned, or overpayment.
    let (units, dust) = crate::units::base_to_units(amount, cfg.unit_scale);
    if units == 0 || dust != 0 {
        return Err(ProgramError::InvalidArgument.into());
    }
    if (units as u128) > debt_units {
        return Err(ProgramError::InvalidArgument.into());
    }

    // Phase 3: SPL transfer (only after validation).
    cpi::deposit(&token_program_view, &user_ata_view, &vault_view, &user_view, amount)?;

    // Phase 4: book the repayment in the engine. Phase 1 already
    // synced; no second sync needed here.
    let engine = zc::engine_mut(data)?;
    engine
        .deposit_fee_credits(user_idx, units as u128, clock.slot)
        .map_err(map_risk_error)?;
    Ok(())
}
