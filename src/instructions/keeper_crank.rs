//! Tag 5 — KeeperCrank. Two-phase: candidates computed off-chain
//! drive `liquidate_at_oracle_not_atomic` calls, with risk-buffer
//! maintenance + maintenance-fee sweep + crank-reward credit.
//!
//! Wire format (Anchor v2, wincode/Borsh `BORSH_CONFIG`). Per migration
//! plan R6, the engine's `LiquidationPolicy` is not Borsh-serializable,
//! so candidates use a wire shim that decodes into engine-space inside
//! the handler.
//!
//!   `[5u8] [caller_idx: u16] [Vec<WireLiquidationCandidate>]`
//!
//! Where each `WireLiquidationCandidate` is `{ idx: u16,
//! policy: Option<WireLiquidationPolicy> }` and
//! `WireLiquidationPolicy = { tag: u8, partial_amount: Option<u128> }`.
//!   - `policy = None`           → engine touch-only (legacy tag 0xFF)
//!   - `policy = Some(tag = 0)`  → `LiquidationPolicy::FullClose`
//!   - `policy = Some(tag = 1)`  → `LiquidationPolicy::ExactPartial(q)`
//!     (`partial_amount` MUST be `Some`)
//!
//! ABI deltas vs legacy (deliberate):
//!   - candidate vector length encoded as wincode's `u32 LE` (Borsh
//!     `Vec<T>` length) instead of the legacy implicit "consume rest";
//!   - `format_version` byte is removed (always v2 now);
//!   - per-candidate touch-only is `Option::None`, not the legacy
//!     `0xFF` tag byte.
//!
//! Accounts (strict order, matches legacy):
//!   1. caller (Signer if `caller_idx != CRANK_NO_CALLER`, else
//!      placeholder)
//!   2. slab (`Account<PercolatorSlab>`, mut)
//!   3. clock (Sysvar<Clock>)
//!   4. oracle (UncheckedAccount)

use crate::constants::{
    CRANK_NO_CALLER, CRANK_REWARD_BPS, FEE_SWEEP_BUDGET, LIQ_BUDGET_PER_CRANK,
    RISK_BUF_CAP, RISK_SCAN_WINDOW, RR_WINDOW_PER_CRANK,
};
use crate::errors::{map_risk_error, PercolatorError};
use crate::guards::{require_initialized, require_no_reentrancy, slab_shape_guard};
use crate::oracle;
use crate::processor::{
    check_idx, compute_current_funding_rate_e9, effective_pos_q_checked,
    ensure_market_accrued_to_now_with_policy, idx_used_in_market, price_move_residual_dt,
    read_price_and_stamp, risk_notional_ceil,
};
use crate::state::{self, PercolatorSlab};
use crate::zc;
use anchor_lang_v2::prelude::*;
use solana_program_error::ProgramError;

/// Wire-only shim for `percolator::LiquidationPolicy` (which is not
/// `BorshDeserialize` upstream). Convert via `to_engine` inside the
/// handler.
#[derive(wincode::SchemaWrite, wincode::SchemaRead, Clone)]
pub struct WireLiquidationPolicy {
    pub tag: u8,
    pub partial_amount: Option<u128>,
}

impl WireLiquidationPolicy {
    fn to_engine(&self) -> Result<percolator::LiquidationPolicy> {
        match self.tag {
            0 => Ok(percolator::LiquidationPolicy::FullClose),
            1 => {
                let q = self
                    .partial_amount
                    .ok_or(ProgramError::InvalidInstructionData)?;
                Ok(percolator::LiquidationPolicy::ExactPartial(q))
            }
            _ => Err(ProgramError::InvalidInstructionData.into()),
        }
    }
}

#[derive(wincode::SchemaWrite, wincode::SchemaRead, Clone)]
pub struct WireLiquidationCandidate {
    pub idx: u16,
    /// `None` = legacy "touch-only" (engine-side risk buffer maintenance
    /// without invoking liquidation policy).
    pub policy: Option<WireLiquidationPolicy>,
}

#[derive(Accounts)]
pub struct KeeperCrank {
    /// Permissionless callers can pass any account here (it is not
    /// signed nor read when `caller_idx == CRANK_NO_CALLER`).
    /// CHECK: signer flag is enforced by the handler when the crank is
    /// non-permissionless; key equality vs the engine-stored owner is
    /// enforced after slab decoding.
    pub caller: UncheckedAccount,
    #[account(mut)]
    pub slab: Account<PercolatorSlab>,
    pub clock: Sysvar<Clock>,
    /// CHECK: foreign-program oracle account, validated by `oracle::*`.
    pub oracle: UncheckedAccount,
}

pub fn handler(
    ctx: &mut Context<KeeperCrank>,
    caller_idx: u16,
    candidates: alloc::vec::Vec<WireLiquidationCandidate>,
) -> Result<()> {
    // Defense-in-depth on the caller-supplied list. The handler also
    // truncates `combined` below, but enforcing here keeps the scan
    // bounded if the cap is loosened.
    const MAX_CANDIDATES: usize = (LIQ_BUDGET_PER_CRANK as usize) * 2;
    if candidates.len() > MAX_CANDIDATES {
        return Err(ProgramError::InvalidInstructionData.into());
    }

    slab_shape_guard(&ctx.accounts.slab)?;

    let permissionless = caller_idx == CRANK_NO_CALLER;
    if !permissionless && !ctx.accounts.caller.account().is_signer() {
        return Err(ProgramError::MissingRequiredSignature.into());
    }

    let clock = *ctx.accounts.clock;
    let oracle_view = *ctx.accounts.oracle.account();
    let caller_addr = *ctx.accounts.caller.address();

    // Convert wire candidates to engine-space.
    let engine_candidates: alloc::vec::Vec<(u16, Option<percolator::LiquidationPolicy>)> = {
        let mut v = alloc::vec::Vec::with_capacity(candidates.len());
        for c in candidates.iter() {
            let policy = match &c.policy {
                None => None,
                Some(p) => Some(p.to_engine()?),
            };
            v.push((c.idx, policy));
        }
        v
    };

    let data: &mut [u8] = state::slab_data_mut(&mut ctx.accounts.slab);
    require_no_reentrancy(data)?;
    require_initialized(data)?;

    // Resolved-market shortcut: settlement is idempotent.
    if zc::engine_ref(data)?.is_resolved() {
        let engine = zc::engine_mut(data)?;
        let (resolved_price, _) = engine.resolved_context();
        if resolved_price == 0 {
            return Err(ProgramError::InvalidAccountData.into());
        }
        return Ok(());
    }

    let mut config = state::read_config(data);

    let is_hyperp = oracle::is_hyperp_mode(&config);
    let (engine_last_oracle_price, price_move_dt, cap_bps, oi_any) = {
        let engine = zc::engine_ref(data)?;
        (
            engine.last_oracle_price,
            price_move_residual_dt(engine, clock.slot)?,
            engine.params.max_price_move_bps_per_slot,
            engine.oi_eff_long_q != 0 || engine.oi_eff_short_q != 0,
        )
    };

    // Anti-retroactivity (§5.5): capture funding rate before oracle read.
    let funding_rate_e9_pre = compute_current_funding_rate_e9(&config)?;

    let price = if is_hyperp {
        oracle::get_engine_oracle_price_e6(
            engine_last_oracle_price,
            price_move_dt,
            clock.slot,
            clock.unix_timestamp,
            &mut config,
            &oracle_view,
            cap_bps,
            oi_any,
        )?
    } else {
        read_price_and_stamp(&mut config, &oracle_view, clock.unix_timestamp, clock.slot, data)?
    };
    state::write_config(data, &config);

    let buf_pre = state::read_risk_buffer(data);

    let engine = zc::engine_mut(data)?;

    // Crank authorization.
    if !permissionless {
        check_idx(engine, caller_idx)?;
        let stored_owner = engine.accounts[caller_idx as usize].owner;
        if !crate::policy::owner_ok(stored_owner, caller_addr.to_bytes()) {
            return Err(PercolatorError::EngineUnauthorized.into());
        }
    }

    // Build `combined` = [risk_buffer entries (FullClose), …caller candidates].
    let mut combined: alloc::vec::Vec<(u16, Option<percolator::LiquidationPolicy>)> =
        alloc::vec::Vec::with_capacity(buf_pre.count as usize + engine_candidates.len());
    for i in 0..buf_pre.count as usize {
        combined.push((
            buf_pre.entries[i].idx,
            Some(percolator::LiquidationPolicy::FullClose),
        ));
    }
    combined.extend_from_slice(&engine_candidates);
    const COMBINED_CAP: usize = RISK_BUF_CAP + (LIQ_BUDGET_PER_CRANK as usize) * 2;
    if combined.len() > COMBINED_CAP {
        combined.truncate(COMBINED_CAP);
    }

    // Fully accrue market to clock.slot BEFORE sweeping fees, per
    // legacy ordering: accrue → candidate-syncs → sweep → keeper_crank.
    // Accruing first means the sweep + crank operate on a fully-accrued
    // market and the engine's internal accrue inside
    // keeper_crank_not_atomic no-ops at dt=0+same-price.
    ensure_market_accrued_to_now_with_policy(engine, &config, clock.slot, price, funding_rate_e9_pre)?;

    // Maintenance-fee sweep + candidate-directed syncs (audit #2).
    let ins_before = engine.insurance_fund.balance.get();
    let mut candidate_syncs = 0usize;
    if config.maintenance_fee_per_slot > 0 {
        let cap = core::cmp::min(LIQ_BUDGET_PER_CRANK as usize, FEE_SWEEP_BUDGET);
        let mut synced: [u16; LIQ_BUDGET_PER_CRANK as usize] =
            [u16::MAX; LIQ_BUDGET_PER_CRANK as usize];
        let mut synced_count = 0usize;
        let mut attempts = 0usize;
        for &(idx, _policy) in combined.iter() {
            if attempts >= cap {
                break;
            }
            if candidate_syncs >= FEE_SWEEP_BUDGET {
                break;
            }
            if !idx_used_in_market(engine, idx as usize) {
                continue;
            }
            attempts += 1;
            let mut already = false;
            for j in 0..synced_count {
                if synced[j] == idx {
                    already = true;
                    break;
                }
            }
            if already {
                continue;
            }
            engine
                .sync_account_fee_to_slot_not_atomic(
                    idx,
                    clock.slot,
                    config.maintenance_fee_per_slot,
                )
                .map_err(map_risk_error)?;
            synced[synced_count] = idx;
            synced_count += 1;
            candidate_syncs += 1;
        }
    }

    let remaining_budget = FEE_SWEEP_BUDGET.saturating_sub(candidate_syncs);
    crate::processor::sweep_maintenance_fees(engine, &mut config, clock.slot, remaining_budget)?;
    let sweep_delta = engine
        .insurance_fund
        .balance
        .get()
        .saturating_sub(ins_before);

    let admit_h_min = engine.params.h_min;
    let admit_h_max = engine.params.h_max;
    let admit_threshold = Some(engine.params.maintenance_margin_bps as u128);
    let _outcome = engine
        .keeper_crank_not_atomic(
            clock.slot,
            price,
            &combined,
            LIQ_BUDGET_PER_CRANK,
            funding_rate_e9_pre,
            admit_h_min,
            admit_h_max,
            admit_threshold,
            RR_WINDOW_PER_CRANK,
        )
        .map_err(map_risk_error)?;

    // Crank reward (50 % of sweep_delta) — non-permissionless callers only.
    if !permissionless
        && config.maintenance_fee_per_slot > 0
        && sweep_delta > 0
        && idx_used_in_market(engine, caller_idx as usize)
    {
        let mut reward = sweep_delta.saturating_mul(CRANK_REWARD_BPS) / 10_000u128;
        let ins_now = engine.insurance_fund.balance.get();
        if reward > ins_now {
            reward = ins_now;
        }
        if reward > 0 {
            engine
                .credit_account_from_insurance_not_atomic(caller_idx, reward, clock.slot)
                .map_err(map_risk_error)?;
        }
        let post_balance = engine.insurance_fund.balance.get();
        if post_balance > ins_now {
            return Err(PercolatorError::EngineCorruptState.into());
        }
    }

    // Engine borrow drop; flush oracle-init flag + config.
    if !state::is_oracle_initialized(data) {
        state::set_oracle_initialized(data);
    }
    state::write_config(data, &config);

    // ── Risk-buffer maintenance (engine borrow dropped) ──
    {
        let mut buf = state::read_risk_buffer(data);
        let engine = zc::engine_ref(data)?;

        // Phase A: scrub dead entries.
        for i in (0..RISK_BUF_CAP).rev() {
            if i >= buf.count as usize {
                continue;
            }
            let eidx = buf.entries[i].idx as usize;
            if !idx_used_in_market(engine, eidx) || effective_pos_q_checked(engine, eidx)? == 0 {
                buf.remove(buf.entries[i].idx);
            }
        }

        // Phase B: refresh surviving entries.
        for i in 0..buf.count as usize {
            let eidx = buf.entries[i].idx as usize;
            let eff = effective_pos_q_checked(engine, eidx)?;
            let notional = risk_notional_ceil(eff, price);
            buf.entries[i].notional = notional;
        }
        buf.recompute_min();

        // Phase C: progressive discovery scan.
        let scan_mod = engine.params.max_accounts as usize;
        let scan_mod = if scan_mod == 0 || scan_mod > percolator::MAX_ACCOUNTS {
            percolator::MAX_ACCOUNTS
        } else {
            scan_mod
        };
        let scan_start = (buf.scan_cursor as usize) % scan_mod;
        for offset in 0..RISK_SCAN_WINDOW {
            let idx = (scan_start + offset) % scan_mod;
            if !idx_used_in_market(engine, idx) {
                continue;
            }
            let eff = effective_pos_q_checked(engine, idx)?;
            if eff == 0 {
                continue;
            }
            let notional = risk_notional_ceil(eff, price);
            buf.upsert(idx as u16, notional);
        }
        buf.scan_cursor = ((scan_start + RISK_SCAN_WINDOW) % scan_mod) as u16;

        // Phase D: ingest caller-supplied candidates.
        for &(cidx, _) in engine_candidates.iter() {
            let ci = cidx as usize;
            if !idx_used_in_market(engine, ci) {
                continue;
            }
            let eff = effective_pos_q_checked(engine, ci)?;
            if eff == 0 {
                buf.remove(cidx);
            } else {
                let notional = risk_notional_ceil(eff, price);
                buf.upsert(cidx, notional);
            }
        }

        state::write_risk_buffer(data, &buf);
    }

    Ok(())
}
