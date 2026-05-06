//! Tag 10 — TradeCpi. User executes a trade against an LP-pinned
//! matcher program via cross-program invocation. The matcher program
//! signs as the LP PDA (`[b"lp", slab, lp_idx]`); the wrapper validates
//! the matcher's return ABI and applies the trade in-engine.
//!
//! Wire format:
//!   `[10u8] [lp_idx: u16] [user_idx: u16] [size: i128] [limit_price_e6: u64]`
//!
//! Accounts (strict order, matches legacy):
//!   1. user            (Signer)
//!   2. lp_owner        (UncheckedAccount, NOT a signer — LP owner has
//!                       delegated trade auth to the matcher)
//!   3. slab            (`Account<PercolatorSlab>`, mut)
//!   4. clock           (Sysvar<Clock>)
//!   5. oracle          (UncheckedAccount)
//!   6. matcher_program (UncheckedAccount, executable)
//!   7. matcher_ctx     (UncheckedAccount, mut, owned by matcher)
//!   8. lp_pda          (UncheckedAccount, derived as
//!                       `find_program_address(["lp", slab, lp_idx])`)
//!   9..  variadic tail accounts forwarded verbatim to the matcher CPI.
//!        Cap: `MAX_MATCHER_TAIL_ACCOUNTS`. The wrapper does NOT
//!        interpret them — the matcher is responsible for validating
//!        keys / owners / data on anything it uses. Available via
//!        `ctx.remaining_accounts()`.

use crate::constants::{
    MATCHER_CALL_LEN, MATCHER_CALL_TAG, MAX_MATCHER_TAIL_ACCOUNTS,
};
use crate::cpi;
use crate::errors::{map_risk_error, PercolatorError};
use crate::guards::{require_initialized, require_no_reentrancy, slab_shape_guard};
use crate::oracle;
use crate::processor::{
    catchup_accrue, check_idx, compute_current_funding_rate_e9, current_trade_fee_paid_cap,
    effective_pos_q_checked, ensure_market_accrued_to_now_with_policy, execute_trade_with_matcher,
    price_move_residual_dt_from_parts, read_price_and_stamp, reject_stuck_target_accrual,
    risk_notional_ceil, target_lag_after_read, CpiMatcher,
};
use crate::state::{self, PercolatorSlab};
use crate::zc;
use anchor_lang_v2::prelude::*;
use solana_program_error::ProgramError;

#[derive(Accounts)]
#[instruction(lp_idx: u16, user_idx: u16, size: i128, limit_price_e6: u64)]
pub struct TradeCpi {
    pub user: Signer,
    /// LP owner — does NOT sign. Authority is delegated to the matcher
    /// via the LP PDA. Equality vs `engine.accounts[lp_idx].owner` is
    /// enforced inside the handler.
    /// CHECK: validated by `policy::owner_ok` against the engine slab.
    pub lp_owner: UncheckedAccount,
    #[account(mut)]
    pub slab: Account<PercolatorSlab>,
    pub clock: Sysvar<Clock>,
    /// CHECK: foreign-program oracle account, validated by `oracle::*`.
    pub oracle: UncheckedAccount,
    /// CHECK: matcher program, executable + owner gate inside handler.
    pub matcher_program: UncheckedAccount,
    /// CHECK: matcher-owned context account; identity binding enforced
    /// by `policy::matcher_identity_ok` against the LP slab record.
    #[account(mut)]
    pub matcher_ctx: UncheckedAccount,
    /// LP PDA `[b"lp", slab, lp_idx]`. Framework-validated; the
    /// canonical bump lands in `ctx.bumps.lp_pda` for the matcher CPI
    /// signing seeds.
    /// CHECK: framework-validated via `seeds` + `bump` constraint.
    #[account(seeds = [b"lp", slab, lp_idx.to_le_bytes().as_ref()], bump)]
    pub lp_pda: UncheckedAccount,
}

pub fn handler(
    ctx: &mut Context<TradeCpi>,
    lp_idx: u16,
    user_idx: u16,
    size: i128,
    limit_price_e6: u64,
) -> Result<()> {
    if size == 0 || size == i128::MIN {
        return Err(ProgramError::InvalidInstructionData.into());
    }
    if lp_idx == user_idx {
        return Err(ProgramError::InvalidInstructionData.into());
    }

    slab_shape_guard(&ctx.accounts.slab)?;

    // Snapshot all AccountView references up front; remaining_accounts
    // takes &mut self.
    let user_addr = *ctx.accounts.user.address();
    let lp_owner_addr = *ctx.accounts.lp_owner.address();
    let oracle_view = *ctx.accounts.oracle.account();
    let matcher_prog_view = *ctx.accounts.matcher_program.account();
    let matcher_ctx_view = *ctx.accounts.matcher_ctx.account();
    let lp_pda_view = *ctx.accounts.lp_pda.account();
    let clock = *ctx.accounts.clock;
    let slab_addr = *ctx.accounts.slab.account().address();

    let matcher_prog_addr = *matcher_prog_view.address();
    let matcher_ctx_addr = *matcher_ctx_view.address();
    let lp_pda_addr = *lp_pda_view.address();

    let tail: alloc::vec::Vec<pinocchio::account::AccountView> = ctx.remaining_accounts();
    if tail.len() > MAX_MATCHER_TAIL_ACCOUNTS {
        return Err(ProgramError::InvalidInstructionData.into());
    }

    // Matcher shape gate (no executable read/write yet).
    let shape = crate::policy::MatcherAccountsShape {
        prog_executable: matcher_prog_view.executable(),
        ctx_executable: matcher_ctx_view.executable(),
        ctx_owner_is_prog: matcher_ctx_view.owner() == &matcher_prog_addr,
        ctx_len_ok: crate::policy::ctx_len_sufficient(matcher_ctx_view.data_len()),
    };
    if !crate::policy::matcher_shape_ok(shape) {
        return Err(ProgramError::InvalidAccountData.into());
    }

    // LP PDA framework-validated; bump cached in ctx.bumps for the
    // matcher CPI signing seeds below.
    let lp_bytes = lp_idx.to_le_bytes();
    let bump = ctx.bumps.lp_pda;
    let _ = lp_pda_addr;

    // Phase 3+4: read slab state, generate nonce, validate matcher identity.
    let (
        lp_account_id,
        mut config,
        config_pre_oracle,
        req_id,
        engine_last_oracle_price,
        engine_last_market_slot,
        engine_max_accrual_dt_slots,
        engine_cap_bps,
        engine_oi_any,
    ) = {
        let data: &mut [u8] = state::slab_data_mut(&mut ctx.accounts.slab);
        require_no_reentrancy(data)?;
        require_initialized(data)?;

        if zc::engine_ref(data)?.is_resolved() {
            return Err(ProgramError::InvalidAccountData.into());
        }

        let config = state::read_config(data);
        let nonce = state::read_req_nonce(data);
        let req_id = crate::policy::nonce_on_success(nonce)
            .ok_or(PercolatorError::EngineOverflow)?;

        let engine = zc::engine_ref(data)?;
        check_idx(engine, lp_idx)?;
        check_idx(engine, user_idx)?;

        // LP must have a configured matcher (non-zero matcher_program).
        if engine.accounts[lp_idx as usize].matcher_program == [0u8; 32] {
            return Err(PercolatorError::EngineAccountKindMismatch.into());
        }

        // Owner authorization (user is a signer; lp_owner is NOT).
        let u_owner = engine.accounts[user_idx as usize].owner;
        if !crate::policy::owner_ok(u_owner, user_addr.to_bytes()) {
            return Err(PercolatorError::EngineUnauthorized.into());
        }
        let l_owner = engine.accounts[lp_idx as usize].owner;
        if !crate::policy::owner_ok(l_owner, lp_owner_addr.to_bytes()) {
            return Err(PercolatorError::EngineUnauthorized.into());
        }

        // Matcher identity binding.
        let lp_acc = &engine.accounts[lp_idx as usize];
        if !crate::policy::matcher_identity_ok(
            lp_acc.matcher_program,
            lp_acc.matcher_context,
            matcher_prog_addr.to_bytes(),
            matcher_ctx_addr.to_bytes(),
        ) {
            return Err(PercolatorError::EngineInvalidMatchingEngine.into());
        }

        let lp_instance_id = state::read_account_generation(data, lp_idx);
        if lp_instance_id == 0 {
            return Err(PercolatorError::EngineAccountNotFound.into());
        }

        (
            lp_instance_id,
            config,
            config,
            req_id,
            engine.last_oracle_price,
            engine.last_market_slot,
            engine.params.max_accrual_dt_slots,
            engine.params.max_price_move_bps_per_slot,
            engine.oi_eff_long_q != 0 || engine.oi_eff_short_q != 0,
        )
    };

    // Anti-retroactivity: capture funding rate before oracle read.
    let funding_rate_e9_pre = compute_current_funding_rate_e9(&config)?;

    // Oracle price.
    let is_hyperp = oracle::is_hyperp_mode(&config);
    let price = if is_hyperp {
        let price_move_dt = price_move_residual_dt_from_parts(
            engine_last_market_slot,
            engine_max_accrual_dt_slots,
            clock.slot,
        )?;
        oracle::get_engine_oracle_price_e6(
            engine_last_oracle_price,
            price_move_dt,
            clock.slot,
            clock.unix_timestamp,
            &mut config,
            &oracle_view,
            engine_cap_bps,
            engine_oi_any,
        )?
    } else {
        let data = state::slab_data_mut(&mut ctx.accounts.slab);
        read_price_and_stamp(&mut config, &oracle_view, clock.unix_timestamp, clock.slot, data)?
    };

    if target_lag_after_read(&config, price) {
        return Err(PercolatorError::CatchupRequired.into());
    }

    // Stack-allocated CPI data (67 bytes).
    let mut cpi_data = [0u8; MATCHER_CALL_LEN];
    cpi_data[0] = MATCHER_CALL_TAG;
    cpi_data[1..9].copy_from_slice(&req_id.to_le_bytes());
    cpi_data[9..11].copy_from_slice(&lp_idx.to_le_bytes());
    cpi_data[11..19].copy_from_slice(&lp_account_id.to_le_bytes());
    cpi_data[19..27].copy_from_slice(&price.to_le_bytes());
    cpi_data[27..43].copy_from_slice(&size.to_le_bytes());
    // bytes 43..67 already zero (padding).

    // Set reentrancy guard BEFORE CPI; clear AFTER.
    {
        let data = state::slab_data_mut(&mut ctx.accounts.slab);
        state::set_cpi_in_progress(data);
    }

    let bump_arr = [bump];
    let signer_seeds: &[&[u8]] = &[b"lp", slab_addr.as_ref(), &lp_bytes, &bump_arr];

    cpi::forward_matcher(
        &matcher_prog_view,
        &matcher_ctx_view,
        &lp_pda_view,
        &tail,
        &cpi_data,
        signer_seeds,
    )?;

    {
        let data = state::slab_data_mut(&mut ctx.accounts.slab);
        state::clear_cpi_in_progress(data);
    }

    // Read matcher return + ABI validation.
    let ret = {
        let ctx_data = matcher_ctx_view
            .try_borrow()
            .map_err(|_| ProgramError::AccountBorrowFailed)?;
        crate::matcher_abi::read_matcher_return(&ctx_data)?
    };
    let ret_fields = crate::policy::MatcherReturnFields {
        abi_version: ret.abi_version,
        flags: ret.flags,
        exec_price_e6: ret.exec_price_e6,
        exec_size: ret.exec_size,
        req_id: ret.req_id,
        lp_account_id: ret.lp_account_id,
        oracle_price_e6: ret.oracle_price_e6,
        reserved: ret.reserved,
    };
    if !crate::policy::abi_ok(ret_fields, lp_account_id, price, size, req_id) {
        return Err(ProgramError::InvalidAccountData.into());
    }

    // Slippage protection (engine-space).
    if limit_price_e6 != 0 && ret.exec_size != 0 {
        let limit_eng = crate::policy::to_engine_price(limit_price_e6, config.invert, config.unit_scale)
            .ok_or(PercolatorError::OracleInvalid)?;
        let inverted = config.invert != 0;
        if size > 0 {
            let bad = if inverted {
                ret.exec_price_e6 < limit_eng
            } else {
                ret.exec_price_e6 > limit_eng
            };
            if bad {
                return Err(ProgramError::InvalidAccountData.into());
            }
        } else {
            let bad = if inverted {
                ret.exec_price_e6 > limit_eng
            } else {
                ret.exec_price_e6 < limit_eng
            };
            if bad {
                return Err(ProgramError::InvalidAccountData.into());
            }
        }
    }

    // Zero-fill: ABI-valid no-op when matcher returns exec_size == 0.
    if ret.exec_size == 0 {
        let data = state::slab_data_mut(&mut ctx.accounts.slab);
        let engine = zc::engine_mut(data)?;
        reject_stuck_target_accrual(&config, engine, clock.slot, price)?;
        catchup_accrue(engine, clock.slot, price, funding_rate_e9_pre)?;
        engine
            .accrue_market_to(clock.slot, price, funding_rate_e9_pre)
            .map_err(map_risk_error)?;
        let mut restored = config_pre_oracle;
        restored.last_good_oracle_slot = config.last_good_oracle_slot;
        restored.last_effective_price_e6 = config.last_effective_price_e6;
        restored.last_oracle_publish_time = config.last_oracle_publish_time;
        restored.oracle_target_price_e6 = config.oracle_target_price_e6;
        restored.oracle_target_publish_time = config.oracle_target_publish_time;
        restored.last_hyperp_index_slot = config.last_hyperp_index_slot;
        state::write_config(data, &restored);
        state::write_req_nonce(data, req_id);
        return Ok(());
    }

    let exec_price = ret.exec_price_e6;
    if exec_price > percolator::MAX_ORACLE_PRICE {
        return Err(PercolatorError::OracleInvalid.into());
    }

    // Anti-off-market band check (§14.3).
    if exec_price > 0 && price > 0 {
        let band_bps = {
            let data = state::slab_data(&ctx.accounts.slab);
            let engine = zc::engine_ref(data)?;
            let fee_bps = engine.params.trading_fee_bps;
            core::cmp::max(fee_bps.saturating_mul(2), 100)
        };
        let diff = if exec_price > price {
            exec_price - price
        } else {
            price - exec_price
        };
        let lhs = (diff as u128).saturating_mul(10_000);
        let rhs = (band_bps as u128).saturating_mul(price as u128);
        if lhs > rhs {
            return Err(PercolatorError::OracleInvalid.into());
        }
    }

    // Trade execution + post-trade EWMA + Hyperp mark.
    {
        let data = state::slab_data_mut(&mut ctx.accounts.slab);
        let engine = zc::engine_mut(data)?;

        ensure_market_accrued_to_now_with_policy(engine, &config, clock.slot, price, funding_rate_e9_pre)?;

        if config.maintenance_fee_per_slot > 0 {
            engine
                .sync_account_fee_to_slot_not_atomic(user_idx, clock.slot, config.maintenance_fee_per_slot)
                .map_err(map_risk_error)?;
            engine
                .sync_account_fee_to_slot_not_atomic(lp_idx, clock.slot, config.maintenance_fee_per_slot)
                .map_err(map_risk_error)?;
        }

        let trade_size = crate::policy::cpi_trade_size(ret.exec_size, size);
        let current_fee_paid_cap =
            current_trade_fee_paid_cap(trade_size, exec_price, engine.params.trading_fee_bps)?;
        let ins_before_cpi = engine.insurance_fund.balance.get();

        let matcher = CpiMatcher {
            exec_price,
            exec_size: trade_size,
        };
        execute_trade_with_matcher(
            engine,
            &matcher,
            lp_idx,
            user_idx,
            clock.slot,
            price,
            trade_size,
            funding_rate_e9_pre,
            lp_account_id,
            0,
        )
        .map_err(map_risk_error)?;

        let old_ewma_cpi = config.mark_ewma_e6;
        let max_change_bps_cpi = engine.params.max_price_move_bps_per_slot;
        if max_change_bps_cpi > 0 {
            let clamped_exec = oracle::clamp_oracle_price(
                crate::policy::mark_ewma_clamp_base(config.last_effective_price_e6),
                ret.exec_price_e6,
                max_change_bps_cpi,
            );
            let fee_paid_cpi = if config.mark_min_fee > 0 {
                let ins_after_cpi = engine.insurance_fund.balance.get();
                let delta = ins_after_cpi
                    .saturating_sub(ins_before_cpi)
                    .min(current_fee_paid_cap);
                core::cmp::min(delta, u64::MAX as u128) as u64
            } else {
                0u64
            };
            let ewma_price_cpi = if old_ewma_cpi == 0 && config.last_effective_price_e6 > 0 {
                config.last_effective_price_e6
            } else {
                clamped_exec
            };
            config.mark_ewma_e6 = crate::policy::ewma_update(
                old_ewma_cpi,
                ewma_price_cpi,
                config.mark_ewma_halflife_slots,
                config.mark_ewma_last_slot,
                clock.slot,
                fee_paid_cpi,
                config.mark_min_fee,
            );
            let full_weight_observation =
                config.mark_min_fee == 0 || fee_paid_cpi >= config.mark_min_fee;
            if full_weight_observation {
                config.mark_ewma_last_slot = clock.slot;
            }
        }

        if is_hyperp {
            config.hyperp_mark_e6 = oracle::clamp_oracle_price(
                config.last_effective_price_e6,
                ret.exec_price_e6,
                max_change_bps_cpi,
            );
            let fee_paid_hyperp = if config.mark_min_fee > 0 {
                let ins_after_cpi = engine.insurance_fund.balance.get();
                let delta = ins_after_cpi
                    .saturating_sub(ins_before_cpi)
                    .min(current_fee_paid_cap);
                core::cmp::min(delta, u64::MAX as u128) as u64
            } else {
                0u64
            };
            let full_weight = config.mark_min_fee == 0 || fee_paid_hyperp >= config.mark_min_fee;
            if full_weight {
                config.last_mark_push_slot = clock.slot as u128;
            }
        }
    }

    // Post-trade positions for the risk buffer.
    let (user_eff_cpi, lp_eff_cpi) = {
        let data = state::slab_data(&ctx.accounts.slab);
        let engine = zc::engine_ref(data)?;
        (
            effective_pos_q_checked(engine, user_idx as usize)?,
            effective_pos_q_checked(engine, lp_idx as usize)?,
        )
    };

    {
        let data = state::slab_data_mut(&mut ctx.accounts.slab);
        state::write_req_nonce(data, req_id);
        state::write_config(data, &config);
        if !state::is_oracle_initialized(data) {
            state::set_oracle_initialized(data);
        }
        let mut buf = state::read_risk_buffer(data);
        for &(idx, eff) in &[(user_idx, user_eff_cpi), (lp_idx, lp_eff_cpi)] {
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
