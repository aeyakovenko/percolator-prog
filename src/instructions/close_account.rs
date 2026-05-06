//! Tag 8 — CloseAccount. Owner closes their account, settling PnL +
//! recovering capital. Works on both live and resolved markets:
//!   - Live: `close_account_not_atomic` (full accrue → sync → close).
//!   - Resolved: `force_close_resolved_not_atomic` (which may return
//!     `ProgressOnly` requiring retry after counterparty reconciles).
//!
//! Wire format:
//!   `[8u8] [user_idx: u16]`   (3 bytes)
//!
//! Accounts (strict order, matches legacy):
//!   1. user (Signer)
//!   2. slab (Account<PercolatorSlab>, mut)
//!   3. vault (UncheckedAccount, mut)
//!   4. user_ata (UncheckedAccount, mut)
//!   5. vault_pda (UncheckedAccount)
//!   6. token_program (UncheckedAccount)
//!   7. clock (Sysvar<Clock>)
//!   8. oracle (UncheckedAccount)

use crate::cpi;
use crate::errors::{map_risk_error, PercolatorError};
use crate::guards::{require_initialized, require_no_reentrancy, slab_shape_guard};
use crate::oracle;
use crate::processor::{
    check_idx, compute_current_funding_rate_e9, ensure_market_accrued_to_now_with_policy,
    price_move_residual_dt, read_price_and_stamp, reject_any_target_lag, sync_account_fee,
};
use crate::state::{self, PercolatorSlab};
use crate::zc;
use anchor_lang_v2::prelude::*;
use solana_program_error::ProgramError;

#[derive(Accounts)]
pub struct CloseAccount {
    pub user: Signer,
    #[account(mut)]
    pub slab: Account<PercolatorSlab>,
    /// CHECK: validated against `MarketConfig.vault_pubkey` + PDA auth.
    #[account(mut)]
    pub vault: crate::spl::TokenAccount,
    /// CHECK: validated as user's SPL token ATA inside the handler
    /// (only when `base_to_pay > 0`).
    #[account(mut)]
    pub user_ata: crate::spl::TokenAccount,
    /// CHECK: framework-validated via `seeds` + `bump` constraint.
    #[account(seeds = [b"vault", slab], bump = slab.config.vault_authority_bump())]
    pub vault_pda: UncheckedAccount,
    pub token_program: Program<crate::spl::TokenProgram>,
    pub clock: Sysvar<Clock>,
    /// CHECK: oracle (only consulted on live markets; ignored on resolved).
    pub oracle: UncheckedAccount,
}

pub fn handler(ctx: &mut Context<CloseAccount>, user_idx: u16) -> Result<()> {


    slab_shape_guard(&ctx.accounts.slab)?;
    let user_addr = *ctx.accounts.user.address();
    let slab_addr = *ctx.accounts.slab.account().address();
    let vault_view = *ctx.accounts.vault.account();
    let user_ata_view = *ctx.accounts.user_ata.account();
    let pda_view = *ctx.accounts.vault_pda.account();
    let token_program_view = *ctx.accounts.token_program.account();
    let oracle_view = *ctx.accounts.oracle.account();
    let clock = *ctx.accounts.clock;

    let data: &mut [u8] = state::slab_data_mut(&mut ctx.accounts.slab);
    require_no_reentrancy(data)?;
    require_initialized(data)?;
    let mut config = state::read_config(data);
    let mint = Address::from(config.collateral_mint);

    let auth = *pda_view.address();
    cpi::verify_vault(&vault_view, &auth, &mint, &Address::from(config.vault_pubkey))?;

    let resolved = zc::engine_ref(data)?.is_resolved();
    let mut funding_rate_e9 = 0i128;
    let price = if resolved {
        let eng = zc::engine_ref(data)?;
        let (settlement, _) = eng.resolved_context();
        if settlement == 0 {
            return Err(ProgramError::InvalidAccountData.into());
        }
        settlement
    } else {
        funding_rate_e9 = compute_current_funding_rate_e9(&config)?;
        let is_hyperp = oracle::is_hyperp_mode(&config);
        let px = if is_hyperp {
            let eng = zc::engine_ref(data)?;
            let p_last = eng.last_oracle_price;
            let price_move_dt = price_move_residual_dt(eng, clock.slot)?;
            let cap_bps = eng.params.max_price_move_bps_per_slot;
            let oi_any = eng.oi_eff_long_q != 0 || eng.oi_eff_short_q != 0;
            oracle::get_engine_oracle_price_e6(
                p_last,
                price_move_dt,
                clock.slot,
                clock.unix_timestamp,
                &mut config,
                &oracle_view,
                cap_bps,
                oi_any,
            )?
        } else {
            read_price_and_stamp(
                &mut config,
                &oracle_view,
                clock.unix_timestamp,
                clock.slot,
                data,
            )?
        };
        state::write_config(data, &config);
        px
    };

    let engine = zc::engine_mut(data)?;
    check_idx(engine, user_idx)?;

    let u_owner = engine.accounts[user_idx as usize].owner;
    if !crate::policy::owner_ok(u_owner, user_addr.to_bytes()) {
        return Err(PercolatorError::EngineUnauthorized.into());
    }

    let amt_units = if resolved {
        let (_settle_px, resolved_slot_anchor) = engine.resolved_context();
        // Realize maintenance fees up to the resolved anchor BEFORE the
        // force-close. Engine doesn't sync the fee cursor itself, so a
        // missed sync would let an account close without paying fees
        // accrued over [last_fee_slot, resolved_slot]. No-op when
        // `maintenance_fee_per_slot == 0`.
        sync_account_fee(engine, &config, user_idx, resolved_slot_anchor)?;
        match engine
            .force_close_resolved_not_atomic(user_idx)
            .map_err(map_risk_error)?
        {
            // Phase-1 reconciliation only — caller must retry after
            // counterparty closes.
            percolator::ResolvedCloseResult::ProgressOnly => return Ok(()),
            percolator::ResolvedCloseResult::Closed(payout) => payout,
        }
    } else {
        let admit_h_min = engine.params.h_min;
        let admit_h_max = engine.params.h_max;
        ensure_market_accrued_to_now_with_policy(
            engine,
            &config,
            clock.slot,
            price,
            funding_rate_e9,
        )?;
        reject_any_target_lag(&config, engine)?;
        sync_account_fee(engine, &config, user_idx, clock.slot)?;
        engine
            .close_account_not_atomic(
                user_idx,
                clock.slot,
                price,
                funding_rate_e9,
                admit_h_min,
                admit_h_max,
                Some(engine.params.maintenance_margin_bps as u128),
            )
            .map_err(map_risk_error)?
    };

    let amt_units_u64: u64 = amt_units
        .try_into()
        .map_err(|_| PercolatorError::EngineOverflow)?;

    if !resolved && !state::is_oracle_initialized(data) {
        state::set_oracle_initialized(data);
    }
    {
        let mut buf = state::read_risk_buffer(data);
        buf.remove(user_idx);
        state::write_risk_buffer(data, &buf);
    }

    let base_to_pay = crate::units::units_to_base_checked(amt_units_u64, config.unit_scale)
        .ok_or(PercolatorError::EngineOverflow)?;

    if base_to_pay > 0 {
        cpi::verify_token_account(&user_ata_view, &user_addr, &mint)?;
        let slab_bytes = slab_addr.to_bytes();
        let bump_arr: [u8; 1] = [config.vault_authority_bump];
        let seeds: [&[u8]; 3] = [b"vault", &slab_bytes, &bump_arr];
        cpi::withdraw(
            &token_program_view,
            &vault_view,
            &user_ata_view,
            &pda_view,
            base_to_pay,
            &seeds,
        )?;
    }
    Ok(())
}
