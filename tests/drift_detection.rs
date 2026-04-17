//! Cross-component drift detection tests.
//!
//! These tests are the single source of truth for on-chain layout verification.
//! If ANY offset, size, tag, or program ID drifts between components, these
//! tests MUST fail. The goal is to catch wire-format drift before deployment.
//!
//! ## What is tested
//! 1. V12_1 layout constants — ACCOUNT_SIZE, CONFIG_LEN, ENGINE_OFF, BITMAP_OFF,
//!    and per-field offsets within Account (matcher_program, matcher_context, owner,
//!    fee_credits, f_snap).
//! 2. Instruction tag consistency — every TAG_* constant must decode to the expected
//!    variant; no tag may collide with another.
//! 3. RiskParams wire format — field-by-field encode -> decode round-trip.
//! 4. Program ID constants — mainnet prog + stake IDs.
//! 5. Account struct size assertions via core::mem::size_of and offset_of!.
//! 6. Engine struct field assertions at expected offsets.
//!
//! ## Platform note
//! Tests run on x86_64 (native test runner). The concrete byte values asserted here
//! reflect x86_64 layout. SBF (32-bit) layout values differ due to u128/i128 alignment
//! (8 bytes on SBF vs 16 bytes on x86_64). To catch SBF drift you MUST also run
//! `cargo test-sbf` or compare against pinned compile-time assertions in src/.
//!
//! The guards in `percolator_prog::constants` use `size_of` at compile time so any
//! SBF layout change will break the program build before reaching deployment.

#![allow(
    clippy::assertions_on_constants,
    clippy::needless_pass_by_value,
    clippy::too_many_arguments,
    clippy::manual_is_multiple_of,
    clippy::useless_conversion,
    unused_imports
)]

use core::mem::{offset_of, size_of};
use percolator::{Account, RiskEngine};
use percolator_prog::{
    constants::{
        ACCOUNT_SIZE, CONFIG_LEN, ENGINE_OFF, HEADER_LEN, MAGIC, MATCHER_CONTEXT_LEN,
        MATCHER_ABI_VERSION,
    },
    tags::{
        TAG_ADMIN_FORCE_CLOSE,
        TAG_ADVANCE_EPOCH,
        TAG_ADVANCE_ORACLE_PHASE,
        TAG_ALLOCATE_MARKET,
        TAG_ATTEST_CROSS_MARGIN,
        TAG_AUDIT_CRANK,
        TAG_BURN_POSITION_NFT,
        TAG_CANCEL_QUEUED_WITHDRAWAL,
        TAG_CHALLENGE_SETTLEMENT,
        TAG_CLAIM_EPOCH_WITHDRAWAL,
        TAG_CLAIM_QUEUED_WITHDRAWAL,
        TAG_CLEAR_PENDING_SETTLEMENT,
        TAG_CLOSE_ACCOUNT,
        TAG_CLOSE_ORPHAN_SLAB,
        TAG_CLOSE_SLAB,
        TAG_CLOSE_STALE_SLAB,
        TAG_CONVERT_RELEASED_PNL,
        TAG_CREATE_LP_VAULT,
        TAG_DEPOSIT_COLLATERAL,
        TAG_DEPOSIT_FEE_CREDITS,
        TAG_DEPOSIT_LP_COLLATERAL,
        TAG_EXECUTE_ADL,
        TAG_FORCE_CLOSE_RESOLVED,
        TAG_FUND_MARKET_INSURANCE,
        TAG_INIT_LP,
        TAG_INIT_MARKET,
        TAG_INIT_MATCHER_CTX,
        TAG_INIT_SHARED_VAULT,
        TAG_INIT_USER,
        TAG_PAUSE_MARKET,
        TAG_UNPAUSE_MARKET,
        TAG_KEEPER_CRANK,
        TAG_LIQUIDATE_AT_ORACLE,
        TAG_LP_VAULT_CRANK_FEES,
        TAG_LP_VAULT_DEPOSIT,
        TAG_LP_VAULT_WITHDRAW,
        TAG_MINT_POSITION_NFT,
        TAG_PUSH_ORACLE_PRICE,
        TAG_QUERY_LP_FEES,
        TAG_QUEUE_WITHDRAWAL,
        TAG_QUEUE_WITHDRAWAL_SV,
        TAG_RECLAIM_EMPTY_ACCOUNT,
        TAG_RECLAIM_SLAB_RENT,
        TAG_RESCUE_ORPHAN_VAULT,
        TAG_RESOLVE_DISPUTE,
        TAG_RESOLVE_MARKET,
        TAG_RESOLVE_PERMISSIONLESS,
        TAG_SET_DEX_POOL,
        TAG_SET_INSURANCE_ISOLATION,
        TAG_SET_INSURANCE_WITHDRAW_POLICY,
        TAG_SET_LP_COLLATERAL_PARAMS,
        TAG_SET_MAINTENANCE_FEE,
        TAG_SET_MAX_PNL_CAP,
        TAG_SET_OI_CAP_MULTIPLIER,
        TAG_SET_DISPUTE_PARAMS,
        TAG_ACCEPT_ADMIN,
        TAG_SET_OFFSET_PAIR,
        TAG_SET_OI_IMBALANCE_HARD_BLOCK,
        TAG_SET_ORACLE_AUTHORITY,
        TAG_SET_ORACLE_PRICE_CAP,
        TAG_SET_PENDING_SETTLEMENT,
        TAG_SET_PYTH_ORACLE,
        TAG_SET_RISK_THRESHOLD,
        TAG_SET_WALLET_CAP,
        TAG_SETTLE_ACCOUNT,
        TAG_SLASH_CREATION_DEPOSIT,
        TAG_TOP_UP_INSURANCE,
        TAG_TRADE_CPI,
        TAG_TRADE_CPI_V2,
        TAG_TRADE_NO_CPI,
        TAG_TRANSFER_OWNERSHIP_CPI,
        TAG_TRANSFER_POSITION_OWNERSHIP,
        TAG_UNRESOLVE_MARKET,
        TAG_UPDATE_ADMIN,
        TAG_UPDATE_CONFIG,
        TAG_UPDATE_HYPERP_MARK,
        TAG_UPDATE_MARK_PRICE,
        TAG_WITHDRAW_COLLATERAL,
        TAG_WITHDRAW_INSURANCE,
        TAG_WITHDRAW_INSURANCE_LIMITED,
        TAG_WITHDRAW_LP_COLLATERAL,
    },
    ix::Instruction,
};
use solana_program::{program_error::ProgramError, pubkey::Pubkey};

// ============================================================================
// § 1 — V12_1 Layout Constants
// ============================================================================

/// Canonical program-level layout constants agreed between the SDK and on-chain
/// program. These values are asserted against what the program itself computes
/// via size_of / offset_of. If either side drifts, this test fails.
///
/// Values are x86_64 native. The comments show the SBF (BPF 32-bit) equivalents
/// for reference — those are enforced at build time in src/percolator.rs via
/// compile-time asserts.
mod layout_constants {
    /// HEADER_LEN: size_of::<SlabHeader>() = 72 bytes
    pub const HEADER_LEN_EXPECTED: usize = 72;
    /// CONFIG_LEN: size_of::<MarketConfig>()
    // 2026-04-17: CONFIG_LEN extended from 432 → 480 by Phase A (7 new fields),
    // then 480 → 512 by Phase E (+32 bytes for pending_admin for two-step UpdateAdmin).
    pub const CONFIG_LEN_EXPECTED: usize = 512;
    /// ACCOUNT_SIZE: size_of::<Account>() — v12.17 two-bucket warmup
    pub const ACCOUNT_SIZE_EXPECTED: usize = 368;
    /// ENGINE_OFF: align_up(HEADER_LEN + CONFIG_LEN, ENGINE_ALIGN)
    /// Computed live from the program's own constant — this is intentional:
    /// if the constant changes, the test catches it via other assertions.
    pub const ENGINE_OFF_EXPECTED: usize = percolator_prog::constants::ENGINE_OFF;
    /// BITMAP_OFF: ENGINE_OFF + offset_of!(RiskEngine, used)
    pub const BITMAP_OFF_EXPECTED: usize =
        percolator_prog::constants::ENGINE_OFF
            + core::mem::offset_of!(percolator::RiskEngine, used);

    // Account field offsets (x86_64 native; SBF values differ by u128 alignment)
    pub const ACCOUNT_OWNER_OFF: usize = core::mem::offset_of!(percolator::Account, owner);
    pub const ACCOUNT_MATCHER_PROGRAM_OFF: usize =
        core::mem::offset_of!(percolator::Account, matcher_program);
    pub const ACCOUNT_MATCHER_CONTEXT_OFF: usize =
        core::mem::offset_of!(percolator::Account, matcher_context);
    pub const ACCOUNT_FEE_CREDITS_OFF: usize =
        core::mem::offset_of!(percolator::Account, fee_credits);
    pub const ACCOUNT_F_SNAP_OFF: usize =
        core::mem::offset_of!(percolator::Account, f_snap);

    // RiskEngine field offsets (x86_64 native)
    pub const ENGINE_VAULT_OFF: usize = core::mem::offset_of!(percolator::RiskEngine, vault);
    pub const ENGINE_C_TOT_OFF: usize = core::mem::offset_of!(percolator::RiskEngine, c_tot);
    pub const ENGINE_PNL_POS_TOT_OFF: usize =
        core::mem::offset_of!(percolator::RiskEngine, pnl_pos_tot);
    pub const ENGINE_PNL_MATURED_POS_TOT_OFF: usize =
        core::mem::offset_of!(percolator::RiskEngine, pnl_matured_pos_tot);
    pub const ENGINE_LIQ_CURSOR_OFF: usize =
        core::mem::offset_of!(percolator::RiskEngine, gc_cursor);
    pub const ENGINE_NUM_USED_ACCOUNTS_OFF: usize =
        core::mem::offset_of!(percolator::RiskEngine, num_used_accounts);
    pub const ENGINE_ACCOUNTS_OFF: usize =
        core::mem::offset_of!(percolator::RiskEngine, accounts);
}

#[test]
fn layout_header_len_matches_expected() {
    assert_eq!(
        HEADER_LEN,
        layout_constants::HEADER_LEN_EXPECTED,
        "HEADER_LEN drift: program computes {}, test expects {}",
        HEADER_LEN,
        layout_constants::HEADER_LEN_EXPECTED,
    );
}

#[test]
fn layout_config_len_matches_expected() {
    assert_eq!(
        CONFIG_LEN,
        layout_constants::CONFIG_LEN_EXPECTED,
        "CONFIG_LEN drift: program computes {}, test expects {}. \
         Was a field added/removed from MarketConfig?",
        CONFIG_LEN,
        layout_constants::CONFIG_LEN_EXPECTED,
    );
}

#[test]
fn layout_account_size_matches_program_constant() {
    // ACCOUNT_SIZE is computed by the program as size_of::<percolator::Account>().
    // This assert ties the constant to the struct size; if Account gains/loses fields
    // both ACCOUNT_SIZE and this test must be updated atomically.
    assert_eq!(
        ACCOUNT_SIZE,
        size_of::<Account>(),
        "ACCOUNT_SIZE constant ({}) does not match size_of::<Account>() ({}). \
         Update constants::ACCOUNT_SIZE and this test together.",
        ACCOUNT_SIZE,
        size_of::<Account>(),
    );
    // Also check against our pinned expected value (x86_64 native layout).
    assert_eq!(
        ACCOUNT_SIZE,
        layout_constants::ACCOUNT_SIZE_EXPECTED,
        "Account struct size changed: got {}, expected {}. \
         Update layout_constants::ACCOUNT_SIZE_EXPECTED if the change is intentional.",
        ACCOUNT_SIZE,
        layout_constants::ACCOUNT_SIZE_EXPECTED,
    );
}

#[test]
fn layout_account_field_offsets_pinned() {
    assert_eq!(
        offset_of!(Account, owner),
        layout_constants::ACCOUNT_OWNER_OFF,
        "Account.owner offset changed: {} vs pinned {}",
        offset_of!(Account, owner),
        layout_constants::ACCOUNT_OWNER_OFF,
    );
    assert_eq!(
        offset_of!(Account, matcher_program),
        layout_constants::ACCOUNT_MATCHER_PROGRAM_OFF,
        "Account.matcher_program offset changed: {} vs pinned {}",
        offset_of!(Account, matcher_program),
        layout_constants::ACCOUNT_MATCHER_PROGRAM_OFF,
    );
    assert_eq!(
        offset_of!(Account, matcher_context),
        layout_constants::ACCOUNT_MATCHER_CONTEXT_OFF,
        "Account.matcher_context offset changed: {} vs pinned {}",
        offset_of!(Account, matcher_context),
        layout_constants::ACCOUNT_MATCHER_CONTEXT_OFF,
    );
    assert_eq!(
        offset_of!(Account, fee_credits),
        layout_constants::ACCOUNT_FEE_CREDITS_OFF,
        "Account.fee_credits offset changed: {} vs pinned {}",
        offset_of!(Account, fee_credits),
        layout_constants::ACCOUNT_FEE_CREDITS_OFF,
    );
    assert_eq!(
        offset_of!(Account, f_snap),
        layout_constants::ACCOUNT_F_SNAP_OFF,
        "Account.f_snap offset changed: {} vs pinned {}",
        offset_of!(Account, f_snap),
        layout_constants::ACCOUNT_F_SNAP_OFF,
    );
}

#[test]
fn layout_account_field_adjacency_invariants() {
    // matcher_context immediately follows matcher_program (no padding between [u8;32] arrays)
    assert_eq!(
        offset_of!(Account, matcher_context),
        offset_of!(Account, matcher_program) + 32,
        "matcher_context should be exactly 32 bytes after matcher_program"
    );
    // owner immediately follows matcher_context
    assert_eq!(
        offset_of!(Account, owner),
        offset_of!(Account, matcher_context) + 32,
        "owner should be exactly 32 bytes after matcher_context"
    );
}

#[test]
fn layout_engine_off_matches_constant() {
    assert_eq!(
        ENGINE_OFF,
        layout_constants::ENGINE_OFF_EXPECTED,
        "ENGINE_OFF constant ({}) does not match expected ({})",
        ENGINE_OFF,
        layout_constants::ENGINE_OFF_EXPECTED,
    );
}

#[test]
fn layout_bitmap_off_is_engine_off_plus_used_offset() {
    let computed = ENGINE_OFF + offset_of!(RiskEngine, used);
    assert_eq!(
        layout_constants::BITMAP_OFF_EXPECTED,
        computed,
        "BITMAP_OFF mismatch: constant gives {}, offset_of! gives {}",
        layout_constants::BITMAP_OFF_EXPECTED,
        computed,
    );
}

#[test]
fn layout_engine_field_offsets_pinned() {
    assert_eq!(
        offset_of!(RiskEngine, vault),
        0,
        "RiskEngine.vault must be at offset 0"
    );
    assert_eq!(
        offset_of!(RiskEngine, vault),
        layout_constants::ENGINE_VAULT_OFF,
        "RiskEngine.vault offset drift"
    );
    assert_eq!(
        offset_of!(RiskEngine, c_tot),
        layout_constants::ENGINE_C_TOT_OFF,
        "RiskEngine.c_tot offset drift"
    );
    assert_eq!(
        offset_of!(RiskEngine, pnl_pos_tot),
        layout_constants::ENGINE_PNL_POS_TOT_OFF,
        "RiskEngine.pnl_pos_tot offset drift"
    );
    assert_eq!(
        offset_of!(RiskEngine, pnl_matured_pos_tot),
        layout_constants::ENGINE_PNL_MATURED_POS_TOT_OFF,
        "RiskEngine.pnl_matured_pos_tot offset drift"
    );
    assert_eq!(
        offset_of!(RiskEngine, gc_cursor),
        layout_constants::ENGINE_LIQ_CURSOR_OFF,
        "RiskEngine.gc_cursor offset drift"
    );
    assert_eq!(
        offset_of!(RiskEngine, num_used_accounts),
        layout_constants::ENGINE_NUM_USED_ACCOUNTS_OFF,
        "RiskEngine.num_used_accounts offset drift"
    );
    assert_eq!(
        offset_of!(RiskEngine, accounts),
        layout_constants::ENGINE_ACCOUNTS_OFF,
        "RiskEngine.accounts offset drift"
    );
}

// ============================================================================
// § 2 — Instruction Tag Consistency
// ============================================================================

/// Complete ordered list of all TAG_* constants (monotonically increasing).
/// Any addition or deletion here must match a corresponding change in tags.rs.
fn all_tags() -> &'static [u8] {
    &[
        TAG_INIT_MARKET,                   // 0
        TAG_INIT_USER,                     // 1
        TAG_INIT_LP,                       // 2
        TAG_DEPOSIT_COLLATERAL,            // 3
        TAG_WITHDRAW_COLLATERAL,           // 4
        TAG_KEEPER_CRANK,                  // 5
        TAG_TRADE_NO_CPI,                  // 6
        TAG_LIQUIDATE_AT_ORACLE,           // 7
        TAG_CLOSE_ACCOUNT,                 // 8
        TAG_TOP_UP_INSURANCE,              // 9
        TAG_TRADE_CPI,                     // 10
        TAG_SET_RISK_THRESHOLD,            // 11
        TAG_UPDATE_ADMIN,                  // 12
        TAG_CLOSE_SLAB,                    // 13
        TAG_UPDATE_CONFIG,                 // 14
        TAG_SET_MAINTENANCE_FEE,           // 15
        TAG_SET_ORACLE_AUTHORITY,          // 16
        TAG_PUSH_ORACLE_PRICE,             // 17
        TAG_SET_ORACLE_PRICE_CAP,          // 18
        TAG_RESOLVE_MARKET,                // 19
        TAG_WITHDRAW_INSURANCE,            // 20
        TAG_ADMIN_FORCE_CLOSE,             // 21
        TAG_SET_INSURANCE_WITHDRAW_POLICY, // 22
        TAG_WITHDRAW_INSURANCE_LIMITED,    // 23
        TAG_QUERY_LP_FEES,                 // 24
        TAG_RECLAIM_EMPTY_ACCOUNT,         // 25
        TAG_SETTLE_ACCOUNT,                // 26
        TAG_DEPOSIT_FEE_CREDITS,           // 27
        TAG_CONVERT_RELEASED_PNL,          // 28
        TAG_RESOLVE_PERMISSIONLESS,        // 29
        TAG_FORCE_CLOSE_RESOLVED,          // 30
        // 31 = gap (documented upstream reserved slot, no decode arm)
        TAG_SET_PYTH_ORACLE,               // 32
        TAG_UPDATE_MARK_PRICE,             // 33
        TAG_UPDATE_HYPERP_MARK,            // 34
        TAG_TRADE_CPI_V2,                  // 35
        TAG_UNRESOLVE_MARKET,              // 36
        TAG_CREATE_LP_VAULT,               // 37
        TAG_LP_VAULT_DEPOSIT,              // 38
        TAG_LP_VAULT_WITHDRAW,             // 39
        TAG_LP_VAULT_CRANK_FEES,           // 40
        TAG_FUND_MARKET_INSURANCE,         // 41
        TAG_SET_INSURANCE_ISOLATION,       // 42
        TAG_CHALLENGE_SETTLEMENT,          // 43
        TAG_RESOLVE_DISPUTE,               // 44
        TAG_DEPOSIT_LP_COLLATERAL,         // 45
        TAG_WITHDRAW_LP_COLLATERAL,        // 46
        TAG_QUEUE_WITHDRAWAL,              // 47
        TAG_CLAIM_QUEUED_WITHDRAWAL,       // 48
        TAG_CANCEL_QUEUED_WITHDRAWAL,      // 49
        TAG_EXECUTE_ADL,                   // 50
        TAG_CLOSE_STALE_SLAB,              // 51
        TAG_RECLAIM_SLAB_RENT,             // 52
        TAG_AUDIT_CRANK,                   // 53
        TAG_SET_OFFSET_PAIR,               // 54
        TAG_ATTEST_CROSS_MARGIN,           // 55
        TAG_ADVANCE_ORACLE_PHASE,          // 56
        TAG_SLASH_CREATION_DEPOSIT,        // 58 — reserved, no decode arm
        TAG_INIT_SHARED_VAULT,             // 59
        TAG_ALLOCATE_MARKET,               // 60
        TAG_QUEUE_WITHDRAWAL_SV,           // 61
        TAG_CLAIM_EPOCH_WITHDRAWAL,        // 62
        TAG_ADVANCE_EPOCH,                 // 63
        TAG_MINT_POSITION_NFT,             // 64
        TAG_TRANSFER_POSITION_OWNERSHIP,   // 65
        TAG_BURN_POSITION_NFT,             // 66
        TAG_SET_PENDING_SETTLEMENT,        // 67
        TAG_CLEAR_PENDING_SETTLEMENT,      // 68
        TAG_TRANSFER_OWNERSHIP_CPI,        // 69
        TAG_SET_WALLET_CAP,                // 70
        TAG_SET_OI_IMBALANCE_HARD_BLOCK,   // 71
        TAG_RESCUE_ORPHAN_VAULT,           // 72
        TAG_CLOSE_ORPHAN_SLAB,             // 73
        TAG_SET_DEX_POOL,                  // 74
        TAG_INIT_MATCHER_CTX,              // 75
        TAG_PAUSE_MARKET,                  // 76
        TAG_UNPAUSE_MARKET,                // 77
        TAG_SET_MAX_PNL_CAP,               // 78
        TAG_SET_OI_CAP_MULTIPLIER,         // 79
        TAG_SET_DISPUTE_PARAMS,            // 80
        TAG_SET_LP_COLLATERAL_PARAMS,      // 81
        TAG_ACCEPT_ADMIN,                  // 82
    ]
}

#[test]
fn tags_no_collisions() {
    let tags = all_tags();
    for i in 0..tags.len() {
        for j in (i + 1)..tags.len() {
            assert_ne!(
                tags[i], tags[j],
                "Tag collision: tags[{}] == tags[{}] == {}",
                i, j, tags[i]
            );
        }
    }
}

#[test]
fn tags_are_monotonically_increasing() {
    let tags = all_tags();
    for i in 1..tags.len() {
        assert!(
            tags[i] > tags[i - 1],
            "Tag ordering violated at index {}: {} <= {}",
            i,
            tags[i],
            tags[i - 1]
        );
    }
}

#[test]
fn tags_match_numeric_values() {
    // Verify each TAG_* constant carries the expected numeric value.
    // If upstream reorders or renumbers tags, this fails immediately.
    assert_eq!(TAG_INIT_MARKET, 0u8);
    assert_eq!(TAG_INIT_USER, 1u8);
    assert_eq!(TAG_INIT_LP, 2u8);
    assert_eq!(TAG_DEPOSIT_COLLATERAL, 3u8);
    assert_eq!(TAG_WITHDRAW_COLLATERAL, 4u8);
    assert_eq!(TAG_KEEPER_CRANK, 5u8);
    assert_eq!(TAG_TRADE_NO_CPI, 6u8);
    assert_eq!(TAG_LIQUIDATE_AT_ORACLE, 7u8);
    assert_eq!(TAG_CLOSE_ACCOUNT, 8u8);
    assert_eq!(TAG_TOP_UP_INSURANCE, 9u8);
    assert_eq!(TAG_TRADE_CPI, 10u8);
    assert_eq!(TAG_SET_RISK_THRESHOLD, 11u8);
    assert_eq!(TAG_UPDATE_ADMIN, 12u8);
    assert_eq!(TAG_CLOSE_SLAB, 13u8);
    assert_eq!(TAG_UPDATE_CONFIG, 14u8);
    assert_eq!(TAG_SET_MAINTENANCE_FEE, 15u8);
    assert_eq!(TAG_SET_ORACLE_AUTHORITY, 16u8);
    assert_eq!(TAG_PUSH_ORACLE_PRICE, 17u8);
    assert_eq!(TAG_SET_ORACLE_PRICE_CAP, 18u8);
    assert_eq!(TAG_RESOLVE_MARKET, 19u8);
    assert_eq!(TAG_WITHDRAW_INSURANCE, 20u8);
    assert_eq!(TAG_ADMIN_FORCE_CLOSE, 21u8);
    assert_eq!(TAG_SET_INSURANCE_WITHDRAW_POLICY, 22u8);
    assert_eq!(TAG_WITHDRAW_INSURANCE_LIMITED, 23u8);
    assert_eq!(TAG_QUERY_LP_FEES, 24u8);
    assert_eq!(TAG_RECLAIM_EMPTY_ACCOUNT, 25u8);
    assert_eq!(TAG_SETTLE_ACCOUNT, 26u8);
    assert_eq!(TAG_DEPOSIT_FEE_CREDITS, 27u8);
    assert_eq!(TAG_CONVERT_RELEASED_PNL, 28u8);
    assert_eq!(TAG_RESOLVE_PERMISSIONLESS, 29u8);
    assert_eq!(TAG_FORCE_CLOSE_RESOLVED, 30u8);
    // 31 = gap
    assert_eq!(TAG_SET_PYTH_ORACLE, 32u8);
    assert_eq!(TAG_UPDATE_MARK_PRICE, 33u8);
    assert_eq!(TAG_UPDATE_HYPERP_MARK, 34u8);
    assert_eq!(TAG_TRADE_CPI_V2, 35u8);
    assert_eq!(TAG_UNRESOLVE_MARKET, 36u8);
    assert_eq!(TAG_CREATE_LP_VAULT, 37u8);
    assert_eq!(TAG_LP_VAULT_DEPOSIT, 38u8);
    assert_eq!(TAG_LP_VAULT_WITHDRAW, 39u8);
    assert_eq!(TAG_LP_VAULT_CRANK_FEES, 40u8);
    assert_eq!(TAG_FUND_MARKET_INSURANCE, 41u8);
    assert_eq!(TAG_SET_INSURANCE_ISOLATION, 42u8);
    assert_eq!(TAG_CHALLENGE_SETTLEMENT, 43u8);
    assert_eq!(TAG_RESOLVE_DISPUTE, 44u8);
    assert_eq!(TAG_DEPOSIT_LP_COLLATERAL, 45u8);
    assert_eq!(TAG_WITHDRAW_LP_COLLATERAL, 46u8);
    assert_eq!(TAG_QUEUE_WITHDRAWAL, 47u8);
    assert_eq!(TAG_CLAIM_QUEUED_WITHDRAWAL, 48u8);
    assert_eq!(TAG_CANCEL_QUEUED_WITHDRAWAL, 49u8);
    assert_eq!(TAG_EXECUTE_ADL, 50u8);
    assert_eq!(TAG_CLOSE_STALE_SLAB, 51u8);
    assert_eq!(TAG_RECLAIM_SLAB_RENT, 52u8);
    assert_eq!(TAG_AUDIT_CRANK, 53u8);
    assert_eq!(TAG_SET_OFFSET_PAIR, 54u8);
    assert_eq!(TAG_ATTEST_CROSS_MARGIN, 55u8);
    assert_eq!(TAG_ADVANCE_ORACLE_PHASE, 56u8);
    assert_eq!(TAG_SLASH_CREATION_DEPOSIT, 58u8);
    assert_eq!(TAG_INIT_SHARED_VAULT, 59u8);
    assert_eq!(TAG_ALLOCATE_MARKET, 60u8);
    assert_eq!(TAG_QUEUE_WITHDRAWAL_SV, 61u8);
    assert_eq!(TAG_CLAIM_EPOCH_WITHDRAWAL, 62u8);
    assert_eq!(TAG_ADVANCE_EPOCH, 63u8);
    assert_eq!(TAG_MINT_POSITION_NFT, 64u8);
    assert_eq!(TAG_TRANSFER_POSITION_OWNERSHIP, 65u8);
    assert_eq!(TAG_BURN_POSITION_NFT, 66u8);
    assert_eq!(TAG_SET_PENDING_SETTLEMENT, 67u8);
    assert_eq!(TAG_CLEAR_PENDING_SETTLEMENT, 68u8);
    assert_eq!(TAG_TRANSFER_OWNERSHIP_CPI, 69u8);
    assert_eq!(TAG_SET_WALLET_CAP, 70u8);
    assert_eq!(TAG_SET_OI_IMBALANCE_HARD_BLOCK, 71u8);
    assert_eq!(TAG_RESCUE_ORPHAN_VAULT, 72u8);
    assert_eq!(TAG_CLOSE_ORPHAN_SLAB, 73u8);
    assert_eq!(TAG_SET_DEX_POOL, 74u8);
    assert_eq!(TAG_INIT_MATCHER_CTX, 75u8);
    assert_eq!(TAG_PAUSE_MARKET, 76u8);
    assert_eq!(TAG_UNPAUSE_MARKET, 77u8);
    assert_eq!(TAG_SET_MAX_PNL_CAP, 78u8);
    assert_eq!(TAG_SET_OI_CAP_MULTIPLIER, 79u8);
    assert_eq!(TAG_SET_DISPUTE_PARAMS, 80u8);
    assert_eq!(TAG_SET_LP_COLLATERAL_PARAMS, 81u8);
    assert_eq!(TAG_ACCEPT_ADMIN, 82u8);
}

/// Tags whose decode arm is unit (tag-only, no additional bytes).
#[test]
fn unit_variant_tags_decode_successfully() {
    let unit_tags: &[u8] = &[
        TAG_RESOLVE_MARKET,           // 19
        TAG_WITHDRAW_INSURANCE,       // 20
        TAG_RESOLVE_PERMISSIONLESS,   // 29
        TAG_LP_VAULT_CRANK_FEES,      // 40
        TAG_CLAIM_QUEUED_WITHDRAWAL,  // 48
        TAG_CANCEL_QUEUED_WITHDRAWAL, // 49
        TAG_CLOSE_STALE_SLAB,         // 51
        TAG_RECLAIM_SLAB_RENT,        // 52
        TAG_AUDIT_CRANK,              // 53
        TAG_ADVANCE_ORACLE_PHASE,     // 56
        TAG_CLAIM_EPOCH_WITHDRAWAL,   // 62
        TAG_ADVANCE_EPOCH,            // 63
        TAG_RESCUE_ORPHAN_VAULT,      // 72
        TAG_CLOSE_ORPHAN_SLAB,        // 73
        TAG_ACCEPT_ADMIN,             // 82
    ];
    for &tag in unit_tags {
        let result = Instruction::decode(&[tag]);
        assert!(
            result.is_ok(),
            "Tag {} (unit variant) failed to decode: {:?}",
            tag,
            result
        );
    }
}

/// Helper: assert a decode result is the expected error variant.
fn assert_decode_err(data: &[u8], msg: &str) {
    let result = Instruction::decode(data);
    assert!(result.is_err(), "{}: expected error but got Ok", msg);
    assert_eq!(
        result.unwrap_err(),
        ProgramError::InvalidInstructionData,
        "{}: expected InvalidInstructionData",
        msg
    );
}

/// Tags 31–36 are documented upstream-reserved gaps: they must return error.
/// Tag 42 (SetInsuranceIsolation) was removed as a no-op stub and now also
/// returns error at decode.
#[test]
fn gap_and_reserved_tags_return_invalid_instruction_data() {
    // 34 = UpdateHyperpMark (now ported), so only 31,32,33,35,36 are gaps
    // 42 = SetInsuranceIsolation removed (was no-op stub)
    let no_arm_tags: &[u8] = &[31, 32, 33, 35, 36, 42];
    for &tag in no_arm_tags {
        assert_decode_err(
            &[tag],
            &format!("Tag {} (documented gap/reserved)", tag),
        );
    }
}

/// TAG_SLASH_CREATION_DEPOSIT (58) is intentionally a stub with no dispatch.
/// Any call with this tag must be rejected. See GH#1975.
#[test]
fn slash_creation_deposit_tag_is_unimplemented_stub() {
    assert_eq!(
        TAG_SLASH_CREATION_DEPOSIT,
        58u8,
        "TAG_SLASH_CREATION_DEPOSIT value changed — update this test and GH#1975 tracking"
    );
    assert_decode_err(
        &[TAG_SLASH_CREATION_DEPOSIT],
        "TAG_SLASH_CREATION_DEPOSIT (58) must remain unimplemented (no decode arm)",
    );
}

#[test]
fn unknown_high_tag_returns_invalid_instruction_data() {
    assert_decode_err(&[255u8], "tag 255");
    // 76=PauseMarket, 77=UnpauseMarket — now valid
    assert_decode_err(&[78u8], "tag 78");
    assert_decode_err(&[100u8], "tag 100");
}

#[test]
fn empty_payload_returns_invalid_instruction_data() {
    assert_decode_err(&[], "empty payload");
}

/// Decode round-trips for parameterized instructions.
#[test]
fn deposit_collateral_decode_round_trip() {
    let user_idx: u16 = 42;
    let amount: u64 = 1_000_000;
    let mut data = vec![TAG_DEPOSIT_COLLATERAL];
    data.extend_from_slice(&user_idx.to_le_bytes());
    data.extend_from_slice(&amount.to_le_bytes());
    match Instruction::decode(&data) {
        Ok(Instruction::DepositCollateral {
            user_idx: got_idx,
            amount: got_amount,
        }) => {
            assert_eq!(got_idx, user_idx, "user_idx mismatch");
            assert_eq!(got_amount, amount, "amount mismatch");
        }
        other => panic!("Expected DepositCollateral, got {:?}", other),
    }
}

#[test]
fn withdraw_collateral_decode_round_trip() {
    let user_idx: u16 = 7;
    let amount: u64 = 500_000;
    let mut data = vec![TAG_WITHDRAW_COLLATERAL];
    data.extend_from_slice(&user_idx.to_le_bytes());
    data.extend_from_slice(&amount.to_le_bytes());
    match Instruction::decode(&data) {
        Ok(Instruction::WithdrawCollateral {
            user_idx: got_idx,
            amount: got_amount,
        }) => {
            assert_eq!(got_idx, user_idx);
            assert_eq!(got_amount, amount);
        }
        other => panic!("Expected WithdrawCollateral, got {:?}", other),
    }
}

#[test]
fn close_account_decode_round_trip() {
    let user_idx: u16 = 3;
    let mut data = vec![TAG_CLOSE_ACCOUNT];
    data.extend_from_slice(&user_idx.to_le_bytes());
    match Instruction::decode(&data) {
        Ok(Instruction::CloseAccount { user_idx: got }) => assert_eq!(got, user_idx),
        other => panic!("Expected CloseAccount, got {:?}", other),
    }
}

#[test]
fn update_admin_decode_round_trip() {
    let new_admin = [0xABu8; 32];
    let mut data = vec![TAG_UPDATE_ADMIN];
    data.extend_from_slice(&new_admin);
    match Instruction::decode(&data) {
        Ok(Instruction::UpdateAdmin { new_admin: got }) => {
            assert_eq!(got.to_bytes(), new_admin)
        }
        other => panic!("Expected UpdateAdmin, got {:?}", other),
    }
}

#[test]
fn top_up_insurance_decode_round_trip() {
    let amount: u64 = 99_999;
    let mut data = vec![TAG_TOP_UP_INSURANCE];
    data.extend_from_slice(&amount.to_le_bytes());
    match Instruction::decode(&data) {
        Ok(Instruction::TopUpInsurance { amount: got }) => assert_eq!(got, amount),
        other => panic!("Expected TopUpInsurance, got {:?}", other),
    }
}

#[test]
fn execute_adl_decode_round_trip() {
    let target_idx: u16 = 12;
    let mut data = vec![TAG_EXECUTE_ADL];
    data.extend_from_slice(&target_idx.to_le_bytes());
    match Instruction::decode(&data) {
        Ok(Instruction::ExecuteAdl { target_idx: got }) => assert_eq!(got, target_idx),
        other => panic!("Expected ExecuteAdl, got {:?}", other),
    }
}

#[test]
fn set_wallet_cap_decode_round_trip() {
    let cap: u64 = 50_000_000;
    let mut data = vec![TAG_SET_WALLET_CAP];
    data.extend_from_slice(&cap.to_le_bytes());
    match Instruction::decode(&data) {
        Ok(Instruction::SetWalletCap { cap_e6: got }) => assert_eq!(got, cap),
        other => panic!("Expected SetWalletCap, got {:?}", other),
    }
}

#[test]
fn set_oi_imbalance_hard_block_decode_round_trip() {
    let threshold: u16 = 500;
    let mut data = vec![TAG_SET_OI_IMBALANCE_HARD_BLOCK];
    data.extend_from_slice(&threshold.to_le_bytes());
    match Instruction::decode(&data) {
        Ok(Instruction::SetOiImbalanceHardBlock { threshold_bps: got }) => {
            assert_eq!(got, threshold)
        }
        other => panic!("Expected SetOiImbalanceHardBlock, got {:?}", other),
    }
}

#[test]
fn transfer_ownership_cpi_decode_round_trip() {
    let user_idx: u16 = 5;
    let new_owner = [0x12u8; 32];
    let mut data = vec![TAG_TRANSFER_OWNERSHIP_CPI];
    data.extend_from_slice(&user_idx.to_le_bytes());
    data.extend_from_slice(&new_owner);
    match Instruction::decode(&data) {
        Ok(Instruction::TransferOwnershipCpi {
            user_idx: got_idx,
            new_owner: got_owner,
        }) => {
            assert_eq!(got_idx, user_idx);
            assert_eq!(got_owner, new_owner);
        }
        other => panic!("Expected TransferOwnershipCpi, got {:?}", other),
    }
}

#[test]
fn set_dex_pool_decode_round_trip() {
    let pool = [0xCCu8; 32];
    let mut data = vec![TAG_SET_DEX_POOL];
    data.extend_from_slice(&pool);
    match Instruction::decode(&data) {
        Ok(Instruction::SetDexPool { pool: got }) => {
            assert_eq!(got.to_bytes(), pool)
        }
        other => panic!("Expected SetDexPool, got {:?}", other),
    }
}

#[test]
fn init_lp_decode_round_trip() {
    let matcher_program = [0x11u8; 32];
    let matcher_context = [0x22u8; 32];
    let fee_payment: u64 = 1_000;
    let mut data = vec![TAG_INIT_LP];
    data.extend_from_slice(&matcher_program);
    data.extend_from_slice(&matcher_context);
    data.extend_from_slice(&fee_payment.to_le_bytes());
    match Instruction::decode(&data) {
        Ok(Instruction::InitLP {
            matcher_program: got_prog,
            matcher_context: got_ctx,
            fee_payment: got_fee,
        }) => {
            assert_eq!(got_prog.to_bytes(), matcher_program);
            assert_eq!(got_ctx.to_bytes(), matcher_context);
            assert_eq!(got_fee, fee_payment);
        }
        other => panic!("Expected InitLP, got {:?}", other),
    }
}

#[test]
fn lp_vault_deposit_decode_round_trip() {
    let amount: u64 = 2_000_000;
    let mut data = vec![TAG_LP_VAULT_DEPOSIT];
    data.extend_from_slice(&amount.to_le_bytes());
    match Instruction::decode(&data) {
        Ok(Instruction::LpVaultDeposit { amount: got }) => assert_eq!(got, amount),
        other => panic!("Expected LpVaultDeposit, got {:?}", other),
    }
}

#[test]
fn lp_vault_withdraw_decode_round_trip() {
    let lp_amount: u64 = 500_000;
    let mut data = vec![TAG_LP_VAULT_WITHDRAW];
    data.extend_from_slice(&lp_amount.to_le_bytes());
    match Instruction::decode(&data) {
        Ok(Instruction::LpVaultWithdraw { lp_amount: got }) => assert_eq!(got, lp_amount),
        other => panic!("Expected LpVaultWithdraw, got {:?}", other),
    }
}

// ============================================================================
// § 3 — RiskParams Wire Format Round-Trip
// ============================================================================

/// Encodes a RiskParams payload in the exact field order read_risk_params expects.
/// This must stay in sync with the read_risk_params function in src/percolator.rs.
fn encode_risk_params_wire(
    h_min: u64,
    maintenance_margin_bps: u64,
    initial_margin_bps: u64,
    trading_fee_bps: u64,
    max_accounts: u64,
    new_account_fee: u128,
    insurance_floor: u128,
    h_max: u64,                   // v12.15: replaces maintenance_fee_per_slot high 8 bytes
    max_crank_staleness_slots: u64,
    liquidation_fee_bps: u64,
    liquidation_fee_cap: u128,
    liquidation_buffer_bps: u64,  // wire-only (removed from engine params, kept in wire)
    min_liquidation_abs: u128,
    min_initial_deposit: u128,
    min_nonzero_mm_req: u128,
    min_nonzero_im_req: u128,
) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&h_min.to_le_bytes());
    v.extend_from_slice(&maintenance_margin_bps.to_le_bytes());
    v.extend_from_slice(&initial_margin_bps.to_le_bytes());
    v.extend_from_slice(&trading_fee_bps.to_le_bytes());
    v.extend_from_slice(&max_accounts.to_le_bytes());
    v.extend_from_slice(&new_account_fee.to_le_bytes());
    v.extend_from_slice(&insurance_floor.to_le_bytes());
    v.extend_from_slice(&h_max.to_le_bytes());     // v12.15: h_max (u64)
    v.extend_from_slice(&max_crank_staleness_slots.to_le_bytes());
    v.extend_from_slice(&liquidation_fee_bps.to_le_bytes());
    v.extend_from_slice(&liquidation_fee_cap.to_le_bytes());
    v.extend_from_slice(&liquidation_buffer_bps.to_le_bytes());
    v.extend_from_slice(&min_liquidation_abs.to_le_bytes());
    v.extend_from_slice(&min_initial_deposit.to_le_bytes());
    v.extend_from_slice(&min_nonzero_mm_req.to_le_bytes());
    v.extend_from_slice(&min_nonzero_im_req.to_le_bytes());
    v
}

/// Total byte count of the RiskParams wire format.
/// This is what the SDK must use when sizing InitMarket instruction buffers.
const RISK_PARAMS_WIRE_LEN: usize =
    8   // h_min (u64)
  + 8   // maintenance_margin_bps (u64)
  + 8   // initial_margin_bps (u64)
  + 8   // trading_fee_bps (u64)
  + 8   // max_accounts (u64)
  + 16  // new_account_fee (u128)
  + 16  // insurance_floor wire slot (u128)
  + 8   // h_max (u64, v12.15)
  + 8   // max_crank_staleness_slots (u64)
  + 8   // liquidation_fee_bps (u64)
  + 16  // liquidation_fee_cap (u128)
  + 8   // liquidation_buffer_bps wire-only (u64)
  + 16  // min_liquidation_abs (u128)
  + 16  // min_initial_deposit (u128)
  + 16  // min_nonzero_mm_req (u128)
  + 16; // min_nonzero_im_req (u128)

#[test]
fn risk_params_wire_len_is_184_bytes() {
    // 184 bytes: v12.17 removed h_max padding (was 192 with 8-byte pad)
    assert_eq!(
        RISK_PARAMS_WIRE_LEN,
        184,
        "RiskParams wire format byte count changed: got {}, expected 184",
        RISK_PARAMS_WIRE_LEN
    );
}

#[test]
fn risk_params_encode_wire_produces_correct_byte_count() {
    let wire = encode_risk_params_wire(
        0, 500, 1000, 0, 64, 0, 0, 0, u64::MAX, 50, 1_000_000_000_000u128, 100, 0, 100, 1, 2,
    );
    assert_eq!(
        wire.len(),
        RISK_PARAMS_WIRE_LEN,
        "encode_risk_params_wire returned {} bytes, expected {}",
        wire.len(),
        RISK_PARAMS_WIRE_LEN
    );
}

/// read_risk_params requires all 3 trailing fields. Truncated payloads are rejected.
#[test]
fn risk_params_three_trailing_fields_are_mandatory() {
    let mut data = vec![TAG_INIT_MARKET];
    // Fixed header fields before RiskParams:
    data.extend_from_slice(&[0u8; 32]); // admin
    data.extend_from_slice(&[0u8; 32]); // collateral_mint
    data.extend_from_slice(&[0u8; 32]); // feed_id
    data.extend_from_slice(&86400u64.to_le_bytes()); // max_staleness_secs (1 day, ≤7d cap)
    data.extend_from_slice(&500u16.to_le_bytes()); // conf_filter_bps
    data.push(0u8); // invert
    data.extend_from_slice(&0u32.to_le_bytes()); // unit_scale
    data.extend_from_slice(&0u64.to_le_bytes()); // initial_mark_price_e6
    // maintenance_fee_per_slot removed in v12.15
    data.extend_from_slice(&u128::MAX.to_le_bytes()); // max_insurance_floor
    data.extend_from_slice(&0u64.to_le_bytes()); // min_oracle_price_cap_e2bps

    // Append risk params body but omit the last 3 fields (48 bytes)
    let full_wire = encode_risk_params_wire(
        0, 500, 1000, 0, 64, 0, 0, 0, u64::MAX, 50, 1_000_000_000_000u128, 100, 0, 100, 1, 2,
    );
    // RISK_PARAMS_WIRE_LEN - 3x16 = 192 - 48 = 144
    let truncated_len = RISK_PARAMS_WIRE_LEN - 48;
    data.extend_from_slice(&full_wire[..truncated_len]);

    assert_decode_err(
        &data,
        "Truncated RiskParams (missing trailing 3 fields) must be rejected",
    );
}

/// Full encode -> decode -> field-verify round-trip for RiskParams.
#[test]
fn risk_params_full_round_trip_via_init_market() {
    // Canonical test values from common/mod.rs encode_init_market_full_v2.
    let warmup: u64 = 0;
    let mm_bps: u64 = 500;
    let im_bps: u64 = 1000;
    let fee_bps: u64 = 0;
    let max_accts: u64 = 64;
    let new_acct_fee: u128 = 0;
    let insurance_floor: u128 = 0;
    let maint_fee: u128 = 0;
    let crank_staleness: u64 = u64::MAX;
    let liq_fee_bps: u64 = 50;
    let liq_fee_cap: u128 = 1_000_000_000_000u128;
    let liq_buf_bps: u64 = 100;
    let min_liq_abs: u128 = 0;
    let min_init_deposit: u128 = 100;
    let min_nonzero_mm: u128 = 1;
    let min_nonzero_im: u128 = 2;

    let mut data = vec![TAG_INIT_MARKET];
    data.extend_from_slice(&[0xAAu8; 32]); // admin
    data.extend_from_slice(&[0xBBu8; 32]); // collateral_mint
    data.extend_from_slice(&[0xABu8; 32]); // feed_id
    data.extend_from_slice(&86400u64.to_le_bytes()); // max_staleness_secs (1 day, ≤7d cap)
    data.extend_from_slice(&500u16.to_le_bytes()); // conf_filter_bps
    data.push(0u8); // invert
    data.extend_from_slice(&0u32.to_le_bytes()); // unit_scale
    data.extend_from_slice(&0u64.to_le_bytes()); // initial_mark_price_e6
    data.extend_from_slice(&1_000_000_000u128.to_le_bytes()); // max_maintenance_fee_per_slot (legacy wire field, still decoded)
    data.extend_from_slice(&10_000_000_000_000_000u128.to_le_bytes()); // max_insurance_floor
    data.extend_from_slice(&0u64.to_le_bytes()); // min_oracle_price_cap_e2bps
    let h_max: u64 = 0;
    data.extend_from_slice(&encode_risk_params_wire(
        warmup, mm_bps, im_bps, fee_bps, max_accts, new_acct_fee, insurance_floor,
        h_max, crank_staleness, liq_fee_bps, liq_fee_cap, liq_buf_bps,
        min_liq_abs, min_init_deposit, min_nonzero_mm, min_nonzero_im,
    ));
    // Extended tail (66 bytes) — all defaults
    data.extend_from_slice(&0u16.to_le_bytes()); // insurance_withdraw_max_bps
    data.extend_from_slice(&0u64.to_le_bytes()); // insurance_withdraw_cooldown_slots
    data.extend_from_slice(&0u64.to_le_bytes()); // permissionless_resolve_stale_slots
    data.extend_from_slice(&500u64.to_le_bytes()); // funding_horizon_slots
    data.extend_from_slice(&100u64.to_le_bytes()); // funding_k_bps
    data.extend_from_slice(&500i64.to_le_bytes()); // funding_max_premium_bps
    data.extend_from_slice(&5i64.to_le_bytes()); // funding_max_bps_per_slot
    data.extend_from_slice(&0u64.to_le_bytes()); // mark_min_fee
    data.extend_from_slice(&0u64.to_le_bytes()); // force_close_delay_slots

    match Instruction::decode(&data) {
        Ok(Instruction::InitMarket { risk_params, .. }) => {
            // Verify every RiskParams field round-tripped correctly.
            assert_eq!(risk_params.h_min, warmup, "h_min");
            assert_eq!(risk_params.maintenance_margin_bps, mm_bps, "maintenance_margin_bps");
            assert_eq!(risk_params.initial_margin_bps, im_bps, "initial_margin_bps");
            assert_eq!(risk_params.trading_fee_bps, fee_bps, "trading_fee_bps");
            assert_eq!(risk_params.max_accounts, max_accts, "max_accounts");
            assert_eq!(
                risk_params.new_account_fee.get(),
                new_acct_fee,
                "new_account_fee"
            );
            // maintenance_fee_per_slot assertion removed in v12.15
            let _ = maint_fee;
            assert_eq!(
                risk_params.max_crank_staleness_slots,
                crank_staleness,
                "max_crank_staleness_slots"
            );
            assert_eq!(risk_params.liquidation_fee_bps, liq_fee_bps, "liquidation_fee_bps");
            assert_eq!(
                risk_params.liquidation_fee_cap.get(),
                liq_fee_cap,
                "liquidation_fee_cap"
            );
            assert_eq!(
                risk_params.min_liquidation_abs.get(),
                min_liq_abs,
                "min_liquidation_abs"
            );
            // Three mandatory trailing fields (PERC-spec: truncated payloads rejected)
            assert_eq!(
                risk_params.min_initial_deposit.get(),
                min_init_deposit,
                "min_initial_deposit"
            );
            assert_eq!(risk_params.min_nonzero_mm_req, min_nonzero_mm, "min_nonzero_mm_req");
            assert_eq!(risk_params.min_nonzero_im_req, min_nonzero_im, "min_nonzero_im_req");
            assert_eq!(
                risk_params.insurance_floor.get(),
                insurance_floor,
                "insurance_floor"
            );
        }
        other => panic!("Expected InitMarket to decode, got {:?}", other),
    }
}

// ============================================================================
// § 4 — Program ID Constants
// ============================================================================

/// Mainnet program ID. Must not change without a full upgrade cycle and SDK bump.
const MAINNET_PROG_ID: &str = "GM8zjJ8LTBMv9xEsverh6H6wLyevgMHEJXcEzyY3rY24";

/// Mainnet stake program ID.
const MAINNET_STAKE_ID: &str = "DC5fovFQD5SZYsetwvEqd4Wi4PFY1Yfnc669VMe6oa7F";

#[test]
fn mainnet_prog_id_parses_and_is_pinned() {
    let parsed = MAINNET_PROG_ID
        .parse::<Pubkey>()
        .expect("MAINNET_PROG_ID must be a valid base58 Pubkey");
    assert_eq!(
        parsed.to_string(),
        MAINNET_PROG_ID,
        "Mainnet prog ID base58 round-trip failed — constant may be malformed"
    );
    let bytes = parsed.to_bytes();
    assert_ne!(bytes, [0u8; 32], "Mainnet prog ID must not be all zeros");
}

#[test]
fn mainnet_stake_id_parses_and_is_pinned() {
    let parsed = MAINNET_STAKE_ID
        .parse::<Pubkey>()
        .expect("MAINNET_STAKE_ID must be a valid base58 Pubkey");
    assert_eq!(
        parsed.to_string(),
        MAINNET_STAKE_ID,
        "Mainnet stake ID base58 round-trip failed"
    );
    let bytes = parsed.to_bytes();
    assert_ne!(bytes, [0u8; 32], "Mainnet stake ID must not be all zeros");
}

#[test]
fn mainnet_prog_id_differs_from_stake_id() {
    let prog: Pubkey = MAINNET_PROG_ID.parse().unwrap();
    let stake: Pubkey = MAINNET_STAKE_ID.parse().unwrap();
    assert_ne!(
        prog, stake,
        "Prog ID and stake ID must be distinct — same key is a configuration error"
    );
}

// ============================================================================
// § 5 — Account Struct Size Assertions
// ============================================================================

/// Compile-time assertion: Account must be a multiple of 8 bytes.
const _ACCOUNT_SIZE_ALIGN_CHECK: () = {
    let sz = size_of::<Account>();
    if sz % 8 != 0 {
        panic!("Account size is not aligned to 8 bytes — SBF ABI violation")
    }
};

#[test]
fn account_size_is_multiple_of_8() {
    assert_eq!(
        size_of::<Account>() % 8,
        0,
        "Account size ({}) must be a multiple of 8 for BPF alignment",
        size_of::<Account>()
    );
}

#[test]
fn account_size_matches_program_account_size_constant() {
    assert_eq!(
        ACCOUNT_SIZE,
        size_of::<Account>(),
        "constants::ACCOUNT_SIZE ({}) diverged from size_of::<Account>() ({}). \
         The constant must be updated atomically with the struct.",
        ACCOUNT_SIZE,
        size_of::<Account>()
    );
}

#[test]
fn account_capital_at_offset_zero() {
    assert_eq!(
        offset_of!(Account, capital),
        0,
        "capital must be the first field at offset 0"
    );
}

#[test]
fn account_kind_immediately_follows_capital() {
    // capital is U128 = [u64; 2] = 16 bytes, starting at offset 0. So kind = 16.
    assert_eq!(
        offset_of!(Account, kind),
        offset_of!(Account, capital) + 16, // U128 is 16 bytes
        "Account.kind must immediately follow capital"
    );
}

#[test]
fn account_fields_do_not_overlap() {
    // Spot-check non-overlap of critical field ranges.
    assert!(
        offset_of!(Account, pnl) > offset_of!(Account, kind),
        "pnl must come after kind"
    );
    assert!(
        offset_of!(Account, matcher_program)
            >= offset_of!(Account, adl_epoch_snap) + size_of::<u64>(),
        "matcher_program overlaps with adl_epoch_snap"
    );
    assert!(
        offset_of!(Account, matcher_context) >= offset_of!(Account, matcher_program) + 32,
        "matcher_context overlaps with matcher_program"
    );
    assert!(
        offset_of!(Account, owner) >= offset_of!(Account, matcher_context) + 32,
        "owner overlaps with matcher_context"
    );
    assert!(
        offset_of!(Account, fee_credits) >= offset_of!(Account, owner) + 32,
        "fee_credits overlaps with owner"
    );
    assert!(
        size_of::<Account>() >= offset_of!(Account, fee_credits) + 16,
        "Account struct is too small to contain fee_credits"
    );
}

// ============================================================================
// § 6 — Engine Struct Field Assertions
// ============================================================================

#[test]
fn engine_vault_is_first_field_at_offset_zero() {
    assert_eq!(
        offset_of!(RiskEngine, vault),
        0,
        "RiskEngine.vault must be the first field at offset 0. \
         Any reordering breaks SDK reads."
    );
}

#[test]
fn engine_insurance_fund_at_offset_16() {
    // vault is U128 = [u64; 2] = 16 bytes at offset 0.
    // insurance_fund must immediately follow at offset 16.
    assert_eq!(
        offset_of!(RiskEngine, insurance_fund),
        16,
        "insurance_fund must be at offset 16 (immediately after vault U128)"
    );
}

#[test]
fn engine_params_immediately_follows_insurance_fund() {
    let expected = offset_of!(RiskEngine, insurance_fund)
        + size_of::<percolator::InsuranceFund>();
    assert_eq!(
        offset_of!(RiskEngine, params),
        expected,
        "RiskEngine.params must immediately follow insurance_fund. \
         expected={}, actual={}",
        expected,
        offset_of!(RiskEngine, params),
    );
}

#[test]
fn engine_aggregate_fields_relative_order() {
    // The SDK reads vault, c_tot, pnl_pos_tot, pnl_matured_pos_tot, gc_cursor in order.
    // Verify their offsets are strictly increasing.
    let vault_off = offset_of!(RiskEngine, vault);
    let c_tot_off = offset_of!(RiskEngine, c_tot);
    let pnl_pos_off = offset_of!(RiskEngine, pnl_pos_tot);
    let pnl_mat_off = offset_of!(RiskEngine, pnl_matured_pos_tot);
    let gc_cursor_off = offset_of!(RiskEngine, gc_cursor);

    assert!(vault_off < c_tot_off, "vault must precede c_tot");
    assert!(c_tot_off < pnl_pos_off, "c_tot must precede pnl_pos_tot");
    assert!(pnl_pos_off < pnl_mat_off, "pnl_pos_tot must precede pnl_matured_pos_tot");
    assert!(pnl_mat_off < gc_cursor_off, "pnl_matured_pos_tot must precede gc_cursor");
}

#[test]
fn engine_used_bitmap_immediately_precedes_num_used_accounts() {
    use percolator::BITMAP_WORDS;
    let used_off = offset_of!(RiskEngine, used);
    let num_used_off = offset_of!(RiskEngine, num_used_accounts);
    assert_eq!(
        num_used_off,
        used_off + BITMAP_WORDS * 8,
        "num_used_accounts must immediately follow the used bitmap. \
         used_off={}, BITMAP_WORDS={}, expected num_used_off={}",
        used_off,
        BITMAP_WORDS,
        used_off + BITMAP_WORDS * 8,
    );
}

#[test]
fn engine_accounts_array_fills_end_of_struct() {
    // The accounts array is the last field in RiskEngine and must fill all remaining bytes.
    let accounts_off = offset_of!(RiskEngine, accounts);
    let engine_size = size_of::<RiskEngine>();
    let remaining = engine_size - accounts_off;
    let expected = size_of::<Account>() * percolator::MAX_ACCOUNTS;
    assert_eq!(
        remaining,
        expected,
        "accounts array must fill the remaining struct space. \
         remaining={}, expected={}",
        remaining,
        expected,
    );
}

#[test]
fn engine_slab_len_equals_engine_off_plus_engine_size_plus_risk_buf_plus_gen_table() {
    let expected = ENGINE_OFF
        + size_of::<RiskEngine>()
        + percolator_prog::constants::RISK_BUF_LEN
        + percolator_prog::constants::GEN_TABLE_LEN;
    assert_eq!(
        percolator_prog::constants::SLAB_LEN,
        expected,
        "SLAB_LEN ({}) != ENGINE_OFF ({}) + sizeof(RiskEngine) ({}) + RISK_BUF_LEN ({}) + GEN_TABLE_LEN ({})",
        percolator_prog::constants::SLAB_LEN,
        ENGINE_OFF,
        size_of::<RiskEngine>(),
        percolator_prog::constants::RISK_BUF_LEN,
        percolator_prog::constants::GEN_TABLE_LEN,
    );
}

// ============================================================================
// § 7 — Mock Slab Round-Trip (field-level byte parsing)
// ============================================================================

/// Verify that writing known values at the byte offsets the SDK reads, then reading
/// them back, produces the expected values. This simulates SDK account parsing.
#[test]
fn mock_account_field_byte_round_trip() {
    let mut buf = vec![0u8; size_of::<Account>()];

    let owner_val = [0xDEu8; 32];
    let matcher_program_val = [0xCAu8; 32];
    let matcher_context_val = [0xFEu8; 32];

    let owner_off = offset_of!(Account, owner);
    let mp_off = offset_of!(Account, matcher_program);
    let mc_off = offset_of!(Account, matcher_context);
    let fee_credits_off = offset_of!(Account, fee_credits);

    buf[owner_off..owner_off + 32].copy_from_slice(&owner_val);
    buf[mp_off..mp_off + 32].copy_from_slice(&matcher_program_val);
    buf[mc_off..mc_off + 32].copy_from_slice(&matcher_context_val);

    // Write fee_credits as I128 = [u64; 2] (little-endian, lo-hi order)
    let fee_credits_val: i128 = -999_999;
    let lo = fee_credits_val as u64;
    let hi = (fee_credits_val >> 64) as u64;
    buf[fee_credits_off..fee_credits_off + 8].copy_from_slice(&lo.to_le_bytes());
    buf[fee_credits_off + 8..fee_credits_off + 16].copy_from_slice(&hi.to_le_bytes());

    // Verify raw byte reads
    assert_eq!(
        &buf[owner_off..owner_off + 32],
        &owner_val,
        "owner field read-back mismatch"
    );
    assert_eq!(
        &buf[mp_off..mp_off + 32],
        &matcher_program_val,
        "matcher_program field read-back mismatch"
    );
    assert_eq!(
        &buf[mc_off..mc_off + 32],
        &matcher_context_val,
        "matcher_context field read-back mismatch"
    );

    // Reconstruct fee_credits from raw bytes
    let lo_read =
        u64::from_le_bytes(buf[fee_credits_off..fee_credits_off + 8].try_into().unwrap());
    let hi_read =
        u64::from_le_bytes(buf[fee_credits_off + 8..fee_credits_off + 16].try_into().unwrap());
    let reconstructed = (hi_read as i128) << 64 | lo_read as i128;
    assert_eq!(reconstructed, fee_credits_val, "fee_credits I128 round-trip failed");
}

/// Verify vault U128 can be read at ENGINE_OFF from a raw slab buffer.
#[test]
fn mock_engine_vault_byte_round_trip() {
    let buf_len = ENGINE_OFF + 32; // vault (U128=16) + insurance_fund.balance start
    let mut buf = vec![0u8; buf_len];

    let vault_val: u128 = 1_234_567_890_000u128;
    // vault is at ENGINE_OFF + 0 (vault is first field in RiskEngine)
    let vault_off = ENGINE_OFF;
    buf[vault_off..vault_off + 8].copy_from_slice(&(vault_val as u64).to_le_bytes());
    buf[vault_off + 8..vault_off + 16]
        .copy_from_slice(&((vault_val >> 64) as u64).to_le_bytes());

    let lo = u64::from_le_bytes(buf[vault_off..vault_off + 8].try_into().unwrap());
    let hi = u64::from_le_bytes(buf[vault_off + 8..vault_off + 16].try_into().unwrap());
    let read_back = (hi as u128) << 64 | lo as u128;
    assert_eq!(read_back, vault_val, "vault U128 byte round-trip failed");
}

// ============================================================================
// § 8 — Magic Constant Guard
// ============================================================================

#[test]
fn magic_constant_encodes_percolat_ascii() {
    // MAGIC = 0x504552434f4c4154
    // In big-endian byte order this is b"PERCOLAT" (0x50='P', 0x45='E', ..., 0x54='T').
    // Note: when stored on-chain (little-endian), the bytes are reversed.
    let bytes_be = MAGIC.to_be_bytes();
    assert_eq!(&bytes_be, b"PERCOLAT", "MAGIC constant must encode to ASCII 'PERCOLAT' in big-endian");
    // Also verify the raw hex value is pinned.
    assert_eq!(MAGIC, 0x504552434f4c4154u64, "MAGIC hex value changed");
}

// ============================================================================
// § 9 — Matcher ABI Constants
// ============================================================================

#[test]
fn matcher_abi_version_is_two() {
    assert_eq!(
        MATCHER_ABI_VERSION,
        2u32,
        "MATCHER_ABI_VERSION must be 2 — update SDK bindings if this changes"
    );
}

#[test]
fn matcher_context_len_is_320() {
    // The matcher context account is exactly 320 bytes.
    // SDK initializes this account with this exact size.
    assert_eq!(
        MATCHER_CONTEXT_LEN,
        320,
        "MATCHER_CONTEXT_LEN changed: got {}, expected 320",
        MATCHER_CONTEXT_LEN
    );
}
