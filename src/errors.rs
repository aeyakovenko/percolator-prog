//! Program error codes.
//!
//! `#[error_code(offset = 0)]` emits `From<PercolatorError> for
//! anchor_lang_v2::Error` (= `ProgramError`) mapping each variant to
//! `Custom(e as u32)`. The `offset = 0` is load-bearing for ABI parity:
//! Anchor v2's default offset is 6000, but the legacy native program
//! shipped these codes at offset 0 via a hand-rolled
//! `ProgramError::Custom(e as u32)`. Variant order is equally load-bearing
//! — any reorder shifts every on-chain error code.

use anchor_lang_v2::prelude::*;
use percolator::RiskError;
use solana_program_error::ProgramError;

#[error_code(offset = 0)]
pub enum PercolatorError {
    #[msg("invalid magic")]
    InvalidMagic,
    #[msg("invalid version")]
    InvalidVersion,
    #[msg("already initialized")]
    AlreadyInitialized,
    #[msg("not initialized")]
    NotInitialized,
    #[msg("invalid slab length")]
    InvalidSlabLen,
    #[msg("invalid oracle key")]
    InvalidOracleKey,
    #[msg("oracle stale")]
    OracleStale,
    #[msg("oracle confidence too wide")]
    OracleConfTooWide,
    #[msg("invalid vault ATA")]
    InvalidVaultAta,
    #[msg("invalid mint")]
    InvalidMint,
    #[msg("expected signer")]
    ExpectedSigner,
    #[msg("expected writable")]
    ExpectedWritable,
    #[msg("oracle invalid")]
    OracleInvalid,
    #[msg("engine: insufficient balance")]
    EngineInsufficientBalance,
    #[msg("engine: undercollateralized")]
    EngineUndercollateralized,
    #[msg("engine: unauthorized")]
    EngineUnauthorized,
    #[msg("engine: invalid matching engine")]
    EngineInvalidMatchingEngine,
    #[msg("engine: PnL not warmed up")]
    EnginePnlNotWarmedUp,
    #[msg("engine: arithmetic overflow")]
    EngineOverflow,
    #[msg("engine: account not found")]
    EngineAccountNotFound,
    #[msg("engine: not an LP account")]
    EngineNotAnLPAccount,
    #[msg("engine: position size mismatch")]
    EnginePositionSizeMismatch,
    #[msg("engine: risk-reduction-only mode")]
    EngineRiskReductionOnlyMode,
    #[msg("engine: account kind mismatch")]
    EngineAccountKindMismatch,
    #[msg("invalid token account")]
    InvalidTokenAccount,
    #[msg("invalid token program")]
    InvalidTokenProgram,
    #[msg("invalid config parameter")]
    InvalidConfigParam,
    #[msg("Hyperp TradeNoCpi disabled")]
    HyperpTradeNoCpiDisabled,
    #[msg("engine: corrupt state")]
    EngineCorruptState,
    /// Wrapper-level: catchup loop required before normal accrual can proceed.
    #[msg("catchup required")]
    CatchupRequired,
    /// Deposit rejected by `tvl_insurance_cap_mult` cap.
    #[msg("deposit cap exceeded")]
    DepositCapExceeded,
    /// `WithdrawInsuranceLimited` called within the cooldown window.
    #[msg("insurance withdraw cooldown")]
    InsuranceWithdrawCooldown,
    /// `WithdrawInsuranceLimited` amount exceeds per-call cap.
    #[msg("insurance withdraw cap exceeded")]
    InsuranceWithdrawCapExceeded,
}

/// Map an engine `RiskError` to a `ProgramError::Custom` whose code matches
/// the corresponding `PercolatorError` variant. Mirrors the legacy mapping.
pub fn map_risk_error(e: RiskError) -> ProgramError {
    let err = match e {
        RiskError::InsufficientBalance => PercolatorError::EngineInsufficientBalance,
        RiskError::Undercollateralized => PercolatorError::EngineUndercollateralized,
        RiskError::Unauthorized => PercolatorError::EngineUnauthorized,
        RiskError::PnlNotWarmedUp => PercolatorError::EnginePnlNotWarmedUp,
        RiskError::Overflow => PercolatorError::EngineOverflow,
        RiskError::AccountNotFound => PercolatorError::EngineAccountNotFound,
        RiskError::SideBlocked => PercolatorError::EngineRiskReductionOnlyMode,
        RiskError::CorruptState => PercolatorError::EngineCorruptState,
    };
    ProgramError::Custom(err as u32)
}
