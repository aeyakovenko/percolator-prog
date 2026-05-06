//! Tag 7 — LiquidateAtOracle. Permissionless full-close liquidation
//! at the live oracle/index price.
//!
//! Wire format:
//!   `[7u8] [target_idx: u16]`   (3 bytes)
//!
//! Accounts (strict order, matches legacy):
//!   1. slab (Account<PercolatorSlab>, mut)
//!   2. clock (Sysvar<Clock>)
//!   3. oracle (UncheckedAccount)        — Pyth/Chainlink (or any
//!      address for Hyperp; the handler decodes via `oracle::*`)
//!
//! Behavior summary (matches legacy `Instruction::LiquidateAtOracle`
//! arm): rejects resolved markets, fetches the live price (Hyperp via
//! `oracle::get_engine_oracle_price_e6`, non-Hyperp via
//! `read_price_and_stamp`), persists updated config, accrues to
//! current slot, syncs the target's maintenance fee, then calls the
//! engine's `liquidate_at_oracle_not_atomic` with `FullClose`. Risk
//! buffer is updated post-liquidation.

use crate::errors::map_risk_error;
use crate::guards::{require_initialized, require_no_reentrancy, slab_shape_guard};
use crate::oracle;
use crate::processor::{
    check_idx, compute_current_funding_rate_e9, effective_pos_q_checked,
    ensure_market_accrued_to_now_with_policy, price_move_residual_dt, read_price_and_stamp,
    risk_notional_ceil, sync_account_fee,
};
use crate::state::{self, PercolatorSlab};
use crate::zc;
use anchor_lang_v2::prelude::*;
use solana_program_error::ProgramError;

#[derive(Accounts)]
pub struct LiquidateAtOracle {
    #[account(mut)]
    pub slab: Account<PercolatorSlab>,
    pub clock: Sysvar<Clock>,
    /// CHECK: foreign-program oracle account, validated by `oracle::*`.
    pub oracle: UncheckedAccount,
}

pub fn handler(ctx: &mut Context<LiquidateAtOracle>, target_idx: u16) -> Result<()> {
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
    check_idx(engine, target_idx)?;

    let admit_h_min = engine.params.h_min;
    let admit_h_max = engine.params.h_max;

    // accrue → sync → liquidate.
    ensure_market_accrued_to_now_with_policy(engine, &config, clock.slot, price, funding_rate_e9)?;
    sync_account_fee(engine, &config, target_idx, clock.slot)?;
    let admit_threshold = Some(engine.params.maintenance_margin_bps as u128);
    engine
        .liquidate_at_oracle_not_atomic(
            target_idx,
            clock.slot,
            price,
            percolator::LiquidationPolicy::FullClose,
            funding_rate_e9,
            admit_h_min,
            admit_h_max,
            admit_threshold,
        )
        .map_err(map_risk_error)?;

    // Risk-buffer maintenance.
    let liq_eff = effective_pos_q_checked(engine, target_idx as usize)?;
    if !state::is_oracle_initialized(data) {
        state::set_oracle_initialized(data);
    }
    {
        let mut buf = state::read_risk_buffer(data);
        if liq_eff == 0 {
            buf.remove(target_idx);
        } else {
            let notional = risk_notional_ceil(liq_eff, price);
            buf.upsert(target_idx, notional);
        }
        state::write_risk_buffer(data, &buf);
    }
    Ok(())
}
