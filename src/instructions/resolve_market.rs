//! Tag 19 — ResolveMarket. Admin resolves a live market via §9.8.
//! Caller picks Ordinary (mode = 0) or Degenerate (mode = 1) — the
//! wrapper no longer silently promotes a stale Ordinary call.
//!
//! Wire format:
//!   `[19u8] [mode: u8]`  (2 bytes; `mode` must be 0 or 1)
//!
//! Accounts (strict order, matches legacy):
//!   1. admin (Signer)                       — must equal header.admin
//!   2. slab (Account<PercolatorSlab>, mut)
//!   3. clock (Sysvar<Clock>)
//!   4. oracle (UncheckedAccount)            — Pyth/Chainlink (or any
//!      address for Hyperp; non-Hyperp Ordinary requires it live)
//!
//! Modes:
//!   - 0 = Ordinary. Live oracle required (Hyperp index OR fresh
//!     external reading). Settles at the canonical mark/oracle price.
//!   - 1 = Degenerate. Oracle is engine-confirmed dead (matured stale
//!     gate, or admin-observed deadness). Settles at
//!     `engine.last_oracle_price` with funding rate = 0.

use crate::errors::{map_risk_error, PercolatorError};
use crate::guards::{require_admin, require_initialized, require_no_reentrancy, slab_shape_guard};
use crate::oracle;
use crate::processor::{
    catchup_accrue, compute_current_funding_rate_e9, hyperp_target_price, price_move_residual_dt,
    read_price_and_stamp, reject_stuck_target_accrual,
};
use crate::state::{self, PercolatorSlab};
use crate::zc;
use anchor_lang_v2::prelude::*;
use solana_program_error::ProgramError;

#[derive(Accounts)]
pub struct ResolveMarket {
    pub admin: Signer,
    #[account(mut)]
    pub slab: Account<PercolatorSlab>,
    pub clock: Sysvar<Clock>,
    /// CHECK: external oracle (Pyth/Chainlink) for non-Hyperp Ordinary;
    /// foreign-program data, validated by `oracle::*` decoders.
    pub oracle: UncheckedAccount,
}

pub fn handler(ctx: &mut Context<ResolveMarket>, mode: u8) -> Result<()> {
    // Anchor v2's Borsh decoder accepts any u8, so this gate replaces
    // the legacy `read_u8` + `mode > 1` check.
    if mode > 1 {
        return Err(ProgramError::InvalidInstructionData.into());
    }

    slab_shape_guard(&ctx.accounts.slab)?;
    let clock = *ctx.accounts.clock;
    let admin_addr = *ctx.accounts.admin.address();
    let oracle_view = *ctx.accounts.oracle.account();

    let data: &mut [u8] = state::slab_data_mut(&mut ctx.accounts.slab);
    require_no_reentrancy(data)?;
    require_initialized(data)?;

    let header = state::read_header(data);
    require_admin(header.admin, &admin_addr)?;

    // Can't re-resolve.
    if zc::engine_ref(data)?.is_resolved() {
        return Err(ProgramError::InvalidAccountData.into());
    }

    let mut config = state::read_config(data);
    let max_change_bps = zc::engine_ref(data)?.params.max_price_move_bps_per_slot;
    // Anti-retroactivity (§5.5): capture funding rate before any config mutation.
    let funding_rate_e9 = compute_current_funding_rate_e9(&config)?;

    // ── Degenerate branch ──────────────────────────────────────────
    // Settle at engine.last_oracle_price with rate = 0. Used for
    // dead-oracle markets (stale gate matured, or admin-observed
    // deadness). Mirrors ResolvePermissionless's terminal settlement.
    if mode == 1 {
        let stale_or_restarted = oracle::permissionless_stale_matured(&config, clock.slot);
        if !stale_or_restarted {
            if oracle::is_hyperp_mode(&config) {
                let oracle_initialized = state::is_oracle_initialized(data);
                let last_update = core::cmp::max(
                    config.mark_ewma_last_slot,
                    config.last_mark_push_slot as u64,
                );
                let max_stale_slots = config.max_staleness_secs.saturating_mul(3);
                let hyperp_stale = clock.slot.saturating_sub(last_update) > max_stale_slots
                    && oracle_initialized;
                if !hyperp_stale {
                    return Err(PercolatorError::OracleInvalid.into());
                }
            } else {
                let live = oracle::read_engine_price_e6(
                    &oracle_view,
                    &config.index_feed_id,
                    clock.unix_timestamp,
                    config.max_staleness_secs,
                    config.conf_filter_bps,
                    config.invert,
                    config.unit_scale,
                );
                match live {
                    Ok(_) => return Err(PercolatorError::OracleInvalid.into()),
                    Err(e)
                        if e == ProgramError::from(PercolatorError::OracleStale)
                            || e == ProgramError::from(PercolatorError::OracleConfTooWide) => {}
                    Err(e) => return Err(e.into()),
                }
            }
        }
        let engine = zc::engine_mut(data)?;
        let p_last = engine.last_oracle_price;
        if p_last == 0 {
            return Err(PercolatorError::OracleInvalid.into());
        }
        engine
            .resolve_market_not_atomic(
                percolator::ResolveMode::Degenerate,
                p_last,
                p_last,
                clock.slot,
                0,
            )
            .map_err(map_risk_error)?;
        config.hyperp_mark_e6 = p_last;
        state::write_config(data, &config);
        return Ok(());
    }

    // ── Ordinary branch ────────────────────────────────────────────
    // Require live oracle. If the stale gate has matured the caller
    // must switch to mode = 1; we do not quietly settle against a
    // dead oracle.
    if oracle::permissionless_stale_matured(&config, clock.slot) {
        return Err(PercolatorError::OracleStale.into());
    }

    // Hyperp markets need their mark initialized to settle.
    if oracle::is_hyperp_mode(&config) && config.hyperp_mark_e6 == 0 {
        return Err(ProgramError::InvalidAccountData.into());
    }

    // Read fresh external oracle for non-Hyperp Ordinary. Pyth/Chainlink
    // dead → bubble up the parse error; admin must select mode = 1 for
    // the Degenerate recovery arm.
    let mut fresh_live_oracle: Option<u64> = None;
    if !oracle::is_hyperp_mode(&config) {
        let fresh = read_price_and_stamp(
            &mut config,
            &oracle_view,
            clock.unix_timestamp,
            clock.slot,
            data,
        )?;
        fresh_live_oracle = Some(fresh);
    }

    // Flush Hyperp index to resolution slot WITHOUT staleness check.
    // Admin must be able to resolve even if mark is stale.
    if oracle::is_hyperp_mode(&config) {
        let engine = zc::engine_ref(data)?;
        let mark = hyperp_target_price(&config);
        if mark > 0 {
            let anchor = if engine.last_oracle_price != 0 {
                engine.last_oracle_price
            } else if config.last_effective_price_e6 != 0 {
                config.last_effective_price_e6
            } else {
                mark
            };
            let dt = price_move_residual_dt(engine, clock.slot)?;
            let oi_any = engine.oi_eff_long_q != 0 || engine.oi_eff_short_q != 0;
            let new_index = if oi_any {
                oracle::clamp_toward_engine_dt(anchor, mark, max_change_bps, dt)
            } else {
                mark
            };
            config.last_effective_price_e6 = new_index;
            if new_index != anchor || new_index == mark {
                config.last_hyperp_index_slot = clock.slot;
            }
        }
        state::write_config(data, &config);
    }

    // Canonical settlement price.
    //   Hyperp: mark EWMA (or hyperp_mark_e6 if EWMA uninitialized).
    //   Non-Hyperp: fresh external reading.
    let settlement_price = if oracle::is_hyperp_mode(&config) {
        let mark = config.mark_ewma_e6;
        if mark > 0 {
            mark
        } else {
            config.hyperp_mark_e6
        }
    } else {
        match fresh_live_oracle {
            Some(fresh) => fresh,
            None => {
                let engine_r = zc::engine_ref(data)?;
                engine_r.last_oracle_price
            }
        }
    };

    // Live-oracle + final-rate selection.
    let oracle_initialized = state::is_oracle_initialized(data);
    let is_hyperp_local = oracle::is_hyperp_mode(&config);
    let hyperp_stale = if is_hyperp_local {
        let last_update = core::cmp::max(
            config.mark_ewma_last_slot,
            config.last_mark_push_slot as u64,
        );
        let max_stale_slots = config.max_staleness_secs.saturating_mul(3);
        clock.slot.saturating_sub(last_update) > max_stale_slots && oracle_initialized
    } else {
        false
    };

    let engine = zc::engine_mut(data)?;

    let (live_oracle, rate_for_final_accrual): (u64, i128) = if let Some(fresh) = fresh_live_oracle
    {
        if config.oracle_target_price_e6 != 0 && fresh != config.oracle_target_price_e6 {
            return Err(PercolatorError::CatchupRequired.into());
        }
        (fresh, funding_rate_e9)
    } else if is_hyperp_local && !hyperp_stale {
        (config.last_effective_price_e6, funding_rate_e9)
    } else {
        return Err(PercolatorError::OracleStale.into());
    };
    let _ = oracle_initialized;

    let resolve_mode = percolator::ResolveMode::Ordinary;

    // Pre-chunk catch-up so the final accrue sees dt ≤ max_dt
    // (Finding 4). Same (price, rate) the final accrue uses, so
    // anti-retroactivity is preserved (Finding 2).
    reject_stuck_target_accrual(&config, engine, clock.slot, live_oracle)?;
    catchup_accrue(engine, clock.slot, live_oracle, rate_for_final_accrual)?;
    engine
        .resolve_market_not_atomic(
            resolve_mode,
            settlement_price,
            live_oracle,
            clock.slot,
            rate_for_final_accrual,
        )
        .map_err(map_risk_error)?;

    state::write_config(data, &config);
    Ok(())
}
