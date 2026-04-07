//! Instruction tag constants for percolator-launch.
//!
//! This file is the single source of truth for instruction numbering.
//! Any CPI caller (percolator-stake, indexers, keepers) MUST use these exact values.
//!
//! NEVER reorder, remove, or reuse a tag number.
//! Always append new instructions at the end.

// Tags 0-30: upstream instructions (sequential)
pub const TAG_INIT_MARKET: u8 = 0;
pub const TAG_INIT_USER: u8 = 1;
pub const TAG_INIT_LP: u8 = 2;
pub const TAG_DEPOSIT_COLLATERAL: u8 = 3;
pub const TAG_WITHDRAW_COLLATERAL: u8 = 4;
pub const TAG_KEEPER_CRANK: u8 = 5;
pub const TAG_TRADE_NO_CPI: u8 = 6;
pub const TAG_LIQUIDATE_AT_ORACLE: u8 = 7;
pub const TAG_CLOSE_ACCOUNT: u8 = 8;
pub const TAG_TOP_UP_INSURANCE: u8 = 9;
pub const TAG_TRADE_CPI: u8 = 10;
pub const TAG_UPDATE_ADMIN: u8 = 11;
pub const TAG_CLOSE_SLAB: u8 = 12;
pub const TAG_UPDATE_CONFIG: u8 = 13;
pub const TAG_SET_ORACLE_AUTHORITY: u8 = 14;
pub const TAG_PUSH_ORACLE_PRICE: u8 = 15;
pub const TAG_SET_ORACLE_PRICE_CAP: u8 = 16;
pub const TAG_RESOLVE_MARKET: u8 = 17;
pub const TAG_WITHDRAW_INSURANCE: u8 = 18;
pub const TAG_SET_INSURANCE_WITHDRAW_POLICY: u8 = 19;
pub const TAG_WITHDRAW_INSURANCE_LIMITED: u8 = 20;
pub const TAG_ADMIN_FORCE_CLOSE_ACCOUNT: u8 = 21;
pub const TAG_QUERY_LP_FEES: u8 = 22;
pub const TAG_RECLAIM_EMPTY_ACCOUNT: u8 = 23;
pub const TAG_SETTLE_ACCOUNT: u8 = 24;
pub const TAG_DEPOSIT_FEE_CREDITS: u8 = 25;
pub const TAG_CONVERT_RELEASED_PNL: u8 = 26;
pub const TAG_RESOLVE_PERMISSIONLESS: u8 = 27;
pub const TAG_FORCE_CLOSE_RESOLVED: u8 = 28;
// Note: upstream uses tags 29/30 for additional instructions if any exist

/// PERC-623: Top up keeper fund (permissionless).
pub const TAG_TOPUP_KEEPER_FUND: u8 = 57;

/// PERC-8400: Rescue orphan vault.
pub const TAG_RESCUE_ORPHAN_VAULT: u8 = 72;

/// PERC-8400: Close orphan slab.
pub const TAG_CLOSE_ORPHAN_SLAB: u8 = 73;

/// PERC-SetDexPool: Pin admin-approved DEX pool address for a HYPERP market.
pub const TAG_SET_DEX_POOL: u8 = 74;

/// Initialize matcher context via CPI to matcher program.
pub const TAG_INIT_MATCHER_CTX: u8 = 75;
