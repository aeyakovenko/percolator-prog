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
pub const TAG_SET_RISK_THRESHOLD: u8 = 11;
pub const TAG_UPDATE_ADMIN: u8 = 12;
pub const TAG_CLOSE_SLAB: u8 = 13;
pub const TAG_UPDATE_CONFIG: u8 = 14;
pub const TAG_SET_MAINTENANCE_FEE: u8 = 15;
pub const TAG_SET_ORACLE_AUTHORITY: u8 = 16;
pub const TAG_PUSH_ORACLE_PRICE: u8 = 17;
pub const TAG_SET_ORACLE_PRICE_CAP: u8 = 18;
pub const TAG_RESOLVE_MARKET: u8 = 19;
pub const TAG_WITHDRAW_INSURANCE: u8 = 20;
pub const TAG_ADMIN_FORCE_CLOSE: u8 = 21;
// Tags 22-28: upstream repurposed these slots. Old names
// (UpdateRiskParams, RenounceAdmin, CreateInsuranceMint,
// DepositInsuranceLp, WithdrawInsuranceLp) no longer exist.
// Correct mappings per decode table:
pub const TAG_SET_INSURANCE_WITHDRAW_POLICY: u8 = 22;
pub const TAG_WITHDRAW_INSURANCE_LIMITED: u8 = 23;
pub const TAG_QUERY_LP_FEES: u8 = 24;
pub const TAG_RECLAIM_EMPTY_ACCOUNT: u8 = 25;
pub const TAG_SETTLE_ACCOUNT: u8 = 26;
pub const TAG_DEPOSIT_FEE_CREDITS: u8 = 27;
pub const TAG_CONVERT_RELEASED_PNL: u8 = 28;
pub const TAG_RESOLVE_PERMISSIONLESS: u8 = 29;
pub const TAG_FORCE_CLOSE_RESOLVED: u8 = 30;
// Tag 31: gap (no decode arm)
// Tags 32-36: reserved upstream slots (no decode arm in fork).
pub const TAG_SET_PYTH_ORACLE: u8 = 32;
pub const TAG_UPDATE_MARK_PRICE: u8 = 33;
pub const TAG_UPDATE_HYPERP_MARK: u8 = 34;
pub const TAG_TRADE_CPI_V2: u8 = 35;
pub const TAG_UNRESOLVE_MARKET: u8 = 36;

// ═══════════════════════════════════════════════════════════════
// LP Vault instructions (PERC-272)
// ═══════════════════════════════════════════════════════════════
/// Create LP vault: initialise state PDA + SPL mint for LP shares (PERC-272).
pub const TAG_CREATE_LP_VAULT: u8 = 37;
/// Deposit into LP vault: transfer SOL → vault, mint LP shares (PERC-272).
pub const TAG_LP_VAULT_DEPOSIT: u8 = 38;
/// Withdraw from LP vault: burn LP shares, receive SOL (PERC-272).
pub const TAG_LP_VAULT_WITHDRAW: u8 = 39;
/// Permissionless crank: distribute accrued fee revenue to LP vault capital (PERC-272).
pub const TAG_LP_VAULT_CRANK_FEES: u8 = 40;
/// Fund per-market isolated insurance balance (PERC-306).
/// Admin deposits tokens into the market's isolated insurance reserve.
pub const TAG_FUND_MARKET_INSURANCE: u8 = 41;
/// Set insurance isolation BPS for a market (PERC-306).
/// Admin configures max % of global fund this market can access.
pub const TAG_SET_INSURANCE_ISOLATION: u8 = 42;
/// PERC-314: Challenge settlement price during dispute window.
pub const TAG_CHALLENGE_SETTLEMENT: u8 = 43;
/// PERC-314: Resolve dispute (admin adjudication).
pub const TAG_RESOLVE_DISPUTE: u8 = 44;
/// PERC-315: Deposit LP vault tokens as perp collateral.
pub const TAG_DEPOSIT_LP_COLLATERAL: u8 = 45;
/// PERC-315: Withdraw LP collateral (position must be closed).
pub const TAG_WITHDRAW_LP_COLLATERAL: u8 = 46;
/// PERC-309: Queue a large LP withdrawal.
pub const TAG_QUEUE_WITHDRAWAL: u8 = 47;
/// PERC-309: Claim one epoch tranche from queued withdrawal.
pub const TAG_CLAIM_QUEUED_WITHDRAWAL: u8 = 48;
/// PERC-309: Cancel queued withdrawal, refund remaining.
pub const TAG_CANCEL_QUEUED_WITHDRAWAL: u8 = 49;
/// PERC-305: Auto-deleverage — surgically close profitable positions when PnL cap hit.
pub const TAG_EXECUTE_ADL: u8 = 50;
/// Close a stale slab (wrong size from old program layout) and recover rent SOL.
/// Skips slab_guard; verifies header magic + admin authority. Admin only.
pub const TAG_CLOSE_STALE_SLAB: u8 = 51;
/// Reclaim rent from an uninitialised slab (magic = 0) when market creation fails mid-flow.
/// The slab account must sign (proves the caller holds the slab keypair).
/// Cannot close an initialised slab (magic == MAGIC) — use CloseSlab (tag 13) for those.
pub const TAG_RECLAIM_SLAB_RENT: u8 = 52;
/// Permissionless on-chain audit crank: walk all accounts and verify conservation invariants.
/// Checks capital, PnL, OI, LP aggregates and solvency. Pauses market on violation.
pub const TAG_AUDIT_CRANK: u8 = 53;
/// Admin: configure cross-market margin offset for a pair of slabs.
/// Creates/updates an OffsetPairConfig PDA at ["cmor_pair", slab_a, slab_b].
pub const TAG_SET_OFFSET_PAIR: u8 = 54;
/// Permissionless: attest user positions across two slabs for portfolio margin credit.
/// Creates/updates a CrossMarginAttestation PDA at ["cmor", user, slab_a, slab_b].
pub const TAG_ATTEST_CROSS_MARGIN: u8 = 55;
/// PERC-622: Advance oracle phase (permissionless crank).
/// Transitions market through Phase 1→2→3 based on time + volume milestones.
pub const TAG_ADVANCE_ORACLE_PHASE: u8 = 56;
/// PERC-623: Top up keeper fund (permissionless).
pub const TAG_TOPUP_KEEPER_FUND: u8 = 57;
/// PERC-629: Slash creation deposit.
///
/// UNIMPLEMENTED — reserved for post-launch.
/// This tag is defined and tested (uniqueness + sequential) but has NO corresponding
/// `Instruction` variant, decode arm, or dispatch handler. CPI calls with tag 58 will
/// return `InvalidInstructionData`. The `creator_history` module implements `compute_slash`
/// and its Kani proofs, but they are not yet wired to an on-chain instruction.
/// See GH#1975 for resolution options before external audit.
pub const TAG_SLASH_CREATION_DEPOSIT: u8 = 58;
/// PERC-628: Initialize the global shared vault.
pub const TAG_INIT_SHARED_VAULT: u8 = 59;
/// PERC-628: Allocate virtual liquidity to a market.
pub const TAG_ALLOCATE_MARKET: u8 = 60;
/// PERC-628: Queue a withdrawal request for the current epoch.
pub const TAG_QUEUE_WITHDRAWAL_SV: u8 = 61;
/// PERC-628: Claim a queued withdrawal after epoch elapses.
pub const TAG_CLAIM_EPOCH_WITHDRAWAL: u8 = 62;
/// PERC-628: Advance the shared vault epoch (permissionless crank).
pub const TAG_ADVANCE_EPOCH: u8 = 63;

// ═══════════════════════════════════════════════════════════════
// Position NFT instructions (PERC-608)
// ═══════════════════════════════════════════════════════════════
/// PERC-608: Mint a Position NFT (Token-2022 + TokenMetadata extension) for an open position.
pub const TAG_MINT_POSITION_NFT: u8 = 64;
/// PERC-608: Transfer position ownership via the NFT.
pub const TAG_TRANSFER_POSITION_OWNERSHIP: u8 = 65;
/// PERC-608: Burn the Position NFT when a position is closed.
pub const TAG_BURN_POSITION_NFT: u8 = 66;
/// PERC-608: Keeper sets pending_settlement flag before funding transfer.
pub const TAG_SET_PENDING_SETTLEMENT: u8 = 67;
/// PERC-608: Keeper clears pending_settlement flag after settlement crank.
pub const TAG_CLEAR_PENDING_SETTLEMENT: u8 = 68;
/// PERC-608: Transfer position ownership via CPI (called by percolator-nft TransferHook).
/// Changes account[user_idx].owner to new_owner. Caller must be the NFT program's
/// mint authority PDA (verified via signer). Admin cannot call this directly.
/// Data: tag(1) + user_idx(2) + new_owner(32)
/// Accounts: [nft_mint_authority(signer), slab(writable), nft_program]
pub const TAG_TRANSFER_OWNERSHIP_CPI: u8 = 69;
/// PERC-8111: Set per-wallet position cap (admin only).
/// Data: tag(1) + cap_e6(8) — 0 = disabled, non-zero = max abs(position_size) in e6.
/// Accounts: [admin(signer), slab(writable)]
pub const TAG_SET_WALLET_CAP: u8 = 70;
/// PERC-8110: Set OI imbalance hard block threshold (admin only).
/// Data: tag(1) + threshold_bps(2) — 0 = disabled, 1-10000 = max imbalance ratio in bps.
/// Accounts: [admin(signer), slab(writable)]
pub const TAG_SET_OI_IMBALANCE_HARD_BLOCK: u8 = 71;
/// PERC-8400: Rescue orphan vault — recover tokens from vault when engine balance
/// is zero due to raw SPL transfer bypassing engine accounting.
/// Admin-only. Requires no open positions (num_used_accounts == 0).
/// Reads actual vault token balance and transfers to admin ATA.
/// Accounts: [admin(signer), slab(writable), admin_ata(writable), vault(writable), token_program, vault_pda]
pub const TAG_RESCUE_ORPHAN_VAULT: u8 = 72;
/// PERC-8400: Close orphan slab — drain lamports when vault is already empty.
/// Layout-agnostic: reads raw bytes for magic + admin, verifies vault ATA is zero.
/// Accounts: [admin(signer,writable), slab(writable), vault(readonly), vault_pda(readonly)]
pub const TAG_CLOSE_ORPHAN_SLAB: u8 = 73;
/// PERC-SetDexPool: Pin the admin-approved DEX pool address on-chain for a HYPERP market.
/// Admin-only. Only valid for HYPERP markets (index_feed_id == [0;32]).
/// Validates pool account ownership (Raydium/Meteora/PumpSwap) and mint match before storing.
/// UpdateHyperpMark will reject any pool that doesn't match the stored address.
/// Data: tag(1) + pool_pubkey(32)
/// Accounts: [admin(signer), slab(writable), pool_account(readonly)]
pub const TAG_SET_DEX_POOL: u8 = 74;
/// CPI to the matcher program to initialize the matcher context account for a given LP slot.
/// The LP PDA must sign via invoke_signed. Admin-only.
/// Data: tag(1) + lp_idx(2) + kind(1) + trading_fee_bps(4) + base_spread_bps(4) +
///       max_total_bps(4) + impact_k_bps(4) + liquidity_notional_e6(16) +
///       max_fill_abs(16) + max_inventory_abs(16) + fee_to_insurance_bps(2) +
///       skew_spread_mult_bps(2) = 72 bytes total
/// Accounts: [admin(signer), slab(readonly), matcher_ctx(writable), matcher_prog(executable), lp_pda]
pub const TAG_INIT_MATCHER_CTX: u8 = 75;
/// Admin emergency pause — blocks Trade, Deposit, Withdraw, InitUser.
/// Crank, Liquidate, and admin ops still work while paused.
pub const TAG_PAUSE_MARKET: u8 = 76;
/// Admin unpause — re-enables all operations.
pub const TAG_UNPAUSE_MARKET: u8 = 77;

#[cfg(test)]
mod tests {
    use super::*;

    /// Ensure no duplicate tag values. Compile-time safety net.
    #[test]
    fn no_duplicate_tags() {
        let tags: &[u8] = &[
            TAG_INIT_MARKET,
            TAG_INIT_USER,
            TAG_INIT_LP,
            TAG_DEPOSIT_COLLATERAL,
            TAG_WITHDRAW_COLLATERAL,
            TAG_KEEPER_CRANK,
            TAG_TRADE_NO_CPI,
            TAG_LIQUIDATE_AT_ORACLE,
            TAG_CLOSE_ACCOUNT,
            TAG_TOP_UP_INSURANCE,
            TAG_TRADE_CPI,
            TAG_SET_RISK_THRESHOLD,
            TAG_UPDATE_ADMIN,
            TAG_CLOSE_SLAB,
            TAG_UPDATE_CONFIG,
            TAG_SET_MAINTENANCE_FEE,
            TAG_SET_ORACLE_AUTHORITY,
            TAG_PUSH_ORACLE_PRICE,
            TAG_SET_ORACLE_PRICE_CAP,
            TAG_RESOLVE_MARKET,
            TAG_WITHDRAW_INSURANCE,
            TAG_ADMIN_FORCE_CLOSE,
            TAG_SET_INSURANCE_WITHDRAW_POLICY,
            TAG_WITHDRAW_INSURANCE_LIMITED,
            TAG_QUERY_LP_FEES,
            TAG_RECLAIM_EMPTY_ACCOUNT,
            TAG_SETTLE_ACCOUNT,
            TAG_DEPOSIT_FEE_CREDITS,
            TAG_CONVERT_RELEASED_PNL,
            TAG_RESOLVE_PERMISSIONLESS,
            TAG_FORCE_CLOSE_RESOLVED,
            // 31 = gap (no decode arm)
            TAG_SET_PYTH_ORACLE,
            TAG_UPDATE_MARK_PRICE,
            TAG_UPDATE_HYPERP_MARK,
            TAG_TRADE_CPI_V2,
            TAG_UNRESOLVE_MARKET,
            TAG_CREATE_LP_VAULT,
            TAG_LP_VAULT_DEPOSIT,
            TAG_LP_VAULT_WITHDRAW,
            TAG_LP_VAULT_CRANK_FEES,
            TAG_FUND_MARKET_INSURANCE,
            TAG_SET_INSURANCE_ISOLATION,
            TAG_CHALLENGE_SETTLEMENT,
            TAG_RESOLVE_DISPUTE,
            TAG_DEPOSIT_LP_COLLATERAL,
            TAG_WITHDRAW_LP_COLLATERAL,
            TAG_QUEUE_WITHDRAWAL,
            TAG_CLAIM_QUEUED_WITHDRAWAL,
            TAG_CANCEL_QUEUED_WITHDRAWAL,
            TAG_EXECUTE_ADL,
            TAG_CLOSE_STALE_SLAB,
            TAG_RECLAIM_SLAB_RENT,
            TAG_AUDIT_CRANK,
            TAG_SET_OFFSET_PAIR,
            TAG_ATTEST_CROSS_MARGIN,
            TAG_ADVANCE_ORACLE_PHASE,
            TAG_TOPUP_KEEPER_FUND,
            TAG_SLASH_CREATION_DEPOSIT,
            TAG_INIT_SHARED_VAULT,
            TAG_ALLOCATE_MARKET,
            TAG_QUEUE_WITHDRAWAL_SV,
            TAG_CLAIM_EPOCH_WITHDRAWAL,
            TAG_ADVANCE_EPOCH,
            TAG_MINT_POSITION_NFT,
            TAG_TRANSFER_POSITION_OWNERSHIP,
            TAG_BURN_POSITION_NFT,
            TAG_SET_PENDING_SETTLEMENT,
            TAG_CLEAR_PENDING_SETTLEMENT,
            TAG_TRANSFER_OWNERSHIP_CPI,
            TAG_SET_WALLET_CAP,
            TAG_SET_OI_IMBALANCE_HARD_BLOCK,
            TAG_RESCUE_ORPHAN_VAULT,
            TAG_CLOSE_ORPHAN_SLAB,
            TAG_SET_DEX_POOL,
            TAG_INIT_MATCHER_CTX,
            TAG_PAUSE_MARKET,
            TAG_UNPAUSE_MARKET,
        ];

        for i in 0..tags.len() {
            for j in (i + 1)..tags.len() {
                assert_ne!(tags[i], tags[j], "Duplicate tag value: {}", tags[i]);
            }
        }
    }

    /// Ensure tags are monotonically increasing (allows gaps for removed instructions).
    #[test]
    fn tags_are_sequential() {
        let tags: &[u8] = &[
            TAG_INIT_MARKET,
            TAG_INIT_USER,
            TAG_INIT_LP,
            TAG_DEPOSIT_COLLATERAL,
            TAG_WITHDRAW_COLLATERAL,
            TAG_KEEPER_CRANK,
            TAG_TRADE_NO_CPI,
            TAG_LIQUIDATE_AT_ORACLE,
            TAG_CLOSE_ACCOUNT,
            TAG_TOP_UP_INSURANCE,
            TAG_TRADE_CPI,
            TAG_SET_RISK_THRESHOLD,
            TAG_UPDATE_ADMIN,
            TAG_CLOSE_SLAB,
            TAG_UPDATE_CONFIG,
            TAG_SET_MAINTENANCE_FEE,
            TAG_SET_ORACLE_AUTHORITY,
            TAG_PUSH_ORACLE_PRICE,
            TAG_SET_ORACLE_PRICE_CAP,
            TAG_RESOLVE_MARKET,
            TAG_WITHDRAW_INSURANCE,
            TAG_ADMIN_FORCE_CLOSE,
            TAG_SET_INSURANCE_WITHDRAW_POLICY,
            TAG_WITHDRAW_INSURANCE_LIMITED,
            TAG_QUERY_LP_FEES,
            TAG_RECLAIM_EMPTY_ACCOUNT,
            TAG_SETTLE_ACCOUNT,
            TAG_DEPOSIT_FEE_CREDITS,
            TAG_CONVERT_RELEASED_PNL,
            TAG_RESOLVE_PERMISSIONLESS,
            TAG_FORCE_CLOSE_RESOLVED,
            // 31 = gap (no decode arm)
            TAG_SET_PYTH_ORACLE,
            TAG_UPDATE_MARK_PRICE,
            TAG_UPDATE_HYPERP_MARK,
            TAG_TRADE_CPI_V2,
            TAG_UNRESOLVE_MARKET,
            TAG_CREATE_LP_VAULT,
            TAG_LP_VAULT_DEPOSIT,
            TAG_LP_VAULT_WITHDRAW,
            TAG_LP_VAULT_CRANK_FEES,
            TAG_FUND_MARKET_INSURANCE,
            TAG_SET_INSURANCE_ISOLATION,
            TAG_CHALLENGE_SETTLEMENT,
            TAG_RESOLVE_DISPUTE,
            TAG_DEPOSIT_LP_COLLATERAL,
            TAG_WITHDRAW_LP_COLLATERAL,
            TAG_QUEUE_WITHDRAWAL,
            TAG_CLAIM_QUEUED_WITHDRAWAL,
            TAG_CANCEL_QUEUED_WITHDRAWAL,
            TAG_EXECUTE_ADL,
            TAG_CLOSE_STALE_SLAB,
            TAG_RECLAIM_SLAB_RENT,
            TAG_AUDIT_CRANK,
            TAG_SET_OFFSET_PAIR,
            TAG_ATTEST_CROSS_MARGIN,
            TAG_ADVANCE_ORACLE_PHASE,
            TAG_TOPUP_KEEPER_FUND,
            TAG_SLASH_CREATION_DEPOSIT,
            TAG_INIT_SHARED_VAULT,
            TAG_ALLOCATE_MARKET,
            TAG_QUEUE_WITHDRAWAL_SV,
            TAG_CLAIM_EPOCH_WITHDRAWAL,
            TAG_ADVANCE_EPOCH,
            TAG_MINT_POSITION_NFT,
            TAG_TRANSFER_POSITION_OWNERSHIP,
            TAG_BURN_POSITION_NFT,
            TAG_SET_PENDING_SETTLEMENT,
            TAG_CLEAR_PENDING_SETTLEMENT,
            // SECURITY(L-6): Tags 69-77 were missing from this test.
            TAG_TRANSFER_OWNERSHIP_CPI,
            TAG_SET_WALLET_CAP,
            TAG_SET_OI_IMBALANCE_HARD_BLOCK,
            TAG_RESCUE_ORPHAN_VAULT,
            TAG_CLOSE_ORPHAN_SLAB,
            TAG_SET_DEX_POOL,
            TAG_INIT_MATCHER_CTX,
            TAG_PAUSE_MARKET,
            TAG_UNPAUSE_MARKET,
        ];

        // Verify monotonically increasing (allows gaps for removed instructions)
        for i in 1..tags.len() {
            assert!(
                tags[i] > tags[i - 1],
                "Tags not monotonically increasing at index {}: {} <= {}",
                i, tags[i], tags[i - 1]
            );
        }
    }

    /// Guard: TAG_SLASH_CREATION_DEPOSIT (58) must remain unimplemented until PERC-629 is
    /// scheduled for mainnet. This test documents the intentional stub status and ensures
    /// the tag never silently gains a dispatch handler without deliberate review.
    ///
    /// If PERC-629 is being implemented: (1) remove this test, (2) add an Instruction variant
    /// and decode arm in percolator.rs, (3) add a dispatch arm, (4) add a security review.
    /// See GH#1975.
    #[test]
    fn tag_slash_creation_deposit_is_unimplemented_stub() {
        // The tag value is reserved in the sequential range but intentionally has no
        // Instruction variant, decode arm, or dispatch handler. Any CPI call with tag 58
        // currently returns InvalidInstructionData.
        //
        // This assertion is a compile-time-value guard: if the constant changes (indicating
        // someone rewired it), this test will draw attention to the change.
        assert_eq!(
            super::TAG_SLASH_CREATION_DEPOSIT,
            58u8,
            "TAG_SLASH_CREATION_DEPOSIT must stay at 58 — change only when PERC-629 is scheduled"
        );
    }
}
