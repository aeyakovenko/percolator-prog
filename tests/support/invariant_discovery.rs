use super::v16_svm::{MarketConfig, V16Svm, INITIAL_PRICE, PRIMARY_ACTOR_COUNT, USER_DEPOSIT};
use percolator::{MarketModeV16, SideModeV16, POS_SCALE};
use percolator_prog::ix::{BatchTradeCpiLeg, BatchTradeLeg, CrankObservationHint};
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
pub enum RetryIntentKind {
    Deposit,
    Withdraw,
    TradeNoCpi,
    TradeCpi,
    BatchTradeNoCpi,
    BatchTradeCpi,
    InsuranceTopUp,
    BackingTopUp,
    AssetActivation,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SupersededIntentKind {
    MatcherConfig,
    PushAuthMark,
    ConfigureAuthMark,
    TradeFeePolicy,
    FeeRedirectPolicy,
    LiquidationFeePolicy,
    MaintenanceFeePolicy,
    ResolvePolicy,
    BackingFeePolicy,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum FeeConsentKind {
    LiveBaseFeeHike,
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
    BatchNoCpi,
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
    pub const ALL: [Self; 2] = [Self::NoCpi, Self::BatchNoCpi];

    fn discriminator(self) -> u8 {
        match self {
            Self::NoCpi => 0,
            Self::BatchNoCpi => 1,
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
        Self::LiveBaseFeeHike,
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
            Self::LiveBaseFeeHike => 0,
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
    pub const ALL: [Self; 9] = [
        Self::MatcherConfig,
        Self::PushAuthMark,
        Self::ConfigureAuthMark,
        Self::TradeFeePolicy,
        Self::FeeRedirectPolicy,
        Self::LiquidationFeePolicy,
        Self::MaintenanceFeePolicy,
        Self::ResolvePolicy,
        Self::BackingFeePolicy,
    ];

    fn discriminator(self) -> u8 {
        match self {
            Self::MatcherConfig => 0,
            Self::PushAuthMark => 1,
            Self::ConfigureAuthMark => 2,
            Self::TradeFeePolicy => 3,
            Self::FeeRedirectPolicy => 4,
            Self::LiquidationFeePolicy => 5,
            Self::MaintenanceFeePolicy => 6,
            Self::ResolvePolicy => 7,
            Self::BackingFeePolicy => 8,
        }
    }
}

impl RetryIntentKind {
    pub const ALL: [Self; 9] = [
        Self::Deposit,
        Self::Withdraw,
        Self::TradeNoCpi,
        Self::TradeCpi,
        Self::BatchTradeNoCpi,
        Self::BatchTradeCpi,
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
            Self::InsuranceTopUp => 6,
            Self::BackingTopUp => 7,
            Self::AssetActivation => 8,
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetGenerationDiscovery {
    pub kind: AssetIntentKind,
    pub old_asset_id: u64,
    pub new_asset_id: u64,
    pub accepted_stale_intent: bool,
    pub mutated_economic_state: bool,
    pub compute_units: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorityIncarnationDiscovery {
    pub kind: AuthorityIntentKind,
    pub accepted_stale_intent: bool,
    pub mutated_economic_state: bool,
    pub compute_units: Option<u64>,
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
    pub victim_payout_loss: u64,
    pub counterparty_payout_gain: u64,
    pub committed_total_payout: u128,
    pub reordered_total_payout: u128,
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
    pub control_reward: u128,
    pub reordered_reward: u128,
    pub control_winner_payout: u128,
    pub reordered_winner_payout: u128,
    pub victim_payout: u128,
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
    pub coalition_profit: u128,
    pub victim_loss: u128,
    pub fee_counterparty_loss: u128,
    pub insurance_gain: u128,
    pub extracted_tokens: u128,
}

impl BilateralMarkFeeDiscovery {
    pub fn is_violation(&self) -> bool {
        self.queued_mark >= self.setup_mark
            && self.coalition_profit != 0
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
    pub fn is_violation(&self) -> bool {
        let reward_gain = self.reordered_reward.saturating_sub(self.control_reward);
        let winner_loss = self
            .control_winner_payout
            .saturating_sub(self.reordered_winner_payout);
        reward_gain != 0
            && reward_gain == winner_loss
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

impl AccrualOrderingDiscovery {
    pub fn is_violation(&self) -> bool {
        self.control_paid != 0
            && self.control_paid == self.control_received
            && self.reordered_paid == 0
            && self.reordered_received == 0
            && self.victim_claim_loss != 0
            && self.victim_claim_loss == self.attacker_claim_gain
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
    let supply_before = env.token_supply_observed();
    if kind == MarketIntentKind::ShutdownAsset {
        env.configure_permissionless_resolve(1_000_000, 1)
            .map_err(|error| format!("configure old-market shutdown policy: {error}"))?;
    }
    let old_market_id = env.primary_market_state().1.assets[0].market_id;
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
        AssetIntentKind::PushAuthMark => env.build_retained_auth_mark(asset_index, stale_price),
        AssetIntentKind::PushEwmaMark => env.build_retained_ewma_mark(asset_index, stale_price),
        AssetIntentKind::ConfigureAuthMark => {
            env.build_retained_auth_config(asset_index, stale_price)
        }
        AssetIntentKind::ConfigureEwmaMark => {
            env.build_retained_ewma_config(asset_index, stale_price, 1, 0)
        }
        AssetIntentKind::ConfigureHybridOracle => {
            let feed = [0x5au8; 32];
            env.build_retained_hybrid_oracle_config(
                asset_index,
                5,
                101,
                0,
                [feed, [0; 32], [0; 32]],
                &[oracle_account.expect("hybrid oracle fixture")],
                1,
                0,
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
        | AssetIntentKind::PushAuthMark
        | AssetIntentKind::ConfigureAuthMark
        | AssetIntentKind::ConfigureEwmaMark
        | AssetIntentKind::ConfigureHybridOracle => env
            .configure_auth_mark(false, asset_index, 4, INITIAL_PRICE)
            .map(|_| ())
            .map_err(|error| format!("configure replacement AuthMark: {error}")),
        AssetIntentKind::PushEwmaMark => env
            .configure_ewma_mark(asset_index, 4, INITIAL_PRICE, 1, 0)
            .map(|_| ())
            .map_err(|error| format!("configure replacement EwmaMark: {error}")),
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
            })
        }
        Err(_) => {
            if before != after {
                return Err(format!(
                    "{kind:?} rejected stale asset transaction did not roll back exactly"
                ));
            }
            Ok(AssetGenerationDiscovery {
                kind,
                old_asset_id,
                new_asset_id,
                accepted_stale_intent: false,
                mutated_economic_state: false,
                compute_units: None,
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

    if kind == RetryIntentKind::AssetActivation {
        env.warp_to_slot(4);
        env.retire_asset(1, 4)
            .map_err(|error| format!("retire first activated generation: {error}"))?;
        env.warp_to_slot(5);
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

fn finish_fee_consent_discovery(
    env: &V16Svm,
    kind: FeeConsentKind,
    before: EconomicFingerprint,
    execution: Result<u64, String>,
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
                accepted_unconsented_terms: true,
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

    if kind == FeeConsentKind::LiveBaseFeeHike {
        env.trade_no_cpi(TAKER, LP, 0, size_q, INITIAL_PRICE, 0)
            .map_err(|error| format!("open under original live fee: {error}"))?;
        let capital_before = total_capital(&env, &[TAKER, LP])?;
        let before = fingerprint(&env);
        let update = env.update_trade_fee_policy(BASE_FEE_BPS);
        let execution = match update {
            Ok(update) => env
                .trade_no_cpi(TAKER, LP, 0, -size_q, INITIAL_PRICE, 0)
                .map(|close| update.compute_units.max(close.compute_units)),
            Err(error) => Err(error),
        };
        let observed_debit = debit_between(
            capital_before,
            total_capital(&env, &[TAKER, LP])?,
            "live base-fee hike",
        )?;
        return finish_fee_consent_discovery(
            &env,
            kind,
            before,
            execution,
            0,
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
                    size_q,
                    fee_bps: CALLER_FEE_BPS,
                    limit_price: 0,
                }],
            )
            .map(|success| success.compute_units),
        FeeConsentKind::LiveBaseFeeHike | FeeConsentKind::PermissionlessActivationFee => {
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
        CREATOR, ASSET, 3, 100, CREATOR, CREATOR, CREATOR, CREATOR,
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

fn crank_discovery_steps(
    env: &mut V16Svm,
    actor: usize,
    slot: u64,
    asset_index: u16,
) -> Result<(), String> {
    let oracle_accounts = env.primary_profile(asset_index as usize).oracle_leg_count;
    for step in 0..4 {
        env.crank(
            actor,
            slot,
            vec![CrankObservationHint {
                asset_index,
                oracle_accounts,
            }],
        )
        .map_err(|error| {
            format!("source-fee crank actor {actor} asset {asset_index} step {step}: {error}")
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
    for (actor, asset_index) in [
        (MARKET_TRADER, ASSET),
        (LP, ASSET),
        (MARKET_TRADER, 0),
        (LP, 0),
    ] {
        crank_discovery_steps(&mut env, actor, 4, asset_index)?;
    }
    env.sync_maintenance_fee(LP, 4)
        .map_err(|error| format!("sync source-backed LP maintenance fee: {error}"))?;

    env.warp_to_slot(5);
    env.push_auth_mark_for_actor(PROVIDER, ASSET, 5, PRICE + 5)
        .map_err(|error| format!("publish source-backed winning mark: {error}"))?;
    env.push_auth_mark(0, 5, PRICE - 5)
        .map_err(|error| format!("publish offsetting losing mark: {error}"))?;
    for (actor, asset_index) in [(MARKET_TRADER, ASSET), (LP, ASSET), (MARKET_TRADER, 0)] {
        crank_discovery_steps(&mut env, actor, 5, asset_index)?;
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
    for (actor, asset_index) in [
        (MARKET_TRADER, ASSET),
        (LP, ASSET),
        (MARKET_TRADER, 0),
        (LP, 0),
    ] {
        crank_discovery_steps(&mut env, actor, 2, asset_index)?;
    }
    env.sync_maintenance_fee(LP, 2)
        .map_err(|error| format!("sync provider-backed LP fee: {error}"))?;
    env.warp_to_slot(3);
    env.push_auth_mark_for_actor(POLICY_AUTHORITY, ASSET, 3, PRICE + 5)
        .map_err(|error| format!("publish provider-backed winning mark: {error}"))?;
    env.push_auth_mark(0, 3, PRICE - 5)
        .map_err(|error| format!("publish provider-backed losing mark: {error}"))?;
    for (actor, asset_index) in [(MARKET_TRADER, ASSET), (LP, ASSET), (MARKET_TRADER, 0)] {
        crank_discovery_steps(&mut env, actor, 3, asset_index)?;
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

fn prime_zero_move_funding_discovery(env: &mut V16Svm) -> Result<(), String> {
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
    env.warp_to_slot(3);
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
) -> Result<AccrualOrderingWorld, String> {
    const PRICE: u64 = 2;
    const Q: i128 = 100 * POS_SCALE as i128;
    let mut env = zero_move_funding_discovery_world(seed)?;
    let supply_before = env.token_supply_observed();
    execute_cpi_close_kind(&mut env, kind, -Q)?;
    prime_zero_move_funding_discovery(&mut env)?;
    if !action_before_settlement {
        for actor in [0, 1] {
            env.crank(actor, 3, zero_move_observation_discovery(&env))
                .map_err(|error| format!("settle trade actor {actor}: {error}"))?;
        }
    }
    execute_cpi_close_kind(&mut env, kind, Q)?;
    if action_before_settlement {
        for actor in [0, 1] {
            env.crank(actor, 3, zero_move_observation_discovery(&env))
                .map_err(|error| format!("late-settle trade actor {actor}: {error}"))?;
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
    })
}

fn run_rebalance_accrual_ordering_world(
    seed: [u8; 32],
    action_before_settlement: bool,
) -> Result<AccrualOrderingWorld, String> {
    const PRICE: u64 = 2;
    const Q: i128 = 100 * POS_SCALE as i128;
    let mut env = zero_move_funding_discovery_world(seed)?;
    let supply_before = env.token_supply_observed();
    env.trade_no_cpi(0, 1, 0, -Q, PRICE, 0)
        .map_err(|error| format!("open rebalance accrual pair: {error}"))?;
    prime_zero_move_funding_discovery(&mut env)?;
    if !action_before_settlement {
        for actor in [0, 1] {
            env.crank(actor, 3, zero_move_observation_discovery(&env))
                .map_err(|error| format!("settle rebalance actor {actor}: {error}"))?;
        }
    }
    env.rebalance_reduce(0, 0, Q.unsigned_abs())
        .map_err(|error| format!("unilateral accrual-boundary reduction: {error}"))?;
    env.crank(1, 3, zero_move_observation_discovery(&env))
        .map_err(|error| format!("settle rebalance counterparty: {error}"))?;
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
    })
}

fn run_forfeit_accrual_ordering_world(
    seed: [u8; 32],
    action_before_settlement: bool,
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
    prime_zero_move_funding_discovery(&mut env)?;
    if !action_before_settlement {
        for actor in 0..4 {
            env.crank(actor, 3, zero_move_observation_discovery(&env))
                .map_err(|error| format!("settle forfeit actor {actor}: {error}"))?;
        }
    }
    env.rebalance_reduce(2, 0, Q_WHALE.unsigned_abs())
        .map_err(|error| format!("enter recovery side mode: {error}"))?;
    if env.primary_market_state().1.assets[0].mode_short != SideModeV16::DrainOnly {
        return Err("public reduction did not enter short-side DrainOnly".into());
    }
    env.forfeit_recovery_leg(0, 0, u128::from(u64::MAX))
        .map_err(|error| format!("forfeit accrual-boundary recovery leg: {error}"))?;
    env.crank(3, 3, zero_move_observation_discovery(&env))
        .map_err(|error| format!("settle forfeit counterparty: {error}"))?;
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
    })
}

fn discover_one_accrual_ordering_violation(
    mut seed: [u8; 32],
    kind: AccrualOrderingKind,
) -> Result<AccrualOrderingDiscovery, String> {
    seed[0] ^= 0x9d;
    seed[1] ^= kind.discriminator();
    let run = |action_before_settlement| match kind {
        AccrualOrderingKind::CpiTradeClose | AccrualOrderingKind::BatchCpiTradeClose => {
            run_trade_accrual_ordering_world(seed, kind, action_before_settlement)
        }
        AccrualOrderingKind::RebalanceReduce => {
            run_rebalance_accrual_ordering_world(seed, action_before_settlement)
        }
        AccrualOrderingKind::RecoveryForfeit => {
            run_forfeit_accrual_ordering_world(seed, action_before_settlement)
        }
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
        .map(|kind| discover_one_accrual_ordering_violation(seed, kind))
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
    env.resolve_stale_permissionless(RESOLVE_SLOT)
        .map_err(|error| format!("permissionless terminal resolve: {error}"))?;
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
        victim_payout_loss,
        counterparty_payout_gain,
        committed_total_payout,
        reordered_total_payout,
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
    match route {
        ProspectiveAccrualRoute::NoCpi => env
            .trade_no_cpi(taker, maker, 0, size_q, price, 0)
            .map(|_| ())
            .map_err(|error| format!("reported-price trade: {error}")),
        ProspectiveAccrualRoute::BatchNoCpi => env
            .batch_trade_no_cpi(
                taker,
                maker,
                vec![BatchTradeLeg {
                    asset_index: 0,
                    size_q,
                    exec_price: price,
                    fee_bps: 0,
                }],
            )
            .map(|_| ())
            .map_err(|error| format!("batch reported-price trade: {error}")),
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
        catchup(&mut env).map_err(|error| format!("trade-first prospective catch-up: {error}"))?;
    } else {
        catchup(&mut env).map_err(|error| format!("control prospective catch-up: {error}"))?;
        stamp(&mut env).map_err(|error| format!("control prospective stamp: {error}"))?;
    }
    let (profile_after, group_after) = env.primary_market_state();
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
    if control.final_mark != reordered.final_mark
        || control.final_effective_price != reordered.final_effective_price
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

    if fee_before_mark_commit {
        env.sync_maintenance_fee_with_reward(0, 2, 10)
            .map_err(|error| format!("early fee sync rejected: {error}"))?;
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
    if !fee_before_mark_commit {
        env.sync_maintenance_fee_with_reward(0, 2, 10)
            .map_err(|error| format!("mark-first fee sync rejected: {error}"))?;
    }

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
    if control.victim_payout != reordered.victim_payout || control_total != reordered_total {
        return Err("fee-order paired worlds did not conserve terminal value".into());
    }
    Ok(PendingMarkFeeOrderingDiscovery {
        control_reward: control.reward,
        reordered_reward: reordered.reward,
        control_winner_payout: control.winner_payout,
        reordered_winner_payout: reordered.winner_payout,
        victim_payout: reordered.victim_payout,
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
    execute_reported_price_route(&mut env, route, 2, 3, TINY_Q, reported_price)
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
    execute_reported_price_route(&mut env, route, 2, 3, -TINY_Q, queued_mark)
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
    for actor in 0..env.actors.len() {
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
    let coalition_profit = extracted_tokens
        .checked_sub(coalition_equity_before)
        .ok_or_else(|| "bilateral coalition did not extract above pre-equity".to_string())?;
    if env.token_supply_observed() != supply_before {
        return Err("bilateral mark-fee world changed SPL supply".into());
    }
    Ok(BilateralMarkFeeDiscovery {
        mode,
        route,
        setup_mark,
        queued_mark,
        coalition_profit,
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
