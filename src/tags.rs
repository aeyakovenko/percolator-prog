//! Instruction tag constants for percolator-launch.
//!
//! This file is the **single source of truth** for instruction numbering.
//! Any CPI caller (percolator-stake, indexers, keepers) MUST use these exact values.
//!
//! ⚠️ NEVER reorder, remove, or reuse a tag number.
//! Always append new instructions at the end.

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
pub const TAG_UPDATE_RISK_PARAMS: u8 = 22;
pub const TAG_RENOUNCE_ADMIN: u8 = 23;
pub const TAG_CREATE_INSURANCE_MINT: u8 = 24;
pub const TAG_DEPOSIT_INSURANCE_LP: u8 = 25;
pub const TAG_WITHDRAW_INSURANCE_LP: u8 = 26;
pub const TAG_PAUSE_MARKET: u8 = 27;
pub const TAG_UNPAUSE_MARKET: u8 = 28;

// ═══════════════════════════════════════════════════════════════
// Future instructions — append here, never reorder above
// ═══════════════════════════════════════════════════════════════
/// Two-step admin transfer: new admin accepts the proposal.
pub const TAG_ACCEPT_ADMIN: u8 = 29;
/// Set insurance withdrawal policy on a resolved market (PERC-110).
pub const TAG_SET_INSURANCE_WITHDRAW_POLICY: u8 = 30;
/// Withdraw limited amount from insurance fund per policy (PERC-110).
pub const TAG_WITHDRAW_INSURANCE_LIMITED: u8 = 31;
/// Configure on-chain Pyth oracle for a market (PERC-117).
pub const TAG_SET_PYTH_ORACLE: u8 = 32;
/// Update mark price EMA (PERC-118, reserved).
pub const TAG_UPDATE_MARK_PRICE: u8 = 33;
/// Update Hyperp mark from DEX oracle (PERC-119).
pub const TAG_UPDATE_HYPERP_MARK: u8 = 34;
/// Optimized TradeCpi with caller-provided PDA bump (PERC-154).
/// Eliminates find_program_address (~1500 CU savings).
pub const TAG_TRADE_CPI_V2: u8 = 35;
/// Unresolve a market: clear RESOLVED flag, re-enable trading (PERC-273).
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
            TAG_UPDATE_RISK_PARAMS,
            TAG_RENOUNCE_ADMIN,
            TAG_CREATE_INSURANCE_MINT,
            TAG_DEPOSIT_INSURANCE_LP,
            TAG_WITHDRAW_INSURANCE_LP,
            TAG_PAUSE_MARKET,
            TAG_UNPAUSE_MARKET,
            TAG_ACCEPT_ADMIN,
            TAG_SET_INSURANCE_WITHDRAW_POLICY,
            TAG_WITHDRAW_INSURANCE_LIMITED,
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
        ];

        for i in 0..tags.len() {
            for j in (i + 1)..tags.len() {
                assert_ne!(tags[i], tags[j], "Duplicate tag value: {}", tags[i]);
            }
        }
    }

    /// Ensure tags are sequential starting from 0.
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
            TAG_UPDATE_RISK_PARAMS,
            TAG_RENOUNCE_ADMIN,
            TAG_CREATE_INSURANCE_MINT,
            TAG_DEPOSIT_INSURANCE_LP,
            TAG_WITHDRAW_INSURANCE_LP,
            TAG_PAUSE_MARKET,
            TAG_UNPAUSE_MARKET,
            TAG_ACCEPT_ADMIN,
            TAG_SET_INSURANCE_WITHDRAW_POLICY,
            TAG_WITHDRAW_INSURANCE_LIMITED,
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
        ];

        for (i, &tag) in tags.iter().enumerate() {
            assert_eq!(
                tag, i as u8,
                "Tag at index {} should be {} but is {}",
                i, i, tag
            );
        }
    }
}
