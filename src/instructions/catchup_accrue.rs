//! Tag 31 — CatchupAccrue. Permissionless market-clock catchup.
//!
//! Wire format:
//!   `[31u8]`   (no args; the instruction is payload-less.)
//!
//! Accounts (strict order, matches legacy):
//!   1. slab (Account<PercolatorSlab>, mut) — market state
//!   2. clock (Sysvar<Clock>)            — runtime clock
//!   3. oracle (UncheckedAccount)        — Pyth/Chainlink (or any
//!      account for Hyperp; the oracle is read for liveness proof)
//!
//! Behavior summary (matches legacy `Instruction::CatchupAccrue` arm).
//! Two modes:
//!   - **COMPLETE** — single call closes the gap to `clock.slot`. The
//!     fresh oracle observation is persisted.
//!   - **PARTIAL** — gap exceeds `CATCHUP_CHUNKS_MAX × max_dt`. The
//!     oracle is read for liveness but the time-travel-sensitive
//!     fields (`last_effective_price_e6`, `last_hyperp_index_slot`)
//!     are rolled back; the liveness stamp + source-feed timestamp
//!     are preserved.

use crate::errors::map_risk_error;
use crate::guards::{require_initialized, require_no_reentrancy, slab_shape_guard};
use crate::oracle;
use crate::processor::{
    catchup_accrue, compute_current_funding_rate_e9, price_move_residual_dt, read_price_and_stamp,
    reject_stuck_target_accrual, CATCHUP_CHUNKS_MAX,
};
use crate::state::{self, PercolatorSlab};
use crate::zc;
use anchor_lang_v2::prelude::*;
use solana_program_error::ProgramError;

#[derive(Accounts)]
pub struct CatchupAccrue {
    #[account(mut)]
    pub slab: Account<PercolatorSlab>,
    pub clock: Sysvar<Clock>,
    /// CHECK: caller-supplied oracle (Pyth `PriceUpdateV2` or
    /// Chainlink OCR2 store, or any address for Hyperp). The handler
    /// decodes the account data via `oracle::*` decoders which validate
    /// owner / disc / feed-id / staleness internally.
    pub oracle: UncheckedAccount,
}

pub fn handler(ctx: &mut Context<CatchupAccrue>) -> Result<()> {
    slab_shape_guard(&ctx.accounts.slab)?;
    let clock = *ctx.accounts.clock; // Sysvar<Clock> derefs to Clock

    let oracle_view = *ctx.accounts.oracle.account();
    let data: &mut [u8] = state::slab_data_mut(&mut ctx.accounts.slab);
    require_no_reentrancy(data)?;
    require_initialized(data)?;

    if zc::engine_ref(data)?.is_resolved() {
        return Err(ProgramError::InvalidAccountData.into());
    }

    // Snapshot pre-read config so PARTIAL can roll back time-travel-
    // sensitive fields without losing the liveness stamp.
    let config_pre = state::read_config(data);
    let mut config = config_pre;

    // Anti-retroactivity (§5.5): pre-read funding rate.
    let funding_rate_e9_pre = compute_current_funding_rate_e9(&config)?;

    // Oracle read — proves the market is live. Mutates `config` in
    // memory (raw/stamp for non-Hyperp, clamp-toward for Hyperp).
    // Failure routes the caller to ResolvePermissionless.
    let is_hyperp = oracle::is_hyperp_mode(&config);
    let fresh_price = if is_hyperp {
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

    let engine = zc::engine_mut(data)?;

    // Engine never seeded — nothing to catch up past. Persist fresh
    // observation since there's no historical interval to leak into.
    if engine.last_oracle_price == 0 {
        state::write_config(data, &config);
        if !state::is_oracle_initialized(data) {
            state::set_oracle_initialized(data);
        }
        return Ok(());
    }
    reject_stuck_target_accrual(&config, engine, clock.slot, fresh_price)?;

    // COMPLETE vs PARTIAL: one call can close the gap iff neither
    // funding nor price-movement would drain equity over a >max_dt
    // window. Match the legacy clause-6 predicate.
    let max_dt = engine.params.max_accrual_dt_slots;
    let max_step_per_call = (CATCHUP_CHUNKS_MAX as u64).saturating_mul(max_dt);
    let gap = clock.slot.saturating_sub(engine.last_market_slot);
    let oi_any = engine.oi_eff_long_q != 0 || engine.oi_eff_short_q != 0;
    let funding_active = funding_rate_e9_pre != 0
        && engine.oi_eff_long_q != 0
        && engine.oi_eff_short_q != 0
        && engine.fund_px_last > 0;
    let price_move_active = engine.last_oracle_price > 0
        && fresh_price != engine.last_oracle_price
        && oi_any;
    let accrual_active = funding_active || price_move_active;
    let can_finish = !accrual_active || gap <= max_step_per_call;

    if can_finish {
        // COMPLETE: chunk to clock.slot using stored P_last (the
        // catchup_accrue invariant pins fund_px_last across chunks).
        // The final residual accrue installs `fresh_price`.
        catchup_accrue(engine, clock.slot, fresh_price, funding_rate_e9_pre)?;
        let flat_same_slot_price_update = fresh_price > 0
            && clock.slot == engine.last_market_slot
            && fresh_price != engine.last_oracle_price
            && engine.oi_eff_long_q == 0
            && engine.oi_eff_short_q == 0;
        if clock.slot > engine.last_market_slot || flat_same_slot_price_update {
            engine
                .accrue_market_to(clock.slot, fresh_price, funding_rate_e9_pre)
                .map_err(map_risk_error)?;
        }
        state::write_config(data, &config);
    } else {
        // PARTIAL: chunk through `target` using stored P_last (NOT
        // the fresh price). Roll back time-travel-sensitive fields
        // so subsequent CatchupAccrue calls observe freshly; preserve
        // the liveness stamp + source-feed timestamp so the same
        // observation can't be replayed for liveness.
        let stored_p_last = engine.last_oracle_price;
        let target = engine.last_market_slot.saturating_add(max_step_per_call);
        catchup_accrue(engine, target, stored_p_last, funding_rate_e9_pre)?;
        if target > engine.last_market_slot {
            engine
                .accrue_market_to(target, stored_p_last, funding_rate_e9_pre)
                .map_err(map_risk_error)?;
        }
        // Selective rollback: revert price/index baselines that
        // would retroactively apply post-observation index to
        // pre-observation engine slots; preserve the liveness fields.
        let mut restored = config_pre;
        restored.last_good_oracle_slot = config.last_good_oracle_slot;
        restored.last_oracle_publish_time = config.last_oracle_publish_time;
        restored.oracle_target_price_e6 = config.oracle_target_price_e6;
        restored.oracle_target_publish_time = config.oracle_target_publish_time;
        state::write_config(data, &restored);
    }

    if !state::is_oracle_initialized(data) {
        state::set_oracle_initialized(data);
    }
    Ok(())
}

