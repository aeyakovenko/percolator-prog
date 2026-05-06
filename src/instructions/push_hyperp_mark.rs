//! Tag 17 — PushHyperpMark. Hyperp-only mark-push instruction. The
//! configured `hyperp_authority` signs and supplies an updated mark
//! price (e6, raw — the handler converts to engine-space).
//!
//! Wire format:
//!   `[17u8] [price_e6: u64] [timestamp: i64]`
//!
//! `timestamp` is legacy filler — Hyperp markets ignore it (the
//! liveness clock is `last_mark_push_slot`, not the wire timestamp).
//!
//! Accounts (strict order, matches legacy):
//!   1. `authority` (signer)              — must equal `config.hyperp_authority`
//!   2. `slab` (mut, owned by program)    — market state
//!
//! Behavior summary (matches legacy `Instruction::PushHyperpMark` arm):
//!   - Reject non-Hyperp markets (no `hyperp_authority` role).
//!   - Reject if `hyperp_authority` is burned ([0; 32]) or doesn't match.
//!   - Hard-timeout gate via `oracle::permissionless_stale_matured`.
//!   - Flush Hyperp index toward `mark` via `clamp_toward_engine_dt`.
//!   - Catch-up + final `accrue_market_to`.
//!   - Clamp incoming mark against the freshly-flushed index.
//!   - Update `hyperp_mark_e6`, `last_mark_push_slot`,
//!     `mark_ewma_e6`, `mark_ewma_last_slot`.

use crate::errors::{map_risk_error, PercolatorError};
use crate::guards::{require_initialized, require_no_reentrancy, slab_shape_guard};
use crate::oracle;
use crate::processor::{
    catchup_accrue, compute_current_funding_rate_e9, hyperp_target_price, price_move_residual_dt,
    reject_stuck_target_accrual,
};
use crate::state::{self, PercolatorSlab};
use crate::zc;
use anchor_lang_v2::prelude::*;
use solana_program_error::ProgramError;

#[derive(Accounts)]
pub struct PushHyperpMark {
    /// CHECK: must equal `config.hyperp_authority`. Signer-ness is
    /// enforced by the `Signer` constraint; key equality is checked
    /// in the handler against the slab body.
    pub authority: Signer,
    /// `Account<PercolatorSlab>` validates v2 disc + program owner.
    #[account(mut)]
    pub slab: Account<PercolatorSlab>,
}

pub fn handler(
    ctx: &mut Context<PushHyperpMark>,
    price_e6: u64,
    timestamp: i64,
) -> Result<()> {
    let _ = timestamp; // legacy wire data; Hyperp ignores it

    slab_shape_guard(&ctx.accounts.slab)?;
    let data: &mut [u8] = state::slab_data_mut(&mut ctx.accounts.slab);

    require_no_reentrancy(data)?;
    require_initialized(data)?;

    if zc::engine_ref(data)?.is_resolved() {
        return Err(ProgramError::InvalidAccountData.into());
    }

    let mut config = state::read_config(data);

    // Hyperp-only: PushHyperpMark is the admin mark-push for
    // internally-priced markets. Non-Hyperp markets price exclusively
    // off Pyth/Chainlink with no authority path.
    if !oracle::is_hyperp_mode(&config) {
        return Err(PercolatorError::EngineUnauthorized.into());
    }
    if config.hyperp_authority == [0u8; 32]
        || config.hyperp_authority != ctx.accounts.authority.address().to_bytes()
    {
        return Err(PercolatorError::EngineUnauthorized.into());
    }

    // Anti-retroactivity: capture funding rate before any config mutation (§5.5).
    let funding_rate_e9 = compute_current_funding_rate_e9(&config)?;

    let push_clock = Clock::get().map_err(|_| ProgramError::UnsupportedSysvar)?;
    if oracle::permissionless_stale_matured(&config, push_clock.slot) {
        return Err(PercolatorError::OracleStale.into());
    }

    // Flush index WITHOUT external staleness check (the hard-timeout
    // gate above covers mark staleness).
    let max_change_bps = zc::engine_ref(data)?.params.max_price_move_bps_per_slot;
    {
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
            let dt = price_move_residual_dt(engine, push_clock.slot)?;
            let oi_any = engine.oi_eff_long_q != 0 || engine.oi_eff_short_q != 0;
            let new_index = if oi_any {
                oracle::clamp_toward_engine_dt(anchor, mark, max_change_bps, dt)
            } else {
                mark
            };
            config.last_effective_price_e6 = new_index;
            if new_index != anchor || new_index == mark {
                config.last_hyperp_index_slot = push_clock.slot;
            }
        }
        state::write_config(data, &config);
        config = state::read_config(data);
    }

    if price_e6 == 0 {
        return Err(PercolatorError::OracleInvalid.into());
    }

    let normalized_price =
        crate::policy::to_engine_price(price_e6, config.invert, config.unit_scale)
            .ok_or(PercolatorError::OracleInvalid)?;

    if normalized_price > percolator::MAX_ORACLE_PRICE {
        return Err(PercolatorError::OracleInvalid.into());
    }

    // Hyperp stale-recovery policy is deliberate: if mark liveness
    // has been lost beyond the catchup envelope while funding is
    // active, catchup_accrue may return CatchupRequired. Such a
    // market is RESOLVE-ONLY — operators who want revivable markets
    // must push frequently enough to stay within the envelope.
    {
        let engine = zc::engine_mut(data)?;
        reject_stuck_target_accrual(
            &config,
            engine,
            push_clock.slot,
            config.last_effective_price_e6,
        )?;
        catchup_accrue(
            engine,
            push_clock.slot,
            config.last_effective_price_e6,
            funding_rate_e9,
        )?;
        engine
            .accrue_market_to(
                push_clock.slot,
                config.last_effective_price_e6,
                funding_rate_e9,
            )
            .map_err(map_risk_error)?;
    }

    // Clamp against index (last_effective_price_e6). Bounds mark-index
    // gap to one cap-width regardless of how many same-slot pushes
    // occur.
    let clamp_base = config.last_effective_price_e6;
    let clamped = oracle::clamp_oracle_price(clamp_base, normalized_price, max_change_bps);
    config.hyperp_mark_e6 = clamped;
    config.last_mark_push_slot = push_clock.slot as u128;

    // Admin push feeds through EWMA like trades (full weight).
    config.mark_ewma_e6 = crate::policy::ewma_update(
        config.mark_ewma_e6,
        clamped,
        config.mark_ewma_halflife_slots,
        config.mark_ewma_last_slot,
        push_clock.slot,
        config.mark_min_fee,
        config.mark_min_fee,
    );
    config.mark_ewma_last_slot = push_clock.slot;
    state::write_config(data, &config);
    Ok(())
}
