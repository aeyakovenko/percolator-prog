//! Tag 29 — ResolvePermissionless. Permissionless degenerate resolve
//! after the hard-timeout staleness window matures. No oracle account
//! at resolve time; settles at `engine.last_oracle_price`.
//!
//! Wire format:
//!   `[29u8]`     (payload-less)
//!
//! Accounts (strict order, matches legacy):
//!   1. slab (Account<SlabHeader>, mut)
//!   2. clock (Sysvar<Clock>)
//!
//! STRICT HARD-TIMEOUT POLICY:
//!   `clock.slot - last_live_slot >= permissionless_resolve_stale_slots`
//!   ⇒ market is dead; anyone may resolve at the engine's stored
//!   last-accrued price. `last_live_slot` is `last_good_oracle_slot`
//!   for non-Hyperp and `max(mark_ewma_last_slot, last_mark_push_slot)`
//!   for Hyperp.

use crate::errors::{map_risk_error, PercolatorError};
use crate::guards::{require_initialized, require_no_reentrancy, slab_shape_guard};
use crate::oracle;
use crate::state::{self, SlabHeader};
use crate::zc;
use anchor_lang_v2::prelude::*;
use solana_program_error::ProgramError;

#[derive(Accounts)]
pub struct ResolvePermissionless {
    #[account(mut)]
    pub slab: Account<SlabHeader>,
    pub clock: Sysvar<Clock>,
}

pub fn handler(ctx: &mut Context<ResolvePermissionless>) -> Result<()> {
    slab_shape_guard(&ctx.accounts.slab)?;
    let clock = *ctx.accounts.clock;

    let data: &mut [u8] = state::slab_data_mut(&mut ctx.accounts.slab);
    require_no_reentrancy(data)?;
    require_initialized(data)?;

    if zc::engine_ref(data)?.is_resolved() {
        return Err(ProgramError::InvalidAccountData.into());
    }

    let mut config = state::read_config(data);

    // A post-init cluster restart bypasses the slot-staleness gate so
    // markets with `permissionless_resolve_stale_slots == 0` can still
    // be resolved after a hard-fork freeze.
    let restarted = oracle::cluster_restarted_since_init(&config);
    if !restarted && config.permissionless_resolve_stale_slots == 0 {
        return Err(PercolatorError::InvalidConfigParam.into());
    }

    if !oracle::permissionless_stale_matured(&config, clock.slot) {
        return Err(PercolatorError::OracleStale.into());
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
    Ok(())
}
