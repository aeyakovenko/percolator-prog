//! Tag 14 — UpdateConfig. Admin tunes funding params + TVL/insurance cap.
//!
//! Wire format (Borsh, LE FixInt):
//!   `[14u8] [funding_horizon_slots: u64] [funding_k_bps: u64]`
//!   `       [funding_max_premium_bps: i64] [funding_max_e9_per_slot: i64]`
//!   `       [tvl_insurance_cap_mult: u16]`              (35 bytes)
//!
//! Accounts (strict order, matches legacy):
//!   1. admin (Signer)                       — must equal header.admin
//!   2. slab (Account<PercolatorSlab>, mut)
//!   3. clock (Sysvar<Clock>)
//!   4. oracle (UncheckedAccount)            — required for non-Hyperp
//!
//! Behavior summary (matches legacy `Instruction::UpdateConfig` arm):
//! ordinary admin reconfigure of live market funding parameters.
//! Refuses resolved + matured markets; refuses degenerate (rate=0)
//! arm by omitting the oracle. Hyperp markets flush the internal
//! index without external staleness check; non-Hyperp markets read
//! a fresh Pyth/Chainlink price via `read_price_and_stamp`. Final
//! `accrue_market_to` brings the engine to the current slot before
//! the new params take effect — anti-retroactivity §5.5.

use crate::errors::{map_risk_error, PercolatorError};
use crate::guards::{require_admin, require_initialized, require_no_reentrancy, slab_shape_guard};
use crate::oracle;
use crate::processor::{
    catchup_accrue, compute_current_funding_rate_e9, hyperp_target_price, price_move_residual_dt,
    read_price_and_stamp, reject_any_target_lag, reject_stuck_target_accrual,
};
use crate::state::{self, PercolatorSlab};
use crate::zc;
use anchor_lang_v2::prelude::*;
use solana_program_error::ProgramError;

#[derive(Accounts)]
pub struct UpdateConfig {
    pub admin: Signer,
    #[account(mut)]
    pub slab: Account<PercolatorSlab>,
    pub clock: Sysvar<Clock>,
    /// CHECK: required for non-Hyperp markets; validated inside
    /// `read_price_and_stamp` (which fails with OracleStale on a dead
    /// passed oracle, refusing the silent degenerate arm).
    pub oracle: UncheckedAccount,
}

pub fn handler(
    ctx: &mut Context<UpdateConfig>,
    funding_horizon_slots: u64,
    funding_k_bps: u64,
    funding_max_premium_bps: i64,
    funding_max_e9_per_slot: i64,
    tvl_insurance_cap_mult: u16,
) -> Result<()> {
    slab_shape_guard(&ctx.accounts.slab)?;
    let clock = *ctx.accounts.clock;
    let admin_addr = *ctx.accounts.admin.address();
    let oracle_view = *ctx.accounts.oracle.account();

    let data: &mut [u8] = state::slab_data_mut(&mut ctx.accounts.slab);
    require_no_reentrancy(data)?;
    require_initialized(data)?;
    if zc::engine_ref(data)?.is_resolved() {
        return Err(ProgramError::InvalidAccountData.into());
    }
    let header = state::read_header(data);
    require_admin(header.admin, &admin_addr)?;

    // Parameter validation.
    if funding_horizon_slots == 0 {
        return Err(PercolatorError::InvalidConfigParam.into());
    }
    // Reject negative funding bounds (reversed clamps panic). Compare
    // against the engine's stored per-market envelope in i128 space.
    if funding_max_premium_bps < 0 || funding_max_e9_per_slot < 0 {
        return Err(PercolatorError::InvalidConfigParam.into());
    }
    let engine_envelope = zc::engine_ref(data)?.params.max_abs_funding_e9_per_slot;
    if (funding_max_e9_per_slot as i128) > engine_envelope as i128 {
        return Err(PercolatorError::InvalidConfigParam.into());
    }

    let mut config = state::read_config(data);

    if funding_k_bps > 100_000 {
        return Err(PercolatorError::InvalidConfigParam.into());
    }

    // Anti-retroactivity (§5.5): capture funding rate before any
    // config mutation.
    let funding_rate_e9 = compute_current_funding_rate_e9(&config)?;

    // Hard-timeout gate: UpdateConfig must not mutate a terminally-
    // stale market. Admin has no "emergency reconfigure" path past
    // the hard timeout — the market is dead, users exit via
    // ResolvePermissionless.
    if oracle::permissionless_stale_matured(&config, clock.slot) {
        return Err(PercolatorError::OracleStale.into());
    }

    // Flush Hyperp index WITHOUT external staleness check (admin
    // recovery path; the hard-timeout gate above handles terminal).
    if oracle::is_hyperp_mode(&config) {
        let engine = zc::engine_ref(data)?;
        let max_change_bps = engine.params.max_price_move_bps_per_slot;
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

    // Accrue to boundary. ORDINARY (Hyperp index OR fresh non-Hyperp
    // oracle) only — admin may NOT enter a degenerate (rate=0) arm by
    // omitting the oracle. `read_price_and_stamp` returning OracleStale
    // / OracleConfTooWide propagates as a hard error so admin must use
    // ResolveMarket / ResolvePermissionless instead.
    {
        let (accrual_price, rate_for_accrual): (u64, i128) = if oracle::is_hyperp_mode(&config) {
            (config.last_effective_price_e6, funding_rate_e9)
        } else {
            match read_price_and_stamp(
                &mut config,
                &oracle_view,
                clock.unix_timestamp,
                clock.slot,
                data,
            ) {
                Ok(price) => {
                    state::write_config(data, &config);
                    (price, funding_rate_e9)
                }
                Err(e) => return Err(e.into()),
            }
        };
        if accrual_price > 0 {
            {
                let engine = zc::engine_mut(data)?;
                reject_stuck_target_accrual(&config, engine, clock.slot, accrual_price)?;
                catchup_accrue(engine, clock.slot, accrual_price, rate_for_accrual)?;
                engine
                    .accrue_market_to(clock.slot, accrual_price, rate_for_accrual)
                    .map_err(map_risk_error)?;
            }
            if !state::is_oracle_initialized(data) {
                state::set_oracle_initialized(data);
            }
        }
    }

    reject_any_target_lag(&config, zc::engine_ref(data)?)?;

    config.funding_horizon_slots = funding_horizon_slots;
    config.funding_k_bps = funding_k_bps;
    config.funding_max_premium_bps = funding_max_premium_bps;
    config.funding_max_e9_per_slot = funding_max_e9_per_slot;
    config.tvl_insurance_cap_mult = tvl_insurance_cap_mult;
    state::write_config(data, &config);
    Ok(())
}
