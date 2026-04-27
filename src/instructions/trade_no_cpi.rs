//! Tag 6 — TradeNoCpi. Bilateral on-chain trade between user and LP
//! without any matcher CPI. Both sides sign; fills at the live oracle
//! price via the in-process `NoOpMatcher`. Hyperp markets reject this
//! path to prevent mark manipulation — Hyperp trades MUST go through
//! TradeCpi with a pinned matcher.
//!
//! Wire format:
//!   `[6u8] [lp_idx: u16] [user_idx: u16] [size: i128]`   (21 bytes)
//!
//! Accounts (strict order, matches legacy):
//!   1. user (Signer)                    — must equal engine.accounts[user_idx].owner
//!   2. lp   (Signer)                    — must equal engine.accounts[lp_idx].owner
//!   3. slab (Account<SlabHeader>, mut)
//!   4. clock (Sysvar<Clock>)
//!   5. oracle (UncheckedAccount)
//!
//! Behavior summary (matches legacy `Instruction::TradeNoCpi` arm).
//! Reads fresh oracle, accrues market, pre-syncs both sides' fees,
//! snapshots insurance balance, runs `execute_trade_with_matcher` with
//! `NoOpMatcher` (fills at oracle price), updates the mark EWMA from
//! the actual fee paid, then refreshes the risk buffer for both
//! counterparties.

use crate::errors::{map_risk_error, PercolatorError};
use crate::guards::{require_initialized, require_no_reentrancy, slab_shape_guard};
use crate::oracle;
use crate::processor::{
    check_idx, compute_current_funding_rate_e9, current_trade_fee_paid_cap,
    effective_pos_q_checked, ensure_market_accrued_to_now_with_policy,
    execute_trade_with_matcher, read_price_and_stamp, reject_any_target_lag, risk_notional_ceil,
    NoOpMatcher,
};
use crate::state::{self, SlabHeader};
use crate::zc;
use anchor_lang_v2::prelude::*;
use solana_program_error::ProgramError;

#[derive(Accounts)]
pub struct TradeNoCpi {
    pub user: Signer,
    pub lp: Signer,
    #[account(mut)]
    pub slab: Account<SlabHeader>,
    pub clock: Sysvar<Clock>,
    /// CHECK: foreign-program oracle account, validated by `oracle::*`.
    pub oracle: UncheckedAccount,
}

pub fn handler(
    ctx: &mut Context<TradeNoCpi>,
    lp_idx: u16,
    user_idx: u16,
    size: i128,
) -> Result<()> {
    if size == 0 || size == i128::MIN {
        return Err(ProgramError::InvalidInstructionData.into());
    }

    slab_shape_guard(&ctx.accounts.slab)?;
    let clock = *ctx.accounts.clock;
    let user_addr = *ctx.accounts.user.address();
    let lp_addr = *ctx.accounts.lp.address();
    let oracle_view = *ctx.accounts.oracle.account();

    let data: &mut [u8] = state::slab_data_mut(&mut ctx.accounts.slab);
    require_no_reentrancy(data)?;
    require_initialized(data)?;

    if zc::engine_ref(data)?.is_resolved() {
        return Err(ProgramError::InvalidAccountData.into());
    }

    let mut config = state::read_config(data);

    // Hyperp mode rejects TradeNoCpi to prevent mark price manipulation.
    if oracle::is_hyperp_mode(&config) {
        return Err(PercolatorError::HyperpTradeNoCpiDisabled.into());
    }

    // Anti-retroactivity (§5.5): capture funding rate before oracle read.
    let funding_rate_e9 = compute_current_funding_rate_e9(&config)?;

    // Read fresh signed oracle (engine enforces caps internally).
    let price = read_price_and_stamp(
        &mut config,
        &oracle_view,
        clock.unix_timestamp,
        clock.slot,
        data,
    )?;
    state::write_config(data, &config);

    let engine = zc::engine_mut(data)?;
    check_idx(engine, lp_idx)?;
    check_idx(engine, user_idx)?;

    // TradeNoCpi: no matcher check. Both sides are bilateral signers,
    // no CPI is invoked. Owner equality enforced here.
    let u_owner = engine.accounts[user_idx as usize].owner;
    if !crate::policy::owner_ok(u_owner, user_addr.to_bytes()) {
        return Err(PercolatorError::EngineUnauthorized.into());
    }
    let l_owner = engine.accounts[lp_idx as usize].owner;
    if !crate::policy::owner_ok(l_owner, lp_addr.to_bytes()) {
        return Err(PercolatorError::EngineUnauthorized.into());
    }

    // accrue → reject_lag → pre-sync fees on both sides → trade.
    ensure_market_accrued_to_now_with_policy(engine, &config, clock.slot, price, funding_rate_e9)?;
    reject_any_target_lag(&config, engine)?;

    // Pre-sync maintenance fees BEFORE capturing ins_before — without
    // this, the fee-paid delta would include maintenance accrual and
    // inflate EWMA weight on small trades after large maintenance
    // sync. The internal sync inside execute_trade_with_matcher
    // becomes a no-op at the same anchor.
    if config.maintenance_fee_per_slot > 0 {
        engine
            .sync_account_fee_to_slot_not_atomic(
                user_idx,
                clock.slot,
                config.maintenance_fee_per_slot,
            )
            .map_err(map_risk_error)?;
        engine
            .sync_account_fee_to_slot_not_atomic(
                lp_idx,
                clock.slot,
                config.maintenance_fee_per_slot,
            )
            .map_err(map_risk_error)?;
    }

    let current_fee_paid_cap =
        current_trade_fee_paid_cap(size, price, engine.params.trading_fee_bps)?;
    let ins_before = engine.insurance_fund.balance.get();

    // Pass `maintenance_fee_per_slot = 0` so the helper's internal
    // sync is a no-op (we pre-synced above).
    execute_trade_with_matcher(
        engine,
        &NoOpMatcher,
        lp_idx,
        user_idx,
        clock.slot,
        price,
        size,
        funding_rate_e9,
        0, // NoOpMatcher ignores lp_account_id
        0,
    )
    .map_err(map_risk_error)?;

    // Update mark EWMA from this fill (NoOpMatcher fills at oracle).
    let max_change_bps = engine.params.max_price_move_bps_per_slot;
    if max_change_bps > 0 {
        let clamped_price = oracle::clamp_oracle_price(
            crate::policy::mark_ewma_clamp_base(config.last_effective_price_e6),
            price,
            max_change_bps,
        );
        let fee_paid_nocpi = if config.mark_min_fee > 0 {
            let ins_after = engine.insurance_fund.balance.get();
            let delta = ins_after
                .saturating_sub(ins_before)
                .min(current_fee_paid_cap);
            core::cmp::min(delta, u64::MAX as u128) as u64
        } else {
            0u64
        };
        let old_ewma = config.mark_ewma_e6;
        // First-fill seed is the oracle price (not the exec price) so
        // an attacker can't imprint a biased mark on the first trade.
        let ewma_price = if old_ewma == 0 && config.last_effective_price_e6 > 0 {
            config.last_effective_price_e6
        } else {
            clamped_price
        };
        config.mark_ewma_e6 = crate::policy::ewma_update(
            old_ewma,
            ewma_price,
            config.mark_ewma_halflife_slots,
            config.mark_ewma_last_slot,
            clock.slot,
            fee_paid_nocpi,
            config.mark_min_fee,
        );
        // Only full-weight observations advance the EWMA clock.
        let full_weight_observation_nocpi =
            config.mark_min_fee == 0 || fee_paid_nocpi >= config.mark_min_fee;
        if full_weight_observation_nocpi {
            config.mark_ewma_last_slot = clock.slot;
        }
    }

    // Post-trade positions for the risk buffer.
    let user_eff_nocpi = effective_pos_q_checked(engine, user_idx as usize)?;
    let lp_eff_nocpi = effective_pos_q_checked(engine, lp_idx as usize)?;
    if !state::is_oracle_initialized(data) {
        state::set_oracle_initialized(data);
    }

    state::write_config(data, &config);

    {
        let mut buf = state::read_risk_buffer(data);
        for &(idx, eff) in &[(user_idx, user_eff_nocpi), (lp_idx, lp_eff_nocpi)] {
            if eff == 0 {
                buf.remove(idx);
            } else {
                let notional = risk_notional_ceil(eff, price);
                buf.upsert(idx, notional);
            }
        }
        state::write_risk_buffer(data, &buf);
    }
    Ok(())
}
