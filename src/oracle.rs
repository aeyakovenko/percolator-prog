//! Oracle helpers — minimal subset needed by the first batch of ported
//! instructions. The full Pyth/Chainlink decoders + price-clamp staircase
//! land alongside oracle-using instructions in Phase 4. Kept in one file
//! to avoid churning import paths when those handlers arrive.

use crate::state::MarketConfig;

/// Hyperp mode is active when `index_feed_id` is all zeros (no external
/// oracle; the market runs on PushHyperpMark + trade-derived EWMA).
#[inline]
pub fn is_hyperp_mode(config: &MarketConfig) -> bool {
    config.index_feed_id == [0u8; 32]
}

/// Pure restart-detection comparison the on-chain path uses after
/// reading the sysvar. Separated so proof harnesses can check it
/// symbolically without stubbing the syscall.
#[inline]
pub fn restart_detected(init_restart_slot: u64, current_last_restart_slot: u64) -> bool {
    current_last_restart_slot > init_restart_slot
}

/// On-chain restart check. Reads `LastRestartSlot::get()` and compares
/// against the slot captured at `InitMarket`.
#[cfg(not(feature = "kani"))]
#[inline]
pub fn cluster_restarted_since_init(config: &MarketConfig) -> bool {
    use solana_sysvar::last_restart_slot::LastRestartSlot;
    use solana_sysvar::Sysvar;
    match LastRestartSlot::get() {
        Ok(lrs) => restart_detected(config.init_restart_slot, lrs.last_restart_slot),
        Err(_) => false,
    }
}

#[cfg(feature = "kani")]
#[inline]
pub fn cluster_restarted_since_init(_config: &MarketConfig) -> bool {
    false
}

/// Hard-timeout predicate: has the market's configured oracle been
/// stale for `>= permissionless_resolve_stale_slots`? Returns false when
/// the feature is disabled (`permissionless_resolve_stale_slots == 0`).
///
/// Liveness slot is `last_good_oracle_slot` for non-Hyperp markets and
/// `last_mark_push_slot` for Hyperp markets — see the legacy doc comment
/// for the full rationale, particularly around why
/// `mark_ewma_last_slot` is *not* a liveness signal.
pub fn permissionless_stale_matured(config: &MarketConfig, clock_slot: u64) -> bool {
    if cluster_restarted_since_init(config) {
        return true;
    }
    if config.permissionless_resolve_stale_slots == 0 {
        return false;
    }
    let last_live_slot = if is_hyperp_mode(config) {
        config.last_mark_push_slot as u64
    } else {
        config.last_good_oracle_slot
    };
    clock_slot.saturating_sub(last_live_slot) >= config.permissionless_resolve_stale_slots
}
