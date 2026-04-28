//! Tag 28 — ConvertReleasedPnl. Voluntary in-engine PnL conversion
//! (spec §10.4.1). Owner only. NO SPL CPI — purely an engine state
//! mutation.
//!
//! Wire format:
//!   `[28u8] [user_idx: u16] [amount: u64]`   (11 bytes)
//!
//! Accounts (strict order, matches legacy):
//!   1. user (Signer)
//!   2. slab (Account<SlabHeader>, mut)
//!   3. clock (Sysvar<Clock>)
//!   4. oracle (UncheckedAccount)

use crate::errors::{map_risk_error, PercolatorError};
use crate::guards::{require_initialized, require_no_reentrancy, slab_shape_guard};
use crate::oracle;
use crate::processor::{
    check_idx, compute_current_funding_rate_e9, ensure_market_accrued_to_now_with_policy,
    price_move_residual_dt, read_price_and_stamp, reject_any_target_lag, sync_account_fee,
};
use crate::state::{self, SlabHeader};
use crate::zc;
use anchor_lang_v2::prelude::*;
use solana_program_error::ProgramError;

#[derive(Accounts)]
pub struct ConvertReleasedPnl {
    pub user: Signer,
    #[account(mut)]
    pub slab: Account<SlabHeader>,
    pub clock: Sysvar<Clock>,
    /// CHECK: foreign-program oracle (Pyth/Chainlink) — decoded by `oracle::*`.
    pub oracle: UncheckedAccount,
}

pub fn handler(
    ctx: &mut Context<ConvertReleasedPnl>,
    user_idx: u16,
    amount: u64,
) -> Result<()> {
    slab_shape_guard(&ctx.accounts.slab)?;
    let user_addr = *ctx.accounts.user.address();
    let oracle_view = *ctx.accounts.oracle.account();
    let clock = *ctx.accounts.clock;

    let data: &mut [u8] = state::slab_data_mut(&mut ctx.accounts.slab);
    require_no_reentrancy(data)?;
    require_initialized(data)?;
    if zc::engine_ref(data)?.is_resolved() {
        return Err(ProgramError::InvalidAccountData.into());
    }

    let mut config = state::read_config(data);
    let is_hyperp = oracle::is_hyperp_mode(&config);
    let funding_rate_e9 = compute_current_funding_rate_e9(&config)?;
    let price = if is_hyperp {
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

    let engine = zc::engine_mut(data)?;
    check_idx(engine, user_idx)?;
    let owner = engine.accounts[user_idx as usize].owner;
    if !crate::policy::owner_ok(owner, user_addr.to_bytes()) {
        return Err(PercolatorError::EngineUnauthorized.into());
    }

    let (units, dust) = crate::units::base_to_units(amount, config.unit_scale);
    if units == 0 || dust != 0 {
        return Err(ProgramError::InvalidArgument.into());
    }

    let admit_h_min = engine.params.h_min;
    let admit_h_max = engine.params.h_max;
    ensure_market_accrued_to_now_with_policy(engine, &config, clock.slot, price, funding_rate_e9)?;
    reject_any_target_lag(&config, engine)?;
    sync_account_fee(engine, &config, user_idx, clock.slot)?;
    engine
        .convert_released_pnl_not_atomic(
            user_idx,
            units as u128,
            price,
            clock.slot,
            funding_rate_e9,
            admit_h_min,
            admit_h_max,
            Some(engine.params.maintenance_margin_bps as u128),
        )
        .map_err(map_risk_error)?;
    if !state::is_oracle_initialized(data) {
        state::set_oracle_initialized(data);
    }
    Ok(())
}
