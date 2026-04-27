//! Tag 26 — SettleAccount. Permissionless single-account settlement
//! (spec §10.2).
//!
//! Wire format:
//!   `[26u8] [user_idx: u16]`  (3 bytes)
//!
//! Accounts (strict order, matches legacy):
//!   1. slab (Account<SlabHeader>, mut) — market state
//!   2. clock (Sysvar<Clock>)            — runtime clock
//!   3. oracle (UncheckedAccount)        — Pyth/Chainlink/etc
//!
//! Behavior summary (matches legacy `Instruction::SettleAccount` arm):
//! reject resolved markets, fetch fresh oracle (Hyperp via internal
//! mark, non-Hyperp via Pyth/Chainlink), persist updated config,
//! `ensure_market_accrued_to_now_with_policy` to bring the engine
//! current, reject any target lag, sync per-account fees, then call
//! the engine's `settle_account_not_atomic` with the maintenance
//! threshold.

use crate::errors::map_risk_error;
use crate::guards::{require_initialized, require_no_reentrancy, slab_shape_guard};
use crate::oracle;
use crate::processor::{
    compute_current_funding_rate_e9, ensure_market_accrued_to_now_with_policy,
    price_move_residual_dt, read_price_and_stamp, reject_any_target_lag, sync_account_fee,
};
use crate::state::{self, SlabHeader};
use crate::zc;
use anchor_lang_v2::prelude::*;
use solana_program_error::ProgramError;

#[derive(Accounts)]
pub struct SettleAccount {
    #[account(mut)]
    pub slab: Account<SlabHeader>,
    pub clock: Sysvar<Clock>,
    /// CHECK: Pyth/Chainlink oracle (or any address for Hyperp).
    /// Validated inside `oracle::*` decoders against owner / disc /
    /// feed-id / staleness.
    pub oracle: UncheckedAccount,
}

pub fn handler(ctx: &mut Context<SettleAccount>, user_idx: u16) -> Result<()> {
    slab_shape_guard(&ctx.accounts.slab)?;
    let clock = *ctx.accounts.clock;

    let oracle_view = *ctx.accounts.oracle.account();
    let data: &mut [u8] = state::slab_data_mut(&mut ctx.accounts.slab);
    require_no_reentrancy(data)?;
    require_initialized(data)?;

    if zc::engine_ref(data)?.is_resolved() {
        return Err(ProgramError::InvalidAccountData.into());
    }

    let mut config = state::read_config(data);
    let is_hyperp = oracle::is_hyperp_mode(&config);
    // Anti-retroactivity (§5.5): capture funding rate before oracle read.
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
    let admit_h_min = engine.params.h_min;
    let admit_h_max = engine.params.h_max;
    // accrue → sync → settle (explicit ordering per legacy doc).
    ensure_market_accrued_to_now_with_policy(engine, &config, clock.slot, price, funding_rate_e9)?;
    reject_any_target_lag(&config, engine)?;
    sync_account_fee(engine, &config, user_idx, clock.slot)?;
    let admit_threshold = Some(engine.params.maintenance_margin_bps as u128);
    engine
        .settle_account_not_atomic(
            user_idx,
            price,
            clock.slot,
            funding_rate_e9,
            admit_h_min,
            admit_h_max,
            admit_threshold,
        )
        .map_err(map_risk_error)?;
    if !state::is_oracle_initialized(data) {
        state::set_oracle_initialized(data);
    }
    Ok(())
}

