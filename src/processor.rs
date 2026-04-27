//! Engine-side processor helpers — funding/accrual, fee sync, oracle
//! reads, account-validity checks. Verbatim port of the legacy
//! `mod processor`'s helper section, EXCLUDING:
//!   - duplicates of `crate::guards` (slab_shape_guard, slab_guard,
//!     require_initialized, require_admin) — single source of truth
//!     stays in `guards.rs`;
//!   - SPL token-account verifiers (verify_vault, verify_vault_empty,
//!     verify_token_account, verify_token_program) — defer to Phase 3
//!     `cpi.rs` alongside the SPL transfer helpers;
//!   - `handle_update_authority` + the `AUTHORITY_*` constant block —
//!     ported to `instructions/update_authority.rs`;
//!   - `execute_trade_with_matcher` — defer to Phase 3 (matcher CPI).
//!
//! Anchor v2 mechanical adjustments mirror those in `oracle.rs`.

#![allow(unused_imports, dead_code, unused_variables, unused_parens, unused_braces)]

use crate::constants::{
    DEFAULT_FUNDING_HORIZON_SLOTS, DEFAULT_FUNDING_K_BPS, DEFAULT_FUNDING_MAX_E9_PER_SLOT,
    DEFAULT_FUNDING_MAX_PREMIUM_BPS, DEFAULT_MARK_EWMA_HALFLIFE_SLOTS, MAGIC, MATCHER_CALL_LEN,
    MATCHER_CALL_TAG, MAX_MATCHER_TAIL_ACCOUNTS, SLAB_LEN,
};
use crate::errors::{map_risk_error, PercolatorError};
use crate::oracle;
use crate::state::{self, MarketConfig, SlabHeader};
use crate::zc;
use percolator::{RiskEngine, RiskError, MAX_ACCOUNTS};
use pinocchio::account::AccountView;
use pinocchio::address::Address;
use pinocchio::sysvars::{clock::Clock, Sysvar};
use solana_program_error::ProgramError;


// settle_and_close_resolved removed — replaced by engine.force_close_resolved_not_atomic()
// which handles K-pair PnL, checked arithmetic, and all settlement internally.

/// Read oracle price for non-Hyperp markets and stamp
/// `last_good_oracle_slot`. Any Pyth/Chainlink parse error propagates
/// unchanged — there is no authority fallback.
///
/// STRICT HARD-TIMEOUT GATE: if the hard stale window has matured
/// (clock.slot - last_good_oracle_slot >=
/// permissionless_resolve_stale_slots), this function rejects with
/// OracleStale even when a fresh external price is supplied. That
/// prevents price-taking instructions (Trade, Withdraw, Crank,
/// Settle, Convert, Catchup) from reviving a terminally dead market
/// — they must route to ResolvePermissionless instead.
pub fn read_price_and_stamp(
    config: &mut crate::state::MarketConfig,
    a_oracle: &AccountView,
    clock_unix_ts: i64,
    clock_slot: u64,
    slab_data: &mut [u8],
) -> Result<u64, ProgramError> {
    if oracle::permissionless_stale_matured(config, clock_slot) {
        return Err(PercolatorError::OracleStale.into());
    }

    // Source the per-slot price-move cap from RiskParams (init-
    // immutable per spec §1.4 solvency envelope). Standard bps.
    let (p_last, max_change_bps, price_move_dt_slots, oi_any) = {
        let engine = zc::engine_ref(slab_data)?;
        (
            engine.last_oracle_price,
            engine.params.max_price_move_bps_per_slot,
            price_move_residual_dt(engine, clock_slot)?,
            engine.oi_eff_long_q != 0 || engine.oi_eff_short_q != 0,
        )
    };

    let external = oracle::read_engine_price_e6(
        a_oracle,
        &config.index_feed_id,
        clock_unix_ts,
        config.max_staleness_secs,
        config.conf_filter_bps,
        config.invert,
        config.unit_scale,
    );
    // Snapshot the source-feed clock before the call so we can
    // tell whether THIS read advanced state. Stale/duplicate
    // observations get the cached price from
    // `clamp_external_price` without advancing the timestamp; we
    // must not stamp the liveness cursor on those — otherwise an
    // attacker can replay an old Pyth account to extend market
    // life past `permissionless_resolve_stale_slots`.
    let prev_publish_time = config.oracle_target_publish_time;
    let price = oracle::clamp_external_price(
        config,
        external,
        p_last,
        max_change_bps,
        price_move_dt_slots,
        oi_any,
    )?;
    if config.oracle_target_publish_time > prev_publish_time {
        config.last_good_oracle_slot = clock_slot;
    }
    Ok(price)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradeExecution {
    /// Actual execution price (may differ from oracle/requested price)
    pub price: u64,
    /// Actual executed size (may be partial fill)
    pub size: i128,
}

/// Trait for pluggable matching engines
pub trait MatchingEngine {
    fn execute_match(
        &self,
        lp_program: &[u8; 32],
        lp_context: &[u8; 32],
        lp_account_id: u64,
        oracle_price: u64,
        size: i128,
    ) -> Result<TradeExecution, RiskError>;
}

/// No-op matching engine (for testing/TradeNoCpi)
pub struct NoOpMatcher;

impl MatchingEngine for NoOpMatcher {
    fn execute_match(
        &self,
        _lp_program: &[u8; 32],
        _lp_context: &[u8; 32],
        _lp_account_id: u64,
        oracle_price: u64,
        size: i128,
    ) -> Result<TradeExecution, RiskError> {
        Ok(TradeExecution {
            price: oracle_price,
            size,
        })
    }
}

struct CpiMatcher {
    exec_price: u64,
    exec_size: i128,
}

impl MatchingEngine for CpiMatcher {
    fn execute_match(
        &self,
        _lp_program: &[u8; 32],
        _lp_context: &[u8; 32],
        _lp_account_id: u64,
        _oracle_price: u64,
        _size: i128,
    ) -> Result<TradeExecution, RiskError> {
        Ok(TradeExecution {
            price: self.exec_price,
            size: self.exec_size,
        })
    }
}

/// Compute funding rate from mark-index premium (all market types).
/// Uses trade-derived EWMA mark vs oracle index.
/// Returns 0 if no trades yet (mark_ewma == 0) or params unset.
/// Compute funding rate in e9-per-slot (ppb) directly.
/// Avoids bps quantization: sub-bps rates are preserved as nonzero ppb values.
/// Realize due maintenance fees for a single account up to `now_slot`.
/// Idempotent: the engine's per-account `last_fee_slot` cursor prevents
/// double-charging over the same interval, and a call at the same anchor
/// as the cursor is a no-op (engine v12.18.4 §4.6.1).
///
/// Wrappers MUST call this before any health-sensitive engine operation
/// on the acting account when `maintenance_fee_per_slot > 0`, so that
/// the margin / withdrawal / close check sees post-fee capital. Between
/// cranks, each acting account self-realizes its share via this call;
/// KeeperCrank sweeps the rest.
///
/// No-op when `maintenance_fee_per_slot == 0`.
///
/// Invariant: capital-sensitive operations MUST fully accrue the
/// market (advance `last_market_slot` to `now_slot`) before syncing
/// per-account fees. Oracle-backed paths satisfy this via
/// `ensure_market_accrued_to_now` upstream. No-oracle paths (Deposit,
/// DepositFeeCredits, InitUser, InitLP, TopUpInsurance,
/// ReclaimEmptyAccount) cannot advance `last_market_slot` (no price /
/// rate available), so they MUST pass an anchor that is already
/// accrued — use `sync_account_fee_bounded_to_market` below rather
/// than calling this helper with a wall-clock slot.
///
/// Calling this with `now_slot > engine.last_market_slot` creates a
/// `current_slot > last_market_slot` split that later breaks the
/// accrual envelope: the next oracle-backed instruction will see an
/// inflated `clock.slot - last_market_slot` dt and fail Overflow.
pub fn sync_account_fee(
    engine: &mut RiskEngine,
    config: &MarketConfig,
    idx: u16,
    now_slot: u64,
) -> Result<(), ProgramError> {
    if config.maintenance_fee_per_slot == 0 {
        return Ok(());
    }
    engine
        .sync_account_fee_to_slot_not_atomic(idx, now_slot, config.maintenance_fee_per_slot)
        .map_err(map_risk_error)
}

/// Fee-sync variant for no-oracle instructions. Caps the fee anchor
/// at `engine.last_market_slot`, leaving full realization of fees
/// accrued over `[last_market_slot, clock.slot]` to the next
/// oracle-backed instruction. Prevents the `current_slot >
/// last_market_slot` split that would otherwise brick later
/// accrual.
///
/// Acceptable trade-off: fees from the unaccrued tail are realized
/// slightly later (on the next trade/crank/withdraw) instead of now.
/// Correctness is preserved because the engine's per-account
/// `last_fee_slot` still advances monotonically to the
/// already-accrued boundary; subsequent sync calls cover the rest.
pub fn sync_account_fee_bounded_to_market(
    engine: &mut RiskEngine,
    config: &MarketConfig,
    idx: u16,
    wallclock_slot: u64,
) -> Result<(), ProgramError> {
    if config.maintenance_fee_per_slot == 0 {
        return Ok(());
    }
    check_idx(engine, idx)?;
    let anchor = core::cmp::min(wallclock_slot, engine.last_market_slot);
    // No-oracle paths must not move fee time beyond the market's
    // accrued slot. If engine.current_slot or this account's fee
    // cursor is already past that boundary, there is nothing safe
    // to realize here; the next oracle-backed op will accrue the
    // market and cover the tail.
    if anchor < engine.current_slot || anchor < engine.accounts[idx as usize].last_fee_slot {
        return Ok(());
    }
    engine
        .sync_account_fee_to_slot_not_atomic(idx, anchor, config.maintenance_fee_per_slot)
        .map_err(map_risk_error)
}

pub fn check_no_oracle_live_envelope(
    engine: &RiskEngine,
    wallclock_slot: u64,
) -> Result<(), ProgramError> {
    let gap = wallclock_slot
        .checked_sub(engine.last_market_slot)
        .ok_or(PercolatorError::EngineOverflow)?;
    let oi_any = engine.oi_eff_long_q != 0 || engine.oi_eff_short_q != 0;
    if oi_any && gap > engine.params.max_accrual_dt_slots {
        return Err(PercolatorError::CatchupRequired.into());
    }
    Ok(())
}

pub fn price_move_residual_dt(
    engine: &RiskEngine,
    wallclock_slot: u64,
) -> Result<u64, ProgramError> {
    price_move_residual_dt_from_parts(
        engine.last_market_slot,
        engine.params.max_accrual_dt_slots,
        wallclock_slot,
    )
}

pub fn price_move_residual_dt_from_parts(
    last_market_slot: u64,
    max_dt: u64,
    wallclock_slot: u64,
) -> Result<u64, ProgramError> {
    let gap = wallclock_slot
        .checked_sub(last_market_slot)
        .ok_or(PercolatorError::EngineOverflow)?;
    if max_dt == 0 || gap <= max_dt {
        return Ok(gap);
    }
    let rem = gap % max_dt;
    Ok(if rem == 0 { max_dt } else { rem })
}

pub fn external_oracle_target_pending(config: &MarketConfig, engine: &RiskEngine) -> bool {
    !oracle::is_hyperp_mode(config)
        && config.oracle_target_price_e6 != 0
        && config.oracle_target_price_e6 != engine.last_oracle_price
}

pub fn hyperp_target_price(config: &MarketConfig) -> u64 {
    if config.mark_ewma_e6 > 0 {
        config.mark_ewma_e6
    } else {
        config.hyperp_mark_e6
    }
}

pub fn oracle_target_pending(config: &MarketConfig, engine: &RiskEngine) -> bool {
    if oracle::is_hyperp_mode(config) {
        let target = hyperp_target_price(config);
        target != 0 && target != engine.last_oracle_price
    } else {
        external_oracle_target_pending(config, engine)
    }
}

pub fn reject_any_target_lag(
    config: &MarketConfig,
    engine: &RiskEngine,
) -> Result<(), ProgramError> {
    if oracle_target_pending(config, engine) {
        return Err(PercolatorError::CatchupRequired.into());
    }
    Ok(())
}

pub fn target_lag_after_read(config: &MarketConfig, effective_price: u64) -> bool {
    if oracle::is_hyperp_mode(config) {
        let target = hyperp_target_price(config);
        target != 0 && target != effective_price
    } else {
        config.oracle_target_price_e6 != 0 && config.oracle_target_price_e6 != effective_price
    }
}

pub fn effective_pos_q_checked(engine: &RiskEngine, idx: usize) -> Result<i128, ProgramError> {
    engine
        .try_effective_pos_q(idx)
        .map_err(|_| PercolatorError::EngineCorruptState.into())
}

pub fn risk_notional_ceil(eff: i128, price: u64) -> u128 {
    percolator::wide_math::mul_div_ceil_u128(
        eff.unsigned_abs(),
        price as u128,
        percolator::POS_SCALE,
    )
}

pub fn current_trade_fee_paid_cap(
    size: i128,
    exec_price: u64,
    trading_fee_bps: u64,
) -> Result<u128, ProgramError> {
    if trading_fee_bps == 0 || size == 0 {
        return Ok(0);
    }
    let notional = percolator::wide_math::mul_div_floor_u128(
        size.unsigned_abs(),
        exec_price as u128,
        percolator::POS_SCALE,
    );
    if notional == 0 {
        return Ok(0);
    }
    let one_side_fee =
        percolator::wide_math::mul_div_ceil_u128(notional, trading_fee_bps as u128, 10_000);
    one_side_fee
        .checked_mul(2)
        .ok_or_else(|| PercolatorError::EngineOverflow.into())
}

pub fn reject_stuck_target_accrual(
    config: &MarketConfig,
    engine: &RiskEngine,
    now_slot: u64,
    price: u64,
) -> Result<(), ProgramError> {
    let dt = now_slot
        .checked_sub(engine.last_market_slot)
        .ok_or(PercolatorError::EngineOverflow)?;
    let oi_any = engine.oi_eff_long_q != 0 || engine.oi_eff_short_q != 0;
    if dt > 0
        && oi_any
        && oracle_target_pending(config, engine)
        && price == engine.last_oracle_price
    {
        return Err(PercolatorError::CatchupRequired.into());
    }
    Ok(())
}

pub fn prepare_lazy_free_head(engine: &mut RiskEngine) -> Result<u16, ProgramError> {
    let max_accounts = core::cmp::min(
        engine.params.max_accounts as usize,
        percolator::MAX_ACCOUNTS,
    );
    let idx = engine.free_head;
    if idx == u16::MAX || (idx as usize) >= max_accounts || engine.is_used(idx as usize) {
        return Err(PercolatorError::EngineOverflow.into());
    }

    let i = idx as usize;
    let valid_head = engine.prev_free[i] == u16::MAX
        && (engine.next_free[i] == u16::MAX
            || ((engine.next_free[i] as usize) < max_accounts
                && !engine.is_used(engine.next_free[i] as usize)
                && engine.prev_free[engine.next_free[i] as usize] == idx));
    if !valid_head {
        if idx as u64 != engine.num_used_accounts as u64 {
            return Err(PercolatorError::EngineOverflow.into());
        }
        let next = if i + 1 < max_accounts {
            (i + 1) as u16
        } else {
            u16::MAX
        };
        engine.prev_free[i] = u16::MAX;
        engine.next_free[i] = next;
        if next != u16::MAX {
            engine.prev_free[next as usize] = idx;
        }
    }

    Ok(idx)
}

/// Maximum number of max_dt chunks the in-line catchup can advance per
/// instruction. Bounded by CU budget — each `accrue_market_to` is cheap
/// but not free. For gaps beyond this, callers must use the dedicated
/// `CatchupAccrue` instruction which commits progress atomically
/// without attempting a main operation afterwards.
///
/// 20 × max_dt = 20 × 100 = 2_000 slots per single instruction. Larger
/// gaps require multiple CatchupAccrue calls — that's the design
/// contract, not a misconfig.
pub const CATCHUP_CHUNKS_MAX: u32 = 20;

/// Pre-chunk market-clock advancement when the gap since the last
/// engine *accrue* exceeds `params.max_accrual_dt_slots`. The engine
/// rejects any single `accrue_market_to` whose funding-active dt
/// exceeds the envelope (spec §1.4 / §5.5 clause 6), so every
/// accrue-bearing instruction (KeeperCrank, TradeCpi, TradeNoCpi,
/// Withdraw, Liquidate, Close, Settle, Convert, live Insurance
/// withdraw, Ordinary ResolveMarket, UpdateConfig) must close that
/// gap before its own accrue.
///
/// Cursor: loops on `engine.last_market_slot`, NOT `current_slot`.
/// `last_market_slot` is the only cursor `accrue_market_to` uses to
/// compute `total_dt = now_slot - last_market_slot`; `current_slot`
/// can be advanced by non-accruing public endpoints (fee sync on Live,
/// deposit/top-up without oracle) so it does not track market accrual.
/// Earlier versions chunked from `current_slot`, which after any
/// no-oracle self-advance would under-report the real gap and let the
/// caller's own `accrue_market_to` hit Overflow on the residual.
///
/// Caller supplies the catchup price and funding rate. Typical usage:
/// the pre-oracle-read funding rate (`funding_rate_e9_pre`) and the
/// fresh (or about-to-be-set) `oracle_price`. Using the caller-supplied
/// rate (not 0) preserves anti-retroactivity — the rate reflects the
/// mark/index state as it was before this instruction, not what the
/// idle interval "should have" been (which is unknowable).
///
/// If the gap exceeds `CATCHUP_CHUNKS_MAX × max_dt`, returns `Err`
/// with `CatchupRequired` so the caller can surface "call CatchupAccrue
/// first" instead of silently returning Ok and letting the subsequent
/// main engine call Overflow-and-rollback (which would discard the
/// catchup progress too, making the market unrecoverable in-line).
///
/// No-op when the gap is already within the envelope, or when
/// `max_dt == 0` (misconfiguration guard), or when the engine has never
/// seen a real oracle observation (`last_oracle_price == 0`; the
/// caller's own `_not_atomic` call will seed it).
pub fn catchup_accrue(
    engine: &mut RiskEngine,
    now_slot: u64,
    price: u64,
    funding_rate_e9: i128,
) -> Result<(), ProgramError> {
    let max_dt = engine.params.max_accrual_dt_slots;
    if max_dt == 0 {
        return Ok(());
    }
    if now_slot <= engine.last_market_slot {
        return Ok(());
    }
    // Market never had a real oracle observation — nothing to catch up.
    // The caller's own _not_atomic call will seed last_oracle_price.
    if engine.last_oracle_price == 0 {
        return Ok(());
    }
    // Mirror the engine's own envelope predicate (§5.5 clause 6, v12.19):
    // accrue_market_to rejects `total_dt > max_dt` when EITHER funding
    // or price movement would drain equity:
    //
    //   funding_active    = rate != 0 AND both OI sides live AND fund_px_last > 0
    //   price_move_active = P_last > 0 AND oracle_price != P_last AND any OI live
    //
    // Prior versions chunked only on `funding_active`. A zero-funding
    // market with live OI and a fresh oracle price different from
    // P_last would then skip catchup, and the caller's final
    // `accrue_market_to(now, fresh, rate)` would itself trip the
    // envelope (and/or the §5.5 step-9 per-slot price-move cap) and
    // make the market unrecoverable in-line.
    //
    // Do not invent intermediate oracle prices. If the clock gap is too
    // large, catch up time using stored P_last only, then let the final
    // real observation pass or fail the engine's dt-scaled price cap.
    let oi_any = engine.oi_eff_long_q != 0 || engine.oi_eff_short_q != 0;
    let funding_active = funding_rate_e9 != 0
        && engine.oi_eff_long_q != 0
        && engine.oi_eff_short_q != 0
        && engine.fund_px_last > 0;
    let price_move_active =
        engine.last_oracle_price > 0 && price != engine.last_oracle_price && oi_any;
    if !funding_active && !price_move_active {
        // Neither accrual driver is active — the engine's envelope
        // predicate will permit a single-call jump. Caller's final
        // accrue_market_to handles it in one shot.
        return Ok(());
    }
    let cap_bps = engine.params.max_price_move_bps_per_slot;
    let mut chunks: u32 = 0;
    while now_slot.saturating_sub(engine.last_market_slot) > max_dt {
        if chunks >= CATCHUP_CHUNKS_MAX {
            // Silently returning Ok here would let the caller's
            // main accrue hit Overflow on the residual, rolling
            // back ALL catchup progress. Surface CatchupRequired
            // so the caller routes to the dedicated CatchupAccrue
            // instruction which commits progress without attempting
            // the main op.
            return Err(PercolatorError::CatchupRequired.into());
        }
        let chunk_dt = max_dt;
        let step_slot = engine.last_market_slot.saturating_add(chunk_dt);
        let prev_price = engine.last_oracle_price;
        engine
            .accrue_market_to(step_slot, prev_price, funding_rate_e9)
            .map_err(map_risk_error)?;
        chunks = chunks.saturating_add(1);
    }
    if price_move_active {
        let remaining = now_slot.saturating_sub(engine.last_market_slot);
        let prev = engine.last_oracle_price;
        let abs_delta = if price >= prev {
            price - prev
        } else {
            prev - price
        };
        if abs_delta != 0 {
            if remaining == 0 {
                return Err(PercolatorError::OracleInvalid.into());
            }
            let lhs = (abs_delta as u128).saturating_mul(10_000u128);
            let rhs = (cap_bps as u128)
                .saturating_mul(remaining as u128)
                .saturating_mul(prev as u128);
            if lhs > rhs {
                return Err(PercolatorError::OracleInvalid.into());
            }
        }
    }
    Ok(())
}

/// Fully advance the engine's market clock to `now_slot` before any
/// per-account fee sync. This is an explicit-ordering helper:
/// `catchup_accrue` brings the gap within the envelope, then a final
/// `accrue_market_to(now_slot)` closes the residual so subsequent
/// `sync_account_fee_to_slot_not_atomic(..., now_slot, ...)` runs
/// against a fully-accrued market.
///
/// Why explicit, when the engine already self-handles it via the main
/// op's internal accrue? Because even though the engine uses
/// `last_market_slot` (not `current_slot`) for funding dt — so the
/// interval is never erased (see
/// `test_fee_sync_does_not_erase_market_accrual_interval`) — making
/// the ordering explicit in the wrapper removes all ambiguity and
/// aligns with the auditor-requested pattern:
/// `ensure_market_accrued_to_now; sync_account_fee; engine.<op>_not_atomic`.
///
/// The main op's internal `accrue_market_to(now_slot, price, rate)`
/// then hits the same-slot + same-price no-op branch (engine §5.4
/// early return) — about 150 CU of redundancy, bought for ordering
/// clarity.
///
/// No-op when the engine has no oracle observation yet (price=0
/// catchup is unsafe). Same-slot price replacement is allowed only
/// for flat markets; live OI still requires elapsed slot budget.
pub fn ensure_market_accrued_to_now(
    engine: &mut RiskEngine,
    now_slot: u64,
    price: u64,
    funding_rate_e9: i128,
) -> Result<(), ProgramError> {
    catchup_accrue(engine, now_slot, price, funding_rate_e9)?;
    let flat_same_slot_price_update = price > 0
        && now_slot == engine.last_market_slot
        && price != engine.last_oracle_price
        && engine.oi_eff_long_q == 0
        && engine.oi_eff_short_q == 0;
    if price > 0 && (now_slot > engine.last_market_slot || flat_same_slot_price_update) {
        engine
            .accrue_market_to(now_slot, price, funding_rate_e9)
            .map_err(map_risk_error)?;
    }
    Ok(())
}

pub fn ensure_market_accrued_to_now_with_policy(
    engine: &mut RiskEngine,
    config: &MarketConfig,
    now_slot: u64,
    price: u64,
    funding_rate_e9: i128,
) -> Result<(), ProgramError> {
    reject_stuck_target_accrual(config, engine, now_slot, price)?;
    ensure_market_accrued_to_now(engine, now_slot, price, funding_rate_e9)
}

/// Incrementally sweep maintenance fees from the current cursor position.
/// Scans bitmap words starting at `(fee_sweep_cursor_word,
/// fee_sweep_cursor_bit)`, calling `sync_account_fee_to_slot_not_atomic`
/// on every set bit. Stops EXACTLY at `FEE_SWEEP_BUDGET` syncs — the bit
/// cursor lets us pause mid-word without losing remaining set bits to
/// budget truncation.
///
/// Correctness: the engine's per-account `last_fee_slot` is the source of
/// truth. When the cursor reaches an account, that account's sync call
/// realizes fees for the *entire* elapsed interval
/// `[account.last_fee_slot, now_slot]` in one charge — no fees are lost
/// between cursor visits. Self-acting accounts realize their own fees
/// inline on every capital-sensitive instruction (see `sync_account_fee`);
/// the sweep handles everything that hasn't self-acted.
///
/// CU bound: at most `FEE_SWEEP_BUDGET` sync calls per crank (strictly,
/// thanks to the bit cursor), plus O(BITMAP_WORDS) word reads. Constant
/// in `max_accounts`, so a 4096-slot market is handled the same as a
/// 64-slot market.
pub fn sweep_maintenance_fees(
    engine: &mut RiskEngine,
    config: &mut MarketConfig,
    now_slot: u64,
    max_syncs: usize,
) -> Result<(), ProgramError> {
    if config.maintenance_fee_per_slot == 0 {
        return Ok(());
    }
    // Early-out when the caller has already exhausted the per-
    // instruction sync budget on pre-sweep candidate syncs.
    if max_syncs == 0 {
        return Ok(());
    }
    const BITMAP_WORDS: usize = (percolator::MAX_ACCOUNTS + 63) / 64;
    // Normalize cursor in case of stale/corrupt values.
    let mut word_cursor = (config.fee_sweep_cursor_word as usize) % BITMAP_WORDS;
    let mut bit_cursor = (config.fee_sweep_cursor_bit as usize) & 63;
    let mut syncs_done: usize = 0;
    let mut words_scanned: usize = 0;
    // Budget check is inside the inner loop so we can stop exactly at
    // max_syncs, not after completing the current word.
    'outer: while words_scanned < BITMAP_WORDS {
        // Skip bits below bit_cursor on the resume word.
        let resume_mask = if bit_cursor == 0 {
            u64::MAX
        } else {
            // Clear bits 0..bit_cursor (they were already processed last call).
            !((1u64 << bit_cursor).wrapping_sub(1))
        };
        let mut bits = engine.used[word_cursor] & resume_mask;
        while bits != 0 {
            if syncs_done >= max_syncs {
                // Stop EXACTLY at budget. Save the next unprocessed bit
                // as the resume point for the following crank.
                let next_bit = bits.trailing_zeros() as usize;
                config.fee_sweep_cursor_word = word_cursor as u64;
                config.fee_sweep_cursor_bit = next_bit as u64;
                return Ok(());
            }
            let bit = bits.trailing_zeros() as usize;
            bits &= bits - 1;
            let idx = word_cursor * 64 + bit;
            if !idx_within_market_capacity(engine, idx) {
                continue;
            }
            engine
                .sync_account_fee_to_slot_not_atomic(
                    idx as u16,
                    now_slot,
                    config.maintenance_fee_per_slot,
                )
                .map_err(map_risk_error)?;
            syncs_done += 1;

            // Permissionless dust reclaim: fee accrual just charged
            // this account; if that drained capital to zero on a
            // flat account (no position, no PnL, no reserve, no
            // pending, no positive fee_credits), free the slot now.
            // Without this, an attacker could fill `max_accounts`
            // with dust and brick onboarding even when fees drain
            // capital, because slot reclamation would still require
            // an explicit per-account `ReclaimEmptyAccount` call.
            //
            // All six flat-clean predicates the engine's reclaim
            // checks are mirrored here so the call CANNOT hit an
            // `Undercollateralized` / `CorruptState` early return.
            // That lets us propagate any remaining error with `?`
            // rather than silently swallowing a `_not_atomic`
            // failure — per the engine contract, a failing
            // `_not_atomic` may have already mutated state and the
            // caller must abort the transaction. Envelope /
            // market-mode guards upstream (KeeperCrank's oracle
            // read + is_resolved gate + accrue_market_to) ensure
            // the remaining engine preconditions hold, so in
            // practice the `?` is unreachable — but if a future
            // engine change introduces a new precondition, we get
            // a transaction rollback instead of silent corruption.
            let acc = &engine.accounts[idx];
            let fee_credits = acc.fee_credits.get();
            if acc.capital.is_zero()
                && acc.position_basis_q == 0
                && acc.pnl == 0
                && acc.reserved_pnl == 0
                && acc.sched_present == 0
                && acc.pending_present == 0
                && fee_credits <= 0
            {
                if fee_credits == i128::MIN {
                    return Err(PercolatorError::EngineCorruptState.into());
                }
                engine
                    .reclaim_empty_account_not_atomic(idx as u16, now_slot)
                    .map_err(map_risk_error)?;
            }
        }
        // Word fully drained — advance to next word, reset bit cursor.
        word_cursor = (word_cursor + 1) % BITMAP_WORDS;
        bit_cursor = 0;
        words_scanned += 1;
        // Budget may have hit right at the end of the word — avoid one
        // wasted iteration on the next (empty in the caller's view) word.
        if syncs_done >= max_syncs {
            break 'outer;
        }
    }
    config.fee_sweep_cursor_word = word_cursor as u64;
    config.fee_sweep_cursor_bit = 0;
    Ok(())
}

pub fn compute_current_funding_rate_e9(config: &MarketConfig) -> Result<i128, ProgramError> {
    if config.funding_max_premium_bps < 0 || config.funding_max_e9_per_slot < 0 {
        return Err(PercolatorError::InvalidConfigParam.into());
    }

    let mark = config.mark_ewma_e6;
    let index = config.last_effective_price_e6;
    if mark == 0 || index == 0 || config.funding_horizon_slots == 0 {
        return Ok(0);
    }

    let diff = mark as i128 - index as i128;
    // premium in e9: diff * 1_000_000_000 / index
    let mut premium_e9 = diff.saturating_mul(1_000_000_000) / (index as i128);

    // Clamp premium: max_premium_bps * 100_000 converts bps to e9
    let max_prem_e9 = (config.funding_max_premium_bps as i128) * 100_000;
    premium_e9 = premium_e9.clamp(-max_prem_e9, max_prem_e9);

    // Apply k multiplier (100 = 1.00x)
    let scaled = premium_e9.saturating_mul(config.funding_k_bps as i128) / 100;

    // Per-slot: divide by horizon
    let per_slot = scaled / (config.funding_horizon_slots as i128);

    // Clamp: funding_max_e9_per_slot is already in engine-native e9 units.
    let max_rate_e9 = config.funding_max_e9_per_slot as i128;
    Ok(per_slot.clamp(-max_rate_e9, max_rate_e9))
}

pub fn execute_trade_with_matcher<M: MatchingEngine>(
    engine: &mut RiskEngine,
    matcher: &M,
    lp_idx: u16,
    user_idx: u16,
    now_slot: u64,
    oracle_price: u64,
    size: i128,
    funding_rate_e9: i128,
    lp_account_id: u64,
    maintenance_fee_per_slot: u128,
) -> Result<(), RiskError> {
    let lp = &engine.accounts[lp_idx as usize];
    let exec = matcher.execute_match(
        &lp.matcher_program,
        &lp.matcher_context,
        lp_account_id,
        oracle_price,
        size,
    )?;
    // POS_SCALE = 1_000_000 in spec v11.5, same as instruction units.
    // No conversion needed.
    let size_q: i128 = exec.size;
    // Spec v12: size_q must be > 0. Account `a` buys from `b`.
    // Positive size = user buys from LP (user goes long).
    // Negative size = LP buys from user (user goes short) — swap order.
    let (a, b, abs_size) = if size_q > 0 {
        (user_idx, lp_idx, size_q)
    } else if size_q < 0 {
        // checked_neg rejects i128::MIN (which has no positive counterpart)
        let pos = size_q.checked_neg().ok_or(RiskError::Overflow)?;
        (lp_idx, user_idx, pos)
    } else {
        return Err(RiskError::Overflow);
    };
    let admit_h_min = engine.params.h_min;
    let admit_h_max = engine.params.h_max;
    // Realize due maintenance fees on both counterparties BEFORE the trade
    // so margin checks see post-fee capital. No-op when fee rate is 0.
    if maintenance_fee_per_slot > 0 {
        engine.sync_account_fee_to_slot_not_atomic(a, now_slot, maintenance_fee_per_slot)?;
        engine.sync_account_fee_to_slot_not_atomic(b, now_slot, maintenance_fee_per_slot)?;
    }
    let admit_threshold = Some(engine.params.maintenance_margin_bps as u128);
    engine.execute_trade_not_atomic(
        a,
        b,
        oracle_price,
        now_slot,
        abs_size,
        exec.price,
        funding_rate_e9,
        admit_h_min,
        admit_h_max,
        admit_threshold,
    )
}

// Legacy `use solana_program::instruction::{...}` dropped — matcher
// CPI plumbing belongs in Phase 3 `cpi.rs`.
#[inline]
pub fn idx_within_market_capacity(engine: &RiskEngine, idx: usize) -> bool {
    idx < MAX_ACCOUNTS && (idx as u64) < engine.params.max_accounts
}

#[inline]
pub fn idx_used_in_market(engine: &RiskEngine, idx: usize) -> bool {
    idx_within_market_capacity(engine, idx) && engine.is_used(idx)
}

pub fn check_idx(engine: &RiskEngine, idx: u16) -> Result<(), ProgramError> {
    if !idx_used_in_market(engine, idx as usize) {
        return Err(PercolatorError::EngineAccountNotFound.into());
    }
    Ok(())
}

#[inline]
pub fn insurance_withdraw_deposits_only(config: &MarketConfig) -> Result<bool, ProgramError> {
    match config.insurance_withdraw_deposits_only {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(PercolatorError::InvalidConfigParam.into()),
    }
}


