//! Tag 25 — ReclaimEmptyAccount. Permissionless reclamation of
//! flat/dust accounts (spec §2.6, §10.7).
//!
//! Wire format:
//!   `[25u8] [user_idx: u16]`   (3 bytes)
//!
//! Accounts (strict order, matches legacy):
//!   1. slab (Account<SlabHeader>, mut) — market state
//!   2. clock (Sysvar<Clock>)            — runtime clock
//!
//! Behavior summary (matches legacy `Instruction::ReclaimEmptyAccount`
//! arm). Recycles a flat/dust account slot without touching side
//! state. Blocks on resolved markets and on the hard-timeout matured
//! gate. Per §10.7 must NOT call `accrue_market_to` and must NOT
//! mutate side state — the only engine call is
//! `reclaim_empty_account_not_atomic` after a bounded-to-market fee
//! sync.

use crate::errors::{map_risk_error, PercolatorError};
use crate::guards::{require_initialized, require_no_reentrancy, slab_shape_guard};
use crate::oracle;
use crate::processor::{check_no_oracle_live_envelope, sync_account_fee_bounded_to_market};
use crate::state::{self, SlabHeader};
use crate::zc;
use anchor_lang_v2::prelude::*;
use solana_program_error::ProgramError;

#[derive(Accounts)]
pub struct ReclaimEmptyAccount {
    #[account(mut)]
    pub slab: Account<SlabHeader>,
    pub clock: Sysvar<Clock>,
}

pub fn handler(ctx: &mut Context<ReclaimEmptyAccount>, user_idx: u16) -> Result<()> {
    slab_shape_guard(&ctx.accounts.slab)?;
    let clock = *ctx.accounts.clock;

    let data: &mut [u8] = state::slab_data_mut(&mut ctx.accounts.slab);
    require_no_reentrancy(data)?;
    require_initialized(data)?;

    if zc::engine_ref(data)?.is_resolved() {
        return Err(ProgramError::InvalidAccountData.into());
    }

    let config = state::read_config(data);
    if oracle::permissionless_stale_matured(&config, clock.slot) {
        return Err(PercolatorError::OracleStale.into());
    }

    let engine = zc::engine_mut(data)?;
    check_no_oracle_live_envelope(engine, clock.slot)?;
    // Bounded-to-market fee sync (no accrue) per §10.7.
    sync_account_fee_bounded_to_market(engine, &config, user_idx, clock.slot)?;
    engine
        .reclaim_empty_account_not_atomic(user_idx, clock.slot)
        .map_err(map_risk_error)?;
    // §10.7: MUST NOT accrue_market_to, MUST NOT mutate side state.
    Ok(())
}
