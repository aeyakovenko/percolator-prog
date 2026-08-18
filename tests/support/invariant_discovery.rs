use super::v16_svm::{
    MarketConfig, PublicTraceEvidence, V16Svm, EXIT_MAKER_DEPOSIT, INITIAL_PRICE,
    PRIMARY_ACTOR_COUNT, USER_DEPOSIT,
};
use percolator::{
    BackingBucketStatusV16, MarketModeV16, SideModeV16, ADL_ONE, BOUND_SCALE, POS_SCALE,
};
use percolator_prog::{
    constants::{ORACLE_LEG_FLAG_DIVIDE_LEG2, ORACLE_LEG_FLAG_DIVIDE_LEG3},
    error::PercolatorError,
    ix::{BatchTradeCpiLeg, BatchTradeLeg, CrankObservationHint},
};
use serde::{Deserialize, Serialize};
use solana_sdk::{account::Account, transaction::Transaction};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum PortfolioIntentKind {
    Deposit,
    Withdraw,
    Close,
    MatcherDisable,
    TradeNoCpi,
    TradeCpi,
    BatchTradeNoCpi,
    BatchTradeCpi,
    ConvertReleasedPnl,
    RebalanceReduce,
    ForfeitRecoveryLeg,
}

impl PortfolioIntentKind {
    pub const ALL: [Self; 11] = [
        Self::Deposit,
        Self::Withdraw,
        Self::Close,
        Self::MatcherDisable,
        Self::TradeNoCpi,
        Self::TradeCpi,
        Self::BatchTradeNoCpi,
        Self::BatchTradeCpi,
        Self::ConvertReleasedPnl,
        Self::RebalanceReduce,
        Self::ForfeitRecoveryLeg,
    ];
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum MarketIntentKind {
    Deposit,
    MatcherEnable,
    TradeFeePolicy,
    FeeRedirectPolicy,
    MaintenanceFeePolicy,
    LiquidationFeePolicy,
    ShutdownAsset,
    ResolveMarket,
    ResolvePolicy,
    RebalanceReduce,
    ForfeitRecoveryLeg,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum AssetIntentKind {
    TradeNoCpi,
    TradeCpi,
    BatchTradeNoCpi,
    BatchTradeCpi,
    PushAuthMark,
    PushEwmaMark,
    ConfigureAuthMark,
    ConfigureEwmaMark,
    ConfigureHybridOracle,
    InsuranceTopUp,
    BackingTopUp,
    InsuranceWithdrawal,
    BackingFeePolicy,
    ResolveMarket,
    ResolvePolicy,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum TerminalGenerationKind {
    MarketResolve,
    MarketResolvePolicy,
    AssetResolvePolicy,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum PositionEpisodeKind {
    RebalanceReduce,
    RecoveryForfeit,
}

impl PositionEpisodeKind {
    pub const ALL: [Self; 2] = [Self::RebalanceReduce, Self::RecoveryForfeit];

    fn discriminator(self) -> u8 {
        match self {
            Self::RebalanceReduce => 0,
            Self::RecoveryForfeit => 1,
        }
    }
}

impl TerminalGenerationKind {
    pub const MARKET: [Self; 2] = [Self::MarketResolve, Self::MarketResolvePolicy];
    pub const ASSET: [Self; 1] = [Self::AssetResolvePolicy];

    fn discriminator(self) -> u8 {
        match self {
            Self::MarketResolve => 0,
            Self::MarketResolvePolicy => 1,
            Self::AssetResolvePolicy => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum AuthorityIntentKind {
    MarketAuthorityHandoff,
    AssetAdminHandoff,
    InsuranceAuthorityHandoff,
    InsuranceOperatorHandoff,
    BackingAuthorityHandoff,
    OracleAuthorityHandoff,
    ResolveMarket,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum FundedRoleKind {
    BackingProvider,
    InsuranceOperator,
    TerminalInsuranceAuthority,
}

impl FundedRoleKind {
    pub const ALL: [Self; 3] = [
        Self::BackingProvider,
        Self::InsuranceOperator,
        Self::TerminalInsuranceAuthority,
    ];

    fn discriminator(self) -> u8 {
        match self {
            Self::BackingProvider => 0,
            Self::InsuranceOperator => 1,
            Self::TerminalInsuranceAuthority => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum RetryIntentKind {
    Deposit,
    Withdraw,
    TradeNoCpi,
    TradeCpi,
    BatchTradeNoCpi,
    BatchTradeCpi,
    ConvertReleasedPnl,
    RebalanceReduce,
    InsuranceTopUp,
    BackingTopUp,
    AssetActivation,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SupersededIntentKind {
    MatcherConfig,
    PushAuthMark,
    ConfigureAuthMark,
    PushEwmaMark,
    ConfigureEwmaMark,
    ConfigureHybridOracle,
    TradeFeePolicy,
    FeeRedirectPolicy,
    LiquidationFeePolicy,
    MaintenanceFeePolicy,
    MarketInitFeePolicy,
    ResolvePolicy,
    BackingFeePolicy,
    BackingFeePolicyShort,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum FeeConsentKind {
    FreshSignedLiveBaseFee,
    RetainedNoCpiBaseFee,
    RetainedBatchNoCpiBaseFee,
    CpiBaseFee,
    BatchCpiBaseFee,
    CpiCallerFee,
    BatchCpiCallerFee,
    PermissionlessActivationFee,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SourceFeeConsentKind {
    NoCpi,
    BatchNoCpi,
    Cpi,
    BatchCpi,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum BackingProviderConsentOrder {
    FundThenPolicy,
    PolicyThenRetainedFund,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum AccrualOrderingKind {
    CpiTradeClose,
    BatchCpiTradeClose,
    RebalanceReduce,
    RecoveryForfeit,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProspectiveAccrualRoute {
    NoCpi,
    Cpi,
    BatchNoCpi,
    BatchCpi,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum PendingMarkSource {
    AuthenticatedPush,
    EwmaPush,
    ReportedPriceTrade,
    ReportedPriceBatch,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum TradeDrivenMarkMode {
    Ewma,
    HybridAfterHours,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ResolvedAdlCloseOrder {
    WinnerThenLoser,
    LoserThenWinner,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum StaleCohortRoute {
    NoCpi,
    BatchNoCpi,
    Cpi,
    BatchCpi,
}

impl StaleCohortRoute {
    pub const ALL: [Self; 4] = [Self::NoCpi, Self::BatchNoCpi, Self::Cpi, Self::BatchCpi];

    fn discriminator(self) -> u8 {
        match self {
            Self::NoCpi => 0,
            Self::BatchNoCpi => 1,
            Self::Cpi => 2,
            Self::BatchCpi => 3,
        }
    }
}

impl ResolvedAdlCloseOrder {
    pub const ALL: [Self; 2] = [Self::WinnerThenLoser, Self::LoserThenWinner];

    fn discriminator(self) -> u8 {
        match self {
            Self::WinnerThenLoser => 0,
            Self::LoserThenWinner => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum CompositeRoundingScale {
    LargeMove,
    MicroMove,
}

impl CompositeRoundingScale {
    pub const ALL: [Self; 2] = [Self::LargeMove, Self::MicroMove];

    fn discriminator(self) -> u8 {
        match self {
            Self::LargeMove => 0,
            Self::MicroMove => 1,
        }
    }
}

impl TradeDrivenMarkMode {
    pub const ALL: [Self; 2] = [Self::Ewma, Self::HybridAfterHours];

    fn discriminator(self) -> u8 {
        match self {
            Self::Ewma => 0,
            Self::HybridAfterHours => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum DiscoveryTradeRoute {
    NoCpi,
    BatchNoCpi,
    Cpi,
    BatchCpi,
}

impl DiscoveryTradeRoute {
    pub const ALL: [Self; 4] = [Self::NoCpi, Self::BatchNoCpi, Self::Cpi, Self::BatchCpi];

    fn discriminator(self) -> u8 {
        match self {
            Self::NoCpi => 0,
            Self::BatchNoCpi => 1,
            Self::Cpi => 2,
            Self::BatchCpi => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum BackingExpiryLanding {
    Before,
    At,
    After,
}

impl BackingExpiryLanding {
    pub const ALL: [Self; 3] = [Self::Before, Self::At, Self::After];

    fn authenticated_slot(self, expiry_slot: u64) -> Result<u64, String> {
        match self {
            Self::Before => expiry_slot
                .checked_sub(1)
                .ok_or_else(|| "pre-expiry landing slot underflow".to_string()),
            Self::At => Ok(expiry_slot),
            Self::After => expiry_slot
                .checked_add(1)
                .ok_or_else(|| "post-expiry landing slot overflow".to_string()),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ActiveLegOrder {
    RescueFirst,
    RescueLast,
}

impl ActiveLegOrder {
    pub const ALL: [Self; 2] = [Self::RescueFirst, Self::RescueLast];

    fn discriminator(self) -> u8 {
        match self {
            Self::RescueFirst => 0,
            Self::RescueLast => 1,
        }
    }
}

impl PendingMarkSource {
    pub const ALL: [Self; 4] = [
        Self::AuthenticatedPush,
        Self::EwmaPush,
        Self::ReportedPriceTrade,
        Self::ReportedPriceBatch,
    ];

    fn discriminator(self) -> u8 {
        match self {
            Self::AuthenticatedPush => 0,
            Self::EwmaPush => 1,
            Self::ReportedPriceTrade => 2,
            Self::ReportedPriceBatch => 3,
        }
    }
}

impl ProspectiveAccrualRoute {
    pub const ALL: [Self; 4] = [Self::NoCpi, Self::Cpi, Self::BatchNoCpi, Self::BatchCpi];

    fn discriminator(self) -> u8 {
        match self {
            Self::NoCpi => 0,
            Self::Cpi => 1,
            Self::BatchNoCpi => 2,
            Self::BatchCpi => 3,
        }
    }
}

impl AccrualOrderingKind {
    pub const ALL: [Self; 4] = [
        Self::CpiTradeClose,
        Self::BatchCpiTradeClose,
        Self::RebalanceReduce,
        Self::RecoveryForfeit,
    ];

    fn discriminator(self) -> u8 {
        match self {
            Self::CpiTradeClose => 0,
            Self::BatchCpiTradeClose => 1,
            Self::RebalanceReduce => 2,
            Self::RecoveryForfeit => 3,
        }
    }
}

impl BackingProviderConsentOrder {
    pub const ALL: [Self; 2] = [Self::FundThenPolicy, Self::PolicyThenRetainedFund];

    fn discriminator(self) -> u8 {
        match self {
            Self::FundThenPolicy => 0,
            Self::PolicyThenRetainedFund => 1,
        }
    }
}

impl SourceFeeConsentKind {
    pub const ALL: [Self; 4] = [Self::NoCpi, Self::BatchNoCpi, Self::Cpi, Self::BatchCpi];

    fn discriminator(self) -> u8 {
        match self {
            Self::NoCpi => 0,
            Self::BatchNoCpi => 1,
            Self::Cpi => 2,
            Self::BatchCpi => 3,
        }
    }
}

impl FeeConsentKind {
    pub const ALL: [Self; 8] = [
        Self::FreshSignedLiveBaseFee,
        Self::RetainedNoCpiBaseFee,
        Self::RetainedBatchNoCpiBaseFee,
        Self::CpiBaseFee,
        Self::BatchCpiBaseFee,
        Self::CpiCallerFee,
        Self::BatchCpiCallerFee,
        Self::PermissionlessActivationFee,
    ];

    fn discriminator(self) -> u8 {
        match self {
            Self::FreshSignedLiveBaseFee => 0,
            Self::RetainedNoCpiBaseFee => 1,
            Self::RetainedBatchNoCpiBaseFee => 2,
            Self::CpiBaseFee => 3,
            Self::BatchCpiBaseFee => 4,
            Self::CpiCallerFee => 5,
            Self::BatchCpiCallerFee => 6,
            Self::PermissionlessActivationFee => 7,
        }
    }
}

impl SupersededIntentKind {
    pub const ALL: [Self; 14] = [
        Self::MatcherConfig,
        Self::PushAuthMark,
        Self::ConfigureAuthMark,
        Self::PushEwmaMark,
        Self::ConfigureEwmaMark,
        Self::ConfigureHybridOracle,
        Self::TradeFeePolicy,
        Self::FeeRedirectPolicy,
        Self::LiquidationFeePolicy,
        Self::MaintenanceFeePolicy,
        Self::MarketInitFeePolicy,
        Self::ResolvePolicy,
        Self::BackingFeePolicy,
        Self::BackingFeePolicyShort,
    ];

    fn discriminator(self) -> u8 {
        match self {
            Self::MatcherConfig => 0,
            Self::PushAuthMark => 1,
            Self::ConfigureAuthMark => 2,
            Self::PushEwmaMark => 3,
            Self::ConfigureEwmaMark => 4,
            Self::ConfigureHybridOracle => 5,
            Self::TradeFeePolicy => 6,
            Self::FeeRedirectPolicy => 7,
            Self::LiquidationFeePolicy => 8,
            Self::MaintenanceFeePolicy => 9,
            Self::MarketInitFeePolicy => 10,
            Self::ResolvePolicy => 11,
            Self::BackingFeePolicy => 12,
            Self::BackingFeePolicyShort => 13,
        }
    }
}

impl RetryIntentKind {
    pub const ALL: [Self; 11] = [
        Self::Deposit,
        Self::Withdraw,
        Self::TradeNoCpi,
        Self::TradeCpi,
        Self::BatchTradeNoCpi,
        Self::BatchTradeCpi,
        Self::ConvertReleasedPnl,
        Self::RebalanceReduce,
        Self::InsuranceTopUp,
        Self::BackingTopUp,
        Self::AssetActivation,
    ];

    fn discriminator(self) -> u8 {
        match self {
            Self::Deposit => 0,
            Self::Withdraw => 1,
            Self::TradeNoCpi => 2,
            Self::TradeCpi => 3,
            Self::BatchTradeNoCpi => 4,
            Self::BatchTradeCpi => 5,
            Self::ConvertReleasedPnl => 6,
            Self::RebalanceReduce => 7,
            Self::InsuranceTopUp => 8,
            Self::BackingTopUp => 9,
            Self::AssetActivation => 10,
        }
    }
}

impl AuthorityIntentKind {
    pub const ALL: [Self; 7] = [
        Self::MarketAuthorityHandoff,
        Self::AssetAdminHandoff,
        Self::InsuranceAuthorityHandoff,
        Self::InsuranceOperatorHandoff,
        Self::BackingAuthorityHandoff,
        Self::OracleAuthorityHandoff,
        Self::ResolveMarket,
    ];

    fn discriminator(self) -> u8 {
        match self {
            Self::MarketAuthorityHandoff => 0,
            Self::AssetAdminHandoff => 1,
            Self::InsuranceAuthorityHandoff => 2,
            Self::InsuranceOperatorHandoff => 3,
            Self::BackingAuthorityHandoff => 4,
            Self::OracleAuthorityHandoff => 5,
            Self::ResolveMarket => 6,
        }
    }

    fn asset_authority_kind(self) -> Option<u8> {
        match self {
            Self::AssetAdminHandoff => Some(percolator_prog::processor::ASSET_AUTH_ADMIN),
            Self::InsuranceAuthorityHandoff => {
                Some(percolator_prog::processor::ASSET_AUTH_INSURANCE)
            }
            Self::InsuranceOperatorHandoff => {
                Some(percolator_prog::processor::ASSET_AUTH_INSURANCE_OPERATOR)
            }
            Self::BackingAuthorityHandoff => {
                Some(percolator_prog::processor::ASSET_AUTH_BACKING_BUCKET)
            }
            Self::OracleAuthorityHandoff => Some(percolator_prog::processor::ASSET_AUTH_ORACLE),
            Self::MarketAuthorityHandoff | Self::ResolveMarket => None,
        }
    }
}

impl AssetIntentKind {
    pub const ALL: [Self; 15] = [
        Self::TradeNoCpi,
        Self::TradeCpi,
        Self::BatchTradeNoCpi,
        Self::BatchTradeCpi,
        Self::PushAuthMark,
        Self::PushEwmaMark,
        Self::ConfigureAuthMark,
        Self::ConfigureEwmaMark,
        Self::ConfigureHybridOracle,
        Self::InsuranceTopUp,
        Self::BackingTopUp,
        Self::InsuranceWithdrawal,
        Self::BackingFeePolicy,
        Self::ResolveMarket,
        Self::ResolvePolicy,
    ];

    fn discriminator(self) -> u8 {
        match self {
            Self::TradeNoCpi => 0,
            Self::TradeCpi => 1,
            Self::BatchTradeNoCpi => 2,
            Self::BatchTradeCpi => 3,
            Self::PushAuthMark => 4,
            Self::PushEwmaMark => 5,
            Self::ConfigureAuthMark => 6,
            Self::ConfigureEwmaMark => 7,
            Self::ConfigureHybridOracle => 8,
            Self::InsuranceTopUp => 9,
            Self::BackingTopUp => 10,
            Self::InsuranceWithdrawal => 11,
            Self::BackingFeePolicy => 12,
            Self::ResolveMarket => 13,
            Self::ResolvePolicy => 14,
        }
    }

    fn uses_actor_authorities(self) -> bool {
        matches!(
            self,
            Self::InsuranceTopUp
                | Self::BackingTopUp
                | Self::InsuranceWithdrawal
                | Self::BackingFeePolicy
        )
    }
}

impl MarketIntentKind {
    pub const ALL: [Self; 11] = [
        Self::Deposit,
        Self::MatcherEnable,
        Self::TradeFeePolicy,
        Self::FeeRedirectPolicy,
        Self::MaintenanceFeePolicy,
        Self::LiquidationFeePolicy,
        Self::ShutdownAsset,
        Self::ResolveMarket,
        Self::ResolvePolicy,
        Self::RebalanceReduce,
        Self::ForfeitRecoveryLeg,
    ];

    fn discriminator(self) -> u8 {
        match self {
            Self::Deposit => 0,
            Self::MatcherEnable => 1,
            Self::TradeFeePolicy => 2,
            Self::FeeRedirectPolicy => 3,
            Self::MaintenanceFeePolicy => 4,
            Self::LiquidationFeePolicy => 5,
            Self::ShutdownAsset => 6,
            Self::ResolveMarket => 7,
            Self::ResolvePolicy => 8,
            Self::RebalanceReduce => 9,
            Self::ForfeitRecoveryLeg => 10,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncarnationDiscovery {
    pub kind: PortfolioIntentKind,
    pub old_portfolio_id: u64,
    pub new_portfolio_id: u64,
    pub accepted_stale_intent: bool,
    pub mutated_economic_state: bool,
    pub compute_units: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarketIncarnationDiscovery {
    pub kind: MarketIntentKind,
    pub old_market_id: u64,
    pub new_market_id: u64,
    pub accepted_stale_intent: bool,
    pub mutated_economic_state: bool,
    pub compute_units: Option<u64>,
    pub public_trace: PublicTraceEvidence,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetGenerationDiscovery {
    pub kind: AssetIntentKind,
    pub old_asset_id: u64,
    pub new_asset_id: u64,
    pub accepted_stale_intent: bool,
    pub mutated_economic_state: bool,
    pub compute_units: Option<u64>,
    pub rejection_was_generation_mismatch: bool,
    pub fresh_intent_landed: bool,
    pub fresh_intent_mutated_economic_state: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalGenerationDiscovery {
    pub kind: TerminalGenerationKind,
    pub old_generation: u64,
    pub new_generation: u64,
    pub stale_intent_rejected: bool,
    pub exact_rollback: bool,
    pub rejection_was_generation_mismatch: bool,
    pub fresh_intent_landed: bool,
    pub victim_payout: u128,
    pub winner_payout: u128,
    pub victim_loss: u128,
    pub winner_gain: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PositionEpisodeDiscovery {
    pub kind: PositionEpisodeKind,
    pub stale_intent_rejected: bool,
    pub exact_rollback: bool,
    pub fresh_intent_landed: bool,
    pub portfolio_id_unchanged: bool,
    pub episode_advanced: bool,
    pub replacement_exposure_preserved: bool,
    pub fresh_intent_changed_exposure: bool,
    pub token_supply_preserved: bool,
}

impl PositionEpisodeDiscovery {
    pub fn satisfies_invariant(&self) -> bool {
        self.stale_intent_rejected
            && self.exact_rollback
            && self.fresh_intent_landed
            && self.portfolio_id_unchanged
            && self.episode_advanced
            && self.replacement_exposure_preserved
            && self.fresh_intent_changed_exposure
            && self.token_supply_preserved
    }
}

impl TerminalGenerationDiscovery {
    pub fn is_violation(&self) -> bool {
        self.victim_loss != 0
            && self.victim_loss == self.winner_gain
            && self.victim_payout < self.winner_payout
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorityIncarnationDiscovery {
    pub kind: AuthorityIntentKind,
    pub accepted_stale_intent: bool,
    pub mutated_economic_state: bool,
    pub compute_units: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FundedRoleDiscovery {
    pub kind: FundedRoleKind,
    pub provider_source_debit: u128,
    pub takeover_landed: bool,
    pub replacement_gain: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedAdlCloseDiscovery {
    pub order: ResolvedAdlCloseOrder,
    pub winner_basis_q: u128,
    pub effective_long_oi_q: u128,
    pub winner_funded_value: u128,
    pub canonical_vault_liquidity: u128,
    pub loser_close_landed: bool,
    pub winner_close_failures: u8,
    pub all_counter_underflow: bool,
    pub exact_rollback: bool,
    pub withdraw_rejected: bool,
    pub portfolio_close_rejected: bool,
    pub winner_external_payout: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StaleCohortNovationDiscovery {
    pub route: StaleCohortRoute,
    pub novation_landed: bool,
    pub pre_stale_long_count: u64,
    pub pre_stale_short_count: u64,
    pub pre_negative_pnl_count: u64,
    pub settlement_cranks: u8,
    pub winner_extracted: u64,
    pub entrant_extracted: u64,
    pub winner_profit: u128,
    pub entrant_principal_loss: u128,
    pub loser_principal_loss: u128,
    pub all_positions_terminal: bool,
    pub token_supply_conserved: bool,
}

impl StaleCohortNovationDiscovery {
    pub fn is_violation(&self) -> bool {
        self.novation_landed
            && self.settlement_cranks != 0
            && self.winner_profit != 0
            && self.entrant_principal_loss != 0
            && self.loser_principal_loss != 0
            && self.winner_profit
                == self
                    .entrant_principal_loss
                    .checked_add(self.loser_principal_loss)
                    .unwrap_or(u128::MAX)
            && self.all_positions_terminal
            && self.token_supply_conserved
    }
}

impl ResolvedAdlCloseDiscovery {
    pub fn is_violation(&self) -> bool {
        self.winner_basis_q > self.effective_long_oi_q
            && self.effective_long_oi_q != 0
            && self.winner_funded_value != 0
            && self.canonical_vault_liquidity >= self.winner_funded_value
            && self.loser_close_landed
            && self.winner_close_failures >= 8
            && self.all_counter_underflow
            && self.exact_rollback
            && self.withdraw_rejected
            && self.portfolio_close_rejected
            && self.winner_external_payout == 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntentReplayDiscovery {
    pub kind: RetryIntentKind,
    pub first_compute_units: u64,
    pub accepted_retry: bool,
    pub duplicated_economic_effect: bool,
    pub retry_compute_units: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeeConsentDiscovery {
    pub kind: FeeConsentKind,
    pub accepted_unconsented_terms: bool,
    pub mutated_economic_state: bool,
    pub authorized_debit: u128,
    pub observed_debit: u128,
    pub compute_units: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceFeeConsentDiscovery {
    pub kind: SourceFeeConsentKind,
    pub accepted_unconsented_fee: bool,
    pub lp_capital_debit: u128,
    pub provider_earnings_credit: u128,
    pub compute_units: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackingProviderConsentDiscovery {
    pub order: BackingProviderConsentOrder,
    pub accepted_provider_terms: bool,
    pub lp_capital_debit: u128,
    pub provider_earnings_credit: u128,
    pub operator_insurance_credit: u128,
    pub operator_withdrawn: u64,
    pub compute_units: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccrualOrderingDiscovery {
    pub kind: AccrualOrderingKind,
    pub unsafe_action_rejected: bool,
    pub rejected_exact_rollback: bool,
    pub retry_landed: bool,
    pub control_paid: u128,
    pub control_received: u128,
    pub reordered_paid: u128,
    pub reordered_received: u128,
    pub victim_claim_loss: u128,
    pub attacker_claim_gain: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalCommitOrderingDiscovery {
    pub committed_mark: u64,
    pub reordered_mark: u64,
    pub unsafe_resolve_rejected: bool,
    pub rejected_exact_rollback: bool,
    pub catchup_steps: u16,
    pub max_catchup_cu: u64,
    pub victim_payout_loss: u64,
    pub counterparty_payout_gain: u64,
    pub committed_total_payout: u128,
    pub reordered_total_payout: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingZeroMoveTerminalDiscovery {
    pub control_f_long_num: i128,
    pub control_f_short_num: i128,
    pub reordered_f_long_num: i128,
    pub reordered_f_short_num: i128,
    pub unsafe_resolve_rejected: bool,
    pub rejected_exact_rollback: bool,
    pub catchup_steps: u16,
    pub max_catchup_cu: u64,
    pub victim_payout_loss: u128,
    pub attacker_payout_gain: u128,
    pub control_total_payout: u128,
    pub reordered_total_payout: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShutdownCommitOrderingDiscovery {
    pub control_f_long_num: i128,
    pub control_f_short_num: i128,
    pub shutdown_f_long_num: i128,
    pub shutdown_f_short_num: i128,
    pub victim_payout_loss: u128,
    pub counterparty_payout_gain: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShutdownCatchupDiscovery {
    pub initial_shutdown_rejected: bool,
    pub rejected_exact_rollback: bool,
    pub catchup_steps: u16,
    pub max_catchup_cu: u64,
    pub retry_landed: bool,
    pub f_long_num: i128,
    pub f_short_num: i128,
    pub users_terminal: bool,
    pub total_payout: u128,
    pub token_supply_conserved: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProspectiveAccrualDiscovery {
    pub route: ProspectiveAccrualRoute,
    pub control_f_short_num: i128,
    pub reordered_f_short_num: i128,
    pub victim_payout_loss: u128,
    pub coalition_payout_gain: u128,
    pub control_total_payout: u128,
    pub reordered_total_payout: u128,
    pub final_mark: u64,
    pub final_effective_price: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingMarkAdmissionDiscovery {
    pub source: PendingMarkSource,
    pub published_target: u64,
    pub stale_engine_target: u64,
    pub committed_mark: u64,
    pub attacker_profit: u128,
    pub victim_loss: u128,
    pub extracted_tokens: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingMarkInheritanceDiscovery {
    pub route: DiscoveryTradeRoute,
    pub movement_cost: u128,
    pub pending_mark: u64,
    pub committed_mark: u64,
    pub victim_loss: u128,
    pub attacker_gain: u128,
    pub extracted_profit: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingTargetOverrideDiscovery {
    pub route: DiscoveryTradeRoute,
    pub control_target: u64,
    pub reordered_target: u64,
    pub movement_fee: u128,
    pub victim_payout_loss: u128,
    pub coalition_profit: u128,
    pub control_supply: u128,
    pub reordered_supply: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingMarkFeeOrderingDiscovery {
    pub pending_sync_rejected_lock: bool,
    pub pending_sync_exact_rollback: bool,
    pub control_reward: u128,
    pub reordered_reward: u128,
    pub control_winner_payout: u128,
    pub reordered_winner_payout: u128,
    pub control_victim_payout: u128,
    pub reordered_victim_payout: u128,
    pub extracted_reward: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarkMovementReserveDiscovery {
    pub route: DiscoveryTradeRoute,
    pub movement_fee: u128,
    pub withdrawn_reserve: u128,
    pub victim_loss: u128,
    pub coalition_gain: u128,
    pub committed_mark: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradeDrivenLiquidationDiscovery {
    pub mode: TradeDrivenMarkMode,
    pub route: ProspectiveAccrualRoute,
    pub movement_fee: u128,
    pub liquidation_reward: u128,
    pub victim_capital_loss: u128,
    pub oi_reduction_q: u128,
    pub coalition_profit: u128,
    pub extracted_tokens: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BilateralMarkFeeDiscovery {
    pub mode: TradeDrivenMarkMode,
    pub route: DiscoveryTradeRoute,
    pub setup_mark: u64,
    pub queued_mark: u64,
    pub coalition_equity_before: u128,
    pub coalition_excess: u128,
    pub victim_loss: u128,
    pub fee_counterparty_loss: u128,
    pub insurance_gain: u128,
    pub extracted_tokens: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompositeRoundingDiscovery {
    pub scale: CompositeRoundingScale,
    pub exact_mark: u64,
    pub rounded_target: u64,
    pub rounded_mark: u64,
    pub certified_liq_deficit: u128,
    pub victim_capital_loss: u128,
    pub oi_reduction_q: u128,
    pub cranker_reward: u128,
    pub extracted_tokens: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObservationOmissionDiscovery {
    pub omitted_landed: bool,
    pub omitted_rejected_nonprogress: bool,
    pub omitted_exact_rollback: bool,
    pub control_f_long_num: i128,
    pub control_f_short_num: i128,
    pub omitted_f_long_num: i128,
    pub omitted_f_short_num: i128,
    pub victim_payout_loss: u128,
    pub counterparty_payout_gain: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FractionalMovementDiscovery {
    pub target_price: u64,
    pub stalled_price: u64,
    pub successful_cranks: u16,
    pub rejected_stalls: u8,
    pub nonmoving_stalls: u8,
    pub long_overpayment: u128,
    pub short_underpayment: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompositeTimeCoherenceDiscovery {
    pub coherent_price: u64,
    pub skewed_target: u64,
    pub skewed_mark: u64,
    pub victim_capital_loss: u128,
    pub oi_reduction_q: u128,
    pub cranker_reward: u128,
    pub extracted_tokens: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HybridTerminalSnapshotDiscovery {
    pub stale_resolve_landed: bool,
    pub stale_terminal_mark: u64,
    pub current_terminal_mark: u64,
    pub victim_payout_loss: u128,
    pub counterparty_payout_gain: u128,
    pub stale_total_payout: u128,
    pub current_total_payout: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalDustDiscovery {
    pub route: ProspectiveAccrualRoute,
    pub attacker_loss: u128,
    pub victim_loss: u128,
    pub vault_remaining: u128,
    pub control_vault_remaining: u128,
    pub control_supply: u128,
    pub dust_supply: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrossDomainInsuranceDiscovery {
    pub unrelated_insurance_spent: u128,
    pub coalition_payout: u128,
    pub coalition_profit: u128,
    pub progress_calls: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrossDomainBackingDiscovery {
    pub unfunded_claim_before_num: u128,
    pub funded_claim_before_num: u128,
    pub funded_backing_consumed_num: u128,
    pub winner_capital_gain: u128,
    pub extracted_tokens: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrossDomainBDiscovery {
    pub b_target_num: u128,
    pub pnl_loss: u128,
    pub wrong_domain_reduction_num: u128,
    pub correct_domain_reduction_num: u128,
    pub reduction_steps: u8,
    pub affected_position_after_q: i128,
    pub principal_withdrawn: u128,
    pub token_supply_conserved: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FullRefreshDiscovery {
    pub omitted_rejected_nonprogress: bool,
    pub omitted_exact_rollback: bool,
    pub omitted_position_before_q: u128,
    pub omitted_position_after_q: u128,
    pub omitted_liq_deficit: u128,
    pub omitted_insurance_delta: u128,
    pub complete_position_before_q: u128,
    pub complete_position_after_q: u128,
    pub complete_liq_deficit: u128,
    pub complete_insurance_delta: u128,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackingExpiryCase {
    pub fee_bps: u16,
    pub expiry_offset: u8,
    pub mark_move_bps: u16,
    pub increase_divisor: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackingExpiryDiscovery {
    pub expiry_slot: u64,
    pub authenticated_slot: u64,
    pub engine_slot: u64,
    pub risk_increase_rejected_stale: bool,
    pub rejected_exact_rollback: bool,
    pub victim_capital_loss: u128,
    pub provider_earnings: u128,
    pub extracted_tokens: u64,
    pub risk_reduction_landed: bool,
    pub position_before_reduction_q: u128,
    pub position_after_reduction_q: u128,
    pub token_supply_conserved: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExpiredBackingTradeRouteDiscovery {
    pub route: DiscoveryTradeRoute,
    pub landing: BackingExpiryLanding,
    pub expiry_slot: u64,
    pub authenticated_slot: u64,
    pub engine_slot: u64,
    pub risk_increase_landed: bool,
    pub rejected_exact_rollback: bool,
    pub counterparty_lien_increase_num: u128,
    pub victim_capital_loss: u128,
    pub provider_earnings: u128,
    pub extracted_tokens: u64,
    pub risk_reduction_landed: bool,
    pub position_before_reduction_q: u128,
    pub position_after_reduction_q: u128,
    pub token_supply_conserved: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetainedMaturityKind {
    BackingTopUp,
}

impl RetainedMaturityKind {
    pub const ALL: [Self; 1] = [Self::BackingTopUp];

    fn discriminator(self) -> u8 {
        match self {
            Self::BackingTopUp => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetainedMaturityDiscovery {
    pub kind: RetainedMaturityKind,
    pub landing: BackingExpiryLanding,
    pub expiry_slot: u64,
    pub landing_slot: u64,
    pub retained_landed: bool,
    pub retained_rejected_expired: bool,
    pub retained_exact_rollback: bool,
    pub control_users_terminal: bool,
    pub delayed_users_terminal: bool,
    pub delayed_funded_value: u128,
    pub delayed_vault_liquidity: u128,
    pub delayed_close_failures: u16,
    pub delayed_progress_failures: u16,
    pub exact_rollback: bool,
    pub landing_provider_source_debit: u128,
    pub landing_vault_token_credit: u128,
    pub landing_internal_vault_credit: u128,
    pub landing_bucket_principal_credit_num: u128,
    pub provider_principal_consumed: u128,
    pub provider_recovery: u128,
    pub control_external_payout: u128,
    pub delayed_external_payout: u128,
    pub token_supply_conserved: bool,
}

impl RetainedMaturityDiscovery {
    pub fn rejects_expired_intent_and_preserves_terminal_progress(&self) -> bool {
        self.landing != BackingExpiryLanding::Before
            && self.landing_slot >= self.expiry_slot
            && !self.retained_landed
            && self.retained_rejected_expired
            && self.retained_exact_rollback
            && self.control_users_terminal
            && self.delayed_users_terminal
            && self.delayed_funded_value == 0
            && self.exact_rollback
            && self.landing_provider_source_debit == 0
            && self.landing_vault_token_credit == 0
            && self.landing_internal_vault_credit == 0
            && self.landing_bucket_principal_credit_num == 0
            && self.provider_principal_consumed == 0
            && self.provider_recovery == 0
            && self.control_external_payout != 0
            && self.delayed_external_payout == self.control_external_payout
            && self.token_supply_conserved
    }

    pub fn accepts_fresh_intent_and_preserves_terminal_progress(&self) -> bool {
        self.landing == BackingExpiryLanding::Before
            && self.landing_slot < self.expiry_slot
            && self.retained_landed
            && !self.retained_rejected_expired
            && !self.retained_exact_rollback
            && self.landing_provider_source_debit != 0
            && self.landing_vault_token_credit == self.landing_provider_source_debit
            && self.landing_internal_vault_credit == self.landing_provider_source_debit
            && self.landing_bucket_principal_credit_num
                == self.landing_provider_source_debit * BOUND_SCALE
            && self.control_users_terminal
            && self.delayed_users_terminal
            && self.delayed_funded_value == 0
            && self.exact_rollback
            && self.provider_principal_consumed == self.landing_provider_source_debit
            && self.provider_recovery == 0
            && self.control_external_payout != 0
            && self.delayed_external_payout >= self.control_external_payout
            && self.delayed_external_payout
                <= self.control_external_payout + self.landing_provider_source_debit
            && self.token_supply_conserved
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpiredBackingConsumerKind {
    ReleasedPnlConversion,
}

impl ExpiredBackingConsumerKind {
    pub const ALL: [Self; 1] = [Self::ReleasedPnlConversion];

    fn discriminator(self) -> u8 {
        match self {
            Self::ReleasedPnlConversion => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExpiredBackingConsumerDiscovery {
    pub kind: ExpiredBackingConsumerKind,
    pub landing: BackingExpiryLanding,
    pub expiry_slot: u64,
    pub authenticated_slot: u64,
    pub engine_slot: u64,
    pub released_pnl: u128,
    pub conversion_landed: bool,
    pub conversion_rejected_stale: bool,
    pub rejected_exact_rollback: bool,
    pub capital_credit: u128,
    pub consumed_backing_num: u128,
    pub extracted_tokens: u64,
    pub senior_capital_before: u128,
    pub senior_withdraw_landed: bool,
    pub senior_withdrawn_tokens: u64,
    pub token_supply_conserved: bool,
}

impl ExpiredBackingConsumerDiscovery {
    pub fn rejects_lapsed_conversion_and_preserves_senior_exit(&self) -> bool {
        self.landing != BackingExpiryLanding::Before
            && self.authenticated_slot >= self.expiry_slot
            && self.engine_slot < self.authenticated_slot
            && self.released_pnl != 0
            && !self.conversion_landed
            && self.conversion_rejected_stale
            && self.rejected_exact_rollback
            && self.capital_credit == 0
            && self.consumed_backing_num == 0
            && self.extracted_tokens == 0
            && self.senior_capital_before != 0
            && self.senior_withdraw_landed
            && u128::from(self.senior_withdrawn_tokens) == self.senior_capital_before
            && self.token_supply_conserved
    }

    pub fn consumes_fresh_backing_nonvacuously(&self) -> bool {
        self.landing == BackingExpiryLanding::Before
            && self.authenticated_slot < self.expiry_slot
            && self.released_pnl != 0
            && self.conversion_landed
            && !self.conversion_rejected_stale
            && !self.rejected_exact_rollback
            && self.capital_credit == self.released_pnl
            && self.consumed_backing_num == self.released_pnl * BOUND_SCALE
            && u128::from(self.extracted_tokens) == self.released_pnl
            && self.senior_capital_before != 0
            && self.senior_withdraw_landed
            && u128::from(self.senior_withdrawn_tokens) == self.senior_capital_before
            && self.token_supply_conserved
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceLienReversalExitRoute {
    PermissionlessCrank,
    RebalanceReduce,
    TradeNoCpi,
    BatchNoCpi,
    TradeCpi,
    BatchCpi,
}

impl SourceLienReversalExitRoute {
    pub const ALL: [Self; 6] = [
        Self::PermissionlessCrank,
        Self::RebalanceReduce,
        Self::TradeNoCpi,
        Self::BatchNoCpi,
        Self::TradeCpi,
        Self::BatchCpi,
    ];

    fn discriminator(self) -> u8 {
        match self {
            Self::PermissionlessCrank => 0,
            Self::RebalanceReduce => 1,
            Self::TradeNoCpi => 2,
            Self::BatchNoCpi => 3,
            Self::TradeCpi => 4,
            Self::BatchCpi => 5,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceLienReversalDiscovery {
    pub route: SourceLienReversalExitRoute,
    pub source_claim_liened_num: u128,
    pub funded_capital: u128,
    pub position_before_q: u128,
    pub position_after_q: u128,
    pub canonical_vault_liquidity: u128,
    pub attempts: u8,
    pub successful_calls: u8,
    pub lock_active_rejections: u8,
    pub rejection_errors: Vec<String>,
    pub exact_rollback: bool,
    pub external_payout: u64,
    pub token_supply_conserved: bool,
}

impl SourceLienReversalDiscovery {
    pub fn preserves_bounded_funded_exit(&self) -> bool {
        self.source_claim_liened_num != 0
            && self.funded_capital != 0
            && self.position_before_q != 0
            && self.position_after_q < self.position_before_q
            && self.canonical_vault_liquidity >= self.funded_capital
            && self.attempts >= 2
            && self.successful_calls != 0
            && self.successful_calls + self.lock_active_rejections <= self.attempts
            && self.exact_rollback
            && self.external_payout == 0
            && self.token_supply_conserved
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrossDomainRoundingOrder {
    Forward,
    Reverse,
}

impl CrossDomainRoundingOrder {
    pub const ALL: [Self; 2] = [Self::Forward, Self::Reverse];

    fn assets(self) -> [u16; 2] {
        match self {
            Self::Forward => [0, 1],
            Self::Reverse => [1, 0],
        }
    }

    fn discriminator(self) -> u8 {
        match self {
            Self::Forward => 0,
            Self::Reverse => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrossDomainRoundingDiscovery {
    pub order: CrossDomainRoundingOrder,
    pub fractional_source_domains: u8,
    pub positive_pnl_before_reversal: i128,
    pub funded_capital: u128,
    pub stranded_position_q: u128,
    pub blocked_public_routes: u8,
    pub later_honest_crank_blocked: bool,
    pub exact_rollback: bool,
    pub canonical_vault_liquidity: u128,
    pub token_supply_conserved: bool,
}

impl CrossDomainRoundingDiscovery {
    pub fn is_persistent_funded_exit_lock(&self) -> bool {
        self.fractional_source_domains == 2
            && self.positive_pnl_before_reversal > 0
            && self.funded_capital != 0
            && self.stranded_position_q != 0
            && self.blocked_public_routes == 6
            && self.later_honest_crank_blocked
            && self.exact_rollback
            && self.canonical_vault_liquidity >= self.funded_capital
            && self.token_supply_conserved
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlatSourceLienEscapeRoute {
    TradeNoCpi,
    BatchNoCpi,
    TradeCpi,
    BatchCpi,
}

impl FlatSourceLienEscapeRoute {
    pub const ALL: [Self; 4] = [
        Self::TradeNoCpi,
        Self::BatchNoCpi,
        Self::TradeCpi,
        Self::BatchCpi,
    ];

    fn discriminator(self) -> u8 {
        match self {
            Self::TradeNoCpi => 0,
            Self::BatchNoCpi => 1,
            Self::TradeCpi => 2,
            Self::BatchCpi => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlatSourceLienDiscovery {
    pub escape_route: FlatSourceLienEscapeRoute,
    pub provider_withdrawal: u128,
    pub flat_position_q: u128,
    pub positive_pnl: i128,
    pub source_claim_liened_num: u128,
    pub conversion_attempts: u8,
    pub conversion_rejections: u8,
    pub later_honest_crank_released_lien: bool,
    pub close_rejected: bool,
    pub round_trip_completed: bool,
    pub round_trip_released_claim: bool,
    pub exact_rollback: bool,
    pub canonical_vault_liquidity: u128,
    pub token_supply_conserved: bool,
}

impl FlatSourceLienDiscovery {
    pub fn is_persistent_backed_claim_lock(&self) -> bool {
        self.provider_withdrawal != 0
            && self.flat_position_q == 0
            && self.positive_pnl > 0
            && self.source_claim_liened_num != 0
            && self.conversion_attempts >= 3
            && self.conversion_rejections == self.conversion_attempts
            && !self.later_honest_crank_released_lien
            && self.close_rejected
            && !self.round_trip_released_claim
            && self.exact_rollback
            && self.canonical_vault_liquidity >= self.positive_pnl as u128
            && self.token_supply_conserved
    }
}

impl BackingExpiryDiscovery {
    pub fn preserves_expiry_normalization(&self) -> bool {
        self.authenticated_slot > self.expiry_slot
            && self.engine_slot < self.authenticated_slot
            && self.risk_increase_rejected_stale
            && self.rejected_exact_rollback
            && self.victim_capital_loss == 0
            && self.provider_earnings == 0
            && self.extracted_tokens == 0
            && self.risk_reduction_landed
            && self.position_after_reduction_q < self.position_before_reduction_q
            && self.token_supply_conserved
    }
}

impl ExpiredBackingTradeRouteDiscovery {
    pub fn preserves_risk_reduction(&self) -> bool {
        self.risk_reduction_landed
            && self.position_after_reduction_q < self.position_before_reduction_q
    }

    pub fn rejects_expired_risk_increase_safely(&self) -> bool {
        self.landing != BackingExpiryLanding::Before
            && self.authenticated_slot >= self.expiry_slot
            && self.engine_slot < self.authenticated_slot
            && !self.risk_increase_landed
            && self.rejected_exact_rollback
            && self.counterparty_lien_increase_num == 0
            && self.victim_capital_loss == 0
            && self.provider_earnings == 0
            && self.extracted_tokens == 0
            && self.token_supply_conserved
    }

    pub fn uses_fresh_backing_nonvacuously(&self) -> bool {
        let fee_path_is_nonvacuous = match self.route {
            DiscoveryTradeRoute::NoCpi | DiscoveryTradeRoute::Cpi => {
                self.victim_capital_loss > 0
                    && self.provider_earnings > 0
                    && u128::from(self.extracted_tokens) == self.provider_earnings
            }
            DiscoveryTradeRoute::BatchNoCpi | DiscoveryTradeRoute::BatchCpi => {
                self.victim_capital_loss == 0
                    && self.provider_earnings == 0
                    && self.extracted_tokens == 0
            }
        };
        self.landing == BackingExpiryLanding::Before
            && self.authenticated_slot < self.expiry_slot
            && self.risk_increase_landed
            && !self.rejected_exact_rollback
            && self.counterparty_lien_increase_num > 0
            && fee_path_is_nonvacuous
            && self.preserves_risk_reduction()
            && self.token_supply_conserved
    }
}

impl FullRefreshDiscovery {
    pub fn preserves_full_refresh_equivalence(&self) -> bool {
        self.omitted_rejected_nonprogress
            && self.omitted_exact_rollback
            && self.omitted_position_after_q == self.omitted_position_before_q
            && self.omitted_insurance_delta == 0
            && self.complete_position_before_q == self.complete_position_after_q
            && self.complete_liq_deficit == 0
            && self.complete_insurance_delta == 0
    }
}

impl CrossDomainBDiscovery {
    pub fn preserves_domain_locality_and_exit(&self) -> bool {
        self.b_target_num != 0
            && self.pnl_loss != 0
            && self.wrong_domain_reduction_num == 0
            && self.correct_domain_reduction_num
                == self.pnl_loss.saturating_mul(percolator::BOUND_SCALE)
            && self.reduction_steps != 0
            && self.affected_position_after_q == 0
            && self.principal_withdrawn != 0
            && self.token_supply_conserved
    }
}

impl CrossDomainBackingDiscovery {
    pub fn is_violation(&self) -> bool {
        self.unfunded_claim_before_num != 0
            && self.funded_claim_before_num == self.unfunded_claim_before_num
            && self.funded_backing_consumed_num == self.funded_claim_before_num.saturating_mul(2)
            && self.winner_capital_gain
                == self
                    .funded_backing_consumed_num
                    .checked_div(percolator::BOUND_SCALE)
                    .unwrap_or(0)
            && u128::from(self.extracted_tokens) > self.winner_capital_gain
    }
}

impl CrossDomainInsuranceDiscovery {
    pub fn is_violation(&self) -> bool {
        self.unrelated_insurance_spent != 0
            && self.coalition_profit != 0
            && self.progress_calls != 0
            && self.coalition_payout != 0
    }
}

impl TerminalDustDiscovery {
    pub fn is_violation(&self) -> bool {
        self.attacker_loss == 1
            && self.victim_loss != 0
            && self.control_vault_remaining == 0
            && self.vault_remaining == self.attacker_loss + self.victim_loss
            && self.control_supply == self.dust_supply
    }
}

impl CompositeTimeCoherenceDiscovery {
    pub fn is_violation(&self) -> bool {
        self.skewed_target > self.coherent_price
            && self.skewed_mark > self.coherent_price
            && self.victim_capital_loss != 0
            && self.oi_reduction_q != 0
            && self.cranker_reward != 0
            && self.cranker_reward == u128::from(self.extracted_tokens)
    }
}

impl HybridTerminalSnapshotDiscovery {
    pub fn is_violation(&self) -> bool {
        self.stale_resolve_landed
            && self.current_terminal_mark > self.stale_terminal_mark
            && self.victim_payout_loss != 0
            && self.victim_payout_loss == self.counterparty_payout_gain
            && self.stale_total_payout == self.current_total_payout
    }
}

impl FractionalMovementDiscovery {
    pub fn is_violation(&self) -> bool {
        self.successful_cranks != 0
            && self.stalled_price > self.target_price
            && (self.rejected_stalls >= 3 || self.nonmoving_stalls >= 3)
            && self.long_overpayment != 0
            && self.long_overpayment == self.short_underpayment
    }
}

impl ObservationOmissionDiscovery {
    pub fn preserves_rounded_transfer(&self) -> bool {
        !self.omitted_landed
            && self.omitted_rejected_nonprogress
            && self.omitted_exact_rollback
            && self.control_f_long_num > 0
            && self.control_f_short_num < 0
            && self.omitted_f_long_num == self.control_f_long_num
            && self.omitted_f_short_num == self.control_f_short_num
            && self.victim_payout_loss == 0
            && self.counterparty_payout_gain == 0
    }
}

impl CompositeRoundingDiscovery {
    pub fn is_violation(&self) -> bool {
        self.rounded_target != self.exact_mark
            && self.rounded_mark != self.exact_mark
            && self.certified_liq_deficit != 0
            && self.victim_capital_loss != 0
            && self.oi_reduction_q != 0
            && self.cranker_reward != 0
            && self.cranker_reward == u128::from(self.extracted_tokens)
    }
}

impl BilateralMarkFeeDiscovery {
    pub fn is_violation(&self) -> bool {
        self.queued_mark > self.setup_mark
            && self.coalition_excess != 0
            && self.victim_loss != 0
            && self.fee_counterparty_loss != 0
            && self.insurance_gain != 0
            && self.extracted_tokens != 0
    }
}

impl TradeDrivenLiquidationDiscovery {
    pub fn is_violation(&self) -> bool {
        self.movement_fee != 0
            && self.liquidation_reward > self.movement_fee
            && self.victim_capital_loss != 0
            && self.oi_reduction_q != 0
            && self.coalition_profit != 0
            && self.extracted_tokens != 0
    }
}

impl MarkMovementReserveDiscovery {
    pub fn is_violation(&self) -> bool {
        self.movement_fee != 0
            && self.withdrawn_reserve == self.movement_fee
            && self.victim_loss != 0
            && self.coalition_gain != 0
            && self.coalition_gain < self.victim_loss
            && self.coalition_gain.checked_add(1) == Some(self.victim_loss)
    }
}

impl PendingMarkFeeOrderingDiscovery {
    pub fn rejects_pending_sync_and_preserves_terminal_value(&self) -> bool {
        self.pending_sync_rejected_lock
            && self.pending_sync_exact_rollback
            && self.reordered_reward == self.control_reward
            && self.reordered_winner_payout == self.control_winner_payout
            && self.reordered_victim_payout == self.control_victim_payout
            && self.extracted_reward as u128 == self.reordered_reward
    }
}

impl PendingTargetOverrideDiscovery {
    pub fn is_violation(&self) -> bool {
        self.reordered_target < self.control_target
            && self.movement_fee != 0
            && self.victim_payout_loss != 0
            && self.coalition_profit != 0
            && self.movement_fee < self.victim_payout_loss
            && self.control_supply == self.reordered_supply
    }
}

impl PendingMarkInheritanceDiscovery {
    pub fn is_violation(&self) -> bool {
        self.pending_mark == self.committed_mark
            && self.movement_cost != 0
            && self.attacker_gain == self.victim_loss
            && self.attacker_gain > self.movement_cost
            && u128::from(self.extracted_profit) == self.attacker_gain - self.movement_cost
    }
}

impl PendingMarkAdmissionDiscovery {
    pub fn is_violation(&self) -> bool {
        self.published_target > self.stale_engine_target
            && self.committed_mark == self.published_target
            && self.attacker_profit != 0
            && self.attacker_profit == self.victim_loss
            && self.attacker_profit == u128::from(self.extracted_tokens)
    }
}

impl ProspectiveAccrualDiscovery {
    pub fn is_violation(&self) -> bool {
        self.control_f_short_num > 0
            && self.reordered_f_short_num == 0
            && self.victim_payout_loss != 0
            && self.victim_payout_loss == self.coalition_payout_gain
            && self.control_total_payout == self.reordered_total_payout
    }
}

impl TerminalCommitOrderingDiscovery {
    pub fn is_violation(&self) -> bool {
        self.committed_mark > self.reordered_mark
            && self.victim_payout_loss != 0
            && self.victim_payout_loss == self.counterparty_payout_gain
            && self.committed_total_payout == self.reordered_total_payout
    }
}

impl PendingZeroMoveTerminalDiscovery {
    pub fn is_violation(&self) -> bool {
        self.control_f_long_num != self.reordered_f_long_num
            || self.control_f_short_num != self.reordered_f_short_num
            || self.victim_payout_loss != 0
            || self.attacker_payout_gain != 0
            || self.control_total_payout != self.reordered_total_payout
    }
}

impl ShutdownCommitOrderingDiscovery {
    pub fn is_violation(&self) -> bool {
        self.control_f_long_num > 0
            && self.control_f_short_num < 0
            && self.shutdown_f_long_num == 0
            && self.shutdown_f_short_num == 0
            && self.victim_payout_loss != 0
            && self.victim_payout_loss == self.counterparty_payout_gain
    }
}

impl AccrualOrderingDiscovery {
    pub fn is_violation(&self) -> bool {
        let omitted = self.control_paid.checked_sub(self.reordered_paid);
        self.control_paid != 0
            && self.control_paid == self.control_received
            && self.reordered_paid == self.reordered_received
            && matches!(omitted, Some(value) if value != 0
                && value == self.victim_claim_loss
                && value == self.attacker_claim_gain)
    }
}

impl BackingProviderConsentDiscovery {
    pub fn is_violation(&self) -> bool {
        self.accepted_provider_terms
            && self.lp_capital_debit != 0
            && self.provider_earnings_credit == 0
            && self.operator_insurance_credit == self.lp_capital_debit
            && u128::from(self.operator_withdrawn) == self.lp_capital_debit
    }
}

impl SourceFeeConsentDiscovery {
    pub fn is_violation(&self) -> bool {
        self.accepted_unconsented_fee
            && self.lp_capital_debit != 0
            && self.lp_capital_debit == self.provider_earnings_credit
    }
}

impl FeeConsentDiscovery {
    pub fn is_violation(&self) -> bool {
        self.accepted_unconsented_terms
            && self.mutated_economic_state
            && self.observed_debit > self.authorized_debit
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SupersessionDiscovery {
    pub kind: SupersededIntentKind,
    pub accepted_stale_intent: bool,
    pub overwrote_newer_state: bool,
    pub compute_units: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatcherMutationOrderDiscovery {
    pub revoked_trade_rejected: bool,
    pub stale_enable_rejected: bool,
    pub stale_enable_exact_rollback: bool,
    pub post_stale_trade_rejected: bool,
    pub fresh_enable_landed: bool,
    pub fresh_round_trip_landed: bool,
    pub sequence_before_revoke: u64,
    pub sequence_after_revoke: u64,
    pub sequence_after_stale: u64,
    pub sequence_after_fresh: u64,
    pub total_payout: u128,
    pub token_supply_conserved: bool,
}

impl MatcherMutationOrderDiscovery {
    pub fn satisfies_invariant(&self) -> bool {
        self.revoked_trade_rejected
            && self.stale_enable_rejected
            && self.stale_enable_exact_rollback
            && self.post_stale_trade_rejected
            && self.fresh_enable_landed
            && self.fresh_round_trip_landed
            && self.sequence_after_revoke == self.sequence_before_revoke + 1
            && self.sequence_after_stale == self.sequence_after_revoke
            && self.sequence_after_fresh == self.sequence_after_revoke + 1
            && self.total_payout == 2_000_000
            && self.token_supply_conserved
    }
}

impl SupersessionDiscovery {
    pub fn is_violation(&self) -> bool {
        self.accepted_stale_intent && self.overwrote_newer_state
    }
}

impl IntentReplayDiscovery {
    pub fn is_violation(&self) -> bool {
        self.accepted_retry && self.duplicated_economic_effect
    }
}

impl AuthorityIncarnationDiscovery {
    pub fn is_violation(&self) -> bool {
        self.accepted_stale_intent && self.mutated_economic_state
    }
}

impl FundedRoleDiscovery {
    pub fn is_violation(&self) -> bool {
        self.takeover_landed
            && self.provider_source_debit != 0
            && self.replacement_gain == self.provider_source_debit
    }
}

impl AssetGenerationDiscovery {
    pub fn is_violation(&self) -> bool {
        self.accepted_stale_intent && self.mutated_economic_state
    }
}

impl MarketIncarnationDiscovery {
    pub fn is_violation(&self) -> bool {
        self.accepted_stale_intent && self.mutated_economic_state
    }
}

impl IncarnationDiscovery {
    pub fn is_violation(&self) -> bool {
        self.accepted_stale_intent && self.mutated_economic_state
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AccountFingerprint {
    lamports: u64,
    owner: [u8; 32],
    executable: bool,
    rent_epoch: u64,
    data: Vec<u8>,
}

impl From<Account> for AccountFingerprint {
    fn from(account: Account) -> Self {
        Self {
            lamports: account.lamports,
            owner: account.owner.to_bytes(),
            executable: account.executable,
            rent_epoch: account.rent_epoch,
            data: account.data,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EconomicFingerprint {
    market: Option<AccountFingerprint>,
    foreign_market: Option<AccountFingerprint>,
    backing_domain_ledger: Option<AccountFingerprint>,
    mint: Option<AccountFingerprint>,
    portfolios: Vec<Option<AccountFingerprint>>,
    token_accounts: Vec<Option<AccountFingerprint>>,
    matcher_contexts: Vec<Option<AccountFingerprint>>,
    token_supply: u128,
}

fn account_fingerprint(
    env: &V16Svm,
    key: &solana_sdk::pubkey::Pubkey,
) -> Option<AccountFingerprint> {
    env.svm.get_account(key).map(Into::into)
}

fn fingerprint(env: &V16Svm) -> EconomicFingerprint {
    let mut portfolio_keys: Vec<_> = env.actors.iter().map(|actor| actor.portfolio).collect();
    portfolio_keys.push(env.foreign_actor.portfolio);

    let mut token_keys = vec![
        env.vault,
        env.foreign_vault,
        env.provider_source_token,
        env.provider_destination_token,
        env.market_admin_destination_token,
        env.foreign_actor.source_token,
        env.foreign_actor.destination_token,
    ];
    for actor in &env.actors {
        token_keys.extend([actor.source_token, actor.destination_token]);
    }

    EconomicFingerprint {
        market: account_fingerprint(env, &env.market),
        foreign_market: account_fingerprint(env, &env.foreign_market),
        backing_domain_ledger: account_fingerprint(env, &env.backing_domain_ledger),
        mint: account_fingerprint(env, &env.mint),
        portfolios: portfolio_keys
            .iter()
            .map(|key| account_fingerprint(env, key))
            .collect(),
        token_accounts: token_keys
            .iter()
            .map(|key| account_fingerprint(env, key))
            .collect(),
        matcher_contexts: env
            .actors
            .iter()
            .map(|actor| account_fingerprint(env, &actor.matcher_context))
            .collect(),
        token_supply: env.token_supply_observed(),
    }
}

fn retained_portfolio_intent(env: &mut V16Svm, kind: PortfolioIntentKind) -> Transaction {
    const SUBJECT: usize = 0;
    const COUNTERPARTY: usize = 1;
    const AMOUNT: u128 = 1_000;
    let size_q = POS_SCALE as i128 / 4;
    match kind {
        PortfolioIntentKind::Deposit => env.build_retained_deposit(SUBJECT, AMOUNT),
        PortfolioIntentKind::Withdraw => env.build_retained_withdrawal(SUBJECT, AMOUNT),
        PortfolioIntentKind::Close => env.build_retained_close_primary_portfolio(SUBJECT),
        PortfolioIntentKind::MatcherDisable => env.build_retained_matcher_config(SUBJECT, 0),
        PortfolioIntentKind::TradeNoCpi => {
            env.build_retained_no_cpi_trade(SUBJECT, COUNTERPARTY, 0, size_q, INITIAL_PRICE)
        }
        PortfolioIntentKind::TradeCpi => {
            env.build_retained_cpi_trade(SUBJECT, COUNTERPARTY, 0, size_q, 0)
        }
        PortfolioIntentKind::BatchTradeNoCpi => {
            env.build_retained_batch_no_cpi_trade(SUBJECT, COUNTERPARTY, 0, size_q, INITIAL_PRICE)
        }
        PortfolioIntentKind::BatchTradeCpi => {
            env.build_retained_batch_cpi_trade(SUBJECT, COUNTERPARTY, 0, size_q, 0)
        }
        PortfolioIntentKind::ConvertReleasedPnl
        | PortfolioIntentKind::RebalanceReduce
        | PortfolioIntentKind::ForfeitRecoveryLeg => {
            unreachable!("lifecycle-bearing portfolio intents use dedicated setup")
        }
    }
}

fn replacement_capital(kind: PortfolioIntentKind) -> u128 {
    match kind {
        PortfolioIntentKind::Close | PortfolioIntentKind::Deposit => 0,
        _ => USER_DEPOSIT,
    }
}

fn finish_portfolio_incarnation_discovery(
    env: &mut V16Svm,
    kind: PortfolioIntentKind,
    old_portfolio_id: u64,
    new_portfolio_id: u64,
    retained: Transaction,
    supply_before: u128,
) -> Result<IncarnationDiscovery, String> {
    if new_portfolio_id <= old_portfolio_id {
        return Err(format!(
            "portfolio incarnation did not advance: {old_portfolio_id} -> {new_portfolio_id}"
        ));
    }
    let before = fingerprint(env);
    let result = env.land_retained(retained);
    let after = fingerprint(env);
    if env.token_supply_observed() != supply_before {
        return Err(format!(
            "{kind:?} incarnation probe changed SPL supply: {supply_before} -> {}",
            env.token_supply_observed()
        ));
    }

    match result {
        Ok(success) => {
            let mutated_economic_state = before != after;
            if !mutated_economic_state {
                return Err(format!(
                    "{kind:?} stale transaction succeeded without an observable state delta"
                ));
            }
            Ok(IncarnationDiscovery {
                kind,
                old_portfolio_id,
                new_portfolio_id,
                accepted_stale_intent: true,
                mutated_economic_state,
                compute_units: Some(success.compute_units),
            })
        }
        Err(_) => {
            if before != after {
                return Err(format!(
                    "{kind:?} rejected stale transaction did not roll back exactly"
                ));
            }
            Ok(IncarnationDiscovery {
                kind,
                old_portfolio_id,
                new_portfolio_id,
                accepted_stale_intent: false,
                mutated_economic_state: false,
                compute_units: None,
            })
        }
    }
}

fn create_released_pnl(
    env: &mut V16Svm,
    winner: usize,
    loser: usize,
    winner_deposit: u128,
    loser_deposit: u128,
    mark_slot: u64,
    start_price: u64,
) -> Result<u128, String> {
    const POSITION_Q: i128 = 20 * POS_SCALE as i128;
    env.deposit_primary(winner, winner_deposit)
        .map_err(|error| format!("fund PnL winner: {error}"))?;
    env.deposit_primary(loser, loser_deposit)
        .map_err(|error| format!("fund PnL loser: {error}"))?;
    env.trade_no_cpi(winner, loser, 0, POSITION_Q, start_price, 0)
        .map_err(|error| format!("open PnL-producing trade: {error}"))?;
    env.warp_to_slot(mark_slot);
    env.push_auth_mark(0, mark_slot, start_price + 5)
        .map_err(|error| format!("publish PnL-producing mark: {error}"))?;
    for actor in [loser, winner] {
        env.crank(
            actor,
            mark_slot,
            vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: 0,
            }],
        )
        .map_err(|error| format!("refresh PnL actor {actor}: {error}"))?;
    }
    env.trade_no_cpi(winner, loser, 0, -POSITION_Q, start_price + 5, 0)
        .map_err(|error| format!("close PnL-producing trade: {error}"))?;
    let released = env.primary_portfolio(winner).pnl.get();
    if released <= 0 {
        return Err(format!("expected positive released PnL, got {released}"));
    }
    u128::try_from(released).map_err(|_| "released PnL conversion overflow".to_string())
}

fn discover_convert_portfolio_incarnation_replay(
    seed: [u8; 32],
) -> Result<IncarnationDiscovery, String> {
    const SUBJECT: usize = 1;
    const OLD_COUNTERPARTY: usize = 0;
    const NEW_COUNTERPARTY: usize = 3;
    const PRICE: u64 = 100;
    const TARGET_CAPITAL: u128 = 1_000_000;
    let kind = PortfolioIntentKind::ConvertReleasedPnl;
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            h_max: 10,
            min_nonzero_mm_req: 1,
            min_nonzero_im_req: 2,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [1; PRIMARY_ACTOR_COUNT],
            actor_token_balances: [2_000_000, 3_000_000, 10_000, 2_000_000, 10],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.top_up_backing_bucket(1, 300, 1_000)
        .map_err(|error| format!("fund short-side backing: {error}"))?;
    let released = create_released_pnl(
        &mut env,
        SUBJECT,
        OLD_COUNTERPARTY,
        TARGET_CAPITAL - 1,
        TARGET_CAPITAL - 1,
        2,
        PRICE,
    )?;
    let old_portfolio_id = env.primary_portfolio_id(SUBJECT);
    let retained = env.build_retained_convert_released_pnl(SUBJECT, u128::MAX);
    env.convert_released_pnl(SUBJECT, released)
        .map_err(|error| format!("consume old-incarnation PnL: {error}"))?;
    let capital = env.primary_portfolio(SUBJECT).capital.get();
    env.withdraw_primary(SUBJECT, capital)
        .map_err(|error| format!("empty old PnL portfolio: {error}"))?;
    env.close_primary_portfolio(SUBJECT)
        .map_err(|error| format!("close old PnL portfolio: {error}"))?;
    env.fund_closed_primary_portfolio(SUBJECT, 1_000_000_000)
        .map_err(|error| format!("fund replacement PnL portfolio: {error}"))?;
    env.reinitialize_primary_portfolio(SUBJECT)
        .map_err(|error| format!("initialize replacement PnL portfolio: {error}"))?;
    let new_portfolio_id = env.primary_portfolio_id(SUBJECT);
    create_released_pnl(
        &mut env,
        SUBJECT,
        NEW_COUNTERPARTY,
        TARGET_CAPITAL,
        TARGET_CAPITAL - 1,
        3,
        PRICE + 5,
    )?;
    finish_portfolio_incarnation_discovery(
        &mut env,
        kind,
        old_portfolio_id,
        new_portfolio_id,
        retained,
        supply_before,
    )
}

fn discover_rebalance_portfolio_incarnation_replay(
    seed: [u8; 32],
) -> Result<IncarnationDiscovery, String> {
    const SUBJECT: usize = 0;
    const COUNTERPARTY: usize = 1;
    const PRICE: u64 = 100;
    const DEPOSIT: u128 = 1_000_000;
    const OLD_SIZE_Q: i128 = 2_000 * POS_SCALE as i128;
    const NEW_SIZE_Q: i128 = 1_000 * POS_SCALE as i128;
    let kind = PortfolioIntentKind::RebalanceReduce;
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            actor_deposits: [DEPOSIT, DEPOSIT, 1, 1, 1],
            actor_token_balances: [2_000_000, 2_000_000, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.trade_no_cpi(SUBJECT, COUNTERPARTY, 0, OLD_SIZE_Q, PRICE, 0)
        .map_err(|error| format!("open old rebalance position: {error}"))?;
    let old_portfolio_id = env.primary_portfolio_id(SUBJECT);
    let retained = env.build_retained_rebalance_reduce(SUBJECT, 0, NEW_SIZE_Q.unsigned_abs());
    env.trade_no_cpi(SUBJECT, COUNTERPARTY, 0, -OLD_SIZE_Q, PRICE, 0)
        .map_err(|error| format!("close old rebalance position: {error}"))?;
    env.withdraw_primary(SUBJECT, DEPOSIT)
        .map_err(|error| format!("empty old rebalance portfolio: {error}"))?;
    env.close_primary_portfolio(SUBJECT)
        .map_err(|error| format!("close old rebalance portfolio: {error}"))?;
    env.fund_closed_primary_portfolio(SUBJECT, 1_000_000_000)
        .map_err(|error| format!("fund replacement rebalance portfolio: {error}"))?;
    env.reinitialize_primary_portfolio(SUBJECT)
        .map_err(|error| format!("initialize replacement rebalance portfolio: {error}"))?;
    let new_portfolio_id = env.primary_portfolio_id(SUBJECT);
    env.deposit_primary(SUBJECT, DEPOSIT)
        .map_err(|error| format!("fund replacement rebalance portfolio: {error}"))?;
    env.trade_no_cpi(SUBJECT, COUNTERPARTY, 0, NEW_SIZE_Q, PRICE, 0)
        .map_err(|error| format!("open replacement rebalance position: {error}"))?;
    finish_portfolio_incarnation_discovery(
        &mut env,
        kind,
        old_portfolio_id,
        new_portfolio_id,
        retained,
        supply_before,
    )
}

fn discover_forfeit_portfolio_incarnation_replay(
    seed: [u8; 32],
) -> Result<IncarnationDiscovery, String> {
    const SUBJECT: usize = 0;
    const COUNTERPARTY: usize = 1;
    const PRICE: u64 = 100;
    const DEPOSIT: u128 = 1_000_000;
    const SIZE_Q: i128 = 5_000 * POS_SCALE as i128;
    let kind = PortfolioIntentKind::ForfeitRecoveryLeg;
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [DEPOSIT, DEPOSIT, 1, 1, 1],
            actor_token_balances: [2_000_000, 2_000_000, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.configure_permissionless_resolve(100, 1)
        .map_err(|error| format!("configure recovery lifecycle: {error}"))?;
    env.trade_no_cpi(SUBJECT, COUNTERPARTY, 0, SIZE_Q, PRICE, 0)
        .map_err(|error| format!("open old recovery leg: {error}"))?;
    env.warp_to_slot(2);
    env.shutdown_asset(0, 2)
        .map_err(|error| format!("shutdown old asset generation: {error}"))?;
    let old_portfolio_id = env.primary_portfolio_id(SUBJECT);
    let retained = env.build_retained_forfeit_recovery_leg(SUBJECT, 0, 1);
    for actor in [SUBJECT, COUNTERPARTY] {
        env.forfeit_recovery_leg(actor, 0, 1)
            .map_err(|error| format!("clear old recovery leg for actor {actor}: {error}"))?;
    }
    env.withdraw_primary(SUBJECT, DEPOSIT)
        .map_err(|error| format!("empty old recovery portfolio: {error}"))?;
    env.close_primary_portfolio(SUBJECT)
        .map_err(|error| format!("close old recovery portfolio: {error}"))?;
    env.warp_to_slot(3);
    env.restart_asset_oracle(0, 3, PRICE)
        .map_err(|error| format!("restart asset for replacement recovery leg: {error}"))?;
    env.configure_auth_mark(false, 0, 3, PRICE)
        .map_err(|error| format!("configure replacement recovery mark: {error}"))?;
    env.fund_closed_primary_portfolio(SUBJECT, 1_000_000_000)
        .map_err(|error| format!("fund replacement recovery portfolio: {error}"))?;
    env.reinitialize_primary_portfolio(SUBJECT)
        .map_err(|error| format!("initialize replacement recovery portfolio: {error}"))?;
    let new_portfolio_id = env.primary_portfolio_id(SUBJECT);
    env.deposit_primary(SUBJECT, DEPOSIT)
        .map_err(|error| format!("fund replacement recovery portfolio: {error}"))?;
    env.trade_no_cpi(SUBJECT, COUNTERPARTY, 0, SIZE_Q, PRICE, 0)
        .map_err(|error| format!("open replacement recovery leg: {error}"))?;
    env.warp_to_slot(4);
    env.shutdown_asset(0, 4)
        .map_err(|error| format!("shutdown replacement asset generation: {error}"))?;
    finish_portfolio_incarnation_discovery(
        &mut env,
        kind,
        old_portfolio_id,
        new_portfolio_id,
        retained,
        supply_before,
    )
}

fn discover_one_portfolio_incarnation_replay(
    mut seed: [u8; 32],
    kind: PortfolioIntentKind,
) -> Result<IncarnationDiscovery, String> {
    const SUBJECT: usize = 0;
    seed[0] ^= 0xa3;
    seed[1] ^= kind as u8;
    match kind {
        PortfolioIntentKind::ConvertReleasedPnl => {
            return discover_convert_portfolio_incarnation_replay(seed);
        }
        PortfolioIntentKind::RebalanceReduce => {
            return discover_rebalance_portfolio_incarnation_replay(seed);
        }
        PortfolioIntentKind::ForfeitRecoveryLeg => {
            return discover_forfeit_portfolio_incarnation_replay(seed);
        }
        _ => {}
    }
    let mut env = V16Svm::new(seed, MarketConfig::default());
    let supply_before = env.token_supply_observed();
    let old_portfolio_id = env.primary_portfolio_id(SUBJECT);
    let retained = retained_portfolio_intent(&mut env, kind);

    let old_capital = env.primary_portfolio(SUBJECT).capital.get();
    env.withdraw_primary(SUBJECT, old_capital)
        .map_err(|error| format!("empty old portfolio: {error}"))?;
    env.close_primary_portfolio(SUBJECT)
        .map_err(|error| format!("close old portfolio: {error}"))?;
    env.fund_closed_primary_portfolio(SUBJECT, 1_000_000_000)
        .map_err(|error| format!("fund replacement portfolio: {error}"))?;
    env.reinitialize_primary_portfolio(SUBJECT)
        .map_err(|error| format!("initialize replacement portfolio: {error}"))?;
    let new_portfolio_id = env.primary_portfolio_id(SUBJECT);
    let replacement_capital = replacement_capital(kind);
    if replacement_capital != 0 {
        env.deposit_primary(SUBJECT, replacement_capital)
            .map_err(|error| format!("fund replacement portfolio: {error}"))?;
    }
    if kind == PortfolioIntentKind::MatcherDisable {
        env.set_matcher_config(SUBJECT, 1)
            .map_err(|error| format!("establish replacement matcher policy: {error}"))?;
    }

    finish_portfolio_incarnation_discovery(
        &mut env,
        kind,
        old_portfolio_id,
        new_portfolio_id,
        retained,
        supply_before,
    )
}

pub fn discover_portfolio_incarnation_replays(
    seed: [u8; 32],
) -> Result<Vec<IncarnationDiscovery>, String> {
    PortfolioIntentKind::ALL
        .into_iter()
        .map(|kind| discover_one_portfolio_incarnation_replay(seed, kind))
        .collect()
}

fn retained_market_intent(env: &mut V16Svm, kind: MarketIntentKind) -> Transaction {
    const SUBJECT: usize = 0;
    match kind {
        MarketIntentKind::Deposit => env.build_retained_deposit(SUBJECT, 1_000),
        MarketIntentKind::MatcherEnable => env.build_retained_matcher_config(SUBJECT, 1),
        MarketIntentKind::TradeFeePolicy => env.build_retained_trade_fee_policy(10_000),
        MarketIntentKind::FeeRedirectPolicy => env.build_retained_fee_redirect_policy(10_000),
        MarketIntentKind::MaintenanceFeePolicy => env.build_retained_maintenance_fee_policy(10_000),
        MarketIntentKind::LiquidationFeePolicy => env.build_retained_liquidation_fee_policy(10_000),
        MarketIntentKind::ShutdownAsset => env.build_retained_shutdown_asset(0, 12),
        MarketIntentKind::ResolveMarket => env.build_retained_resolve_market(),
        MarketIntentKind::ResolvePolicy => env.build_retained_permissionless_resolve_policy(17, 29),
        MarketIntentKind::RebalanceReduce | MarketIntentKind::ForfeitRecoveryLeg => {
            unreachable!("lifecycle-bearing market intents use dedicated setup")
        }
    }
}

fn publicly_recreate_market(
    env: &mut V16Svm,
    config: MarketConfig,
    reinit_slot: u64,
) -> Result<(), String> {
    for actor in 0..PRIMARY_ACTOR_COUNT {
        let capital = env.primary_portfolio(actor).capital.get();
        env.withdraw_primary(actor, capital)
            .map_err(|error| format!("empty old-market portfolio {actor}: {error}"))?;
        env.close_primary_portfolio(actor)
            .map_err(|error| format!("close old-market portfolio {actor}: {error}"))?;
    }
    env.resolve_market()
        .map_err(|error| format!("resolve old market: {error}"))?;
    env.close_primary_slab()
        .map_err(|error| format!("close old market: {error}"))?;
    env.warp_to_slot(reinit_slot);
    env.fund_closed_primary_market()
        .map_err(|error| format!("fund replacement market: {error}"))?;
    env.recreate_primary_vault()
        .map_err(|error| format!("recreate replacement vault: {error}"))?;
    env.reinitialize_primary_market(config)
        .map_err(|error| format!("initialize replacement market: {error}"))?;
    Ok(())
}

fn finish_market_incarnation_discovery(
    env: &mut V16Svm,
    kind: MarketIntentKind,
    old_market_id: u64,
    new_market_id: u64,
    retained: Transaction,
    supply_before: u128,
) -> Result<MarketIncarnationDiscovery, String> {
    let before = fingerprint(env);
    let result = env.land_retained(retained);
    let after = fingerprint(env);
    if env.token_supply_observed() != supply_before {
        return Err(format!(
            "{kind:?} market-incarnation probe changed SPL supply: {supply_before} -> {}",
            env.token_supply_observed()
        ));
    }
    let public_trace = env.finish_public_trace();

    match result {
        Ok(success) => {
            let mutated_economic_state = before != after;
            if !mutated_economic_state {
                return Err(format!(
                    "{kind:?} stale market transaction succeeded without an observable state delta"
                ));
            }
            Ok(MarketIncarnationDiscovery {
                kind,
                old_market_id,
                new_market_id,
                accepted_stale_intent: true,
                mutated_economic_state,
                compute_units: Some(success.compute_units),
                public_trace,
            })
        }
        Err(_) => {
            if before != after {
                return Err(format!(
                    "{kind:?} rejected stale market transaction did not roll back exactly"
                ));
            }
            Ok(MarketIncarnationDiscovery {
                kind,
                old_market_id,
                new_market_id,
                accepted_stale_intent: false,
                mutated_economic_state: false,
                compute_units: None,
                public_trace,
            })
        }
    }
}

fn initialize_replacement_traders(
    env: &mut V16Svm,
    actors: &[usize],
    deposit: u128,
) -> Result<(), String> {
    for &actor in actors {
        env.fund_closed_primary_portfolio(actor, 1_000_000_000)
            .map_err(|error| format!("fund replacement trader {actor}: {error}"))?;
        env.reinitialize_primary_portfolio(actor)
            .map_err(|error| format!("initialize replacement trader {actor}: {error}"))?;
        env.deposit_primary(actor, deposit)
            .map_err(|error| format!("deposit replacement trader {actor}: {error}"))?;
    }
    Ok(())
}

fn discover_rebalance_market_incarnation_replay(
    seed: [u8; 32],
) -> Result<MarketIncarnationDiscovery, String> {
    const SUBJECT: usize = 0;
    const COUNTERPARTY: usize = 1;
    const PRICE: u64 = 100;
    const DEPOSIT: u128 = 1_000_000;
    const OLD_SIZE_Q: i128 = 2_000 * POS_SCALE as i128;
    const NEW_SIZE_Q: i128 = 1_000 * POS_SCALE as i128;
    const REINIT_SLOT: u64 = 10;
    let kind = MarketIntentKind::RebalanceReduce;
    let config = MarketConfig {
        initial_price: PRICE,
        actor_deposits: [DEPOSIT, DEPOSIT, 1, 1, 1],
        actor_token_balances: [2_000_000, 2_000_000, 1, 1, 1],
        ..MarketConfig::default()
    };
    let mut env = V16Svm::new(seed, config);
    env.begin_public_trace();
    let supply_before = env.token_supply_observed();
    let old_market_id = env.primary_market_state().1.assets[0].market_id;
    env.trade_no_cpi(SUBJECT, COUNTERPARTY, 0, OLD_SIZE_Q, PRICE, 0)
        .map_err(|error| format!("open old-market rebalance position: {error}"))?;
    let retained = env.build_retained_rebalance_reduce(SUBJECT, 0, NEW_SIZE_Q.unsigned_abs());
    env.trade_no_cpi(SUBJECT, COUNTERPARTY, 0, -OLD_SIZE_Q, PRICE, 0)
        .map_err(|error| format!("close old-market rebalance position: {error}"))?;
    publicly_recreate_market(&mut env, config, REINIT_SLOT)?;
    env.configure_auth_mark(false, 0, REINIT_SLOT, PRICE)
        .map_err(|error| format!("configure replacement-market mark: {error}"))?;
    let new_market_id = env.primary_market_state().1.assets[0].market_id;
    initialize_replacement_traders(&mut env, &[SUBJECT, COUNTERPARTY], DEPOSIT)?;
    env.trade_no_cpi(SUBJECT, COUNTERPARTY, 0, NEW_SIZE_Q, PRICE, 0)
        .map_err(|error| format!("open replacement-market rebalance position: {error}"))?;
    finish_market_incarnation_discovery(
        &mut env,
        kind,
        old_market_id,
        new_market_id,
        retained,
        supply_before,
    )
}

fn discover_forfeit_market_incarnation_replay(
    seed: [u8; 32],
) -> Result<MarketIncarnationDiscovery, String> {
    const SUBJECT: usize = 0;
    const COUNTERPARTY: usize = 1;
    const PRICE: u64 = 100;
    const DEPOSIT: u128 = 1_000_000;
    const SIZE_Q: i128 = 5_000 * POS_SCALE as i128;
    const REINIT_SLOT: u64 = 10;
    let kind = MarketIntentKind::ForfeitRecoveryLeg;
    let config = MarketConfig {
        initial_price: PRICE,
        maintenance_margin_bps: 1_000,
        initial_margin_bps: 1_000,
        max_price_move_bps_per_slot: 500,
        max_accrual_dt_slots: 1,
        min_funding_lifetime_slots: 1,
        actor_deposits: [DEPOSIT, DEPOSIT, 1, 1, 1],
        actor_token_balances: [2_000_000, 2_000_000, 1, 1, 1],
        ..MarketConfig::default()
    };
    let mut env = V16Svm::new(seed, config);
    env.begin_public_trace();
    let supply_before = env.token_supply_observed();
    env.configure_permissionless_resolve(100, 1)
        .map_err(|error| format!("configure old-market recovery lifecycle: {error}"))?;
    let old_market_id = env.primary_market_state().1.assets[0].market_id;
    env.trade_no_cpi(SUBJECT, COUNTERPARTY, 0, SIZE_Q, PRICE, 0)
        .map_err(|error| format!("open old-market recovery leg: {error}"))?;
    env.warp_to_slot(2);
    env.shutdown_asset(0, 2)
        .map_err(|error| format!("shutdown old market asset: {error}"))?;
    let retained = env.build_retained_forfeit_recovery_leg(SUBJECT, 0, 1);
    for actor in [SUBJECT, COUNTERPARTY] {
        env.forfeit_recovery_leg(actor, 0, 1)
            .map_err(|error| format!("clear old-market recovery actor {actor}: {error}"))?;
    }
    publicly_recreate_market(&mut env, config, REINIT_SLOT)?;
    env.configure_permissionless_resolve(100, 1)
        .map_err(|error| format!("configure replacement recovery lifecycle: {error}"))?;
    env.configure_auth_mark(false, 0, REINIT_SLOT, PRICE)
        .map_err(|error| format!("configure replacement recovery mark: {error}"))?;
    let new_market_id = env.primary_market_state().1.assets[0].market_id;
    initialize_replacement_traders(&mut env, &[SUBJECT, COUNTERPARTY], DEPOSIT)?;
    env.trade_no_cpi(SUBJECT, COUNTERPARTY, 0, SIZE_Q, PRICE, 0)
        .map_err(|error| format!("open replacement-market recovery leg: {error}"))?;
    env.warp_to_slot(REINIT_SLOT + 1);
    env.shutdown_asset(0, REINIT_SLOT + 1)
        .map_err(|error| format!("shutdown replacement market asset: {error}"))?;
    finish_market_incarnation_discovery(
        &mut env,
        kind,
        old_market_id,
        new_market_id,
        retained,
        supply_before,
    )
}

fn discover_one_market_incarnation_replay(
    mut seed: [u8; 32],
    kind: MarketIntentKind,
) -> Result<MarketIncarnationDiscovery, String> {
    const SUBJECT: usize = 0;
    const REINIT_SLOT: u64 = 10;
    seed[0] ^= 0xb7;
    seed[1] ^= kind.discriminator();
    match kind {
        MarketIntentKind::RebalanceReduce => {
            return discover_rebalance_market_incarnation_replay(seed);
        }
        MarketIntentKind::ForfeitRecoveryLeg => {
            return discover_forfeit_market_incarnation_replay(seed);
        }
        _ => {}
    }
    let config = MarketConfig {
        initial_price: INITIAL_PRICE,
        h_max: 6_480_000,
        min_nonzero_mm_req: 599,
        min_nonzero_im_req: 600,
        maintenance_margin_bps: 500,
        initial_margin_bps: 500,
        liquidation_fee_bps: 5,
        liquidation_fee_cap: percolator::MAX_PROTOCOL_FEE_ABS,
        max_price_move_bps_per_slot: 24,
        max_accrual_dt_slots: 20,
        max_abs_funding_e9_per_slot: 1_000,
        min_funding_lifetime_slots: 10_000_000,
        maintenance_fee_per_slot: 1,
        actor_deposits: [1; PRIMARY_ACTOR_COUNT],
        ..MarketConfig::default()
    };
    let mut env = V16Svm::new(seed, config);
    env.begin_public_trace();
    let supply_before = env.token_supply_observed();
    if kind == MarketIntentKind::ShutdownAsset {
        env.configure_permissionless_resolve(1_000_000, 1)
            .map_err(|error| format!("configure old-market shutdown policy: {error}"))?;
    }
    let old_market_id = env.primary_market_state().1.assets[0].market_id;
    let old_matcher_sequence = (kind == MarketIntentKind::MatcherEnable)
        .then(|| env.primary_portfolio_matcher_sequence(SUBJECT));
    let retained = retained_market_intent(&mut env, kind);

    publicly_recreate_market(&mut env, config, REINIT_SLOT)?;
    let new_market_id = env.primary_market_state().1.assets[0].market_id;
    if matches!(
        kind,
        MarketIntentKind::Deposit | MarketIntentKind::MatcherEnable
    ) {
        env.fund_closed_primary_portfolio(SUBJECT, 1_000_000_000)
            .map_err(|error| format!("fund replacement portfolio: {error}"))?;
        env.reinitialize_primary_portfolio(SUBJECT)
            .map_err(|error| format!("initialize replacement portfolio: {error}"))?;
    }
    if kind == MarketIntentKind::MatcherEnable {
        // Market recreation resets both portfolio IDs and retained-action sequences. Advance
        // the replacement through legitimate owner mutations until the old generation's
        // expected sequence collides again; sequence binding alone is not market-incarnation
        // binding. The dynamic target keeps this probe valid as more owner operations consume
        // the shared sequence.
        let target = old_matcher_sequence.expect("matcher target captured above");
        while env.primary_portfolio_matcher_sequence(SUBJECT) < target {
            env.set_matcher_config(SUBJECT, 0)
                .map_err(|error| format!("align replacement matcher sequence: {error}"))?;
        }
        if env.primary_portfolio_matcher_sequence(SUBJECT) != target {
            return Err(format!(
                "replacement matcher sequence overshot retained target {target}"
            ));
        }
    }
    if kind == MarketIntentKind::ShutdownAsset {
        env.configure_permissionless_resolve(1_000_000, 1)
            .map_err(|error| format!("configure replacement shutdown policy: {error}"))?;
        env.warp_to_slot(12);
    }

    finish_market_incarnation_discovery(
        &mut env,
        kind,
        old_market_id,
        new_market_id,
        retained,
        supply_before,
    )
}

pub fn discover_market_incarnation_replays(
    seed: [u8; 32],
) -> Result<Vec<MarketIncarnationDiscovery>, String> {
    MarketIntentKind::ALL
        .into_iter()
        .map(|kind| discover_one_market_incarnation_replay(seed, kind))
        .collect()
}

pub fn discover_terminal_generation_replay(
    mut seed: [u8; 32],
    kind: TerminalGenerationKind,
) -> Result<TerminalGenerationDiscovery, String> {
    const PRICE: u64 = 100;
    const ADVERSE_MARK: u64 = 110;
    const DEPOSIT: u128 = 1_000_000;
    const SIZE_Q: i128 = 10_000 * POS_SCALE as i128;
    const RESTART_SLOT: u64 = 10;
    const MARK_SLOT: u64 = 20;

    seed[0] ^= 0xd4;
    seed[1] ^= kind.discriminator();
    let config = MarketConfig {
        initial_price: PRICE,
        actor_deposits: [DEPOSIT; PRIMARY_ACTOR_COUNT],
        actor_token_balances: [2_000_000; PRIMARY_ACTOR_COUNT],
        ..MarketConfig::default()
    };
    let mut env = V16Svm::new(seed, config);
    let supply_before = env.token_supply_observed();
    if kind == TerminalGenerationKind::AssetResolvePolicy {
        env.configure_permissionless_resolve(1_000_000, 1)
            .map_err(|error| format!("configure old asset lifecycle: {error}"))?;
    }
    let old_generation = env.primary_market_state().1.assets[0].market_id;
    let retained = match kind {
        TerminalGenerationKind::MarketResolve => env.build_retained_resolve_market(),
        TerminalGenerationKind::MarketResolvePolicy
        | TerminalGenerationKind::AssetResolvePolicy => {
            env.build_retained_permissionless_resolve_policy(1, 1)
        }
    };

    match kind {
        TerminalGenerationKind::MarketResolve | TerminalGenerationKind::MarketResolvePolicy => {
            publicly_recreate_market(&mut env, config, RESTART_SLOT)?;
            env.configure_auth_mark(false, 0, RESTART_SLOT, PRICE)
                .map_err(|error| format!("configure replacement-market mark: {error}"))?;
            initialize_replacement_traders(&mut env, &[0, 1], DEPOSIT)?;
        }
        TerminalGenerationKind::AssetResolvePolicy => {
            env.warp_to_slot(RESTART_SLOT - 1);
            let shutdown_slot = RESTART_SLOT - 1;
            let before_shutdown = fingerprint(&env);
            let initial_shutdown = env.shutdown_asset(0, shutdown_slot);
            let mut shutdown_landed = initial_shutdown.is_ok();
            if let Err(error) = initial_shutdown {
                if !is_engine_stale_error(&error) || fingerprint(&env) != before_shutdown {
                    return Err(format!(
                        "shutdown old asset generation did not reject stale state exactly: {error}"
                    ));
                }
                for step in 0..16 {
                    let oracle_accounts = env.primary_profile(0).oracle_leg_count;
                    env.crank(
                        0,
                        shutdown_slot,
                        vec![CrankObservationHint {
                            asset_index: 0,
                            oracle_accounts,
                        }],
                    )
                    .map_err(|crank_error| {
                        format!("old-asset shutdown catch-up crank {step}: {crank_error}")
                    })?;
                    match env.shutdown_asset(0, shutdown_slot) {
                        Ok(_) => {
                            shutdown_landed = true;
                            break;
                        }
                        Err(error) if is_engine_stale_error(&error) => {}
                        Err(error) => {
                            return Err(format!(
                                "old-asset shutdown retry returned unexpected error: {error}"
                            ));
                        }
                    }
                }
            }
            if !shutdown_landed {
                return Err("old asset did not shut down after bounded public catch-up".into());
            }
            env.warp_to_slot(RESTART_SLOT);
            env.restart_asset_oracle(0, RESTART_SLOT, PRICE)
                .map_err(|error| format!("restart replacement asset: {error}"))?;
            env.configure_auth_mark(false, 0, RESTART_SLOT, PRICE)
                .map_err(|error| format!("configure replacement-asset mark: {error}"))?;
        }
    }
    let new_generation = env.primary_market_state().1.assets[0].market_id;
    if kind == TerminalGenerationKind::AssetResolvePolicy && new_generation <= old_generation {
        return Err(format!(
            "asset restart did not advance generation: {old_generation}->{new_generation}"
        ));
    }

    env.trade_no_cpi(0, 1, 0, SIZE_Q, PRICE, 0)
        .map_err(|error| format!("open replacement user positions: {error}"))?;
    env.warp_to_slot(MARK_SLOT);
    env.push_auth_mark(0, MARK_SLOT, ADVERSE_MARK)
        .map_err(|error| format!("publish replacement adverse mark: {error}"))?;
    let observation = vec![CrankObservationHint {
        asset_index: 0,
        oracle_accounts: env.primary_profile(0).oracle_leg_count,
    }];
    env.crank(0, MARK_SLOT, observation.clone())
        .map_err(|error| format!("refresh replacement winner: {error}"))?;
    env.crank(1, MARK_SLOT, observation)
        .map_err(|error| format!("refresh replacement victim: {error}"))?;
    if env.primary_market_state().1.assets[0].effective_price != ADVERSE_MARK {
        return Err("replacement market did not commit adverse mark".into());
    }

    let before_replay = fingerprint(&env);
    let replay_result = env.land_retained(retained);
    if kind == TerminalGenerationKind::AssetResolvePolicy {
        let error = match replay_result {
            Ok(success) => {
                return Err(format!(
                    "old asset-generation resolve policy landed in {} CU",
                    success.compute_units
                ));
            }
            Err(error) => error,
        };
        let expected = format!(
            "Custom({})",
            PercolatorError::AssetGenerationMismatch as u32
        );
        if !error.contains(&expected) {
            return Err(format!(
                "old asset-generation resolve policy rejected for the wrong reason: expected {expected}, got {error}"
            ));
        }
        if fingerprint(&env) != before_replay {
            return Err(
                "rejected asset-generation resolve policy did not roll back exactly".into(),
            );
        }
        let fresh = env.build_retained_permissionless_resolve_policy(1, 1);
        let before_fresh = fingerprint(&env);
        env.land_retained(fresh).map_err(|error| {
            format!("current asset-generation resolve policy rejected: {error}")
        })?;
        if fingerprint(&env) == before_fresh {
            return Err("current asset-generation resolve policy made no state change".into());
        }
        if env.token_supply_observed() != supply_before {
            return Err("asset-generation resolve-policy protection changed SPL supply".into());
        }
        return Ok(TerminalGenerationDiscovery {
            kind,
            old_generation,
            new_generation,
            stale_intent_rejected: true,
            exact_rollback: true,
            rejection_was_generation_mismatch: true,
            fresh_intent_landed: true,
            victim_payout: 0,
            winner_payout: 0,
            victim_loss: 0,
            winner_gain: 0,
        });
    }
    replay_result.map_err(|error| format!("old-generation terminal intent rejected: {error}"))?;
    if kind != TerminalGenerationKind::MarketResolve {
        env.warp_to_slot(MARK_SLOT + 2);
        env.resolve_stale_permissionless(MARK_SLOT + 2)
            .map_err(|error| format!("stale policy did not terminalize replacement: {error}"))?;
    }
    if env.primary_market_state().1.mode != MarketModeV16::Resolved {
        return Err("old-generation capability did not resolve replacement market".into());
    }

    env.warp_to_slot(MARK_SLOT + 3);
    let victim_destination = env.actors[1].destination_token;
    let winner_destination = env.actors[0].destination_token;
    let victim_before = env.token_amount(victim_destination);
    let winner_before = env.token_amount(winner_destination);
    env.close_resolved_primary(1)
        .map_err(|error| format!("close terminal replay victim: {error}"))?;
    env.close_resolved_primary(0)
        .map_err(|error| format!("close terminal replay winner: {error}"))?;
    let victim_payout = u128::from(
        env.token_amount(victim_destination)
            .checked_sub(victim_before)
            .ok_or_else(|| "terminal victim destination decreased".to_string())?,
    );
    let winner_payout = u128::from(
        env.token_amount(winner_destination)
            .checked_sub(winner_before)
            .ok_or_else(|| "terminal winner destination decreased".to_string())?,
    );
    let victim_loss = DEPOSIT.saturating_sub(victim_payout);
    let winner_gain = winner_payout.saturating_sub(DEPOSIT);
    if env.token_supply_observed() != supply_before {
        return Err("terminal generation replay changed SPL supply".into());
    }
    Ok(TerminalGenerationDiscovery {
        kind,
        old_generation,
        new_generation,
        stale_intent_rejected: false,
        exact_rollback: false,
        rejection_was_generation_mismatch: false,
        fresh_intent_landed: false,
        victim_payout,
        winner_payout,
        victim_loss,
        winner_gain,
    })
}

fn discover_rebalance_position_episode(seed: [u8; 32]) -> Result<PositionEpisodeDiscovery, String> {
    const PRICE: u64 = 100;
    const ADVERSE_MARK: u64 = 50;
    const DEPOSIT: u128 = 1_000_000;
    const OLD_SIZE_Q: i128 = 2_000 * POS_SCALE as i128;
    const NEW_SIZE_Q: i128 = 1_000 * POS_SCALE as i128;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            actor_deposits: [DEPOSIT, DEPOSIT, 0, 0, 0],
            actor_token_balances: [2_000_000, 2_000_000, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.configure_auth_mark(false, 0, 1, PRICE)
        .map_err(|error| format!("configure position-episode mark: {error}"))?;
    env.trade_no_cpi(0, 1, 0, OLD_SIZE_Q, PRICE, 0)
        .map_err(|error| format!("open old position episode: {error}"))?;
    let signed_portfolio_id = env.primary_portfolio_id(0);
    let signed_position_epoch = env.primary_portfolio_position_epoch(0);
    let retained = env.build_retained_rebalance_reduce(0, 0, NEW_SIZE_Q.unsigned_abs());
    env.trade_no_cpi(0, 1, 0, -OLD_SIZE_Q, PRICE, 0)
        .map_err(|error| format!("close old position episode: {error}"))?;
    if discovery_position(&env.primary_portfolio(0), 0)? != 0 {
        return Err("old rebalance position episode did not close".into());
    }
    env.trade_no_cpi(0, 1, 0, NEW_SIZE_Q, PRICE, 0)
        .map_err(|error| format!("open replacement position episode: {error}"))?;

    env.warp_to_slot(2);
    env.push_auth_mark(0, 2, ADVERSE_MARK)
        .map_err(|error| format!("publish adverse replacement mark: {error}"))?;
    env.crank(
        0,
        2,
        vec![CrankObservationHint {
            asset_index: 0,
            oracle_accounts: env.primary_profile(0).oracle_leg_count,
        }],
    )
    .map_err(|error| format!("refresh replacement position at adverse mark: {error}"))?;
    if env.primary_portfolio(0).capital.get() >= DEPOSIT {
        return Err("adverse mark did not debit replacement owner".into());
    }
    let portfolio_id_unchanged = env.primary_portfolio_id(0) == signed_portfolio_id;
    let episode_advanced = env.primary_portfolio_position_epoch(0) > signed_position_epoch;
    let replacement_before = discovery_position(&env.primary_portfolio(0), 0)?;
    if replacement_before == 0 {
        return Err("replacement rebalance episode has no exposure".into());
    }
    let market_before = env.market_data(false);
    let portfolio_before = env.primary_portfolio_data(0);
    let vault_before = env.svm.get_account(&env.vault);
    let supply_before_replay = env.token_supply_observed();
    let stale_intent_rejected = env.land_retained(retained).is_err();
    let exact_rollback = env.market_data(false) == market_before
        && env.primary_portfolio_data(0) == portfolio_before
        && env.svm.get_account(&env.vault) == vault_before
        && env.token_supply_observed() == supply_before_replay;
    let replacement_after_stale = discovery_position(&env.primary_portfolio(0), 0)?;
    let replacement_exposure_preserved = replacement_after_stale == replacement_before;

    if !stale_intent_rejected {
        return Ok(PositionEpisodeDiscovery {
            kind: PositionEpisodeKind::RebalanceReduce,
            stale_intent_rejected,
            exact_rollback,
            fresh_intent_landed: false,
            portfolio_id_unchanged,
            episode_advanced,
            replacement_exposure_preserved,
            fresh_intent_changed_exposure: false,
            token_supply_preserved: env.token_supply_observed() == supply_before,
        });
    }

    let current = env.build_retained_rebalance_reduce(0, 0, NEW_SIZE_Q.unsigned_abs());
    let fresh_intent_landed = env.land_retained(current).is_ok();
    let replacement_after_fresh = discovery_position(&env.primary_portfolio(0), 0)?;
    let fresh_intent_changed_exposure = fresh_intent_landed && replacement_after_fresh == 0;
    Ok(PositionEpisodeDiscovery {
        kind: PositionEpisodeKind::RebalanceReduce,
        stale_intent_rejected,
        exact_rollback,
        fresh_intent_landed,
        portfolio_id_unchanged,
        episode_advanced,
        replacement_exposure_preserved,
        fresh_intent_changed_exposure,
        token_supply_preserved: env.token_supply_observed() == supply_before,
    })
}

fn discover_recovery_position_episode(seed: [u8; 32]) -> Result<PositionEpisodeDiscovery, String> {
    const VICTIM_CAPITAL: u128 = 20_000;
    const FIRST_LOSER_CAPITAL: u128 = 10_000;
    const SECOND_LOSER_CAPITAL: u128 = 20_000;
    const POSITION_Q: i128 = 10_000 * POS_SCALE as i128;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: 1,
            max_trading_fee_bps: 10,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            max_bankrupt_close_lifetime_slots: 1,
            public_b_chunk_atoms: 1,
            actor_deposits: [
                VICTIM_CAPITAL,
                FIRST_LOSER_CAPITAL,
                SECOND_LOSER_CAPITAL,
                0,
                0,
            ],
            actor_token_balances: [100_000, 100_000, 100_000, 1, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.configure_auth_mark(false, 0, 1, 1)
        .map_err(|error| format!("configure recovery-episode mark: {error}"))?;
    env.trade_no_cpi(0, 1, 0, POSITION_Q, 1, 0)
        .map_err(|error| format!("open first recovery episode: {error}"))?;
    env.warp_to_slot(2);
    env.push_auth_mark(0, 2, 2)
        .map_err(|error| format!("publish first bankruptcy mark: {error}"))?;
    let observation = vec![CrankObservationHint {
        asset_index: 0,
        oracle_accounts: env.primary_profile(0).oracle_leg_count,
    }];
    env.crank(1, 2, observation.clone())
        .map_err(|error| format!("observe first bankrupt loser: {error}"))?;
    for step in 0..3 {
        env.crank(1, 2, Vec::new())
            .map_err(|error| format!("advance first bankrupt loser step {step}: {error}"))?;
    }
    if env.primary_market_state().1.assets[0].mode_long != SideModeV16::ResetPending
        || discovery_position(&env.primary_portfolio(0), 0)? == 0
    {
        return Err("first bankruptcy did not create a live forfeitable recovery leg".into());
    }

    let signed_portfolio_id = env.primary_portfolio_id(0);
    let signed_position_epoch = env.primary_portfolio_position_epoch(0);
    let intended = env.build_retained_forfeit_recovery_leg(0, 0, u128::from(u64::MAX));
    let retained = env.build_retained_forfeit_recovery_leg(0, 0, u128::from(u64::MAX));
    if let Err(error) = env.land_retained(intended) {
        return Err(format!("land intended first-episode forfeit: {error}"));
    }
    env.finalize_reset_side(0, 0)
        .map_err(|error| format!("finalize first recovery episode: {error}"))?;
    if discovery_position(&env.primary_portfolio(0), 0)? != 0 {
        return Err("first recovery episode remained active".into());
    }

    env.trade_no_cpi(0, 2, 0, POSITION_Q, 2, 0)
        .map_err(|error| format!("open replacement recovery episode: {error}"))?;
    env.warp_to_slot(3);
    env.push_auth_mark(0, 3, 4)
        .map_err(|error| format!("publish second bankruptcy mark: {error}"))?;
    env.crank(2, 3, observation)
        .map_err(|error| format!("observe second bankrupt loser: {error}"))?;
    for step in 0..3 {
        env.crank(2, 3, Vec::new())
            .map_err(|error| format!("advance second bankrupt loser step {step}: {error}"))?;
    }
    if env.primary_market_state().1.assets[0].mode_long != SideModeV16::ResetPending
        || discovery_position(&env.primary_portfolio(0), 0)? == 0
    {
        return Err("replacement recovery episode was not live before stale consent".into());
    }
    let portfolio_id_unchanged = env.primary_portfolio_id(0) == signed_portfolio_id;
    let episode_advanced = env.primary_portfolio_position_epoch(0) > signed_position_epoch;
    let replacement_before = discovery_position(&env.primary_portfolio(0), 0)?;
    let market_before = env.market_data(false);
    let portfolio_before = env.primary_portfolio_data(0);
    let vault_before = env.svm.get_account(&env.vault);
    let supply_before_replay = env.token_supply_observed();
    let stale_intent_rejected = env.land_retained(retained).is_err();
    let exact_rollback = env.market_data(false) == market_before
        && env.primary_portfolio_data(0) == portfolio_before
        && env.svm.get_account(&env.vault) == vault_before
        && env.token_supply_observed() == supply_before_replay;
    let replacement_after_stale = discovery_position(&env.primary_portfolio(0), 0)?;
    let replacement_exposure_preserved = replacement_after_stale == replacement_before;

    if !stale_intent_rejected {
        return Ok(PositionEpisodeDiscovery {
            kind: PositionEpisodeKind::RecoveryForfeit,
            stale_intent_rejected,
            exact_rollback,
            fresh_intent_landed: false,
            portfolio_id_unchanged,
            episode_advanced,
            replacement_exposure_preserved,
            fresh_intent_changed_exposure: false,
            token_supply_preserved: env.token_supply_observed() == supply_before,
        });
    }

    let current = env.build_retained_forfeit_recovery_leg(0, 0, u128::from(u64::MAX));
    let fresh_intent_landed = env.land_retained(current).is_ok();
    let replacement_after_fresh = discovery_position(&env.primary_portfolio(0), 0)?;
    let fresh_intent_changed_exposure = fresh_intent_landed && replacement_after_fresh == 0;
    Ok(PositionEpisodeDiscovery {
        kind: PositionEpisodeKind::RecoveryForfeit,
        stale_intent_rejected,
        exact_rollback,
        fresh_intent_landed,
        portfolio_id_unchanged,
        episode_advanced,
        replacement_exposure_preserved,
        fresh_intent_changed_exposure,
        token_supply_preserved: env.token_supply_observed() == supply_before,
    })
}

pub fn discover_position_episode_replays(
    mut seed: [u8; 32],
) -> Result<Vec<PositionEpisodeDiscovery>, String> {
    PositionEpisodeKind::ALL
        .into_iter()
        .map(|kind| {
            let mut scenario_seed = seed;
            scenario_seed[0] ^= 0xe4;
            scenario_seed[1] ^= kind.discriminator();
            seed[2] = seed[2].wrapping_add(1);
            match kind {
                PositionEpisodeKind::RebalanceReduce => {
                    discover_rebalance_position_episode(scenario_seed)
                }
                PositionEpisodeKind::RecoveryForfeit => {
                    discover_recovery_position_episode(scenario_seed)
                }
            }
        })
        .collect()
}

fn configure_old_asset_intent(
    env: &mut V16Svm,
    kind: AssetIntentKind,
    asset_index: u16,
    authority_actor: usize,
) -> Result<Option<solana_sdk::pubkey::Pubkey>, String> {
    match kind {
        AssetIntentKind::PushAuthMark => env
            .configure_auth_mark(false, asset_index, 1, INITIAL_PRICE)
            .map(|_| None)
            .map_err(|error| format!("configure old AuthMark: {error}")),
        AssetIntentKind::PushEwmaMark => env
            .configure_ewma_mark(asset_index, 1, INITIAL_PRICE, 1, 0)
            .map(|_| None)
            .map_err(|error| format!("configure old EwmaMark: {error}")),
        AssetIntentKind::InsuranceTopUp => env
            .update_asset_authority_from_admin(
                asset_index,
                percolator_prog::processor::ASSET_AUTH_INSURANCE,
                authority_actor,
            )
            .map(|_| None)
            .map_err(|error| format!("install old insurance authority: {error}")),
        AssetIntentKind::InsuranceWithdrawal => {
            for authority_kind in [
                percolator_prog::processor::ASSET_AUTH_INSURANCE,
                percolator_prog::processor::ASSET_AUTH_INSURANCE_OPERATOR,
            ] {
                env.update_asset_authority_from_admin(asset_index, authority_kind, authority_actor)
                    .map_err(|error| format!("install old insurance role: {error}"))?;
            }
            Ok(None)
        }
        AssetIntentKind::BackingTopUp | AssetIntentKind::BackingFeePolicy => env
            .update_asset_authority_from_admin(
                asset_index,
                percolator_prog::processor::ASSET_AUTH_BACKING_BUCKET,
                authority_actor,
            )
            .map(|_| None)
            .map_err(|error| format!("install old backing authority: {error}")),
        AssetIntentKind::ConfigureHybridOracle => {
            env.set_clock(1, 100);
            let feed = [0x5au8; 32];
            Ok(Some(env.set_pyth_price(
                &feed,
                INITIAL_PRICE as i64,
                -6,
                0,
                101,
            )))
        }
        _ => Ok(None),
    }
}

fn retained_asset_intent(
    env: &mut V16Svm,
    kind: AssetIntentKind,
    asset_index: u16,
    authority_actor: usize,
    oracle_account: Option<solana_sdk::pubkey::Pubkey>,
) -> Transaction {
    const SUBJECT: usize = 0;
    const COUNTERPARTY: usize = 1;
    const AMOUNT: u128 = 1_000;
    let size_q = POS_SCALE as i128 / 4;
    let stale_price = INITIAL_PRICE / 2;
    let domain = asset_index * 2;
    match kind {
        AssetIntentKind::TradeNoCpi => env.build_retained_no_cpi_trade(
            SUBJECT,
            COUNTERPARTY,
            asset_index,
            size_q,
            INITIAL_PRICE,
        ),
        AssetIntentKind::TradeCpi => {
            env.build_retained_cpi_trade(SUBJECT, COUNTERPARTY, asset_index, size_q, 0)
        }
        AssetIntentKind::BatchTradeNoCpi => env.build_retained_batch_no_cpi_trade(
            SUBJECT,
            COUNTERPARTY,
            asset_index,
            size_q,
            INITIAL_PRICE,
        ),
        AssetIntentKind::BatchTradeCpi => {
            env.build_retained_batch_cpi_trade(SUBJECT, COUNTERPARTY, asset_index, size_q, 0)
        }
        AssetIntentKind::PushAuthMark => {
            env.build_retained_auth_mark_with_sequence(asset_index, stale_price, u64::MAX)
        }
        AssetIntentKind::PushEwmaMark => {
            env.build_retained_ewma_mark_with_sequence(asset_index, stale_price, u64::MAX)
        }
        AssetIntentKind::ConfigureAuthMark => {
            env.build_retained_auth_config_with_sequence(asset_index, stale_price, u64::MAX)
        }
        AssetIntentKind::ConfigureEwmaMark => {
            env.build_retained_ewma_config_with_sequence(asset_index, stale_price, 1, 0, u64::MAX)
        }
        AssetIntentKind::ConfigureHybridOracle => {
            let feed = [0x5au8; 32];
            env.build_retained_hybrid_oracle_config_with_sequence(
                asset_index,
                5,
                101,
                0,
                [feed, [0; 32], [0; 32]],
                &[oracle_account.expect("hybrid oracle fixture")],
                1,
                0,
                u64::MAX,
            )
        }
        AssetIntentKind::InsuranceTopUp => {
            env.build_retained_insurance_domain_top_up_for_actor(authority_actor, domain, AMOUNT)
        }
        AssetIntentKind::BackingTopUp => {
            env.build_retained_backing_bucket_top_up_for_actor(authority_actor, domain, AMOUNT, 100)
        }
        AssetIntentKind::InsuranceWithdrawal => {
            env.build_retained_insurance_withdrawal_for_actor(authority_actor, asset_index, AMOUNT)
        }
        AssetIntentKind::BackingFeePolicy => {
            env.build_retained_backing_fee_policy_for_actor(authority_actor, domain, 100, 5_000)
        }
        AssetIntentKind::ResolveMarket => env.build_retained_resolve_market(),
        AssetIntentKind::ResolvePolicy => env.build_retained_permissionless_resolve_policy(17, 29),
    }
}

fn configure_replacement_asset(
    env: &mut V16Svm,
    kind: AssetIntentKind,
    asset_index: u16,
    authority_actor: usize,
) -> Result<(), String> {
    match kind {
        AssetIntentKind::TradeNoCpi
        | AssetIntentKind::TradeCpi
        | AssetIntentKind::BatchTradeNoCpi
        | AssetIntentKind::BatchTradeCpi
        | AssetIntentKind::PushAuthMark => env
            .configure_auth_mark(false, asset_index, 4, INITIAL_PRICE)
            .map(|_| ())
            .map_err(|error| format!("configure replacement AuthMark: {error}")),
        AssetIntentKind::PushEwmaMark => env
            .configure_ewma_mark(asset_index, 4, INITIAL_PRICE, 1, 0)
            .map(|_| ())
            .map_err(|error| format!("configure replacement EwmaMark: {error}")),
        // Configuration requests must be rejected by generation binding even when they are the
        // first oracle configuration attempted after slot reuse. Consuming the observation
        // sequence with a replacement configuration first would hide that replay ordering.
        AssetIntentKind::ConfigureAuthMark
        | AssetIntentKind::ConfigureEwmaMark
        | AssetIntentKind::ConfigureHybridOracle => Ok(()),
        AssetIntentKind::InsuranceWithdrawal => env
            .top_up_insurance_domain_for_actor(authority_actor, asset_index * 2, 1_000)
            .map(|_| ())
            .map_err(|error| format!("fund replacement insurance reserve: {error}")),
        _ => Ok(()),
    }
}

fn discover_one_asset_generation_replay(
    mut seed: [u8; 32],
    kind: AssetIntentKind,
) -> Result<AssetGenerationDiscovery, String> {
    const ASSET: u16 = 1;
    const AUTHORITY_ACTOR: usize = 2;
    const ACTIVATION_PAYER: usize = 3;
    seed[0] ^= 0xc9;
    seed[1] ^= kind.discriminator();
    let mut env = V16Svm::new(seed, MarketConfig::default());
    let supply_before = env.token_supply_observed();
    let oracle_account = configure_old_asset_intent(&mut env, kind, ASSET, AUTHORITY_ACTOR)?;
    if kind == AssetIntentKind::InsuranceWithdrawal {
        env.top_up_insurance_domain_for_actor(AUTHORITY_ACTOR, ASSET * 2, 1_000)
            .map_err(|error| format!("fund old insurance reserve: {error}"))?;
    }
    let old_asset_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    let retained = retained_asset_intent(&mut env, kind, ASSET, AUTHORITY_ACTOR, oracle_account);
    if kind == AssetIntentKind::InsuranceWithdrawal {
        env.withdraw_insurance_asset(AUTHORITY_ACTOR, ASSET, 1_000)
            .map_err(|error| format!("clear old insurance reserve: {error}"))?;
    }

    env.update_market_init_fee_policy(1)
        .map_err(|error| format!("configure permissionless asset activation: {error}"))?;
    env.warp_to_slot(3);
    env.retire_asset(ASSET, 3)
        .map_err(|error| format!("retire old asset: {error}"))?;
    env.warp_to_slot(4);
    if kind.uses_actor_authorities() {
        env.activate_permissionless_asset_with_actor_authorities(
            ACTIVATION_PAYER,
            ASSET,
            4,
            INITIAL_PRICE,
            AUTHORITY_ACTOR,
            AUTHORITY_ACTOR,
            AUTHORITY_ACTOR,
            AUTHORITY_ACTOR,
            1,
        )
        .map_err(|error| format!("activate actor-authority replacement asset: {error}"))?;
    } else {
        env.activate_permissionless_asset(ACTIVATION_PAYER, ASSET, 4, INITIAL_PRICE, 1)
            .map_err(|error| format!("activate replacement asset: {error}"))?;
    }
    let new_asset_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    if new_asset_id <= old_asset_id {
        return Err(format!(
            "asset generation did not advance: {old_asset_id} -> {new_asset_id}"
        ));
    }
    configure_replacement_asset(&mut env, kind, ASSET, AUTHORITY_ACTOR)?;
    if matches!(
        kind,
        AssetIntentKind::PushAuthMark
            | AssetIntentKind::PushEwmaMark
            | AssetIntentKind::ConfigureAuthMark
            | AssetIntentKind::ConfigureEwmaMark
            | AssetIntentKind::ConfigureHybridOracle
    ) {
        env.warp_to_slot(5);
    }
    if kind == AssetIntentKind::ConfigureHybridOracle {
        env.set_clock(5, 101);
    }

    let before = fingerprint(&env);
    let result = env.land_retained(retained);
    let after = fingerprint(&env);
    if env.token_supply_observed() != supply_before {
        return Err(format!(
            "{kind:?} asset-generation probe changed SPL supply: {supply_before} -> {}",
            env.token_supply_observed()
        ));
    }

    match result {
        Ok(success) => {
            let mutated_economic_state = before != after;
            if !mutated_economic_state {
                return Err(format!(
                    "{kind:?} stale asset transaction succeeded without an observable state delta"
                ));
            }
            Ok(AssetGenerationDiscovery {
                kind,
                old_asset_id,
                new_asset_id,
                accepted_stale_intent: true,
                mutated_economic_state,
                compute_units: Some(success.compute_units),
                rejection_was_generation_mismatch: false,
                fresh_intent_landed: false,
                fresh_intent_mutated_economic_state: false,
            })
        }
        Err(error) => {
            if before != after {
                return Err(format!(
                    "{kind:?} rejected stale asset transaction did not roll back exactly"
                ));
            }
            let expected = format!(
                "Custom({})",
                PercolatorError::AssetGenerationMismatch as u32
            );
            if !error.contains(&expected) {
                return Err(format!(
                    "{kind:?} stale asset transaction rejected for the wrong reason: expected {expected}, got {error}"
                ));
            }

            let fresh =
                retained_asset_intent(&mut env, kind, ASSET, AUTHORITY_ACTOR, oracle_account);
            let before_fresh = fingerprint(&env);
            let fresh_result = env.land_retained(fresh);
            let after_fresh = fingerprint(&env);
            let fresh_intent_landed = fresh_result.is_ok();
            let fresh_intent_mutated_economic_state = before_fresh != after_fresh;
            if !fresh_intent_landed || !fresh_intent_mutated_economic_state {
                return Err(format!(
                    "{kind:?} current-generation control was not live: result={fresh_result:?}, mutated={fresh_intent_mutated_economic_state}"
                ));
            }
            if env.token_supply_observed() != supply_before {
                return Err(format!(
                    "{kind:?} current-generation control changed SPL supply: {supply_before} -> {}",
                    env.token_supply_observed()
                ));
            }
            Ok(AssetGenerationDiscovery {
                kind,
                old_asset_id,
                new_asset_id,
                accepted_stale_intent: false,
                mutated_economic_state: false,
                compute_units: None,
                rejection_was_generation_mismatch: true,
                fresh_intent_landed,
                fresh_intent_mutated_economic_state,
            })
        }
    }
}

pub fn discover_asset_generation_replays(
    seed: [u8; 32],
) -> Result<Vec<AssetGenerationDiscovery>, String> {
    let trace = std::env::var_os("PERCOLATOR_DISCOVERY_TRACE").is_some();
    let mut discoveries = Vec::with_capacity(AssetIntentKind::ALL.len());
    for kind in AssetIntentKind::ALL {
        if trace {
            eprintln!("asset-generation probe start: {kind:?}");
        }
        let discovery = discover_one_asset_generation_replay(seed, kind).map_err(|error| {
            if trace {
                eprintln!("asset-generation probe error: {kind:?}: {error}");
            }
            error
        })?;
        if trace {
            eprintln!(
                "asset-generation probe finish: {kind:?} violation={}",
                discovery.is_violation()
            );
        }
        discoveries.push(discovery);
    }
    Ok(discoveries)
}

pub fn discover_asset_generation_replay(
    seed: [u8; 32],
    kind: AssetIntentKind,
) -> Result<AssetGenerationDiscovery, String> {
    discover_one_asset_generation_replay(seed, kind)
}

fn discover_one_authority_incarnation_replay(
    mut seed: [u8; 32],
    kind: AuthorityIntentKind,
) -> Result<AuthorityIncarnationDiscovery, String> {
    const ASSET: u16 = 1;
    const AUTHORITY_A: usize = 0;
    const AUTHORITY_B: usize = 1;
    const TARGET: usize = 2;
    seed[0] ^= 0xd5;
    seed[1] ^= kind.discriminator();
    let mut env = V16Svm::new(seed, MarketConfig::default());
    let supply_before = env.token_supply_observed();

    let retained = match kind {
        AuthorityIntentKind::MarketAuthorityHandoff => {
            env.build_retained_market_authority_handoff_from_admin(TARGET)
        }
        AuthorityIntentKind::ResolveMarket => env.build_retained_resolve_market(),
        _ => {
            let authority_kind = kind
                .asset_authority_kind()
                .expect("asset authority operation");
            env.update_asset_authority_from_admin(ASSET, authority_kind, AUTHORITY_A)
                .map_err(|error| format!("install authority A: {error}"))?;
            env.build_retained_asset_authority_handoff_between_actors(
                ASSET,
                authority_kind,
                AUTHORITY_A,
                TARGET,
            )
        }
    };

    match kind {
        AuthorityIntentKind::MarketAuthorityHandoff | AuthorityIntentKind::ResolveMarket => {
            env.update_market_authority_from_admin(AUTHORITY_B)
                .map_err(|error| format!("rotate market authority A to B: {error}"))?;
            env.update_market_authority_to_admin(AUTHORITY_B)
                .map_err(|error| format!("rotate market authority B to A: {error}"))?;
        }
        _ => {
            let authority_kind = kind
                .asset_authority_kind()
                .expect("asset authority operation");
            env.update_asset_authority_between_actors(
                ASSET,
                authority_kind,
                AUTHORITY_A,
                AUTHORITY_B,
            )
            .map_err(|error| format!("rotate asset authority A to B: {error}"))?;
            env.update_asset_authority_between_actors(
                ASSET,
                authority_kind,
                AUTHORITY_B,
                AUTHORITY_A,
            )
            .map_err(|error| format!("rotate asset authority B to A: {error}"))?;
        }
    }

    let before = fingerprint(&env);
    let result = env.land_retained(retained);
    let after = fingerprint(&env);
    if env.token_supply_observed() != supply_before {
        return Err(format!(
            "{kind:?} authority-incarnation probe changed SPL supply: {supply_before} -> {}",
            env.token_supply_observed()
        ));
    }

    match result {
        Ok(success) => {
            let mutated_economic_state = before != after;
            if !mutated_economic_state {
                return Err(format!(
                    "{kind:?} stale authority transaction succeeded without an observable state delta"
                ));
            }
            Ok(AuthorityIncarnationDiscovery {
                kind,
                accepted_stale_intent: true,
                mutated_economic_state,
                compute_units: Some(success.compute_units),
            })
        }
        Err(_) => {
            if before != after {
                return Err(format!(
                    "{kind:?} rejected stale authority transaction did not roll back exactly"
                ));
            }
            Ok(AuthorityIncarnationDiscovery {
                kind,
                accepted_stale_intent: false,
                mutated_economic_state: false,
                compute_units: None,
            })
        }
    }
}

pub fn discover_authority_incarnation_replays(
    seed: [u8; 32],
) -> Result<Vec<AuthorityIncarnationDiscovery>, String> {
    AuthorityIntentKind::ALL
        .into_iter()
        .map(|kind| discover_one_authority_incarnation_replay(seed, kind))
        .collect()
}

fn discover_one_funded_role_seizure(
    mut seed: [u8; 32],
    kind: FundedRoleKind,
) -> Result<FundedRoleDiscovery, String> {
    const PROVIDER: usize = 0;
    const REPLACEMENT: usize = 1;
    const ASSET_ADMIN: usize = 2;
    const ASSET: u16 = 0;
    const DOMAIN: u16 = 0;
    const PRINCIPAL: u128 = 500;
    seed[0] ^= 0x75;
    seed[1] ^= kind.discriminator();
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            actor_deposits: [1; PRIMARY_ACTOR_COUNT],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();

    env.update_asset_authority_from_admin(
        ASSET,
        percolator_prog::processor::ASSET_AUTH_ADMIN,
        ASSET_ADMIN,
    )
    .map_err(|error| format!("delegate cold asset admin: {error}"))?;

    let seized_role = match kind {
        FundedRoleKind::BackingProvider => {
            env.update_asset_authority_between_actors(
                ASSET,
                percolator_prog::processor::ASSET_AUTH_BACKING_BUCKET,
                ASSET_ADMIN,
                PROVIDER,
            )
            .map_err(|error| format!("install independent backing provider: {error}"))?;
            percolator_prog::processor::ASSET_AUTH_BACKING_BUCKET
        }
        FundedRoleKind::InsuranceOperator => {
            for authority_kind in [
                percolator_prog::processor::ASSET_AUTH_INSURANCE,
                percolator_prog::processor::ASSET_AUTH_INSURANCE_OPERATOR,
            ] {
                env.update_asset_authority_between_actors(
                    ASSET,
                    authority_kind,
                    ASSET_ADMIN,
                    PROVIDER,
                )
                .map_err(|error| {
                    format!("install independent insurance role {authority_kind}: {error}")
                })?;
            }
            percolator_prog::processor::ASSET_AUTH_INSURANCE_OPERATOR
        }
        FundedRoleKind::TerminalInsuranceAuthority => {
            env.update_asset_authority_between_actors(
                ASSET,
                percolator_prog::processor::ASSET_AUTH_INSURANCE,
                ASSET_ADMIN,
                PROVIDER,
            )
            .map_err(|error| format!("install independent insurance authority: {error}"))?;
            percolator_prog::processor::ASSET_AUTH_INSURANCE
        }
    };

    let provider_source = env.actors[PROVIDER].source_token;
    let provider_source_before = u128::from(env.token_amount(provider_source));
    match kind {
        FundedRoleKind::BackingProvider => env
            .top_up_backing_bucket_for_actor(PROVIDER, DOMAIN, PRINCIPAL, 100_000)
            .map_err(|error| format!("fund backing principal: {error}"))?,
        FundedRoleKind::InsuranceOperator | FundedRoleKind::TerminalInsuranceAuthority => env
            .top_up_insurance_domain_for_actor(PROVIDER, DOMAIN, PRINCIPAL)
            .map_err(|error| format!("fund insurance principal: {error}"))?,
    };
    let provider_source_after = u128::from(env.token_amount(provider_source));
    let provider_source_debit = provider_source_before
        .checked_sub(provider_source_after)
        .ok_or_else(|| "funded-role provider source increased".to_string())?;
    if provider_source_debit != PRINCIPAL {
        return Err(format!(
            "{kind:?} did not commit the expected principal: {provider_source_debit}"
        ));
    }

    let before_takeover = fingerprint(&env);
    let takeover =
        env.update_asset_authority_between_actors(ASSET, seized_role, ASSET_ADMIN, REPLACEMENT);
    let takeover_landed = takeover.is_ok();
    if !takeover_landed && fingerprint(&env) != before_takeover {
        return Err(format!(
            "{kind:?} rejected takeover did not roll back exactly"
        ));
    }

    if kind == FundedRoleKind::TerminalInsuranceAuthority {
        for actor in 0..PRIMARY_ACTOR_COUNT {
            env.withdraw_primary(actor, 1)
                .map_err(|error| format!("withdraw terminal fixture actor {actor}: {error}"))?;
            env.close_primary_portfolio(actor)
                .map_err(|error| format!("close terminal fixture actor {actor}: {error}"))?;
        }
        env.resolve_market()
            .map_err(|error| format!("resolve terminal insurance fixture: {error}"))?;
    }

    let replacement_destination = env.actors[REPLACEMENT].destination_token;
    let replacement_before = u128::from(env.token_amount(replacement_destination));
    if takeover_landed {
        match kind {
            FundedRoleKind::BackingProvider => env
                .withdraw_backing_bucket_for_actor(REPLACEMENT, DOMAIN, PRINCIPAL)
                .map_err(|error| format!("replacement withdraw backing principal: {error}"))?,
            FundedRoleKind::InsuranceOperator => env
                .withdraw_insurance_asset(REPLACEMENT, ASSET, PRINCIPAL)
                .map_err(|error| format!("replacement withdraw live insurance: {error}"))?,
            FundedRoleKind::TerminalInsuranceAuthority => env
                .withdraw_terminal_insurance_for_actor(REPLACEMENT, PRINCIPAL)
                .map_err(|error| format!("replacement withdraw terminal insurance: {error}"))?,
        };
    }
    let replacement_gain = u128::from(env.token_amount(replacement_destination))
        .checked_sub(replacement_before)
        .ok_or_else(|| "funded-role replacement destination decreased".to_string())?;
    if env.token_supply_observed() != supply_before {
        return Err(format!(
            "{kind:?} funded-role probe changed SPL supply: {supply_before} -> {}",
            env.token_supply_observed()
        ));
    }
    Ok(FundedRoleDiscovery {
        kind,
        provider_source_debit,
        takeover_landed,
        replacement_gain,
    })
}

pub fn discover_funded_role_seizures(seed: [u8; 32]) -> Result<Vec<FundedRoleDiscovery>, String> {
    FundedRoleKind::ALL
        .into_iter()
        .map(|kind| discover_one_funded_role_seizure(seed, kind))
        .collect()
}

fn retained_retry_pair(env: &mut V16Svm, kind: RetryIntentKind) -> (Transaction, Transaction) {
    const SUBJECT: usize = 0;
    const COUNTERPARTY: usize = 1;
    const AUTHORITY: usize = 2;
    const ASSET: u16 = 0;
    const AMOUNT: u128 = 1_000;
    let size_q = POS_SCALE as i128 / 4;
    let build = |env: &mut V16Svm| match kind {
        RetryIntentKind::Deposit => env.build_retained_deposit(SUBJECT, AMOUNT),
        RetryIntentKind::Withdraw => env.build_retained_withdrawal(SUBJECT, AMOUNT),
        RetryIntentKind::TradeNoCpi => {
            env.build_retained_no_cpi_trade(SUBJECT, COUNTERPARTY, ASSET, size_q, INITIAL_PRICE)
        }
        RetryIntentKind::TradeCpi => {
            env.build_retained_cpi_trade(SUBJECT, COUNTERPARTY, ASSET, size_q, 0)
        }
        RetryIntentKind::BatchTradeNoCpi => env.build_retained_batch_no_cpi_trade(
            SUBJECT,
            COUNTERPARTY,
            ASSET,
            size_q,
            INITIAL_PRICE,
        ),
        RetryIntentKind::BatchTradeCpi => {
            env.build_retained_batch_cpi_trade(SUBJECT, COUNTERPARTY, ASSET, size_q, 0)
        }
        RetryIntentKind::ConvertReleasedPnl => {
            env.build_retained_convert_released_pnl(SUBJECT, u128::MAX)
        }
        RetryIntentKind::RebalanceReduce => {
            env.build_retained_rebalance_reduce(SUBJECT, ASSET, POS_SCALE as u128)
        }
        RetryIntentKind::InsuranceTopUp => {
            env.build_retained_insurance_domain_top_up_for_actor(AUTHORITY, 0, AMOUNT)
        }
        RetryIntentKind::BackingTopUp => {
            env.build_retained_backing_bucket_top_up_for_actor(AUTHORITY, 1, AMOUNT, 100)
        }
        RetryIntentKind::AssetActivation => env.build_retained_permissionless_asset_activation(
            SUBJECT,
            1,
            3,
            INITIAL_PRICE,
            500,
            AUTHORITY,
            AUTHORITY,
            AUTHORITY,
            AUTHORITY,
        ),
    };
    (build(env), build(env))
}

fn discover_one_intent_retry(
    mut seed: [u8; 32],
    kind: RetryIntentKind,
) -> Result<IntentReplayDiscovery, String> {
    const AUTHORITY: usize = 2;
    seed[0] ^= 0xe1;
    seed[1] ^= kind.discriminator();
    let mut env = V16Svm::new(seed, MarketConfig::default());
    let supply_before = env.token_supply_observed();

    match kind {
        RetryIntentKind::ConvertReleasedPnl => {
            create_released_pnl(&mut env, 0, 1, 1_000_000, 1_000_000, 2, INITIAL_PRICE)?;
        }
        RetryIntentKind::RebalanceReduce => {
            env.trade_no_cpi(0, 1, 0, 2 * POS_SCALE as i128, INITIAL_PRICE, 0)
                .map_err(|error| format!("prepare retained rebalance reduction: {error}"))?;
        }
        RetryIntentKind::InsuranceTopUp => {
            env.update_asset_authority_from_admin(
                0,
                percolator_prog::processor::ASSET_AUTH_INSURANCE,
                AUTHORITY,
            )
            .map_err(|error| format!("install insurance authority: {error}"))?;
        }
        RetryIntentKind::BackingTopUp => {
            env.update_asset_authority_from_admin(
                0,
                percolator_prog::processor::ASSET_AUTH_BACKING_BUCKET,
                AUTHORITY,
            )
            .map_err(|error| format!("install backing authority: {error}"))?;
        }
        RetryIntentKind::AssetActivation => {
            env.update_market_init_fee_policy(500)
                .map_err(|error| format!("configure activation fee: {error}"))?;
            env.warp_to_slot(2);
            env.retire_asset(1, 2)
                .map_err(|error| format!("retire reusable asset slot: {error}"))?;
            env.warp_to_slot(3);
        }
        _ => {}
    }

    let (intended, retry) = retained_retry_pair(&mut env, kind);
    let first = env
        .land_retained(intended)
        .map_err(|error| format!("{kind:?} intended execution rejected: {error}"))?;

    match kind {
        RetryIntentKind::ConvertReleasedPnl => {
            create_released_pnl(&mut env, 0, 1, 1_000_000, 1_000_000, 3, INITIAL_PRICE + 5)?;
        }
        RetryIntentKind::AssetActivation => {
            env.warp_to_slot(4);
            env.retire_asset(1, 4)
                .map_err(|error| format!("retire first activated generation: {error}"))?;
            env.warp_to_slot(5);
        }
        _ => {}
    }

    let before_retry = fingerprint(&env);
    let result = env.land_retained(retry);
    let after_retry = fingerprint(&env);
    if env.token_supply_observed() != supply_before {
        return Err(format!(
            "{kind:?} retry probe changed SPL supply: {supply_before} -> {}",
            env.token_supply_observed()
        ));
    }

    match result {
        Ok(success) => {
            let duplicated_economic_effect = before_retry != after_retry;
            if !duplicated_economic_effect {
                return Err(format!(
                    "{kind:?} retry succeeded without an observable economic delta"
                ));
            }
            Ok(IntentReplayDiscovery {
                kind,
                first_compute_units: first.compute_units,
                accepted_retry: true,
                duplicated_economic_effect,
                retry_compute_units: Some(success.compute_units),
            })
        }
        Err(_) => {
            if before_retry != after_retry {
                return Err(format!("{kind:?} rejected retry did not roll back exactly"));
            }
            Ok(IntentReplayDiscovery {
                kind,
                first_compute_units: first.compute_units,
                accepted_retry: false,
                duplicated_economic_effect: false,
                retry_compute_units: None,
            })
        }
    }
}

pub fn discover_intent_retries(seed: [u8; 32]) -> Result<Vec<IntentReplayDiscovery>, String> {
    RetryIntentKind::ALL
        .into_iter()
        .map(|kind| discover_one_intent_retry(seed, kind))
        .collect()
}

fn prepare_superseded_intent(
    env: &mut V16Svm,
    kind: SupersededIntentKind,
) -> Result<Transaction, String> {
    const AUTHORITY: usize = 2;
    match kind {
        SupersededIntentKind::MatcherConfig => {
            let retained = env.build_retained_matcher_config(0, 1);
            env.set_matcher_config(0, 0)
                .map_err(|error| format!("install newer matcher policy: {error}"))?;
            Ok(retained)
        }
        SupersededIntentKind::PushAuthMark => {
            env.configure_auth_mark(false, 0, 0, INITIAL_PRICE)
                .map_err(|error| format!("configure AuthMark: {error}"))?;
            let retained = env.build_retained_auth_mark(0, INITIAL_PRICE * 9 / 10);
            env.warp_to_slot(1);
            env.push_auth_mark(0, 1, INITIAL_PRICE * 11 / 10)
                .map_err(|error| format!("install newer authenticated mark: {error}"))?;
            Ok(retained)
        }
        SupersededIntentKind::ConfigureAuthMark => {
            let retained = env.build_retained_auth_config(0, INITIAL_PRICE * 9 / 10);
            env.warp_to_slot(1);
            env.configure_auth_mark(false, 0, 1, INITIAL_PRICE * 11 / 10)
                .map_err(|error| format!("install newer AuthMark configuration: {error}"))?;
            Ok(retained)
        }
        SupersededIntentKind::PushEwmaMark => {
            env.configure_ewma_mark(0, 0, INITIAL_PRICE, 1, 0)
                .map_err(|error| format!("configure EWMA mark: {error}"))?;
            let retained = env.build_retained_ewma_mark(0, INITIAL_PRICE * 9 / 10);
            env.warp_to_slot(1);
            env.push_ewma_mark(0, 1, INITIAL_PRICE * 11 / 10)
                .map_err(|error| format!("install newer EWMA observation: {error}"))?;
            Ok(retained)
        }
        SupersededIntentKind::ConfigureEwmaMark => {
            let retained = env.build_retained_ewma_config(0, INITIAL_PRICE * 9 / 10, 1, 0);
            env.warp_to_slot(1);
            env.configure_ewma_mark(0, 1, INITIAL_PRICE * 11 / 10, 1, 0)
                .map_err(|error| format!("install newer EWMA configuration: {error}"))?;
            Ok(retained)
        }
        SupersededIntentKind::ConfigureHybridOracle => {
            const FEED_ID: [u8; 32] = [0x41; 32];
            env.set_clock(1, 100);
            let feed = env.set_pyth_price(&FEED_ID, INITIAL_PRICE as i64, 0, 0, 100);
            let retained = env.build_retained_hybrid_oracle_config(
                0,
                1,
                100,
                0,
                [FEED_ID, [0; 32], [0; 32]],
                &[feed],
                3,
                500,
            );
            env.configure_auth_mark(false, 0, 1, INITIAL_PRICE * 11 / 10)
                .map_err(|error| format!("install newer cross-mode configuration: {error}"))?;
            Ok(retained)
        }
        SupersededIntentKind::TradeFeePolicy => {
            let retained = env.build_retained_trade_fee_policy(9_000);
            env.update_trade_fee_policy(1_000)
                .map_err(|error| format!("install newer trade-fee policy: {error}"))?;
            Ok(retained)
        }
        SupersededIntentKind::FeeRedirectPolicy => {
            let retained = env.build_retained_fee_redirect_policy(9_000);
            env.update_fee_redirect_policy(1_000)
                .map_err(|error| format!("install newer fee-redirect policy: {error}"))?;
            Ok(retained)
        }
        SupersededIntentKind::LiquidationFeePolicy => {
            let retained = env.build_retained_liquidation_fee_policy(9_000);
            env.update_liquidation_fee_policy(1_000)
                .map_err(|error| format!("install newer liquidation-fee policy: {error}"))?;
            Ok(retained)
        }
        SupersededIntentKind::MaintenanceFeePolicy => {
            let retained = env.build_retained_maintenance_fee_policy(9_000);
            env.update_maintenance_fee_policy(1_000)
                .map_err(|error| format!("install newer maintenance-fee policy: {error}"))?;
            Ok(retained)
        }
        SupersededIntentKind::MarketInitFeePolicy => {
            let retained = env.build_retained_market_init_fee_policy(9_000);
            env.update_market_init_fee_policy(1_000)
                .map_err(|error| format!("install newer market-init-fee policy: {error}"))?;
            Ok(retained)
        }
        SupersededIntentKind::ResolvePolicy => {
            let retained = env.build_retained_permissionless_resolve_policy(17, 29);
            env.configure_permissionless_resolve(31, 43)
                .map_err(|error| format!("install newer resolve policy: {error}"))?;
            Ok(retained)
        }
        SupersededIntentKind::BackingFeePolicy => {
            env.update_asset_authority_from_admin(
                0,
                percolator_prog::processor::ASSET_AUTH_INSURANCE,
                AUTHORITY,
            )
            .map_err(|error| format!("install backing-fee policy authority: {error}"))?;
            let retained = env.build_retained_backing_fee_policy_for_actor(AUTHORITY, 0, 5_000, 0);
            env.update_backing_fee_policy_for_actor(AUTHORITY, 0, 0, 0)
                .map_err(|error| format!("install newer backing-fee policy: {error}"))?;
            Ok(retained)
        }
        SupersededIntentKind::BackingFeePolicyShort => {
            env.update_asset_authority_from_admin(
                0,
                percolator_prog::processor::ASSET_AUTH_INSURANCE,
                AUTHORITY,
            )
            .map_err(|error| format!("install short backing-fee policy authority: {error}"))?;
            let retained = env.build_retained_backing_fee_policy_for_actor(AUTHORITY, 1, 5_000, 0);
            env.update_backing_fee_policy_for_actor(AUTHORITY, 1, 0, 0)
                .map_err(|error| format!("install newer short backing-fee policy: {error}"))?;
            Ok(retained)
        }
    }
}

fn discover_one_superseded_intent(
    mut seed: [u8; 32],
    kind: SupersededIntentKind,
) -> Result<SupersessionDiscovery, String> {
    seed[0] ^= 0xf3;
    seed[1] ^= kind.discriminator();
    let mut env = V16Svm::new(seed, MarketConfig::default());
    let supply_before = env.token_supply_observed();
    let retained = prepare_superseded_intent(&mut env, kind)?;
    let newer_state = fingerprint(&env);
    let result = env.land_retained(retained);
    let after = fingerprint(&env);
    if env.token_supply_observed() != supply_before {
        return Err(format!(
            "{kind:?} supersession probe changed SPL supply: {supply_before} -> {}",
            env.token_supply_observed()
        ));
    }

    match result {
        Ok(success) => {
            let overwrote_newer_state = newer_state != after;
            if !overwrote_newer_state {
                return Err(format!(
                    "{kind:?} stale control succeeded without overwriting newer state"
                ));
            }
            Ok(SupersessionDiscovery {
                kind,
                accepted_stale_intent: true,
                overwrote_newer_state,
                compute_units: Some(success.compute_units),
            })
        }
        Err(_) => {
            if newer_state != after {
                return Err(format!(
                    "{kind:?} rejected stale control did not roll back exactly"
                ));
            }
            Ok(SupersessionDiscovery {
                kind,
                accepted_stale_intent: false,
                overwrote_newer_state: false,
                compute_units: None,
            })
        }
    }
}

pub fn discover_superseded_intents(seed: [u8; 32]) -> Result<Vec<SupersessionDiscovery>, String> {
    let trace = std::env::var_os("PERCOLATOR_DISCOVERY_TRACE").is_some();
    let mut discoveries = Vec::with_capacity(SupersededIntentKind::ALL.len());
    for kind in SupersededIntentKind::ALL {
        if trace {
            eprintln!("supersession probe start: {kind:?}");
        }
        let discovery = discover_one_superseded_intent(seed, kind).map_err(|error| {
            if trace {
                eprintln!("supersession probe error: {kind:?}: {error}");
            }
            error
        })?;
        if trace {
            eprintln!(
                "supersession probe finish: {kind:?} violation={}",
                discovery.is_violation()
            );
        }
        discoveries.push(discovery);
    }
    Ok(discoveries)
}

pub fn verify_matcher_mutation_order_safety(
    mut seed: [u8; 32],
) -> Result<MatcherMutationOrderDiscovery, String> {
    const PRICE: u64 = 100;
    const DEPOSIT: u128 = 1_000_000;
    const SIZE_Q: i128 = 10_000 * POS_SCALE as i128;
    const LP: usize = 0;
    const ATTACKER: usize = 1;

    seed[0] ^= 0x93;
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            actor_deposits: [DEPOSIT, DEPOSIT, 0, 0, 0],
            actor_token_balances: [2_000_000, 2_000_000, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.configure_auth_mark(false, 0, 1, PRICE)
        .map_err(|error| format!("configure matcher-order mark: {error}"))?;
    let sequence_before_revoke = env.primary_portfolio_matcher_sequence(LP);
    let retained_enable = env.build_retained_matcher_config(LP, 1);
    env.set_matcher_config(LP, 0)
        .map_err(|error| format!("revoke LP matcher: {error}"))?;
    let sequence_after_revoke = env.primary_portfolio_matcher_sequence(LP);

    let market_after_revoke = env.market_data(false);
    let lp_after_revoke = env.primary_portfolio_data(LP);
    let attacker_after_revoke = env.primary_portfolio_data(ATTACKER);
    let revoked_trade_rejected = env.trade_cpi(ATTACKER, LP, 0, SIZE_Q, 0, 0).is_err();
    if !revoked_trade_rejected
        || env.market_data(false) != market_after_revoke
        || env.primary_portfolio_data(LP) != lp_after_revoke
        || env.primary_portfolio_data(ATTACKER) != attacker_after_revoke
    {
        return Err("revoked matcher did not reject CPI fill atomically".into());
    }

    let stale_state = fingerprint(&env);
    let stale_enable_rejected = env.land_retained(retained_enable).is_err();
    let stale_enable_exact_rollback = fingerprint(&env) == stale_state;
    let sequence_after_stale = env.primary_portfolio_matcher_sequence(LP);
    let post_stale_state = fingerprint(&env);
    let post_stale_trade_rejected = env.trade_cpi(ATTACKER, LP, 0, SIZE_Q, 0, 0).is_err()
        && fingerprint(&env) == post_stale_state;
    if !stale_enable_rejected
        || !stale_enable_exact_rollback
        || !post_stale_trade_rejected
        || sequence_after_stale != sequence_after_revoke
    {
        return Err(format!(
            "stale matcher enable was not rejected atomically: rejected={stale_enable_rejected}, \
             rollback={stale_enable_exact_rollback}, trade_rejected={post_stale_trade_rejected}, \
             sequence={sequence_after_revoke}/{sequence_after_stale}"
        ));
    }

    env.set_matcher_config(LP, 1)
        .map_err(|error| format!("fresh matcher enable rejected: {error}"))?;
    let sequence_after_fresh = env.primary_portfolio_matcher_sequence(LP);
    let fresh_enable_landed = sequence_after_fresh == sequence_after_revoke + 1;
    env.trade_cpi(ATTACKER, LP, 0, SIZE_Q, 0, 0)
        .map_err(|error| format!("fresh matcher open rejected: {error}"))?;
    env.trade_cpi(ATTACKER, LP, 0, -SIZE_Q, 0, 0)
        .map_err(|error| format!("fresh matcher close rejected: {error}"))?;
    let fresh_round_trip_landed = env.primary_portfolio(LP).capital.get() == DEPOSIT
        && env.primary_portfolio(ATTACKER).capital.get() == DEPOSIT;
    let lp_withdrawal = env
        .withdraw_primary(LP, DEPOSIT)
        .map_err(|error| format!("fresh matcher LP exit failed: {error}"))?;
    let attacker_withdrawal = env
        .withdraw_primary(ATTACKER, DEPOSIT)
        .map_err(|error| format!("fresh matcher taker exit failed: {error}"))?;
    let total_payout = u128::from(env.token_amount(env.actors[LP].destination_token))
        + u128::from(env.token_amount(env.actors[ATTACKER].destination_token));
    let token_supply_conserved = env.token_supply_observed() == supply_before;
    if !fresh_enable_landed
        || !fresh_round_trip_landed
        || total_payout != 2 * DEPOSIT
        || lp_withdrawal.compute_units >= crate::support::v16_svm::TX_CU_LIMIT
        || attacker_withdrawal.compute_units >= crate::support::v16_svm::TX_CU_LIMIT
        || !token_supply_conserved
    {
        return Err(format!(
            "fresh matcher control failed: enable={fresh_enable_landed}, \
             round_trip={fresh_round_trip_landed}, payout={total_payout}, \
             supply={token_supply_conserved}"
        ));
    }
    Ok(MatcherMutationOrderDiscovery {
        revoked_trade_rejected,
        stale_enable_rejected,
        stale_enable_exact_rollback,
        post_stale_trade_rejected,
        fresh_enable_landed,
        fresh_round_trip_landed,
        sequence_before_revoke,
        sequence_after_revoke,
        sequence_after_stale,
        sequence_after_fresh,
        total_payout,
        token_supply_conserved,
    })
}

fn finish_fee_consent_discovery(
    env: &V16Svm,
    kind: FeeConsentKind,
    before: EconomicFingerprint,
    execution: Result<u64, String>,
    terms_were_unconsented: bool,
    authorized_debit: u128,
    observed_debit: u128,
    supply_before: u128,
) -> Result<FeeConsentDiscovery, String> {
    let after = fingerprint(env);
    if env.token_supply_observed() != supply_before {
        return Err(format!(
            "{kind:?} consent probe changed SPL supply: {supply_before} -> {}",
            env.token_supply_observed()
        ));
    }
    match execution {
        Ok(compute_units) => {
            let mutated_economic_state = before != after;
            if !mutated_economic_state {
                return Err(format!(
                    "{kind:?} accepted unconsented terms without an observable state delta"
                ));
            }
            Ok(FeeConsentDiscovery {
                kind,
                accepted_unconsented_terms: terms_were_unconsented,
                mutated_economic_state,
                authorized_debit,
                observed_debit,
                compute_units: Some(compute_units),
            })
        }
        Err(_) => {
            if before != after {
                return Err(format!("{kind:?} rejected terms did not roll back exactly"));
            }
            if observed_debit != 0 {
                return Err(format!(
                    "{kind:?} rejected terms still debited {observed_debit} atoms"
                ));
            }
            Ok(FeeConsentDiscovery {
                kind,
                accepted_unconsented_terms: false,
                mutated_economic_state: false,
                authorized_debit,
                observed_debit,
                compute_units: None,
            })
        }
    }
}

fn total_capital(env: &V16Svm, actors: &[usize]) -> Result<u128, String> {
    actors.iter().try_fold(0u128, |total, &actor| {
        total
            .checked_add(env.primary_portfolio(actor).capital.get())
            .ok_or_else(|| "portfolio capital total overflow".to_string())
    })
}

fn debit_between(before: u128, after: u128, context: &str) -> Result<u128, String> {
    before
        .checked_sub(after)
        .ok_or_else(|| format!("{context} increased signer value from {before} to {after}"))
}

fn discover_trade_fee_consent_violation(
    seed: [u8; 32],
    kind: FeeConsentKind,
) -> Result<FeeConsentDiscovery, String> {
    const TAKER: usize = 0;
    const LP: usize = 1;
    const BASE_FEE_BPS: u64 = 500;
    const CALLER_FEE_BPS: u64 = 10_000;
    let size_q = POS_SCALE as i128;
    let mut env = V16Svm::new(seed, MarketConfig::default());
    let supply_before = env.token_supply_observed();

    if kind == FeeConsentKind::FreshSignedLiveBaseFee {
        env.trade_no_cpi(TAKER, LP, 0, size_q, INITIAL_PRICE, 0)
            .map_err(|error| format!("open under original live fee: {error}"))?;
        let capital_before = total_capital(&env, &[TAKER, LP])?;
        let before = fingerprint(&env);
        let update = env.update_trade_fee_policy(BASE_FEE_BPS);
        let execution = match update {
            Ok(update) => env
                .trade_no_cpi(TAKER, LP, 0, -size_q, INITIAL_PRICE, BASE_FEE_BPS)
                .map(|close| update.compute_units.max(close.compute_units)),
            Err(error) => Err(error),
        };
        let observed_debit = debit_between(
            capital_before,
            total_capital(&env, &[TAKER, LP])?,
            "live base-fee hike",
        )?;
        const SIGNED_CLOSE_DEBIT: u128 = 100_000;
        if observed_debit != SIGNED_CLOSE_DEBIT {
            return Err(format!(
                "fresh signed live fee debited {observed_debit}, expected {SIGNED_CLOSE_DEBIT}"
            ));
        }
        return finish_fee_consent_discovery(
            &env,
            kind,
            before,
            execution,
            false,
            SIGNED_CLOSE_DEBIT,
            observed_debit,
            supply_before,
        );
    }

    let retained = match kind {
        FeeConsentKind::RetainedNoCpiBaseFee => {
            Some(env.build_retained_no_cpi_trade(TAKER, LP, 0, size_q, INITIAL_PRICE))
        }
        FeeConsentKind::RetainedBatchNoCpiBaseFee => {
            Some(env.build_retained_batch_no_cpi_trade(TAKER, LP, 0, size_q, INITIAL_PRICE))
        }
        _ => None,
    };
    if matches!(
        kind,
        FeeConsentKind::CpiBaseFee | FeeConsentKind::BatchCpiBaseFee
    ) {
        env.set_matcher_config_with_trade_fee_cap(LP, 1, 0)
            .map_err(|error| format!("bind zero LP base-fee consent: {error}"))?;
    }
    if matches!(
        kind,
        FeeConsentKind::RetainedNoCpiBaseFee
            | FeeConsentKind::RetainedBatchNoCpiBaseFee
            | FeeConsentKind::CpiBaseFee
            | FeeConsentKind::BatchCpiBaseFee
    ) {
        env.update_trade_fee_policy(BASE_FEE_BPS)
            .map_err(|error| format!("install post-consent base fee: {error}"))?;
    }

    let capital_before = match kind {
        FeeConsentKind::CpiBaseFee
        | FeeConsentKind::BatchCpiBaseFee
        | FeeConsentKind::CpiCallerFee
        | FeeConsentKind::BatchCpiCallerFee => env.primary_portfolio(LP).capital.get(),
        _ => total_capital(&env, &[TAKER, LP])?,
    };
    let trade_market_id = env.primary_market_state().1.assets[0].market_id;
    let before = fingerprint(&env);
    let execution = match kind {
        FeeConsentKind::RetainedNoCpiBaseFee | FeeConsentKind::RetainedBatchNoCpiBaseFee => env
            .land_retained(retained.expect("retained bilateral trade"))
            .map(|success| success.compute_units),
        FeeConsentKind::CpiBaseFee => env
            .trade_cpi(TAKER, LP, 0, size_q, 0, 0)
            .map(|success| success.compute_units),
        FeeConsentKind::BatchCpiBaseFee => env
            .batch_trade_cpi(
                TAKER,
                LP,
                vec![BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: trade_market_id,
                    size_q,
                    fee_bps: 0,
                    limit_price: 0,
                }],
            )
            .map(|success| success.compute_units),
        FeeConsentKind::CpiCallerFee => env
            .trade_cpi(TAKER, LP, 0, size_q, CALLER_FEE_BPS, 0)
            .map(|success| success.compute_units),
        FeeConsentKind::BatchCpiCallerFee => env
            .batch_trade_cpi(
                TAKER,
                LP,
                vec![BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: trade_market_id,
                    size_q,
                    fee_bps: CALLER_FEE_BPS,
                    limit_price: 0,
                }],
            )
            .map(|success| success.compute_units),
        FeeConsentKind::FreshSignedLiveBaseFee | FeeConsentKind::PermissionlessActivationFee => {
            unreachable!()
        }
    };
    let capital_after = match kind {
        FeeConsentKind::CpiBaseFee
        | FeeConsentKind::BatchCpiBaseFee
        | FeeConsentKind::CpiCallerFee
        | FeeConsentKind::BatchCpiCallerFee => env.primary_portfolio(LP).capital.get(),
        _ => total_capital(&env, &[TAKER, LP])?,
    };
    let observed_debit = debit_between(capital_before, capital_after, "trade fee consent")?;
    finish_fee_consent_discovery(
        &env,
        kind,
        before,
        execution,
        true,
        0,
        observed_debit,
        supply_before,
    )
}

fn discover_activation_fee_consent_violation(
    seed: [u8; 32],
) -> Result<FeeConsentDiscovery, String> {
    const CREATOR: usize = 0;
    const ASSET: u16 = 1;
    const ADVERTISED_FEE: u128 = 1;
    const CHANGED_FEE: u128 = 1_000;
    let kind = FeeConsentKind::PermissionlessActivationFee;
    let mut env = V16Svm::new(seed, MarketConfig::default());
    let supply_before = env.token_supply_observed();
    env.warp_to_slot(2);
    env.retire_asset(ASSET, 2)
        .map_err(|error| format!("retire activation slot: {error}"))?;
    env.update_market_init_fee_policy(ADVERTISED_FEE)
        .map_err(|error| format!("publish activation fee: {error}"))?;
    env.warp_to_slot(3);
    let retained = env.build_retained_permissionless_asset_activation(
        CREATOR,
        ASSET,
        3,
        100,
        ADVERTISED_FEE,
        CREATOR,
        CREATOR,
        CREATOR,
        CREATOR,
    );
    env.update_market_init_fee_policy(CHANGED_FEE)
        .map_err(|error| format!("change activation fee after consent: {error}"))?;
    let source = env.actors[CREATOR].source_token;
    let source_before = u128::from(env.token_amount(source));
    let before = fingerprint(&env);
    let execution = env
        .land_retained(retained)
        .map(|success| success.compute_units);
    let observed_debit = debit_between(
        source_before,
        u128::from(env.token_amount(source)),
        "permissionless activation fee",
    )?;
    finish_fee_consent_discovery(
        &env,
        kind,
        before,
        execution,
        true,
        ADVERTISED_FEE,
        observed_debit,
        supply_before,
    )
}

fn discover_one_fee_consent_violation(
    mut seed: [u8; 32],
    kind: FeeConsentKind,
) -> Result<FeeConsentDiscovery, String> {
    seed[0] ^= 0x6d;
    seed[1] ^= kind.discriminator();
    match kind {
        FeeConsentKind::PermissionlessActivationFee => {
            discover_activation_fee_consent_violation(seed)
        }
        _ => discover_trade_fee_consent_violation(seed, kind),
    }
}

pub fn discover_fee_consent_violations(seed: [u8; 32]) -> Result<Vec<FeeConsentDiscovery>, String> {
    FeeConsentKind::ALL
        .into_iter()
        .map(|kind| discover_one_fee_consent_violation(seed, kind))
        .collect()
}

fn crank_discovery_steps_for_assets(
    env: &mut V16Svm,
    actor: usize,
    slot: u64,
    asset_indices: &[u16],
) -> Result<(), String> {
    let observations = asset_indices
        .iter()
        .copied()
        .map(|asset_index| CrankObservationHint {
            asset_index,
            oracle_accounts: env.primary_profile(asset_index as usize).oracle_leg_count,
        })
        .collect::<Vec<_>>();
    for step in 0..4 {
        env.crank(actor, slot, observations.clone())
            .map_err(|error| {
                format!(
                    "source-fee crank actor {actor} assets {asset_indices:?} step {step}: {error}"
                )
            })?;
    }
    Ok(())
}

fn prepare_source_backed_fee_world(seed: [u8; 32]) -> Result<V16Svm, String> {
    const PROVIDER: usize = 0;
    const MARKET_TRADER: usize = 1;
    const LP: usize = 2;
    const ASSET: u16 = 1;
    const WINNING_DOMAIN: u16 = ASSET * 2 + 1;
    const PRICE: u64 = 100;
    const WINNING_SIZE_Q: i128 = 200 * POS_SCALE as i128;
    const LOSING_SIZE_Q: i128 = 100 * POS_SCALE as i128;
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            h_max: 2,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            maintenance_fee_per_slot: 30,
            actor_deposits: [10_000, 10_000, 3_190, USER_DEPOSIT, USER_DEPOSIT],
            ..MarketConfig::default()
        },
    );
    env.update_market_init_fee_policy(1)
        .map_err(|error| format!("configure source asset activation: {error}"))?;
    env.configure_auth_mark(false, 0, 1, PRICE)
        .map_err(|error| format!("configure base source-fee mark: {error}"))?;
    env.warp_to_slot(2);
    env.retire_asset(ASSET, 2)
        .map_err(|error| format!("retire source asset slot: {error}"))?;
    env.warp_to_slot(3);
    env.activate_permissionless_asset_for_actor(PROVIDER, ASSET, 3, PRICE, PROVIDER, 1)
        .map_err(|error| format!("activate source-backed asset: {error}"))?;
    env.configure_auth_mark_for_actor(PROVIDER, ASSET, 3, PRICE)
        .map_err(|error| format!("configure source-backed mark: {error}"))?;
    env.top_up_backing_bucket_for_actor(PROVIDER, WINNING_DOMAIN, 5_000, 100)
        .map_err(|error| format!("fund source-backing bucket: {error}"))?;
    env.trade_no_cpi(MARKET_TRADER, LP, ASSET, -WINNING_SIZE_Q, PRICE, 0)
        .map_err(|error| format!("open source-backed winning leg: {error}"))?;
    env.trade_no_cpi(MARKET_TRADER, LP, 0, -LOSING_SIZE_Q, PRICE, 0)
        .map_err(|error| format!("open source-backed losing leg: {error}"))?;

    env.warp_to_slot(4);
    env.push_auth_mark_for_actor(PROVIDER, ASSET, 4, PRICE)
        .map_err(|error| format!("prime source-backed mark: {error}"))?;
    env.push_auth_mark(0, 4, PRICE)
        .map_err(|error| format!("prime base mark: {error}"))?;
    for actor in [MARKET_TRADER, LP] {
        crank_discovery_steps_for_assets(&mut env, actor, 4, &[ASSET, 0])?;
    }
    env.sync_maintenance_fee(LP, 4)
        .map_err(|error| format!("sync source-backed LP maintenance fee: {error}"))?;

    env.warp_to_slot(5);
    env.push_auth_mark_for_actor(PROVIDER, ASSET, 5, PRICE + 5)
        .map_err(|error| format!("publish source-backed winning mark: {error}"))?;
    env.push_auth_mark(0, 5, PRICE - 5)
        .map_err(|error| format!("publish offsetting losing mark: {error}"))?;
    for actor in [MARKET_TRADER, LP] {
        crank_discovery_steps_for_assets(&mut env, actor, 5, &[ASSET, 0])?;
    }
    if env.primary_portfolio(LP).pnl.get() <= 0 {
        return Err(format!(
            "source-fee fixture produced no positive LP claim: {}",
            env.primary_portfolio(LP).pnl.get()
        ));
    }
    Ok(env)
}

fn discover_one_source_fee_consent_violation(
    mut seed: [u8; 32],
    kind: SourceFeeConsentKind,
) -> Result<SourceFeeConsentDiscovery, String> {
    const PROVIDER: usize = 0;
    const LP: usize = 2;
    const WINNING_DOMAIN: u16 = 3;
    const INCREASE_Q: i128 = 20 * POS_SCALE as i128;
    const EXEC_PRICE: u64 = 95;
    seed[0] ^= 0x7b;
    seed[1] ^= kind.discriminator();
    let mut env = prepare_source_backed_fee_world(seed)?;
    let supply_before = env.token_supply_observed();
    let retained = match kind {
        SourceFeeConsentKind::NoCpi => {
            Some(env.build_retained_no_cpi_trade(PROVIDER, LP, 0, -INCREASE_Q, EXEC_PRICE))
        }
        SourceFeeConsentKind::BatchNoCpi => {
            Some(env.build_retained_batch_no_cpi_trade(PROVIDER, LP, 0, -INCREASE_Q, EXEC_PRICE))
        }
        SourceFeeConsentKind::Cpi | SourceFeeConsentKind::BatchCpi => None,
    };
    env.update_backing_fee_policy_for_actor(PROVIDER, WINNING_DOMAIN, 5_000, 0)
        .map_err(|error| format!("install post-consent source-backing fee: {error}"))?;
    env.warp_to_slot(6);
    let lp_before = env.primary_portfolio(LP).capital.get();
    let trade_market_id = env.primary_market_state().1.assets[0].market_id;
    let provider_before = env.primary_market_state().1.source_backing_buckets
        [WINNING_DOMAIN as usize]
        .utilization_fee_earnings;
    let before = fingerprint(&env);
    let execution = match kind {
        SourceFeeConsentKind::NoCpi | SourceFeeConsentKind::BatchNoCpi => env
            .land_retained(retained.expect("retained source-fee trade"))
            .map(|success| success.compute_units),
        SourceFeeConsentKind::Cpi => env
            .trade_cpi(PROVIDER, LP, 0, -INCREASE_Q, 0, 0)
            .map(|success| success.compute_units),
        SourceFeeConsentKind::BatchCpi => env
            .batch_trade_cpi(
                PROVIDER,
                LP,
                vec![BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: trade_market_id,
                    size_q: -INCREASE_Q,
                    fee_bps: 0,
                    limit_price: 0,
                }],
            )
            .map(|success| success.compute_units),
    };
    let after = fingerprint(&env);
    let lp_capital_debit = debit_between(
        lp_before,
        env.primary_portfolio(LP).capital.get(),
        "source-backed LP fee",
    )?;
    let provider_earnings_credit = env.primary_market_state().1.source_backing_buckets
        [WINNING_DOMAIN as usize]
        .utilization_fee_earnings
        .checked_sub(provider_before)
        .ok_or_else(|| "source-backing provider earnings decreased".to_string())?;
    if env.token_supply_observed() != supply_before {
        return Err(format!(
            "{kind:?} source-fee probe changed SPL supply: {supply_before} -> {}",
            env.token_supply_observed()
        ));
    }
    match execution {
        Ok(compute_units) => {
            if before == after {
                return Err(format!(
                    "{kind:?} source-fee trade succeeded without state mutation"
                ));
            }
            Ok(SourceFeeConsentDiscovery {
                kind,
                accepted_unconsented_fee: true,
                lp_capital_debit,
                provider_earnings_credit,
                compute_units: Some(compute_units),
            })
        }
        Err(_) => {
            if before != after || lp_capital_debit != 0 || provider_earnings_credit != 0 {
                return Err(format!(
                    "{kind:?} rejected source-fee trade did not roll back exactly"
                ));
            }
            Ok(SourceFeeConsentDiscovery {
                kind,
                accepted_unconsented_fee: false,
                lp_capital_debit: 0,
                provider_earnings_credit: 0,
                compute_units: None,
            })
        }
    }
}

pub fn discover_source_fee_consent_violations(
    seed: [u8; 32],
) -> Result<Vec<SourceFeeConsentDiscovery>, String> {
    SourceFeeConsentKind::ALL
        .into_iter()
        .map(|kind| discover_one_source_fee_consent_violation(seed, kind))
        .collect()
}

fn rejected_backing_provider_consent(
    env: &V16Svm,
    order: BackingProviderConsentOrder,
    before: EconomicFingerprint,
    supply_before: u128,
) -> Result<BackingProviderConsentDiscovery, String> {
    if before != fingerprint(env) {
        return Err(format!(
            "{order:?} rejected provider-policy transition did not roll back exactly"
        ));
    }
    if env.token_supply_observed() != supply_before {
        return Err(format!(
            "{order:?} rejected provider-policy transition changed SPL supply"
        ));
    }
    Ok(BackingProviderConsentDiscovery {
        order,
        accepted_provider_terms: false,
        lp_capital_debit: 0,
        provider_earnings_credit: 0,
        operator_insurance_credit: 0,
        operator_withdrawn: 0,
        compute_units: None,
    })
}

fn discover_one_backing_provider_consent_violation(
    mut seed: [u8; 32],
    order: BackingProviderConsentOrder,
) -> Result<BackingProviderConsentDiscovery, String> {
    const MARKET_TRADER: usize = 0;
    const PROVIDER: usize = 1;
    const POLICY_AUTHORITY: usize = 2;
    const OPERATOR: usize = 3;
    const LP: usize = PRIMARY_ACTOR_COUNT - 1;
    const ASSET: u16 = 1;
    const WINNING_DOMAIN: u16 = ASSET * 2 + 1;
    const PRICE: u64 = 100;
    const WINNING_SIZE_Q: i128 = 200 * POS_SCALE as i128;
    const LOSING_SIZE_Q: i128 = 100 * POS_SCALE as i128;
    const INCREASE_Q: i128 = 20 * POS_SCALE as i128;
    const BACKING_PRINCIPAL: u128 = 5_000;
    const BACKING_FEE_BPS: u16 = 5_000;
    seed[0] ^= 0x8f;
    seed[1] ^= order.discriminator();
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            h_max: 2,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            maintenance_fee_per_slot: 30,
            actor_deposits: [10_000, 1, 1, 10_000, 3_130],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    for (authority_kind, actor) in [
        (
            percolator_prog::processor::ASSET_AUTH_INSURANCE,
            POLICY_AUTHORITY,
        ),
        (
            percolator_prog::processor::ASSET_AUTH_INSURANCE_OPERATOR,
            OPERATOR,
        ),
        (
            percolator_prog::processor::ASSET_AUTH_BACKING_BUCKET,
            PROVIDER,
        ),
        (
            percolator_prog::processor::ASSET_AUTH_ORACLE,
            POLICY_AUTHORITY,
        ),
    ] {
        env.update_asset_authority_from_admin(ASSET, authority_kind, actor)
            .map_err(|error| {
                format!("install provider-consent authority {authority_kind}: {error}")
            })?;
    }
    env.update_backing_fee_policy_for_actor(POLICY_AUTHORITY, WINNING_DOMAIN, BACKING_FEE_BPS, 0)
        .map_err(|error| format!("publish provider-approved fee split: {error}"))?;
    let retained_top_up = env.build_retained_backing_bucket_top_up_for_actor(
        PROVIDER,
        WINNING_DOMAIN,
        BACKING_PRINCIPAL,
        100,
    );

    let mut max_cu = 0;
    match order {
        BackingProviderConsentOrder::FundThenPolicy => {
            let top_up = env
                .top_up_backing_bucket_for_actor(PROVIDER, WINNING_DOMAIN, BACKING_PRINCIPAL, 100)
                .map_err(|error| format!("fund provider-approved bucket: {error}"))?;
            max_cu = max_cu.max(top_up.compute_units);
            let before_policy = fingerprint(&env);
            match env.update_backing_fee_policy_for_actor(
                POLICY_AUTHORITY,
                WINNING_DOMAIN,
                BACKING_FEE_BPS,
                10_000,
            ) {
                Ok(policy) => max_cu = max_cu.max(policy.compute_units),
                Err(_) => {
                    return rejected_backing_provider_consent(
                        &env,
                        order,
                        before_policy,
                        supply_before,
                    );
                }
            }
        }
        BackingProviderConsentOrder::PolicyThenRetainedFund => {
            let policy = env
                .update_backing_fee_policy_for_actor(
                    POLICY_AUTHORITY,
                    WINNING_DOMAIN,
                    BACKING_FEE_BPS,
                    10_000,
                )
                .map_err(|error| format!("replace provider-visible fee split: {error}"))?;
            max_cu = max_cu.max(policy.compute_units);
            let before_top_up = fingerprint(&env);
            match env.land_retained(retained_top_up) {
                Ok(top_up) => max_cu = max_cu.max(top_up.compute_units),
                Err(_) => {
                    return rejected_backing_provider_consent(
                        &env,
                        order,
                        before_top_up,
                        supply_before,
                    );
                }
            }
        }
    }

    env.trade_no_cpi(MARKET_TRADER, LP, ASSET, -WINNING_SIZE_Q, PRICE, 0)
        .map_err(|error| format!("open provider-backed winning leg: {error}"))?;
    env.trade_no_cpi(MARKET_TRADER, LP, 0, -LOSING_SIZE_Q, PRICE, 0)
        .map_err(|error| format!("open provider-backed losing leg: {error}"))?;
    env.warp_to_slot(2);
    env.push_auth_mark_for_actor(POLICY_AUTHORITY, ASSET, 2, PRICE)
        .map_err(|error| format!("prime provider-backed mark: {error}"))?;
    env.push_auth_mark(0, 2, PRICE)
        .map_err(|error| format!("prime provider base mark: {error}"))?;
    for actor in [MARKET_TRADER, LP] {
        crank_discovery_steps_for_assets(&mut env, actor, 2, &[ASSET, 0])?;
    }
    env.sync_maintenance_fee(LP, 2)
        .map_err(|error| format!("sync provider-backed LP fee: {error}"))?;
    env.warp_to_slot(3);
    env.push_auth_mark_for_actor(POLICY_AUTHORITY, ASSET, 3, PRICE + 5)
        .map_err(|error| format!("publish provider-backed winning mark: {error}"))?;
    env.push_auth_mark(0, 3, PRICE - 5)
        .map_err(|error| format!("publish provider-backed losing mark: {error}"))?;
    for actor in [MARKET_TRADER, LP] {
        crank_discovery_steps_for_assets(&mut env, actor, 3, &[ASSET, 0])?;
    }
    if env.primary_portfolio(LP).pnl.get() <= 0 {
        return Err(format!(
            "provider-consent fixture produced no positive LP claim: {}",
            env.primary_portfolio(LP).pnl.get()
        ));
    }

    let retained_trade = env.build_retained_no_cpi_trade(OPERATOR, LP, 0, -INCREASE_Q, PRICE - 5);
    let before = env.primary_market_state().1;
    let provider_before =
        before.source_backing_buckets[WINNING_DOMAIN as usize].utilization_fee_earnings;
    let operator_before = before.insurance_domain_budget[WINNING_DOMAIN as usize];
    let lp_before = env.primary_portfolio(LP).capital.get();
    let trade = env
        .land_retained(retained_trade)
        .map_err(|error| format!("land provider-fee-generating trade: {error}"))?;
    max_cu = max_cu.max(trade.compute_units);
    let after = env.primary_market_state().1;
    let provider_earnings_credit = after.source_backing_buckets[WINNING_DOMAIN as usize]
        .utilization_fee_earnings
        .checked_sub(provider_before)
        .ok_or_else(|| "provider earnings decreased".to_string())?;
    let operator_insurance_credit = after.insurance_domain_budget[WINNING_DOMAIN as usize]
        .checked_sub(operator_before)
        .ok_or_else(|| "operator insurance budget decreased".to_string())?;
    let lp_capital_debit = debit_between(
        lp_before,
        env.primary_portfolio(LP).capital.get(),
        "provider-consent LP debit",
    )?;
    if provider_earnings_credit
        .checked_add(operator_insurance_credit)
        .ok_or_else(|| "provider fee split overflow".to_string())?
        != lp_capital_debit
    {
        return Err(format!(
            "provider fee attribution mismatch: lp={lp_capital_debit}, provider={provider_earnings_credit}, operator={operator_insurance_credit}"
        ));
    }

    let operator_destination = env.actors[OPERATOR].destination_token;
    let operator_destination_before = env.token_amount(operator_destination);
    if operator_insurance_credit != 0 {
        let withdrawal = env
            .withdraw_insurance_asset(OPERATOR, ASSET, operator_insurance_credit)
            .map_err(|error| format!("withdraw redirected provider fee: {error}"))?;
        max_cu = max_cu.max(withdrawal.compute_units);
    }
    let operator_withdrawn = env
        .token_amount(operator_destination)
        .checked_sub(operator_destination_before)
        .ok_or_else(|| "operator destination decreased".to_string())?;
    if env.token_supply_observed() != supply_before {
        return Err(format!(
            "{order:?} provider-consent probe changed SPL supply: {supply_before} -> {}",
            env.token_supply_observed()
        ));
    }
    Ok(BackingProviderConsentDiscovery {
        order,
        accepted_provider_terms: true,
        lp_capital_debit,
        provider_earnings_credit,
        operator_insurance_credit,
        operator_withdrawn,
        compute_units: Some(max_cu),
    })
}

pub fn discover_backing_provider_consent_violations(
    seed: [u8; 32],
) -> Result<Vec<BackingProviderConsentDiscovery>, String> {
    BackingProviderConsentOrder::ALL
        .into_iter()
        .map(|order| discover_one_backing_provider_consent_violation(seed, order))
        .collect()
}

#[derive(Clone, Copy, Debug)]
struct AccrualOrderingWorld {
    attacker_claim: u128,
    victim_claim: u128,
    paid: u128,
    received: u128,
    unsafe_action_rejected: bool,
    rejected_exact_rollback: bool,
    retry_landed: bool,
}

fn accrual_ordering_snapshot(env: &V16Svm) -> (Vec<u8>, Vec<Vec<u8>>, Vec<u64>, u128) {
    (
        env.market_data(false),
        (0..PRIMARY_ACTOR_COUNT)
            .map(|actor| env.primary_portfolio_data(actor))
            .collect(),
        (0..PRIMARY_ACTOR_COUNT)
            .map(|actor| env.token_amount(env.actors[actor].destination_token))
            .collect(),
        env.token_supply_observed(),
    )
}

fn is_engine_stale_error(error: &str) -> bool {
    error.contains("Custom(19)") || error.contains("custom program error: 0x13")
}

fn zero_move_funding_discovery_world(seed: [u8; 32]) -> Result<V16Svm, String> {
    const PRICE: u64 = 2;
    const TARGET: u64 = 1;
    const DEPOSIT: u128 = 1_000_000;
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            max_price_move_bps_per_slot: 24,
            max_accrual_dt_slots: 1,
            max_abs_funding_e9_per_slot: 1_000,
            min_funding_lifetime_slots: 1,
            actor_deposits: [DEPOSIT, DEPOSIT, DEPOSIT, DEPOSIT, USER_DEPOSIT],
            ..MarketConfig::default()
        },
    );
    env.warp_to_slot(2);
    env.push_auth_mark(0, 2, TARGET)
        .map_err(|error| format!("stage zero-move funding target: {error}"))?;
    Ok(env)
}

fn zero_move_observation_discovery(env: &V16Svm) -> Vec<CrankObservationHint> {
    vec![CrankObservationHint {
        asset_index: 0,
        oracle_accounts: env.primary_profile(0).oracle_leg_count,
    }]
}

fn prime_zero_move_funding_discovery(env: &mut V16Svm, settlement_slot: u64) -> Result<(), String> {
    for actor in [0, 1] {
        env.crank(actor, 2, zero_move_observation_discovery(env))
            .map_err(|error| format!("prime zero-move actor {actor}: {error}"))?;
    }
    let (_, group) = env.primary_market_state();
    if group.assets[0].effective_price != 2
        || group.assets[0].f_long_num != 0
        || group.assets[0].f_short_num != 0
    {
        return Err(format!(
            "zero-move prime unexpectedly changed price/funding: price={}, F=({}, {})",
            group.assets[0].effective_price,
            group.assets[0].f_long_num,
            group.assets[0].f_short_num
        ));
    }
    if settlement_slot <= 2 {
        return Err("zero-move settlement slot must follow checkpoint activation".into());
    }
    env.warp_to_slot(settlement_slot);
    Ok(())
}

fn accrue_zero_move_asset_to_slot(env: &mut V16Svm, settlement_slot: u64) -> Result<(), String> {
    for _ in 0..16 {
        if env.primary_market_state().1.assets[0].slot_last >= settlement_slot {
            return Ok(());
        }
        env.crank(4, settlement_slot, zero_move_observation_discovery(env))
            .map_err(|error| format!("advance zero-move asset cursor: {error}"))?;
    }
    Err(format!(
        "zero-move asset did not reach slot {settlement_slot} in bounded cranks"
    ))
}

fn settle_zero_move_actors(
    env: &mut V16Svm,
    settlement_slot: u64,
    actors: &[usize],
) -> Result<(), String> {
    accrue_zero_move_asset_to_slot(env, settlement_slot)?;
    for &actor in actors {
        env.crank(actor, settlement_slot, zero_move_observation_discovery(env))
            .map_err(|error| format!("settle zero-move actor {actor}: {error}"))?;
    }
    Ok(())
}

fn portfolio_claim(env: &V16Svm, actor: usize) -> Result<u128, String> {
    let portfolio = env.primary_portfolio(actor);
    let claim = i128::try_from(portfolio.capital.get())
        .map_err(|_| format!("actor {actor} capital exceeds i128"))?
        .checked_add(portfolio.pnl.get())
        .ok_or_else(|| format!("actor {actor} claim overflow"))?;
    u128::try_from(claim).map_err(|_| format!("actor {actor} claim became negative"))
}

fn funding_totals(env: &V16Svm, payer: usize, receiver: usize) -> (u128, u128) {
    (
        env.primary_portfolio(payer)
            .funding_short_paid_atoms_total
            .get(),
        env.primary_portfolio(receiver)
            .funding_long_received_atoms_total
            .get(),
    )
}

fn execute_cpi_close_kind(
    env: &mut V16Svm,
    kind: AccrualOrderingKind,
    size_q: i128,
) -> Result<(), String> {
    let market_id = env.primary_market_state().1.assets[0].market_id;
    match kind {
        AccrualOrderingKind::CpiTradeClose => env
            .trade_cpi(0, 1, 0, size_q, 0, 0)
            .map(|_| ())
            .map_err(|error| format!("CPI accrual-boundary trade: {error}")),
        AccrualOrderingKind::BatchCpiTradeClose => env
            .batch_trade_cpi(
                0,
                1,
                vec![BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id,
                    size_q,
                    fee_bps: 0,
                    limit_price: 0,
                }],
            )
            .map(|_| ())
            .map_err(|error| format!("batch CPI accrual-boundary trade: {error}")),
        _ => unreachable!(),
    }
}

fn run_trade_accrual_ordering_world(
    seed: [u8; 32],
    kind: AccrualOrderingKind,
    action_before_settlement: bool,
    settlement_slot: u64,
    recover_rejected_action: bool,
) -> Result<AccrualOrderingWorld, String> {
    const PRICE: u64 = 2;
    const Q: i128 = 100 * POS_SCALE as i128;
    let mut env = zero_move_funding_discovery_world(seed)?;
    let supply_before = env.token_supply_observed();
    execute_cpi_close_kind(&mut env, kind, -Q)?;
    prime_zero_move_funding_discovery(&mut env, settlement_slot)?;
    if !action_before_settlement {
        settle_zero_move_actors(&mut env, settlement_slot, &[0, 1])?;
    }
    let mut unsafe_action_rejected = false;
    let mut rejected_exact_rollback = false;
    let mut retry_landed = false;
    if action_before_settlement && recover_rejected_action {
        let before = accrual_ordering_snapshot(&env);
        let result = execute_cpi_close_kind(&mut env, kind, Q);
        unsafe_action_rejected = matches!(&result, Err(error) if is_engine_stale_error(error));
        if !unsafe_action_rejected {
            return Err(format!(
                "{kind:?} multi-segment close returned unexpected result: {result:?}"
            ));
        }
        rejected_exact_rollback = accrual_ordering_snapshot(&env) == before;
        settle_zero_move_actors(&mut env, settlement_slot, &[0, 1])?;
        execute_cpi_close_kind(&mut env, kind, Q)?;
        retry_landed = true;
    } else {
        execute_cpi_close_kind(&mut env, kind, Q)?;
        if action_before_settlement {
            settle_zero_move_actors(&mut env, settlement_slot, &[0, 1])?;
        }
    }
    let (paid, received) = funding_totals(&env, 0, 1);
    for actor in [0, 1] {
        let pnl = env.primary_portfolio(actor).pnl.get();
        if pnl > 0 {
            env.convert_released_pnl(actor, pnl as u128)
                .map_err(|error| format!("convert trade actor {actor} PnL: {error}"))?;
        }
        let capital = env.primary_portfolio(actor).capital.get();
        env.withdraw_primary(actor, capital)
            .map_err(|error| format!("withdraw trade actor {actor}: {error}"))?;
    }
    if env.token_supply_observed() != supply_before {
        return Err("trade accrual-order world changed SPL supply".into());
    }
    Ok(AccrualOrderingWorld {
        attacker_claim: u128::from(env.token_amount(env.actors[0].destination_token)),
        victim_claim: u128::from(env.token_amount(env.actors[1].destination_token)),
        paid,
        received,
        unsafe_action_rejected,
        rejected_exact_rollback,
        retry_landed,
    })
}

fn run_rebalance_accrual_ordering_world(
    seed: [u8; 32],
    action_before_settlement: bool,
    settlement_slot: u64,
    recover_rejected_action: bool,
) -> Result<AccrualOrderingWorld, String> {
    const PRICE: u64 = 2;
    const Q: i128 = 100 * POS_SCALE as i128;
    let mut env = zero_move_funding_discovery_world(seed)?;
    let supply_before = env.token_supply_observed();
    env.trade_no_cpi(0, 1, 0, -Q, PRICE, 0)
        .map_err(|error| format!("open rebalance accrual pair: {error}"))?;
    prime_zero_move_funding_discovery(&mut env, settlement_slot)?;
    if !action_before_settlement {
        settle_zero_move_actors(&mut env, settlement_slot, &[0, 1])?;
    }
    let mut unsafe_action_rejected = false;
    let mut rejected_exact_rollback = false;
    let mut retry_landed = false;
    if action_before_settlement && recover_rejected_action {
        let before = accrual_ordering_snapshot(&env);
        let result = env.rebalance_reduce(0, 0, Q.unsigned_abs());
        unsafe_action_rejected = matches!(&result, Err(error) if is_engine_stale_error(error));
        if !unsafe_action_rejected {
            return Err(format!(
                "multi-segment unilateral reduction returned unexpected result: {result:?}"
            ));
        }
        rejected_exact_rollback = accrual_ordering_snapshot(&env) == before;
        settle_zero_move_actors(&mut env, settlement_slot, &[0, 1])?;
        env.rebalance_reduce(0, 0, Q.unsigned_abs())
            .map_err(|error| format!("retry unilateral accrual-boundary reduction: {error}"))?;
        retry_landed = true;
    } else {
        env.rebalance_reduce(0, 0, Q.unsigned_abs())
            .map_err(|error| format!("unilateral accrual-boundary reduction: {error}"))?;
        settle_zero_move_actors(&mut env, settlement_slot, &[1])?;
    }
    let (paid, received) = funding_totals(&env, 0, 1);
    let victim_claim = portfolio_claim(&env, 1)?;
    let attacker_capital = env.primary_portfolio(0).capital.get();
    env.withdraw_primary(0, attacker_capital)
        .map_err(|error| format!("withdraw rebalance actor: {error}"))?;
    if env.token_supply_observed() != supply_before {
        return Err("rebalance accrual-order world changed SPL supply".into());
    }
    Ok(AccrualOrderingWorld {
        attacker_claim: u128::from(env.token_amount(env.actors[0].destination_token)),
        victim_claim,
        paid,
        received,
        unsafe_action_rejected,
        rejected_exact_rollback,
        retry_landed,
    })
}

fn run_forfeit_accrual_ordering_world(
    seed: [u8; 32],
    action_before_settlement: bool,
    settlement_slot: u64,
    recover_rejected_action: bool,
) -> Result<AccrualOrderingWorld, String> {
    const PRICE: u64 = 2;
    const Q_ATTACKER: i128 = 5 * POS_SCALE as i128;
    const Q_WHALE: i128 = 95 * POS_SCALE as i128;
    let mut env = zero_move_funding_discovery_world(seed)?;
    let supply_before = env.token_supply_observed();
    env.trade_no_cpi(1, 2, 0, -Q_WHALE, PRICE, 0)
        .map_err(|error| format!("open forfeit whale pair: {error}"))?;
    env.trade_no_cpi(0, 3, 0, -Q_ATTACKER, PRICE, 0)
        .map_err(|error| format!("open forfeit accrual pair: {error}"))?;
    prime_zero_move_funding_discovery(&mut env, settlement_slot)?;
    if !action_before_settlement {
        settle_zero_move_actors(&mut env, settlement_slot, &[0, 1, 2, 3])?;
    }
    let mut unsafe_action_rejected = false;
    let mut rejected_exact_rollback = false;
    let mut retry_landed = false;
    if action_before_settlement && recover_rejected_action {
        let before = accrual_ordering_snapshot(&env);
        let result = env.rebalance_reduce(2, 0, Q_WHALE.unsigned_abs());
        unsafe_action_rejected = matches!(&result, Err(error) if is_engine_stale_error(error));
        if !unsafe_action_rejected {
            return Err(format!(
                "multi-segment recovery transition returned unexpected result: {result:?}"
            ));
        }
        rejected_exact_rollback = accrual_ordering_snapshot(&env) == before;
        settle_zero_move_actors(&mut env, settlement_slot, &[0, 1, 2, 3])?;
        env.rebalance_reduce(2, 0, Q_WHALE.unsigned_abs())
            .map_err(|error| format!("retry recovery side transition: {error}"))?;
        retry_landed = true;
    } else {
        env.rebalance_reduce(2, 0, Q_WHALE.unsigned_abs())
            .map_err(|error| format!("enter recovery side mode: {error}"))?;
    }
    if env.primary_market_state().1.assets[0].mode_short != SideModeV16::DrainOnly {
        return Err("public reduction did not enter short-side DrainOnly".into());
    }
    env.forfeit_recovery_leg(0, 0, u128::from(u64::MAX))
        .map_err(|error| format!("forfeit accrual-boundary recovery leg: {error}"))?;
    settle_zero_move_actors(&mut env, settlement_slot, &[3])?;
    let (paid, received) = funding_totals(&env, 0, 3);
    let victim_claim = portfolio_claim(&env, 3)?;
    let attacker_capital = env.primary_portfolio(0).capital.get();
    env.withdraw_primary(0, attacker_capital)
        .map_err(|error| format!("withdraw forfeit actor: {error}"))?;
    if env.token_supply_observed() != supply_before {
        return Err("forfeit accrual-order world changed SPL supply".into());
    }
    Ok(AccrualOrderingWorld {
        attacker_claim: u128::from(env.token_amount(env.actors[0].destination_token)),
        victim_claim,
        paid,
        received,
        unsafe_action_rejected,
        rejected_exact_rollback,
        retry_landed,
    })
}

fn discover_one_accrual_ordering_violation(
    mut seed: [u8; 32],
    kind: AccrualOrderingKind,
    settlement_slot: u64,
    recover_rejected_action: bool,
) -> Result<AccrualOrderingDiscovery, String> {
    seed[0] ^= 0x9d;
    seed[1] ^= kind.discriminator();
    let run = |action_before_settlement| match kind {
        AccrualOrderingKind::CpiTradeClose | AccrualOrderingKind::BatchCpiTradeClose => {
            run_trade_accrual_ordering_world(
                seed,
                kind,
                action_before_settlement,
                settlement_slot,
                recover_rejected_action,
            )
        }
        AccrualOrderingKind::RebalanceReduce => run_rebalance_accrual_ordering_world(
            seed,
            action_before_settlement,
            settlement_slot,
            recover_rejected_action,
        ),
        AccrualOrderingKind::RecoveryForfeit => run_forfeit_accrual_ordering_world(
            seed,
            action_before_settlement,
            settlement_slot,
            recover_rejected_action,
        ),
    };
    let control = run(false)?;
    let reordered = run(true)?;
    let victim_claim_loss = control
        .victim_claim
        .checked_sub(reordered.victim_claim)
        .ok_or_else(|| format!("{kind:?} reordered path increased victim claim"))?;
    let attacker_claim_gain = reordered
        .attacker_claim
        .checked_sub(control.attacker_claim)
        .ok_or_else(|| format!("{kind:?} reordered path decreased attacker claim"))?;
    if control
        .attacker_claim
        .checked_add(control.victim_claim)
        .ok_or_else(|| "control claim total overflow".to_string())?
        != reordered
            .attacker_claim
            .checked_add(reordered.victim_claim)
            .ok_or_else(|| "reordered claim total overflow".to_string())?
    {
        return Err(format!("{kind:?} paired worlds did not conserve claims"));
    }
    Ok(AccrualOrderingDiscovery {
        kind,
        unsafe_action_rejected: reordered.unsafe_action_rejected,
        rejected_exact_rollback: reordered.rejected_exact_rollback,
        retry_landed: reordered.retry_landed,
        control_paid: control.paid,
        control_received: control.received,
        reordered_paid: reordered.paid,
        reordered_received: reordered.received,
        victim_claim_loss,
        attacker_claim_gain,
    })
}

pub fn discover_accrual_ordering_violations(
    seed: [u8; 32],
) -> Result<Vec<AccrualOrderingDiscovery>, String> {
    AccrualOrderingKind::ALL
        .into_iter()
        .map(|kind| discover_one_accrual_ordering_violation(seed, kind, 3, false))
        .collect()
}

pub fn discover_multi_segment_accrual_ordering_violations(
    seed: [u8; 32],
) -> Result<Vec<AccrualOrderingDiscovery>, String> {
    AccrualOrderingKind::ALL
        .into_iter()
        .map(|kind| discover_one_accrual_ordering_violation(seed, kind, 5, true))
        .collect()
}

fn drain_resolved_discovery_actor(env: &mut V16Svm, actor: usize) -> Result<u128, String> {
    let destination = env.actors[actor].destination_token;
    let payout_before = env.token_amount(destination);
    for _ in 0..512 {
        let market_before = env.market_data(false);
        let portfolio_before = env.primary_portfolio_data(actor);
        let destination_before = env.token_amount(destination);
        let _ = env.close_resolved_primary(actor);
        let _ = env.claim_resolved_payout_topup_primary(actor);
        if env.market_data(false) == market_before
            && env.primary_portfolio_data(actor) == portfolio_before
            && env.token_amount(destination) == destination_before
        {
            return env
                .token_amount(destination)
                .checked_sub(payout_before)
                .map(u128::from)
                .ok_or_else(|| format!("resolved actor {actor} payout decreased"));
        }
    }
    Err(format!(
        "resolved actor {actor} did not reach a fixed point in 512 calls"
    ))
}

#[derive(Clone, Copy, Debug)]
struct TerminalCommitWorld {
    effective_mark: u64,
    unsafe_resolve_rejected: bool,
    rejected_exact_rollback: bool,
    catchup_steps: u16,
    max_catchup_cu: u64,
    long_payout: u64,
    short_payout: u64,
}

fn run_terminal_commit_world(
    seed: [u8; 32],
    commit_before_resolve: bool,
) -> Result<TerminalCommitWorld, String> {
    const PRICE: u64 = 1_000_000;
    const MARK: u64 = 1_010_000;
    const DEPOSIT: u128 = 2_000_000_000;
    const SIZE_Q: i128 = 1_000 * POS_SCALE as i128;
    const PUSH_SLOT: u64 = 2;
    const RESOLVE_SLOT: u64 = 5;
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            h_max: 20,
            max_trading_fee_bps: 100,
            max_price_move_bps_per_slot: 100,
            max_accrual_dt_slots: 20,
            min_funding_lifetime_slots: 20,
            actor_deposits: [DEPOSIT, DEPOSIT, USER_DEPOSIT, USER_DEPOSIT, USER_DEPOSIT],
            actor_token_balances: [
                2_500_000_000,
                2_500_000_000,
                200_000_000,
                200_000_000,
                2_500_000_000,
            ],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.configure_permissionless_resolve(3, 1)
        .map_err(|error| format!("configure terminal resolve: {error}"))?;
    env.trade_no_cpi(0, 1, 0, SIZE_Q, PRICE, 0)
        .map_err(|error| format!("open terminal ordering pair: {error}"))?;
    env.warp_to_slot(PUSH_SLOT);
    env.push_auth_mark(0, PUSH_SLOT, MARK)
        .map_err(|error| format!("publish pending terminal mark: {error}"))?;
    let (_, pending) = env.primary_market_state();
    if env.primary_profile(0).mark_ewma_e6 != MARK || pending.assets[0].effective_price != PRICE {
        return Err("terminal ordering fixture did not retain a pending mark".into());
    }
    if commit_before_resolve {
        env.crank(
            0,
            PUSH_SLOT,
            vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: 0,
            }],
        )
        .map_err(|error| format!("commit pending terminal mark: {error}"))?;
    }
    let mut unsafe_resolve_rejected = false;
    let mut rejected_exact_rollback = false;
    let mut catchup_steps = 0u16;
    let mut max_catchup_cu = 0u64;
    let mut resolved = false;
    if !commit_before_resolve {
        let before_rejection = fingerprint(&env);
        let result = env.resolve_stale_permissionless(RESOLVE_SLOT);
        unsafe_resolve_rejected = matches!(
            &result,
            Err(error)
                if error.contains("Custom(19)") || error.contains("custom program error: 0x13")
        );
        if !unsafe_resolve_rejected {
            return Err(format!(
                "unsafe permissionless terminal resolve returned unexpected result: {result:?}"
            ));
        }
        rejected_exact_rollback = fingerprint(&env) == before_rejection;
        for _ in 0..16 {
            let catchup = env
                .crank(
                    0,
                    RESOLVE_SLOT,
                    vec![CrankObservationHint {
                        asset_index: 0,
                        oracle_accounts: 0,
                    }],
                )
                .map_err(|error| format!("permissionless terminal catch-up: {error}"))?;
            catchup_steps = catchup_steps
                .checked_add(1)
                .ok_or_else(|| "terminal catch-up step overflow".to_string())?;
            max_catchup_cu = max_catchup_cu.max(catchup.compute_units);
            let before_retry = fingerprint(&env);
            match env.resolve_stale_permissionless(RESOLVE_SLOT) {
                Ok(_) => {
                    resolved = true;
                    break;
                }
                Err(error)
                    if error.contains("Custom(19)")
                        || error.contains("custom program error: 0x13") =>
                {
                    rejected_exact_rollback &= fingerprint(&env) == before_retry;
                }
                Err(error) => {
                    return Err(format!(
                        "permissionless terminal resolve retry returned unexpected error: {error}"
                    ));
                }
            }
        }
        if !resolved {
            return Err("terminal world did not resolve within 16 bounded catch-up calls".into());
        }
    } else {
        env.resolve_stale_permissionless(RESOLVE_SLOT)
            .map_err(|error| format!("committed terminal resolve: {error}"))?;
        resolved = true;
    }
    debug_assert!(resolved);
    let (_, resolved) = env.primary_market_state();
    if resolved.mode != MarketModeV16::Resolved {
        return Err("permissionless resolver did not terminalize market".into());
    }
    let effective_mark = resolved.assets[0].effective_price;
    env.warp_to_slot(RESOLVE_SLOT + 1);
    let short_payout = drain_resolved_discovery_actor(&mut env, 1)?;
    let long_payout = drain_resolved_discovery_actor(&mut env, 0)?;
    if env.token_supply_observed() != supply_before {
        return Err("terminal ordering world changed SPL supply".into());
    }
    Ok(TerminalCommitWorld {
        effective_mark,
        unsafe_resolve_rejected,
        rejected_exact_rollback,
        catchup_steps,
        max_catchup_cu,
        long_payout: u64::try_from(long_payout)
            .map_err(|_| "long terminal payout exceeds SPL range")?,
        short_payout: u64::try_from(short_payout)
            .map_err(|_| "short terminal payout exceeds SPL range")?,
    })
}

pub fn discover_terminal_commit_ordering(
    mut seed: [u8; 32],
) -> Result<TerminalCommitOrderingDiscovery, String> {
    seed[0] ^= 0xa5;
    let committed = run_terminal_commit_world(seed, true)?;
    let reordered = run_terminal_commit_world(seed, false)?;
    let victim_payout_loss = committed
        .long_payout
        .checked_sub(reordered.long_payout)
        .ok_or_else(|| "resolve-first ordering increased victim payout".to_string())?;
    let counterparty_payout_gain = reordered
        .short_payout
        .checked_sub(committed.short_payout)
        .ok_or_else(|| "resolve-first ordering decreased counterparty payout".to_string())?;
    let committed_total_payout = u128::from(committed.long_payout)
        .checked_add(u128::from(committed.short_payout))
        .ok_or_else(|| "committed terminal payout overflow".to_string())?;
    let reordered_total_payout = u128::from(reordered.long_payout)
        .checked_add(u128::from(reordered.short_payout))
        .ok_or_else(|| "reordered terminal payout overflow".to_string())?;
    Ok(TerminalCommitOrderingDiscovery {
        committed_mark: committed.effective_mark,
        reordered_mark: reordered.effective_mark,
        unsafe_resolve_rejected: reordered.unsafe_resolve_rejected,
        rejected_exact_rollback: reordered.rejected_exact_rollback,
        catchup_steps: reordered.catchup_steps,
        max_catchup_cu: reordered.max_catchup_cu,
        victim_payout_loss,
        counterparty_payout_gain,
        committed_total_payout,
        reordered_total_payout,
    })
}

#[derive(Clone, Copy, Debug)]
struct PendingZeroMoveTerminalWorld {
    f_long_num: i128,
    f_short_num: i128,
    unsafe_resolve_rejected: bool,
    rejected_exact_rollback: bool,
    catchup_steps: u16,
    max_catchup_cu: u64,
    payer_payout: u128,
    receiver_payout: u128,
}

fn run_pending_zero_move_terminal_world(
    seed: [u8; 32],
    commit_before_maturity: bool,
) -> Result<PendingZeroMoveTerminalWorld, String> {
    const PRICE: u64 = 2;
    const MARK: u64 = 1;
    const DEPOSIT: u128 = 1_000_000;
    const SIZE_Q: i128 = 100 * POS_SCALE as i128;
    const PUSH_SLOT: u64 = 2;
    const RESOLVE_SLOT: u64 = 5;
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            max_price_move_bps_per_slot: 24,
            max_accrual_dt_slots: 20,
            max_abs_funding_e9_per_slot: 1_000,
            min_funding_lifetime_slots: 20,
            actor_deposits: [DEPOSIT, DEPOSIT, USER_DEPOSIT, USER_DEPOSIT, USER_DEPOSIT],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.configure_permissionless_resolve(3, 1)
        .map_err(|error| format!("configure zero-move terminal resolve: {error}"))?;
    env.trade_no_cpi(0, 1, 0, -SIZE_Q, PRICE, 0)
        .map_err(|error| format!("open zero-move terminal pair: {error}"))?;
    env.warp_to_slot(PUSH_SLOT);
    env.push_auth_mark(0, PUSH_SLOT, MARK)
        .map_err(|error| format!("publish zero-move pending mark: {error}"))?;
    let oracle_accounts = env.primary_profile(0).oracle_leg_count;
    let observations = || {
        vec![CrankObservationHint {
            asset_index: 0,
            oracle_accounts,
        }]
    };
    let profile = env.primary_profile(0);
    let (_, pending_group) = env.primary_market_state();
    if profile.funding_mark_pending_e6 != MARK
        || profile.funding_mark_pending_slot != PUSH_SLOT
        || pending_group.assets[0].effective_price != PRICE
    {
        return Err(format!(
            "zero-move terminal fixture did not retain the publication boundary: profile={profile:?}, engine_price={}",
            pending_group.assets[0].effective_price
        ));
    }
    if commit_before_maturity {
        env.crank(0, PUSH_SLOT, observations())
            .map_err(|error| format!("commit zero-move publication boundary: {error}"))?;
    }

    env.warp_to_slot(RESOLVE_SLOT);
    let mut unsafe_resolve_rejected = false;
    let mut rejected_exact_rollback = false;
    if !commit_before_maturity {
        let before = fingerprint(&env);
        let result = env.resolve_stale_permissionless(RESOLVE_SLOT);
        unsafe_resolve_rejected = matches!(&result, Err(error) if is_engine_stale_error(error));
        if !unsafe_resolve_rejected {
            return Err(format!(
                "zero-move unsafe terminal resolve returned unexpected result: {result:?}"
            ));
        }
        rejected_exact_rollback = fingerprint(&env) == before;
    }

    let mut catchup_steps = 0u16;
    let mut max_catchup_cu = 0u64;
    let mut resolved = false;
    for _ in 0..16 {
        let catchup = env
            .crank(0, RESOLVE_SLOT, observations())
            .map_err(|error| format!("zero-move terminal catch-up crank: {error}"))?;
        catchup_steps = catchup_steps
            .checked_add(1)
            .ok_or_else(|| "zero-move terminal catch-up count overflow".to_string())?;
        max_catchup_cu = max_catchup_cu.max(catchup.compute_units);
        if commit_before_maturity && env.primary_market_state().1.assets[0].slot_last < RESOLVE_SLOT
        {
            continue;
        }
        let before = fingerprint(&env);
        match env.resolve_stale_permissionless(RESOLVE_SLOT) {
            Ok(_) => {
                resolved = true;
                break;
            }
            Err(error) if is_engine_stale_error(&error) => {
                rejected_exact_rollback &= fingerprint(&env) == before;
            }
            Err(error) => {
                return Err(format!(
                    "zero-move terminal resolve retry returned unexpected error: {error}"
                ));
            }
        }
    }
    if !resolved {
        return Err("zero-move terminal world did not resolve in 16 bounded cranks".into());
    }
    let (_, resolved_group) = env.primary_market_state();
    let f_long_num = resolved_group.assets[0].f_long_num;
    let f_short_num = resolved_group.assets[0].f_short_num;
    if resolved_group.assets[0].effective_price != PRICE {
        return Err("zero-move terminal fixture unexpectedly moved effective price".into());
    }
    env.warp_to_slot(RESOLVE_SLOT + 1);
    let receiver_payout = drain_resolved_discovery_actor(&mut env, 1)?;
    let payer_payout = drain_resolved_discovery_actor(&mut env, 0)?;
    if env.token_supply_observed() != supply_before {
        return Err("zero-move terminal world changed SPL supply".into());
    }
    Ok(PendingZeroMoveTerminalWorld {
        f_long_num,
        f_short_num,
        unsafe_resolve_rejected,
        rejected_exact_rollback,
        catchup_steps,
        max_catchup_cu,
        payer_payout,
        receiver_payout,
    })
}

pub fn discover_pending_zero_move_terminal_ordering(
    mut seed: [u8; 32],
) -> Result<PendingZeroMoveTerminalDiscovery, String> {
    seed[0] ^= 0x39;
    seed[1] ^= 0xa5;
    let control = run_pending_zero_move_terminal_world(seed, true)?;
    let reordered = run_pending_zero_move_terminal_world(seed, false)?;
    let victim_payout_loss = control
        .receiver_payout
        .checked_sub(reordered.receiver_payout)
        .ok_or_else(|| "zero-move reordered path increased receiver payout".to_string())?;
    let attacker_payout_gain = reordered
        .payer_payout
        .checked_sub(control.payer_payout)
        .ok_or_else(|| "zero-move reordered path decreased payer payout".to_string())?;
    let control_total_payout = control
        .payer_payout
        .checked_add(control.receiver_payout)
        .ok_or_else(|| "zero-move control payout overflow".to_string())?;
    let reordered_total_payout = reordered
        .payer_payout
        .checked_add(reordered.receiver_payout)
        .ok_or_else(|| "zero-move reordered payout overflow".to_string())?;
    Ok(PendingZeroMoveTerminalDiscovery {
        control_f_long_num: control.f_long_num,
        control_f_short_num: control.f_short_num,
        reordered_f_long_num: reordered.f_long_num,
        reordered_f_short_num: reordered.f_short_num,
        unsafe_resolve_rejected: reordered.unsafe_resolve_rejected,
        rejected_exact_rollback: reordered.rejected_exact_rollback,
        catchup_steps: reordered.catchup_steps,
        max_catchup_cu: reordered.max_catchup_cu,
        victim_payout_loss,
        attacker_payout_gain,
        control_total_payout,
        reordered_total_payout,
    })
}

#[derive(Clone, Copy, Debug)]
struct ShutdownCommitWorld {
    f_long_num: i128,
    f_short_num: i128,
    long_payout: u128,
    short_payout: u128,
}

fn run_shutdown_commit_world(
    mut seed: [u8; 32],
    commit_before_shutdown: bool,
) -> Result<ShutdownCommitWorld, String> {
    const LONG: usize = 0;
    const SHORT: usize = 1;
    const ORACLE: usize = 2;
    const PRICE: u64 = 100;
    const MARK: u64 = 99;
    const SIZE_Q: i128 = 100_000 * POS_SCALE as i128;
    const DEPOSIT: u128 = 100_000_000;
    const PRIME_SLOT: u64 = 2;
    const SHUTDOWN_SLOT: u64 = 3;
    const FORCE_CLOSE_SLOT: u64 = 4;
    seed[0] ^= 0x54;
    seed[1] ^= u8::from(commit_before_shutdown);
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            max_price_move_bps_per_slot: 1,
            max_accrual_dt_slots: 1,
            max_abs_funding_e9_per_slot: 10_000,
            min_funding_lifetime_slots: 1,
            actor_deposits: [DEPOSIT, DEPOSIT, 1, 1, 1],
            actor_token_balances: [DEPOSIT as u64, DEPOSIT as u64, 2, 2, 2],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.configure_permissionless_resolve(1_000, 1)
        .map_err(|error| format!("configure shutdown recovery: {error}"))?;
    env.update_asset_authority_from_admin(0, percolator_prog::processor::ASSET_AUTH_ORACLE, ORACLE)
        .map_err(|error| format!("separate shutdown oracle: {error}"))?;
    env.trade_no_cpi(LONG, SHORT, 0, SIZE_Q, PRICE, 0)
        .map_err(|error| format!("open shutdown funding pair: {error}"))?;

    env.warp_to_slot(PRIME_SLOT);
    env.push_auth_mark_for_actor(ORACLE, 0, PRIME_SLOT, MARK)
        .map_err(|error| format!("publish committed shutdown mark: {error}"))?;
    env.crank(
        LONG,
        PRIME_SLOT,
        vec![CrankObservationHint {
            asset_index: 0,
            oracle_accounts: 0,
        }],
    )
    .map_err(|error| format!("prime shutdown funding checkpoint: {error}"))?;
    let (_, primed) = env.primary_market_state();
    if primed.assets[0].effective_price != PRICE
        || primed.assets[0].f_long_num != 0
        || primed.assets[0].f_short_num != 0
    {
        return Err("shutdown fixture did not isolate a zero-price-move funding segment".into());
    }

    env.warp_to_slot(SHUTDOWN_SLOT);
    if commit_before_shutdown {
        env.crank(
            LONG,
            SHUTDOWN_SLOT,
            vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: 0,
            }],
        )
        .map_err(|error| format!("commit funding before shutdown: {error}"))?;
    }
    env.shutdown_asset(0, SHUTDOWN_SLOT)
        .map_err(|error| format!("shutdown exposed asset: {error}"))?;
    let (_, shutdown) = env.primary_market_state();
    if shutdown.assets[0].effective_price != PRICE {
        return Err("shutdown funding fixture unexpectedly moved effective price".into());
    }
    let f_long_num = shutdown.assets[0].f_long_num;
    let f_short_num = shutdown.assets[0].f_short_num;

    env.warp_to_slot(FORCE_CLOSE_SLOT);
    env.force_close_abandoned_asset(ORACLE, LONG, SHORT, 0, FORCE_CLOSE_SLOT, SIZE_Q as u128)
        .map_err(|error| format!("force close shutdown pair: {error}"))?;
    env.resolve_market()
        .map_err(|error| format!("resolve shutdown fixture: {error}"))?;
    env.warp_to_slot(FORCE_CLOSE_SLOT + 1);
    let long_payout = drain_resolved_discovery_actor(&mut env, LONG)?;
    let short_payout = drain_resolved_discovery_actor(&mut env, SHORT)?;
    if env.token_supply_observed() != supply_before {
        return Err("shutdown commit world changed SPL supply".into());
    }
    Ok(ShutdownCommitWorld {
        f_long_num,
        f_short_num,
        long_payout,
        short_payout,
    })
}

pub fn discover_shutdown_commit_ordering(
    seed: [u8; 32],
) -> Result<ShutdownCommitOrderingDiscovery, String> {
    let control = run_shutdown_commit_world(seed, true)?;
    let shutdown_first = run_shutdown_commit_world(seed, false)?;
    let victim_payout_loss = control
        .long_payout
        .checked_sub(shutdown_first.long_payout)
        .ok_or_else(|| "shutdown-first ordering increased long payout".to_string())?;
    let counterparty_payout_gain = shutdown_first
        .short_payout
        .checked_sub(control.short_payout)
        .ok_or_else(|| "shutdown-first ordering decreased short payout".to_string())?;
    if control.long_payout + control.short_payout
        != shutdown_first.long_payout + shutdown_first.short_payout
    {
        return Err("shutdown order worlds did not conserve terminal payouts".into());
    }
    Ok(ShutdownCommitOrderingDiscovery {
        control_f_long_num: control.f_long_num,
        control_f_short_num: control.f_short_num,
        shutdown_f_long_num: shutdown_first.f_long_num,
        shutdown_f_short_num: shutdown_first.f_short_num,
        victim_payout_loss,
        counterparty_payout_gain,
    })
}

pub fn discover_shutdown_catchup_liveness(
    mut seed: [u8; 32],
) -> Result<ShutdownCatchupDiscovery, String> {
    const LONG: usize = 0;
    const SHORT: usize = 1;
    const ORACLE: usize = 2;
    const PRICE: u64 = 2;
    const MARK: u64 = 1;
    const SIZE_Q: i128 = 100 * POS_SCALE as i128;
    const DEPOSIT: u128 = 1_000_000;
    const PUBLISH_SLOT: u64 = 2;
    const SHUTDOWN_SLOT: u64 = 5;
    seed[0] ^= 0x54;
    seed[1] ^= 0x39;
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            max_price_move_bps_per_slot: 24,
            max_accrual_dt_slots: 1,
            max_abs_funding_e9_per_slot: 1_000,
            min_funding_lifetime_slots: 1,
            actor_deposits: [DEPOSIT, DEPOSIT, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.configure_permissionless_resolve(1_000, 1)
        .map_err(|error| format!("configure shutdown catch-up policy: {error}"))?;
    env.update_asset_authority_from_admin(0, percolator_prog::processor::ASSET_AUTH_ORACLE, ORACLE)
        .map_err(|error| format!("separate shutdown catch-up oracle: {error}"))?;
    env.trade_no_cpi(LONG, SHORT, 0, SIZE_Q, PRICE, 0)
        .map_err(|error| format!("open shutdown catch-up pair: {error}"))?;
    env.warp_to_slot(PUBLISH_SLOT);
    env.push_auth_mark_for_actor(ORACLE, 0, PUBLISH_SLOT, MARK)
        .map_err(|error| format!("publish shutdown catch-up mark: {error}"))?;
    env.warp_to_slot(SHUTDOWN_SLOT);

    let before_rejection = fingerprint(&env);
    let initial = env.shutdown_asset(0, SHUTDOWN_SLOT);
    let initial_shutdown_rejected = matches!(&initial, Err(error) if is_engine_stale_error(error));
    if !initial_shutdown_rejected {
        return Err(format!(
            "shutdown with a pending checkpoint returned unexpected result: {initial:?}"
        ));
    }
    let rejected_exact_rollback = fingerprint(&env) == before_rejection;

    let oracle_accounts = env.primary_profile(0).oracle_leg_count;
    let mut catchup_steps = 0u16;
    let mut max_catchup_cu = 0u64;
    for _ in 0..16 {
        if env.primary_market_state().1.assets[0].slot_last >= SHUTDOWN_SLOT
            && env.primary_profile(0).funding_mark_pending_e6 == 0
        {
            break;
        }
        let catchup = env
            .crank(
                LONG,
                SHUTDOWN_SLOT,
                vec![CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts,
                }],
            )
            .map_err(|error| format!("shutdown permissionless catch-up crank: {error}"))?;
        catchup_steps = catchup_steps
            .checked_add(1)
            .ok_or_else(|| "shutdown catch-up count overflow".to_string())?;
        max_catchup_cu = max_catchup_cu.max(catchup.compute_units);
    }
    if env.primary_market_state().1.assets[0].slot_last < SHUTDOWN_SLOT
        || env.primary_profile(0).funding_mark_pending_e6 != 0
    {
        return Err("shutdown checkpoint did not catch up in 16 public cranks".into());
    }
    let retry = env
        .shutdown_asset(0, SHUTDOWN_SLOT)
        .map_err(|error| format!("shutdown retry after public catch-up: {error}"))?;
    let (_, shutdown) = env.primary_market_state();
    let retry_landed = shutdown.assets[0].lifecycle == percolator::AssetLifecycleV16::Recovery;
    let f_long_num = shutdown.assets[0].f_long_num;
    let f_short_num = shutdown.assets[0].f_short_num;
    if !retry_landed {
        return Err("shutdown retry did not enter asset recovery".into());
    }
    if retry.compute_units >= 1_400_000 {
        return Err("shutdown retry exceeded the transaction CU ceiling".into());
    }

    env.warp_to_slot(SHUTDOWN_SLOT + 1);
    env.force_close_abandoned_asset(ORACLE, LONG, SHORT, 0, SHUTDOWN_SLOT + 1, SIZE_Q as u128)
        .map_err(|error| format!("force close shutdown catch-up pair: {error}"))?;
    env.resolve_market()
        .map_err(|error| format!("resolve shutdown catch-up market: {error}"))?;
    env.warp_to_slot(SHUTDOWN_SLOT + 2);
    let long_payout = drain_resolved_discovery_actor(&mut env, LONG)?;
    let short_payout = drain_resolved_discovery_actor(&mut env, SHORT)?;
    let users_terminal = discovery_portfolio_is_terminal(&env.primary_portfolio(LONG))?
        && discovery_portfolio_is_terminal(&env.primary_portfolio(SHORT))?;
    let total_payout = long_payout
        .checked_add(short_payout)
        .ok_or_else(|| "shutdown catch-up payout overflow".to_string())?;
    Ok(ShutdownCatchupDiscovery {
        initial_shutdown_rejected,
        rejected_exact_rollback,
        catchup_steps,
        max_catchup_cu,
        retry_landed,
        f_long_num,
        f_short_num,
        users_terminal,
        total_payout,
        token_supply_conserved: env.token_supply_observed() == supply_before,
    })
}

fn execute_reported_price_route(
    env: &mut V16Svm,
    route: ProspectiveAccrualRoute,
    taker: usize,
    maker: usize,
    size_q: i128,
    price: u64,
) -> Result<(), String> {
    let market_id = env.primary_market_state().1.assets[0].market_id;
    match route {
        ProspectiveAccrualRoute::NoCpi => env
            .trade_no_cpi(taker, maker, 0, size_q, price, 0)
            .map(|_| ())
            .map_err(|error| format!("reported-price trade: {error}")),
        ProspectiveAccrualRoute::Cpi => env
            .trade_cpi(taker, maker, 0, size_q, 0, 0)
            .map(|_| ())
            .map_err(|error| format!("CPI reported-price trade: {error}")),
        ProspectiveAccrualRoute::BatchNoCpi => env
            .batch_trade_no_cpi(
                taker,
                maker,
                vec![BatchTradeLeg {
                    asset_index: 0,
                    market_id,
                    size_q,
                    exec_price: price,
                    fee_bps: 0,
                }],
            )
            .map(|_| ())
            .map_err(|error| format!("batch reported-price trade: {error}")),
        ProspectiveAccrualRoute::BatchCpi => env
            .batch_trade_cpi(
                taker,
                maker,
                vec![BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id,
                    size_q,
                    fee_bps: 0,
                    limit_price: 0,
                }],
            )
            .map(|_| ())
            .map_err(|error| format!("batch CPI reported-price trade: {error}")),
    }
}

fn execute_discovery_trade_route(
    env: &mut V16Svm,
    route: DiscoveryTradeRoute,
    taker: usize,
    maker: usize,
    asset_index: u16,
    size_q: i128,
    price: u64,
) -> Result<(), String> {
    let market_id = env.primary_market_state().1.assets[asset_index as usize].market_id;
    match route {
        DiscoveryTradeRoute::NoCpi => env
            .trade_no_cpi(taker, maker, asset_index, size_q, price, 0)
            .map(|_| ()),
        DiscoveryTradeRoute::BatchNoCpi => env
            .batch_trade_no_cpi(
                taker,
                maker,
                vec![BatchTradeLeg {
                    asset_index,
                    market_id,
                    size_q,
                    exec_price: price,
                    fee_bps: 0,
                }],
            )
            .map(|_| ()),
        DiscoveryTradeRoute::Cpi => env
            .trade_cpi(taker, maker, asset_index, size_q, 0, 0)
            .map(|_| ()),
        DiscoveryTradeRoute::BatchCpi => env
            .batch_trade_cpi(
                taker,
                maker,
                vec![BatchTradeCpiLeg {
                    asset_index,
                    market_id,
                    size_q,
                    fee_bps: 0,
                    limit_price: 0,
                }],
            )
            .map(|_| ()),
    }
}

fn build_retained_discovery_trade(
    env: &mut V16Svm,
    route: DiscoveryTradeRoute,
    taker: usize,
    maker: usize,
    asset_index: u16,
    size_q: i128,
    price: u64,
) -> Transaction {
    match route {
        DiscoveryTradeRoute::NoCpi => {
            env.build_retained_no_cpi_trade(taker, maker, asset_index, size_q, price)
        }
        DiscoveryTradeRoute::BatchNoCpi => {
            env.build_retained_batch_no_cpi_trade(taker, maker, asset_index, size_q, price)
        }
        DiscoveryTradeRoute::Cpi => {
            env.build_retained_cpi_trade(taker, maker, asset_index, size_q, 0)
        }
        DiscoveryTradeRoute::BatchCpi => {
            env.build_retained_batch_cpi_trade(taker, maker, asset_index, size_q, 0)
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ProspectiveAccrualWorld {
    coalition_payout: u128,
    victim_payout: u128,
    final_mark: u64,
    final_effective_price: u64,
    f_short_num: i128,
    total_payout: u128,
}

fn run_prospective_accrual_world(
    seed: [u8; 32],
    route: ProspectiveAccrualRoute,
    stamp_before_catchup: bool,
) -> Result<ProspectiveAccrualWorld, String> {
    const PRICE: u64 = 1_000_000;
    const DEPOSIT: u128 = 100_000_000;
    const MARK_HALFLIFE: u64 = 1_000_000;
    const PREP_SLOT: u64 = 1_001;
    const CATCHUP_SLOT: u64 = 1_102;
    const PUSH_TARGET: u64 = 556_555_556;
    const STAMP_EXEC_PRICE: u64 = 1_010_100;
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            max_price_move_bps_per_slot: 1,
            max_accrual_dt_slots: 1_000,
            max_abs_funding_e9_per_slot: 10_000,
            min_funding_lifetime_slots: 1_000,
            actor_deposits: [DEPOSIT, DEPOSIT, DEPOSIT, DEPOSIT, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    let configured_slot = env.current_slot();
    env.configure_ewma_mark(0, configured_slot, PRICE, MARK_HALFLIFE, 0)
        .map_err(|error| format!("configure prospective EWMA: {error}"))?;
    execute_reported_price_route(&mut env, route, 0, 1, POS_SCALE as i128, PRICE)?;
    execute_reported_price_route(&mut env, route, 2, 3, POS_SCALE as i128, PRICE)?;
    env.warp_to_slot(PREP_SLOT);
    for _ in 0..4 {
        env.crank(
            1,
            PREP_SLOT,
            vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: 0,
            }],
        )
        .map_err(|error| format!("prime prospective funding clock: {error}"))?;
        if env.primary_market_state().1.assets[0].slot_last == PREP_SLOT {
            break;
        }
    }
    if env.primary_market_state().1.assets[0].slot_last != PREP_SLOT {
        return Err("prospective funding clock did not reach prep slot".into());
    }
    env.push_ewma_mark(0, PREP_SLOT, PUSH_TARGET)
        .map_err(|error| format!("publish prospective funding premium: {error}"))?;
    let after_push = env.primary_profile(0);
    if after_push.mark_ewma_e6 != 1_500_000
        || after_push.funding_mark_e6 != after_push.mark_ewma_e6
        || after_push.funding_mark_pending_e6 != 0
        || after_push.funding_mark_pending_slot != 0
        || env.primary_market_state().1.assets[0].effective_price != PRICE
    {
        return Err(format!(
            "prospective premium setup drifted: mark={}, effective={}",
            after_push.mark_ewma_e6,
            env.primary_market_state().1.assets[0].effective_price
        ));
    }

    env.warp_to_slot(CATCHUP_SLOT);
    let stamp = |env: &mut V16Svm| {
        execute_reported_price_route(env, route, 2, 3, -(POS_SCALE as i128), STAMP_EXEC_PRICE)
    };
    let catchup = |env: &mut V16Svm| {
        env.crank(
            1,
            CATCHUP_SLOT,
            vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: 0,
            }],
        )
        .map(|_| ())
    };
    if stamp_before_catchup {
        stamp(&mut env).map_err(|error| format!("trade-first prospective stamp: {error}"))?;
        let staged = env.primary_profile(0);
        if staged.funding_mark_e6 != after_push.funding_mark_e6
            || staged.funding_mark_pending_e6 == 0
            || staged.funding_mark_pending_slot != CATCHUP_SLOT
        {
            return Err(format!(
                "trade-first prospective mark did not retain its first boundary: {staged:?}"
            ));
        }
        catchup(&mut env).map_err(|error| format!("trade-first prospective catch-up: {error}"))?;
    } else {
        catchup(&mut env).map_err(|error| format!("control prospective catch-up: {error}"))?;
        stamp(&mut env).map_err(|error| format!("control prospective stamp: {error}"))?;
    }
    let (profile_after, group_after) = env.primary_market_state();
    let checkpoint_after = env.primary_profile(0);
    if checkpoint_after.funding_mark_e6 != checkpoint_after.mark_ewma_e6
        || checkpoint_after.funding_mark_pending_e6 != 0
        || checkpoint_after.funding_mark_pending_slot != 0
    {
        return Err(format!(
            "prospective checkpoint did not commit after catch-up: {checkpoint_after:?}"
        ));
    }
    if group_after.assets[0].slot_last != CATCHUP_SLOT {
        return Err(format!(
            "prospective catch-up stopped at slot {}",
            group_after.assets[0].slot_last
        ));
    }
    env.resolve_market()
        .map_err(|error| format!("resolve prospective funding world: {error}"))?;
    let stamper_short_payout = drain_resolved_discovery_actor(&mut env, 3)?;
    let victim_payout = drain_resolved_discovery_actor(&mut env, 1)?;
    let attacker_payout = drain_resolved_discovery_actor(&mut env, 0)?;
    let stamper_long_payout = drain_resolved_discovery_actor(&mut env, 2)?;
    let coalition_payout = attacker_payout
        .checked_add(stamper_long_payout)
        .and_then(|value| value.checked_add(stamper_short_payout))
        .ok_or_else(|| "prospective coalition payout overflow".to_string())?;
    let total_payout = coalition_payout
        .checked_add(victim_payout)
        .ok_or_else(|| "prospective total payout overflow".to_string())?;
    if env.token_supply_observed() != supply_before {
        return Err("prospective funding world changed SPL supply".into());
    }
    Ok(ProspectiveAccrualWorld {
        coalition_payout,
        victim_payout,
        final_mark: profile_after.mark_ewma_e6,
        final_effective_price: group_after.assets[0].effective_price,
        f_short_num: group_after.assets[0].f_short_num,
        total_payout,
    })
}

fn discover_one_prospective_accrual_violation(
    mut seed: [u8; 32],
    route: ProspectiveAccrualRoute,
) -> Result<ProspectiveAccrualDiscovery, String> {
    seed[0] ^= 0xb6;
    seed[1] ^= route.discriminator();
    let control = run_prospective_accrual_world(seed, route, false)?;
    let reordered = run_prospective_accrual_world(seed, route, true)?;
    let fixed_execution_price = matches!(
        route,
        ProspectiveAccrualRoute::NoCpi | ProspectiveAccrualRoute::BatchNoCpi
    );
    if fixed_execution_price
        && (control.final_mark != reordered.final_mark
            || control.final_effective_price != reordered.final_effective_price)
    {
        return Err(format!(
            "{route:?} paired worlds changed final prices: control={control:?}, reordered={reordered:?}"
        ));
    }
    let victim_payout_loss = control
        .victim_payout
        .checked_sub(reordered.victim_payout)
        .ok_or_else(|| "trade-first ordering increased victim payout".to_string())?;
    let coalition_payout_gain = reordered
        .coalition_payout
        .checked_sub(control.coalition_payout)
        .ok_or_else(|| "trade-first ordering decreased coalition payout".to_string())?;
    Ok(ProspectiveAccrualDiscovery {
        route,
        control_f_short_num: control.f_short_num,
        reordered_f_short_num: reordered.f_short_num,
        victim_payout_loss,
        coalition_payout_gain,
        control_total_payout: control.total_payout,
        reordered_total_payout: reordered.total_payout,
        final_mark: reordered.final_mark,
        final_effective_price: reordered.final_effective_price,
    })
}

pub fn discover_prospective_accrual_violations(
    seed: [u8; 32],
) -> Result<Vec<ProspectiveAccrualDiscovery>, String> {
    ProspectiveAccrualRoute::ALL
        .into_iter()
        .map(|route| discover_one_prospective_accrual_violation(seed, route))
        .collect()
}

fn discover_one_pending_mark_admission(
    mut seed: [u8; 32],
    source: PendingMarkSource,
) -> Result<PendingMarkAdmissionDiscovery, String> {
    const OLD_MARK: u64 = 100;
    const AUTH_TARGET: u64 = 200;
    const EWMA_TARGET: u64 = 150;
    const ATTACK_SIZE_Q: i128 = 10_000 * POS_SCALE as i128;
    const EXISTING_SIZE_Q: i128 = POS_SCALE as i128;

    seed[0] ^= 0xc7;
    seed[1] ^= source.discriminator();
    let (mut env, attacker, victim, attack_size_q, published_target) =
        match source {
            PendingMarkSource::AuthenticatedPush | PendingMarkSource::EwmaPush => {
                let mut env = V16Svm::new(
                    seed,
                    MarketConfig {
                        initial_price: OLD_MARK,
                        max_price_move_bps_per_slot: 10_000,
                        max_accrual_dt_slots: 1,
                        actor_deposits: [1_000_100, 4_000_000, USER_DEPOSIT, USER_DEPOSIT, 1],
                        ..MarketConfig::default()
                    },
                );
                match source {
                    PendingMarkSource::AuthenticatedPush => env
                        .configure_auth_mark(false, 0, 0, OLD_MARK)
                        .map_err(|error| format!("configure authenticated mark: {error}"))?,
                    PendingMarkSource::EwmaPush => env
                        .configure_ewma_mark(0, 0, OLD_MARK, 1, 0)
                        .map_err(|error| format!("configure EWMA mark: {error}"))?,
                    PendingMarkSource::ReportedPriceTrade
                    | PendingMarkSource::ReportedPriceBatch => unreachable!(),
                };
                env.trade_cpi(0, 1, 0, EXISTING_SIZE_Q, 0, 0)
                    .map_err(|error| format!("open liveness-control position: {error}"))?;
                env.warp_to_slot(2);
                let published_target = match source {
                    PendingMarkSource::AuthenticatedPush => {
                        env.push_auth_mark(0, 2, AUTH_TARGET)
                            .map_err(|error| format!("publish authenticated mark: {error}"))?;
                        AUTH_TARGET
                    }
                    PendingMarkSource::EwmaPush => {
                        env.push_ewma_mark(0, 2, AUTH_TARGET)
                            .map_err(|error| format!("publish EWMA target: {error}"))?;
                        EWMA_TARGET
                    }
                    PendingMarkSource::ReportedPriceTrade
                    | PendingMarkSource::ReportedPriceBatch => unreachable!(),
                };
                let attack_size_q = ATTACK_SIZE_Q
                    .checked_add(EXISTING_SIZE_Q)
                    .ok_or_else(|| "pending-mark attack size overflow".to_string())?;
                (env, 0usize, 1usize, attack_size_q, published_target)
            }
            PendingMarkSource::ReportedPriceTrade | PendingMarkSource::ReportedPriceBatch => {
                let mut env = V16Svm::new(
                    seed,
                    MarketConfig {
                        initial_price: OLD_MARK,
                        max_price_move_bps_per_slot: 10_000,
                        max_accrual_dt_slots: 1,
                        actor_deposits: [1_000, 1_000, 1_000_000, 4_000_000, 1],
                        ..MarketConfig::default()
                    },
                );
                env.configure_ewma_mark(0, 0, OLD_MARK, 1, 0)
                    .map_err(|error| format!("configure trade-driven EWMA: {error}"))?;
                env.warp_to_slot(2);
                let route = match source {
                    PendingMarkSource::ReportedPriceTrade => ProspectiveAccrualRoute::NoCpi,
                    PendingMarkSource::ReportedPriceBatch => ProspectiveAccrualRoute::BatchNoCpi,
                    PendingMarkSource::AuthenticatedPush | PendingMarkSource::EwmaPush => {
                        unreachable!()
                    }
                };
                execute_reported_price_route(&mut env, route, 0, 1, EXISTING_SIZE_Q, AUTH_TARGET)?;
                execute_reported_price_route(&mut env, route, 0, 1, -EXISTING_SIZE_Q, OLD_MARK)?;
                (env, 2usize, 3usize, ATTACK_SIZE_Q, EWMA_TARGET)
            }
        };

    let supply_before = env.token_supply_observed();
    let (pending_profile, pending_group) = env.primary_market_state();
    let stale_engine_target = pending_group.assets[0].raw_oracle_target_price;
    if pending_profile.mark_ewma_e6 != published_target
        || pending_group.assets[0].effective_price != OLD_MARK
        || stale_engine_target != OLD_MARK
    {
        return Err(format!(
            "{source:?} did not create wrapper/engine mark lag: profile={}, raw={}, effective={}",
            pending_profile.mark_ewma_e6,
            stale_engine_target,
            pending_group.assets[0].effective_price
        ));
    }

    let victim_capital_before = env.primary_portfolio(victim).capital.get();
    env.trade_cpi(attacker, victim, 0, ATTACK_SIZE_Q, 0, 0)
        .map_err(|error| format!("{source:?} stale-price risk increase rejected: {error}"))?;
    env.warp_to_slot(3);
    for _ in 0..8 {
        for actor in [attacker, victim] {
            let _ = env.crank(
                actor,
                3,
                vec![CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                }],
            );
        }
        if env.primary_market_state().1.assets[0].effective_price == published_target {
            break;
        }
    }
    let committed_mark = env.primary_market_state().1.assets[0].effective_price;
    if committed_mark != published_target {
        return Err(format!(
            "{source:?} pending mark did not commit: {committed_mark}/{published_target}"
        ));
    }

    env.trade_cpi(attacker, victim, 0, -attack_size_q, 0, 0)
        .map_err(|error| format!("{source:?} close stale-price exposure: {error}"))?;
    let attacker_profit = u128::try_from(env.primary_portfolio(attacker).pnl.get())
        .map_err(|_| format!("{source:?} attacker did not realize positive PnL"))?;
    let victim_loss = victim_capital_before
        .checked_sub(env.primary_portfolio(victim).capital.get())
        .ok_or_else(|| format!("{source:?} victim capital increased"))?;
    if attacker_profit == 0 || attacker_profit != victim_loss {
        return Err(format!(
            "{source:?} stale-mark transfer mismatch: attacker={attacker_profit}, victim={victim_loss}"
        ));
    }
    env.convert_released_pnl(attacker, attacker_profit)
        .map_err(|error| format!("{source:?} convert stale-mark PnL: {error}"))?;
    env.withdraw_primary(attacker, attacker_profit)
        .map_err(|error| format!("{source:?} withdraw stale-mark PnL: {error}"))?;
    let extracted_tokens = env.token_amount(env.actors[attacker].destination_token);
    if u128::from(extracted_tokens) != attacker_profit
        || env.token_supply_observed() != supply_before
    {
        return Err(format!(
            "{source:?} stale-mark value was not externally extractable: tokens={extracted_tokens}, profit={attacker_profit}"
        ));
    }
    Ok(PendingMarkAdmissionDiscovery {
        source,
        published_target,
        stale_engine_target,
        committed_mark,
        attacker_profit,
        victim_loss,
        extracted_tokens,
    })
}

pub fn discover_pending_mark_admission_violations(
    seed: [u8; 32],
) -> Result<Vec<PendingMarkAdmissionDiscovery>, String> {
    PendingMarkSource::ALL
        .into_iter()
        .map(|source| discover_one_pending_mark_admission(seed, source))
        .collect()
}

fn discover_one_pending_mark_inheritance(
    mut seed: [u8; 32],
    route: DiscoveryTradeRoute,
) -> Result<PendingMarkInheritanceDiscovery, String> {
    const MARK: u64 = 1_000_000;
    const LARGE_Q: i128 = 50 * POS_SCALE as i128;
    const LARGE_DEPOSIT: u128 = 100_000_000;
    const SEED_DEPOSIT: u128 = 2_000_000;

    seed[0] ^= 0xd8;
    seed[1] ^= route.discriminator();
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: MARK,
            max_trading_fee_bps: 100,
            max_price_move_bps_per_slot: 100,
            max_accrual_dt_slots: 1,
            actor_deposits: [SEED_DEPOSIT, SEED_DEPOSIT, LARGE_DEPOSIT, LARGE_DEPOSIT, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.configure_ewma_mark(0, 1, MARK, 1, 0)
        .map_err(|error| format!("{route:?} configure EWMA mark: {error}"))?;
    env.top_up_backing_bucket(1, 10_000_000, 100)
        .map_err(|error| format!("{route:?} fund source backing: {error}"))?;
    env.warp_to_slot(2);
    let retained = build_retained_discovery_trade(&mut env, route, 2, 3, 0, LARGE_Q, MARK);

    let seed_capital_before = env
        .primary_portfolio(0)
        .capital
        .get()
        .checked_add(env.primary_portfolio(1).capital.get())
        .ok_or_else(|| "seed-pair capital overflow".to_string())?;
    env.trade_no_cpi(0, 1, 0, POS_SCALE as i128, MARK * 2, 0)
        .map_err(|error| format!("{route:?} seed paid EWMA move: {error}"))?;
    let seed_capital_after = env
        .primary_portfolio(0)
        .capital
        .get()
        .checked_add(env.primary_portfolio(1).capital.get())
        .ok_or_else(|| "post-move seed-pair capital overflow".to_string())?;
    let movement_cost = seed_capital_before
        .checked_sub(seed_capital_after)
        .ok_or_else(|| "EWMA move increased seed-pair capital".to_string())?;
    let pending_mark = env.primary_profile(0).mark_ewma_e6;
    if movement_cost == 0
        || pending_mark <= MARK
        || env.primary_market_state().1.assets[0].effective_price != MARK
    {
        return Err(format!(
            "{route:?} did not create a paid pending mark: cost={movement_cost}, target={pending_mark}"
        ));
    }

    env.land_retained(retained)
        .map_err(|error| format!("{route:?} retained trade no longer lands: {error}"))?;
    env.warp_to_slot(3);
    let oracle_accounts = env.primary_profile(0).oracle_leg_count;
    let observations = || {
        vec![CrankObservationHint {
            asset_index: 0,
            oracle_accounts,
        }]
    };
    env.crank(2, 3, observations())
        .map_err(|error| format!("{route:?} apply pending mark to attacker: {error}"))?;
    env.crank(3, 3, observations())
        .map_err(|error| format!("{route:?} apply pending mark to victim: {error}"))?;
    let committed_mark = env.primary_market_state().1.assets[0].effective_price;
    if committed_mark != pending_mark {
        return Err(format!(
            "{route:?} pending mark did not commit: {committed_mark}/{pending_mark}"
        ));
    }

    execute_discovery_trade_route(&mut env, route, 2, 3, 0, -LARGE_Q, committed_mark)
        .map_err(|error| format!("{route:?} close inherited exposure: {error}"))?;
    let victim_loss = LARGE_DEPOSIT
        .checked_sub(env.primary_portfolio(3).capital.get())
        .ok_or_else(|| "pending-mark victim capital increased".to_string())?;
    let attacker_pnl = env.primary_portfolio(2).pnl.get();
    if attacker_pnl != victim_loss as i128 {
        return Err(format!(
            "{route:?} inherited PnL mismatch: pnl={attacker_pnl}, loss={victim_loss}"
        ));
    }
    env.convert_released_pnl(2, victim_loss)
        .map_err(|error| format!("{route:?} convert inherited PnL: {error}"))?;
    let attacker_gain = env
        .primary_portfolio(2)
        .capital
        .get()
        .checked_sub(LARGE_DEPOSIT)
        .ok_or_else(|| "attacker remained below principal".to_string())?;
    let net_profit = attacker_gain
        .checked_sub(movement_cost)
        .ok_or_else(|| "mark movement cost exceeded attacker gain".to_string())?;
    env.withdraw_primary(2, net_profit)
        .map_err(|error| format!("{route:?} withdraw net inherited profit: {error}"))?;
    let extracted_profit = env.token_amount(env.actors[2].destination_token);
    if u128::from(extracted_profit) != net_profit || env.token_supply_observed() != supply_before {
        return Err(format!(
            "{route:?} inherited profit was not externally extractable: {extracted_profit}/{net_profit}"
        ));
    }
    Ok(PendingMarkInheritanceDiscovery {
        route,
        movement_cost,
        pending_mark,
        committed_mark,
        victim_loss,
        attacker_gain,
        extracted_profit,
    })
}

pub fn discover_pending_mark_inheritance_violations(
    seed: [u8; 32],
) -> Result<Vec<PendingMarkInheritanceDiscovery>, String> {
    DiscoveryTradeRoute::ALL
        .into_iter()
        .map(|route| discover_one_pending_mark_inheritance(seed, route))
        .collect()
}

#[derive(Clone, Copy, Debug)]
struct PendingTargetWorld {
    target: u64,
    movement_fee: u128,
    victim_payout: u128,
    coalition_payout: u128,
    supply: u128,
}

fn run_pending_target_world(
    seed: [u8; 32],
    route: DiscoveryTradeRoute,
    insert_reported_price_round_trip: bool,
) -> Result<PendingTargetWorld, String> {
    const BASIS: u64 = 10_000_000;
    const DIRECTIONAL_Q: i128 = 1_000 * POS_SCALE as i128;
    const ROUND_TRIP_Q: i128 = 100 * POS_SCALE as i128;
    const DIRECTIONAL_DEPOSIT: u128 = 20_000_000_000;
    const ROUND_TRIP_DEPOSIT: u128 = 2_000_000_000;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: BASIS,
            max_trading_fee_bps: 10_000,
            max_price_move_bps_per_slot: 5_000,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [
                DIRECTIONAL_DEPOSIT,
                DIRECTIONAL_DEPOSIT,
                ROUND_TRIP_DEPOSIT,
                ROUND_TRIP_DEPOSIT,
                1,
            ],
            actor_token_balances: [
                25_000_000_000,
                25_000_000_000,
                3_000_000_000,
                3_000_000_000,
                1,
            ],
            ..MarketConfig::default()
        },
    );
    env.configure_ewma_mark(0, 1, BASIS, 1, 0)
        .map_err(|error| format!("{route:?} configure EWMA: {error}"))?;
    env.trade_no_cpi(0, 1, 0, DIRECTIONAL_Q, BASIS, 0)
        .map_err(|error| format!("{route:?} open directional OI: {error}"))?;
    let observations = || {
        vec![CrankObservationHint {
            asset_index: 0,
            oracle_accounts: 0,
        }]
    };
    for slot in 2..=5 {
        env.warp_to_slot(slot);
        env.push_ewma_mark(0, slot, 1)
            .map_err(|error| format!("{route:?} publish low mark at slot {slot}: {error}"))?;
        for actor in [0, 1] {
            env.crank(actor, slot, observations())
                .map_err(|error| format!("{route:?} accrue actor {actor}: {error}"))?;
        }
    }
    let low_price = env.primary_market_state().1.assets[0].effective_price;
    if low_price >= BASIS / 5 {
        return Err(format!("{route:?} bounded mark did not reach low regime"));
    }

    env.warp_to_slot(6);
    for actor in [0, 1] {
        env.crank(actor, 6, observations())
            .map_err(|error| format!("{route:?} advance actor before rebound: {error}"))?;
    }
    let rebound_input = BASIS
        .checked_mul(2)
        .and_then(|value| value.checked_sub(low_price))
        .ok_or_else(|| "pending-target rebound input overflow".to_string())?;
    env.push_ewma_mark(0, 6, rebound_input)
        .map_err(|error| format!("{route:?} publish pending rebound: {error}"))?;
    if env.primary_profile(0).mark_ewma_e6 != BASIS
        || env.primary_market_state().1.assets[0].effective_price != low_price
    {
        return Err(format!("{route:?} honest rebound did not remain pending"));
    }

    env.warp_to_slot(7);
    let insurance_before = env.primary_market_state().1.insurance;
    if insert_reported_price_round_trip {
        execute_discovery_trade_route(&mut env, route, 2, 3, 0, ROUND_TRIP_Q, 1)
            .map_err(|error| format!("{route:?} open target-mutating round trip: {error}"))?;
        execute_discovery_trade_route(&mut env, route, 2, 3, 0, -ROUND_TRIP_Q, low_price)
            .map_err(|error| format!("{route:?} close target-mutating round trip: {error}"))?;
    }
    let movement_fee = env
        .primary_market_state()
        .1
        .insurance
        .checked_sub(insurance_before)
        .ok_or_else(|| "target mutation reduced insurance".to_string())?;
    let target = env.primary_profile(0).mark_ewma_e6;

    let mut slot = 7u64;
    loop {
        for actor in [0, 1] {
            env.crank(actor, slot, observations())
                .map_err(|error| format!("{route:?} converge actor {actor}: {error}"))?;
        }
        if env.primary_market_state().1.assets[0].effective_price == target {
            break;
        }
        slot = slot
            .checked_add(1)
            .ok_or_else(|| "target convergence slot overflow".to_string())?;
        if slot >= 24 {
            return Err(format!("{route:?} pending target did not converge"));
        }
        env.warp_to_slot(slot);
    }
    env.trade_no_cpi(0, 1, 0, -DIRECTIONAL_Q, target, 0)
        .map_err(|error| format!("{route:?} close directional OI: {error}"))?;
    env.resolve_market()
        .map_err(|error| format!("{route:?} resolve target world: {error}"))?;
    for actor in 0..4 {
        env.close_resolved_primary(actor)
            .map_err(|error| format!("{route:?} close resolved actor {actor}: {error}"))?;
    }
    for actor in 0..4 {
        let _ = env.claim_resolved_payout_topup_primary(actor);
    }
    let victim_payout = u128::from(env.token_amount(env.actors[0].destination_token));
    let coalition_payout = [1usize, 2, 3].into_iter().try_fold(0u128, |sum, actor| {
        sum.checked_add(u128::from(
            env.token_amount(env.actors[actor].destination_token),
        ))
        .ok_or_else(|| "pending-target coalition payout overflow".to_string())
    })?;
    Ok(PendingTargetWorld {
        target,
        movement_fee,
        victim_payout,
        coalition_payout,
        supply: env.token_supply_observed(),
    })
}

fn discover_one_pending_target_override(
    mut seed: [u8; 32],
    route: DiscoveryTradeRoute,
) -> Result<PendingTargetOverrideDiscovery, String> {
    const COALITION_DEPOSITS: u128 = 24_000_000_000;
    seed[0] ^= 0xe9;
    seed[1] ^= route.discriminator();
    let control = run_pending_target_world(seed, route, false)?;
    let reordered = run_pending_target_world(seed, route, true)?;
    let victim_payout_loss = control
        .victim_payout
        .checked_sub(reordered.victim_payout)
        .ok_or_else(|| "target mutation increased victim payout".to_string())?;
    let coalition_profit = reordered
        .coalition_payout
        .checked_sub(COALITION_DEPOSITS)
        .ok_or_else(|| "target mutation coalition did not recover deposits".to_string())?;
    Ok(PendingTargetOverrideDiscovery {
        route,
        control_target: control.target,
        reordered_target: reordered.target,
        movement_fee: reordered.movement_fee,
        victim_payout_loss,
        coalition_profit,
        control_supply: control.supply,
        reordered_supply: reordered.supply,
    })
}

pub fn discover_pending_target_override_violations(
    seed: [u8; 32],
) -> Result<Vec<PendingTargetOverrideDiscovery>, String> {
    DiscoveryTradeRoute::ALL
        .into_iter()
        .map(|route| discover_one_pending_target_override(seed, route))
        .collect()
}

#[derive(Clone, Copy, Debug)]
struct PendingMarkFeeWorld {
    pending_sync_rejected_lock: bool,
    pending_sync_exact_rollback: bool,
    reward: u128,
    victim_payout: u128,
    winner_payout: u128,
    extracted_reward: u64,
}

fn run_pending_mark_fee_world(
    seed: [u8; 32],
    fee_before_mark_commit: bool,
) -> Result<PendingMarkFeeWorld, String> {
    const OPEN_PRICE: u64 = 100;
    const ADVERSE_PRICE: u64 = 50;
    const DEPOSIT: u128 = 100_000;
    const SIZE_Q: i128 = 1_000 * POS_SCALE as i128;
    const FEE_PER_SLOT: u128 = 12_500;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: OPEN_PRICE,
            maintenance_margin_bps: 10_000,
            initial_margin_bps: 10_000,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            maintenance_fee_per_slot: FEE_PER_SLOT,
            actor_deposits: [DEPOSIT, DEPOSIT, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.configure_auth_mark(false, 0, 1, OPEN_PRICE)
        .map_err(|error| format!("configure fee-order authenticated mark: {error}"))?;
    env.update_maintenance_fee_policy(10_000)
        .map_err(|error| format!("configure maintenance reward: {error}"))?;
    env.trade_no_cpi(0, 1, 0, SIZE_Q, OPEN_PRICE, 0)
        .map_err(|error| format!("open fee-order positions: {error}"))?;

    env.warp_to_slot(9);
    env.push_auth_mark(0, 9, OPEN_PRICE)
        .map_err(|error| format!("advance authenticated fee clock: {error}"))?;
    for _ in 0..16 {
        let _ = env.crank(
            2,
            9,
            vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: 0,
            }],
        );
        if env.primary_market_state().1.assets[0].slot_last == 9 {
            break;
        }
    }
    if env.primary_portfolio(0).last_fee_slot.get() != 1
        || env.primary_market_state().1.assets[0].slot_last != 9
    {
        return Err("fee-order setup did not isolate maintenance debt".into());
    }

    env.warp_to_slot(10);
    env.push_auth_mark(0, 10, ADVERSE_PRICE)
        .map_err(|error| format!("publish adverse pending mark: {error}"))?;
    let (pending_profile, pending_group) = env.primary_market_state();
    if pending_profile.oracle_target_price_e6 != ADVERSE_PRICE
        || pending_group.assets[0].effective_price != OPEN_PRICE
    {
        return Err("fee-order setup did not retain the adverse mark".into());
    }

    let mut pending_sync_rejected_lock = false;
    let mut pending_sync_exact_rollback = true;
    if fee_before_mark_commit {
        let before = fingerprint(&env);
        match env.sync_maintenance_fee_with_reward(0, 2, 10) {
            Ok(_) => return Err("pending-mark fee sync still landed".into()),
            Err(error)
                if error.contains("Custom(21)") || error.contains("custom program error: 0x15") =>
            {
                pending_sync_rejected_lock = true;
                pending_sync_exact_rollback = fingerprint(&env) == before;
            }
            Err(error) => {
                return Err(format!(
                    "pending-mark fee sync returned an unexpected error: {error}"
                ))
            }
        }
    }
    for _ in 0..16 {
        let _ = env.crank(
            0,
            10,
            vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: 0,
            }],
        );
        if env.primary_market_state().1.assets[0].effective_price == ADVERSE_PRICE {
            break;
        }
    }
    if env.primary_market_state().1.assets[0].effective_price != ADVERSE_PRICE {
        return Err("adverse pending mark did not commit".into());
    }
    env.sync_maintenance_fee_with_reward(0, 2, 10)
        .map_err(|error| format!("post-commit fee sync rejected: {error}"))?;

    let cranker_capital = env.primary_portfolio(2).capital.get();
    let reward = cranker_capital
        .checked_sub(1)
        .ok_or_else(|| "cranker capital fell below deposit".to_string())?;
    env.withdraw_primary(2, cranker_capital)
        .map_err(|error| format!("withdraw maintenance reward: {error}"))?;
    let extracted_reward = env
        .token_amount(env.actors[2].destination_token)
        .checked_sub(1)
        .ok_or_else(|| "cranker withdrawal lost deposit".to_string())?;
    if u128::from(extracted_reward) != reward {
        return Err("maintenance reward did not reach SPL destination".into());
    }

    for _ in 0..24 {
        for actor in [0, 1] {
            let _ = env.crank(
                actor,
                10,
                vec![CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                }],
            );
        }
    }
    env.resolve_market()
        .map_err(|error| format!("resolve fee-order world: {error}"))?;
    let victim_payout = drain_resolved_discovery_actor(&mut env, 0)?;
    let winner_payout = drain_resolved_discovery_actor(&mut env, 1)?;
    if env.token_supply_observed() != supply_before {
        return Err("fee-order world changed SPL supply".into());
    }
    Ok(PendingMarkFeeWorld {
        pending_sync_rejected_lock,
        pending_sync_exact_rollback,
        reward,
        victim_payout,
        winner_payout,
        extracted_reward,
    })
}

pub fn discover_pending_mark_fee_ordering(
    mut seed: [u8; 32],
) -> Result<PendingMarkFeeOrderingDiscovery, String> {
    seed[0] ^= 0xfa;
    let control = run_pending_mark_fee_world(seed, false)?;
    let reordered = run_pending_mark_fee_world(seed, true)?;
    let control_total = control
        .reward
        .checked_add(control.victim_payout)
        .and_then(|value| value.checked_add(control.winner_payout))
        .ok_or_else(|| "control fee-order total overflow".to_string())?;
    let reordered_total = reordered
        .reward
        .checked_add(reordered.victim_payout)
        .and_then(|value| value.checked_add(reordered.winner_payout))
        .ok_or_else(|| "reordered fee-order total overflow".to_string())?;
    if !reordered.pending_sync_rejected_lock
        || !reordered.pending_sync_exact_rollback
        || control.reward != reordered.reward
        || control.victim_payout != reordered.victim_payout
        || control.winner_payout != reordered.winner_payout
        || control.extracted_reward != reordered.extracted_reward
        || control_total != reordered_total
    {
        return Err(format!(
            "fee-order paired worlds did not reject and converge: \
             control={control:?}, reordered={reordered:?}"
        ));
    }
    Ok(PendingMarkFeeOrderingDiscovery {
        pending_sync_rejected_lock: reordered.pending_sync_rejected_lock,
        pending_sync_exact_rollback: reordered.pending_sync_exact_rollback,
        control_reward: control.reward,
        reordered_reward: reordered.reward,
        control_winner_payout: control.winner_payout,
        reordered_winner_payout: reordered.winner_payout,
        control_victim_payout: control.victim_payout,
        reordered_victim_payout: reordered.victim_payout,
        extracted_reward: reordered.extracted_reward,
    })
}

fn discover_one_mark_movement_reserve_violation(
    mut seed: [u8; 32],
    route: DiscoveryTradeRoute,
) -> Result<MarkMovementReserveDiscovery, String> {
    const ASSET: u16 = 1;
    const MARK: u64 = 1_000_000;
    const LOW_PRINT: u64 = 1;
    const POSITION_Q: i128 = 1_000 * POS_SCALE as i128;
    const DEPOSIT: u128 = 2_000_000_000;
    const INIT_FEE: u128 = 1;

    seed[0] ^= 0x1b;
    seed[1] ^= route.discriminator();
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: MARK,
            h_max: 20,
            max_trading_fee_bps: 100,
            max_price_move_bps_per_slot: 100,
            max_accrual_dt_slots: 20,
            min_funding_lifetime_slots: 20,
            actor_deposits: [DEPOSIT, DEPOSIT, DEPOSIT, DEPOSIT, 1],
            actor_token_balances: [
                2_500_000_000,
                2_500_000_000,
                2_500_000_000,
                2_500_000_000,
                1,
            ],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.update_market_init_fee_policy(INIT_FEE)
        .map_err(|error| format!("{route:?} configure activation fee: {error}"))?;
    env.warp_to_slot(2);
    env.retire_asset(ASSET, 2)
        .map_err(|error| format!("{route:?} retire reusable asset: {error}"))?;
    env.warp_to_slot(3);
    env.activate_permissionless_asset_for_actor(0, ASSET, 3, MARK, 0, INIT_FEE)
        .map_err(|error| format!("{route:?} activate fee-provider asset: {error}"))?;
    env.configure_ewma_mark_for_actor(0, ASSET, 3, MARK, 1, 0)
        .map_err(|error| format!("{route:?} configure fee-provider EWMA: {error}"))?;

    execute_discovery_trade_route(&mut env, route, 0, 3, ASSET, -POSITION_Q, MARK)
        .map_err(|error| format!("{route:?} open independent exposure: {error}"))?;
    env.warp_to_slot(4);
    let insurance_before = env.primary_market_state().1.insurance;
    env.trade_no_cpi(1, 2, ASSET, POSITION_Q, LOW_PRINT, 0)
        .map_err(|error| format!("{route:?} pay downward mark movement: {error}"))?;
    env.trade_no_cpi(1, 2, ASSET, -POSITION_Q, LOW_PRINT, 0)
        .map_err(|error| format!("{route:?} flatten mark-moving pair: {error}"))?;
    let movement_fee = env
        .primary_market_state()
        .1
        .insurance
        .checked_sub(insurance_before)
        .ok_or_else(|| "mark movement reduced insurance".to_string())?;
    let queued_mark = env.primary_profile(ASSET as usize).mark_ewma_e6;
    if movement_fee == 0 || queued_mark >= MARK {
        return Err(format!(
            "{route:?} failed to create paid downward mark: fee={movement_fee}, mark={queued_mark}"
        ));
    }

    let destination_before = env.token_amount(env.actors[0].destination_token);
    env.withdraw_insurance_asset(0, ASSET, movement_fee)
        .map_err(|error| format!("{route:?} withdraw movement reserve: {error}"))?;
    let withdrawn_reserve = u128::from(
        env.token_amount(env.actors[0].destination_token)
            .checked_sub(destination_before)
            .ok_or_else(|| "reserve destination decreased".to_string())?,
    );

    let oracle_accounts = env.primary_profile(ASSET as usize).oracle_leg_count;
    let observations = || {
        vec![CrankObservationHint {
            asset_index: ASSET,
            oracle_accounts,
        }]
    };
    env.crank(0, 4, observations())
        .map_err(|error| format!("{route:?} apply mark to coalition: {error}"))?;
    env.crank(3, 4, observations())
        .map_err(|error| format!("{route:?} apply mark to victim: {error}"))?;
    let committed_mark = env.primary_market_state().1.assets[ASSET as usize].effective_price;
    if committed_mark >= MARK {
        return Err(format!("{route:?} downward mark did not commit"));
    }
    execute_discovery_trade_route(&mut env, route, 0, 3, ASSET, POSITION_Q, committed_mark)
        .map_err(|error| format!("{route:?} close independent exposure: {error}"))?;
    env.crank(0, 4, Vec::new())
        .map_err(|error| format!("{route:?} settle coalition close: {error}"))?;
    env.crank(3, 4, Vec::new())
        .map_err(|error| format!("{route:?} settle victim close: {error}"))?;

    for actor in 0..4 {
        let pnl = env.primary_portfolio(actor).pnl.get();
        if pnl > 0 {
            env.convert_released_pnl(actor, pnl as u128)
                .map_err(|error| format!("{route:?} convert actor {actor} PnL: {error}"))?;
        }
        let capital = env.primary_portfolio(actor).capital.get();
        if capital != 0 {
            env.withdraw_primary(actor, capital)
                .map_err(|error| format!("{route:?} withdraw actor {actor}: {error}"))?;
        }
    }
    let coalition_payout = [0usize, 1, 2].into_iter().try_fold(0u128, |sum, actor| {
        sum.checked_add(u128::from(
            env.token_amount(env.actors[actor].destination_token),
        ))
        .ok_or_else(|| "movement-reserve coalition payout overflow".to_string())
    })?;
    let victim_payout = u128::from(env.token_amount(env.actors[3].destination_token));
    let coalition_committed = DEPOSIT
        .checked_mul(3)
        .and_then(|value| value.checked_add(INIT_FEE))
        .ok_or_else(|| "movement-reserve coalition commitment overflow".to_string())?;
    let victim_loss = DEPOSIT
        .checked_sub(victim_payout)
        .ok_or_else(|| "movement-reserve victim payout exceeded deposit".to_string())?;
    let coalition_gain = coalition_payout
        .checked_sub(coalition_committed)
        .ok_or_else(|| "movement-reserve coalition did not recover deposits".to_string())?;
    if env.token_supply_observed() != supply_before {
        return Err("movement-reserve world changed SPL supply".into());
    }
    Ok(MarkMovementReserveDiscovery {
        route,
        movement_fee,
        withdrawn_reserve,
        victim_loss,
        coalition_gain,
        committed_mark,
    })
}

pub fn discover_mark_movement_reserve_violations(
    seed: [u8; 32],
) -> Result<Vec<MarkMovementReserveDiscovery>, String> {
    DiscoveryTradeRoute::ALL
        .into_iter()
        .map(|route| discover_one_mark_movement_reserve_violation(seed, route))
        .collect()
}

fn discover_one_trade_driven_liquidation(
    mut seed: [u8; 32],
    mode: TradeDrivenMarkMode,
    route: ProspectiveAccrualRoute,
) -> Result<TradeDrivenLiquidationDiscovery, String> {
    const MARK: u64 = 1_000_000;
    const VICTIM_DEPOSIT: u128 = 50_000;
    const HONEST_DEPOSIT: u128 = 2_000_000;
    const ATTACK_DEPOSIT: u128 = 1_000;
    const CRANKER_DEPOSIT: u128 = 1;
    const TINY_Q: i128 = (POS_SCALE / 10_000) as i128;

    seed[0] ^= 0x2c;
    seed[1] ^= mode.discriminator();
    seed[2] ^= route.discriminator();
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: MARK,
            h_max: 6_480_000,
            min_nonzero_mm_req: 599,
            min_nonzero_im_req: 600,
            maintenance_margin_bps: 500,
            initial_margin_bps: 500,
            liquidation_fee_bps: 5,
            liquidation_fee_cap: percolator::MAX_PROTOCOL_FEE_ABS,
            min_liquidation_abs: 500,
            max_price_move_bps_per_slot: 24,
            max_accrual_dt_slots: 1,
            max_abs_funding_e9_per_slot: 1_000,
            min_funding_lifetime_slots: 10_000_000,
            actor_deposits: [
                VICTIM_DEPOSIT,
                HONEST_DEPOSIT,
                ATTACK_DEPOSIT,
                ATTACK_DEPOSIT,
                CRANKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    env.update_liquidation_fee_policy(10_000)
        .map_err(|error| format!("{mode:?} configure liquidation reward: {error}"))?;

    let mut oracle_accounts = Vec::new();
    let (trade_slot, reported_price) = match mode {
        TradeDrivenMarkMode::Ewma => {
            env.configure_ewma_mark(0, 1, MARK, 1, 0)
                .map_err(|error| format!("{route:?} configure EWMA: {error}"))?;
            env.warp_to_slot(2);
            (2, 999_800)
        }
        TradeDrivenMarkMode::HybridAfterHours => {
            env.set_clock(1, 100);
            let feed = [0xedu8; 32];
            let initial_oracle = env.set_pyth_price(&feed, MARK as i64, -6, 0, 100);
            env.configure_hybrid_oracle(
                0,
                1,
                100,
                0,
                [feed, [0; 32], [0; 32]],
                &[initial_oracle],
                1,
                0,
            )
            .map_err(|error| format!("{route:?} configure hybrid fallback: {error}"))?;
            oracle_accounts.push(env.set_pyth_price(&feed, 999_850, -6, 0, 1_000));
            env.set_clock(3, 1_000);
            (3, 999_850)
        }
    };

    env.trade_no_cpi(0, 1, 0, POS_SCALE as i128, MARK, 0)
        .map_err(|error| format!("{mode:?} open independent victim: {error}"))?;
    let supply_before = env.token_supply_observed();
    let victim_capital_before = env.primary_portfolio(0).capital.get();
    let victim_oi_before = env.primary_market_state().1.assets[0].oi_eff_long_q;
    let insurance_before = env.primary_market_state().1.insurance;
    let move_size_q = if matches!(
        route,
        ProspectiveAccrualRoute::Cpi | ProspectiveAccrualRoute::BatchCpi
    ) {
        env.set_matcher_spreads(3, 2, 0)
            .map_err(|error| format!("{mode:?} {route:?} configure adverse matcher: {error}"))?;
        -TINY_Q
    } else {
        TINY_Q
    };
    execute_reported_price_route(&mut env, route, 2, 3, move_size_q, reported_price)
        .map_err(|error| format!("{mode:?} {route:?} move mark: {error}"))?;
    let (profile_after_move, group_after_move) = env.primary_market_state();
    let movement_fee = group_after_move
        .insurance
        .checked_sub(insurance_before)
        .ok_or_else(|| "trade-driven mark movement reduced insurance".to_string())?;
    let queued_mark = profile_after_move.mark_ewma_e6;
    if movement_fee == 0 || queued_mark >= MARK {
        return Err(format!(
            "{mode:?} {route:?} did not create paid adverse mark: fee={movement_fee}, mark={queued_mark}"
        ));
    }

    let cranker_before = env.primary_portfolio(4).capital.get();
    for attempt in 0..8 {
        env.crank_with_reward(
            4,
            0,
            trade_slot,
            if attempt == 0 {
                vec![CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: oracle_accounts.len() as u8,
                }]
            } else {
                Vec::new()
            },
            if attempt == 0 { &oracle_accounts } else { &[] },
        )
        .map_err(|error| format!("{mode:?} {route:?} liquidation crank {attempt}: {error}"))?;
        if env.primary_market_state().1.assets[0].oi_eff_long_q < victim_oi_before {
            break;
        }
    }
    let victim_oi_after = env.primary_market_state().1.assets[0].oi_eff_long_q;
    let oi_reduction_q = victim_oi_before
        .checked_sub(victim_oi_after)
        .ok_or_else(|| "liquidation increased victim-side OI".to_string())?;
    let liquidation_reward = env
        .primary_portfolio(4)
        .capital
        .get()
        .checked_sub(cranker_before)
        .ok_or_else(|| "liquidation reduced cranker capital".to_string())?;
    let victim_capital_loss = victim_capital_before
        .checked_sub(env.primary_portfolio(0).capital.get())
        .ok_or_else(|| "liquidation increased victim capital".to_string())?;

    for actor in [2usize, 3] {
        env.crank(actor, trade_slot, Vec::new())
            .map_err(|error| format!("{mode:?} refresh coalition actor {actor}: {error}"))?;
    }
    execute_reported_price_route(&mut env, route, 2, 3, -move_size_q, queued_mark)
        .map_err(|error| format!("{mode:?} {route:?} close mark-moving pair: {error}"))?;
    for actor in [2usize, 3] {
        let pnl = env.primary_portfolio(actor).pnl.get();
        if pnl > 0 {
            env.convert_released_pnl(actor, pnl as u128)
                .map_err(|error| format!("{mode:?} convert actor {actor} PnL: {error}"))?;
        }
    }
    for actor in [2usize, 3, 4] {
        let capital = env.primary_portfolio(actor).capital.get();
        if capital != 0 {
            env.withdraw_primary(actor, capital)
                .map_err(|error| format!("{mode:?} withdraw coalition actor {actor}: {error}"))?;
        }
    }
    let extracted_tokens = [2usize, 3, 4].into_iter().try_fold(0u128, |sum, actor| {
        sum.checked_add(u128::from(
            env.token_amount(env.actors[actor].destination_token),
        ))
        .ok_or_else(|| "trade-driven coalition payout overflow".to_string())
    })?;
    let coalition_committed = ATTACK_DEPOSIT
        .checked_mul(2)
        .and_then(|value| value.checked_add(CRANKER_DEPOSIT))
        .ok_or_else(|| "trade-driven coalition commitment overflow".to_string())?;
    let coalition_profit = extracted_tokens
        .checked_sub(coalition_committed)
        .ok_or_else(|| "trade-driven coalition did not recover deposits".to_string())?;
    if env.token_supply_observed() != supply_before {
        return Err("trade-driven liquidation changed SPL supply".into());
    }
    Ok(TradeDrivenLiquidationDiscovery {
        mode,
        route,
        movement_fee,
        liquidation_reward,
        victim_capital_loss,
        oi_reduction_q,
        coalition_profit,
        extracted_tokens,
    })
}

pub fn discover_trade_driven_liquidation_violations(
    seed: [u8; 32],
) -> Result<Vec<TradeDrivenLiquidationDiscovery>, String> {
    let mut discoveries = Vec::new();
    for mode in TradeDrivenMarkMode::ALL {
        for route in ProspectiveAccrualRoute::ALL {
            discoveries.push(discover_one_trade_driven_liquidation(seed, mode, route)?);
        }
    }
    Ok(discoveries)
}

fn discovery_portfolio_equity(env: &V16Svm, actor: usize) -> Result<i128, String> {
    let account = env.primary_portfolio(actor);
    i128::try_from(account.capital.get())
        .map_err(|_| "portfolio capital exceeds signed range".to_string())?
        .checked_add(account.pnl.get())
        .ok_or_else(|| "portfolio equity overflow".to_string())
}

fn discover_one_bilateral_mark_fee_violation(
    mut seed: [u8; 32],
    mode: TradeDrivenMarkMode,
    route: DiscoveryTradeRoute,
) -> Result<BilateralMarkFeeDiscovery, String> {
    if !matches!(
        route,
        DiscoveryTradeRoute::Cpi | DiscoveryTradeRoute::BatchCpi
    ) {
        return Err(format!("{route:?} is not a matcher-priced route"));
    }
    const MARK: u64 = 1_000_000;
    const ADVERSE_MARK: u64 = 1_999_999;
    const MOVER_Q: i128 = POS_SCALE as i128;
    const BENEFICIARY_Q: i128 = 10 * POS_SCALE as i128;
    const LARGE_DEPOSIT: u128 = 50_000_000;

    seed[0] ^= 0x3d;
    seed[1] ^= mode.discriminator();
    seed[2] ^= route.discriminator();
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: MARK,
            max_trading_fee_bps: 10_000,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [
                MARK as u128,
                LARGE_DEPOSIT,
                LARGE_DEPOSIT,
                LARGE_DEPOSIT,
                LARGE_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    let close_lp = env.add_primary_actor(seed, 0, 200_000_000, LARGE_DEPOSIT);
    let supply_before = env.token_supply_observed();
    let hybrid_feed = match mode {
        TradeDrivenMarkMode::Ewma => {
            env.configure_ewma_mark(0, 1, MARK, 1, 0)
                .map_err(|error| format!("configure bilateral EWMA: {error}"))?;
            None
        }
        TradeDrivenMarkMode::HybridAfterHours => {
            env.set_clock(1, 100);
            let feed = [0xceu8; 32];
            let initial_oracle = env.set_pyth_price(&feed, MARK as i64, -6, 100, 100);
            env.configure_hybrid_oracle(
                0,
                1,
                100,
                0,
                [feed, [0; 32], [0; 32]],
                &[initial_oracle],
                1,
                100,
            )
            .map_err(|error| format!("configure bilateral hybrid: {error}"))?;
            Some(feed)
        }
    };

    env.set_matcher_spreads(1, 0, 9_000)
        .map_err(|error| format!("configure opening passive matcher: {error}"))?;
    env.trade_cpi(0, 1, 0, -MOVER_Q, 0, 0)
        .map_err(|error| format!("open future underfunded mover: {error}"))?;
    env.trade_no_cpi(2, 3, 0, BENEFICIARY_Q, MARK, 0)
        .map_err(|error| format!("open independent beneficiary book: {error}"))?;
    env.trade_no_cpi(4, close_lp, 0, BENEFICIARY_Q, MARK, 0)
        .map_err(|error| format!("open independent extraction book: {error}"))?;
    env.set_matcher_spreads(close_lp, 0, 9_000)
        .map_err(|error| format!("configure extraction matcher: {error}"))?;

    let hybrid_tail = match mode {
        TradeDrivenMarkMode::Ewma => {
            env.warp_to_slot(10);
            env.push_ewma_mark(0, 10, ADVERSE_MARK)
                .map_err(|error| format!("publish bilateral EWMA mark: {error}"))?;
            None
        }
        TradeDrivenMarkMode::HybridAfterHours => {
            env.set_clock(10, 110);
            Some(env.set_pyth_price(
                &hybrid_feed.ok_or_else(|| "hybrid feed missing".to_string())?,
                ADVERSE_MARK as i64,
                -6,
                100,
                110,
            ))
        }
    };
    let setup_actor_count = env.actors.len();
    for step in 0..16 {
        if env.primary_market_state().1.assets[0].slot_last >= 10 {
            break;
        }
        let observations = vec![CrankObservationHint {
            asset_index: 0,
            oracle_accounts: u8::from(hybrid_tail.is_some()),
        }];
        let result = if let Some(oracle) = hybrid_tail {
            env.crank_with_oracles(1, 10, observations, &[oracle])
        } else {
            env.crank(1, 10, observations)
        };
        result.map_err(|error| format!("setup market catch-up step {step}: {error}"))?;
    }
    if env.primary_market_state().1.assets[0].slot_last < 10 {
        return Err("bilateral setup market did not reach slot 10".into());
    }
    for actor in 0..setup_actor_count {
        if actor == 1 {
            continue;
        }
        let observations = if actor == 0 || hybrid_tail.is_some() {
            vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: u8::from(hybrid_tail.is_some()),
            }]
        } else {
            Vec::new()
        };
        if let Some(oracle) = hybrid_tail {
            env.crank_with_oracles(actor, 10, observations, &[oracle])
        } else {
            env.crank(actor, 10, observations)
        }
        .map_err(|error| format!("setup crank actor {actor}: {error}"))?;
    }
    let setup_mark = env.primary_market_state().1.assets[0].effective_price;
    let mover_capital = env.primary_portfolio(0).capital.get();
    if mover_capital == 0 || mover_capital >= u128::from(setup_mark) {
        return Err(format!(
            "fixture did not leave a live underfunded mover: capital={mover_capital}, mark={setup_mark}"
        ));
    }

    let coalition_equity_before = discovery_portfolio_equity(&env, 0)?
        .checked_add(discovery_portfolio_equity(&env, 2)?)
        .ok_or_else(|| "coalition pre-equity overflow".to_string())?;
    let victim_equity_before = discovery_portfolio_equity(&env, 3)?;
    let fee_counterparty_equity_before = discovery_portfolio_equity(&env, 1)?;
    let insurance_before = env.primary_market_state().1.insurance;

    env.set_matcher_spreads(1, 9_000, 9_000)
        .map_err(|error| format!("configure exit matcher: {error}"))?;
    match mode {
        TradeDrivenMarkMode::Ewma => env.warp_to_slot(20),
        TradeDrivenMarkMode::HybridAfterHours => env.set_clock(20, 1_000),
    }
    execute_discovery_trade_route(&mut env, route, 0, 1, 0, MOVER_Q, 0)
        .map_err(|error| format!("underfunded risk-reducing exit: {error}"))?;
    let queued_mark = env.primary_profile(0).mark_ewma_e6;
    for actor in [2usize, 3] {
        let observations = if actor == 2 || hybrid_tail.is_some() {
            vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: u8::from(hybrid_tail.is_some()),
            }]
        } else {
            Vec::new()
        };
        if let Some(oracle) = hybrid_tail {
            env.crank_with_oracles(actor, 20, observations, &[oracle])
        } else {
            env.crank(actor, 20, observations)
        }
        .map_err(|error| format!("apply bilateral mark to actor {actor}: {error}"))?;
    }

    let victim_loss = u128::try_from(
        victim_equity_before
            .checked_sub(discovery_portfolio_equity(&env, 3)?)
            .ok_or_else(|| "bilateral mark increased victim equity".to_string())?,
    )
    .map_err(|_| "bilateral victim loss is negative".to_string())?;
    let fee_counterparty_loss = u128::try_from(
        fee_counterparty_equity_before
            .checked_sub(discovery_portfolio_equity(&env, 1)?)
            .ok_or_else(|| "bilateral mark increased fee counterparty equity".to_string())?,
    )
    .map_err(|_| "fee counterparty loss is negative".to_string())?;
    let insurance_gain = env
        .primary_market_state()
        .1
        .insurance
        .checked_sub(insurance_before)
        .ok_or_else(|| "bilateral mark reduced insurance".to_string())?;

    env.trade_cpi(2, close_lp, 0, -BENEFICIARY_Q, 0, 0)
        .map_err(|error| format!("close beneficiary through independent LP: {error}"))?;
    let released = env.primary_portfolio(2).pnl.get().max(0) as u128;
    if released == 0 {
        return Err("bilateral mark produced no releasable coalition PnL".into());
    }
    env.convert_released_pnl(2, released)
        .map_err(|error| format!("convert bilateral coalition PnL: {error}"))?;
    for actor in [2usize, 0] {
        let capital = env.primary_portfolio(actor).capital.get();
        if capital != 0 {
            env.withdraw_primary(actor, capital)
                .map_err(|error| format!("withdraw bilateral actor {actor}: {error}"))?;
        }
    }
    let extracted_tokens = u128::from(env.token_amount(env.actors[0].destination_token))
        .checked_add(u128::from(
            env.token_amount(env.actors[2].destination_token),
        ))
        .ok_or_else(|| "bilateral extracted SPL overflow".to_string())?;
    let coalition_equity_before = u128::try_from(coalition_equity_before)
        .map_err(|_| "coalition began insolvent".to_string())?;
    let coalition_excess = extracted_tokens.saturating_sub(coalition_equity_before);
    if env.token_supply_observed() != supply_before {
        return Err("bilateral mark-fee world changed SPL supply".into());
    }
    Ok(BilateralMarkFeeDiscovery {
        mode,
        route,
        setup_mark,
        queued_mark,
        coalition_equity_before,
        coalition_excess,
        victim_loss,
        fee_counterparty_loss,
        insurance_gain,
        extracted_tokens,
    })
}

pub fn discover_bilateral_mark_fee_violations(
    seed: [u8; 32],
) -> Result<Vec<BilateralMarkFeeDiscovery>, String> {
    let mut discoveries = Vec::new();
    for mode in TradeDrivenMarkMode::ALL {
        for route in [DiscoveryTradeRoute::Cpi, DiscoveryTradeRoute::BatchCpi] {
            discoveries.push(discover_one_bilateral_mark_fee_violation(
                seed, mode, route,
            )?);
        }
    }
    Ok(discoveries)
}

fn discover_one_composite_rounding_violation(
    mut seed: [u8; 32],
    scale: CompositeRoundingScale,
) -> Result<CompositeRoundingDiscovery, String> {
    seed[0] ^= 0x4e;
    seed[1] ^= scale.discriminator();
    let (
        exact_mark,
        initial_prices,
        fresh_prices,
        victim_deposit,
        counterparty_deposit,
        cranker_deposit,
        size_units,
        catch_up_steps,
        catch_up_dt,
        soft_stale_slots,
    ) = match scale {
        CompositeRoundingScale::LargeMove => (
            1_500_000u64,
            [3i64, 1_000_000, 2],
            [3i64, 2_000_000, 1],
            540_000u128,
            540_000u128,
            1_000u128,
            1u128,
            12usize,
            20u64,
            1_000u64,
        ),
        CompositeRoundingScale::MicroMove => (
            1_002_000u64,
            [501i64, 1_000_000, 500],
            [501i64, 500_000_000, 1],
            50_100_000u128,
            50_100_000u128,
            1u128,
            1_000u128,
            1usize,
            1u64,
            3u64,
        ),
    };
    let exact_composite = |prices: [i64; 3]| -> Result<u128, String> {
        let p0 = u128::try_from(prices[0]).map_err(|_| "negative composite leg 0")?;
        let p1 = u128::try_from(prices[1]).map_err(|_| "negative composite leg 1")?;
        let p2 = u128::try_from(prices[2]).map_err(|_| "negative composite leg 2")?;
        p0.checked_mul(1_000_000_000_000)
            .and_then(|value| value.checked_div(p1.checked_mul(p2)?))
            .ok_or_else(|| "exact composite arithmetic failed".to_string())
    };
    if exact_composite(initial_prices)? != u128::from(exact_mark)
        || exact_composite(fresh_prices)? != u128::from(exact_mark)
    {
        return Err(format!("{scale:?} fixture changed exact composite value"));
    }

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: exact_mark,
            h_max: 6_480_000,
            min_nonzero_mm_req: 599,
            min_nonzero_im_req: 600,
            maintenance_margin_bps: 500,
            initial_margin_bps: 500,
            liquidation_fee_bps: 5,
            liquidation_fee_cap: percolator::MAX_PROTOCOL_FEE_ABS,
            max_price_move_bps_per_slot: 24,
            max_accrual_dt_slots: 20,
            max_abs_funding_e9_per_slot: 1_000,
            min_funding_lifetime_slots: 10_000_000,
            actor_deposits: [
                victim_deposit,
                counterparty_deposit,
                cranker_deposit,
                USER_DEPOSIT,
                1,
            ],
            ..MarketConfig::default()
        },
    );
    env.update_liquidation_fee_policy(5_000)
        .map_err(|error| format!("{scale:?} configure cranker share: {error}"))?;
    env.set_clock(1, 100);
    let feeds = [[0xe1u8; 32], [0xe2u8; 32], [0xe3u8; 32]];
    let initial_oracles: Vec<_> = initial_prices
        .iter()
        .enumerate()
        .map(|(index, price)| env.set_pyth_price(&feeds[index], *price, -6, 0, 100))
        .collect();
    env.configure_hybrid_oracle(
        0,
        1,
        100,
        ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3,
        feeds,
        &initial_oracles,
        soft_stale_slots,
        0,
    )
    .map_err(|error| format!("{scale:?} configure exact composite: {error}"))?;
    if env.primary_market_state().0.oracle_target_price_e6 != exact_mark {
        return Err(format!(
            "{scale:?} initial target differs from exact composite"
        ));
    }

    let size_q = size_units
        .checked_mul(POS_SCALE)
        .and_then(|value| i128::try_from(value).ok())
        .ok_or_else(|| "composite victim size overflow".to_string())?;
    env.trade_no_cpi(0, 1, 0, size_q, exact_mark, 0)
        .map_err(|error| format!("{scale:?} open victim exposure: {error}"))?;
    let victim_capital_before = env.primary_portfolio(0).capital.get();
    let oi_before = env.primary_market_state().1.assets[0].oi_eff_long_q;
    let cranker_capital_before = env.primary_portfolio(2).capital.get();
    let supply_before = env.token_supply_observed();

    let fresh_oracles: Vec<_> = fresh_prices
        .iter()
        .enumerate()
        .map(|(index, price)| env.set_pyth_price(&feeds[index], *price, -6, 0, 101))
        .collect();
    let observations = || {
        vec![CrankObservationHint {
            asset_index: 0,
            oracle_accounts: 3,
        }]
    };
    let mut slot = 1u64;
    for _ in 0..catch_up_steps {
        slot = slot
            .checked_add(catch_up_dt)
            .ok_or_else(|| "composite catch-up slot overflow".to_string())?;
        env.set_clock(slot, 101);
        env.crank_with_oracles(2, slot, observations(), &fresh_oracles)
            .map_err(|error| format!("{scale:?} catch-up at slot {slot}: {error}"))?;
    }
    env.crank_with_oracles(0, slot, observations(), &fresh_oracles)
        .map_err(|error| format!("{scale:?} refresh victim: {error}"))?;
    let victim_cert = env
        .primary_portfolio(0)
        .health_cert
        .try_to_runtime()
        .map_err(|error| format!("{scale:?} decode victim certificate: {error:?}"))?;
    let certified_liq_deficit = victim_cert.certified_liq_deficit;
    if certified_liq_deficit != 0 {
        env.crank_with_reward(2, 0, slot, observations(), &fresh_oracles)
            .map_err(|error| format!("{scale:?} rounded-price liquidation: {error}"))?;
    }
    let (wrapper_after, group_after) = env.primary_market_state();
    let victim_capital_loss = victim_capital_before
        .checked_sub(env.primary_portfolio(0).capital.get())
        .ok_or_else(|| "composite liquidation increased victim capital".to_string())?;
    let oi_reduction_q = oi_before
        .checked_sub(group_after.assets[0].oi_eff_long_q)
        .ok_or_else(|| "composite liquidation increased victim OI".to_string())?;
    let cranker_reward = env
        .primary_portfolio(2)
        .capital
        .get()
        .checked_sub(cranker_capital_before)
        .ok_or_else(|| "composite liquidation reduced cranker capital".to_string())?;
    if cranker_reward != 0 {
        env.withdraw_primary(2, cranker_reward)
            .map_err(|error| format!("{scale:?} withdraw liquidation reward: {error}"))?;
    }
    let extracted_tokens = env.token_amount(env.actors[2].destination_token);
    if env.token_supply_observed() != supply_before {
        return Err("composite rounding world changed SPL supply".into());
    }
    Ok(CompositeRoundingDiscovery {
        scale,
        exact_mark,
        rounded_target: wrapper_after.oracle_target_price_e6,
        rounded_mark: group_after.assets[0].effective_price,
        certified_liq_deficit,
        victim_capital_loss,
        oi_reduction_q,
        cranker_reward,
        extracted_tokens,
    })
}

pub fn discover_composite_rounding_violations(
    seed: [u8; 32],
) -> Result<Vec<CompositeRoundingDiscovery>, String> {
    CompositeRoundingScale::ALL
        .into_iter()
        .map(|scale| discover_one_composite_rounding_violation(seed, scale))
        .collect()
}

#[derive(Clone, Copy, Debug)]
struct ObservationOmissionWorld {
    call_landed: bool,
    rejected_nonprogress: bool,
    exact_rollback: bool,
    f_long_num: i128,
    f_short_num: i128,
    victim_payout: u128,
    counterparty_payout: u128,
}

fn run_observation_omission_world(
    seed: [u8; 32],
    omit_selected_observation: bool,
) -> Result<ObservationOmissionWorld, String> {
    const PRICE: u64 = 100;
    const MARK: u64 = 99;
    const DEPOSIT: u128 = 100_000_000;
    const SIZE_Q: i128 = 100_000 * POS_SCALE as i128;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            max_price_move_bps_per_slot: 1,
            max_accrual_dt_slots: 1,
            max_abs_funding_e9_per_slot: 10_000,
            min_funding_lifetime_slots: 1,
            actor_deposits: [DEPOSIT, DEPOSIT, DEPOSIT, DEPOSIT, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.trade_no_cpi(0, 1, 0, SIZE_Q, PRICE, 0)
        .map_err(|error| format!("open selected-asset pair: {error}"))?;
    env.trade_no_cpi(2, 3, 1, POS_SCALE as i128, PRICE, 0)
        .map_err(|error| format!("open unrelated-epoch pair: {error}"))?;

    env.warp_to_slot(2);
    env.push_auth_mark(0, 2, MARK)
        .map_err(|error| format!("stage selected rounded mark: {error}"))?;
    let selected_observation = vec![CrankObservationHint {
        asset_index: 0,
        oracle_accounts: env.primary_profile(0).oracle_leg_count,
    }];
    env.crank(0, 2, selected_observation.clone())
        .map_err(|error| format!("prime selected checkpoint: {error}"))?;
    let primed = env.primary_market_state().1.assets[0];
    if primed.effective_price != PRICE || primed.slot_last != 2 || primed.f_long_num != 0 {
        return Err("selected checkpoint was not stationary and unfunded".into());
    }

    env.warp_to_slot(3);
    env.push_auth_mark(1, 3, MARK)
        .map_err(|error| format!("stage unrelated epoch mark: {error}"))?;
    crank_asset_progress(&mut env, 2, 3, 1, 8)
        .map_err(|error| format!("advance unrelated epoch: {error}"))?;
    crank_asset_progress(&mut env, 3, 3, 1, 8)
        .map_err(|error| format!("refresh unrelated counterparty: {error}"))?;
    let before_omission = fingerprint(&env);
    let refresh = env.crank(
        0,
        3,
        if omit_selected_observation {
            Vec::new()
        } else {
            selected_observation.clone()
        },
    );
    let call_landed = refresh.is_ok();
    let (rejected_nonprogress, exact_rollback) = if omit_selected_observation {
        match refresh {
            Err(error) if error.contains("Custom(22)") => {
                let exact = fingerprint(&env) == before_omission;
                env.crank(0, 3, selected_observation)
                    .map_err(|error| format!("observed recovery after rejection: {error}"))?;
                (true, exact)
            }
            Err(error) => {
                return Err(format!(
                    "omitted selected observation returned an unexpected error: {error}"
                ))
            }
            Ok(_) => (false, false),
        }
    } else {
        refresh.map_err(|error| format!("selected funding refresh rejected: {error}"))?;
        (false, false)
    };
    let funded = env.primary_market_state().1.assets[0];

    env.crank(1, 3, Vec::new())
        .map_err(|error| format!("settle selected counterparty: {error}"))?;
    env.trade_no_cpi(0, 1, 0, -SIZE_Q, PRICE, 0)
        .map_err(|error| format!("close selected pair: {error}"))?;
    env.trade_no_cpi(2, 3, 1, -(POS_SCALE as i128), PRICE, 0)
        .map_err(|error| format!("close unrelated pair: {error}"))?;
    if env.primary_portfolio(0).pnl.get() > 0 {
        env.convert_released_pnl(0, u128::MAX)
            .map_err(|error| format!("convert selected funding PnL: {error}"))?;
    }
    let victim_capital = env.primary_portfolio(0).capital.get();
    let counterparty_capital = env.primary_portfolio(1).capital.get();
    env.withdraw_primary(0, victim_capital)
        .map_err(|error| format!("withdraw selected victim: {error}"))?;
    env.withdraw_primary(1, counterparty_capital)
        .map_err(|error| format!("withdraw selected counterparty: {error}"))?;
    if env.token_supply_observed() != supply_before {
        return Err("observation-omission world changed SPL supply".into());
    }
    Ok(ObservationOmissionWorld {
        call_landed,
        rejected_nonprogress,
        exact_rollback,
        f_long_num: funded.f_long_num,
        f_short_num: funded.f_short_num,
        victim_payout: u128::from(env.token_amount(env.actors[0].destination_token)),
        counterparty_payout: u128::from(env.token_amount(env.actors[1].destination_token)),
    })
}

pub fn discover_observation_omission_violation(
    mut seed: [u8; 32],
) -> Result<ObservationOmissionDiscovery, String> {
    seed[0] ^= 0x5f;
    let control = run_observation_omission_world(seed, false)?;
    let omitted = run_observation_omission_world(seed, true)?;
    let victim_payout_loss = control
        .victim_payout
        .checked_sub(omitted.victim_payout)
        .ok_or_else(|| "omitted observation increased victim payout".to_string())?;
    let counterparty_payout_gain = omitted
        .counterparty_payout
        .checked_sub(control.counterparty_payout)
        .ok_or_else(|| "omitted observation decreased counterparty payout".to_string())?;
    let control_total = control
        .victim_payout
        .checked_add(control.counterparty_payout)
        .ok_or_else(|| "control omission total overflow".to_string())?;
    let omitted_total = omitted
        .victim_payout
        .checked_add(omitted.counterparty_payout)
        .ok_or_else(|| "omitted observation total overflow".to_string())?;
    if control_total != omitted_total {
        return Err("observation-omission worlds did not conserve payouts".into());
    }
    Ok(ObservationOmissionDiscovery {
        omitted_landed: omitted.call_landed,
        omitted_rejected_nonprogress: omitted.rejected_nonprogress,
        omitted_exact_rollback: omitted.exact_rollback,
        control_f_long_num: control.f_long_num,
        control_f_short_num: control.f_short_num,
        omitted_f_long_num: omitted.f_long_num,
        omitted_f_short_num: omitted.f_short_num,
        victim_payout_loss,
        counterparty_payout_gain,
    })
}

pub fn discover_fractional_movement_stall(
    mut seed: [u8; 32],
) -> Result<FractionalMovementDiscovery, String> {
    const OPEN_PRICE: u64 = 100;
    const TARGET_PRICE: u64 = 1;
    const CAP_BPS: u64 = 24;
    const MAX_DT: u64 = 20;
    const DEPOSIT: u128 = 1_000_000;

    seed[0] ^= 0x6a;
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: OPEN_PRICE,
            max_price_move_bps_per_slot: CAP_BPS,
            max_accrual_dt_slots: MAX_DT,
            min_funding_lifetime_slots: MAX_DT,
            actor_deposits: [DEPOSIT, DEPOSIT, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.configure_permissionless_resolve(10_000, 1)
        .map_err(|error| format!("configure fractional stale resolution: {error}"))?;
    env.configure_auth_mark(false, 0, 1, OPEN_PRICE)
        .map_err(|error| format!("configure fractional authenticated mark: {error}"))?;
    env.trade_no_cpi(0, 1, 0, POS_SCALE as i128, OPEN_PRICE, 0)
        .map_err(|error| format!("open fractional-movement pair: {error}"))?;
    env.warp_to_slot(2);
    env.push_auth_mark(0, 2, TARGET_PRICE)
        .map_err(|error| format!("publish fractional target: {error}"))?;

    let mut slot = 2u64
        .checked_add(MAX_DT)
        .ok_or_else(|| "initial fractional crank slot overflow".to_string())?;
    let mut successful_cranks = 0u16;
    let mut nonmoving_stalls = 0u8;
    let mut rejected_stalls = 0u8;
    for _ in 0..200 {
        env.warp_to_slot(slot);
        let price_before = env.primary_market_state().1.assets[0].effective_price;
        let market_before = env.market_data(false);
        let long_before = env.primary_portfolio_data(0);
        let short_before = env.primary_portfolio_data(1);
        match env.crank(
            1,
            slot,
            vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: 0,
            }],
        ) {
            Ok(_) => {
                successful_cranks = successful_cranks
                    .checked_add(1)
                    .ok_or_else(|| "fractional successful-crank count overflow".to_string())?;
                let price_after = env.primary_market_state().1.assets[0].effective_price;
                if price_after == price_before {
                    nonmoving_stalls = nonmoving_stalls.saturating_add(1);
                } else {
                    nonmoving_stalls = 0;
                    rejected_stalls = 0;
                }
            }
            Err(_) => {
                if env.market_data(false) != market_before
                    || env.primary_portfolio_data(0) != long_before
                    || env.primary_portfolio_data(1) != short_before
                {
                    return Err("rejected fractional crank did not roll back".into());
                }
                rejected_stalls = rejected_stalls.saturating_add(1);
            }
        }
        let price_after = env.primary_market_state().1.assets[0].effective_price;
        if price_after == TARGET_PRICE {
            return Err("fractional movement reached target; no stall discovered".into());
        }
        if (nonmoving_stalls >= 3 || rejected_stalls >= 3) && price_after > TARGET_PRICE {
            break;
        }
        slot = slot
            .checked_add(MAX_DT)
            .ok_or_else(|| "fractional crank slot overflow".to_string())?;
    }
    let stalled_price = env.primary_market_state().1.assets[0].effective_price;
    if successful_cranks == 0
        || (nonmoving_stalls < 3 && rejected_stalls < 3)
        || stalled_price <= TARGET_PRICE
    {
        return Err(format!(
            "fractional target did not reach stable stall: price={stalled_price}, success={successful_cranks}, no-op={nonmoving_stalls}, reject={rejected_stalls}"
        ));
    }

    let resolve_slot = slot
        .checked_add(10_001)
        .ok_or_else(|| "fractional resolve slot overflow".to_string())?;
    env.resolve_stale_permissionless(resolve_slot)
        .map_err(|error| format!("resolve fractional-stall market: {error}"))?;
    env.warp_to_slot(
        resolve_slot
            .checked_add(1)
            .ok_or_else(|| "fractional close slot overflow".to_string())?,
    );
    let long_payout = drain_resolved_discovery_actor(&mut env, 0)?;
    let short_payout = drain_resolved_discovery_actor(&mut env, 1)?;
    let target_long_payout = DEPOSIT
        .checked_sub(u128::from(OPEN_PRICE - TARGET_PRICE))
        .ok_or_else(|| "target long payout underflow".to_string())?;
    let target_short_payout = DEPOSIT
        .checked_add(u128::from(OPEN_PRICE - TARGET_PRICE))
        .ok_or_else(|| "target short payout overflow".to_string())?;
    let long_overpayment = long_payout
        .checked_sub(target_long_payout)
        .ok_or_else(|| "fractional stall underpaid long".to_string())?;
    let short_underpayment = target_short_payout
        .checked_sub(short_payout)
        .ok_or_else(|| "fractional stall overpaid short".to_string())?;
    if long_payout
        .checked_add(short_payout)
        .ok_or_else(|| "fractional payout total overflow".to_string())?
        != DEPOSIT * 2
        || env.token_supply_observed() != supply_before
    {
        return Err("fractional stall did not conserve terminal payout/SPL supply".into());
    }
    Ok(FractionalMovementDiscovery {
        target_price: TARGET_PRICE,
        stalled_price,
        successful_cranks,
        rejected_stalls,
        nonmoving_stalls,
        long_overpayment,
        short_underpayment,
    })
}

#[derive(Clone, Copy, Debug)]
struct HybridTerminalWorld {
    resolve_landed: bool,
    terminal_mark: u64,
    victim_payout: u128,
    counterparty_payout: u128,
}

fn run_hybrid_terminal_world(
    mut seed: [u8; 32],
    ingest_current_reports: bool,
) -> Result<HybridTerminalWorld, String> {
    const VICTIM: usize = 0;
    const COUNTERPARTY: usize = 1;
    const ORACLE_AUTHORITY: usize = 2;
    const OPEN_PRICE: u64 = 100_000;
    const CAPITAL: u128 = 100_000_000;
    const SIZE_Q: i128 = 1_000 * POS_SCALE as i128;
    const FEEDS: [[u8; 32]; 3] = [[0xacu8; 32], [0xadu8; 32], [0u8; 32]];
    seed[0] ^= 0x61;
    seed[1] ^= u8::from(ingest_current_reports);
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: OPEN_PRICE,
            h_max: 20,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 20,
            min_funding_lifetime_slots: 20,
            actor_deposits: [CAPITAL, CAPITAL, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.set_clock(1, 160);
    let initial_leg_0 = env.set_pyth_price(&FEEDS[0], OPEN_PRICE as i64, -6, 0, 160);
    let initial_leg_1 = env.set_pyth_price(&FEEDS[1], 1_000_000, -6, 0, 100);
    env.configure_hybrid_oracle(0, 1, 160, 0, FEEDS, &[initial_leg_0, initial_leg_1], 3, 500)
        .map_err(|error| format!("configure terminal Hybrid oracle: {error}"))?;
    env.update_asset_authority_from_admin(
        0,
        percolator_prog::processor::ASSET_AUTH_ORACLE,
        ORACLE_AUTHORITY,
    )
    .map_err(|error| format!("separate terminal oracle authority: {error}"))?;
    env.trade_no_cpi(VICTIM, COUNTERPARTY, 0, SIZE_Q, OPEN_PRICE, 0)
        .map_err(|error| format!("open Hybrid terminal pair: {error}"))?;

    env.set_clock(100, 220);
    let current_leg_0 = env.set_pyth_price(&FEEDS[0], OPEN_PRICE as i64, -6, 0, 160);
    let current_leg_1 = env.set_pyth_price(&FEEDS[1], 1_100_000, -6, 0, 220);
    if ingest_current_reports {
        env.crank_with_oracles(
            ORACLE_AUTHORITY,
            100,
            vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: 2,
            }],
            &[current_leg_0, current_leg_1],
        )
        .map_err(|error| format!("ingest current Hybrid reports: {error}"))?;
        let authenticated_slot = env.current_slot();
        let mut accrual_ready = false;
        for step in 0..16 {
            if env.primary_market_state().1.assets[0].slot_last >= authenticated_slot {
                accrual_ready = true;
                break;
            }
            env.crank_with_oracles(
                ORACLE_AUTHORITY,
                authenticated_slot,
                vec![CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 2,
                }],
                &[current_leg_0, current_leg_1],
            )
            .map_err(|error| format!("current Hybrid terminal catch-up crank {step}: {error}"))?;
        }
        if !accrual_ready {
            return Err("current Hybrid terminal state did not catch up in bounded cranks".into());
        }
    }
    let before_resolve = fingerprint(&env);
    let resolve = env.resolve_market();
    let resolve_landed = resolve.is_ok();
    if !resolve_landed {
        if fingerprint(&env) != before_resolve {
            return Err("rejected stale Hybrid resolve did not roll back exactly".into());
        }
        return Ok(HybridTerminalWorld {
            resolve_landed,
            terminal_mark: env.primary_market_state().1.assets[0].effective_price,
            victim_payout: 0,
            counterparty_payout: 0,
        });
    }
    let terminal_mark = env.primary_market_state().1.assets[0].effective_price;
    let counterparty_payout = drain_resolved_discovery_actor(&mut env, COUNTERPARTY)?;
    let victim_payout = drain_resolved_discovery_actor(&mut env, VICTIM)?;
    if env.token_supply_observed() != supply_before {
        return Err("Hybrid terminal world changed SPL supply".into());
    }
    Ok(HybridTerminalWorld {
        resolve_landed,
        terminal_mark,
        victim_payout,
        counterparty_payout,
    })
}

pub fn discover_hybrid_terminal_snapshot_violation(
    seed: [u8; 32],
) -> Result<HybridTerminalSnapshotDiscovery, String> {
    let stale = run_hybrid_terminal_world(seed, false)?;
    let current = run_hybrid_terminal_world(seed, true)?;
    if !current.resolve_landed {
        return Err(format!(
            "current Hybrid terminal control did not resolve: stale={stale:?}, current={current:?}"
        ));
    }
    let victim_payout_loss = current
        .victim_payout
        .checked_sub(stale.victim_payout)
        .ok_or_else(|| "stale Hybrid resolve increased victim payout".to_string())?;
    let counterparty_payout_gain = stale
        .counterparty_payout
        .checked_sub(current.counterparty_payout)
        .ok_or_else(|| "stale Hybrid resolve decreased counterparty payout".to_string())?;
    let stale_total_payout = stale
        .victim_payout
        .checked_add(stale.counterparty_payout)
        .ok_or_else(|| "stale Hybrid payout overflow".to_string())?;
    let current_total_payout = current
        .victim_payout
        .checked_add(current.counterparty_payout)
        .ok_or_else(|| "current Hybrid payout overflow".to_string())?;
    Ok(HybridTerminalSnapshotDiscovery {
        stale_resolve_landed: stale.resolve_landed,
        stale_terminal_mark: stale.terminal_mark,
        current_terminal_mark: current.terminal_mark,
        victim_payout_loss,
        counterparty_payout_gain,
        stale_total_payout,
        current_total_payout,
    })
}

pub fn discover_composite_time_coherence_violation(
    mut seed: [u8; 32],
) -> Result<CompositeTimeCoherenceDiscovery, String> {
    const COHERENT_PRICE: u64 = 1_500_000;
    const INITIAL_A: i64 = 3_000_000;
    const INITIAL_B: i64 = 2_000_000;
    const FRESH_A: i64 = 6_000_000;
    const FRESH_B: i64 = 4_000_000;

    seed[0] ^= 0x7b;
    let coherent_initial = i128::from(INITIAL_A)
        .checked_mul(1_000_000)
        .and_then(|value| value.checked_div(i128::from(INITIAL_B)))
        .ok_or_else(|| "initial coherent cross-rate arithmetic failed".to_string())?;
    let coherent_fresh = i128::from(FRESH_A)
        .checked_mul(1_000_000)
        .and_then(|value| value.checked_div(i128::from(FRESH_B)))
        .ok_or_else(|| "fresh coherent cross-rate arithmetic failed".to_string())?;
    if coherent_initial != i128::from(COHERENT_PRICE)
        || coherent_fresh != i128::from(COHERENT_PRICE)
    {
        return Err("composite-time fixture changed coherent cross-rate".into());
    }

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: COHERENT_PRICE,
            h_max: 6_480_000,
            min_nonzero_mm_req: 599,
            min_nonzero_im_req: 600,
            maintenance_margin_bps: 500,
            initial_margin_bps: 500,
            liquidation_fee_bps: 5,
            liquidation_fee_cap: percolator::MAX_PROTOCOL_FEE_ABS,
            max_price_move_bps_per_slot: 24,
            max_accrual_dt_slots: 20,
            max_abs_funding_e9_per_slot: 1_000,
            min_funding_lifetime_slots: 10_000_000,
            actor_deposits: [540_000, 540_000, 1_000, USER_DEPOSIT, 1],
            ..MarketConfig::default()
        },
    );
    env.update_liquidation_fee_policy(5_000)
        .map_err(|error| format!("configure coherent cranker share: {error}"))?;
    env.set_clock(1, 100);
    let feeds = [[0xf1u8; 32], [0xf2u8; 32], [0u8; 32]];
    let initial_a = env.set_pyth_price(&feeds[0], INITIAL_A, -6, 0, 100);
    let initial_b = env.set_pyth_price(&feeds[1], INITIAL_B, -6, 0, 100);
    env.configure_hybrid_oracle(
        0,
        1,
        100,
        ORACLE_LEG_FLAG_DIVIDE_LEG2,
        feeds,
        &[initial_a, initial_b],
        1_000,
        0,
    )
    .map_err(|error| format!("configure coherent cross-rate: {error}"))?;
    if env.primary_market_state().0.oracle_target_price_e6 != COHERENT_PRICE {
        return Err("initial composite target differs from coherent cross-rate".into());
    }

    let size_q = (POS_SCALE as i128)
        .checked_mul(35)
        .and_then(|value| value.checked_div(100))
        .ok_or_else(|| "composite-time victim size overflow".to_string())?;
    env.trade_no_cpi(1, 0, 0, size_q, COHERENT_PRICE, 0)
        .map_err(|error| format!("open coherent-price victim short: {error}"))?;
    let victim_capital_before = env.primary_portfolio(0).capital.get();
    let oi_before = env.primary_market_state().1.assets[0].oi_eff_short_q;
    let cranker_capital_before = env.primary_portfolio(2).capital.get();
    let supply_before = env.token_supply_observed();

    let fresh_a = env.set_pyth_price(&feeds[0], FRESH_A, -6, 0, 101);
    let skewed = [fresh_a, initial_b];
    let observations = || {
        vec![CrankObservationHint {
            asset_index: 0,
            oracle_accounts: 2,
        }]
    };
    let mut slot = 1u64;
    for _ in 0..12 {
        slot = slot
            .checked_add(20)
            .ok_or_else(|| "composite-time catch-up slot overflow".to_string())?;
        env.set_clock(slot, 101);
        env.crank_with_oracles(2, slot, observations(), &skewed)
            .map_err(|error| format!("skewed composite catch-up at slot {slot}: {error}"))?;
    }
    env.crank_with_oracles(0, slot, observations(), &skewed)
        .map_err(|error| format!("refresh skewed-price victim: {error}"))?;
    let victim_cert = env
        .primary_portfolio(0)
        .health_cert
        .try_to_runtime()
        .map_err(|error| format!("decode skewed victim certificate: {error:?}"))?;
    if victim_cert.certified_liq_deficit == 0 {
        return Err("skewed composite did not certify liquidation".into());
    }
    env.crank_with_reward(2, 0, slot, observations(), &skewed)
        .map_err(|error| format!("skewed-price liquidation: {error}"))?;
    let (wrapper_after, group_after) = env.primary_market_state();
    let victim_capital_loss = victim_capital_before
        .checked_sub(env.primary_portfolio(0).capital.get())
        .ok_or_else(|| "skewed liquidation increased victim capital".to_string())?;
    let oi_reduction_q = oi_before
        .checked_sub(group_after.assets[0].oi_eff_short_q)
        .ok_or_else(|| "skewed liquidation increased short OI".to_string())?;
    let cranker_reward = env
        .primary_portfolio(2)
        .capital
        .get()
        .checked_sub(cranker_capital_before)
        .ok_or_else(|| "skewed liquidation reduced cranker capital".to_string())?;
    env.withdraw_primary(2, cranker_reward)
        .map_err(|error| format!("withdraw skewed liquidation reward: {error}"))?;
    let extracted_tokens = env.token_amount(env.actors[2].destination_token);
    if env.token_supply_observed() != supply_before {
        return Err("composite-time world changed SPL supply".into());
    }
    Ok(CompositeTimeCoherenceDiscovery {
        coherent_price: COHERENT_PRICE,
        skewed_target: wrapper_after.oracle_target_price_e6,
        skewed_mark: group_after.assets[0].effective_price,
        victim_capital_loss,
        oi_reduction_q,
        cranker_reward,
        extracted_tokens,
    })
}

#[derive(Clone, Copy, Debug)]
struct TerminalDustWorld {
    low_price: u64,
    victim_payout: u128,
    coalition_payout: u128,
    vault_remaining: u128,
    supply: u128,
}

fn run_terminal_dust_world(
    seed: [u8; 32],
    route: ProspectiveAccrualRoute,
    include_dust_round_trip: bool,
) -> Result<TerminalDustWorld, String> {
    const BASIS: u64 = 10_000_000;
    const DIRECTIONAL_Q: i128 = 1_000 * POS_SCALE as i128;
    const DUST_Q: i128 = 1;
    const DIRECTIONAL_DEPOSIT: u128 = 20_000_000_000;
    const DUST_DEPOSIT: u128 = 1_000;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: BASIS,
            max_trading_fee_bps: 10_000,
            max_price_move_bps_per_slot: 5_000,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [
                DIRECTIONAL_DEPOSIT,
                DIRECTIONAL_DEPOSIT,
                DUST_DEPOSIT,
                DUST_DEPOSIT,
                1,
            ],
            actor_token_balances: [25_000_000_000, 25_000_000_000, 1_000_000, 1_000_000, 1],
            ..MarketConfig::default()
        },
    );
    env.configure_ewma_mark(0, 1, BASIS, 1, 0)
        .map_err(|error| format!("{route:?} configure terminal EWMA: {error}"))?;
    env.trade_no_cpi(0, 1, 0, DIRECTIONAL_Q, BASIS, 0)
        .map_err(|error| format!("{route:?} open terminal directional OI: {error}"))?;
    let observations = || {
        vec![CrankObservationHint {
            asset_index: 0,
            oracle_accounts: 0,
        }]
    };
    for slot in 2..=5 {
        env.warp_to_slot(slot);
        env.push_ewma_mark(0, slot, 1)
            .map_err(|error| format!("{route:?} publish terminal low mark: {error}"))?;
        for actor in [0, 1] {
            env.crank(actor, slot, observations())
                .map_err(|error| format!("{route:?} accrue terminal actor {actor}: {error}"))?;
        }
    }
    let low_price = env.primary_market_state().1.assets[0].effective_price;
    if low_price >= BASIS / 5 {
        return Err(format!("{route:?} terminal setup did not reach low mark"));
    }
    if include_dust_round_trip {
        execute_reported_price_route(&mut env, route, 2, 3, DUST_Q, low_price)
            .map_err(|error| format!("{route:?} open dust position: {error}"))?;
    }

    env.warp_to_slot(6);
    for actor in [0, 1] {
        env.crank(actor, 6, observations())
            .map_err(|error| format!("{route:?} advance terminal actor: {error}"))?;
    }
    let rebound_input = BASIS
        .checked_mul(2)
        .and_then(|value| value.checked_sub(low_price))
        .ok_or_else(|| "terminal rebound input overflow".to_string())?;
    env.push_ewma_mark(0, 6, rebound_input)
        .map_err(|error| format!("{route:?} publish terminal rebound: {error}"))?;
    if env.primary_profile(0).mark_ewma_e6 != BASIS {
        return Err(format!("{route:?} terminal rebound missed basis"));
    }

    env.warp_to_slot(7);
    let mut slot = 7u64;
    loop {
        for actor in [0, 1] {
            env.crank(actor, slot, observations())
                .map_err(|error| format!("{route:?} converge terminal actor: {error}"))?;
        }
        if env.primary_market_state().1.assets[0].effective_price == BASIS {
            break;
        }
        slot = slot
            .checked_add(1)
            .ok_or_else(|| "terminal convergence slot overflow".to_string())?;
        if slot >= 24 {
            return Err(format!("{route:?} terminal rebound did not converge"));
        }
        env.warp_to_slot(slot);
    }
    if include_dust_round_trip {
        execute_reported_price_route(&mut env, route, 2, 3, -DUST_Q, BASIS)
            .map_err(|error| format!("{route:?} close dust position: {error}"))?;
    }
    env.trade_no_cpi(0, 1, 0, -DIRECTIONAL_Q, BASIS, 0)
        .map_err(|error| format!("{route:?} close terminal directional OI: {error}"))?;
    env.resolve_market()
        .map_err(|error| format!("{route:?} resolve terminal-dust world: {error}"))?;
    for actor in 0..PRIMARY_ACTOR_COUNT {
        env.close_resolved_primary(actor)
            .map_err(|error| format!("{route:?} initial resolved close actor {actor}: {error}"))?;
    }
    for _ in 0..16 {
        for actor in 0..PRIMARY_ACTOR_COUNT {
            let _ = env.close_resolved_primary(actor);
            let _ = env.claim_resolved_payout_topup_primary(actor);
        }
    }
    let market_before = env.market_data(false);
    let destinations_before: Vec<_> = (0..PRIMARY_ACTOR_COUNT)
        .map(|actor| env.token_amount(env.actors[actor].destination_token))
        .collect();
    for actor in 0..PRIMARY_ACTOR_COUNT {
        let _ = env.close_resolved_primary(actor);
        let _ = env.claim_resolved_payout_topup_primary(actor);
    }
    let destinations_after: Vec<_> = (0..PRIMARY_ACTOR_COUNT)
        .map(|actor| env.token_amount(env.actors[actor].destination_token))
        .collect();
    if env.market_data(false) != market_before || destinations_after != destinations_before {
        return Err(format!("{route:?} terminal payout was not quiescent"));
    }

    let victim_payout = u128::from(env.token_amount(env.actors[0].destination_token));
    let coalition_payout = [1usize, 2, 3].into_iter().try_fold(0u128, |sum, actor| {
        sum.checked_add(u128::from(
            env.token_amount(env.actors[actor].destination_token),
        ))
        .ok_or_else(|| "terminal-dust coalition payout overflow".to_string())
    })?;
    let vault_remaining = env.primary_market_state().1.vault;
    if vault_remaining != u128::from(env.token_amount(env.vault)) {
        return Err("terminal-dust engine/SPL vault mismatch".into());
    }
    Ok(TerminalDustWorld {
        low_price,
        victim_payout,
        coalition_payout,
        vault_remaining,
        supply: env.token_supply_observed(),
    })
}

fn discover_one_terminal_dust_violation(
    mut seed: [u8; 32],
    route: ProspectiveAccrualRoute,
) -> Result<TerminalDustDiscovery, String> {
    const COALITION_DEPOSITS: u128 = 20_000_002_000;
    const VICTIM_DEPOSIT: u128 = 20_000_000_000;
    seed[0] ^= 0x8c;
    seed[1] ^= route.discriminator();
    let control = run_terminal_dust_world(seed, route, false)?;
    let dust = run_terminal_dust_world(seed, route, true)?;
    if control.low_price != dust.low_price
        || control.victim_payout != VICTIM_DEPOSIT
        || control.coalition_payout != COALITION_DEPOSITS
    {
        return Err(format!(
            "{route:?} terminal control world did not fully reconcile"
        ));
    }
    let attacker_loss = COALITION_DEPOSITS
        .checked_sub(dust.coalition_payout)
        .ok_or_else(|| "dust round trip increased coalition payout".to_string())?;
    let victim_loss = VICTIM_DEPOSIT
        .checked_sub(dust.victim_payout)
        .ok_or_else(|| "dust round trip increased victim payout".to_string())?;
    Ok(TerminalDustDiscovery {
        route,
        attacker_loss,
        victim_loss,
        vault_remaining: dust.vault_remaining,
        control_vault_remaining: control.vault_remaining,
        control_supply: control.supply,
        dust_supply: dust.supply,
    })
}

pub fn discover_terminal_dust_violations(
    seed: [u8; 32],
) -> Result<Vec<TerminalDustDiscovery>, String> {
    ProspectiveAccrualRoute::ALL
        .into_iter()
        .map(|route| discover_one_terminal_dust_violation(seed, route))
        .collect()
}

pub fn discover_cross_domain_insurance_violation(
    mut seed: [u8; 32],
) -> Result<CrossDomainInsuranceDiscovery, String> {
    const MARK: u64 = 100;
    const COALITION_DEPOSIT: u128 = 20_200;
    const INSURANCE_TOP_UP: u128 = 100_000;

    seed[0] ^= 0x9d;
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: MARK,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            maintenance_fee_per_slot: 200,
            actor_deposits: [200, 10_000, 10_000, 1, 1],
            actor_token_balances: [1_000, 20_000, 20_000, 10, 10],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.top_up_insurance_domain(1, INSURANCE_TOP_UP)
        .map_err(|error| format!("fund unrelated insurance domain: {error}"))?;
    env.trade_no_cpi(0, 2, 0, POS_SCALE as i128, MARK, 0)
        .map_err(|error| format!("open surviving asset-0 leg: {error}"))?;
    env.trade_no_cpi(0, 1, 1, -(POS_SCALE as i128), MARK, 0)
        .map_err(|error| format!("open loss-bearing asset-1 leg: {error}"))?;

    env.warp_to_slot(2);
    for asset_index in [0u16, 1] {
        env.crank(
            3,
            2,
            vec![CrankObservationHint {
                asset_index,
                oracle_accounts: 0,
            }],
        )
        .map_err(|error| format!("accrue asset {asset_index}: {error}"))?;
    }
    env.sync_maintenance_fee(0, 2)
        .map_err(|error| format!("exhaust debtor capital: {error}"))?;
    if env.primary_portfolio(0).capital.get() != 0 {
        return Err("maintenance setup did not exhaust debtor capital".into());
    }

    let mut mark = MARK;
    for slot in 3..=12 {
        mark = mark
            .checked_mul(2)
            .ok_or_else(|| "cross-domain adverse mark overflow".to_string())?;
        env.warp_to_slot(slot);
        env.push_auth_mark(1, slot, mark)
            .map_err(|error| format!("publish asset-1 mark at slot {slot}: {error}"))?;
        env.crank(
            3,
            slot,
            vec![CrankObservationHint {
                asset_index: 1,
                oracle_accounts: 0,
            }],
        )
        .map_err(|error| format!("advance asset-1 mark at slot {slot}: {error}"))?;
    }
    env.rebalance_reduce(0, 1, POS_SCALE)
        .map_err(|error| format!("flatten loss-bearing asset-1 leg: {error}"))?;
    if env.primary_portfolio(0).pnl.get() >= 0
        || env.primary_market_state().1.assets[0].stored_pos_count_long == 0
    {
        return Err(
            "cross-domain setup did not retain debt plus surviving asset-0 exposure".into(),
        );
    }

    let spent_before = env.primary_market_state().1.insurance_domain_spent[1];
    let mut progress_calls = 0u16;
    for _ in 0..512 {
        let account = env.primary_portfolio(0);
        if account.pnl.get() >= 0
            && env.primary_market_state().1.assets[0].stored_pos_count_long == 0
        {
            break;
        }
        match env.crank(0, 12, Vec::new()) {
            Ok(_) => {
                progress_calls = progress_calls
                    .checked_add(1)
                    .ok_or_else(|| "cross-domain progress count overflow".to_string())?;
            }
            Err(error) if error.contains("Custom(23)") => break,
            Err(error) => return Err(format!("cross-domain liquidation failed: {error}")),
        }
    }
    let unrelated_insurance_spent = env.primary_market_state().1.insurance_domain_spent[1]
        .checked_sub(spent_before)
        .ok_or_else(|| "unrelated insurance spent counter decreased".to_string())?;

    env.resolve_market()
        .map_err(|error| format!("resolve cross-domain world: {error}"))?;
    let loser_payout = drain_resolved_discovery_actor(&mut env, 0)?;
    let counterparty_payout = drain_resolved_discovery_actor(&mut env, 2)?;
    let winner_payout = drain_resolved_discovery_actor(&mut env, 1)?;
    let coalition_payout = loser_payout
        .checked_add(counterparty_payout)
        .and_then(|value| value.checked_add(winner_payout))
        .ok_or_else(|| "cross-domain coalition payout overflow".to_string())?;
    let coalition_profit = coalition_payout
        .checked_sub(COALITION_DEPOSIT)
        .ok_or_else(|| "cross-domain coalition did not recover deposits".to_string())?;
    if env.token_supply_observed() != supply_before {
        return Err("cross-domain insurance world changed SPL supply".into());
    }
    Ok(CrossDomainInsuranceDiscovery {
        unrelated_insurance_spent,
        coalition_payout,
        coalition_profit,
        progress_calls,
    })
}

pub fn discover_cross_domain_backing_violation(
    mut seed: [u8; 32],
) -> Result<CrossDomainBackingDiscovery, String> {
    const INITIAL_PRICE: u64 = 100;
    const MOVED_PRICE: u64 = 105;
    const SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const CLAIM_PER_ASSET: u128 = 100;
    const UNFUNDED_SOURCE_DOMAIN: usize = 1;
    const FUNDED_SOURCE_DOMAIN: usize = 3;
    const DEPOSIT: u128 = 1_000;

    seed[0] ^= 0xae;
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: INITIAL_PRICE,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [DEPOSIT; PRIMARY_ACTOR_COUNT],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    let provider_source_before = env.token_amount(env.provider_source_token);
    env.top_up_backing_bucket(FUNDED_SOURCE_DOMAIN as u16, 2 * CLAIM_PER_ASSET, 10)
        .map_err(|error| format!("fund higher source domain: {error}"))?;
    if provider_source_before.checked_sub(env.token_amount(env.provider_source_token))
        != Some((2 * CLAIM_PER_ASSET) as u64)
    {
        return Err("provider top-up did not debit exact principal".into());
    }

    for asset in [0u16, 1] {
        env.trade_no_cpi(0, 1, asset, SIZE_Q, INITIAL_PRICE, 0)
            .map_err(|error| format!("open winner asset {asset}: {error}"))?;
    }
    env.warp_to_slot(2);
    for asset in [0u16, 1] {
        env.push_auth_mark(asset, 2, MOVED_PRICE)
            .map_err(|error| format!("move asset {asset}: {error}"))?;
        env.crank(
            0,
            2,
            vec![CrankObservationHint {
                asset_index: asset,
                oracle_accounts: env.primary_profile(asset as usize).oracle_leg_count,
            }],
        )
        .map_err(|error| format!("refresh winner asset {asset}: {error}"))?;
    }
    let before = env.primary_market_state().1;
    let unfunded_claim_before_num =
        before.source_credit[UNFUNDED_SOURCE_DOMAIN].positive_claim_bound_num;
    let funded_claim_before_num =
        before.source_credit[FUNDED_SOURCE_DOMAIN].positive_claim_bound_num;
    if env.primary_portfolio(0).pnl.get() != (2 * CLAIM_PER_ASSET) as i128
        || unfunded_claim_before_num != CLAIM_PER_ASSET * percolator::BOUND_SCALE
        || funded_claim_before_num != CLAIM_PER_ASSET * percolator::BOUND_SCALE
        || before.source_credit[UNFUNDED_SOURCE_DOMAIN].fresh_reserved_backing_num != 0
        || before.source_credit[FUNDED_SOURCE_DOMAIN].fresh_reserved_backing_num
            != 2 * CLAIM_PER_ASSET * percolator::BOUND_SCALE
    {
        return Err(
            "source-domain fixture did not create one unfunded and one funded claim".into(),
        );
    }

    for asset in [0u16, 1] {
        env.trade_no_cpi(0, 2, asset, -SIZE_Q, MOVED_PRICE, 0)
            .map_err(|error| format!("flatten winner asset {asset}: {error}"))?;
    }
    env.convert_released_pnl(0, CLAIM_PER_ASSET)
        .map_err(|error| format!("first aggregate conversion: {error}"))?;
    let after_first = env.primary_market_state().1;
    if after_first.source_credit[UNFUNDED_SOURCE_DOMAIN].positive_claim_bound_num != 0
        || after_first.source_credit[FUNDED_SOURCE_DOMAIN].positive_claim_bound_num
            != CLAIM_PER_ASSET * percolator::BOUND_SCALE
        || after_first.source_backing_buckets[FUNDED_SOURCE_DOMAIN].consumed_liened_backing_num
            != CLAIM_PER_ASSET * percolator::BOUND_SCALE
    {
        return Err(
            "first conversion did not expose source claim/backing desynchronization".into(),
        );
    }

    env.trade_no_cpi(0, 2, 0, POS_SCALE as i128, MOVED_PRICE, 0)
        .map_err(|error| format!("refresh-certificate open: {error}"))?;
    env.trade_no_cpi(0, 2, 0, -(POS_SCALE as i128), MOVED_PRICE, 0)
        .map_err(|error| format!("refresh-certificate close: {error}"))?;
    env.convert_released_pnl(0, CLAIM_PER_ASSET)
        .map_err(|error| format!("second aggregate conversion: {error}"))?;
    let after_second = env.primary_market_state().1;
    let funded_backing_consumed_num =
        after_second.source_backing_buckets[FUNDED_SOURCE_DOMAIN].consumed_liened_backing_num;
    let winner_capital_gain = env
        .primary_portfolio(0)
        .capital
        .get()
        .checked_sub(DEPOSIT)
        .ok_or_else(|| "winner did not gain capital".to_string())?;
    if env.primary_portfolio(0).pnl.get() != 0 {
        return Err("winner retained PnL after second conversion".into());
    }
    let winner_capital = env.primary_portfolio(0).capital.get();
    env.withdraw_primary(0, winner_capital)
        .map_err(|error| format!("withdraw cross-domain winner: {error}"))?;
    let extracted_tokens = env.token_amount(env.actors[0].destination_token);
    if env.token_supply_observed() != supply_before {
        return Err("cross-domain backing world changed SPL supply".into());
    }
    Ok(CrossDomainBackingDiscovery {
        unfunded_claim_before_num,
        funded_claim_before_num,
        funded_backing_consumed_num,
        winner_capital_gain,
        extracted_tokens,
    })
}

fn discover_one_resolved_adl_close_lock(
    mut seed: [u8; 32],
    order: ResolvedAdlCloseOrder,
) -> Result<ResolvedAdlCloseDiscovery, String> {
    const WINNER: usize = 0;
    const LOSER: usize = 1;
    const OPEN_MARK: u64 = 100;
    const BANKRUPTCY_MARK: u64 = 500;
    const WINNER_DEPOSIT: u128 = 1_000;
    const LOSER_DEPOSIT: u128 = 900;
    const TRADE_SIZE_Q: i128 = (2 * POS_SCALE) as i128;
    const ATTEMPTS: u8 = 8;

    seed[0] ^= 0x61;
    seed[1] ^= order.discriminator();
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: OPEN_MARK,
            maintenance_margin_bps: 10_000,
            initial_margin_bps: 10_000,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [WINNER_DEPOSIT, LOSER_DEPOSIT, 0, 0, 0],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.trade_no_cpi(WINNER, LOSER, 0, TRADE_SIZE_Q, OPEN_MARK, 0)
        .map_err(|error| format!("open resolved-ADL pair: {error}"))?;
    env.warp_to_slot(6);
    env.push_auth_mark(0, 6, BANKRUPTCY_MARK)
        .map_err(|error| format!("publish resolved-ADL bankruptcy mark: {error}"))?;
    crank_asset_progress(&mut env, LOSER, 6, 0, 16)
        .map_err(|error| format!("resolved-ADL loser progress: {error}"))?;
    crank_asset_progress(&mut env, WINNER, 6, 0, 16)
        .map_err(|error| format!("resolved-ADL winner progress: {error}"))?;

    let (_, after_adl) = env.primary_market_state();
    let winner_before_resolve = env.primary_portfolio(WINNER);
    let winner_leg = winner_before_resolve
        .legs
        .iter()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .find(|leg| leg.active && leg.asset_index == 0)
        .ok_or_else(|| "resolved-ADL winner has no active leg".to_string())?;
    let winner_basis_q = winner_leg.basis_pos_q.unsigned_abs();
    let effective_long_oi_q = after_adl.assets[0].oi_eff_long_q;
    if after_adl.assets[0].a_long >= ADL_ONE
        || winner_basis_q <= effective_long_oi_q
        || effective_long_oi_q == 0
    {
        return Err(format!(
            "resolved-ADL setup did not create a scaled winner: a_long={}, basis={winner_basis_q}, oi={effective_long_oi_q}",
            after_adl.assets[0].a_long
        ));
    }

    env.resolve_market()
        .map_err(|error| format!("resolve ADL market: {error}"))?;
    let winner_at_resolve = env.primary_portfolio(WINNER);
    let winner_funded_value = winner_at_resolve
        .capital
        .get()
        .checked_add(winner_at_resolve.pnl.get().max(0) as u128)
        .ok_or_else(|| "resolved-ADL winner value overflow".to_string())?;
    let winner_destination = env.actors[WINNER].destination_token;
    let winner_destination_before = env.token_amount(winner_destination);

    let mut winner_close_failures = 0u8;
    let mut all_counter_underflow = true;
    let mut exact_rollback = true;
    let mut loser_close_landed = false;
    if order == ResolvedAdlCloseOrder::LoserThenWinner {
        loser_close_landed = env.close_resolved_primary_signed(LOSER).is_ok();
    }
    for attempt in 0..ATTEMPTS {
        let before = fingerprint(&env);
        match env.close_resolved_primary_signed(WINNER) {
            Ok(_) => break,
            Err(error) => {
                winner_close_failures = winner_close_failures.saturating_add(1);
                all_counter_underflow &= error.contains("Custom(25)");
                exact_rollback &= fingerprint(&env) == before;
            }
        }
        if attempt == 0 && order == ResolvedAdlCloseOrder::WinnerThenLoser {
            loser_close_landed = env.close_resolved_primary_signed(LOSER).is_ok();
        }
    }
    let canonical_vault_liquidity = u128::from(env.token_amount(env.vault));
    if env.primary_market_state().1.vault != canonical_vault_liquidity {
        return Err("resolved-ADL internal vault diverged from SPL custody".into());
    }

    let before_withdraw = fingerprint(&env);
    let withdraw_rejected = env.withdraw_primary(WINNER, 1).is_err();
    exact_rollback &= fingerprint(&env) == before_withdraw;
    let before_portfolio_close = fingerprint(&env);
    let portfolio_close_rejected = env.close_primary_portfolio(WINNER).is_err();
    exact_rollback &= fingerprint(&env) == before_portfolio_close;
    if env.token_supply_observed() != supply_before {
        return Err("resolved-ADL close attempts changed SPL supply".into());
    }
    let winner_external_payout = env
        .token_amount(winner_destination)
        .checked_sub(winner_destination_before)
        .ok_or_else(|| "resolved-ADL winner destination decreased".to_string())?;

    Ok(ResolvedAdlCloseDiscovery {
        order,
        winner_basis_q,
        effective_long_oi_q,
        winner_funded_value,
        canonical_vault_liquidity,
        loser_close_landed,
        winner_close_failures,
        all_counter_underflow,
        exact_rollback,
        withdraw_rejected,
        portfolio_close_rejected,
        winner_external_payout,
    })
}

pub fn discover_resolved_adl_close_locks(
    seed: [u8; 32],
) -> Result<Vec<ResolvedAdlCloseDiscovery>, String> {
    ResolvedAdlCloseOrder::ALL
        .into_iter()
        .map(|order| discover_one_resolved_adl_close_lock(seed, order))
        .collect()
}

fn discover_one_stale_cohort_novation(
    mut seed: [u8; 32],
    route: StaleCohortRoute,
) -> Result<StaleCohortNovationDiscovery, String> {
    const WINNER: usize = 0;
    const LOSER: usize = 1;
    const ENTRANT: usize = 2;
    const OPEN_PRICE: u64 = 100;
    const LOSS_PRICE: u64 = 500;
    const SIZE_Q: i128 = (10 * POS_SCALE) as i128;
    const WINNER_DEPOSIT: u128 = 5_000;
    const LOSER_DEPOSIT: u128 = 1_000;
    const ENTRANT_DEPOSIT: u128 = 1_000_000;
    const WINNER_PROFIT: u128 = 4_000;

    seed[0] ^= 0x39;
    seed[1] ^= route.discriminator();
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: OPEN_PRICE,
            maintenance_margin_bps: 10_000,
            initial_margin_bps: 10_000,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [WINNER_DEPOSIT, LOSER_DEPOSIT, ENTRANT_DEPOSIT, 0, 0],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.top_up_backing_bucket(1, 10_000, 100)
        .map_err(|error| format!("fund stale-cohort source backing: {error}"))?;
    env.trade_no_cpi(WINNER, LOSER, 0, SIZE_Q, OPEN_PRICE, 0)
        .map_err(|error| format!("open stale-cohort exposure: {error}"))?;
    let observation = vec![CrankObservationHint {
        asset_index: 0,
        oracle_accounts: 0,
    }];
    for slot in 2..=4 {
        env.warp_to_slot(slot);
        env.push_auth_mark(0, slot, LOSS_PRICE)
            .map_err(|error| format!("publish stale-cohort loss mark at {slot}: {error}"))?;
        env.crank(WINNER, slot, observation.clone())
            .map_err(|error| format!("refresh stale-cohort winner at {slot}: {error}"))?;
    }
    let winner_before_conversion = env.primary_portfolio(WINNER);
    if winner_before_conversion.pnl.get() != WINNER_PROFIT as i128
        || env.primary_portfolio(LOSER).pnl.get() != 0
    {
        return Err(format!(
            "stale-cohort fixture lost its asymmetric settlement: winner_pnl={}, loser_pnl={}",
            winner_before_conversion.pnl.get(),
            env.primary_portfolio(LOSER).pnl.get()
        ));
    }
    let (_, pre_novation_group) = env.primary_market_state();
    let pre_stale_long_count = pre_novation_group.assets[0].stale_account_count_long;
    let pre_stale_short_count = pre_novation_group.assets[0].stale_account_count_short;
    let pre_negative_pnl_count = pre_novation_group.negative_pnl_account_count;
    let trade_market_id = pre_novation_group.assets[0].market_id;
    let novation_landed = match route {
        StaleCohortRoute::NoCpi => env
            .trade_no_cpi(WINNER, ENTRANT, 0, -SIZE_Q, LOSS_PRICE, 0)
            .is_ok(),
        StaleCohortRoute::BatchNoCpi => env
            .batch_trade_no_cpi(
                WINNER,
                ENTRANT,
                vec![BatchTradeLeg {
                    asset_index: 0,
                    market_id: trade_market_id,
                    size_q: -SIZE_Q,
                    exec_price: LOSS_PRICE,
                    fee_bps: 0,
                }],
            )
            .is_ok(),
        StaleCohortRoute::Cpi => env.trade_cpi(WINNER, ENTRANT, 0, -SIZE_Q, 0, 0).is_ok(),
        StaleCohortRoute::BatchCpi => env
            .batch_trade_cpi(
                WINNER,
                ENTRANT,
                vec![BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: trade_market_id,
                    size_q: -SIZE_Q,
                    fee_bps: 0,
                    limit_price: 0,
                }],
            )
            .is_ok(),
    };
    if !novation_landed {
        return Ok(StaleCohortNovationDiscovery {
            route,
            novation_landed,
            pre_stale_long_count,
            pre_stale_short_count,
            pre_negative_pnl_count,
            settlement_cranks: 0,
            winner_extracted: 0,
            entrant_extracted: 0,
            winner_profit: 0,
            entrant_principal_loss: 0,
            loser_principal_loss: 0,
            all_positions_terminal: false,
            token_supply_conserved: env.token_supply_observed() == supply_before,
        });
    }
    if discovery_position(&env.primary_portfolio(WINNER), 0)? != 0
        || discovery_position(&env.primary_portfolio(ENTRANT), 0)? != SIZE_Q
    {
        return Err("unsigned stale-cohort transfer produced the wrong exposure".into());
    }

    let mut settlement_cranks = 0u8;
    for _ in 0..64 {
        let mut round_progressed = false;
        for actor in [LOSER, ENTRANT, WINNER] {
            if env.crank(actor, 4, observation.clone()).is_ok() {
                settlement_cranks = settlement_cranks.saturating_add(1);
                round_progressed = true;
            }
        }
        let (_, group) = env.primary_market_state();
        if group.assets[0].stale_account_count_long == 0
            && group.assets[0].stale_account_count_short == 0
            && group.negative_pnl_account_count == 0
            && group.b_stale_account_count == 0
            && group.pending_domain_loss_barriers.iter().all(|&v| v == 0)
        {
            break;
        }
        if !round_progressed {
            return Err("stale-cohort settlement reached a nonprogressing public state".into());
        }
    }

    if discovery_position(&env.primary_portfolio(ENTRANT), 0)? != 0 {
        env.forfeit_recovery_leg(ENTRANT, 0, 1)
            .map_err(|error| format!("detach stale-cohort entrant recovery leg: {error}"))?;
    }
    let _ = env.finalize_reset_side(0, 0);
    for actor in [ENTRANT, WINNER] {
        for _ in 0..4 {
            let _ = env.crank(actor, 4, observation.clone());
        }
    }

    let terminal_winner_pnl = env.primary_portfolio(WINNER).pnl.get();
    if terminal_winner_pnl != WINNER_PROFIT as i128 {
        return Err(format!(
            "stale-cohort winner claim changed before resolution: {terminal_winner_pnl}"
        ));
    }
    let all_positions_terminal = [WINNER, LOSER, ENTRANT]
        .into_iter()
        .map(|actor| discovery_position(&env.primary_portfolio(actor), 0))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .all(|position| position == 0);
    if !all_positions_terminal {
        return Err("stale-cohort world retained exposure before terminal resolution".into());
    }
    env.resolve_market()
        .map_err(|error| format!("resolve stale-cohort market: {error}"))?;
    for _ in 0..24 {
        for actor in [LOSER, ENTRANT, WINNER] {
            env.close_resolved_primary_signed(actor)
                .map_err(|error| format!("close stale-cohort actor {actor}: {error}"))?;
        }
    }
    let winner_extracted = env.token_amount(env.actors[WINNER].destination_token);
    let entrant_extracted = env.token_amount(env.actors[ENTRANT].destination_token);
    let loser_extracted = env.token_amount(env.actors[LOSER].destination_token);
    let winner_profit = u128::from(winner_extracted)
        .checked_sub(WINNER_DEPOSIT)
        .ok_or_else(|| "stale-cohort winner did not profit".to_string())?;
    let entrant_principal_loss = ENTRANT_DEPOSIT
        .checked_sub(u128::from(entrant_extracted))
        .ok_or_else(|| "stale-cohort entrant gained capital".to_string())?;
    let loser_principal_loss = LOSER_DEPOSIT
        .checked_sub(u128::from(loser_extracted))
        .ok_or_else(|| "stale-cohort loser gained capital".to_string())?;

    Ok(StaleCohortNovationDiscovery {
        route,
        novation_landed,
        pre_stale_long_count,
        pre_stale_short_count,
        pre_negative_pnl_count,
        settlement_cranks,
        winner_extracted,
        entrant_extracted,
        winner_profit,
        entrant_principal_loss,
        loser_principal_loss,
        all_positions_terminal,
        token_supply_conserved: env.token_supply_observed() == supply_before,
    })
}

pub fn discover_stale_cohort_novations(
    seed: [u8; 32],
) -> Result<Vec<StaleCohortNovationDiscovery>, String> {
    StaleCohortRoute::ALL
        .into_iter()
        .map(|route| discover_one_stale_cohort_novation(seed, route))
        .collect()
}

fn discovery_source_claim(
    account: &percolator_prog::state::PortfolioAccountV16,
    domain: usize,
) -> u128 {
    account
        .source_domains
        .iter()
        .find(|source| {
            source.source_claim_market_id.get() != 0 && source.domain.get() as usize == domain
        })
        .map(|source| source.source_claim_bound_num.get())
        .unwrap_or(0)
}

fn discovery_position(
    account: &percolator_prog::state::PortfolioAccountV16,
    asset_index: u16,
) -> Result<i128, String> {
    account.legs.iter().try_fold(0i128, |sum, leg| {
        let decoded = leg
            .try_to_runtime()
            .map_err(|error| format!("decode portfolio leg: {error:?}"))?;
        if decoded.active && decoded.asset_index == u32::from(asset_index) {
            sum.checked_add(decoded.basis_pos_q)
                .ok_or_else(|| "portfolio position overflow".to_string())
        } else {
            Ok(sum)
        }
    })
}

fn discovery_total_abs_position(
    account: &percolator_prog::state::PortfolioAccountV16,
) -> Result<u128, String> {
    account.legs.iter().try_fold(0u128, |sum, leg| {
        let decoded = leg
            .try_to_runtime()
            .map_err(|error| format!("decode portfolio leg: {error:?}"))?;
        if decoded.active {
            sum.checked_add(decoded.basis_pos_q.unsigned_abs())
                .ok_or_else(|| "portfolio absolute-position sum overflow".to_string())
        } else {
            Ok(sum)
        }
    })
}

fn discovery_counterparty_lien_for_domain(
    account: &percolator_prog::state::PortfolioAccountV16,
    domain: u16,
) -> u128 {
    account
        .source_domains
        .iter()
        .find(|source| {
            source.source_claim_market_id.get() != 0 && source.domain.get() == u32::from(domain)
        })
        .map(|source| source.source_lien_counterparty_backing_num.get())
        .unwrap_or(0)
}

fn crank_asset_progress(
    env: &mut V16Svm,
    actor: usize,
    slot: u64,
    asset_index: u16,
    attempts: usize,
) -> Result<(), String> {
    let oracle_accounts = env.primary_profile(asset_index as usize).oracle_leg_count;
    let mut progressed = false;
    for _ in 0..attempts {
        match env.crank(
            actor,
            slot,
            vec![CrankObservationHint {
                asset_index,
                oracle_accounts,
            }],
        ) {
            Ok(_) => progressed = true,
            Err(error) if progressed && error.contains("Custom(22)") => break,
            Err(error) => {
                return Err(format!(
                    "actor {actor} asset {asset_index} failed before progress: {error}"
                ))
            }
        }
    }
    if !progressed {
        return Err(format!(
            "actor {actor} asset {asset_index} made no progress"
        ));
    }
    Ok(())
}

fn build_full_refresh_discovery_world(
    seed: [u8; 32],
    route: DiscoveryTradeRoute,
    leg_order: ActiveLegOrder,
) -> Result<(V16Svm, u128, u128), String> {
    const ADVERSE_PRICE: u64 = 1_000_000;
    const ADVERSE_TARGET: u64 = 997_600;
    const RESCUE_PRICE: u64 = 100;
    const RESCUE_MARK: u64 = 99;
    const ADVERSE_SIZE_Q: i128 = 50 * POS_SCALE as i128;
    const RESCUE_SIZE_Q: i128 = 100_000 * POS_SCALE as i128;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            h_max: 6_480_000,
            min_nonzero_mm_req: 599,
            min_nonzero_im_req: 600,
            maintenance_margin_bps: 500,
            initial_margin_bps: 500,
            liquidation_fee_bps: 5,
            liquidation_fee_cap: percolator::MAX_PROTOCOL_FEE_ABS,
            max_price_move_bps_per_slot: 24,
            max_accrual_dt_slots: 1,
            max_abs_funding_e9_per_slot: 10_000,
            min_funding_lifetime_slots: 1,
            actor_deposits: [
                3_115_000,
                50_000_000,
                USER_DEPOSIT,
                USER_DEPOSIT,
                EXIT_MAKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    env.configure_auth_mark(false, 0, 1, RESCUE_PRICE)
        .map_err(|error| format!("configure rescue mark: {error}"))?;
    env.configure_auth_mark(false, 1, 1, ADVERSE_PRICE)
        .map_err(|error| format!("configure adverse mark: {error}"))?;
    env.top_up_backing_bucket(1, 200_000, 10)
        .map_err(|error| format!("top up rescue backing: {error}"))?;
    let ordered_legs = match leg_order {
        ActiveLegOrder::RescueFirst => [
            (0, RESCUE_SIZE_Q, RESCUE_PRICE, "rescue"),
            (1, ADVERSE_SIZE_Q, ADVERSE_PRICE, "adverse"),
        ],
        ActiveLegOrder::RescueLast => [
            (1, ADVERSE_SIZE_Q, ADVERSE_PRICE, "adverse"),
            (0, RESCUE_SIZE_Q, RESCUE_PRICE, "rescue"),
        ],
    };
    for (asset_index, size_q, price, label) in ordered_legs {
        execute_discovery_trade_route(&mut env, route, 0, 1, asset_index, size_q, price)
            .map_err(|error| format!("open {label} leg via {route:?}: {error}"))?;
    }

    let opened = env.primary_portfolio(0);
    let active_assets = opened
        .legs
        .iter()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .filter(|leg| leg.active)
        .map(|leg| leg.asset_index)
        .collect::<Vec<_>>();
    let expected_assets = match leg_order {
        ActiveLegOrder::RescueFirst => [0, 1],
        ActiveLegOrder::RescueLast => [1, 0],
    };
    if active_assets != expected_assets {
        return Err(format!(
            "full-refresh fixture lost {leg_order:?} ordering via {route:?}: {active_assets:?}"
        ));
    }
    let position_before_q = discovery_total_abs_position(&opened)?;

    env.warp_to_slot(2);
    env.push_auth_mark(0, 2, RESCUE_MARK)
        .map_err(|error| format!("stage rounded rescue mark: {error}"))?;
    crank_asset_progress(&mut env, 2, 2, 0, 4)?;
    let primed = env.primary_market_state().1;
    if primed.assets[0].effective_price != RESCUE_PRICE
        || primed.assets[0].slot_last != 2
        || primed.assets[0].f_long_num != 0
    {
        return Err("rescue leg did not reach the rounded zero-funding state".into());
    }

    env.warp_to_slot(3);
    env.push_auth_mark(1, 3, ADVERSE_TARGET)
        .map_err(|error| format!("stage adverse mark: {error}"))?;
    crank_asset_progress(&mut env, 2, 3, 1, 4)?;
    let stale_group = env.primary_market_state().1;
    if stale_group.assets[0].effective_price != RESCUE_PRICE || stale_group.assets[0].slot_last != 2
    {
        return Err("adverse prefix consumed the later rescue observation".into());
    }
    Ok((env, position_before_q, stale_group.insurance))
}

pub fn discover_full_refresh_omission_violation(
    seed: [u8; 32],
) -> Result<FullRefreshDiscovery, String> {
    discover_active_leg_currentness_violation(
        seed,
        DiscoveryTradeRoute::NoCpi,
        ActiveLegOrder::RescueLast,
    )
}

pub fn discover_active_leg_currentness_violation(
    mut seed: [u8; 32],
    route: DiscoveryTradeRoute,
    leg_order: ActiveLegOrder,
) -> Result<FullRefreshDiscovery, String> {
    seed[0] ^= 0x22;
    seed[1] ^= route.discriminator();
    seed[2] ^= leg_order.discriminator();
    let (mut omitted, _, omitted_insurance_before) =
        build_full_refresh_discovery_world(seed, route, leg_order)?;
    let omitted_position_before_q = discovery_total_abs_position(&omitted.primary_portfolio(0))?;
    let first_before = fingerprint(&omitted);
    let first_result = omitted.crank(0, 3, Vec::new());
    let (omitted_rejected_nonprogress, omitted_exact_rollback) = match first_result {
        Err(error) if error.contains("Custom(22)") => (true, fingerprint(&omitted) == first_before),
        Err(error) => {
            return Err(format!(
                "omitted-observation first crank returned an unexpected error: {error}"
            ))
        }
        Ok(_) => {
            let second_before = fingerprint(&omitted);
            match omitted.crank(0, 3, Vec::new()) {
                Err(error) if error.contains("Custom(22)") => {
                    (true, fingerprint(&omitted) == second_before)
                }
                Err(error) => {
                    return Err(format!(
                        "omitted-observation liquidation returned an unexpected error: {error}"
                    ))
                }
                Ok(_) => (false, false),
            }
        }
    };
    let omitted_account = omitted.primary_portfolio(0);
    let omitted_cert = omitted_account
        .health_cert
        .try_to_runtime()
        .map_err(|error| format!("decode omitted certificate: {error:?}"))?;
    let omitted_position_after_q = discovery_total_abs_position(&omitted_account)?;
    let omitted_insurance_delta = omitted
        .primary_market_state()
        .1
        .insurance
        .checked_sub(omitted_insurance_before)
        .ok_or_else(|| "omitted path decreased insurance".to_string())?;

    let (mut complete, complete_position_before_q, complete_insurance_before) =
        build_full_refresh_discovery_world(seed, route, leg_order)?;
    let rescue_oracle_accounts = complete.primary_profile(0).oracle_leg_count;
    complete
        .crank(
            2,
            3,
            vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: rescue_oracle_accounts,
            }],
        )
        .map_err(|error| format!("complete-world rescue observation: {error}"))?;
    if complete.primary_market_state().1.assets[0].f_long_num == 0 {
        return Err("complete-world rescue observation booked no funding".into());
    }
    let complete_oracle_account_counts = [
        complete.primary_profile(0).oracle_leg_count,
        complete.primary_profile(1).oracle_leg_count,
    ];
    let complete_observations = || {
        vec![
            CrankObservationHint {
                asset_index: 0,
                oracle_accounts: complete_oracle_account_counts[0],
            },
            CrankObservationHint {
                asset_index: 1,
                oracle_accounts: complete_oracle_account_counts[1],
            },
        ]
    };
    complete
        .crank(1, 3, complete_observations())
        .map_err(|error| format!("complete-world counterparty refresh: {error}"))?;
    complete
        .crank(0, 3, complete_observations())
        .map_err(|error| format!("complete-world user refresh: {error}"))?;
    let complete_account = complete.primary_portfolio(0);
    let complete_position_after_q = discovery_total_abs_position(&complete_account)?;
    let complete_cert = complete_account
        .health_cert
        .try_to_runtime()
        .map_err(|error| format!("decode complete certificate: {error:?}"))?;
    let complete_insurance_delta = complete
        .primary_market_state()
        .1
        .insurance
        .checked_sub(complete_insurance_before)
        .ok_or_else(|| "complete path decreased insurance".to_string())?;

    if complete_position_before_q != omitted_position_before_q {
        return Err("paired full-refresh fixtures opened different positions".into());
    }
    Ok(FullRefreshDiscovery {
        omitted_rejected_nonprogress,
        omitted_exact_rollback,
        omitted_position_before_q,
        omitted_position_after_q,
        omitted_liq_deficit: omitted_cert.certified_liq_deficit,
        omitted_insurance_delta,
        complete_position_before_q,
        complete_position_after_q,
        complete_liq_deficit: complete_cert.certified_liq_deficit,
        complete_insurance_delta,
    })
}

pub fn discover_backing_expiry_violation(
    mut seed: [u8; 32],
    case: BackingExpiryCase,
) -> Result<BackingExpiryDiscovery, String> {
    const PRICE: u64 = 100;
    const DOMAIN: u16 = 1;
    const BUCKET_AMOUNT: u128 = 100_000;
    const OPEN_Q: i128 = 1_000 * POS_SCALE as i128;

    seed[0] ^= 0x67;
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 5_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [
                52_501,
                1_000_000,
                USER_DEPOSIT,
                USER_DEPOSIT,
                EXIT_MAKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    let fee_bps = case.fee_bps.clamp(1, 10_000);
    let expiry_offset = u64::from(case.expiry_offset.clamp(2, 8));
    let mark_move_bps = u64::from(case.mark_move_bps.clamp(100, 1_000));
    let increase_divisor = i128::from(case.increase_divisor.clamp(10, 100));
    let increase_q = OPEN_Q
        .checked_div(increase_divisor)
        .ok_or_else(|| "backing-expiry increase divisor is zero".to_string())?;

    env.configure_auth_mark(false, 0, env.current_slot(), PRICE)
        .map_err(|error| format!("configure backing-expiry mark: {error}"))?;
    env.update_backing_fee_policy(DOMAIN, fee_bps, 0)
        .map_err(|error| format!("configure backing fee policy: {error}"))?;
    let expiry_slot = env
        .current_slot()
        .checked_add(expiry_offset)
        .ok_or_else(|| "backing expiry slot overflow".to_string())?;
    env.top_up_backing_bucket(DOMAIN, BUCKET_AMOUNT, expiry_slot)
        .map_err(|error| format!("top up backing bucket: {error}"))?;
    env.trade_no_cpi(0, 1, 0, OPEN_Q, PRICE, 0)
        .map_err(|error| format!("open source-backed position: {error}"))?;

    let mark_slot = env
        .current_slot()
        .checked_add(1)
        .ok_or_else(|| "backing mark slot overflow".to_string())?;
    env.warp_to_slot(mark_slot);
    let winning_mark = PRICE
        .checked_mul(
            10_000u64
                .checked_add(mark_move_bps)
                .ok_or_else(|| "backing mark bps overflow".to_string())?,
        )
        .and_then(|value| value.checked_div(10_000))
        .ok_or_else(|| "backing winning mark overflow".to_string())?;
    env.push_auth_mark(0, mark_slot, winning_mark)
        .map_err(|error| format!("push backing winning mark: {error}"))?;
    crank_asset_progress(&mut env, 1, mark_slot, 0, 4)?;
    crank_asset_progress(&mut env, 0, mark_slot, 0, 4)?;

    let before_group = env.primary_market_state().1;
    let before_bucket = before_group.source_backing_buckets[DOMAIN as usize];
    if before_bucket.status != BackingBucketStatusV16::Fresh
        || before_bucket.expiry_slot != expiry_slot
    {
        return Err("backing was not fresh before retained trade construction".into());
    }
    let capital_before = env.primary_portfolio(0).capital.get();
    let provider_before = env.token_amount(env.provider_destination_token);
    let supply_before = env.token_supply_observed();
    let retained = env.build_retained_no_cpi_trade(0, 1, 0, increase_q, winning_mark);

    let authenticated_slot = expiry_slot
        .checked_add(1)
        .ok_or_else(|| "post-expiry landing slot overflow".to_string())?;
    env.warp_to_slot(authenticated_slot);
    let before_rejection = fingerprint(&env);
    let retained_result = env.land_retained(retained);
    let risk_increase_rejected_stale = matches!(
        &retained_result,
        Err(error) if error.contains("Custom(19)")
    );
    if let Err(error) = &retained_result {
        if !risk_increase_rejected_stale {
            return Err(format!(
                "retained post-expiry trade returned an unexpected error: {error}"
            ));
        }
    }
    let rejected_exact_rollback = retained_result.is_err() && fingerprint(&env) == before_rejection;

    let after_group = env.primary_market_state().1;
    let after_bucket = after_group.source_backing_buckets[DOMAIN as usize];
    let provider_earnings = after_bucket
        .utilization_fee_earnings
        .checked_sub(before_bucket.utilization_fee_earnings)
        .ok_or_else(|| "post-expiry provider earnings decreased".to_string())?;
    let victim_capital_loss = capital_before
        .checked_sub(env.primary_portfolio(0).capital.get())
        .ok_or_else(|| "post-expiry trade increased victim capital".to_string())?;
    if provider_earnings != 0 {
        env.withdraw_backing_bucket_earnings(DOMAIN, provider_earnings)
            .map_err(|error| format!("withdraw unexpected post-expiry earnings: {error}"))?;
    }
    let extracted_tokens = env
        .token_amount(env.provider_destination_token)
        .checked_sub(provider_before)
        .ok_or_else(|| "provider destination token balance decreased".to_string())?;
    let position_before_reduction_q =
        discovery_position(&env.primary_portfolio(0), 0)?.unsigned_abs();
    let risk_reduction_landed = env
        .trade_no_cpi(0, 1, 0, -increase_q, winning_mark, 0)
        .is_ok();
    let position_after_reduction_q =
        discovery_position(&env.primary_portfolio(0), 0)?.unsigned_abs();
    Ok(BackingExpiryDiscovery {
        expiry_slot,
        authenticated_slot,
        engine_slot: after_group.current_slot,
        risk_increase_rejected_stale,
        rejected_exact_rollback,
        victim_capital_loss,
        provider_earnings,
        extracted_tokens,
        risk_reduction_landed,
        position_before_reduction_q,
        position_after_reduction_q,
        token_supply_conserved: env.token_supply_observed() == supply_before,
    })
}

pub fn discover_backing_expiry_trade_route_boundary(
    mut seed: [u8; 32],
    route: DiscoveryTradeRoute,
    expiry_offset: u8,
    landing: BackingExpiryLanding,
) -> Result<ExpiredBackingTradeRouteDiscovery, String> {
    const WINNER: usize = 0;
    const LOSER: usize = 1;
    const ASSET: u16 = 0;
    const DOMAIN: u16 = 1;
    const PRICE: u64 = 100;
    const WINNING_MARK: u64 = 105;
    const OPEN_Q: i128 = 1_000 * POS_SCALE as i128;
    const INCREASE_Q: i128 = 50 * POS_SCALE as i128;
    const BUCKET_AMOUNT: u128 = 100_000;

    seed[0] ^= 0x6f;
    seed[1] ^= route.discriminator();
    seed[2] ^= expiry_offset;
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 5_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [
                52_501,
                1_000_000,
                USER_DEPOSIT,
                USER_DEPOSIT,
                EXIT_MAKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    let start_slot = env.current_slot();
    let expiry_slot = start_slot
        .checked_add(u64::from(expiry_offset.clamp(2, 6)))
        .ok_or_else(|| "trade-route expiry overflow".to_string())?;
    let authenticated_slot = landing.authenticated_slot(expiry_slot)?;
    let supply_before = env.token_supply_observed();
    env.configure_auth_mark(false, ASSET, start_slot, PRICE)
        .map_err(|error| format!("configure trade-route mark: {error}"))?;
    // Both batch routes intentionally reject active backing-fee policy. They still need the
    // freshness invariant because a zero-fee fill can create a counterparty-backed lien.
    if matches!(route, DiscoveryTradeRoute::NoCpi | DiscoveryTradeRoute::Cpi) {
        env.update_backing_fee_policy(DOMAIN, 5_000, 0)
            .map_err(|error| format!("configure trade-route backing fee: {error}"))?;
    }
    if route == DiscoveryTradeRoute::Cpi {
        env.set_matcher_backing_fee_cap(LOSER, 5_000)
            .map_err(|error| format!("authorize CPI trade-route backing fee: {error}"))?;
    }
    env.top_up_backing_bucket(DOMAIN, BUCKET_AMOUNT, expiry_slot)
        .map_err(|error| format!("fund trade-route backing: {error}"))?;
    env.trade_no_cpi(WINNER, LOSER, ASSET, OPEN_Q, PRICE, 0)
        .map_err(|error| format!("open trade-route position: {error}"))?;

    let mark_slot = start_slot
        .checked_add(1)
        .ok_or_else(|| "trade-route mark slot overflow".to_string())?;
    env.warp_to_slot(mark_slot);
    env.push_auth_mark(ASSET, mark_slot, WINNING_MARK)
        .map_err(|error| format!("publish trade-route winning mark: {error}"))?;
    crank_asset_progress(&mut env, LOSER, mark_slot, ASSET, 4)?;
    crank_asset_progress(&mut env, WINNER, mark_slot, ASSET, 4)?;

    let before_group = env.primary_market_state().1;
    let before_bucket = before_group.source_backing_buckets[DOMAIN as usize];
    if before_bucket.status != BackingBucketStatusV16::Fresh
        || before_bucket.expiry_slot != expiry_slot
        || before_group.current_slot > authenticated_slot
    {
        return Err("trade-route backing was not cached-fresh before retention".into());
    }
    let lien_before =
        discovery_counterparty_lien_for_domain(&env.primary_portfolio(WINNER), DOMAIN)
            .checked_add(discovery_counterparty_lien_for_domain(
                &env.primary_portfolio(LOSER),
                DOMAIN,
            ))
            .ok_or_else(|| "trade-route before-lien overflow".to_string())?;
    let capital_before = env.primary_portfolio(WINNER).capital.get();
    let provider_before = env.token_amount(env.provider_destination_token);
    let retained = build_retained_discovery_trade(
        &mut env,
        route,
        WINNER,
        LOSER,
        ASSET,
        INCREASE_Q,
        WINNING_MARK,
    );
    let before_retained = fingerprint(&env);

    env.warp_to_slot(authenticated_slot);
    let retained_result = env.land_retained(retained);
    let risk_increase_landed = retained_result.is_ok();
    let rejected_exact_rollback = retained_result.is_err() && fingerprint(&env) == before_retained;
    let after_group = env.primary_market_state().1;
    let engine_slot = after_group.current_slot;
    let lien_after = discovery_counterparty_lien_for_domain(&env.primary_portfolio(WINNER), DOMAIN)
        .checked_add(discovery_counterparty_lien_for_domain(
            &env.primary_portfolio(LOSER),
            DOMAIN,
        ))
        .ok_or_else(|| "trade-route after-lien overflow".to_string())?;
    let counterparty_lien_increase_num = lien_after
        .checked_sub(lien_before)
        .ok_or_else(|| "trade-route counterparty lien decreased".to_string())?;
    let victim_capital_loss = capital_before
        .checked_sub(env.primary_portfolio(WINNER).capital.get())
        .unwrap_or(0);
    let provider_earnings = after_group.source_backing_buckets[DOMAIN as usize]
        .utilization_fee_earnings
        .checked_sub(before_bucket.utilization_fee_earnings)
        .ok_or_else(|| "trade-route provider earnings decreased".to_string())?;
    if provider_earnings != 0 {
        env.withdraw_backing_bucket_earnings(DOMAIN, provider_earnings)
            .map_err(|error| format!("withdraw trade-route backing fee: {error}"))?;
    }
    let extracted_tokens = env
        .token_amount(env.provider_destination_token)
        .checked_sub(provider_before)
        .ok_or_else(|| "trade-route provider destination decreased".to_string())?;

    let position_before_reduction_q =
        discovery_position(&env.primary_portfolio(WINNER), ASSET)?.unsigned_abs();
    let risk_reduction_landed = execute_discovery_trade_route(
        &mut env,
        route,
        WINNER,
        LOSER,
        ASSET,
        -INCREASE_Q,
        WINNING_MARK,
    )
    .is_ok();
    let position_after_reduction_q =
        discovery_position(&env.primary_portfolio(WINNER), ASSET)?.unsigned_abs();

    Ok(ExpiredBackingTradeRouteDiscovery {
        route,
        landing,
        expiry_slot,
        authenticated_slot,
        engine_slot,
        risk_increase_landed,
        rejected_exact_rollback,
        counterparty_lien_increase_num,
        victim_capital_loss,
        provider_earnings,
        extracted_tokens,
        risk_reduction_landed,
        position_before_reduction_q,
        position_after_reduction_q,
        token_supply_conserved: env.token_supply_observed() == supply_before,
    })
}

pub fn discover_backing_expiry_trade_route_boundaries(
    seed: [u8; 32],
    expiry_offset: u8,
) -> Result<Vec<ExpiredBackingTradeRouteDiscovery>, String> {
    BackingExpiryLanding::ALL
        .into_iter()
        .flat_map(|landing| {
            DiscoveryTradeRoute::ALL
                .into_iter()
                .map(move |route| (landing, route))
        })
        .map(|(landing, route)| {
            discover_backing_expiry_trade_route_boundary(seed, route, expiry_offset, landing)
        })
        .collect()
}

#[derive(Clone, Copy, Debug)]
struct RetainedMaturityWorld {
    retained_landed: bool,
    retained_rejected_expired: bool,
    retained_exact_rollback: bool,
    users_terminal: bool,
    funded_value: u128,
    vault_liquidity: u128,
    close_failures: u16,
    progress_failures: u16,
    exact_rollback: bool,
    landing_provider_source_debit: u128,
    landing_vault_token_credit: u128,
    landing_internal_vault_credit: u128,
    landing_bucket_principal_credit_num: u128,
    provider_principal_consumed: u128,
    provider_recovery: u128,
    external_payout: u128,
    token_supply_conserved: bool,
}

fn discovery_portfolio_is_terminal(
    account: &percolator_prog::state::PortfolioAccountV16,
) -> Result<bool, String> {
    let has_active_leg = account.legs.iter().try_fold(false, |active, leg| {
        leg.try_to_runtime()
            .map(|decoded| active || decoded.active)
            .map_err(|error| format!("decode terminal portfolio leg: {error:?}"))
    })?;
    Ok(account.capital.get() == 0
        && account.pnl.get() == 0
        && !has_active_leg
        && account
            .source_domains
            .iter()
            .all(|source| source.source_claim_bound_num.get() == 0))
}

fn discovery_portfolio_funded_value(
    account: &percolator_prog::state::PortfolioAccountV16,
) -> Result<u128, String> {
    Ok(account.capital.get())
}

fn run_retained_maturity_world(
    mut seed: [u8; 32],
    kind: RetainedMaturityKind,
    expiry_offset: u8,
    submit_retained: bool,
    landing: BackingExpiryLanding,
) -> Result<(u64, u64, RetainedMaturityWorld), String> {
    const WINNER: usize = 0;
    const LOSER: usize = 1;
    const PROVIDER: usize = 2;
    const PUBLISHER: usize = 3;
    const ASSET: u16 = 1;
    const WINNING_DOMAIN: u16 = ASSET * 2 + 1;
    const OPEN_PRICE: u64 = 100;
    const SETTLED_SLOT: u64 = 3;
    const BACKING: u128 = 500;
    const SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const CONTROL_ATTEMPTS: usize = 16;

    seed[0] ^= 0x63;
    seed[1] ^= kind.discriminator();
    seed[2] ^= expiry_offset;
    let expiry_slot = SETTLED_SLOT
        .checked_add(u64::from(expiry_offset.clamp(2, 6)))
        .ok_or_else(|| "retained maturity expiry overflow".to_string())?;
    let landing_slot = landing.authenticated_slot(expiry_slot)?;
    let resolve_slot = landing_slot
        .checked_add(6)
        .ok_or_else(|| "retained maturity resolve overflow".to_string())?;
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: OPEN_PRICE,
            maintenance_margin_bps: 10_000,
            initial_margin_bps: 10_000,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [1_000, 1_000, 0, 0, 0],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.update_asset_authority_from_admin(
        ASSET,
        percolator_prog::processor::ASSET_AUTH_BACKING_BUCKET,
        PROVIDER,
    )
    .map_err(|error| format!("install retained-maturity backing provider: {error}"))?;
    env.configure_permissionless_resolve(
        resolve_slot
            .checked_sub(SETTLED_SLOT)
            .ok_or_else(|| "retained maturity stale-window underflow".to_string())?,
        1,
    )
    .map_err(|error| format!("configure retained-maturity resolution: {error}"))?;
    env.trade_no_cpi(WINNER, LOSER, ASSET, SIZE_Q, OPEN_PRICE, 0)
        .map_err(|error| format!("open retained-maturity position pair: {error}"))?;

    let oracle_accounts = env.primary_profile(ASSET as usize).oracle_leg_count;
    let observations = || {
        vec![CrankObservationHint {
            asset_index: ASSET,
            oracle_accounts,
        }]
    };
    for (slot, mark) in [(2, 200), (SETTLED_SLOT, 350)] {
        env.warp_to_slot(slot);
        env.push_auth_mark(ASSET, slot, mark)
            .map_err(|error| format!("publish retained-maturity mark {mark}: {error}"))?;
        env.crank(PUBLISHER, slot, observations())
            .map_err(|error| format!("commit retained-maturity mark {mark}: {error}"))?;
    }
    if env.primary_market_state().1.assets[ASSET as usize].effective_price != 350 {
        return Err("retained-maturity setup did not commit the adverse mark".into());
    }

    let provider_source_before = env.token_amount(env.actors[PROVIDER].source_token);
    let vault_token_before = env.token_amount(env.vault);
    let internal_vault_before = env.primary_market_state().1.vault;
    let bucket_principal_before = env.primary_market_state().1.source_backing_buckets
        [WINNING_DOMAIN as usize]
        .fresh_unliened_backing_num;
    let retained = match kind {
        RetainedMaturityKind::BackingTopUp => env.build_retained_backing_bucket_top_up_for_actor(
            PROVIDER,
            WINNING_DOMAIN,
            BACKING,
            expiry_slot,
        ),
    };
    env.warp_to_slot(landing_slot);
    let (retained_landed, retained_rejected_expired, retained_exact_rollback) = if submit_retained {
        let before = fingerprint(&env);
        match env.land_retained(retained) {
            Ok(_) => (true, false, false),
            Err(error) if error.contains("Custom(9)") => (false, true, fingerprint(&env) == before),
            Err(error) => {
                return Err(format!(
                    "retained maturity submission returned an unexpected error: {error}"
                ))
            }
        }
    } else {
        (false, false, true)
    };
    if env.current_slot() != landing_slot {
        return Err("retained maturity did not reach the authenticated landing slot".into());
    }
    let landing_provider_source_debit = provider_source_before
        .checked_sub(env.token_amount(env.actors[PROVIDER].source_token))
        .ok_or_else(|| "retained maturity provider source increased at landing".to_string())?;
    let landing_vault_token_credit = env
        .token_amount(env.vault)
        .checked_sub(vault_token_before)
        .ok_or_else(|| "retained maturity vault token balance decreased at landing".to_string())?;
    let landing_group = env.primary_market_state().1;
    let landing_internal_vault_credit = landing_group
        .vault
        .checked_sub(internal_vault_before)
        .ok_or_else(|| "retained maturity internal vault decreased at landing".to_string())?;
    let landing_bucket_principal_credit_num = landing_group.source_backing_buckets
        [WINNING_DOMAIN as usize]
        .fresh_unliened_backing_num
        .checked_sub(bucket_principal_before)
        .ok_or_else(|| "retained maturity bucket principal decreased at landing".to_string())?;

    env.resolve_stale_permissionless(resolve_slot)
        .map_err(|error| format!("resolve retained-maturity world: {error}"))?;
    env.warp_to_slot(resolve_slot + 1);
    let mut close_failures = 0u16;
    let mut progress_failures = 0u16;
    let mut exact_rollback = true;
    for _ in 0..CONTROL_ATTEMPTS {
        for actor in [LOSER, WINNER] {
            if discovery_portfolio_is_terminal(&env.primary_portfolio(actor))? {
                continue;
            }
            let before = fingerprint(&env);
            if env.close_resolved_primary_signed(actor).is_err() {
                close_failures = close_failures.saturating_add(1);
                exact_rollback &= fingerprint(&env) == before;
            }
            if discovery_portfolio_is_terminal(&env.primary_portfolio(actor))? {
                continue;
            }
            let before = fingerprint(&env);
            if env.crank(actor, resolve_slot + 1, observations()).is_err() {
                progress_failures = progress_failures.saturating_add(1);
                exact_rollback &= fingerprint(&env) == before;
            }
        }
        if discovery_portfolio_is_terminal(&env.primary_portfolio(WINNER))?
            && discovery_portfolio_is_terminal(&env.primary_portfolio(LOSER))?
        {
            break;
        }
    }

    let users_terminal = discovery_portfolio_is_terminal(&env.primary_portfolio(WINNER))?
        && discovery_portfolio_is_terminal(&env.primary_portfolio(LOSER))?;
    let funded_value = discovery_portfolio_funded_value(&env.primary_portfolio(WINNER))?
        .checked_add(discovery_portfolio_funded_value(
            &env.primary_portfolio(LOSER),
        )?)
        .ok_or_else(|| "retained maturity funded value overflow".to_string())?;
    let vault_liquidity = u128::from(env.token_amount(env.vault));
    if env.primary_market_state().1.vault != vault_liquidity {
        return Err("retained-maturity internal vault diverged from SPL custody".into());
    }
    let provider_principal_consumed = provider_source_before
        .checked_sub(env.token_amount(env.actors[PROVIDER].source_token))
        .ok_or_else(|| "retained-maturity provider source increased".to_string())?;
    let provider_recovery = u128::from(env.token_amount(env.actors[PROVIDER].destination_token));
    let external_payout = [WINNER, LOSER].into_iter().try_fold(0u128, |sum, actor| {
        sum.checked_add(u128::from(
            env.token_amount(env.actors[actor].destination_token),
        ))
        .ok_or_else(|| "retained maturity payout overflow".to_string())
    })?;

    Ok((
        expiry_slot,
        landing_slot,
        RetainedMaturityWorld {
            retained_landed,
            retained_rejected_expired,
            retained_exact_rollback,
            users_terminal,
            funded_value,
            vault_liquidity,
            close_failures,
            progress_failures,
            exact_rollback,
            landing_provider_source_debit: u128::from(landing_provider_source_debit),
            landing_vault_token_credit: u128::from(landing_vault_token_credit),
            landing_internal_vault_credit,
            landing_bucket_principal_credit_num,
            provider_principal_consumed: u128::from(provider_principal_consumed),
            provider_recovery,
            external_payout,
            token_supply_conserved: env.token_supply_observed() == supply_before,
        },
    ))
}

pub fn discover_retained_maturity_terminal_locks(
    seed: [u8; 32],
    expiry_offset: u8,
) -> Result<Vec<RetainedMaturityDiscovery>, String> {
    discover_retained_maturity_boundary(seed, expiry_offset, BackingExpiryLanding::After)
}

pub fn discover_retained_maturity_boundary(
    seed: [u8; 32],
    expiry_offset: u8,
    landing: BackingExpiryLanding,
) -> Result<Vec<RetainedMaturityDiscovery>, String> {
    RetainedMaturityKind::ALL
        .into_iter()
        .map(|kind| {
            let (_, _, control) =
                run_retained_maturity_world(seed, kind, expiry_offset, false, landing)?;
            let (expiry_slot, landing_slot, delayed) =
                run_retained_maturity_world(seed, kind, expiry_offset, true, landing)?;
            Ok(RetainedMaturityDiscovery {
                kind,
                landing,
                expiry_slot,
                landing_slot,
                retained_landed: delayed.retained_landed,
                retained_rejected_expired: delayed.retained_rejected_expired,
                retained_exact_rollback: delayed.retained_exact_rollback,
                control_users_terminal: control.users_terminal,
                delayed_users_terminal: delayed.users_terminal,
                delayed_funded_value: delayed.funded_value,
                delayed_vault_liquidity: delayed.vault_liquidity,
                delayed_close_failures: delayed.close_failures,
                delayed_progress_failures: delayed.progress_failures,
                exact_rollback: delayed.exact_rollback,
                landing_provider_source_debit: delayed.landing_provider_source_debit,
                landing_vault_token_credit: delayed.landing_vault_token_credit,
                landing_internal_vault_credit: delayed.landing_internal_vault_credit,
                landing_bucket_principal_credit_num: delayed.landing_bucket_principal_credit_num,
                provider_principal_consumed: delayed.provider_principal_consumed,
                provider_recovery: delayed.provider_recovery,
                control_external_payout: control.external_payout,
                delayed_external_payout: delayed.external_payout,
                token_supply_conserved: control.token_supply_conserved
                    && delayed.token_supply_conserved,
            })
        })
        .collect()
}

pub fn discover_retained_maturity_boundaries(
    seed: [u8; 32],
    expiry_offset: u8,
) -> Result<Vec<RetainedMaturityDiscovery>, String> {
    BackingExpiryLanding::ALL
        .into_iter()
        .map(|landing| discover_retained_maturity_boundary(seed, expiry_offset, landing))
        .collect::<Result<Vec<_>, _>>()
        .map(|discoveries| discoveries.into_iter().flatten().collect())
}

fn discover_one_backing_expiry_consumer_boundary(
    mut seed: [u8; 32],
    kind: ExpiredBackingConsumerKind,
    expiry_offset: u8,
    landing: BackingExpiryLanding,
) -> Result<ExpiredBackingConsumerDiscovery, String> {
    const WINNER: usize = 0;
    const LOSER: usize = 1;
    const PROVIDER: usize = 2;
    const ASSET: u16 = 0;
    const WINNING_DOMAIN: u16 = 1;
    const INITIAL_PRICE: u64 = 100;
    const WINNING_MARK: u64 = 105;
    const SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const BACKING: u128 = 150;

    seed[0] ^= 0x64;
    seed[1] ^= kind.discriminator();
    seed[2] ^= expiry_offset;
    let expiry_slot = 2u64
        .checked_add(u64::from(expiry_offset.clamp(1, 6)))
        .ok_or_else(|| "expired-consumer expiry overflow".to_string())?;
    let authenticated_slot = landing.authenticated_slot(expiry_slot)?;
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: INITIAL_PRICE,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [1_000, 1_000, 0, 0, 0],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.update_asset_authority_from_admin(
        ASSET,
        percolator_prog::processor::ASSET_AUTH_BACKING_BUCKET,
        PROVIDER,
    )
    .map_err(|error| format!("install expired-consumer provider: {error}"))?;
    env.top_up_backing_bucket_for_actor(PROVIDER, WINNING_DOMAIN, BACKING, expiry_slot)
        .map_err(|error| format!("fund expiring backing consumer: {error}"))?;
    env.trade_no_cpi(WINNER, LOSER, ASSET, SIZE_Q, INITIAL_PRICE, 0)
        .map_err(|error| format!("open expired-consumer position: {error}"))?;

    env.warp_to_slot(2);
    env.push_auth_mark(ASSET, 2, WINNING_MARK)
        .map_err(|error| format!("publish expired-consumer winning mark: {error}"))?;
    let oracle_accounts = env.primary_profile(ASSET as usize).oracle_leg_count;
    let observations = || {
        vec![CrankObservationHint {
            asset_index: ASSET,
            oracle_accounts,
        }]
    };
    for actor in [LOSER, WINNER] {
        env.crank(actor, 2, observations())
            .map_err(|error| format!("refresh expired-consumer actor {actor}: {error}"))?;
    }
    env.trade_no_cpi(WINNER, LOSER, ASSET, -SIZE_Q, WINNING_MARK, 0)
        .map_err(|error| format!("release source-backed PnL: {error}"))?;
    if discovery_position(&env.primary_portfolio(WINNER), ASSET)? != 0 {
        return Err("expired-consumer winner did not flatten before expiry".into());
    }
    let released_pnl = u128::try_from(env.primary_portfolio(WINNER).pnl.get())
        .map_err(|_| "expired-consumer winner had no positive released PnL".to_string())?;
    if released_pnl == 0 {
        return Err("expired-consumer fixture released zero PnL".into());
    }
    let before_group = env.primary_market_state().1;
    let consumed_before =
        before_group.source_backing_buckets[WINNING_DOMAIN as usize].consumed_liened_backing_num;
    let capital_before = env.primary_portfolio(WINNER).capital.get();
    let destination_before = env.token_amount(env.actors[WINNER].destination_token);

    env.warp_to_slot(authenticated_slot);
    let before_rejection = fingerprint(&env);
    let conversion_result = match kind {
        ExpiredBackingConsumerKind::ReleasedPnlConversion => {
            env.convert_released_pnl(WINNER, released_pnl)
        }
    };
    let conversion_landed = conversion_result.is_ok();
    let conversion_rejected_stale = matches!(
        &conversion_result,
        Err(error) if error.contains("Custom(19)")
    );
    if let Err(error) = &conversion_result {
        if !conversion_rejected_stale {
            return Err(format!(
                "expired-consumer conversion returned an unexpected error: {error}"
            ));
        }
    }
    let rejected_exact_rollback =
        conversion_result.is_err() && fingerprint(&env) == before_rejection;
    let capital_credit = env
        .primary_portfolio(WINNER)
        .capital
        .get()
        .checked_sub(capital_before)
        .ok_or_else(|| "expired-consumer conversion decreased capital".to_string())?;
    if conversion_landed {
        env.withdraw_primary(WINNER, released_pnl)
            .map_err(|error| format!("withdraw expired-consumer credit: {error}"))?;
    }
    let after_group = env.primary_market_state().1;
    let consumed_backing_num = after_group.source_backing_buckets[WINNING_DOMAIN as usize]
        .consumed_liened_backing_num
        .checked_sub(consumed_before)
        .ok_or_else(|| "expired-consumer backing consumption decreased".to_string())?;
    let extracted_tokens = env
        .token_amount(env.actors[WINNER].destination_token)
        .checked_sub(destination_before)
        .ok_or_else(|| "expired-consumer destination decreased".to_string())?;
    let senior_destination_before = env.token_amount(env.actors[WINNER].destination_token);
    let senior_withdraw_landed = env.withdraw_primary(WINNER, capital_before).is_ok();
    let senior_withdrawn_tokens = env
        .token_amount(env.actors[WINNER].destination_token)
        .checked_sub(senior_destination_before)
        .ok_or_else(|| "expired-consumer senior destination decreased".to_string())?;

    Ok(ExpiredBackingConsumerDiscovery {
        kind,
        landing,
        expiry_slot,
        authenticated_slot,
        engine_slot: after_group.current_slot,
        released_pnl,
        conversion_landed,
        conversion_rejected_stale,
        rejected_exact_rollback,
        capital_credit,
        consumed_backing_num,
        extracted_tokens,
        senior_capital_before: capital_before,
        senior_withdraw_landed,
        senior_withdrawn_tokens,
        token_supply_conserved: env.token_supply_observed() == supply_before,
    })
}

pub fn discover_backing_expiry_consumer_boundary(
    seed: [u8; 32],
    expiry_offset: u8,
    landing: BackingExpiryLanding,
) -> Result<Vec<ExpiredBackingConsumerDiscovery>, String> {
    ExpiredBackingConsumerKind::ALL
        .into_iter()
        .map(|kind| {
            discover_one_backing_expiry_consumer_boundary(seed, kind, expiry_offset, landing)
        })
        .collect()
}

pub fn discover_backing_expiry_consumer_boundaries(
    seed: [u8; 32],
    expiry_offset: u8,
) -> Result<Vec<ExpiredBackingConsumerDiscovery>, String> {
    BackingExpiryLanding::ALL
        .into_iter()
        .flat_map(|landing| {
            ExpiredBackingConsumerKind::ALL
                .into_iter()
                .map(move |kind| (landing, kind))
        })
        .map(|(landing, kind)| {
            discover_one_backing_expiry_consumer_boundary(seed, kind, expiry_offset, landing)
        })
        .collect()
}

pub fn discover_expired_backing_consumers(
    seed: [u8; 32],
    expiry_offset: u8,
) -> Result<Vec<ExpiredBackingConsumerDiscovery>, String> {
    discover_backing_expiry_consumer_boundary(seed, expiry_offset, BackingExpiryLanding::After)
}

fn discover_one_source_lien_reversal_exit(
    mut seed: [u8; 32],
    route: SourceLienReversalExitRoute,
    increase_divisor: u8,
) -> Result<SourceLienReversalDiscovery, String> {
    const OWNER: usize = 0;
    const COUNTERPARTY: usize = 1;
    const KEEPER: usize = 2;
    const ASSET: u16 = 0;
    const DOMAIN: usize = 1;
    const OPEN_PRICE: u64 = 100;
    const WINNING_PRICE: u64 = 105;
    const OPEN_Q: i128 = 1_000 * POS_SCALE as i128;
    const ATTEMPTS: u8 = 2;

    seed[0] ^= 0x68;
    seed[1] ^= route.discriminator();
    seed[2] ^= increase_divisor;
    let increase_q = OPEN_Q
        .checked_div(i128::from(increase_divisor.clamp(10, 40)))
        .ok_or_else(|| "source-lien increase divisor is zero".to_string())?;
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: OPEN_PRICE,
            h_max: 10,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 5_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [52_501, 1_000_000, 0, 0, 0],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.top_up_backing_bucket(DOMAIN as u16, 100_000, 100)
        .map_err(|error| format!("fund source-lien reversal backing: {error}"))?;
    env.trade_no_cpi(OWNER, COUNTERPARTY, ASSET, OPEN_Q, OPEN_PRICE, 0)
        .map_err(|error| format!("open source-lien reversal pair: {error}"))?;

    env.warp_to_slot(2);
    env.push_auth_mark(ASSET, 2, WINNING_PRICE)
        .map_err(|error| format!("publish source-lien winning mark: {error}"))?;
    let oracle_accounts = env.primary_profile(ASSET as usize).oracle_leg_count;
    let observation = || {
        vec![CrankObservationHint {
            asset_index: ASSET,
            oracle_accounts,
        }]
    };
    env.crank(KEEPER, 2, observation())
        .map_err(|error| format!("publish source-lien winning mark: {error}"))?;
    for actor in [COUNTERPARTY, OWNER] {
        crank_asset_progress(&mut env, actor, 2, ASSET, 4)?;
    }
    if env.primary_portfolio(OWNER).pnl.get() <= 0 {
        return Err("source-lien reversal owner earned no positive PnL".into());
    }
    env.trade_no_cpi(OWNER, COUNTERPARTY, ASSET, increase_q, WINNING_PRICE, 0)
        .map_err(|error| format!("create source-credit lien: {error}"))?;
    let liened_account = env.primary_portfolio(OWNER);
    let source_claim_liened_num = liened_account
        .source_domains
        .iter()
        .find(|source| {
            source.source_claim_market_id.get() != 0 && source.domain.get() as usize == DOMAIN
        })
        .map(|source| source.source_claim_liened_num.get())
        .unwrap_or(0);
    if source_claim_liened_num == 0 {
        return Err("risk increase created no source-credit lien".into());
    }

    env.warp_to_slot(3);
    env.push_auth_mark(ASSET, 3, OPEN_PRICE)
        .map_err(|error| format!("publish source-lien reversal mark: {error}"))?;
    env.crank(KEEPER, 3, observation())
        .map_err(|error| format!("publish source-lien reversal mark: {error}"))?;
    crank_asset_progress(&mut env, COUNTERPARTY, 3, ASSET, 4)?;

    let funded_capital = env.primary_portfolio(OWNER).capital.get();
    let position_before_q =
        discovery_position(&env.primary_portfolio(OWNER), ASSET)?.unsigned_abs();
    let canonical_vault_liquidity = u128::from(env.token_amount(env.vault));
    if env.primary_market_state().1.vault != canonical_vault_liquidity {
        return Err("source-lien reversal vault diverged from SPL custody".into());
    }
    let destination_before = env.token_amount(env.actors[OWNER].destination_token);
    let mut attempts = ATTEMPTS;
    let mut successful_calls = 0u8;
    let mut lock_active_rejections = 0u8;
    let mut rejection_errors = Vec::new();
    let mut exact_rollback = true;
    let trade_market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    for _ in 0..ATTEMPTS {
        let before = fingerprint(&env);
        let result = match route {
            SourceLienReversalExitRoute::PermissionlessCrank => env.crank(OWNER, 3, vec![]),
            SourceLienReversalExitRoute::RebalanceReduce => {
                env.rebalance_reduce(OWNER, ASSET, POS_SCALE)
            }
            SourceLienReversalExitRoute::TradeNoCpi => env.trade_no_cpi(
                OWNER,
                COUNTERPARTY,
                ASSET,
                -(POS_SCALE as i128),
                OPEN_PRICE,
                0,
            ),
            SourceLienReversalExitRoute::BatchNoCpi => env.batch_trade_no_cpi(
                OWNER,
                COUNTERPARTY,
                vec![BatchTradeLeg {
                    asset_index: ASSET,
                    market_id: trade_market_id,
                    size_q: -(POS_SCALE as i128),
                    exec_price: OPEN_PRICE,
                    fee_bps: 0,
                }],
            ),
            SourceLienReversalExitRoute::TradeCpi => {
                env.trade_cpi(OWNER, COUNTERPARTY, ASSET, -(POS_SCALE as i128), 0, 0)
            }
            SourceLienReversalExitRoute::BatchCpi => env.batch_trade_cpi(
                OWNER,
                COUNTERPARTY,
                vec![BatchTradeCpiLeg {
                    asset_index: ASSET,
                    market_id: trade_market_id,
                    size_q: -(POS_SCALE as i128),
                    fee_bps: 0,
                    limit_price: 0,
                }],
            ),
        };
        match result {
            Ok(_) => successful_calls = successful_calls.saturating_add(1),
            Err(error) => {
                if error.contains("Custom(21)") || error.contains("custom program error: 0x15") {
                    lock_active_rejections = lock_active_rejections.saturating_add(1);
                }
                exact_rollback &= fingerprint(&env) == before;
                rejection_errors.push(error);
            }
        }
    }
    if route == SourceLienReversalExitRoute::PermissionlessCrank {
        attempts = attempts.saturating_add(1);
        let before = fingerprint(&env);
        match env.rebalance_reduce(OWNER, ASSET, POS_SCALE) {
            Ok(_) => successful_calls = successful_calls.saturating_add(1),
            Err(error) => {
                if error.contains("Custom(21)") || error.contains("custom program error: 0x15") {
                    lock_active_rejections = lock_active_rejections.saturating_add(1);
                }
                exact_rollback &= fingerprint(&env) == before;
                rejection_errors.push(error);
            }
        }
    }
    let position_after_q = discovery_position(&env.primary_portfolio(OWNER), ASSET)?.unsigned_abs();
    let external_payout = env
        .token_amount(env.actors[OWNER].destination_token)
        .checked_sub(destination_before)
        .ok_or_else(|| "source-lien reversal destination decreased".to_string())?;
    Ok(SourceLienReversalDiscovery {
        route,
        source_claim_liened_num,
        funded_capital,
        position_before_q,
        position_after_q,
        canonical_vault_liquidity,
        attempts,
        successful_calls,
        lock_active_rejections,
        rejection_errors,
        exact_rollback,
        external_payout,
        token_supply_conserved: env.token_supply_observed() == supply_before,
    })
}

pub fn discover_source_lien_reversal_exit_locks(
    seed: [u8; 32],
    increase_divisor: u8,
) -> Result<Vec<SourceLienReversalDiscovery>, String> {
    SourceLienReversalExitRoute::ALL
        .into_iter()
        .map(|route| discover_one_source_lien_reversal_exit(seed, route, increase_divisor))
        .collect()
}

fn discover_one_cross_domain_rounding_exit_lock(
    mut seed: [u8; 32],
    order: CrossDomainRoundingOrder,
) -> Result<CrossDomainRoundingDiscovery, String> {
    const TARGET: usize = 0;
    const HELPER: usize = 1;
    const TARGET_SHORTS: [usize; 2] = [2, 3];
    const FIRST_HELPER_SHORT: usize = 4;
    const OPEN_PRICE: u64 = 100;
    const REVERSAL_PRICE: u64 = 1;

    seed[0] ^= 0x6a;
    seed[1] ^= order.discriminator();
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: OPEN_PRICE,
            h_max: 2,
            maintenance_margin_bps: 10_000,
            initial_margin_bps: 10_000,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [10_000, 10_000, 102, 102, 200],
            ..MarketConfig::default()
        },
    );
    let second_helper_short = env.add_primary_actor(seed, 0, 1_000, 200);
    let helper_shorts = [FIRST_HELPER_SHORT, second_helper_short];
    let supply_before = env.token_supply_observed();
    let assets = order.assets();
    let oracle_accounts = [
        env.primary_profile(0).oracle_leg_count,
        env.primary_profile(1).oracle_leg_count,
    ];
    let observation = |asset_index| {
        vec![CrankObservationHint {
            asset_index,
            oracle_accounts: oracle_accounts[asset_index as usize],
        }]
    };
    let mut slot = env.current_slot();
    for (offset, asset_index) in assets.into_iter().enumerate() {
        env.trade_no_cpi(
            TARGET,
            TARGET_SHORTS[offset],
            asset_index,
            POS_SCALE as i128,
            OPEN_PRICE,
            0,
        )
        .map_err(|error| format!("open target asset {asset_index}: {error}"))?;

        slot = slot
            .checked_add(1)
            .ok_or_else(|| "cross-domain setup slot overflow".to_string())?;
        env.warp_to_slot(slot);
        env.push_auth_mark(asset_index, slot, OPEN_PRICE)
            .map_err(|error| format!("publish opening mark for asset {asset_index}: {error}"))?;
        env.crank(TARGET, slot, observation(asset_index))
            .map_err(|error| format!("crank target opening asset {asset_index}: {error}"))?;
        env.crank(TARGET_SHORTS[offset], slot, observation(asset_index))
            .map_err(|error| format!("crank target short opening asset {asset_index}: {error}"))?;

        env.trade_no_cpi(
            HELPER,
            helper_shorts[offset],
            asset_index,
            2 * POS_SCALE as i128,
            OPEN_PRICE,
            0,
        )
        .map_err(|error| format!("open helper asset {asset_index}: {error}"))?;
    }

    for gain_price in [200, 300] {
        slot = slot
            .checked_add(1)
            .ok_or_else(|| "cross-domain gain slot overflow".to_string())?;
        env.warp_to_slot(slot);
        for asset_index in assets {
            env.push_auth_mark(asset_index, slot, gain_price)
                .map_err(|error| format!("publish gain mark for asset {asset_index}: {error}"))?;
        }
        for (offset, asset_index) in assets.into_iter().enumerate() {
            env.crank(TARGET_SHORTS[offset], slot, observation(asset_index))
                .map_err(|error| format!("crank target short gain asset {asset_index}: {error}"))?;
        }
    }
    for (offset, asset_index) in assets.into_iter().enumerate() {
        env.crank(helper_shorts[offset], slot, observation(asset_index))
            .map_err(|error| format!("crank helper short gain asset {asset_index}: {error}"))?;
    }
    for actor in [TARGET, HELPER] {
        env.crank(actor, slot, observation(0))
            .map_err(|error| format!("crank aggregate winner {actor}: {error}"))?;
    }

    let positive_pnl_before_reversal = env.primary_portfolio(TARGET).pnl.get();
    let market_before_reversal = env.primary_market_state().1;
    let fractional_source_domains = [1usize, 3]
        .into_iter()
        .filter(|domain| {
            let rate = market_before_reversal.source_credit[*domain].credit_rate_num;
            rate != 0 && rate < percolator::CREDIT_RATE_SCALE
        })
        .count() as u8;

    let affected_asset = assets[0];
    slot = slot
        .checked_add(1)
        .ok_or_else(|| "cross-domain reversal slot overflow".to_string())?;
    env.warp_to_slot(slot);
    env.push_auth_mark(affected_asset, slot, REVERSAL_PRICE)
        .map_err(|error| format!("publish reversal mark for asset {affected_asset}: {error}"))?;
    env.crank(TARGET_SHORTS[0], slot, observation(affected_asset))
        .map_err(|error| format!("crank reversal counterparty asset {affected_asset}: {error}"))?;

    let target_before_exit = env.primary_portfolio(TARGET);
    let funded_capital = target_before_exit.capital.get();
    let stranded_position_q = assets.into_iter().try_fold(0u128, |sum, asset_index| {
        sum.checked_add(discovery_position(&target_before_exit, asset_index)?.unsigned_abs())
            .ok_or_else(|| "cross-domain position total overflow".to_string())
    })?;
    let canonical_vault_liquidity = u128::from(env.token_amount(env.vault));
    if env.primary_market_state().1.vault != canonical_vault_liquidity {
        return Err("cross-domain rounding vault diverged from SPL custody".into());
    }

    let mut blocked_public_routes = 0u8;
    let mut exact_rollback = true;
    let trade_market_id = env.primary_market_state().1.assets[affected_asset as usize].market_id;
    for route in SourceLienReversalExitRoute::ALL {
        let before = fingerprint(&env);
        let result = match route {
            SourceLienReversalExitRoute::PermissionlessCrank => {
                env.crank(TARGET, slot, observation(affected_asset))
            }
            SourceLienReversalExitRoute::RebalanceReduce => {
                env.rebalance_reduce(TARGET, affected_asset, POS_SCALE)
            }
            SourceLienReversalExitRoute::TradeNoCpi => env.trade_no_cpi(
                TARGET,
                TARGET_SHORTS[0],
                affected_asset,
                -(POS_SCALE as i128),
                REVERSAL_PRICE,
                0,
            ),
            SourceLienReversalExitRoute::BatchNoCpi => env.batch_trade_no_cpi(
                TARGET,
                TARGET_SHORTS[0],
                vec![BatchTradeLeg {
                    asset_index: affected_asset,
                    market_id: trade_market_id,
                    size_q: -(POS_SCALE as i128),
                    exec_price: REVERSAL_PRICE,
                    fee_bps: 0,
                }],
            ),
            SourceLienReversalExitRoute::TradeCpi => env.trade_cpi(
                TARGET,
                TARGET_SHORTS[0],
                affected_asset,
                -(POS_SCALE as i128),
                0,
                0,
            ),
            SourceLienReversalExitRoute::BatchCpi => env.batch_trade_cpi(
                TARGET,
                TARGET_SHORTS[0],
                vec![BatchTradeCpiLeg {
                    asset_index: affected_asset,
                    market_id: trade_market_id,
                    size_q: -(POS_SCALE as i128),
                    fee_bps: 0,
                    limit_price: 0,
                }],
            ),
        };
        let after = fingerprint(&env);
        match result {
            Ok(_) if after != before => break,
            Ok(_) => blocked_public_routes = blocked_public_routes.saturating_add(1),
            Err(_) => {
                blocked_public_routes = blocked_public_routes.saturating_add(1);
                exact_rollback &= after == before;
            }
        }
    }

    let mut later_honest_crank_blocked = false;
    if blocked_public_routes == SourceLienReversalExitRoute::ALL.len() as u8 {
        slot = slot
            .checked_add(1)
            .ok_or_else(|| "cross-domain later slot overflow".to_string())?;
        env.warp_to_slot(slot);
        env.push_auth_mark(affected_asset, slot, REVERSAL_PRICE)
            .map_err(|error| format!("publish later reversal mark: {error}"))?;
        let _ = env.crank(TARGET_SHORTS[0], slot, observation(affected_asset));
        let before = fingerprint(&env);
        let result = env.crank(TARGET, slot, observation(affected_asset));
        let after = fingerprint(&env);
        later_honest_crank_blocked = match result {
            Ok(_) => after == before,
            Err(_) => {
                exact_rollback &= after == before;
                true
            }
        };
    }

    Ok(CrossDomainRoundingDiscovery {
        order,
        fractional_source_domains,
        positive_pnl_before_reversal,
        funded_capital,
        stranded_position_q,
        blocked_public_routes,
        later_honest_crank_blocked,
        exact_rollback,
        canonical_vault_liquidity,
        token_supply_conserved: env.token_supply_observed() == supply_before,
    })
}

pub fn discover_cross_domain_rounding_exit_locks(
    seed: [u8; 32],
) -> Result<Vec<CrossDomainRoundingDiscovery>, String> {
    CrossDomainRoundingOrder::ALL
        .into_iter()
        .map(|order| discover_one_cross_domain_rounding_exit_lock(seed, order))
        .collect()
}

fn discover_one_flat_source_lien_claim_lock(
    mut seed: [u8; 32],
    provider_withdrawal: u128,
    escape_route: FlatSourceLienEscapeRoute,
) -> Result<FlatSourceLienDiscovery, String> {
    const WINNER: usize = 0;
    const COUNTERPARTY: usize = 1;
    const PRICE: u64 = 100;
    const ASSET0_SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const ASSET1_SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const SAFE_INCREASE_Q: i128 = POS_SCALE as i128;
    const CONVERSION_ATTEMPTS: u8 = 3;

    seed[0] ^= 0x6b;
    seed[1] ^= provider_withdrawal as u8;
    seed[2] ^= escape_route.discriminator();
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            h_max: 4,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [313, 1_000, 0, 0, 0],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    let oracle_accounts = [
        env.primary_profile(0).oracle_leg_count,
        env.primary_profile(1).oracle_leg_count,
    ];
    let observation = |asset_index| {
        vec![CrankObservationHint {
            asset_index,
            oracle_accounts: oracle_accounts[asset_index as usize],
        }]
    };
    let complete_observations = || {
        [0u16, 1]
            .into_iter()
            .map(|asset_index| CrankObservationHint {
                asset_index,
                oracle_accounts: oracle_accounts[asset_index as usize],
            })
            .collect::<Vec<_>>()
    };
    env.top_up_backing_bucket(1, 150, 10)
        .map_err(|error| format!("fund flat source-lien backing: {error}"))?;
    env.trade_no_cpi(WINNER, COUNTERPARTY, 0, ASSET0_SIZE_Q, PRICE, 0)
        .map_err(|error| format!("open flat-lien winning leg: {error}"))?;
    env.trade_no_cpi(WINNER, COUNTERPARTY, 1, ASSET1_SIZE_Q, PRICE, 0)
        .map_err(|error| format!("open flat-lien adverse leg: {error}"))?;

    env.warp_to_slot(2);
    env.push_auth_mark(0, 2, 105)
        .map_err(|error| format!("publish flat-lien winning mark: {error}"))?;
    env.push_auth_mark(1, 2, 95)
        .map_err(|error| format!("publish flat-lien adverse mark: {error}"))?;
    for actor in [COUNTERPARTY, WINNER] {
        env.crank(actor, 2, complete_observations())
            .map_err(|error| format!("settle flat-lien actor {actor}: {error}"))?;
    }
    env.withdraw_backing_bucket(1, provider_withdrawal)
        .map_err(|error| format!("lower flat-lien backing watermark: {error}"))?;

    env.warp_to_slot(3);
    env.trade_no_cpi(WINNER, COUNTERPARTY, 1, SAFE_INCREASE_Q, 95, 0)
        .map_err(|error| format!("create flat source lien: {error}"))?;
    env.trade_no_cpi(
        WINNER,
        COUNTERPARTY,
        1,
        -(ASSET1_SIZE_Q + SAFE_INCREASE_Q),
        95,
        0,
    )
    .map_err(|error| format!("flatten adverse source leg: {error}"))?;
    env.trade_no_cpi(WINNER, COUNTERPARTY, 0, -ASSET0_SIZE_Q, 105, 0)
        .map_err(|error| format!("flatten winning source leg: {error}"))?;

    let flat = env.primary_portfolio(WINNER);
    let flat_position_q = [0u16, 1].into_iter().try_fold(0u128, |sum, asset_index| {
        sum.checked_add(discovery_position(&flat, asset_index)?.unsigned_abs())
            .ok_or_else(|| "flat source-lien position overflow".to_string())
    })?;
    let positive_pnl = flat.pnl.get();
    let source_claim_liened_num = flat
        .source_domains
        .iter()
        .map(|source| source.source_claim_liened_num.get())
        .try_fold(0u128, |sum, value| {
            sum.checked_add(value)
                .ok_or_else(|| "flat source-lien total overflow".to_string())
        })?;
    if flat_position_q != 0 || positive_pnl <= 0 || source_claim_liened_num == 0 {
        return Err(format!(
            "flat source-lien precondition missing: position={flat_position_q}, pnl={positive_pnl}, lien={source_claim_liened_num}"
        ));
    }

    let before_crank_lien = source_claim_liened_num;
    let _ = env.crank(WINNER, 3, observation(0));
    let after_crank_lien: u128 = env
        .primary_portfolio(WINNER)
        .source_domains
        .iter()
        .map(|source| source.source_claim_liened_num.get())
        .sum();
    let mut later_honest_crank_released_lien = after_crank_lien < before_crank_lien;
    let mut conversion_rejections = 0u8;
    let mut exact_rollback = true;
    for amount in [positive_pnl as u128, 1] {
        let before = fingerprint(&env);
        match env.convert_released_pnl(WINNER, amount) {
            Ok(_) => {}
            Err(_) => {
                conversion_rejections = conversion_rejections.saturating_add(1);
                exact_rollback &= fingerprint(&env) == before;
            }
        }
    }

    env.warp_to_slot(4);
    env.push_auth_mark(0, 4, 105)
        .map_err(|error| format!("publish later flat-lien winning mark: {error}"))?;
    env.push_auth_mark(1, 4, 95)
        .map_err(|error| format!("publish later flat-lien adverse mark: {error}"))?;
    for (actor, asset_index) in [(COUNTERPARTY, 0), (COUNTERPARTY, 1), (WINNER, 0)] {
        let _ = env.crank(actor, 4, observation(asset_index));
    }
    let later_lien: u128 = env
        .primary_portfolio(WINNER)
        .source_domains
        .iter()
        .map(|source| source.source_claim_liened_num.get())
        .sum();
    later_honest_crank_released_lien |= later_lien < after_crank_lien;
    let before_later_conversion = fingerprint(&env);
    match env.convert_released_pnl(WINNER, positive_pnl as u128) {
        Ok(_) => {}
        Err(_) => {
            conversion_rejections = conversion_rejections.saturating_add(1);
            exact_rollback &= fingerprint(&env) == before_later_conversion;
        }
    }

    let before_close = fingerprint(&env);
    let close_rejected = env.close_primary_portfolio(WINNER).is_err();
    if close_rejected {
        exact_rollback &= fingerprint(&env) == before_close;
    }

    let mut round_trip_completed = false;
    let mut round_trip_released_claim = false;
    if close_rejected {
        let route_trade = |env: &mut V16Svm, size_q: i128| {
            let market_id = env.primary_market_state().1.assets[0].market_id;
            match escape_route {
                FlatSourceLienEscapeRoute::TradeNoCpi => {
                    env.trade_no_cpi(WINNER, COUNTERPARTY, 0, size_q, 105, 0)
                }
                FlatSourceLienEscapeRoute::BatchNoCpi => env.batch_trade_no_cpi(
                    WINNER,
                    COUNTERPARTY,
                    vec![BatchTradeLeg {
                        asset_index: 0,
                        market_id,
                        size_q,
                        exec_price: 105,
                        fee_bps: 0,
                    }],
                ),
                FlatSourceLienEscapeRoute::TradeCpi => {
                    env.trade_cpi(WINNER, COUNTERPARTY, 0, size_q, 0, 0)
                }
                FlatSourceLienEscapeRoute::BatchCpi => env.batch_trade_cpi(
                    WINNER,
                    COUNTERPARTY,
                    vec![BatchTradeCpiLeg {
                        asset_index: 0,
                        market_id,
                        size_q,
                        fee_bps: 0,
                        limit_price: 0,
                    }],
                ),
            }
        };
        let before_open = fingerprint(&env);
        let open = route_trade(&mut env, POS_SCALE as i128);
        if open.is_err() {
            exact_rollback &= fingerprint(&env) == before_open;
        } else {
            let before_flatten = fingerprint(&env);
            let flatten = route_trade(&mut env, -(POS_SCALE as i128));
            if flatten.is_err() {
                exact_rollback &= fingerprint(&env) == before_flatten;
            } else {
                round_trip_completed = true;
                let _ = env.crank(WINNER, 4, observation(0));
                let after_round_trip = env.primary_portfolio(WINNER);
                let lien_after_round_trip: u128 = after_round_trip
                    .source_domains
                    .iter()
                    .map(|source| source.source_claim_liened_num.get())
                    .sum();
                round_trip_released_claim = lien_after_round_trip < source_claim_liened_num;
                if after_round_trip.pnl.get() > 0 {
                    round_trip_released_claim |= env
                        .convert_released_pnl(WINNER, after_round_trip.pnl.get() as u128)
                        .is_ok();
                }
            }
        }
    }
    let canonical_vault_liquidity = u128::from(env.token_amount(env.vault));
    if env.primary_market_state().1.vault != canonical_vault_liquidity {
        return Err("flat source-lien vault diverged from SPL custody".into());
    }
    Ok(FlatSourceLienDiscovery {
        escape_route,
        provider_withdrawal,
        flat_position_q,
        positive_pnl,
        source_claim_liened_num,
        conversion_attempts: CONVERSION_ATTEMPTS,
        conversion_rejections,
        later_honest_crank_released_lien,
        close_rejected,
        round_trip_completed,
        round_trip_released_claim,
        exact_rollback,
        canonical_vault_liquidity,
        token_supply_conserved: env.token_supply_observed() == supply_before,
    })
}

pub fn discover_flat_source_lien_claim_locks(
    seed: [u8; 32],
    provider_withdrawal: u128,
) -> Result<Vec<FlatSourceLienDiscovery>, String> {
    FlatSourceLienEscapeRoute::ALL
        .into_iter()
        .map(|route| discover_one_flat_source_lien_claim_lock(seed, provider_withdrawal, route))
        .collect()
}

pub fn discover_cross_domain_b_violation(
    mut seed: [u8; 32],
) -> Result<CrossDomainBDiscovery, String> {
    const INITIAL_PRICE: u64 = 100;
    const FIRST_MARK: u64 = 105;
    const BANKRUPTCY_MARK: u64 = 500;
    const WINNER_Q: i128 = 20 * POS_SCALE as i128;
    const UNFUNDED_DOMAIN: usize = 1;
    const FUNDED_DOMAIN: usize = 3;
    const WINNER_DEPOSIT: u128 = 100_000;

    seed[0] ^= 0xbf;
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [WINNER_DEPOSIT, 100_000, 250, 100_000, EXIT_MAKER_DEPOSIT],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    for asset_index in [0u16, 1] {
        env.configure_auth_mark(false, asset_index, 1, INITIAL_PRICE)
            .map_err(|error| format!("configure asset {asset_index} mark: {error}"))?;
    }
    env.top_up_backing_bucket(FUNDED_DOMAIN as u16, 20_000, 1_000)
        .map_err(|error| format!("fund affected source domain: {error}"))?;
    for asset_index in [0u16, 1] {
        env.trade_no_cpi(0, 1, asset_index, WINNER_Q, INITIAL_PRICE, 0)
            .map_err(|error| format!("open asset {asset_index} claim pair: {error}"))?;
    }

    env.warp_to_slot(2);
    for asset_index in [0u16, 1] {
        env.push_auth_mark(asset_index, 2, FIRST_MARK)
            .map_err(|error| format!("move asset {asset_index}: {error}"))?;
        crank_asset_progress(&mut env, 0, 2, asset_index, 4)?;
    }
    let first_claims = env.primary_portfolio(0);
    if discovery_source_claim(&first_claims, UNFUNDED_DOMAIN) != 100 * percolator::BOUND_SCALE
        || discovery_source_claim(&first_claims, FUNDED_DOMAIN) != 100 * percolator::BOUND_SCALE
    {
        return Err("B-settlement setup did not create equal source claims".into());
    }

    env.trade_no_cpi(0, 2, 1, POS_SCALE as i128, FIRST_MARK, 0)
        .map_err(|error| format!("open bankrupt asset-1 counterposition: {error}"))?;
    env.warp_to_slot(7);
    env.push_auth_mark(1, 7, BANKRUPTCY_MARK)
        .map_err(|error| format!("publish bankruptcy mark: {error}"))?;
    crank_asset_progress(&mut env, 0, 7, 1, 16)?;
    let observation = vec![CrankObservationHint {
        asset_index: 1,
        oracle_accounts: env.primary_profile(1).oracle_leg_count,
    }];
    let mut bankrupt_progress = false;
    for _ in 0..12 {
        if env.crank(2, 7, observation.clone()).is_ok() {
            bankrupt_progress = true;
        }
        if env.primary_market_state().1.assets[1].b_long_num != 0 {
            break;
        }
    }
    let b_target_num = env.primary_market_state().1.assets[1].b_long_num;
    if !bankrupt_progress || b_target_num == 0 {
        return Err("public liquidation did not book asset-1 B loss".into());
    }

    let before_settle = env.primary_portfolio(0);
    let unfunded_claim_before_num = discovery_source_claim(&before_settle, UNFUNDED_DOMAIN);
    let funded_claim_before_num = discovery_source_claim(&before_settle, FUNDED_DOMAIN);
    if unfunded_claim_before_num == 0 || funded_claim_before_num <= unfunded_claim_before_num {
        return Err("B-settlement claims lack domain discriminator".into());
    }
    crank_asset_progress(&mut env, 0, 7, 1, 8)?;
    let after_settle = env.primary_portfolio(0);
    let settled_b_snap = after_settle
        .legs
        .iter()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .find(|leg| leg.active && leg.asset_index == 1)
        .map(|leg| leg.b_snap)
        .ok_or_else(|| "winner lost affected leg during B settlement".to_string())?;
    if settled_b_snap != b_target_num {
        return Err("winner B snapshot did not reach market target".into());
    }
    let pnl_loss = u128::try_from(
        before_settle
            .pnl
            .get()
            .checked_sub(after_settle.pnl.get())
            .ok_or_else(|| "B-settlement PnL subtraction overflow".to_string())?,
    )
    .map_err(|_| "B settlement did not reduce winner PnL".to_string())?;
    let unfunded_claim_after_num = discovery_source_claim(&after_settle, UNFUNDED_DOMAIN);
    let funded_claim_after_num = discovery_source_claim(&after_settle, FUNDED_DOMAIN);
    let wrong_domain_reduction_num = unfunded_claim_before_num
        .checked_sub(unfunded_claim_after_num)
        .ok_or_else(|| "unaffected source claim increased".to_string())?;
    let correct_domain_reduction_num = funded_claim_before_num
        .checked_sub(funded_claim_after_num)
        .ok_or_else(|| "affected source claim increased".to_string())?;
    let expected_reduction = pnl_loss
        .checked_mul(percolator::BOUND_SCALE)
        .ok_or_else(|| "B claim reduction overflow".to_string())?;
    if wrong_domain_reduction_num != 0 || correct_domain_reduction_num != expected_reduction {
        return Err(format!(
            "B-settlement was not source-domain local: loss={pnl_loss}, unrelated={wrong_domain_reduction_num}, affected={correct_domain_reduction_num}"
        ));
    }

    let mut reduction_steps = 0u8;
    loop {
        let position = discovery_position(&env.primary_portfolio(0), 1)?;
        if position == 0 {
            break;
        }
        if position < 0 {
            return Err(format!("affected position flipped during exit: {position}"));
        }
        let position_q = position as u128;
        let candidates = [
            position_q,
            (position_q / 2).max(1),
            (position_q / 3).max(1),
            (position_q / 4).max(1),
            (position_q / 5).max(1),
            (position_q / 7).max(1),
            (position_q / 10).max(1),
            position_q.min(POS_SCALE),
            position_q % POS_SCALE,
            position_q.saturating_sub(POS_SCALE),
            1,
        ];
        let mut progressed = false;
        let mut failures = Vec::new();
        for (candidate_index, reduce_q) in candidates.into_iter().enumerate() {
            if reduce_q == 0 || candidates[..candidate_index].contains(&reduce_q) {
                continue;
            }
            let market_before = env.market_data(false);
            let portfolio_before = env.primary_portfolio_data(0);
            match env.rebalance_reduce(0, 1, reduce_q) {
                Ok(_) => {
                    reduction_steps = reduction_steps
                        .checked_add(1)
                        .ok_or_else(|| "B reduction-step count overflow".to_string())?;
                    for _ in 0..4 {
                        if env.crank(0, 7, observation.clone()).is_err() {
                            break;
                        }
                    }
                    progressed = true;
                    break;
                }
                Err(error) => {
                    if env.market_data(false) != market_before
                        || env.primary_portfolio_data(0) != portfolio_before
                    {
                        return Err(format!("failed B reduction {reduce_q} mutated state"));
                    }
                    failures.push(format!("{reduce_q}: {error}"));
                }
            }
        }
        if progressed {
            if reduction_steps >= 64 {
                return Err("B reduction search exceeded 64 steps".into());
            }
            continue;
        }
        return Err(format!(
            "no bounded public reduction progressed position {position_q}: {}",
            failures.join("; ")
        ));
    }
    let affected_position_after_q = discovery_position(&env.primary_portfolio(0), 1)?;
    let asset_zero_position_q = discovery_position(&env.primary_portfolio(0), 0)?;
    if asset_zero_position_q <= 0 {
        return Err(format!(
            "unrelated asset position changed unexpectedly: {asset_zero_position_q}"
        ));
    }
    env.trade_no_cpi(0, 1, 0, -asset_zero_position_q, FIRST_MARK, 0)
        .map_err(|error| format!("close unrelated asset after B settlement: {error}"))?;
    if discovery_position(&env.primary_portfolio(0), 0)? != 0
        || discovery_position(&env.primary_portfolio(0), 1)? != 0
    {
        return Err("winner did not reach a flat public state".into());
    }

    let winner_capital = env.primary_portfolio(0).capital.get();
    let destination_before = env.token_amount(env.actors[0].destination_token);
    env.withdraw_primary(0, winner_capital)
        .map_err(|error| format!("withdraw flat winner principal: {error}"))?;
    let principal_withdrawn = u128::from(
        env.token_amount(env.actors[0].destination_token)
            .checked_sub(destination_before)
            .ok_or_else(|| "winner destination decreased".to_string())?,
    );
    let token_supply_conserved = env.token_supply_observed() == supply_before;
    if principal_withdrawn != winner_capital || !token_supply_conserved {
        return Err(format!(
            "winner principal reconciliation failed: withdrew={principal_withdrawn}/{winner_capital}, supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }
    Ok(CrossDomainBDiscovery {
        b_target_num,
        pnl_loss,
        wrong_domain_reduction_num,
        correct_domain_reduction_num,
        reduction_steps,
        affected_position_after_q,
        principal_withdrawn,
        token_supply_conserved,
    })
}
