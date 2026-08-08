use super::v16_svm::{
    MarketConfig, TxSuccess, V16Svm, ASSET_COUNT, EXIT_MAKER_INDEX, PRIMARY_ACTOR_COUNT,
    TX_CU_LIMIT, USER_COUNT,
};
use percolator::{
    v16_domain_pair_for_asset_index, AssetLifecycleV16, BackingBucketStatusV16, MarketModeV16,
    PortfolioLegV16, SideModeV16, SideV16, BOUND_SCALE, CREDIT_RATE_SCALE,
    PORTFOLIO_SOURCE_DOMAIN_CAP, POS_SCALE,
};
use percolator_prog::{
    constants::{
        MARKET_GROUP_OFF, ORACLE_LEG_FLAG_DIVIDE_LEG2, ORACLE_LEG_FLAG_DIVIDE_LEG3,
        ORACLE_MODE_EWMA_MARK,
    },
    ix::{BatchTradeCpiLeg, BatchTradeLeg, CrankObservationHint},
    state::{MarketGroupV16, PortfolioAccountV16},
};
use proptest::prelude::*;
use serde::{Deserialize, Serialize};
use solana_sdk::{pubkey::Pubkey, signature::Signer, transaction::Transaction};
use std::collections::{BTreeSet, VecDeque};

const MIN_LIVENESS_DRAIN_LIMIT: usize = 256;
const MAX_LIVENESS_DRAIN_LIMIT: usize = 100_000;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum TradeRoute {
    NoCpi,
    Cpi,
    BatchNoCpi,
    BatchCpi,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum PortfolioIncarnationTradeSide {
    AccountA,
    AccountB,
}

impl TradeRoute {
    fn index(self) -> usize {
        match self {
            Self::NoCpi => 0,
            Self::Cpi => 1,
            Self::BatchNoCpi => 2,
            Self::BatchCpi => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum HintMode {
    Complete,
    Reversed,
    Empty,
    Duplicate,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SubstitutionKind {
    ForeignTradePortfolio,
    ForeignDepositVault,
    ForeignWithdrawVault,
    ForeignCrankPortfolio,
    MismatchedMatcherBinding,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnownBlocker {
    LiveLapsedSourceBacking,
    OmittedRescueAccrualLiquidation,
    PostExpiryBackingFee,
    TradeRetryReplay,
    AssetGenerationTradeReplay,
    CpiCallerFeeSiphon,
    CpiBackingFeeSiphon,
    CompositeOracleRounding,
    RoundedFundingOmission,
    PendingEwmaInheritance,
    ReclaimableEwmaFee,
    TradeFundingErasure,
    RebalanceFundingErasure,
    ForfeitFundingErasure,
    TradeDrivenLiquidationReward,
    CrossDomainBackingDoubleSpend,
    AssetGenerationMarkReplay,
    AssetGenerationConfigReplay,
    CrossDomainBSettlement,
    PendingEwmaTargetOverride,
    TerminalDustPayoutErasure,
    CrossMarginInsuranceDrain,
    CompositeOracleTimeSkew,
    UnstagedMarkTarget,
    PendingMarkFeeReward,
    FractionalCapSettlement,
    ProspectiveFundingRewrite,
    ResolveBeforeCommittedAccrual,
    BilateralFeeSupport,
    DelayedAssetAuthorityRevival,
    CollateralTopUpGenerationReplay,
    InsuranceWithdrawalGenerationReplay,
    InsuranceTopUpRetryReplay,
    BackingTopUpGenerationReplay,
    ActivationRetryReplay,
    BackingTopUpRetryReplay,
    WithdrawalRetryLiquidation,
    DepositRetryReplay,
    PortfolioIncarnationWithdrawal,
    PortfolioIncarnationDeposit,
    MarketIncarnationDeposit,
    ResolveGenerationReplay,
    ShutdownGenerationReplay,
    ActivationFeeConsent,
    BilateralBaseFeeConsent,
    MaintenancePolicyGenerationReplay,
    FeeRedirectGenerationReplay,
    BackingFeeGenerationReplay,
    LiquidationPolicyGenerationReplay,
    DelayedMaintenancePolicyReplay,
    DelayedLiquidationPolicyReplay,
    DelayedTradeFeePolicyReplay,
    DelayedFeeRedirectPolicyReplay,
    DelayedBackingFeePolicyReplay,
    DelayedOracleIntentReplay,
    BackingFeeConsentReplay,
    AuthorityHandoffAbaReplay,
    DelayedResolvePolicyReplay,
    ResolveAuthorityIncarnationReplay,
    PortfolioCloseIncarnationReplay,
    MatcherGrantPortfolioIncarnationReplay,
    TradePortfolioIncarnationReplay,
    ConvertPortfolioIncarnationReplay,
    ForfeitPortfolioIncarnationReplay,
    MatcherGrantMarketGenerationReplay,
    TradeFeeMarketGenerationReplay,
    ForfeitMarketGenerationReplay,
}

impl KnownBlocker {
    pub const COUNT: usize = 67;

    pub const fn index(self) -> usize {
        match self {
            Self::LiveLapsedSourceBacking => 0,
            Self::OmittedRescueAccrualLiquidation => 1,
            Self::PostExpiryBackingFee => 2,
            Self::TradeRetryReplay => 3,
            Self::AssetGenerationTradeReplay => 4,
            Self::CpiCallerFeeSiphon => 5,
            Self::CpiBackingFeeSiphon => 6,
            Self::CompositeOracleRounding => 7,
            Self::RoundedFundingOmission => 8,
            Self::PendingEwmaInheritance => 9,
            Self::ReclaimableEwmaFee => 10,
            Self::TradeFundingErasure => 11,
            Self::RebalanceFundingErasure => 12,
            Self::ForfeitFundingErasure => 13,
            Self::TradeDrivenLiquidationReward => 14,
            Self::CrossDomainBackingDoubleSpend => 15,
            Self::AssetGenerationMarkReplay => 16,
            Self::AssetGenerationConfigReplay => 17,
            Self::CrossDomainBSettlement => 18,
            Self::PendingEwmaTargetOverride => 19,
            Self::TerminalDustPayoutErasure => 20,
            Self::CrossMarginInsuranceDrain => 21,
            Self::CompositeOracleTimeSkew => 22,
            Self::UnstagedMarkTarget => 23,
            Self::PendingMarkFeeReward => 24,
            Self::FractionalCapSettlement => 25,
            Self::ProspectiveFundingRewrite => 26,
            Self::ResolveBeforeCommittedAccrual => 27,
            Self::BilateralFeeSupport => 28,
            Self::DelayedAssetAuthorityRevival => 29,
            Self::CollateralTopUpGenerationReplay => 30,
            Self::InsuranceWithdrawalGenerationReplay => 31,
            Self::InsuranceTopUpRetryReplay => 32,
            Self::BackingTopUpGenerationReplay => 33,
            Self::ActivationRetryReplay => 34,
            Self::BackingTopUpRetryReplay => 35,
            Self::WithdrawalRetryLiquidation => 36,
            Self::DepositRetryReplay => 37,
            Self::PortfolioIncarnationWithdrawal => 38,
            Self::PortfolioIncarnationDeposit => 39,
            Self::MarketIncarnationDeposit => 40,
            Self::ResolveGenerationReplay => 41,
            Self::ShutdownGenerationReplay => 42,
            Self::ActivationFeeConsent => 43,
            Self::BilateralBaseFeeConsent => 44,
            Self::MaintenancePolicyGenerationReplay => 45,
            Self::FeeRedirectGenerationReplay => 46,
            Self::BackingFeeGenerationReplay => 47,
            Self::LiquidationPolicyGenerationReplay => 48,
            Self::DelayedMaintenancePolicyReplay => 49,
            Self::DelayedLiquidationPolicyReplay => 50,
            Self::DelayedTradeFeePolicyReplay => 51,
            Self::DelayedFeeRedirectPolicyReplay => 52,
            Self::DelayedBackingFeePolicyReplay => 53,
            Self::DelayedOracleIntentReplay => 54,
            Self::BackingFeeConsentReplay => 55,
            Self::AuthorityHandoffAbaReplay => 56,
            Self::DelayedResolvePolicyReplay => 57,
            Self::ResolveAuthorityIncarnationReplay => 58,
            Self::PortfolioCloseIncarnationReplay => 59,
            Self::MatcherGrantPortfolioIncarnationReplay => 60,
            Self::TradePortfolioIncarnationReplay => 61,
            Self::ConvertPortfolioIncarnationReplay => 62,
            Self::ForfeitPortfolioIncarnationReplay => 63,
            Self::MatcherGrantMarketGenerationReplay => 64,
            Self::TradeFeeMarketGenerationReplay => 65,
            Self::ForfeitMarketGenerationReplay => 66,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PostExpiryBackingCase {
    pub fee_bps: u16,
    pub expiry_offset: u8,
    pub mark_move_bps: u16,
    pub increase_divisor: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PostExpiryBackingReproduction {
    pub blocker: KnownBlocker,
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
pub struct OmittedRescueReproduction {
    pub blocker: KnownBlocker,
    pub omitted_rejected_nonprogress: bool,
    pub omitted_exact_rollback: bool,
    pub omitted_position_before_q: u128,
    pub omitted_position_after_q: u128,
    pub omitted_insurance_delta: u128,
    pub complete_position_after_q: u128,
    pub complete_liquidation_deficit: u128,
    pub complete_insurance_delta: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradeRetryReplayReproduction {
    pub blocker: KnownBlocker,
    pub route: TradeRoute,
    pub victim_extra_loss: u64,
    pub attacker_extra_payout: u64,
    pub control_total_payout: u128,
    pub replay_total_payout: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetGenerationReplayReproduction {
    pub blocker: KnownBlocker,
    pub route: TradeRoute,
    pub old_market_id: u64,
    pub new_market_id: u64,
    pub victim_loss: u64,
    pub attacker_payout: u64,
    pub total_payout: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpiCallerFeeProtection {
    pub blocker: KnownBlocker,
    pub route: TradeRoute,
    pub requested_fee_bps: u64,
    pub max_trade_cu: u64,
    pub attacker_profit: u64,
    pub lp_loss: u64,
    pub withdrawable_insurance: u128,
    pub insurance_withdraw_rejected: bool,
    pub rejected_exact_rollback: bool,
    pub total_payout: u128,
    pub token_supply_conserved: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpiBaseFeeConsentProtection {
    pub blocker: KnownBlocker,
    pub route: TradeRoute,
    pub rejecting_cap_bps: u16,
    pub installed_fee_bps: u64,
    pub invalid_cap_rejected: bool,
    pub invalid_cap_exact_rollback: bool,
    pub stale_fill_rejected: bool,
    pub stale_fill_exact_rollback: bool,
    pub position_epoch_preserved: bool,
    pub unconsented_lp_loss: u64,
    pub unconsented_insurance_delta: u128,
    pub consented_cap_bps: u16,
    pub consented_lp_fee: u64,
    pub consented_insurance_fee: u64,
    pub total_payout: u128,
    pub open_cu: u64,
    pub close_cu: u64,
    pub max_route_cu: u64,
    pub token_supply_conserved: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpiBackingFeeProtection {
    pub blocker: KnownBlocker,
    pub matcher_cap_bps: u16,
    pub rejected_without_consent: bool,
    pub rejected_exact_rollback: bool,
    pub unconsented_provider_earnings: u128,
    pub lp_capital_loss: u128,
    pub provider_earnings: u128,
    pub extracted_tokens: u64,
    pub attacker_capital_delta: i128,
    pub zero_cap_risk_reduction_landed: bool,
    pub max_route_cu: u64,
    pub token_supply_conserved: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum CompositeRoundingCase {
    Pr329LargeMove,
    Pr381MicroMove,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum TargetStagingCase {
    AuthMarkPush,
    EwmaMarkPush,
    EwmaSingleTrade,
    EwmaBatchTrade,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum TradeDrivenLiquidationMode {
    Ewma,
    HybridAfterHours,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum BilateralFeeMode {
    Ewma,
    HybridAfterHours,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum AssetGenerationMarkPath {
    Auth,
    Ewma,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum AssetGenerationConfigPath {
    Auth,
    Ewma,
    Hybrid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelayedOracleIntentPath {
    PushAuth,
    ConfigureAuth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackingFeeConsentOrder {
    FundedThenPolicy,
    PolicyThenTopUp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthorityHandoffAbaPath {
    Market,
    AssetInsuranceOperator,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompositeRoundingReproduction {
    pub blocker: KnownBlocker,
    pub case: CompositeRoundingCase,
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
pub struct RoundedFundingOmissionReproduction {
    pub blocker: KnownBlocker,
    pub omitted_rejected_nonprogress: bool,
    pub omitted_exact_rollback: bool,
    pub control_f_long_num: i128,
    pub control_f_short_num: i128,
    pub attack_f_long_num: i128,
    pub attack_f_short_num: i128,
    pub victim_payout_loss: u64,
    pub attacker_payout_gain: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingEwmaInheritanceReproduction {
    pub blocker: KnownBlocker,
    pub route: TradeRoute,
    pub seed_cost: u128,
    pub victim_loss: u128,
    pub attacker_gain: u128,
    pub net_extracted_tokens: u64,
    pub pending_mark: u64,
    pub applied_mark: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReclaimableEwmaFeeReproduction {
    pub blocker: KnownBlocker,
    pub route: TradeRoute,
    pub fee_paid: u128,
    pub fee_reclaimed: u128,
    pub victim_loss: u128,
    pub attacker_gain: u128,
    pub effective_mark: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradeFundingErasureReproduction {
    pub blocker: KnownBlocker,
    pub route: TradeRoute,
    pub control_f_long_num: i128,
    pub control_f_short_num: i128,
    pub attack_f_long_num: i128,
    pub attack_f_short_num: i128,
    pub victim_payout_loss: u64,
    pub attacker_payout_gain: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RebalanceFundingErasureReproduction {
    pub blocker: KnownBlocker,
    pub control_attacker_paid: u128,
    pub control_victim_received: u128,
    pub attack_attacker_paid: u128,
    pub attack_victim_received: u128,
    pub victim_claim_loss: u128,
    pub attacker_payout_gain: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForfeitFundingErasureReproduction {
    pub blocker: KnownBlocker,
    pub control_attacker_paid: u128,
    pub control_victim_received: u128,
    pub attack_attacker_paid: u128,
    pub attack_victim_received: u128,
    pub victim_claim_loss: i128,
    pub attacker_payout_gain: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradeDrivenLiquidationRewardReproduction {
    pub blocker: KnownBlocker,
    pub mode: TradeDrivenLiquidationMode,
    pub route: TradeRoute,
    pub movement_fee: u128,
    pub victim_penalty: u128,
    pub cranker_reward: u128,
    pub victim_capital_loss: u128,
    pub attacker_extracted: u128,
    pub attacker_profit: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrossDomainBackingDoubleSpendReproduction {
    pub blocker: KnownBlocker,
    pub unfunded_claim_before_num: u128,
    pub funded_claim_before_num: u128,
    pub funded_backing_consumed_num: u128,
    pub winner_capital_gain: u128,
    pub extracted_tokens: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetGenerationMarkReplayReproduction {
    pub blocker: KnownBlocker,
    pub path: AssetGenerationMarkPath,
    pub old_market_id: u64,
    pub new_market_id: u64,
    pub landed_mark: u64,
    pub victim_equity_loss: u128,
    pub beneficiary_extra_payout: u64,
    pub observed_token_supply: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetGenerationConfigReplayReproduction {
    pub blocker: KnownBlocker,
    pub path: AssetGenerationConfigPath,
    pub old_market_id: u64,
    pub new_market_id: u64,
    pub stale_entry_price: u64,
    pub restored_mark: u64,
    pub victim_equity_loss: u128,
    pub beneficiary_extra_payout: u64,
    pub observed_token_supply: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrossDomainBSettlementReproduction {
    pub blocker: KnownBlocker,
    pub b_target_num: u128,
    pub pnl_loss: u128,
    pub unfunded_claim_before_num: u128,
    pub unfunded_claim_after_num: u128,
    pub funded_claim_before_num: u128,
    pub funded_claim_after_num: u128,
    pub wrong_domain_reduction_num: u128,
    pub correct_domain_reduction_num: u128,
    pub reduction_steps: u8,
    pub affected_position_after_q: i128,
    pub principal_withdrawn: u128,
    pub token_supply_conserved: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingEwmaTargetOverrideReproduction {
    pub blocker: KnownBlocker,
    pub route: TradeRoute,
    pub low_price: u64,
    pub control_target: u64,
    pub attack_target: u64,
    pub movement_fee: u128,
    pub displaced_victim_pnl: u128,
    pub attacker_profit: u128,
    pub victim_withdrawn: u64,
    pub attacker_withdrawn: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalDustPayoutErasureReproduction {
    pub blocker: KnownBlocker,
    pub route: TradeRoute,
    pub attacker_loss: u128,
    pub victim_loss: u128,
    pub vault_remaining: u128,
    pub victim_withdrawn: u128,
    pub attacker_withdrawn: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrossMarginInsuranceDrainReproduction {
    pub blocker: KnownBlocker,
    pub unrelated_insurance_spent: u128,
    pub attacker_payout: u128,
    pub attacker_profit: u128,
    pub liquidation_calls: u16,
    pub loser_close_calls: u16,
    pub counterparty_close_calls: u16,
    pub winner_close_calls: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompositeOracleTimeSkewReproduction {
    pub blocker: KnownBlocker,
    pub coherent_price: u64,
    pub skewed_target: u64,
    pub skewed_mark: u64,
    pub victim_capital_loss: u128,
    pub oi_reduction_q: u128,
    pub cranker_reward: u128,
    pub extracted_tokens: u64,
    pub max_crank_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetStagingReproduction {
    pub blocker: KnownBlocker,
    pub case: TargetStagingCase,
    pub wrapper_target: u64,
    pub stale_engine_target: u64,
    pub moved_engine_mark: u64,
    pub attacker_profit: u128,
    pub victim_capital_loss: u128,
    pub attacker_withdrawn: u64,
    pub attack_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingMarkFeeRewardReproduction {
    pub blocker: KnownBlocker,
    pub pending_sync_rejected_lock: bool,
    pub pending_sync_exact_rollback: bool,
    pub control_reward: u64,
    pub reordered_reward: u64,
    pub control_winner_payout: u64,
    pub reordered_winner_payout: u64,
    pub victim_payout: u64,
    pub extracted_reward: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FractionalCapSettlementReproduction {
    pub blocker: KnownBlocker,
    pub target_price: u64,
    pub stalled_price: u64,
    pub successful_cranks: u16,
    pub rollback_stalls: u8,
    pub long_payout: u64,
    pub short_payout: u64,
    pub long_overpayment: u64,
    pub short_underpayment: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProspectiveFundingRewriteReproduction {
    pub blocker: KnownBlocker,
    pub route: TradeRoute,
    pub control_f_short_num: i128,
    pub attack_f_short_num: i128,
    pub stamp_fee: u128,
    pub final_mark: u64,
    pub final_effective_price: u64,
    pub victim_payout_loss: u128,
    pub attacker_coalition_gain: u128,
    pub control_total_payout: u128,
    pub attack_total_payout: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolveBeforeCommittedAccrualReproduction {
    pub blocker: KnownBlocker,
    pub control_mark: u64,
    pub attack_mark: u64,
    pub unsafe_resolve_rejected: bool,
    pub rejected_exact_rollback: bool,
    pub victim_payout_loss: u64,
    pub attacker_payout_gain: u64,
    pub control_total_payout: u128,
    pub attack_total_payout: u128,
    pub catchup_steps: u16,
    pub catchup_cu: u64,
    pub attack_resolve_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BilateralFeeSupportReproduction {
    pub blocker: KnownBlocker,
    pub mode: BilateralFeeMode,
    pub route: TradeRoute,
    pub setup_mark: u64,
    pub queued_mark: u64,
    pub coalition_equity_before: u128,
    pub coalition_excess: u128,
    pub victim_loss: u128,
    pub fee_lp_loss: u128,
    pub insurance_gain: u128,
    pub extracted_tokens: u128,
    pub max_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelayedAssetAuthorityRevivalReproduction {
    pub blocker: KnownBlocker,
    pub provider_loss: u64,
    pub attacker_extraction: u64,
    pub funded_reserve: u128,
    pub reserve_after: u128,
    pub handoff_cu: u64,
    pub withdrawal_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CollateralTopUpGenerationReplayReproduction {
    pub blocker: KnownBlocker,
    pub old_market_id: u64,
    pub new_market_id: u64,
    pub victim_loss: u64,
    pub attacker_extraction: u64,
    pub replay_cu: u64,
    pub withdrawal_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InsuranceWithdrawalGenerationReplayReproduction {
    pub blocker: KnownBlocker,
    pub old_market_id: u64,
    pub new_market_id: u64,
    pub replacement_provider_loss: u64,
    pub attacker_extraction: u64,
    pub replay_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InsuranceTopUpRetryReplayReproduction {
    pub blocker: KnownBlocker,
    pub intended_contribution: u64,
    pub duplicate_loss: u64,
    pub operator_extraction: u64,
    pub insured_remainder: u128,
    pub first_cu: u64,
    pub replay_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackingTopUpGenerationReplayReproduction {
    pub blocker: KnownBlocker,
    pub old_market_id: u64,
    pub new_market_id: u64,
    pub provider_loss: u64,
    pub attacker_profit: u128,
    pub attacker_payout: u128,
    pub replay_cu: u64,
    pub max_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActivationRetryReplayReproduction {
    pub blocker: KnownBlocker,
    pub first_market_id: u64,
    pub replay_market_id: u64,
    pub intended_fee: u64,
    pub duplicate_loss: u64,
    pub beneficiary_extraction: u64,
    pub insured_remainder: u128,
    pub replay_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActivationFeeConsentProtection {
    pub blocker: KnownBlocker,
    pub signed_max_fee: u64,
    pub installed_unauthorized_fee: u64,
    pub stale_policy_rejected: bool,
    pub rejected_exact_rollback: bool,
    pub unconsented_creator_loss: u64,
    pub unconsented_insurance_delta: u128,
    pub consented_max_fee: u64,
    pub current_fee: u64,
    pub charged_fee: u64,
    pub insured_fee: u128,
    pub asset_active: bool,
    pub activation_cu: u64,
    pub token_supply_conserved: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BilateralBaseFeeConsentProtection {
    pub blocker: KnownBlocker,
    pub route: TradeRoute,
    pub signed_fee_bps: u64,
    pub installed_fee_bps: u64,
    pub stale_open_rejected: bool,
    pub stale_close_rejected: bool,
    pub rejected_exact_rollback: bool,
    pub unconsented_victim_loss: u64,
    pub unconsented_insurance_delta: u128,
    pub consented_victim_fee: u64,
    pub consented_insurance_fee: u64,
    pub total_payout: u128,
    pub open_cu: u64,
    pub close_cu: u64,
    pub token_supply_conserved: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaintenancePolicyGenerationReplayReproduction {
    pub blocker: KnownBlocker,
    pub old_asset_market_id: u64,
    pub new_asset_market_id: u64,
    pub victim_loss: u64,
    pub attacker_extraction: u64,
    pub live_oi_q: u128,
    pub replay_cu: u64,
    pub sync_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiquidationPolicyGenerationReplayReproduction {
    pub blocker: KnownBlocker,
    pub old_asset_market_id: u64,
    pub new_asset_market_id: u64,
    pub victim_capital_loss: u64,
    pub attacker_extraction: u64,
    pub insurance_delta: u128,
    pub live_oi_q: u128,
    pub replay_cu: u64,
    pub liquidation_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelayedMaintenancePolicyReplayReproduction {
    pub blocker: KnownBlocker,
    pub victim_loss: u64,
    pub attacker_extraction: u64,
    pub insurance_delta: u128,
    pub live_oi_q: u128,
    pub correction_cu: u64,
    pub replay_cu: u64,
    pub sync_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelayedLiquidationPolicyReplayReproduction {
    pub blocker: KnownBlocker,
    pub victim_capital_loss: u64,
    pub attacker_extraction: u64,
    pub insurance_delta: u128,
    pub live_oi_q: u128,
    pub correction_cu: u64,
    pub replay_cu: u64,
    pub liquidation_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelayedTradeFeePolicyReplayProtection {
    pub blocker: KnownBlocker,
    pub stale_policy_landed: bool,
    pub stale_trade_rejected: bool,
    pub rejected_exact_rollback: bool,
    pub victim_loss: u64,
    pub attacker_profit: u64,
    pub extracted_fee: u64,
    pub correction_cu: u64,
    pub replay_cu: u64,
    pub trade_cu: u64,
    pub withdrawal_cu: u64,
    pub token_supply_conserved: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelayedFeeRedirectPolicyReplayReproduction {
    pub blocker: KnownBlocker,
    pub victim_loss: u64,
    pub attacker_profit: u64,
    pub extracted_fee: u64,
    pub correction_cu: u64,
    pub replay_cu: u64,
    pub trade_cu: u64,
    pub withdrawal_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelayedBackingFeePolicyReplayReproduction {
    pub blocker: KnownBlocker,
    pub victim_loss: u64,
    pub provider_extraction: u64,
    pub backing_earnings: u128,
    pub correction_cu: u64,
    pub replay_cu: u64,
    pub trade_cu: u64,
    pub withdrawal_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelayedOracleIntentReplayReproduction {
    pub blocker: KnownBlocker,
    pub path: DelayedOracleIntentPath,
    pub stale_mark: u64,
    pub restored_mark: u64,
    pub victim_loss: u64,
    pub beneficiary_gain: u64,
    pub replay_cu: u64,
    pub max_crank_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackingFeeConsentReplayReproduction {
    pub blocker: KnownBlocker,
    pub order: BackingFeeConsentOrder,
    pub provider_loss: u64,
    pub operator_gain: u64,
    pub charged_fee: u64,
    pub replay_cu: u64,
    pub trade_cu: u64,
    pub max_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuthorityHandoffAbaReplayReproduction {
    pub blocker: KnownBlocker,
    pub path: AuthorityHandoffAbaPath,
    pub attacker_extraction: u64,
    pub control_withdrawal_blocked: bool,
    pub reserve_before: u128,
    pub reserve_after: u128,
    pub replay_cu: u64,
    pub withdrawal_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelayedResolvePolicyReplayReproduction {
    pub blocker: KnownBlocker,
    pub victim_loss: u64,
    pub attacker_gain: u64,
    pub unsafe_resolve_rejected: bool,
    pub rejected_exact_rollback: bool,
    pub catchup_steps: u16,
    pub max_crank_cu: u64,
    pub replay_price: u64,
    pub control_price: u64,
    pub replay_cu: u64,
    pub resolve_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolveAuthorityIncarnationReplayReproduction {
    pub blocker: KnownBlocker,
    pub victim_loss: u64,
    pub winner_gain: u64,
    pub replay_price: u64,
    pub control_price: u64,
    pub replay_cu: u64,
    pub max_crank_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortfolioCloseIncarnationReplayReproduction {
    pub blocker: KnownBlocker,
    pub original_portfolio_id: u64,
    pub replacement_portfolio_id: u64,
    pub drained_lamports: u64,
    pub market_lamport_gain: u64,
    pub replay_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatcherGrantPortfolioIncarnationReplayProtection {
    pub blocker: KnownBlocker,
    pub original_portfolio_id: u64,
    pub replacement_portfolio_id: u64,
    pub stale_replay_rejected: bool,
    pub rejected_exact_rollback: bool,
    pub control_trade_blocked: bool,
    pub fresh_grant_landed: bool,
    pub fresh_round_trip_landed: bool,
    pub owner_exit_landed: bool,
    pub max_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatcherGrantMarketGenerationReplayReproduction {
    pub blocker: KnownBlocker,
    pub old_market_id: u64,
    pub new_market_id: u64,
    pub control_trade_blocked: bool,
    pub liquidation_slot: u64,
    pub cranker_reward: u128,
    pub extracted_reward: u64,
    pub replay_cu: u64,
    pub max_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradeFeeMarketGenerationReplayProtection {
    pub blocker: KnownBlocker,
    pub old_market_id: u64,
    pub new_market_id: u64,
    pub stale_policy_landed: bool,
    pub stale_trade_rejected: bool,
    pub rejected_exact_rollback: bool,
    pub recovery_trade_landed: bool,
    pub victim_loss: u64,
    pub attacker_profit: u64,
    pub extracted_fee: u64,
    pub replay_cu: u64,
    pub trade_cu: u64,
    pub max_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForfeitMarketGenerationReplayReproduction {
    pub blocker: KnownBlocker,
    pub old_market_id: u64,
    pub new_market_id: u64,
    pub victim_loss: u64,
    pub stranded_vault: u128,
    pub control_slab_closed: bool,
    pub replay_slab_blocked: bool,
    pub replay_cu: u64,
    pub max_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TradePortfolioIncarnationReplayReproduction {
    pub blocker: KnownBlocker,
    pub route: TradeRoute,
    pub replacement_side: PortfolioIncarnationTradeSide,
    pub original_portfolio_id: u64,
    pub replacement_portfolio_id: u64,
    pub control_position_q: i128,
    pub liquidation_slot: u64,
    pub cranker_reward: u128,
    pub extracted_reward: u64,
    pub replay_cu: u64,
    pub max_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConvertPortfolioIncarnationReplayReproduction {
    pub blocker: KnownBlocker,
    pub original_portfolio_id: u64,
    pub replacement_portfolio_id: u64,
    pub released_pnl: u128,
    pub victim_loss: u64,
    pub cranker_extraction: u64,
    pub replay_cu: u64,
    pub sync_cu: u64,
    pub max_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForfeitPortfolioIncarnationReplayReproduction {
    pub blocker: KnownBlocker,
    pub original_portfolio_id: u64,
    pub replacement_portfolio_id: u64,
    pub stale_replay_rejected: bool,
    pub rejected_exact_rollback: bool,
    pub control_victim_payout: u64,
    pub replay_victim_payout: u64,
    pub control_attacker_payout: u64,
    pub replay_attacker_payout: u64,
    pub control_slab_closed: bool,
    pub replay_slab_closed: bool,
    pub max_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeeRedirectGenerationReplayReproduction {
    pub blocker: KnownBlocker,
    pub old_market_id: u64,
    pub new_market_id: u64,
    pub victim_loss: u64,
    pub attacker_profit: u64,
    pub redirected_fee: u128,
    pub replay_cu: u64,
    pub trade_cu: u64,
    pub withdrawal_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackingFeeGenerationReplayReproduction {
    pub blocker: KnownBlocker,
    pub old_market_id: u64,
    pub new_market_id: u64,
    pub victim_loss: u64,
    pub attacker_extraction: u64,
    pub backing_earnings: u128,
    pub replay_cu: u64,
    pub trade_cu: u64,
    pub withdrawal_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackingTopUpRetryReplayReproduction {
    pub blocker: KnownBlocker,
    pub intended_contribution: u64,
    pub duplicate_loss: u64,
    pub beneficiary_extra_payout: u128,
    pub control_winner_payout: u128,
    pub replay_winner_payout: u128,
    pub replay_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WithdrawalRetryLiquidationReproduction {
    pub blocker: KnownBlocker,
    pub intended_withdrawal: u64,
    pub duplicate_withdrawal: u64,
    pub liquidation_slot: u64,
    pub restored_equity_surplus: i128,
    pub cranker_reward: u128,
    pub extracted_reward: u64,
    pub replay_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DepositRetryReplayReproduction {
    pub blocker: KnownBlocker,
    pub intended_contribution: u64,
    pub duplicate_loss: u64,
    pub beneficiary_extra_payout: u128,
    pub control_winner_payout: u128,
    pub replay_winner_payout: u128,
    pub replay_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortfolioIncarnationWithdrawalReproduction {
    pub blocker: KnownBlocker,
    pub old_portfolio_id: u64,
    pub new_portfolio_id: u64,
    pub stale_withdrawal: u64,
    pub liquidation_slot: u64,
    pub restored_equity_surplus: i128,
    pub cranker_reward: u128,
    pub extracted_reward: u64,
    pub replay_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortfolioIncarnationDepositReproduction {
    pub blocker: KnownBlocker,
    pub old_portfolio_id: u64,
    pub new_portfolio_id: u64,
    pub stale_deposit: u64,
    pub beneficiary_extra_payout: u128,
    pub control_winner_payout: u128,
    pub replay_winner_payout: u128,
    pub replay_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MarketIncarnationDepositReproduction {
    pub blocker: KnownBlocker,
    pub old_asset_market_id: u64,
    pub new_asset_market_id: u64,
    pub stale_deposit: u64,
    pub beneficiary_extra_payout: u128,
    pub control_winner_payout: u128,
    pub replay_winner_payout: u128,
    pub replay_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolveGenerationReplayReproduction {
    pub blocker: KnownBlocker,
    pub old_market_id: u64,
    pub new_market_id: u64,
    pub victim_loss: u64,
    pub beneficiary_gain: u64,
    pub control_victim_payout: u128,
    pub replay_victim_payout: u128,
    pub control_winner_payout: u128,
    pub replay_winner_payout: u128,
    pub replay_cu: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShutdownGenerationReplayReproduction {
    pub blocker: KnownBlocker,
    pub old_market_id: u64,
    pub new_market_id: u64,
    pub victim_loss: u64,
    pub beneficiary_gain: u64,
    pub control_victim_payout: u128,
    pub replay_victim_payout: u128,
    pub control_winner_payout: u128,
    pub replay_winner_payout: u128,
    pub replay_cu: u64,
    pub force_close_cu: u64,
}

impl SubstitutionKind {
    const ALL: [Self; 5] = [
        Self::ForeignTradePortfolio,
        Self::ForeignDepositVault,
        Self::ForeignWithdrawVault,
        Self::ForeignCrankPortfolio,
        Self::MismatchedMatcherBinding,
    ];

    fn index(self) -> usize {
        match self {
            Self::ForeignTradePortfolio => 0,
            Self::ForeignDepositVault => 1,
            Self::ForeignWithdrawVault => 2,
            Self::ForeignCrankPortfolio => 3,
            Self::MismatchedMatcherBinding => 4,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Action {
    Trade {
        route: TradeRoute,
        taker: u8,
        maker: u8,
        asset: u8,
        units: i8,
        fee_bps: u16,
        price_move_bps: i16,
        prefer_reduce: bool,
    },
    ConfigureEwma {
        asset: u8,
        halflife_slots: u16,
        mark_min_fee: u16,
    },
    PushMark {
        asset: u8,
        dt: u8,
        move_bps: i16,
    },
    Crank {
        actor: u8,
        hints: HintMode,
    },
    Deposit {
        actor: u8,
        amount: u16,
    },
    Withdraw {
        actor: u8,
        amount: u16,
    },
    SyncMaintenanceFee {
        actor: u8,
        dt: u8,
    },
    SetMatcherConfig {
        actor: u8,
        enabled: bool,
        trade_fee_cap_bps: u16,
    },
    TopUpInsurance {
        domain: u8,
        amount: u16,
    },
    TopUpBacking {
        domain: u8,
        amount: u16,
        expiry_delta: u8,
    },
    ConvertReleasedPnl {
        actor: u8,
        amount: u16,
    },
    RebalanceReduce {
        actor: u8,
        asset: u8,
    },
    ConfigurePermissionlessResolve {
        stale_slots: u16,
        force_close_delay_slots: u16,
    },
    ShutdownAsset {
        asset: u8,
        dt: u8,
    },
    RotateOracleAuthority {
        asset: u8,
        new_actor: u8,
    },
    CrossMarketSubstitution {
        actor: u8,
    },
    AccountSubstitution {
        actor: u8,
        kind: SubstitutionKind,
    },
    RetainTrade {
        taker: u8,
        maker: u8,
        asset: u8,
        units: i8,
    },
    LandRetained,
    AdvanceBlockhash,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SmallMarketConfig {
    pub max_price_move_bps_per_slot: u64,
    pub max_accrual_dt_slots: u64,
    pub max_abs_funding_e9_per_slot: u64,
    pub maintenance_fee_per_slot: u128,
}

impl Default for SmallMarketConfig {
    fn default() -> Self {
        let config = MarketConfig::default();
        Self {
            max_price_move_bps_per_slot: config.max_price_move_bps_per_slot,
            max_accrual_dt_slots: config.max_accrual_dt_slots,
            max_abs_funding_e9_per_slot: config.max_abs_funding_e9_per_slot,
            maintenance_fee_per_slot: config.maintenance_fee_per_slot,
        }
    }
}

impl From<SmallMarketConfig> for MarketConfig {
    fn from(config: SmallMarketConfig) -> Self {
        Self {
            max_price_move_bps_per_slot: config.max_price_move_bps_per_slot,
            max_accrual_dt_slots: config.max_accrual_dt_slots,
            max_abs_funding_e9_per_slot: config.max_abs_funding_e9_per_slot,
            min_funding_lifetime_slots: config.max_accrual_dt_slots,
            maintenance_fee_per_slot: config.maintenance_fee_per_slot,
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Scenario {
    pub seed: [u8; 32],
    pub config: SmallMarketConfig,
    pub actions: Vec<Action>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Coverage {
    pub loaded_program_hash: [u8; 32],
    pub route_success: [u64; 4],
    pub route_reject: [u64; 4],
    pub crank_progress: u64,
    pub crank_rank_component_seen: [u64; 6],
    pub crank_rank_component_reduced: [u64; 6],
    pub crank_rank_nodes: BTreeSet<u8>,
    pub crank_rank_edges: BTreeSet<(u8, u8)>,
    pub mark_updates: u64,
    pub oracle_reconfigs: u64,
    pub maintenance_syncs: u64,
    pub extended_action_attempts: [u64; 8],
    pub matcher_config_updates: u64,
    pub insurance_topups: u64,
    pub backing_topups: u64,
    pub pnl_conversions: u64,
    pub rebalance_reductions: u64,
    pub resolve_policy_updates: u64,
    pub lifecycle_updates: u64,
    pub authority_updates: u64,
    pub deposits: u64,
    pub withdrawals: u64,
    pub token_frame_checks: u64,
    pub substitution_rejections: [u64; 5],
    pub retained_landed: u64,
    pub retained_rejected: u64,
    pub user_positions_closed: u64,
    pub liquidation_steps: u64,
    pub liquidated_abs_q: u128,
    pub known_blocker_hits: [u64; KnownBlocker::COUNT],
    pub known_blocker_exit_locks: [u64; KnownBlocker::COUNT],
    pub max_cu: u64,
}

impl Default for Coverage {
    fn default() -> Self {
        Self {
            loaded_program_hash: [0; 32],
            route_success: [0; 4],
            route_reject: [0; 4],
            crank_progress: 0,
            crank_rank_component_seen: [0; 6],
            crank_rank_component_reduced: [0; 6],
            crank_rank_nodes: BTreeSet::new(),
            crank_rank_edges: BTreeSet::new(),
            mark_updates: 0,
            oracle_reconfigs: 0,
            maintenance_syncs: 0,
            extended_action_attempts: [0; 8],
            matcher_config_updates: 0,
            insurance_topups: 0,
            backing_topups: 0,
            pnl_conversions: 0,
            rebalance_reductions: 0,
            resolve_policy_updates: 0,
            lifecycle_updates: 0,
            authority_updates: 0,
            deposits: 0,
            withdrawals: 0,
            token_frame_checks: 0,
            substitution_rejections: [0; 5],
            retained_landed: 0,
            retained_rejected: 0,
            user_positions_closed: 0,
            liquidation_steps: 0,
            liquidated_abs_q: 0,
            known_blocker_hits: [0; KnownBlocker::COUNT],
            known_blocker_exit_locks: [0; KnownBlocker::COUNT],
            max_cu: 0,
        }
    }
}

impl Coverage {
    pub fn assert_pull_request_non_vacuity(&self) -> Result<(), String> {
        if self.loaded_program_hash == [0; 32] {
            return Err("production SBF artifact hash was not recorded".into());
        }
        for (index, successes) in self.route_success.iter().copied().enumerate() {
            if successes == 0 {
                return Err(format!(
                    "trade route {index} had no successful public execution"
                ));
            }
        }
        if self.crank_progress == 0 {
            return Err("sole public crank never demonstrated rank-decreasing progress".into());
        }
        if self.deposits == 0 {
            return Err("owner-authorized deposit route had no successful public execution".into());
        }
        if self.token_frame_checks == 0 {
            return Err("successful public routes had no per-account SPL frame checks".into());
        }
        for (index, rejections) in self.substitution_rejections.iter().copied().enumerate() {
            if rejections == 0 {
                return Err(format!(
                    "public account-substitution boundary {index} was never rejected"
                ));
            }
        }
        if self.user_positions_closed == 0
            && self.known_blocker_exit_locks.iter().all(|hits| *hits == 0)
        {
            return Err("normal-user exit campaign closed no live position".into());
        }
        if self.liquidation_steps == 0 || self.liquidated_abs_q == 0 {
            return Err(
                "public crank never reduced a currently certified liquidatable position".into(),
            );
        }
        if self.max_cu > TX_CU_LIMIT {
            return Err(format!(
                "successful public instruction exceeded CU limit: {}",
                self.max_cu
            ));
        }
        Ok(())
    }

    fn observe_success(&mut self, route: Option<TradeRoute>, success: &TxSuccess) {
        if let Some(route) = route {
            self.route_success[route.index()] += 1;
        }
        self.max_cu = self.max_cu.max(success.compute_units);
    }

    fn merge(&mut self, other: Self) {
        if other.loaded_program_hash != [0; 32] {
            if self.loaded_program_hash == [0; 32] {
                self.loaded_program_hash = other.loaded_program_hash;
            } else {
                assert_eq!(
                    self.loaded_program_hash, other.loaded_program_hash,
                    "coverage cannot merge evidence from different SBF artifacts"
                );
            }
        }
        for index in 0..self.route_success.len() {
            self.route_success[index] += other.route_success[index];
            self.route_reject[index] += other.route_reject[index];
        }
        for index in 0..self.substitution_rejections.len() {
            self.substitution_rejections[index] += other.substitution_rejections[index];
        }
        self.crank_progress += other.crank_progress;
        for index in 0..self.crank_rank_component_seen.len() {
            self.crank_rank_component_seen[index] += other.crank_rank_component_seen[index];
            self.crank_rank_component_reduced[index] += other.crank_rank_component_reduced[index];
        }
        self.mark_updates += other.mark_updates;
        self.oracle_reconfigs += other.oracle_reconfigs;
        self.maintenance_syncs += other.maintenance_syncs;
        for (target, value) in self
            .extended_action_attempts
            .iter_mut()
            .zip(other.extended_action_attempts)
        {
            *target += value;
        }
        self.matcher_config_updates += other.matcher_config_updates;
        self.insurance_topups += other.insurance_topups;
        self.backing_topups += other.backing_topups;
        self.pnl_conversions += other.pnl_conversions;
        self.rebalance_reductions += other.rebalance_reductions;
        self.resolve_policy_updates += other.resolve_policy_updates;
        self.lifecycle_updates += other.lifecycle_updates;
        self.authority_updates += other.authority_updates;
        self.deposits += other.deposits;
        self.withdrawals += other.withdrawals;
        self.token_frame_checks += other.token_frame_checks;
        self.retained_landed += other.retained_landed;
        self.retained_rejected += other.retained_rejected;
        self.user_positions_closed += other.user_positions_closed;
        self.liquidation_steps += other.liquidation_steps;
        self.liquidated_abs_q += other.liquidated_abs_q;
        for (target, value) in self
            .known_blocker_hits
            .iter_mut()
            .zip(other.known_blocker_hits)
        {
            *target += value;
        }
        for (target, value) in self
            .known_blocker_exit_locks
            .iter_mut()
            .zip(other.known_blocker_exit_locks)
        {
            *target += value;
        }
        self.max_cu = self.max_cu.max(other.max_cu);
        self.crank_rank_nodes.extend(other.crank_rank_nodes);
        self.crank_rank_edges.extend(other.crank_rank_edges);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ProgressRank {
    market_mark_lag: u128,
    market_loss_lag: u128,
    market_locks: u128,
    b_work: u128,
    stale_legs: u128,
    health_work: u128,
}

impl ProgressRank {
    fn actionable(self) -> bool {
        self != Self::default()
    }

    fn account_actionable(self) -> bool {
        self.b_work != 0 || self.stale_legs != 0 || self.health_work != 0
    }

    fn reduced_from(self, before: Self) -> bool {
        (
            self.market_mark_lag,
            self.market_loss_lag,
            self.market_locks,
            self.b_work,
            self.stale_legs,
            self.health_work,
        ) < (
            before.market_mark_lag,
            before.market_loss_lag,
            before.market_locks,
            before.b_work,
            before.stale_legs,
            before.health_work,
        )
    }

    fn components(self) -> [u128; 6] {
        [
            self.market_mark_lag,
            self.market_loss_lag,
            self.market_locks,
            self.b_work,
            self.stale_legs,
            self.health_work,
        ]
    }

    fn class_mask(self) -> u8 {
        self.components()
            .into_iter()
            .enumerate()
            .fold(0u8, |mask, (index, value)| {
                mask | (u8::from(value != 0) << index)
            })
    }
}

fn add_mod_with_carry(lhs: u128, rhs: u128, modulus: u128) -> (u128, u128) {
    debug_assert!(modulus != 0 && lhs < modulus && rhs < modulus);
    let gap = modulus - rhs;
    if lhs >= gap {
        (1, lhs - gap)
    } else {
        (0, lhs + rhs)
    }
}

// Exact floor(lhs * rhs / denominator) without using the engine's U256 implementation or
// overflowing u128. INV-030 uses this as an independent persisted-state oracle.
fn reference_mul_div_floor(lhs: u128, rhs: u128, denominator: u128) -> Result<u128, String> {
    if denominator == 0 {
        return Err("source-credit reference division by zero".into());
    }
    let whole = lhs / denominator;
    let mut quotient = whole
        .checked_mul(rhs)
        .ok_or("source-credit reference quotient overflow")?;
    let reduced_lhs = lhs % denominator;
    let mut remainder = 0u128;
    let mut fractional = 0u128;
    for bit in (0..u128::BITS).rev() {
        fractional = fractional
            .checked_mul(2)
            .ok_or("source-credit reference fractional overflow")?;
        let (double_carry, doubled) = add_mod_with_carry(remainder, remainder, denominator);
        fractional = fractional
            .checked_add(double_carry)
            .ok_or("source-credit reference fractional carry overflow")?;
        remainder = doubled;
        if rhs & (1u128 << bit) != 0 {
            let (add_carry, next) = add_mod_with_carry(remainder, reduced_lhs, denominator);
            fractional = fractional
                .checked_add(add_carry)
                .ok_or("source-credit reference add carry overflow")?;
            remainder = next;
        }
    }
    quotient = quotient
        .checked_add(fractional)
        .ok_or("source-credit reference result overflow")?;
    Ok(quotient)
}

pub(crate) fn assert_source_credit_rates(
    label: &str,
    group: &MarketGroupV16,
) -> Result<(), String> {
    for (domain, source) in group.source_credit.iter().copied().enumerate() {
        if source.exact_positive_claim_num > source.positive_claim_bound_num {
            return Err(format!(
                "{label} source domain {domain} exact claim exceeds its bound"
            ));
        }
        if source.fresh_reserved_backing_num < source.valid_liened_backing_num {
            return Err(format!(
                "{label} source domain {domain} liens more counterparty backing than reserved"
            ));
        }
        let insurance_encumbered = source
            .valid_liened_insurance_num
            .checked_add(source.impaired_liened_insurance_num)
            .ok_or_else(|| format!("{label} source domain {domain} insurance lien overflow"))?;
        if source.insurance_credit_reserved_num < insurance_encumbered {
            return Err(format!(
                "{label} source domain {domain} liens more insurance than reserved"
            ));
        }
        let available = source
            .fresh_reserved_backing_num
            .checked_sub(source.valid_liened_backing_num)
            .and_then(|counterparty| {
                source
                    .insurance_credit_reserved_num
                    .checked_sub(insurance_encumbered)
                    .and_then(|insurance| counterparty.checked_add(insurance))
            })
            .ok_or_else(|| format!("{label} source domain {domain} available backing invalid"))?;
        let expected = if source.positive_claim_bound_num == 0
            || available >= source.positive_claim_bound_num
        {
            CREDIT_RATE_SCALE
        } else {
            reference_mul_div_floor(
                available,
                CREDIT_RATE_SCALE,
                source.positive_claim_bound_num,
            )?
            .min(CREDIT_RATE_SCALE)
        };
        if source.credit_rate_num != expected {
            return Err(format!(
                "{label} source domain {domain} persisted credit rate {} != independent \
                 {expected} (available={available}, claim_bound={})",
                source.credit_rate_num, source.positive_claim_bound_num
            ));
        }
    }
    Ok(())
}

fn assert_source_claim_bound_attribution(
    label: &str,
    group: &MarketGroupV16,
    portfolios: &[PortfolioAccountV16],
) -> Result<(), String> {
    let expected_portfolios = u64::try_from(portfolios.len())
        .map_err(|_| format!("{label} portfolio census does not fit u64"))?;
    if group.materialized_portfolio_count != expected_portfolios {
        return Err(format!(
            "{label} source-claim census is incomplete: market records {} materialized \
             portfolios, harness has {expected_portfolios}",
            group.materialized_portfolio_count
        ));
    }

    let mut attributed = vec![0u128; group.source_credit.len()];
    for (portfolio_index, portfolio) in portfolios.iter().enumerate() {
        for (slot, source) in portfolio.source_domains.iter().copied().enumerate() {
            if !source.is_occupied() {
                continue;
            }
            let domain = source.domain.get() as usize;
            let domain_sum = attributed.get_mut(domain).ok_or_else(|| {
                format!(
                    "{label} portfolio {portfolio_index} source slot {slot} names missing domain \
                     {domain}"
                )
            })?;
            *domain_sum = domain_sum
                .checked_add(source.source_claim_bound_num.get())
                .ok_or_else(|| {
                    format!("{label} source domain {domain} portfolio attribution overflow")
                })?;
        }
    }

    let mut domain_total = 0u128;
    for (domain, source) in group.source_credit.iter().copied().enumerate() {
        if attributed[domain] != source.positive_claim_bound_num {
            return Err(format!(
                "{label} source domain {domain} market claim bound {} != independent portfolio \
                 attribution {}",
                source.positive_claim_bound_num, attributed[domain]
            ));
        }
        domain_total = domain_total
            .checked_add(source.positive_claim_bound_num)
            .ok_or_else(|| format!("{label} aggregate source-claim bound overflow"))?;
    }
    if domain_total != group.source_claim_bound_total_num {
        return Err(format!(
            "{label} source-claim aggregate {} != market O(1) total {}",
            domain_total, group.source_claim_bound_total_num
        ));
    }
    Ok(())
}

fn assert_reservation_encumbrance_census(
    label: &str,
    group: &MarketGroupV16,
    portfolios: &[PortfolioAccountV16],
) -> Result<(), String> {
    let domain_count = group.source_credit.len();
    if group.source_backing_buckets.len() != domain_count
        || group.insurance_credit_reservations.len() != domain_count
    {
        return Err(format!(
            "{label}: source, bucket, and insurance-reservation domain counts differ"
        ));
    }
    let expected_portfolios = u64::try_from(portfolios.len())
        .map_err(|_| format!("{label}: encumbrance portfolio census does not fit u64"))?;
    if group.materialized_portfolio_count != expected_portfolios {
        return Err(format!(
            "{label}: encumbrance census has {} portfolios but market records {}",
            portfolios.len(),
            group.materialized_portfolio_count,
        ));
    }

    let mut account_counterparty_backing = vec![0u128; domain_count];
    let mut account_insurance_backing = vec![0u128; domain_count];
    let mut account_impaired_insurance_backing = vec![0u128; domain_count];
    for (portfolio_index, portfolio) in portfolios.iter().enumerate() {
        for (slot, account_source) in portfolio.source_domains.iter().copied().enumerate() {
            if !account_source.is_occupied() {
                continue;
            }
            let domain = account_source.domain.get() as usize;
            if domain >= domain_count {
                return Err(format!(
                    "{label}: portfolio {portfolio_index} source slot {slot} names missing domain {domain}"
                ));
            }
            let counterparty_face = account_source.source_claim_counterparty_liened_num.get();
            let insurance_face = account_source.source_claim_insurance_liened_num.get();
            let classified_face = counterparty_face
                .checked_add(insurance_face)
                .ok_or_else(|| format!("{label}: domain {domain} account lien-face overflow"))?;
            if classified_face != account_source.source_claim_liened_num.get() {
                return Err(format!(
                    "{label}: portfolio {portfolio_index} domain {domain} lien face is not singly classified"
                ));
            }
            let counterparty_backing = account_source.source_lien_counterparty_backing_num.get();
            let insurance_backing = account_source.source_lien_insurance_backing_num.get();
            let classified_backing = counterparty_backing
                .checked_add(insurance_backing)
                .ok_or_else(|| format!("{label}: domain {domain} account lien-backing overflow"))?;
            let expected_backing = account_source
                .source_lien_effective_reserved
                .get()
                .checked_mul(BOUND_SCALE)
                .ok_or_else(|| format!("{label}: domain {domain} effective lien overflow"))?;
            if classified_backing != expected_backing {
                return Err(format!(
                    "{label}: portfolio {portfolio_index} domain {domain} backing labels {classified_backing} != effective lien {expected_backing}"
                ));
            }
            checked_stock_add(
                &mut account_counterparty_backing[domain],
                counterparty_backing,
                &format!("{label} domain {domain} account counterparty liens"),
            )?;
            checked_stock_add(
                &mut account_insurance_backing[domain],
                insurance_backing,
                &format!("{label} domain {domain} account insurance liens"),
            )?;
            let impaired_insurance_backing = account_source
                .source_lien_impaired_effective_reserved
                .get()
                .checked_mul(BOUND_SCALE)
                .ok_or_else(|| {
                    format!("{label}: domain {domain} impaired effective lien overflow")
                })?;
            checked_stock_add(
                &mut account_impaired_insurance_backing[domain],
                impaired_insurance_backing,
                &format!("{label} domain {domain} account impaired insurance liens"),
            )?;
        }
    }

    for domain in 0..domain_count {
        let source = group.source_credit[domain];
        let bucket = group.source_backing_buckets[domain];
        let reservation = group.insurance_credit_reservations[domain];
        let bucket_fresh = bucket
            .fresh_unliened_backing_num
            .checked_add(bucket.valid_liened_backing_num)
            .ok_or_else(|| format!("{label}: domain {domain} fresh-backing overflow"))?;
        if source.fresh_reserved_backing_num != bucket_fresh
            || source.provider_receivable_num != bucket.consumed_liened_backing_num
            || source.spent_backing_num < source.provider_receivable_num
            || source.valid_liened_backing_num != bucket.valid_liened_backing_num
            || source.impaired_liened_backing_num != bucket.impaired_liened_backing_num
            || source.insurance_credit_reserved_num != reservation.insurance_credit_reserved_num
            || source.valid_liened_insurance_num != reservation.valid_liened_insurance_num
            || source.impaired_liened_insurance_num != reservation.impaired_liened_insurance_num
        {
            return Err(format!(
                "{label}: domain {domain} source/bucket/reservation encumbrance ledger diverged"
            ));
        }
        let market_counterparty_backing = source
            .valid_liened_backing_num
            .checked_add(source.impaired_liened_backing_num)
            .ok_or_else(|| format!("{label}: domain {domain} market counterparty lien overflow"))?;
        if account_counterparty_backing[domain] != market_counterparty_backing {
            return Err(format!(
                "{label}: domain {domain} account counterparty liens {} != market valid+impaired {}",
                account_counterparty_backing[domain], market_counterparty_backing,
            ));
        }
        if account_insurance_backing[domain] != source.valid_liened_insurance_num
            || account_impaired_insurance_backing[domain] != source.impaired_liened_insurance_num
        {
            return Err(format!(
                "{label}: domain {domain} account insurance lien lifecycle diverged from market valid/impaired ledgers"
            ));
        }
    }
    Ok(())
}

fn assert_primary_source_claim_bound_attribution(label: &str, env: &V16Svm) -> Result<(), String> {
    let (_, group) = env.primary_market_state();
    let portfolios: Vec<_> = (0..PRIMARY_ACTOR_COUNT)
        .map(|actor| env.primary_portfolio(actor))
        .collect();
    assert_source_credit_rates(label, &group)?;
    assert_source_claim_bound_attribution(label, &group, &portfolios)
}

pub fn assert_public_encumbrance_census(label: &str, env: &V16Svm) -> Result<(), String> {
    let (_, primary) = env.primary_market_state();
    let primary_portfolios = (0..env.actors.len())
        .map(|actor| env.primary_portfolio(actor))
        .collect::<Vec<_>>();
    assert_reservation_encumbrance_census(
        &format!("{label} primary"),
        &primary,
        &primary_portfolios,
    )?;

    let (_, foreign) = env.foreign_market_state();
    let foreign_portfolios = [env.foreign_portfolio()];
    assert_reservation_encumbrance_census(
        &format!("{label} foreign"),
        &foreign,
        &foreign_portfolios,
    )
}

fn checked_stock_add(total: &mut u128, value: u128, label: &str) -> Result<(), String> {
    *total = total
        .checked_add(value)
        .ok_or_else(|| format!("{label}: independent stock census overflow"))?;
    Ok(())
}

fn checked_count_add(total: &mut u64, value: u64, label: &str) -> Result<(), String> {
    *total = total
        .checked_add(value)
        .ok_or_else(|| format!("{label}: independent count census overflow"))?;
    Ok(())
}

fn decoded_flag(value: u8, label: &str, field: &str) -> Result<bool, String> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(format!(
            "{label}: portfolio has invalid {field} byte {value}"
        )),
    }
}

fn bound_num_to_atoms_ceil(value: u128, label: &str) -> Result<u128, String> {
    let whole = value / BOUND_SCALE;
    if value % BOUND_SCALE == 0 {
        Ok(whole)
    } else {
        whole
            .checked_add(1)
            .ok_or_else(|| format!("{label}: bound-to-atom ceiling overflow"))
    }
}

fn assert_market_stock_census(
    label: &str,
    group: &MarketGroupV16,
    market_data: &[u8],
    portfolios: &[PortfolioAccountV16],
    spl_vault_atoms: u128,
) -> Result<(), String> {
    let header_len = core::mem::size_of::<percolator::MarketGroupV16HeaderAccount>();
    let header_end = MARKET_GROUP_OFF
        .checked_add(header_len)
        .ok_or_else(|| format!("{label}: raw market-header range overflow"))?;
    let header_bytes = market_data
        .get(MARKET_GROUP_OFF..header_end)
        .ok_or_else(|| format!("{label}: raw market data is shorter than the engine header"))?;
    let header =
        bytemuck::pod_read_unaligned::<percolator::MarketGroupV16HeaderAccount>(header_bytes);

    let mut capital_total = 0u128;
    let mut positive_pnl_total = 0u128;
    let mut cancel_escrow_total = 0u128;
    let mut stale_count = 0u64;
    let mut b_stale_count = 0u64;
    let mut negative_pnl_count = 0u64;
    for (portfolio_index, portfolio) in portfolios.iter().enumerate() {
        checked_stock_add(
            &mut capital_total,
            portfolio.capital.get(),
            &format!("{label} portfolio {portfolio_index} capital"),
        )?;
        let pnl = portfolio.pnl.get();
        if pnl > 0 {
            checked_stock_add(
                &mut positive_pnl_total,
                pnl as u128,
                &format!("{label} portfolio {portfolio_index} positive PnL"),
            )?;
        } else if pnl < 0 {
            checked_count_add(
                &mut negative_pnl_count,
                1,
                &format!("{label} portfolio {portfolio_index} negative PnL"),
            )?;
        }
        checked_stock_add(
            &mut cancel_escrow_total,
            portfolio.cancel_deposit_escrow.get(),
            &format!("{label} portfolio {portfolio_index} cancel escrow"),
        )?;
        if decoded_flag(portfolio.stale_state, label, "stale_state")? {
            checked_count_add(&mut stale_count, 1, label)?;
        }
        if decoded_flag(portfolio.b_stale_state, label, "b_stale_state")? {
            checked_count_add(&mut b_stale_count, 1, label)?;
        }
    }

    let mut backing_provider_earnings = 0u128;
    for (domain, bucket) in group.source_backing_buckets.iter().enumerate() {
        checked_stock_add(
            &mut backing_provider_earnings,
            bucket.utilization_fee_earnings,
            &format!("{label} domain {domain} backing earnings"),
        )?;
    }

    let mut source_claim_bound_num = 0u128;
    let mut source_fresh_backing_num = 0u128;
    let mut source_insurance_reserved_atoms = 0u128;
    for (domain, source) in group.source_credit.iter().enumerate() {
        checked_stock_add(
            &mut source_claim_bound_num,
            source.positive_claim_bound_num,
            &format!("{label} domain {domain} source claim"),
        )?;
        checked_stock_add(
            &mut source_fresh_backing_num,
            source.fresh_reserved_backing_num,
            &format!("{label} domain {domain} fresh backing"),
        )?;
        checked_stock_add(
            &mut source_insurance_reserved_atoms,
            bound_num_to_atoms_ceil(
                source.insurance_credit_reserved_num,
                &format!("{label} domain {domain} insurance reservation"),
            )?,
            &format!("{label} domain {domain} insurance reservation"),
        )?;
    }

    let mut insurance_budget_remaining = 0u128;
    for (domain, (&budget, &spent)) in group
        .insurance_domain_budget
        .iter()
        .zip(&group.insurance_domain_spent)
        .enumerate()
    {
        let remaining = budget.checked_sub(spent).ok_or_else(|| {
            format!("{label}: domain {domain} insurance spend exceeds its budget")
        })?;
        checked_stock_add(
            &mut insurance_budget_remaining,
            remaining,
            &format!("{label} domain {domain} insurance budget"),
        )?;
    }

    let mut resolved_payout_blockers = 0u64;
    for (asset_index, asset) in group.assets.iter().enumerate() {
        for value in [
            asset.stored_pos_count_long,
            asset.stored_pos_count_short,
            asset.stale_account_count_long,
            asset.stale_account_count_short,
        ] {
            checked_count_add(&mut resolved_payout_blockers, value, label)?;
        }
        let (long_domain, short_domain) = v16_domain_pair_for_asset_index(asset_index)
            .map_err(|error| format!("{label}: asset {asset_index} domain mapping: {error:?}"))?;
        for domain in [long_domain, short_domain] {
            let barrier = *group
                .pending_domain_loss_barriers
                .get(domain)
                .ok_or_else(|| {
                    format!("{label}: asset {asset_index} is missing domain {domain} barrier")
                })?;
            checked_count_add(&mut resolved_payout_blockers, barrier, label)?;
        }
    }

    let materialized_count = u64::try_from(portfolios.len())
        .map_err(|_| format!("{label}: materialized portfolio count does not fit u64"))?;
    let aggregate_checks = [
        ("capital", capital_total, group.c_tot, header.c_tot.get()),
        (
            "positive PnL",
            positive_pnl_total,
            group.pnl_pos_tot,
            header.pnl_pos_tot.get(),
        ),
        (
            "backing earnings",
            backing_provider_earnings,
            group.backing_provider_earnings_total,
            header.backing_provider_earnings_total.get(),
        ),
        (
            "source claims",
            source_claim_bound_num,
            group.source_claim_bound_total_num,
            header.source_claim_bound_total_num.get(),
        ),
        (
            "source insurance reservations",
            source_insurance_reserved_atoms,
            group.source_insurance_credit_reserved_total_atoms,
            header.source_insurance_credit_reserved_total_atoms.get(),
        ),
        (
            "insurance budget remaining",
            insurance_budget_remaining,
            group.insurance_domain_budget_remaining_total,
            header.insurance_domain_budget_remaining_total.get(),
        ),
    ];
    for (name, independent, decoded, raw) in aggregate_checks {
        if independent != decoded || independent != raw {
            return Err(format!(
                "{label}: {name} aggregate mismatch: independent={independent}, decoded={decoded}, raw={raw}"
            ));
        }
    }
    if source_fresh_backing_num != header.source_fresh_backing_total_num.get() {
        return Err(format!(
            "{label}: fresh-backing aggregate mismatch: independent={source_fresh_backing_num}, raw={}",
            header.source_fresh_backing_total_num.get(),
        ));
    }

    let count_checks = [
        (
            "materialized portfolios",
            materialized_count,
            group.materialized_portfolio_count,
            header.materialized_portfolio_count.get(),
        ),
        (
            "stale certificates",
            stale_count,
            group.stale_certificate_count,
            header.stale_certificate_count.get(),
        ),
        (
            "B-stale accounts",
            b_stale_count,
            group.b_stale_account_count,
            header.b_stale_account_count.get(),
        ),
        (
            "negative-PnL accounts",
            negative_pnl_count,
            group.negative_pnl_account_count,
            header.negative_pnl_account_count.get(),
        ),
        (
            "resolved-payout blockers",
            resolved_payout_blockers,
            group.resolved_payout_blocker_count,
            header.resolved_payout_blocker_count.get(),
        ),
    ];
    for (name, independent, decoded, raw) in count_checks {
        if independent != decoded || independent != raw {
            return Err(format!(
                "{label}: {name} mismatch: independent={independent}, decoded={decoded}, raw={raw}"
            ));
        }
    }

    if group.vault != spl_vault_atoms || header.vault.get() != spl_vault_atoms {
        return Err(format!(
            "{label}: vault mismatch: SPL={spl_vault_atoms}, decoded={}, raw={}",
            group.vault,
            header.vault.get(),
        ));
    }
    let fresh_backing_atoms = source_fresh_backing_num / BOUND_SCALE;
    let explicit_stock = capital_total
        .checked_add(group.insurance)
        .and_then(|value| value.checked_add(backing_provider_earnings))
        .and_then(|value| value.checked_add(fresh_backing_atoms))
        .and_then(|value| value.checked_add(cancel_escrow_total))
        .ok_or_else(|| format!("{label}: explicit stock sum overflow"))?;
    let junior_residual = spl_vault_atoms.checked_sub(explicit_stock).ok_or_else(|| {
        format!(
            "{label}: explicit stocks exceed custody: capital={capital_total}, insurance={}, earnings={backing_provider_earnings}, backing={fresh_backing_atoms}, escrow={cancel_escrow_total}, vault={spl_vault_atoms}",
            group.insurance,
        )
    })?;
    if explicit_stock.checked_add(junior_residual) != Some(spl_vault_atoms) {
        return Err(format!("{label}: exact stock partition failed"));
    }
    Ok(())
}

pub fn assert_public_stock_census(label: &str, env: &V16Svm) -> Result<(), String> {
    let (_, primary) = env.primary_market_state();
    let primary_portfolios = (0..env.actors.len())
        .map(|actor| env.primary_portfolio(actor))
        .collect::<Vec<_>>();
    assert_market_stock_census(
        &format!("{label} primary"),
        &primary,
        &env.market_data(false),
        &primary_portfolios,
        u128::from(env.token_amount(env.vault)),
    )?;

    let (_, foreign) = env.foreign_market_state();
    let foreign_portfolios = [env.foreign_portfolio()];
    assert_market_stock_census(
        &format!("{label} foreign"),
        &foreign,
        &env.market_data(true),
        &foreign_portfolios,
        u128::from(env.token_amount(env.foreign_vault)),
    )
}

#[derive(Clone)]
struct Snapshot {
    primary_market: Vec<u8>,
    foreign_market: Vec<u8>,
    primary_portfolios: Vec<Vec<u8>>,
    foreign_portfolio: Vec<u8>,
    backing_domain_ledger: Vec<u8>,
    token_accounts: Vec<(Pubkey, Vec<u8>)>,
    matcher_contexts: Vec<Vec<u8>>,
    economic_account_lamports: Vec<(Pubkey, u64)>,
}

struct RetainedTrade {
    transaction: Transaction,
    taker: usize,
    maker: usize,
    legs: Vec<(usize, i128)>,
}

enum CrankFailure {
    Rejected(String),
    Invariant(String),
}

impl CrankFailure {
    fn into_message(self) -> String {
        match self {
            Self::Rejected(message) | Self::Invariant(message) => message,
        }
    }
}

pub struct ScenarioRunner {
    env: V16Svm,
    positions: [[i128; ASSET_COUNT]; PRIMARY_ACTOR_COUNT],
    protocol_positions: [i128; ASSET_COUNT],
    liveness_limit: usize,
    retained: VecDeque<RetainedTrade>,
    last_trade_rejection: Option<String>,
    pub coverage: Coverage,
}

impl ScenarioRunner {
    pub fn new(scenario: &Scenario) -> Result<Self, String> {
        let mut out = Self::new_unprefixed(scenario)?;
        out.run_required_prefix()?;
        Ok(out)
    }

    fn new_unprefixed(scenario: &Scenario) -> Result<Self, String> {
        let env = V16Svm::new(scenario.seed, scenario.config.into());
        let loaded_program_hash = env.loaded_program_hash.to_bytes();
        let out = Self {
            env,
            positions: [[0; ASSET_COUNT]; PRIMARY_ACTOR_COUNT],
            protocol_positions: [0; ASSET_COUNT],
            liveness_limit: scenario_liveness_limit(scenario)?,
            retained: VecDeque::new(),
            last_trade_rejection: None,
            coverage: Coverage {
                loaded_program_hash,
                ..Coverage::default()
            },
        };
        out.assert_global_invariants()?;
        Ok(out)
    }

    pub fn run_safety_prefix(&mut self, actions: &[Action]) -> Result<(), String> {
        for (index, action) in actions.iter().enumerate() {
            self.apply_action(action)
                .map_err(|error| format!("action {index} {action:?}: {error}"))?;
            self.assert_global_invariants()
                .map_err(|error| format!("after action {index} {action:?}: {error}"))?;
        }
        Ok(())
    }

    pub fn run_permissionless_progress_campaign(&mut self) -> Result<(), String> {
        self.drain_cranks(self.liveness_limit)?;
        self.assert_global_invariants()
    }

    pub fn quarantine_known_progress_blocker(&mut self, error: &str) -> Result<bool, String> {
        let (_, group) = self.env.primary_market_state();
        let lapsed_live_backing = group.mode == MarketModeV16::Live
            && group.source_backing_buckets.iter().any(|bucket| {
                bucket.status == BackingBucketStatusV16::Fresh
                    && bucket.expiry_slot <= group.current_slot
                    && (bucket.fresh_unliened_backing_num != 0
                        || bucket.valid_liened_backing_num != 0
                        || bucket.consumed_liened_backing_num != 0
                        || bucket.impaired_liened_backing_num != 0)
            });
        let stale_rejection =
            error.contains("Custom(19)") || error.contains("custom program error: 0x13");
        if !lapsed_live_backing || !stale_rejection {
            return Ok(false);
        }

        let before = self.snapshot();
        let retry = self.drain_one_progress_step(None);
        self.assert_snapshot_unchanged(&before)?;
        let retry_error = match retry {
            Ok(()) => {
                return Err(
                    "candidate PR 204 blocker progressed on an identical public retry".into(),
                )
            }
            Err(error) => error,
        };
        if !(retry_error.contains("Custom(19)")
            || retry_error.contains("custom program error: 0x13"))
        {
            return Err(format!(
                "candidate PR 204 blocker changed rejection on identical retry: {retry_error}"
            ));
        }
        self.coverage.known_blocker_hits[KnownBlocker::LiveLapsedSourceBacking.index()] += 1;
        Ok(true)
    }

    pub fn run_direct_user_exit_campaign(&mut self) -> Result<(), String> {
        for user in 0..PRIMARY_ACTOR_COUNT {
            for asset in 0..ASSET_COUNT {
                if self.positions[user][asset] == 0 {
                    continue;
                }
                if !self.try_normal_exit(user, asset)? {
                    self.run_permissionless_progress_campaign()
                        .map_err(|error| {
                            format!(
                            "normal exit needed public progress, but crank could not converge: \
                             {error}"
                        )
                        })?;
                }
                if !self.try_normal_exit(user, asset)? {
                    let size = self.positions[user][asset];
                    let legs = vec![(asset, -size)];
                    let diagnostics = self
                        .normal_exit_counterparties(user, asset, size)
                        .into_iter()
                        .map(|counterparty| {
                            self.trade_diagnostics(user, counterparty, &legs)
                                .map(|diagnostic| format!("maker {counterparty}: {diagnostic}"))
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .join("; ");
                    return Err(format!(
                        "all public trade routes rejected normal exit for user {user} asset \
                         {asset}; last rejection={:?}; {diagnostics}",
                        self.last_trade_rejection
                    ));
                }
                self.coverage.user_positions_closed += 1;
            }
        }
        self.assert_global_invariants()?;
        for actor in 0..PRIMARY_ACTOR_COUNT {
            for asset in 0..ASSET_COUNT {
                if self.positions[actor][asset] != 0 {
                    return Err(format!(
                        "exit campaign stranded actor {actor} asset {asset} position {}",
                        self.positions[actor][asset]
                    ));
                }
            }
        }

        for actor in 0..PRIMARY_ACTOR_COUNT {
            let capital = self.env.primary_portfolio(actor).capital.get();
            if capital == 0 {
                continue;
            }
            let before = self.snapshot();
            let vault_before = u128::from(self.env.token_amount(self.env.vault));
            let destination_before = self
                .env
                .token_amount(self.env.actors[actor].destination_token);
            let success = self
                .env
                .withdraw_primary(actor, capital)
                .map_err(|error| format!("actor {actor} cannot withdraw {capital}: {error}"))?;
            self.coverage.withdrawals += 1;
            self.coverage.observe_success(None, &success);
            self.assert_portfolio_frame(&before, &[actor])?;
            let destination_after = self
                .env
                .token_amount(self.env.actors[actor].destination_token);
            let vault_after = u128::from(self.env.token_amount(self.env.vault));
            if u128::from(destination_after).checked_sub(u128::from(destination_before))
                != Some(capital)
                || vault_after.checked_add(capital) != Some(vault_before)
            {
                return Err(format!(
                    "actor {actor} withdrawal vault/destination delta did not equal authorized debit"
                ));
            }
            self.assert_token_frame(
                &before,
                &[self.env.actors[actor].destination_token, self.env.vault],
            )?;
            if self.env.primary_portfolio(actor).capital.get() != 0 {
                return Err(format!(
                    "actor {actor} retained capital after full withdrawal"
                ));
            }
            self.assert_global_invariants()?;
        }
        let foreign_capital = self.env.foreign_market_state().1.c_tot;
        if foreign_capital != 0 {
            let before = self.snapshot();
            let vault_before = u128::from(self.env.token_amount(self.env.foreign_vault));
            let destination_before = u128::from(
                self.env
                    .token_amount(self.env.foreign_actor.destination_token),
            );
            let success = self
                .env
                .withdraw_foreign(foreign_capital)
                .map_err(|error| format!("foreign user cannot withdraw: {error}"))?;
            self.coverage.withdrawals += 1;
            self.coverage.observe_success(None, &success);
            let vault_after = u128::from(self.env.token_amount(self.env.foreign_vault));
            let destination_after = u128::from(
                self.env
                    .token_amount(self.env.foreign_actor.destination_token),
            );
            if vault_after.checked_add(foreign_capital) != Some(vault_before)
                || destination_before.checked_add(foreign_capital) != Some(destination_after)
            {
                return Err("foreign withdrawal did not preserve exact token deltas".into());
            }
            self.assert_token_frame(
                &before,
                &[
                    self.env.foreign_actor.destination_token,
                    self.env.foreign_vault,
                ],
            )?;
        }
        self.assert_global_invariants()
    }

    fn try_normal_exit(&mut self, user: usize, asset: usize) -> Result<bool, String> {
        let size = self.positions[user][asset];
        if size == 0 {
            return Ok(true);
        }
        let preferred = match (user * ASSET_COUNT + asset) % 4 {
            0 => TradeRoute::NoCpi,
            1 => TradeRoute::Cpi,
            2 => TradeRoute::BatchNoCpi,
            _ => TradeRoute::BatchCpi,
        };
        let legs = vec![(asset, -size)];
        let mut rejections = Vec::new();
        for counterparty in self.normal_exit_counterparties(user, asset, size) {
            for route in [
                preferred,
                TradeRoute::NoCpi,
                TradeRoute::Cpi,
                TradeRoute::BatchNoCpi,
                TradeRoute::BatchCpi,
            ] {
                if self.execute_trade(route, user, counterparty, legs.clone(), 0, 0, false)? {
                    return Ok(true);
                }
                if let Some(error) = self.last_trade_rejection.clone() {
                    rejections.push(error);
                }
            }
        }
        if self.try_rebalance_exit(user, asset, size)? {
            return Ok(true);
        }
        if let Some(error) = self.last_trade_rejection.clone() {
            rejections.push(error);
        }
        let size_after_rebalance = self.positions[user][asset];
        if size_after_rebalance == 0 {
            return Ok(true);
        }
        if self.try_dead_leg_forfeit_exit(user, asset, size_after_rebalance)? {
            return Ok(true);
        }
        if let Some(error) = self.last_trade_rejection.clone() {
            if rejections.last() != Some(&error) {
                rejections.push(error);
            }
        }
        self.last_trade_rejection = Some(rejections.join(" | "));
        Ok(false)
    }

    fn try_rebalance_exit(
        &mut self,
        user: usize,
        asset: usize,
        size_before: i128,
    ) -> Result<bool, String> {
        let before = self.snapshot();
        match self
            .env
            .rebalance_reduce(user, asset as u16, size_before.unsigned_abs())
        {
            Ok(success) => {
                self.coverage.observe_success(None, &success);
                self.assert_portfolio_frame(&before, &[user])?;
                self.assert_no_token_side_effects(&before)?;
                let size_after = decoded_legs(&self.env.primary_portfolio(user))
                    .into_iter()
                    .filter(|leg| leg.active && leg.asset_index as usize == asset)
                    .try_fold(0i128, |total, leg| {
                        total
                            .checked_add(leg.basis_pos_q)
                            .ok_or("rebalance exit position overflow")
                    })?;
                let user_delta = size_after
                    .checked_sub(size_before)
                    .ok_or("rebalance exit delta overflow")?;
                self.positions[user][asset] = size_after;
                self.protocol_positions[asset] = self.protocol_positions[asset]
                    .checked_sub(user_delta)
                    .ok_or("rebalance exit protocol attribution overflow")?;
                self.assert_positions_match()?;
                Ok(size_after == 0)
            }
            Err(error) => {
                self.last_trade_rejection = Some(format!(
                    "RebalanceReduce owner {user} asset {asset}: {error}"
                ));
                self.assert_snapshot_unchanged(&before)?;
                Ok(false)
            }
        }
    }

    fn try_dead_leg_forfeit_exit(
        &mut self,
        user: usize,
        asset: usize,
        size_before: i128,
    ) -> Result<bool, String> {
        let account = self.env.primary_portfolio(user);
        let Some(leg) = decoded_legs(&account)
            .into_iter()
            .find(|leg| leg.active && leg.asset_index as usize == asset)
        else {
            return Ok(false);
        };
        let (_, group) = self.env.primary_market_state();
        let engine_asset = &group.assets[asset];
        let side_mode = match leg.side {
            percolator::SideV16::Long => engine_asset.mode_long,
            percolator::SideV16::Short => engine_asset.mode_short,
        };
        let forfeit_enabled = group.mode == MarketModeV16::Recovery
            || engine_asset.lifecycle == AssetLifecycleV16::Recovery
            || matches!(
                side_mode,
                SideModeV16::DrainOnly | SideModeV16::ResetPending
            );
        if !forfeit_enabled {
            return Ok(false);
        }

        let before = self.snapshot();
        match self
            .env
            .forfeit_recovery_leg(user, asset as u16, u128::from(u64::MAX))
        {
            Ok(success) => {
                self.coverage.observe_success(None, &success);
                self.assert_portfolio_frame(&before, &[user])?;
                self.assert_no_token_side_effects(&before)?;
                let size_after = decoded_legs(&self.env.primary_portfolio(user))
                    .into_iter()
                    .filter(|leg| leg.active && leg.asset_index as usize == asset)
                    .try_fold(0i128, |total, leg| {
                        total
                            .checked_add(leg.basis_pos_q)
                            .ok_or("dead-leg forfeit position overflow")
                    })?;
                let user_delta = size_after
                    .checked_sub(size_before)
                    .ok_or("dead-leg forfeit delta overflow")?;
                self.positions[user][asset] = size_after;
                self.protocol_positions[asset] = self.protocol_positions[asset]
                    .checked_sub(user_delta)
                    .ok_or("dead-leg forfeit protocol attribution overflow")?;
                self.assert_positions_match()?;
                Ok(size_after == 0)
            }
            Err(error) => {
                self.last_trade_rejection = Some(format!(
                    "ForfeitRecoveryLeg owner {user} asset {asset}: {error}"
                ));
                self.assert_snapshot_unchanged(&before)?;
                Ok(false)
            }
        }
    }

    fn normal_exit_counterparties(&self, user: usize, asset: usize, size: i128) -> Vec<usize> {
        let mut counterparties: Vec<_> = (0..PRIMARY_ACTOR_COUNT)
            .filter(|candidate| *candidate != user)
            .filter(|candidate| {
                let candidate_size = self.positions[*candidate][asset];
                candidate_size != 0
                    && candidate_size.signum() == -size.signum()
                    && candidate_size.unsigned_abs() >= size.unsigned_abs()
            })
            .collect();
        if user != EXIT_MAKER_INDEX && !counterparties.contains(&EXIT_MAKER_INDEX) {
            counterparties.push(EXIT_MAKER_INDEX);
        }
        counterparties
    }

    fn run_required_prefix(&mut self) -> Result<(), String> {
        let q = POS_SCALE as i128;
        self.execute_deposit(0, 1, true)?;
        self.execute_trade(TradeRoute::NoCpi, 0, 1, vec![(0, q)], 0, 0, true)?;
        self.execute_trade(TradeRoute::Cpi, 2, 3, vec![(0, q)], 0, 0, true)?;
        self.execute_trade(
            TradeRoute::BatchNoCpi,
            0,
            1,
            vec![(0, q / 2), (1, -(q / 2)), (2, q / 4)],
            0,
            0,
            true,
        )?;
        self.execute_trade(
            TradeRoute::BatchCpi,
            2,
            3,
            vec![(0, q / 2), (1, -(q / 2)), (2, q / 4)],
            0,
            0,
            true,
        )?;

        for kind in SubstitutionKind::ALL {
            self.execute_account_substitution(0, kind)?;
        }

        let next_slot = self.env.current_slot() + 1;
        self.env.warp_to_slot(next_slot);
        let (_, group) = self.env.primary_market_state();
        let required_move_bps = group.config.max_price_move_bps_per_slot.clamp(1, 10);
        let required_mark = super::v16_svm::INITIAL_PRICE
            .checked_add(
                super::v16_svm::INITIAL_PRICE
                    .checked_mul(required_move_bps)
                    .ok_or("required mark multiplication overflow")?
                    / 10_000,
            )
            .ok_or("required mark overflow")?;
        let before = self.snapshot();
        let mark_success = self
            .env
            .push_auth_mark(0, next_slot, required_mark)
            .map_err(|error| format!("required mark update failed: {error}"))?;
        self.coverage.mark_updates += 1;
        self.coverage.observe_success(None, &mark_success);
        self.assert_no_token_side_effects(&before)?;
        self.drain_actor(0, self.liveness_limit)?;
        self.assert_global_invariants()
    }

    fn apply_action(&mut self, action: &Action) -> Result<(), String> {
        match *action {
            Action::Trade {
                route,
                taker,
                maker,
                asset,
                units,
                fee_bps,
                price_move_bps,
                prefer_reduce,
            } => {
                let taker = taker as usize % USER_COUNT;
                let mut maker = maker as usize % USER_COUNT;
                if maker == taker {
                    maker = (maker + 1) % USER_COUNT;
                }
                let asset = asset as usize % ASSET_COUNT;
                let existing = self.positions[taker][asset];
                let unit_size = (units as i128).clamp(-3, 3) * POS_SCALE as i128 / 4;
                let size = if prefer_reduce && existing != 0 {
                    -existing
                } else if unit_size == 0 {
                    POS_SCALE as i128 / 4
                } else {
                    unit_size
                };
                let legs = if matches!(route, TradeRoute::BatchNoCpi | TradeRoute::BatchCpi) {
                    let other = (asset + 1) % ASSET_COUNT;
                    let other_size = if prefer_reduce && self.positions[taker][other] != 0 {
                        -self.positions[taker][other]
                    } else {
                        -size
                    };
                    vec![(asset, size), (other, other_size)]
                } else {
                    vec![(asset, size)]
                };
                self.execute_trade(
                    route,
                    taker,
                    maker,
                    legs,
                    fee_bps as u64,
                    price_move_bps,
                    false,
                )
                .map(|_| ())
            }
            Action::ConfigureEwma {
                asset,
                halflife_slots,
                mark_min_fee,
            } => {
                let asset = asset as usize % ASSET_COUNT;
                let before = self.snapshot();
                let mark = self.env.primary_profile(asset).mark_ewma_e6;
                match self.env.configure_ewma_mark(
                    asset as u16,
                    self.env.current_slot(),
                    mark,
                    u64::from(halflife_slots.max(1)),
                    u64::from(mark_min_fee),
                ) {
                    Ok(success) => {
                        self.coverage.oracle_reconfigs += 1;
                        self.coverage.observe_success(None, &success);
                        self.assert_portfolio_frame(&before, &[])?;
                        self.assert_no_token_side_effects(&before)?;
                    }
                    Err(_) => self.assert_snapshot_unchanged(&before)?,
                }
                Ok(())
            }
            Action::PushMark {
                asset,
                dt,
                move_bps,
            } => {
                let asset = asset as usize % ASSET_COUNT;
                let next_slot = self.env.current_slot() + (dt.max(1) as u64).min(4);
                self.env.warp_to_slot(next_slot);
                let old_mark = self.env.primary_profile(asset).mark_ewma_e6;
                let move_bps = move_bps.clamp(-500, 500) as i128;
                let numerator = (old_mark as i128)
                    .checked_mul(10_000 + move_bps)
                    .ok_or("mark multiplication overflow")?;
                let new_mark = u64::try_from((numerator / 10_000).max(1))
                    .map_err(|_| "mark conversion overflow")?;
                let before = self.snapshot();
                let result = if self.env.primary_profile(asset).oracle_mode == ORACLE_MODE_EWMA_MARK
                {
                    self.env.push_ewma_mark(asset as u16, next_slot, new_mark)
                } else {
                    self.env.push_auth_mark(asset as u16, next_slot, new_mark)
                };
                match result {
                    Ok(success) => {
                        self.coverage.mark_updates += 1;
                        self.coverage.observe_success(None, &success);
                        self.assert_portfolio_frame(&before, &[])?;
                        self.assert_no_token_side_effects(&before)?;
                    }
                    Err(_) => self.assert_snapshot_unchanged(&before)?,
                }
                Ok(())
            }
            Action::Crank { actor, hints } => {
                let actor = actor as usize % PRIMARY_ACTOR_COUNT;
                self.execute_crank(actor, hints, matches!(hints, HintMode::Complete))
                    .map_err(CrankFailure::into_message)
            }
            Action::Deposit { actor, amount } => self
                .execute_deposit(
                    actor as usize % USER_COUNT,
                    u128::from(amount.max(1)),
                    false,
                )
                .map(|_| ()),
            Action::Withdraw { actor, amount } => {
                let actor = actor as usize % USER_COUNT;
                let amount = amount as u128;
                let before = self.snapshot();
                let capital_before = self.env.primary_portfolio(actor).capital.get();
                let before_vault = u128::from(self.env.token_amount(self.env.vault));
                let destination_before = self
                    .env
                    .token_amount(self.env.actors[actor].destination_token);
                match self.env.withdraw_primary(actor, amount) {
                    Ok(success) => {
                        self.coverage.withdrawals += 1;
                        self.coverage.observe_success(None, &success);
                        self.assert_portfolio_frame(&before, &[actor])?;
                        let capital_after = self.env.primary_portfolio(actor).capital.get();
                        let destination_after = self
                            .env
                            .token_amount(self.env.actors[actor].destination_token);
                        if capital_before.checked_sub(amount) != Some(capital_after)
                            || destination_after as u128 != destination_before as u128 + amount
                            || u128::from(self.env.token_amount(self.env.vault)).checked_add(amount)
                                != Some(before_vault)
                        {
                            return Err(
                                "withdrawal debit/credit did not match owner authorization".into(),
                            );
                        }
                        self.assert_token_frame(
                            &before,
                            &[self.env.actors[actor].destination_token, self.env.vault],
                        )?;
                    }
                    Err(_) => self.assert_snapshot_unchanged(&before)?,
                }
                Ok(())
            }
            Action::SyncMaintenanceFee { actor, dt } => {
                let actor = actor as usize % PRIMARY_ACTOR_COUNT;
                let next_slot = self.env.current_slot() + u64::from(dt.clamp(1, 4));
                self.env.warp_to_slot(next_slot);
                let before = self.snapshot();
                match self.env.sync_maintenance_fee(actor, next_slot) {
                    Ok(success) => {
                        self.coverage.maintenance_syncs += 1;
                        self.coverage.observe_success(None, &success);
                        self.assert_portfolio_frame(&before, &[actor])?;
                        self.assert_no_token_side_effects(&before)?;
                    }
                    Err(_) => self.assert_snapshot_unchanged(&before)?,
                }
                Ok(())
            }
            Action::SetMatcherConfig {
                actor,
                enabled,
                trade_fee_cap_bps,
            } => {
                self.coverage.extended_action_attempts[0] += 1;
                let actor = actor as usize % PRIMARY_ACTOR_COUNT;
                let before = self.snapshot();
                match self.env.set_matcher_config_with_trade_fee_cap(
                    actor,
                    u8::from(enabled),
                    if enabled {
                        trade_fee_cap_bps.min(10_000)
                    } else {
                        0
                    },
                ) {
                    Ok(success) => {
                        self.coverage.matcher_config_updates += 1;
                        self.coverage.observe_success(None, &success);
                        self.assert_portfolio_frame(&before, &[actor])?;
                        self.assert_no_token_side_effects(&before)?;
                    }
                    Err(_) => self.assert_snapshot_unchanged(&before)?,
                }
                Ok(())
            }
            Action::TopUpInsurance { domain, amount } => {
                self.coverage.extended_action_attempts[1] += 1;
                let domain = u16::from(domain) % (ASSET_COUNT as u16 * 2);
                let amount = u128::from(amount.max(1));
                let before = self.snapshot();
                let source_before =
                    u128::from(self.env.token_amount(self.env.provider_source_token));
                let vault_before = u128::from(self.env.token_amount(self.env.vault));
                match self.env.top_up_insurance_domain(domain, amount) {
                    Ok(success) => {
                        self.coverage.insurance_topups += 1;
                        self.coverage.observe_success(None, &success);
                        self.assert_portfolio_frame(&before, &[])?;
                        let source_after =
                            u128::from(self.env.token_amount(self.env.provider_source_token));
                        let vault_after = u128::from(self.env.token_amount(self.env.vault));
                        if source_after.checked_add(amount) != Some(source_before)
                            || vault_before.checked_add(amount) != Some(vault_after)
                        {
                            return Err(
                                "insurance top-up did not preserve exact source/vault deltas"
                                    .into(),
                            );
                        }
                        self.assert_token_frame(
                            &before,
                            &[self.env.provider_source_token, self.env.vault],
                        )?;
                    }
                    Err(_) => self.assert_snapshot_unchanged(&before)?,
                }
                Ok(())
            }
            Action::TopUpBacking {
                domain,
                amount,
                expiry_delta,
            } => {
                self.coverage.extended_action_attempts[2] += 1;
                let domain = u16::from(domain) % (ASSET_COUNT as u16 * 2);
                let amount = u128::from(amount.max(1));
                let expiry_slot = self
                    .env
                    .current_slot()
                    .checked_add(u64::from(expiry_delta))
                    .ok_or("backing expiry overflow")?;
                let before = self.snapshot();
                let source_before =
                    u128::from(self.env.token_amount(self.env.provider_source_token));
                let vault_before = u128::from(self.env.token_amount(self.env.vault));
                match self.env.top_up_backing_bucket(domain, amount, expiry_slot) {
                    Ok(success) => {
                        self.coverage.backing_topups += 1;
                        self.coverage.observe_success(None, &success);
                        self.assert_portfolio_frame(&before, &[])?;
                        let source_after =
                            u128::from(self.env.token_amount(self.env.provider_source_token));
                        let vault_after = u128::from(self.env.token_amount(self.env.vault));
                        if source_after.checked_add(amount) != Some(source_before)
                            || vault_before.checked_add(amount) != Some(vault_after)
                        {
                            return Err(
                                "backing top-up did not preserve exact source/vault deltas".into(),
                            );
                        }
                        self.assert_token_frame(
                            &before,
                            &[self.env.provider_source_token, self.env.vault],
                        )?;
                    }
                    Err(_) => self.assert_snapshot_unchanged(&before)?,
                }
                Ok(())
            }
            Action::ConvertReleasedPnl { actor, amount } => {
                self.coverage.extended_action_attempts[3] += 1;
                let actor = actor as usize % PRIMARY_ACTOR_COUNT;
                let before = self.snapshot();
                match self
                    .env
                    .convert_released_pnl(actor, u128::from(amount.max(1)))
                {
                    Ok(success) => {
                        self.coverage.pnl_conversions += 1;
                        self.coverage.observe_success(None, &success);
                        self.assert_portfolio_frame(&before, &[actor])?;
                        self.assert_no_token_side_effects(&before)?;
                    }
                    Err(_) => self.assert_snapshot_unchanged(&before)?,
                }
                Ok(())
            }
            Action::RebalanceReduce { actor, asset } => {
                self.coverage.extended_action_attempts[4] += 1;
                let actor = actor as usize % PRIMARY_ACTOR_COUNT;
                let asset = asset as usize % ASSET_COUNT;
                let size_before = self.positions[actor][asset];
                if size_before != 0 && self.try_rebalance_exit(actor, asset, size_before)? {
                    self.coverage.rebalance_reductions += 1;
                }
                Ok(())
            }
            Action::ConfigurePermissionlessResolve {
                stale_slots,
                force_close_delay_slots,
            } => {
                self.coverage.extended_action_attempts[5] += 1;
                let before = self.snapshot();
                match self.env.configure_permissionless_resolve(
                    u64::from(stale_slots.max(1)),
                    u64::from(force_close_delay_slots.max(1)),
                ) {
                    Ok(success) => {
                        self.coverage.resolve_policy_updates += 1;
                        self.coverage.observe_success(None, &success);
                        self.assert_portfolio_frame(&before, &[])?;
                        self.assert_no_token_side_effects(&before)?;
                    }
                    Err(_) => self.assert_snapshot_unchanged(&before)?,
                }
                Ok(())
            }
            Action::ShutdownAsset { asset, dt } => {
                self.coverage.extended_action_attempts[6] += 1;
                let asset = asset as usize % ASSET_COUNT;
                let next_slot = self
                    .env
                    .current_slot()
                    .checked_add(u64::from(dt.min(4)))
                    .ok_or("shutdown slot overflow")?;
                self.env.warp_to_slot(next_slot);
                let before = self.snapshot();
                match self.env.shutdown_asset(asset as u16, next_slot) {
                    Ok(success) => {
                        self.coverage.lifecycle_updates += 1;
                        self.coverage.observe_success(None, &success);
                        self.assert_portfolio_frame(&before, &[])?;
                        self.assert_no_token_side_effects(&before)?;
                        let (_, group) = self.env.primary_market_state();
                        if group.assets[asset].lifecycle != AssetLifecycleV16::Recovery {
                            return Err(format!(
                                "successful public shutdown left asset {asset} in {:?}",
                                group.assets[asset].lifecycle
                            ));
                        }
                    }
                    Err(_) => self.assert_snapshot_unchanged(&before)?,
                }
                Ok(())
            }
            Action::RotateOracleAuthority { asset, new_actor } => {
                self.coverage.extended_action_attempts[7] += 1;
                let asset = asset as usize % ASSET_COUNT;
                let new_actor = new_actor as usize % USER_COUNT;
                let before = self.snapshot();
                match self.env.update_asset_authority_from_admin(
                    asset as u16,
                    percolator_prog::processor::ASSET_AUTH_ORACLE,
                    new_actor,
                ) {
                    Ok(success) => {
                        self.coverage.authority_updates += 1;
                        self.coverage.observe_success(None, &success);
                        self.assert_portfolio_frame(&before, &[])?;
                        self.assert_no_token_side_effects(&before)?;
                    }
                    Err(_) => self.assert_snapshot_unchanged(&before)?,
                }
                Ok(())
            }
            Action::CrossMarketSubstitution { actor } => self.execute_account_substitution(
                actor as usize % USER_COUNT,
                SubstitutionKind::ForeignTradePortfolio,
            ),
            Action::AccountSubstitution { actor, kind } => {
                self.execute_account_substitution(actor as usize % USER_COUNT, kind)
            }
            Action::RetainTrade {
                taker,
                maker,
                asset,
                units,
            } => {
                let taker = taker as usize % USER_COUNT;
                let mut maker = maker as usize % USER_COUNT;
                if maker == taker {
                    maker = (maker + 1) % USER_COUNT;
                }
                let asset = asset as usize % ASSET_COUNT;
                let mut size = (units as i128).clamp(-2, 2) * POS_SCALE as i128 / 4;
                if size == 0 {
                    size = POS_SCALE as i128 / 4;
                }
                let price = self.env.primary_market_state().1.assets[asset].effective_price;
                let transaction =
                    self.env
                        .build_retained_no_cpi_trade(taker, maker, asset as u16, size, price);
                self.retained.push_back(RetainedTrade {
                    transaction,
                    taker,
                    maker,
                    legs: vec![(asset, size)],
                });
                Ok(())
            }
            Action::LandRetained => {
                let Some(retained) = self.retained.pop_front() else {
                    return Ok(());
                };
                let taker_before = self.env.primary_portfolio(retained.taker);
                let maker_before = self.env.primary_portfolio(retained.maker);
                let (_, group_before) = self.env.primary_market_state();
                let before = self.snapshot();
                match self.env.land_retained(retained.transaction) {
                    Ok(success) => {
                        self.coverage.retained_landed += 1;
                        self.coverage
                            .observe_success(Some(TradeRoute::NoCpi), &success);
                        self.record_trade(
                            retained.taker,
                            retained.maker,
                            &retained.legs,
                            &taker_before,
                            &maker_before,
                            &group_before,
                        )?;
                        self.assert_portfolio_frame(&before, &[retained.taker, retained.maker])?;
                        self.assert_no_token_side_effects(&before)?;
                    }
                    Err(_) => {
                        self.coverage.retained_rejected += 1;
                        self.assert_snapshot_unchanged(&before)?;
                    }
                }
                Ok(())
            }
            Action::AdvanceBlockhash => {
                self.env.expire_blockhash();
                Ok(())
            }
        }
    }

    fn execute_deposit(
        &mut self,
        actor: usize,
        amount: u128,
        must_succeed: bool,
    ) -> Result<bool, String> {
        let before = self.snapshot();
        let source = self.env.actors[actor].source_token;
        let source_before = u128::from(self.env.token_amount(source));
        let vault_before = u128::from(self.env.token_amount(self.env.vault));
        let capital_before = self.env.primary_portfolio(actor).capital.get();
        match self.env.deposit_primary(actor, amount) {
            Ok(success) => {
                self.coverage.deposits += 1;
                self.coverage.observe_success(None, &success);
                self.assert_portfolio_frame(&before, &[actor])?;
                let source_after = u128::from(self.env.token_amount(source));
                let vault_after = u128::from(self.env.token_amount(self.env.vault));
                let capital_after = self.env.primary_portfolio(actor).capital.get();
                if source_after.checked_add(amount) != Some(source_before)
                    || vault_before.checked_add(amount) != Some(vault_after)
                    || capital_before.checked_add(amount) != Some(capital_after)
                {
                    return Err(format!(
                        "deposit {amount} for actor {actor} did not preserve exact source/vault/capital deltas"
                    ));
                }
                self.assert_token_frame(&before, &[source, self.env.vault])?;
                Ok(true)
            }
            Err(error) => {
                self.assert_snapshot_unchanged(&before)?;
                if must_succeed {
                    Err(format!(
                        "valid owner deposit {amount} failed for actor {actor}: {error}"
                    ))
                } else {
                    Ok(false)
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_trade(
        &mut self,
        route: TradeRoute,
        taker: usize,
        maker: usize,
        legs: Vec<(usize, i128)>,
        fee_bps: u64,
        price_move_bps: i16,
        must_succeed: bool,
    ) -> Result<bool, String> {
        if legs.is_empty() {
            return Ok(false);
        }
        let taker_before = self.env.primary_portfolio(taker);
        let maker_before = self.env.primary_portfolio(maker);
        let before = self.snapshot();
        let market = self.env.primary_market_state().1;
        let result = match route {
            TradeRoute::NoCpi => {
                let (asset, size) = legs[0];
                self.env.trade_no_cpi(
                    taker,
                    maker,
                    asset as u16,
                    size,
                    moved_price(market.assets[asset].effective_price, price_move_bps)?,
                    fee_bps,
                )
            }
            TradeRoute::Cpi => {
                let (asset, size) = legs[0];
                self.env
                    .trade_cpi(taker, maker, asset as u16, size, fee_bps, 0)
            }
            TradeRoute::BatchNoCpi => {
                let encoded = legs
                    .iter()
                    .map(|(asset, size)| {
                        Ok(BatchTradeLeg {
                            asset_index: *asset as u16,
                            market_id: market.assets[*asset].market_id,
                            size_q: *size,
                            exec_price: moved_price(
                                market.assets[*asset].effective_price,
                                price_move_bps,
                            )?,
                            fee_bps,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                self.env.batch_trade_no_cpi(taker, maker, encoded)
            }
            TradeRoute::BatchCpi => {
                let encoded = legs
                    .iter()
                    .map(|(asset, size)| BatchTradeCpiLeg {
                        asset_index: *asset as u16,
                        market_id: market.assets[*asset].market_id,
                        size_q: *size,
                        fee_bps,
                        limit_price: 0,
                    })
                    .collect();
                self.env.batch_trade_cpi(taker, maker, encoded)
            }
        };
        match result {
            Ok(success) => {
                self.last_trade_rejection = None;
                self.coverage.observe_success(Some(route), &success);
                self.record_trade(taker, maker, &legs, &taker_before, &maker_before, &market)?;
                self.assert_portfolio_frame(&before, &[taker, maker])?;
                self.assert_no_token_side_effects(&before)?;
                Ok(true)
            }
            Err(error) => {
                self.last_trade_rejection =
                    Some(format!("{route:?} taker {taker} maker {maker}: {error}"));
                self.coverage.route_reject[route.index()] += 1;
                self.assert_snapshot_unchanged(&before)?;
                if must_succeed {
                    Err(format!(
                        "valid risk-reducing {route:?} failed for taker {taker}, maker {maker}: \
                         {error}; {}",
                        self.trade_diagnostics(taker, maker, &legs)?
                    ))
                } else {
                    Ok(false)
                }
            }
        }
    }

    fn execute_account_substitution(
        &mut self,
        actor: usize,
        kind: SubstitutionKind,
    ) -> Result<(), String> {
        let before = self.snapshot();
        let result = match kind {
            SubstitutionKind::ForeignTradePortfolio => self
                .env
                .cross_market_trade_substitution(actor, POS_SCALE as i128 / 4),
            SubstitutionKind::ForeignDepositVault => {
                self.env.cross_market_deposit_vault_substitution(actor, 1)
            }
            SubstitutionKind::ForeignWithdrawVault => {
                self.env.cross_market_withdraw_vault_substitution(actor, 1)
            }
            SubstitutionKind::ForeignCrankPortfolio => self
                .env
                .cross_market_crank_portfolio_substitution(self.env.current_slot()),
            SubstitutionKind::MismatchedMatcherBinding => {
                let maker = (actor + 1) % USER_COUNT;
                let substituted_binding = (maker + 1) % USER_COUNT;
                self.env
                    .cpi_matcher_binding_substitution(actor, maker, substituted_binding)
            }
        };
        match result {
            Ok(_) => Err(format!(
                "public account substitution {kind:?} unexpectedly succeeded"
            )),
            Err(_) => {
                self.coverage.substitution_rejections[kind.index()] += 1;
                self.assert_snapshot_unchanged(&before)
            }
        }
    }

    fn record_trade(
        &mut self,
        taker: usize,
        maker: usize,
        legs: &[(usize, i128)],
        taker_before: &PortfolioAccountV16,
        maker_before: &PortfolioAccountV16,
        group_before: &MarketGroupV16,
    ) -> Result<(), String> {
        let mut taker_deltas = [0i128; ASSET_COUNT];
        let mut maker_deltas = [0i128; ASSET_COUNT];
        for &(asset, size) in legs {
            taker_deltas[asset] = taker_deltas[asset]
                .checked_add(size)
                .ok_or("taker ghost position overflow")?;
            maker_deltas[asset] = maker_deltas[asset]
                .checked_sub(size)
                .ok_or("maker ghost position overflow")?;
        }
        self.reconcile_account_position_changes(
            taker,
            taker_before,
            group_before,
            taker_deltas,
            false,
            "matched trade taker",
        )?;
        self.reconcile_account_position_changes(
            maker,
            maker_before,
            group_before,
            maker_deltas,
            false,
            "matched trade maker",
        )?;
        self.assert_positions_match()
    }

    fn execute_crank(
        &mut self,
        actor: usize,
        hints: HintMode,
        require_progress: bool,
    ) -> Result<(), CrankFailure> {
        let account_before = self.env.primary_portfolio(actor);
        let (_, group_before) = self.env.primary_market_state();
        let before = self.snapshot();
        let rank_before = self.progress_rank(actor).map_err(CrankFailure::Invariant)?;
        let diagnostics_before = self.liveness_diagnostics();
        let liquidation_authorized = self.current_liquidation_authorization(actor);
        let observations = match hints {
            HintMode::Complete => self
                .selected_observation(actor)
                .map_err(CrankFailure::Invariant)?,
            HintMode::Reversed => all_observations().into_iter().rev().collect(),
            HintMode::Empty => vec![],
            HintMode::Duplicate => vec![
                CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                },
                CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                },
            ],
        };
        match self
            .env
            .crank(actor, self.env.current_slot(), observations.clone())
        {
            Ok(success) => {
                self.coverage.observe_success(None, &success);
                self.assert_portfolio_frame(&before, &[actor])
                    .map_err(CrankFailure::Invariant)?;
                self.assert_no_token_side_effects(&before)
                    .map_err(CrankFailure::Invariant)?;
                self.reconcile_account_position_changes(
                    actor,
                    &account_before,
                    &group_before,
                    [0; ASSET_COUNT],
                    liquidation_authorized,
                    "permissionless crank",
                )
                .map_err(CrankFailure::Invariant)?;
                let rank_after = self.progress_rank(actor).map_err(CrankFailure::Invariant)?;
                if require_progress
                    && rank_before.actionable()
                    && !rank_after.reduced_from(rank_before)
                {
                    return Err(CrankFailure::Invariant(format!(
                        "sole public crank succeeded without rank decrease: {rank_before:?} -> \
                         {rank_after:?}; before={diagnostics_before}; after={}",
                        self.liveness_diagnostics()
                    )));
                }
                if rank_after.reduced_from(rank_before) {
                    self.record_permissionless_rank_reduction(rank_before, rank_after);
                }
                Ok(())
            }
            Err(error) => {
                self.assert_snapshot_unchanged(&before)
                    .map_err(CrankFailure::Invariant)?;
                if require_progress && rank_before.actionable() {
                    Err(CrankFailure::Rejected(format!(
                        "sole public crank rejected actionable rank {rank_before:?} with \
                         observations {observations:?}: {error}; {}",
                        self.liveness_diagnostics()
                    )))
                } else {
                    Ok(())
                }
            }
        }
    }

    fn record_permissionless_rank_reduction(
        &mut self,
        rank_before: ProgressRank,
        rank_after: ProgressRank,
    ) {
        self.coverage.crank_progress += 1;
        let before_components = rank_before.components();
        let after_components = rank_after.components();
        for index in 0..before_components.len() {
            if before_components[index] != 0 {
                self.coverage.crank_rank_component_seen[index] += 1;
            }
            if after_components[index] < before_components[index] {
                self.coverage.crank_rank_component_reduced[index] += 1;
            }
        }
        let before_class = rank_before.class_mask();
        let after_class = rank_after.class_mask();
        self.coverage.crank_rank_nodes.insert(before_class);
        self.coverage.crank_rank_nodes.insert(after_class);
        self.coverage
            .crank_rank_edges
            .insert((before_class, after_class));
    }

    fn selected_observation(&self, actor: usize) -> Result<Vec<CrankObservationHint>, String> {
        let (_, group) = self.env.primary_market_state();
        let rank = self.progress_rank(actor)?;
        let terminal_or_global_lock =
            group.bankruptcy_hlock_active || group.threshold_stress_active;
        if rank.b_work != 0 || terminal_or_global_lock {
            return Ok(vec![]);
        }
        Ok((0..ASSET_COUNT)
            .filter(|asset| {
                let profile = self.env.primary_profile(*asset);
                let engine_asset = &group.assets[*asset];
                let has_price_delta = engine_asset.raw_oracle_target_price
                    != engine_asset.effective_price
                    || profile.mark_ewma_e6 != engine_asset.effective_price;
                let has_loss_currentness_lag = asset_contributes_to_loss_stale(engine_asset)
                    && self.env.current_slot() > engine_asset.slot_last;
                (has_price_delta || has_loss_currentness_lag)
                    && self.env.current_slot() > engine_asset.slot_last
            })
            .map(|asset| CrankObservationHint {
                asset_index: asset as u16,
                oracle_accounts: self.env.primary_profile(asset).oracle_leg_count,
            })
            .collect())
    }

    fn current_liquidation_authorization(&self, actor: usize) -> bool {
        let account = self.env.primary_portfolio(actor);
        let (_, group) = self.env.primary_market_state();
        let Ok(cert) = account.health_cert.try_to_runtime() else {
            return false;
        };
        cert.valid
            && cert.certified_liq_deficit != 0
            && cert.cert_oracle_epoch == group.oracle_epoch
            && cert.cert_funding_epoch == group.funding_epoch
            && cert.cert_risk_epoch == group.risk_epoch
            && cert.cert_asset_set_epoch == group.asset_set_epoch
            && cert.active_bitmap_at_cert == account.active_bitmap.map(|word| word.get())
    }

    fn prior_reset_cleanup_eligible(
        account_before: &PortfolioAccountV16,
        group_before: &MarketGroupV16,
        asset: usize,
    ) -> bool {
        decoded_legs(account_before)
            .iter()
            .find(|leg| leg.active && leg.asset_index as usize == asset)
            .map(|leg| {
                let engine_asset = &group_before.assets[asset];
                let (mode, epoch, effective_oi) = match leg.side {
                    SideV16::Long => (
                        engine_asset.mode_long,
                        engine_asset.epoch_long,
                        engine_asset.oi_eff_long_q,
                    ),
                    SideV16::Short => (
                        engine_asset.mode_short,
                        engine_asset.epoch_short,
                        engine_asset.oi_eff_short_q,
                    ),
                };
                mode == SideModeV16::ResetPending
                    && effective_oi == 0
                    && leg.epoch_snap.checked_add(1) == Some(epoch)
            })
            .unwrap_or(false)
    }

    fn reconcile_account_position_changes(
        &mut self,
        actor: usize,
        account_before: &PortfolioAccountV16,
        group_before: &MarketGroupV16,
        expected_deltas: [i128; ASSET_COUNT],
        allow_unspecified_reduction: bool,
        context: &str,
    ) -> Result<(), String> {
        let before_positions = observed_positions(account_before)?;
        if before_positions != self.positions[actor] {
            return Err(format!(
                "{context} actor {actor} pre-state diverged from ghost: observed={before_positions:?}, ghost={:?}",
                self.positions[actor]
            ));
        }
        let observed = observed_positions(&self.env.primary_portfolio(actor))?;
        for (asset, new) in observed.into_iter().enumerate() {
            let old = before_positions[asset];
            let delta = new
                .checked_sub(old)
                .ok_or("observed account position delta overflow")?;
            let expected = expected_deltas[asset];
            let ordinary_post = old
                .checked_add(expected)
                .ok_or("expected account position overflow")?;
            if new == ordinary_post {
                self.positions[actor][asset] = new;
                continue;
            }
            let prior_reset_cleanup =
                Self::prior_reset_cleanup_eligible(account_before, group_before, asset)
                    && new == expected;
            let same_side_or_flat = (old > 0 && new >= 0) || (old < 0 && new <= 0);
            let strict_reduction =
                old != 0 && same_side_or_flat && new.unsigned_abs() < old.unsigned_abs();
            let liquidation_reduction =
                expected == 0 && allow_unspecified_reduction && strict_reduction;
            if !prior_reset_cleanup && !liquidation_reduction {
                return Err(format!(
                    "{context} changed actor {actor} asset {asset} from {old} to {new} (delta \
                     {delta}) instead of signed post-state {ordinary_post}; no liquidation or \
                     prior-reset witness explains the difference"
                ));
            }
            let unilateral_delta = delta
                .checked_sub(expected)
                .ok_or("unilateral position delta overflow")?;
            self.protocol_positions[asset] = self.protocol_positions[asset]
                .checked_sub(unilateral_delta)
                .ok_or("protocol unilateral-position attribution overflow")?;
            self.positions[actor][asset] = new;
            if !prior_reset_cleanup {
                let reduced = old.unsigned_abs() - new.unsigned_abs();
                self.coverage.liquidation_steps += 1;
                self.coverage.liquidated_abs_q = self
                    .coverage
                    .liquidated_abs_q
                    .checked_add(reduced)
                    .ok_or("liquidation coverage overflow")?;
            } else {
                self.coverage.user_positions_closed += u64::from(new == 0);
            }
        }
        Ok(())
    }

    fn drain_actor(&mut self, actor: usize, limit: usize) -> Result<(), String> {
        for _ in 0..limit {
            self.advance_liveness_clock_if_needed()?;
            if !self.progress_rank(actor)?.actionable() {
                return Ok(());
            }
            self.drain_one_progress_step(Some(actor))?;
        }
        let rank = self.progress_rank(actor)?;
        if rank.actionable() {
            return Err(format!(
                "actor {actor} did not converge in {limit} cranks; rank {rank:?}"
            ));
        }
        Ok(())
    }

    fn drain_cranks(&mut self, limit: usize) -> Result<(), String> {
        for _ in 0..limit {
            self.advance_liveness_clock_if_needed()?;
            if !(0..PRIMARY_ACTOR_COUNT)
                .map(|actor| self.progress_rank(actor))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .any(ProgressRank::actionable)
            {
                return Ok(());
            }
            self.drain_one_progress_step(None)?;
        }
        Err(format!(
            "permissionless drain exceeded deterministic bound {limit}; {}",
            self.liveness_diagnostics()
        ))
    }

    fn drain_one_progress_step(&mut self, preferred: Option<usize>) -> Result<(), String> {
        if self.try_finalize_reset_side()? {
            return Ok(());
        }
        let mut candidates = Vec::with_capacity(PRIMARY_ACTOR_COUNT);
        if let Some(actor) = preferred {
            candidates.push(actor);
        }
        candidates.extend((0..PRIMARY_ACTOR_COUNT).filter(|actor| Some(*actor) != preferred));

        let ranks = candidates
            .iter()
            .map(|actor| Ok((*actor, self.progress_rank(*actor)?)))
            .collect::<Result<Vec<_>, String>>()?;
        let has_account_work = ranks.iter().any(|(_, rank)| rank.account_actionable());
        let mut failures = Vec::new();
        for (actor, before) in ranks {
            if !before.actionable() || (has_account_work && !before.account_actionable()) {
                continue;
            }
            match self.execute_crank(actor, HintMode::Complete, true) {
                Ok(()) => return Ok(()),
                Err(CrankFailure::Rejected(error)) => {
                    failures.push(format!("actor {actor} rank {before:?}: {error}"))
                }
                Err(CrankFailure::Invariant(error)) => {
                    return Err(format!("actor {actor} rank {before:?}: {error}"))
                }
            }
        }
        Err(format!(
            "all independently actionable public crank candidates rejected or failed to progress: \
             {}; {}",
            failures.join(" | "),
            self.liveness_diagnostics()
        ))
    }

    fn try_finalize_reset_side(&mut self) -> Result<bool, String> {
        let (_, group) = self.env.primary_market_state();
        let pending: Vec<_> = group
            .assets
            .iter()
            .take(ASSET_COUNT)
            .enumerate()
            .flat_map(|(asset, state)| {
                [
                    (asset, 0u8, state.mode_long),
                    (asset, 1u8, state.mode_short),
                ]
            })
            .filter(|(asset, side, mode)| {
                *mode == SideModeV16::ResetPending && reset_side_finalizable(&group, *asset, *side)
            })
            .collect();
        if pending.is_empty() {
            return Ok(false);
        }
        let pending_before = reset_pending_side_count(&group);
        let mut failures = Vec::new();
        let mut stale_prerequisites = 0usize;
        for (asset, side, _) in pending {
            let rank_before = self.progress_rank(0)?;
            let before = self.snapshot();
            match self.env.finalize_reset_side(asset as u16, side) {
                Ok(success) => {
                    self.coverage.observe_success(None, &success);
                    self.assert_portfolio_frame(&before, &[])?;
                    self.assert_no_token_side_effects(&before)?;
                    let (_, after) = self.env.primary_market_state();
                    let pending_after = reset_pending_side_count(&after);
                    if pending_after >= pending_before {
                        return Err(format!(
                            "FinalizeResetSide succeeded without lowering pending-side rank: {pending_before} -> {pending_after}"
                        ));
                    }
                    let rank_after = self.progress_rank(0)?;
                    if !rank_after.reduced_from(rank_before) {
                        return Err(format!(
                            "FinalizeResetSide succeeded without lowering the public liveness rank: {rank_before:?} -> {rank_after:?}"
                        ));
                    }
                    self.record_permissionless_rank_reduction(rank_before, rank_after);
                    return Ok(true);
                }
                Err(error) => {
                    self.assert_snapshot_unchanged(&before)?;
                    if error.contains("Custom(19)") || error.contains("custom program error: 0x13")
                    {
                        stale_prerequisites += 1;
                    } else {
                        failures.push(format!("asset {asset} side {side}: {error}"));
                    }
                }
            }
        }
        if failures.is_empty() && stale_prerequisites != 0 {
            return Ok(false);
        }
        Err(format!(
            "all permissionless ResetPending finalizers rejected: {}",
            failures.join(" | ")
        ))
    }

    fn liveness_diagnostics(&self) -> String {
        let (_, group) = self.env.primary_market_state();
        let assets: Vec<_> = group
            .assets
            .iter()
            .take(ASSET_COUNT)
            .enumerate()
            .map(|(index, asset)| {
                format!(
                    "{index}:slot={},px={}/{},k={}/{},f={}/{},mode={:?}/{:?},oi={}/{},\
                     stored={}/{},stale={}/{},pending={}/{},weight={}/{}",
                    asset.slot_last,
                    asset.effective_price,
                    asset.raw_oracle_target_price,
                    asset.k_long,
                    asset.k_short,
                    asset.f_long_num,
                    asset.f_short_num,
                    asset.mode_long,
                    asset.mode_short,
                    asset.oi_eff_long_q,
                    asset.oi_eff_short_q,
                    asset.stored_pos_count_long,
                    asset.stored_pos_count_short,
                    asset.stale_account_count_long,
                    asset.stale_account_count_short,
                    asset.pending_obligation_count_long,
                    asset.pending_obligation_count_short,
                    asset.loss_weight_sum_long,
                    asset.loss_weight_sum_short,
                )
            })
            .collect();
        let accounts: Vec<_> = (0..PRIMARY_ACTOR_COUNT)
            .map(|actor| {
                let account = self.env.primary_portfolio(actor);
                (
                    actor,
                    account.health_cert.try_to_runtime().ok(),
                    decoded_legs(&account),
                )
            })
            .collect();
        format!(
            "liveness_state={{clock:{}, market_current:{}, market_slot:{}, epochs:[{},{},{},{}], \
             locks:[{},{},{}], assets:{assets:?}, accounts:{accounts:?}}}",
            self.env.current_slot(),
            group.current_slot,
            group.slot_last,
            group.oracle_epoch,
            group.funding_epoch,
            group.risk_epoch,
            group.asset_set_epoch,
            group.bankruptcy_hlock_active,
            group.threshold_stress_active,
            group.loss_stale_active,
        )
    }

    fn advance_liveness_clock_if_needed(&mut self) -> Result<(), String> {
        let has_account_work = (0..PRIMARY_ACTOR_COUNT)
            .map(|actor| self.progress_rank(actor))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .any(ProgressRank::account_actionable);
        if has_account_work {
            return Ok(());
        }
        let (_, group) = self.env.primary_market_state();
        let needs_time = group.assets.iter().take(ASSET_COUNT).any(|asset| {
            asset.raw_oracle_target_price != asset.effective_price
                && self.env.current_slot() <= asset.slot_last
        });
        if needs_time {
            self.env
                .warp_to_slot(self.env.current_slot() + group.config.max_accrual_dt_slots.max(1));
        }
        Ok(())
    }

    fn progress_rank(&self, actor: usize) -> Result<ProgressRank, String> {
        let account = self.env.primary_portfolio(actor);
        let (_, group) = self.env.primary_market_state();
        let authenticated_slot = self.env.current_slot();
        let mut market_mark_lag = 0u128;
        let mut market_loss_lag = 0u128;
        for asset in 0..ASSET_COUNT {
            let profile = self.env.primary_profile(asset);
            let engine_asset = &group.assets[asset];
            let engine_gap = engine_asset
                .raw_oracle_target_price
                .abs_diff(engine_asset.effective_price);
            let authenticated_gap = profile.mark_ewma_e6.abs_diff(engine_asset.effective_price);
            let price_gap = engine_gap.max(authenticated_gap);
            if price_gap != 0 && self.env.current_slot() > engine_asset.slot_last {
                market_mark_lag = market_mark_lag
                    .checked_add(price_gap as u128)
                    .and_then(|rank| {
                        rank.checked_add(
                            self.env
                                .current_slot()
                                .saturating_sub(engine_asset.slot_last)
                                as u128,
                        )
                    })
                    .ok_or("mark-progress rank overflow")?;
            }
            if asset_contributes_to_loss_stale(engine_asset) {
                market_loss_lag = market_loss_lag
                    .checked_add(u128::from(
                        authenticated_slot.saturating_sub(engine_asset.slot_last),
                    ))
                    .ok_or("loss-currentness rank overflow")?;
            }
        }
        let mut b_work = 0u128;
        let mut stale_legs = 0u128;
        let mut active = 0u128;
        for leg in decoded_legs(&account) {
            if !leg.active {
                continue;
            }
            active += 1;
            b_work = b_work
                .checked_add(leg.b_rem)
                .ok_or("B-work rank overflow")?;
            if leg.b_stale {
                b_work = b_work.checked_add(1).ok_or("B-work flag overflow")?;
            }
            if leg.stale {
                stale_legs += 1;
            }
        }
        if account.b_stale_state != 0 {
            b_work = b_work
                .checked_add(1)
                .ok_or("portfolio B-stale rank overflow")?;
        }
        if account.stale_state != 0 {
            stale_legs = stale_legs
                .checked_add(1)
                .ok_or("portfolio stale rank overflow")?;
        }
        let cert = &account.health_cert;
        let cert_epoch_mismatch = cert.cert_oracle_epoch.get() != group.oracle_epoch
            || cert.cert_funding_epoch.get() != group.funding_epoch
            || cert.cert_risk_epoch.get() != group.risk_epoch
            || cert.cert_asset_set_epoch.get() != group.asset_set_epoch
            || cert.active_bitmap_at_cert != account.active_bitmap;
        // Recovery legs remain owner-exitable, but the engine deliberately excludes them from
        // auto-crank refresh/liquidation selection. Keep the permissionless-crank rank scoped to
        // the same dispatch domain; the separate direct-user campaign owns Recovery exit
        // liveness and INV-082 has a public regression for the classifier boundary.
        let crank_dispatchable_abs_q = decoded_legs(&account)
            .into_iter()
            .filter(|leg| {
                leg.active
                    && group
                        .assets
                        .get(leg.asset_index as usize)
                        .map(|asset| {
                            matches!(
                                asset.lifecycle,
                                AssetLifecycleV16::Active | AssetLifecycleV16::DrainOnly
                            )
                        })
                        .unwrap_or(false)
            })
            .try_fold(0u128, |sum, leg| {
                sum.checked_add(leg.basis_pos_q.unsigned_abs())
                    .ok_or("active-position rank overflow")
            })?;
        let health_work = if active == 0 || crank_dispatchable_abs_q == 0 {
            0
        } else if cert.valid == 0 || cert_epoch_mismatch {
            crank_dispatchable_abs_q
                .checked_add(1u128 << 120)
                .ok_or("invalid-health rank overflow")?
        } else if cert.certified_liq_deficit.get() != 0 {
            crank_dispatchable_abs_q
        } else {
            0
        };
        let lapsed_live_backing = if group.mode == MarketModeV16::Live {
            account
                .source_domains
                .iter()
                .filter(|source| source.is_occupied())
                .filter_map(|source| {
                    group
                        .source_backing_buckets
                        .get(source.domain.get() as usize)
                })
                .filter(|bucket| {
                    bucket.status == BackingBucketStatusV16::Fresh
                        && bucket.expiry_slot <= group.current_slot
                        && (bucket.fresh_unliened_backing_num != 0
                            || bucket.valid_liened_backing_num != 0
                            || bucket.consumed_liened_backing_num != 0
                            || bucket.impaired_liened_backing_num != 0)
                })
                .count() as u128
        } else {
            0
        };
        // Auto-crank receives one caller-selected portfolio. Reset work is actionable for that
        // call only when this portfolio owns an old-epoch leg or the side is already finalizable.
        // Include the future finalize step when this account owns the last stored leg so clear
        // changes 2 -> 1 and finalize changes 1 -> 0 without making unrelated empty accounts
        // appear actionable.
        let loss_work = reset_pending_work_for_account(&group, &account)?
            .max(u128::from(group.loss_stale_active));
        let market_locks = u128::from(group.bankruptcy_hlock_active)
            .checked_add(u128::from(group.threshold_stress_active))
            .and_then(|value| value.checked_add(loss_work))
            .and_then(|value| value.checked_add(lapsed_live_backing))
            .ok_or("market-lock progress rank overflow")?;
        Ok(ProgressRank {
            market_mark_lag,
            market_loss_lag,
            market_locks,
            b_work,
            stale_legs,
            health_work,
        })
    }

    fn assert_global_invariants(&self) -> Result<(), String> {
        if self.env.token_supply_observed() != self.env.initial_token_supply {
            return Err(format!(
                "SPL custody changed total supply: observed {}, expected {}",
                self.env.token_supply_observed(),
                self.env.initial_token_supply
            ));
        }
        let (_, primary) = self.env.primary_market_state();
        let (_, foreign) = self.env.foreign_market_state();
        if primary.vault != self.env.token_amount(self.env.vault) as u128 {
            return Err("primary engine vault diverged from SPL vault".into());
        }
        if foreign.vault != self.env.token_amount(self.env.foreign_vault) as u128 {
            return Err("foreign engine vault diverged from SPL vault".into());
        }
        let primary_portfolios: Vec<_> = (0..PRIMARY_ACTOR_COUNT)
            .map(|actor| self.env.primary_portfolio(actor))
            .collect();
        let primary_capital: u128 = primary_portfolios
            .iter()
            .map(|portfolio| portfolio.capital.get())
            .sum();
        if primary_capital != primary.c_tot {
            return Err(format!(
                "primary c_tot {} != independent portfolio sum {}",
                primary.c_tot, primary_capital
            ));
        }
        let foreign_capital = self.env.foreign_portfolio().capital.get();
        if foreign_capital != foreign.c_tot {
            return Err(format!(
                "foreign c_tot {} != independent portfolio sum {}",
                foreign.c_tot, foreign_capital
            ));
        }
        let primary_senior = primary
            .c_tot
            .checked_add(primary.insurance)
            .ok_or("primary senior-value overflow")?;
        if primary.vault < primary_senior {
            return Err(format!(
                "unbacked primary value: vault {}, capital {}, insurance {}",
                primary.vault, primary.c_tot, primary.insurance
            ));
        }
        let foreign_senior = foreign
            .c_tot
            .checked_add(foreign.insurance)
            .ok_or("foreign senior-value overflow")?;
        if foreign.vault < foreign_senior {
            return Err("unbacked foreign value".into());
        }
        assert_source_credit_rates("primary", &primary)?;
        assert_source_credit_rates("foreign", &foreign)?;
        assert_source_claim_bound_attribution("primary", &primary, &primary_portfolios)?;
        let foreign_portfolios = [self.env.foreign_portfolio()];
        assert_source_claim_bound_attribution("foreign", &foreign, &foreign_portfolios)?;
        assert_public_stock_census("stateful post-transition", &self.env)?;
        assert_public_encumbrance_census("stateful post-transition", &self.env)?;
        self.assert_positions_match()
    }

    fn assert_positions_match(&self) -> Result<(), String> {
        let (_, group) = self.env.primary_market_state();
        let mut observed = [[0i128; ASSET_COUNT]; PRIMARY_ACTOR_COUNT];
        let mut observed_long_oi = [0u128; ASSET_COUNT];
        let mut observed_short_oi = [0u128; ASSET_COUNT];
        let mut observed_long_count = [0u64; ASSET_COUNT];
        let mut observed_short_count = [0u64; ASSET_COUNT];
        let mut asset_has_stale_leg = [false; ASSET_COUNT];
        for (actor, row) in observed.iter_mut().enumerate() {
            let account = self.env.primary_portfolio(actor);
            let mut seen_assets = [false; ASSET_COUNT];
            for (slot, encoded_leg) in account.legs.iter().enumerate() {
                let leg = encoded_leg.try_to_runtime().map_err(|error| {
                    format!("actor {actor} leg slot {slot} failed to decode: {error:?}")
                })?;
                let bitmap_word = slot / u64::BITS as usize;
                let bitmap_bit = slot % u64::BITS as usize;
                let active_bit = account
                    .active_bitmap
                    .get(bitmap_word)
                    .map(|word| word.get() & (1u64 << bitmap_bit) != 0)
                    .unwrap_or(false);
                if active_bit != leg.active {
                    return Err(format!(
                        "actor {actor} leg slot {slot} disagrees with active bitmap"
                    ));
                }
                if !leg.active {
                    if !leg.is_empty() {
                        return Err(format!(
                            "actor {actor} leg slot {slot} hides nonempty inactive state"
                        ));
                    }
                    continue;
                }
                let asset = leg.asset_index as usize;
                if asset >= ASSET_COUNT {
                    return Err(format!("actor {actor} has out-of-world asset {asset}"));
                }
                if seen_assets[asset] {
                    return Err(format!(
                        "actor {actor} has duplicate active legs for asset {asset}"
                    ));
                }
                seen_assets[asset] = true;
                if leg.market_id != group.assets[asset].market_id {
                    return Err(format!(
                        "actor {actor} asset {asset} leg generation {} != current {}",
                        leg.market_id, group.assets[asset].market_id
                    ));
                }
                let (side_mode, side_epoch) = match leg.side {
                    SideV16::Long => (
                        group.assets[asset].mode_long,
                        group.assets[asset].epoch_long,
                    ),
                    SideV16::Short => (
                        group.assets[asset].mode_short,
                        group.assets[asset].epoch_short,
                    ),
                };
                let epoch_bound = if side_mode == SideModeV16::ResetPending {
                    leg.epoch_snap.checked_add(1) == Some(side_epoch)
                } else {
                    leg.epoch_snap == side_epoch
                };
                if !epoch_bound {
                    return Err(format!(
                        "actor {actor} asset {asset} {:?} leg epoch {} is not bound to {:?} epoch {}",
                        leg.side, leg.epoch_snap, side_mode, side_epoch
                    ));
                }
                if leg.basis_pos_q == i128::MIN
                    || (leg.basis_pos_q > 0 && leg.side != SideV16::Long)
                    || (leg.basis_pos_q < 0 && leg.side != SideV16::Short)
                {
                    return Err(format!(
                        "actor {actor} asset {asset} has inconsistent side/basis {:?}/{}",
                        leg.side, leg.basis_pos_q
                    ));
                }
                row[asset] = row[asset]
                    .checked_add(leg.basis_pos_q)
                    .ok_or("observed position overflow")?;
                asset_has_stale_leg[asset] |= leg.stale || leg.b_stale;
                match leg.side {
                    SideV16::Long => {
                        observed_long_oi[asset] = observed_long_oi[asset]
                            .checked_add(leg.basis_pos_q.unsigned_abs())
                            .ok_or("observed long OI overflow")?;
                        observed_long_count[asset] = observed_long_count[asset]
                            .checked_add(1)
                            .ok_or("observed long position-count overflow")?;
                    }
                    SideV16::Short => {
                        observed_short_oi[asset] = observed_short_oi[asset]
                            .checked_add(leg.basis_pos_q.unsigned_abs())
                            .ok_or("observed short OI overflow")?;
                        observed_short_count[asset] = observed_short_count[asset]
                            .checked_add(1)
                            .ok_or("observed short position-count overflow")?;
                    }
                }
            }
            self.assert_source_domain_shape(actor, &account, &group)?;
        }
        if observed != self.positions {
            return Err(format!(
                "public position deltas diverged from ghost model\nobserved={observed:?}\nghost={:?}",
                self.positions
            ));
        }
        for asset in 0..ASSET_COUNT {
            let engine_asset = &group.assets[asset];
            if group.mode == MarketModeV16::Live
                && matches!(
                    engine_asset.lifecycle,
                    AssetLifecycleV16::Active | AssetLifecycleV16::DrainOnly
                )
                && engine_asset.oi_eff_long_q != engine_asset.oi_eff_short_q
            {
                return Err(format!(
                    "live asset {asset} has unmatched effective OI {}/{}",
                    engine_asset.oi_eff_long_q, engine_asset.oi_eff_short_q
                ));
            }
            if observed_long_count[asset] != engine_asset.stored_pos_count_long
                || observed_short_count[asset] != engine_asset.stored_pos_count_short
            {
                return Err(format!(
                    "asset {asset} stored position counts {}/{} != independent {}/{}",
                    engine_asset.stored_pos_count_long,
                    engine_asset.stored_pos_count_short,
                    observed_long_count[asset],
                    observed_short_count[asset]
                ));
            }
            if engine_asset.oi_eff_long_q > observed_long_oi[asset]
                || engine_asset.oi_eff_short_q > observed_short_oi[asset]
            {
                return Err(format!(
                    "asset {asset} effective OI {}/{} exceeds independently observed raw basis {}/{}",
                    engine_asset.oi_eff_long_q,
                    engine_asset.oi_eff_short_q,
                    observed_long_oi[asset],
                    observed_short_oi[asset]
                ));
            }
            for (side, effective_oi, raw_basis, stored_count, pending_count, mode, loss_weight) in [
                (
                    "long",
                    engine_asset.oi_eff_long_q,
                    observed_long_oi[asset],
                    engine_asset.stored_pos_count_long,
                    engine_asset.pending_obligation_count_long,
                    engine_asset.mode_long,
                    engine_asset.loss_weight_sum_long,
                ),
                (
                    "short",
                    engine_asset.oi_eff_short_q,
                    observed_short_oi[asset],
                    engine_asset.stored_pos_count_short,
                    engine_asset.pending_obligation_count_short,
                    engine_asset.mode_short,
                    engine_asset.loss_weight_sum_short,
                ),
            ] {
                if mode == SideModeV16::ResetPending && (effective_oi != 0 || loss_weight != 0) {
                    return Err(format!(
                        "asset {asset} {side} ResetPending retained effective OI {effective_oi} or loss weight {loss_weight}"
                    ));
                }
                let live_reducible = group.mode == MarketModeV16::Live
                    && matches!(
                        engine_asset.lifecycle,
                        AssetLifecycleV16::Active | AssetLifecycleV16::DrainOnly
                    );
                if live_reducible
                    && effective_oi == 0
                    && raw_basis != 0
                    && stored_count != 0
                    && pending_count == 0
                    && mode != SideModeV16::ResetPending
                {
                    return Err(format!(
                        "asset {asset} {side} has zero effective OI with raw basis {raw_basis} in {mode:?}; no bounded reset continuation"
                    ));
                }
            }
            if self.protocol_positions[asset] == 0
                && !asset_has_stale_leg[asset]
                && engine_asset.pending_obligation_count_long == 0
                && engine_asset.pending_obligation_count_short == 0
                && engine_asset.mode_long == SideModeV16::Normal
                && engine_asset.mode_short == SideModeV16::Normal
                && (observed_long_oi[asset] != engine_asset.oi_eff_long_q
                    || observed_short_oi[asset] != engine_asset.oi_eff_short_q)
            {
                return Err(format!(
                    "asset {asset} effective OI {}/{} != current-leg sum {}/{}; market={:?}, \
                     lifecycle={:?}, side_modes={:?}/{:?}, pending={}/{}, protocol_position={}",
                    engine_asset.oi_eff_long_q,
                    engine_asset.oi_eff_short_q,
                    observed_long_oi[asset],
                    observed_short_oi[asset],
                    group.mode,
                    engine_asset.lifecycle,
                    engine_asset.mode_long,
                    engine_asset.mode_short,
                    engine_asset.pending_obligation_count_long,
                    engine_asset.pending_obligation_count_short,
                    self.protocol_positions[asset],
                ));
            }
            let user_net: i128 = observed.iter().map(|positions| positions[asset]).sum();
            let net = user_net
                .checked_add(self.protocol_positions[asset])
                .ok_or("user/protocol position sum overflow")?;
            if net != 0 {
                return Err(format!(
                    "asset {asset} position attribution diverged: users={user_net}, \
                     protocol={}, net={net}; observed={observed:?}, ghost={:?}",
                    self.protocol_positions[asset], self.positions
                ));
            }
        }
        Ok(())
    }

    fn assert_source_domain_shape(
        &self,
        actor: usize,
        account: &percolator_prog::state::PortfolioAccountV16,
        group: &MarketGroupV16,
    ) -> Result<(), String> {
        let mut seen = [false; ASSET_COUNT * 2];
        for (slot, source) in account.source_domains.iter().copied().enumerate() {
            if !source.is_occupied() {
                if source.domain.get() != 0 || source.source_claim_market_id.get() != 0 {
                    return Err(format!(
                        "actor {actor} source slot {slot} retains a noncanonical empty tag"
                    ));
                }
                continue;
            }
            let domain = source.domain.get() as usize;
            if domain >= seen.len() {
                return Err(format!(
                    "actor {actor} source slot {slot} has out-of-world domain {domain}"
                ));
            }
            if seen[domain] {
                return Err(format!(
                    "actor {actor} has duplicate source-credit domain {domain}"
                ));
            }
            seen[domain] = true;
            let asset = domain / 2;
            if source.source_claim_market_id.get() != group.assets[asset].market_id {
                return Err(format!(
                    "actor {actor} source domain {domain} generation {} != current {}",
                    source.source_claim_market_id.get(),
                    group.assets[asset].market_id
                ));
            }
            let classified_lien = source
                .source_claim_counterparty_liened_num
                .get()
                .checked_add(source.source_claim_insurance_liened_num.get())
                .ok_or("classified source-lien overflow")?;
            if classified_lien != source.source_claim_liened_num.get() {
                return Err(format!(
                    "actor {actor} source domain {domain} double-counts or drops lien face"
                ));
            }
            let total_locked = source
                .source_claim_liened_num
                .get()
                .checked_add(source.source_claim_impaired_num.get())
                .ok_or("source locked-claim overflow")?;
            if total_locked > source.source_claim_bound_num.get() {
                return Err(format!(
                    "actor {actor} source domain {domain} locks more than its claim bound"
                ));
            }
        }
        Ok(())
    }

    fn trade_diagnostics(
        &self,
        taker: usize,
        maker: usize,
        legs: &[(usize, i128)],
    ) -> Result<String, String> {
        let (_, group) = self.env.primary_market_state();
        let taker_account = self.env.primary_portfolio(taker);
        let maker_account = self.env.primary_portfolio(maker);
        let assets: Vec<_> = legs
            .iter()
            .map(|(asset, _)| {
                let state = &group.assets[*asset];
                (
                    *asset,
                    state.lifecycle,
                    state.mode_long,
                    state.mode_short,
                    state.raw_oracle_target_price,
                    state.effective_price,
                    state.slot_last,
                )
            })
            .collect();
        Ok(format!(
            "market={{mode:{:?}, recovery:{:?}, slot:{}, current:{}, oracle_epoch:{}, \
             funding_epoch:{}, risk_epoch:{}, locks:[{},{},{}]}} assets={assets:?} \
             taker={{rank:{:?}, stale:{}, b_stale:{}, rebalance_lock:{}, liquidation_lock:{}, \
             cert:{:?}}} maker={{rank:{:?}, stale:{}, b_stale:{}, rebalance_lock:{}, \
             liquidation_lock:{}, cert:{:?}}}",
            group.mode,
            group.recovery_reason,
            group.slot_last,
            group.current_slot,
            group.oracle_epoch,
            group.funding_epoch,
            group.risk_epoch,
            group.bankruptcy_hlock_active,
            group.threshold_stress_active,
            group.loss_stale_active,
            self.progress_rank(taker)?,
            taker_account.stale_state,
            taker_account.b_stale_state,
            taker_account.rebalance_lock,
            taker_account.liquidation_lock,
            taker_account.health_cert.try_to_runtime().ok(),
            self.progress_rank(maker)?,
            maker_account.stale_state,
            maker_account.b_stale_state,
            maker_account.rebalance_lock,
            maker_account.liquidation_lock,
            maker_account.health_cert.try_to_runtime().ok(),
        ))
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            primary_market: self.env.market_data(false),
            foreign_market: self.env.market_data(true),
            primary_portfolios: self.env.all_primary_portfolio_data(),
            foreign_portfolio: self.env.foreign_portfolio_data(),
            backing_domain_ledger: self.env.backing_domain_ledger_data(),
            token_accounts: self.env.all_token_account_data(),
            matcher_contexts: self.env.all_matcher_context_data(),
            economic_account_lamports: self.env.all_economic_account_lamports(),
        }
    }

    fn assert_portfolio_frame(
        &self,
        before: &Snapshot,
        mutable_actors: &[usize],
    ) -> Result<(), String> {
        for actor in 0..PRIMARY_ACTOR_COUNT {
            if mutable_actors.contains(&actor) {
                continue;
            }
            let after = self.env.primary_portfolio_data(actor);
            if before.primary_portfolios[actor] != after {
                return Err(format!(
                    "action mutated unrelated primary portfolio {actor}"
                ));
            }
        }
        if before.foreign_portfolio != self.env.foreign_portfolio_data() {
            return Err("primary action mutated foreign portfolio".into());
        }
        if before.foreign_market != self.env.market_data(true) {
            return Err("primary action mutated foreign market".into());
        }
        Ok(())
    }

    fn assert_no_token_side_effects(&mut self, before: &Snapshot) -> Result<(), String> {
        self.assert_token_frame(before, &[])
    }

    fn assert_token_frame(
        &mut self,
        before: &Snapshot,
        mutable_tokens: &[Pubkey],
    ) -> Result<(), String> {
        let after = self.env.all_token_account_data();
        if before.token_accounts.len() != after.len() {
            return Err("tracked SPL account set changed during public instruction".into());
        }
        for ((before_key, before_data), (after_key, after_data)) in
            before.token_accounts.iter().zip(after.iter())
        {
            if before_key != after_key {
                return Err(
                    "tracked SPL account ordering changed during public instruction".into(),
                );
            }
            if !mutable_tokens.contains(before_key) && before_data != after_data {
                return Err(format!(
                    "public instruction mutated unauthorized SPL account {before_key}"
                ));
            }
        }
        self.coverage.token_frame_checks += 1;
        Ok(())
    }

    fn assert_snapshot_unchanged(&self, before: &Snapshot) -> Result<(), String> {
        if before.primary_market != self.env.market_data(false)
            || before.foreign_market != self.env.market_data(true)
            || before.primary_portfolios != self.env.all_primary_portfolio_data()
            || before.foreign_portfolio != self.env.foreign_portfolio_data()
            || before.backing_domain_ledger != self.env.backing_domain_ledger_data()
            || before.token_accounts != self.env.all_token_account_data()
            || before.matcher_contexts != self.env.all_matcher_context_data()
            || before.economic_account_lamports != self.env.all_economic_account_lamports()
        {
            return Err(
                "rejected public transaction changed account bytes, tokens, or lamports".into(),
            );
        }
        Ok(())
    }
}

pub fn run_scenario(scenario: &Scenario) -> Result<Coverage, String> {
    let mut progress_runner = ScenarioRunner::new(scenario)?;
    progress_runner.run_safety_prefix(&scenario.actions)?;
    if let Err(error) = progress_runner.run_permissionless_progress_campaign() {
        if !progress_runner.quarantine_known_progress_blocker(&error)? {
            return Err(error);
        }
    }

    let mut exit_runner = ScenarioRunner::new(scenario)?;
    exit_runner.run_safety_prefix(&scenario.actions)?;
    if let Err(error) = exit_runner.run_direct_user_exit_campaign() {
        let is_failed_exit = error.starts_with("normal exit needed public progress");
        if !is_failed_exit || !exit_runner.quarantine_known_progress_blocker(&error)? {
            return Err(error);
        }
        exit_runner.coverage.known_blocker_exit_locks
            [KnownBlocker::LiveLapsedSourceBacking.index()] += 1;
    }
    progress_runner.coverage.merge(exit_runner.coverage);

    let liquidation_coverage = run_liquidation_exit_probe(scenario.seed)?;
    progress_runner.coverage.merge(liquidation_coverage);
    progress_runner.coverage.assert_pull_request_non_vacuity()?;
    Ok(progress_runner.coverage)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundedLivenessGraphEvidence {
    pub scenario_count: usize,
    pub coverage: Coverage,
}

pub fn run_bounded_public_liveness_graph() -> Result<BoundedLivenessGraphEvidence, String> {
    let configs = [
        SmallMarketConfig {
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            max_abs_funding_e9_per_slot: 0,
            maintenance_fee_per_slot: 0,
        },
        SmallMarketConfig {
            max_price_move_bps_per_slot: 50,
            max_accrual_dt_slots: 2,
            max_abs_funding_e9_per_slot: 0,
            maintenance_fee_per_slot: 1,
        },
    ];
    let action_programs = [
        vec![],
        vec![Action::PushMark {
            asset: 0,
            dt: 1,
            move_bps: 500,
        }],
        vec![Action::PushMark {
            asset: 1,
            dt: 2,
            move_bps: -500,
        }],
        vec![
            Action::PushMark {
                asset: 0,
                dt: 1,
                move_bps: 500,
            },
            Action::PushMark {
                asset: 1,
                dt: 1,
                move_bps: -500,
            },
        ],
        vec![
            Action::SyncMaintenanceFee { actor: 0, dt: 2 },
            Action::PushMark {
                asset: 2,
                dt: 1,
                move_bps: 500,
            },
        ],
    ];

    let mut coverage = Coverage::default();
    let mut scenario_count = 0usize;
    for (config_index, config) in configs.into_iter().enumerate() {
        for (program_index, actions) in action_programs.iter().enumerate() {
            let mut seed = [0x82; 32];
            seed[0] ^= config_index as u8;
            seed[1] ^= program_index as u8;
            let scenario = Scenario {
                seed,
                config,
                actions: actions.clone(),
            };
            let mut runner = ScenarioRunner::new(&scenario)?;
            runner.run_safety_prefix(&scenario.actions)?;
            runner.run_permissionless_progress_campaign()?;
            coverage.merge(runner.coverage);
            scenario_count += 1;
        }
    }

    coverage.merge(run_liquidation_exit_probe([0x83; 32])?);
    Ok(BoundedLivenessGraphEvidence {
        scenario_count,
        coverage,
    })
}

const BOUNDED_REFERENCE_ACTION_COUNT: usize = 11;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct BoundedReferenceNode {
    positions: [[i128; ASSET_COUNT]; PRIMARY_ACTOR_COUNT],
    protocol_positions: [i128; ASSET_COUNT],
    capitals: [u128; PRIMARY_ACTOR_COUNT],
    market_mode: u8,
    lifecycles: [u8; ASSET_COUNT],
    side_modes: [[u8; 2]; ASSET_COUNT],
    effective_prices: [u64; ASSET_COUNT],
    raw_targets: [u64; ASSET_COUNT],
    wrapper_marks: [u64; ASSET_COUNT],
    oracle_authorities: [[u8; 32]; ASSET_COUNT],
    matcher_sequences: [u64; PRIMARY_ACTOR_COUNT],
    rank_classes: [u8; PRIMARY_ACTOR_COUNT],
    c_tot: u128,
    vault: u128,
    insurance: u128,
    source_claim_bound_total_num: u128,
    source_fresh_backing_total_num: u128,
    resolve_policy: [u64; 2],
    current_slot: u64,
    epochs: [u64; 4],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundedReferenceGraphEvidence {
    pub word_count: usize,
    pub transition_count: usize,
    pub unique_node_count: usize,
    pub unique_edge_count: usize,
    pub action_attempts: [u64; BOUNDED_REFERENCE_ACTION_COUNT],
    pub action_state_changes: [u64; BOUNDED_REFERENCE_ACTION_COUNT],
    pub coverage: Coverage,
}

impl ScenarioRunner {
    fn bounded_reference_node(&self) -> Result<BoundedReferenceNode, String> {
        self.assert_global_invariants()?;
        let (config, group) = self.env.primary_market_state();
        let capitals = std::array::from_fn(|actor| self.env.primary_portfolio(actor).capital.get());
        let lifecycles = std::array::from_fn(|asset| group.assets[asset].lifecycle as u8);
        let side_modes = std::array::from_fn(|asset| {
            [
                group.assets[asset].mode_long as u8,
                group.assets[asset].mode_short as u8,
            ]
        });
        let effective_prices = std::array::from_fn(|asset| group.assets[asset].effective_price);
        let raw_targets = std::array::from_fn(|asset| group.assets[asset].raw_oracle_target_price);
        let wrapper_marks =
            std::array::from_fn(|asset| self.env.primary_profile(asset).mark_ewma_e6);
        let oracle_authorities =
            std::array::from_fn(|asset| self.env.primary_profile(asset).oracle_authority);
        let matcher_sequences =
            std::array::from_fn(|actor| self.env.primary_portfolio_matcher_sequence(actor));
        let mut rank_classes = [0u8; PRIMARY_ACTOR_COUNT];
        for (actor, class) in rank_classes.iter_mut().enumerate() {
            *class = self.progress_rank(actor)?.class_mask();
        }
        let source_fresh_backing_total_num =
            group
                .source_credit
                .iter()
                .try_fold(0u128, |total, source| {
                    total
                        .checked_add(source.fresh_reserved_backing_num)
                        .ok_or("bounded reference backing total overflow")
                })?;

        Ok(BoundedReferenceNode {
            positions: self.positions,
            protocol_positions: self.protocol_positions,
            capitals,
            market_mode: group.mode as u8,
            lifecycles,
            side_modes,
            effective_prices,
            raw_targets,
            wrapper_marks,
            oracle_authorities,
            matcher_sequences,
            rank_classes,
            c_tot: group.c_tot,
            vault: group.vault,
            insurance: group.insurance,
            source_claim_bound_total_num: group.source_claim_bound_total_num,
            source_fresh_backing_total_num,
            resolve_policy: [
                config.permissionless_resolve_stale_slots,
                config.force_close_delay_slots,
            ],
            current_slot: group.current_slot,
            epochs: [
                group.oracle_epoch,
                group.funding_epoch,
                group.risk_epoch,
                group.asset_set_epoch,
            ],
        })
    }
}

fn bounded_reference_actions() -> [Action; BOUNDED_REFERENCE_ACTION_COUNT] {
    [
        Action::Deposit {
            actor: 0,
            amount: 17,
        },
        Action::Withdraw {
            actor: 0,
            amount: 11,
        },
        Action::Trade {
            route: TradeRoute::NoCpi,
            taker: 0,
            maker: 1,
            asset: 0,
            units: 1,
            fee_bps: 0,
            price_move_bps: 0,
            prefer_reduce: false,
        },
        Action::Trade {
            route: TradeRoute::BatchCpi,
            taker: 2,
            maker: 3,
            asset: 1,
            units: 1,
            fee_bps: 0,
            price_move_bps: 0,
            prefer_reduce: false,
        },
        Action::PushMark {
            asset: 0,
            dt: 1,
            move_bps: 100,
        },
        Action::Crank {
            actor: 0,
            hints: HintMode::Complete,
        },
        Action::SetMatcherConfig {
            actor: 1,
            enabled: false,
            trade_fee_cap_bps: 0,
        },
        Action::TopUpBacking {
            domain: 0,
            amount: 7,
            expiry_delta: 3,
        },
        Action::RotateOracleAuthority {
            asset: 0,
            new_actor: 1,
        },
        Action::ConfigurePermissionlessResolve {
            stale_slots: 1_000,
            force_close_delay_slots: 100,
        },
        Action::ShutdownAsset { asset: 2, dt: 1 },
    ]
}

pub fn run_bounded_reference_equivalence_graph() -> Result<BoundedReferenceGraphEvidence, String> {
    type Edge = (BoundedReferenceNode, u8, BoundedReferenceNode);

    fn replay_word(
        word: &[(usize, Action)],
        nodes: &mut BTreeSet<BoundedReferenceNode>,
        edges: &mut BTreeSet<Edge>,
        action_attempts: &mut [u64; BOUNDED_REFERENCE_ACTION_COUNT],
        action_state_changes: &mut [u64; BOUNDED_REFERENCE_ACTION_COUNT],
        coverage: &mut Coverage,
    ) -> Result<(), String> {
        let scenario = Scenario {
            seed: [0x86; 32],
            config: SmallMarketConfig {
                max_price_move_bps_per_slot: 500,
                max_accrual_dt_slots: 2,
                max_abs_funding_e9_per_slot: 0,
                maintenance_fee_per_slot: 1,
            },
            actions: vec![],
        };
        let mut runner = ScenarioRunner::new_unprefixed(&scenario)?;
        let mut before = runner.bounded_reference_node()?;
        nodes.insert(before.clone());
        for (action_index, action) in word {
            runner.run_safety_prefix(std::slice::from_ref(action))?;
            let after = runner.bounded_reference_node()?;
            action_attempts[*action_index] += 1;
            if after != before {
                action_state_changes[*action_index] += 1;
            }
            nodes.insert(after.clone());
            edges.insert((before, *action_index as u8, after.clone()));
            before = after;
        }
        coverage.merge(runner.coverage);
        Ok(())
    }

    let actions = bounded_reference_actions();
    let mut nodes = BTreeSet::new();
    let mut edges = BTreeSet::new();
    let mut action_attempts = [0u64; BOUNDED_REFERENCE_ACTION_COUNT];
    let mut action_state_changes = [0u64; BOUNDED_REFERENCE_ACTION_COUNT];
    let mut coverage = Coverage::default();
    let mut word_count = 0usize;
    let mut transition_count = 0usize;

    replay_word(
        &[],
        &mut nodes,
        &mut edges,
        &mut action_attempts,
        &mut action_state_changes,
        &mut coverage,
    )?;
    word_count += 1;
    for (first_index, first) in actions.iter().enumerate() {
        replay_word(
            &[(first_index, first.clone())],
            &mut nodes,
            &mut edges,
            &mut action_attempts,
            &mut action_state_changes,
            &mut coverage,
        )?;
        word_count += 1;
        transition_count += 1;
        for (second_index, second) in actions.iter().enumerate() {
            replay_word(
                &[(first_index, first.clone()), (second_index, second.clone())],
                &mut nodes,
                &mut edges,
                &mut action_attempts,
                &mut action_state_changes,
                &mut coverage,
            )?;
            word_count += 1;
            transition_count += 2;
        }
    }

    Ok(BoundedReferenceGraphEvidence {
        word_count,
        transition_count,
        unique_node_count: nodes.len(),
        unique_edge_count: edges.len(),
        action_attempts,
        action_state_changes,
        coverage,
    })
}

#[allow(dead_code)]
pub fn verify_positive_claim_bound_attribution_lifecycle(
    mut seed: [u8; 32],
    position_units: u8,
    price_move: u8,
    reverse_conversion_order: bool,
) -> Result<(), String> {
    const WINNERS: [usize; 2] = [0, 1];
    const LOSERS: [usize; 2] = [2, 3];
    const MARKET_CRANKER: usize = 4;
    const ASSET: u16 = 0;
    const SOURCE_DOMAIN: usize = 1;
    const START_PRICE: u64 = 100;
    const EXPIRY_SLOT: u64 = 20;

    fn convert_and_check(
        env: &mut V16Svm,
        actor: usize,
        domain: usize,
        amount: u128,
        step: &str,
    ) -> Result<(), String> {
        let (_, before_group) = env.primary_market_state();
        let before_account = env.primary_portfolio(actor);
        let before_claim = source_claim_for_domain(&before_account, domain);
        let before_pnl = before_account.pnl.get();
        let before_capital = before_account.capital.get();
        let before_vault_tokens = env.token_amount(env.vault);
        let amount_i128 = i128::try_from(amount)
            .map_err(|_| format!("{step} conversion amount does not fit i128"))?;
        let burn_num = amount
            .checked_mul(percolator::BOUND_SCALE)
            .ok_or_else(|| format!("{step} claim-burn conversion overflow"))?;
        if amount == 0 || before_pnl < amount_i128 || before_claim < burn_num {
            return Err(format!(
                "{step} invalid generated conversion: amount={amount}, pnl={before_pnl}, \
                 claim={before_claim}"
            ));
        }

        env.convert_released_pnl(actor, amount)
            .map_err(|error| format!("{step} public ConvertReleasedPnl: {error}"))?;
        let (_, after_group) = env.primary_market_state();
        let after_account = env.primary_portfolio(actor);
        let after_claim = source_claim_for_domain(&after_account, domain);
        if after_claim.checked_add(burn_num) != Some(before_claim)
            || after_group.source_credit[domain]
                .positive_claim_bound_num
                .checked_add(burn_num)
                != Some(before_group.source_credit[domain].positive_claim_bound_num)
            || after_group
                .source_claim_bound_total_num
                .checked_add(burn_num)
                != Some(before_group.source_claim_bound_total_num)
        {
            return Err(format!(
                "{step} did not burn exactly one attributed claim delta: account \
                 {before_claim}->{after_claim}, domain {}->{}, total {}->{}",
                before_group.source_credit[domain].positive_claim_bound_num,
                after_group.source_credit[domain].positive_claim_bound_num,
                before_group.source_claim_bound_total_num,
                after_group.source_claim_bound_total_num
            ));
        }
        if after_account.pnl.get() != before_pnl - amount_i128
            || after_account.capital.get().checked_sub(before_capital) != Some(amount)
            || after_group.vault != before_group.vault
            || env.token_amount(env.vault) != before_vault_tokens
        {
            return Err(format!(
                "{step} conversion did not preserve custody and reclassify exactly {amount}"
            ));
        }
        assert_primary_source_claim_bound_attribution(step, env)
    }

    seed[0] ^= 0x29;
    let position_units = u128::from(position_units.clamp(1, 4));
    let price_move = u64::from(price_move.clamp(5, 20));
    let position_q_u128 = position_units
        .checked_mul(POS_SCALE)
        .ok_or("INV-029 position multiplication overflow")?;
    let position_q =
        i128::try_from(position_q_u128).map_err(|_| "INV-029 position does not fit i128")?;
    let winning_price = START_PRICE
        .checked_add(price_move)
        .ok_or("INV-029 winning price overflow")?;
    let reduced_price_move = price_move / 2;
    let reduced_price = START_PRICE
        .checked_add(reduced_price_move)
        .ok_or("INV-029 reduced price overflow")?;
    let expected_claim_per_winner = position_units
        .checked_mul(u128::from(price_move))
        .ok_or("INV-029 expected claim overflow")?;
    let backing = expected_claim_per_winner
        .checked_mul(WINNERS.len() as u128)
        .and_then(|value| value.checked_add(100))
        .ok_or("INV-029 backing setup overflow")?;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: START_PRICE,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [1_000, 1_000, 1_000, 1_000, 1],
            actor_token_balances: [1_000, 1_000, 1_000, 1_000, 1],
            ..MarketConfig::default()
        },
    );
    env.configure_auth_mark(false, ASSET, 1, START_PRICE)
        .map_err(|error| format!("INV-029 configure AuthMark: {error}"))?;
    env.top_up_backing_bucket(SOURCE_DOMAIN as u16, backing, EXPIRY_SLOT)
        .map_err(|error| format!("INV-029 backing top-up: {error}"))?;
    assert_primary_source_claim_bound_attribution("INV-029 initialized", &env)?;

    for (&winner, &loser) in WINNERS.iter().zip(LOSERS.iter()) {
        env.trade_no_cpi(winner, loser, ASSET, position_q, START_PRICE, 0)
            .map_err(|error| format!("INV-029 open pair {winner}/{loser}: {error}"))?;
        assert_primary_source_claim_bound_attribution("INV-029 after open", &env)?;
    }
    for slot in 2..=3 {
        env.warp_to_slot(slot);
        env.push_auth_mark(ASSET, slot, winning_price)
            .map_err(|error| format!("INV-029 publish winning mark at {slot}: {error}"))?;
        crank_adapter_steps(&mut env, MARKET_CRANKER, slot, ASSET, 8)?;
    }
    for actor in 0..4 {
        crank_adapter_steps(&mut env, actor, 3, ASSET, 16)?;
        assert_primary_source_claim_bound_attribution("INV-029 after account crank", &env)?;
    }

    let (_, peak_group) = env.primary_market_state();
    let peak_claims = WINNERS
        .map(|winner| source_claim_for_domain(&env.primary_portfolio(winner), SOURCE_DOMAIN));
    let expected_claim_num = expected_claim_per_winner
        .checked_mul(percolator::BOUND_SCALE)
        .ok_or("INV-029 expected claim-num overflow")?;
    let expected_peak_domain_claim = expected_claim_num
        .checked_mul(WINNERS.len() as u128)
        .ok_or("INV-029 peak domain claim overflow")?;
    if peak_claims != [expected_claim_num; 2]
        || peak_group.source_credit[SOURCE_DOMAIN].positive_claim_bound_num
            != expected_peak_domain_claim
    {
        return Err(format!(
            "INV-029 setup did not create two independently attributed claims: \
             claims={peak_claims:?}, domain={}",
            peak_group.source_credit[SOURCE_DOMAIN].positive_claim_bound_num
        ));
    }

    for slot in 4..=5 {
        env.warp_to_slot(slot);
        env.push_auth_mark(ASSET, slot, reduced_price)
            .map_err(|error| format!("INV-029 publish reduced mark at {slot}: {error}"))?;
        crank_adapter_steps(&mut env, MARKET_CRANKER, slot, ASSET, 8)?;
    }
    for actor in 0..4 {
        crank_adapter_steps(&mut env, actor, 5, ASSET, 16)?;
        assert_primary_source_claim_bound_attribution("INV-029 after partial claim burn", &env)?;
    }
    let reduced_claim_per_winner = position_units
        .checked_mul(u128::from(reduced_price_move))
        .and_then(|value| value.checked_mul(percolator::BOUND_SCALE))
        .ok_or("INV-029 reduced claim-num overflow")?;
    let expected_reduced_domain_claim = reduced_claim_per_winner
        .checked_mul(WINNERS.len() as u128)
        .ok_or("INV-029 reduced domain claim overflow")?;
    let (_, reduced_group) = env.primary_market_state();
    let reduced_claims = WINNERS
        .map(|winner| source_claim_for_domain(&env.primary_portfolio(winner), SOURCE_DOMAIN));
    if reduced_claims != [reduced_claim_per_winner; 2]
        || reduced_group.source_credit[SOURCE_DOMAIN].positive_claim_bound_num
            != expected_reduced_domain_claim
        || reduced_group.source_credit[SOURCE_DOMAIN].positive_claim_bound_num
            >= peak_group.source_credit[SOURCE_DOMAIN].positive_claim_bound_num
    {
        return Err(format!(
            "INV-029 less-favorable authenticated mark did not partially burn both claims: \
             peak={peak_claims:?}, reduced={reduced_claims:?}, domain={}",
            reduced_group.source_credit[SOURCE_DOMAIN].positive_claim_bound_num
        ));
    }

    for (&winner, &loser) in WINNERS.iter().zip(LOSERS.iter()) {
        env.trade_no_cpi(winner, loser, ASSET, -position_q, reduced_price, 0)
            .map_err(|error| format!("INV-029 close pair {winner}/{loser}: {error}"))?;
        assert_primary_source_claim_bound_attribution("INV-029 after close", &env)?;
        if position_abs_for_asset(&env.primary_portfolio(winner), ASSET as usize).is_ok() {
            return Err(format!(
                "INV-029 winner {winner} retained a leg after full close"
            ));
        }
    }
    let (_, closed_group) = env.primary_market_state();
    if closed_group.source_credit[SOURCE_DOMAIN].positive_claim_bound_num
        != reduced_group.source_credit[SOURCE_DOMAIN].positive_claim_bound_num
    {
        return Err("INV-029 flattening changed claim attribution before conversion".into());
    }
    let expected_reduced_pnl = i128::try_from(
        position_units
            .checked_mul(u128::from(reduced_price_move))
            .ok_or("INV-029 reduced PnL overflow")?,
    )
    .map_err(|_| "INV-029 reduced PnL does not fit i128")?;
    let closed_pnls = [0usize, 1, 2, 3].map(|actor| env.primary_portfolio(actor).pnl.get());
    let expected_recovery_pnl = i128::try_from(
        position_units
            .checked_mul(u128::from(price_move - reduced_price_move))
            .ok_or("INV-029 recovery PnL overflow")?,
    )
    .map_err(|_| "INV-029 recovery PnL does not fit i128")?;
    if closed_pnls
        != [
            expected_reduced_pnl,
            expected_reduced_pnl,
            expected_recovery_pnl,
            expected_recovery_pnl,
        ]
    {
        return Err(format!(
            "INV-029 closing both pairs at the effective mark changed economic PnL: expected \
             winners={expected_reduced_pnl}, settled-loss recovery={expected_recovery_pnl}, \
             observed={closed_pnls:?}, \
             effective_price={}, raw_target={}",
            closed_group.assets[ASSET as usize].effective_price,
            closed_group.assets[ASSET as usize].raw_oracle_target_price
        ));
    }

    let order = if reverse_conversion_order {
        [WINNERS[1], WINNERS[0]]
    } else {
        WINNERS
    };
    let first_pnl = env.primary_portfolio(order[0]).pnl.get();
    let first_pnl = u128::try_from(first_pnl)
        .map_err(|_| "INV-029 first winner did not retain positive released PnL")?;
    convert_and_check(
        &mut env,
        order[0],
        SOURCE_DOMAIN,
        first_pnl,
        "INV-029 first-winner conversion",
    )?;
    crank_adapter_steps(&mut env, order[1], 5, ASSET, 16).map_err(|error| {
        format!("INV-029 recertify second winner after peer conversion: {error}")
    })?;
    assert_primary_source_claim_bound_attribution("INV-029 after peer recertification", &env)?;
    let second_pnl = env.primary_portfolio(order[1]).pnl.get();
    let second_pnl = u128::try_from(second_pnl)
        .map_err(|_| "INV-029 second winner did not retain positive released PnL")?;
    convert_and_check(
        &mut env,
        order[1],
        SOURCE_DOMAIN,
        second_pnl,
        "INV-029 second-winner conversion",
    )?;

    const RECOVERY_SOURCE_DOMAIN: usize = 0;
    let recovery_claim_per_loser = u128::try_from(expected_recovery_pnl)
        .map_err(|_| "INV-029 recovery claim does not fit u128")?;
    let recovery_claim_num = recovery_claim_per_loser
        .checked_mul(percolator::BOUND_SCALE)
        .ok_or("INV-029 recovery claim-num overflow")?;
    let expected_recovery_domain_claim = recovery_claim_num
        .checked_mul(LOSERS.len() as u128)
        .ok_or("INV-029 recovery domain claim overflow")?;
    let recovery_claims = LOSERS.map(|loser| {
        source_claim_for_domain(&env.primary_portfolio(loser), RECOVERY_SOURCE_DOMAIN)
    });
    let (_, winner_converted_group) = env.primary_market_state();
    if recovery_claims != [recovery_claim_num; 2]
        || winner_converted_group.source_credit[RECOVERY_SOURCE_DOMAIN].positive_claim_bound_num
            != expected_recovery_domain_claim
    {
        return Err(format!(
            "INV-029 settled-loss recovery was not attributed to the opposite source domain: \
             claims={recovery_claims:?}, domain={}",
            winner_converted_group.source_credit[RECOVERY_SOURCE_DOMAIN].positive_claim_bound_num
        ));
    }
    let recovery_backing = recovery_claim_per_loser
        .checked_mul(LOSERS.len() as u128)
        .and_then(|value| value.checked_add(100))
        .ok_or("INV-029 recovery backing overflow")?;
    env.top_up_backing_bucket_without_ledger(
        RECOVERY_SOURCE_DOMAIN as u16,
        recovery_backing,
        EXPIRY_SLOT,
    )
    .map_err(|error| format!("INV-029 recovery-domain backing top-up: {error}"))?;
    assert_primary_source_claim_bound_attribution("INV-029 after recovery backing", &env)?;

    let loser_order = if reverse_conversion_order {
        [LOSERS[1], LOSERS[0]]
    } else {
        LOSERS
    };
    for (index, loser) in loser_order.into_iter().enumerate() {
        crank_adapter_steps(&mut env, loser, 5, ASSET, 16)
            .map_err(|error| format!("INV-029 recertify recovery claimant {loser}: {error}"))?;
        assert_primary_source_claim_bound_attribution("INV-029 recovery recertification", &env)?;
        let recovery_pnl = u128::try_from(env.primary_portfolio(loser).pnl.get())
            .map_err(|_| format!("INV-029 recovery claimant {loser} lost positive PnL"))?;
        convert_and_check(
            &mut env,
            loser,
            RECOVERY_SOURCE_DOMAIN,
            recovery_pnl,
            if index == 0 {
                "INV-029 first recovery conversion"
            } else {
                "INV-029 second recovery conversion"
            },
        )?;
    }

    let (_, terminal_group) = env.primary_market_state();
    if terminal_group.source_credit[SOURCE_DOMAIN].positive_claim_bound_num != 0
        || terminal_group.source_claim_bound_total_num != 0
        || WINNERS.iter().any(|winner| {
            source_claim_for_domain(&env.primary_portfolio(*winner), SOURCE_DOMAIN) != 0
        })
    {
        let domain_claims: Vec<_> = terminal_group
            .source_credit
            .iter()
            .map(|source| source.positive_claim_bound_num)
            .collect();
        let portfolio_claims: Vec<_> = (0..PRIMARY_ACTOR_COUNT)
            .map(|actor| {
                let portfolio = env.primary_portfolio(actor);
                let claims: Vec<_> = portfolio
                    .source_domains
                    .iter()
                    .filter(|source| source.is_occupied())
                    .map(|source| (source.domain.get(), source.source_claim_bound_num.get()))
                    .collect();
                (actor, portfolio.pnl.get(), claims)
            })
            .collect();
        return Err(format!(
            "INV-029 completed conversions left source claims: total={}, domains={domain_claims:?}, \
             portfolios={portfolio_claims:?}",
            terminal_group.source_claim_bound_total_num
        ));
    }
    assert_primary_source_claim_bound_attribution("INV-029 terminal", &env)
}

#[allow(dead_code)]
pub fn verify_source_credit_rate_lifecycle(
    mut seed: [u8; 32],
    initial_backing: u16,
    added_backing: u16,
    price_move: u8,
) -> Result<(), String> {
    const WINNERS: [usize; 2] = [0, 1];
    const LOSERS: [usize; 2] = [2, 3];
    const MARKET_CRANKER: usize = 4;
    const ASSET: u16 = 0;
    const SOURCE_DOMAIN: usize = 1;
    const START_PRICE: u64 = 100;
    const POSITION_Q: i128 = POS_SCALE as i128;
    const EXPIRY_SLOT: u64 = 6;

    seed[0] ^= 0x30;
    let initial_backing = u128::from(initial_backing.clamp(1, 100));
    let added_backing = u128::from(added_backing.clamp(1, 100));
    let price_move = u64::from(price_move.clamp(5, 20));
    let winning_price = 230u64
        .checked_add(price_move)
        .ok_or("INV-030 winning price overflow")?;
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: START_PRICE,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [150, 150, 150, 150, 1],
            actor_token_balances: [150, 150, 150, 150, 1],
            ..MarketConfig::default()
        },
    );
    env.configure_auth_mark(false, ASSET, 1, START_PRICE)
        .map_err(|error| format!("INV-030 configure AuthMark: {error}"))?;
    env.top_up_backing_bucket(SOURCE_DOMAIN as u16, initial_backing, EXPIRY_SLOT)
        .map_err(|error| format!("INV-030 initial backing top-up: {error}"))?;
    for (&winner, &loser) in WINNERS.iter().zip(LOSERS.iter()) {
        env.trade_no_cpi(winner, loser, ASSET, POSITION_Q, START_PRICE, 0)
            .map_err(|error| format!("INV-030 open balanced pair {winner}/{loser}: {error}"))?;
    }

    for slot in 2..=3 {
        env.warp_to_slot(slot);
        env.push_auth_mark(ASSET, slot, winning_price)
            .map_err(|error| format!("INV-030 publish winning mark at {slot}: {error}"))?;
        crank_adapter_steps(&mut env, MARKET_CRANKER, slot, ASSET, 8)?;
    }
    for winner in WINNERS {
        crank_adapter_steps(&mut env, winner, 3, ASSET, 16)?;
    }
    let (_, after_claim) = env.primary_market_state();
    assert_source_credit_rates("INV-030 after claim", &after_claim)?;
    let claim_source = after_claim.source_credit[SOURCE_DOMAIN];
    if claim_source.positive_claim_bound_num == 0
        || claim_source.credit_rate_num == 0
        || claim_source.credit_rate_num >= CREDIT_RATE_SCALE
    {
        return Err(format!(
            "INV-030 setup did not create a discounted source claim: {claim_source:?}"
        ));
    }
    let baseline_rate = claim_source.credit_rate_num;
    let claim_bound = claim_source.positive_claim_bound_num;

    env.top_up_backing_bucket(SOURCE_DOMAIN as u16, added_backing, EXPIRY_SLOT)
        .map_err(|error| format!("INV-030 incremental backing top-up: {error}"))?;
    let (_, after_add) = env.primary_market_state();
    assert_source_credit_rates("INV-030 after backing add", &after_add)?;
    let raised_rate = after_add.source_credit[SOURCE_DOMAIN].credit_rate_num;
    if raised_rate <= baseline_rate || raised_rate >= CREDIT_RATE_SCALE {
        return Err(format!(
            "INV-030 fresh backing did not strictly and conservatively raise the rate: \
             {baseline_rate}->{raised_rate}, claim={claim_bound}"
        ));
    }
    let vault_before_expiry = after_add.vault;
    let vault_tokens_before_expiry = env.token_amount(env.vault);

    env.warp_to_slot(EXPIRY_SLOT);
    env.push_auth_mark(ASSET, EXPIRY_SLOT, winning_price)
        .map_err(|error| format!("INV-030 publish expiry-boundary mark: {error}"))?;
    crank_adapter_steps(&mut env, WINNERS[0], EXPIRY_SLOT, ASSET, 24)?;
    let (_, after_expiry) = env.primary_market_state();
    assert_source_credit_rates("INV-030 after expiry", &after_expiry)?;
    let expired_bucket = after_expiry.source_backing_buckets[SOURCE_DOMAIN];
    let expired_source = after_expiry.source_credit[SOURCE_DOMAIN];
    if expired_bucket.status == BackingBucketStatusV16::Fresh
        || expired_source.positive_claim_bound_num != claim_bound
        || expired_source.credit_rate_num != 0
    {
        return Err(format!(
            "INV-030 expiry did not fail closed without deleting the claim: \
             bucket={expired_bucket:?}, source={expired_source:?}, claim={claim_bound}"
        ));
    }
    if after_expiry.vault != vault_before_expiry
        || env.token_amount(env.vault) != vault_tokens_before_expiry
    {
        return Err("INV-030 backing expiry moved internal or SPL custody".into());
    }

    let position_before_exit =
        position_abs_for_asset(&env.primary_portfolio(WINNERS[0]), ASSET as usize)?;
    env.rebalance_reduce(WINNERS[0], ASSET, POS_SCALE / 4)
        .map_err(|error| format!("INV-030 owner reduction after fail-closed expiry: {error}"))?;
    let position_after_exit =
        position_abs_for_asset(&env.primary_portfolio(WINNERS[0]), ASSET as usize)?;
    if position_after_exit != position_before_exit - POS_SCALE / 4
        || env.token_amount(env.vault) != vault_tokens_before_expiry
    {
        return Err(format!(
            "INV-030 owner reduction did not preserve custody and reduce risk: \
             {position_before_exit}->{position_after_exit}"
        ));
    }

    let refill = initial_backing
        .checked_add(added_backing)
        .ok_or("INV-030 refill overflow")?;
    env.top_up_backing_bucket(
        SOURCE_DOMAIN as u16,
        refill,
        EXPIRY_SLOT
            .checked_add(10)
            .ok_or("INV-030 refill expiry overflow")?,
    )
    .map_err(|error| format!("INV-030 refill expired source: {error}"))?;
    let (_, after_refill) = env.primary_market_state();
    assert_source_credit_rates("INV-030 after refill", &after_refill)?;
    let refilled_source = after_refill.source_credit[SOURCE_DOMAIN];
    if after_refill.source_backing_buckets[SOURCE_DOMAIN].status != BackingBucketStatusV16::Fresh
        || refilled_source.credit_rate_num == 0
        || refilled_source.credit_rate_num >= CREDIT_RATE_SCALE
    {
        return Err(format!(
            "INV-030 independently backed refill did not restore bounded credit: \
             {:?}",
            refilled_source
        ));
    }
    Ok(())
}

fn scenario_liveness_limit(scenario: &Scenario) -> Result<usize, String> {
    let cap_bps_per_step = scenario
        .config
        .max_price_move_bps_per_slot
        .checked_mul(scenario.config.max_accrual_dt_slots.max(1))
        .ok_or("liveness cap multiplication overflow")?
        .max(1);
    let mut mark_steps = div_ceil_u64(100, cap_bps_per_step);
    let mut authenticated_dt = 1u64;
    for action in &scenario.actions {
        match action {
            Action::PushMark { dt, move_bps, .. } => {
                mark_steps = mark_steps
                    .checked_add(div_ceil_u64(
                        u64::from(move_bps.unsigned_abs().min(500)),
                        cap_bps_per_step,
                    ))
                    .ok_or("liveness mark-step bound overflow")?;
                authenticated_dt = authenticated_dt
                    .checked_add(u64::from((*dt).clamp(1, 4)))
                    .ok_or("liveness clock bound overflow")?;
            }
            Action::SyncMaintenanceFee { dt, .. } => {
                authenticated_dt = authenticated_dt
                    .checked_add(u64::from((*dt).clamp(1, 4)))
                    .ok_or("liveness fee-clock bound overflow")?;
            }
            Action::ShutdownAsset { dt, .. } => {
                authenticated_dt = authenticated_dt
                    .checked_add(u64::from((*dt).min(4)))
                    .ok_or("liveness shutdown-clock bound overflow")?;
            }
            _ => {}
        }
    }
    let slot_steps = div_ceil_u64(
        authenticated_dt,
        scenario.config.max_accrual_dt_slots.max(1),
    )
    .checked_mul(ASSET_COUNT as u64)
    .ok_or("liveness slot-step bound overflow")?;
    let global_step_fanout = (PRIMARY_ACTOR_COUNT as u64)
        .checked_add(1)
        .ok_or("liveness fanout overflow")?;
    let account_steps = (PRIMARY_ACTOR_COUNT as u64)
        .checked_mul(ASSET_COUNT as u64)
        .and_then(|value| value.checked_mul(16))
        // Settling one account's source-attributed K/F claim may advance risk_epoch and
        // invalidate every modeled certificate once before the account-local sweep converges.
        .and_then(|value| value.checked_mul(global_step_fanout))
        .ok_or("liveness account-step bound overflow")?;
    let derived = mark_steps
        // A market step can invalidate every modeled portfolio certificate once;
        // the extra step is the market continuation itself.
        .checked_mul(global_step_fanout)
        .and_then(|value| value.checked_add(slot_steps))
        .and_then(|value| value.checked_add(account_steps))
        // Each global source domain can add one bounded Fresh -> Expired/Impaired
        // continuation plus one certificate refresh per modeled portfolio.
        .and_then(|value| {
            (PORTFOLIO_SOURCE_DOMAIN_CAP as u64)
                .checked_mul(global_step_fanout)
                .and_then(|source_steps| value.checked_add(source_steps))
        })
        .and_then(|value| value.checked_add(64))
        .ok_or("liveness total bound overflow")?;
    let derived =
        usize::try_from(derived).map_err(|_| "liveness bound does not fit usize".to_string())?;
    let limit = derived.max(MIN_LIVENESS_DRAIN_LIMIT);
    if limit > MAX_LIVENESS_DRAIN_LIMIT {
        return Err(format!(
            "derived liveness bound {limit} exceeds harness guard {MAX_LIVENESS_DRAIN_LIMIT}"
        ));
    }
    Ok(limit)
}

fn div_ceil_u64(value: u64, divisor: u64) -> u64 {
    value.div_ceil(divisor)
}

fn run_liquidation_exit_probe(mut seed: [u8; 32]) -> Result<Coverage, String> {
    seed[0] ^= 0xa5;
    let scenario = Scenario {
        seed,
        config: SmallMarketConfig {
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            max_abs_funding_e9_per_slot: 0,
            maintenance_fee_per_slot: 0,
        },
        actions: vec![],
    };
    let mut runner = ScenarioRunner::new_unprefixed(&scenario)?;
    let open_q = 80i128
        .checked_mul(POS_SCALE as i128)
        .ok_or("liquidation probe size overflow")?;
    runner.execute_trade(
        TradeRoute::NoCpi,
        0,
        EXIT_MAKER_INDEX,
        vec![(0, -open_q)],
        0,
        0,
        true,
    )?;

    let mark_slot = runner.env.current_slot() + 1;
    runner.env.warp_to_slot(mark_slot);
    let adverse_mark = super::v16_svm::INITIAL_PRICE
        .checked_mul(2)
        .ok_or("liquidation mark overflow")?;
    let before_mark = runner.snapshot();
    let mark = runner
        .env
        .push_auth_mark(0, mark_slot, adverse_mark)
        .map_err(|error| format!("liquidation probe mark rejected: {error}"))?;
    runner.coverage.mark_updates += 1;
    runner.coverage.observe_success(None, &mark);
    runner.assert_no_token_side_effects(&before_mark)?;
    runner.drain_actor(0, runner.liveness_limit)?;
    if runner.coverage.liquidation_steps == 0 {
        return Err("adverse-price probe reached no public liquidation step".into());
    }

    let remaining = runner.positions[0][0];
    if remaining != 0 {
        runner.execute_trade(
            TradeRoute::NoCpi,
            0,
            EXIT_MAKER_INDEX,
            vec![(0, -remaining)],
            0,
            0,
            true,
        )?;
        runner.coverage.user_positions_closed += 1;
    }
    runner.drain_actor(0, runner.liveness_limit)?;
    if runner.positions[0] != [0; ASSET_COUNT] {
        return Err(format!(
            "liquidated user retained positions after public risk reduction: {:?}",
            runner.positions[0]
        ));
    }

    let capital = runner.env.primary_portfolio(0).capital.get();
    let before_withdrawal = runner.snapshot();
    let vault_before = u128::from(runner.env.token_amount(runner.env.vault));
    let destination_before = runner
        .env
        .token_amount(runner.env.actors[0].destination_token);
    let withdrawal = runner
        .env
        .withdraw_primary(0, capital)
        .map_err(|error| format!("liquidated user cannot withdraw remaining capital: {error}"))?;
    runner.coverage.withdrawals += 1;
    runner.coverage.observe_success(None, &withdrawal);
    let destination_after = runner
        .env
        .token_amount(runner.env.actors[0].destination_token);
    let vault_after = u128::from(runner.env.token_amount(runner.env.vault));
    if u128::from(destination_before).checked_add(capital) != Some(u128::from(destination_after))
        || vault_after.checked_add(capital) != Some(vault_before)
    {
        return Err("liquidated user's owner withdrawal was not credited exactly".into());
    }
    runner.assert_token_frame(
        &before_withdrawal,
        &[runner.env.actors[0].destination_token, runner.env.vault],
    )?;
    runner.assert_global_invariants()?;
    Ok(runner.coverage)
}

fn tracked_economic_accounts(env: &V16Svm) -> Vec<(Pubkey, Option<solana_sdk::account::Account>)> {
    let mut keys = vec![
        env.market,
        env.foreign_market,
        env.mint,
        env.vault,
        env.foreign_vault,
        env.backing_domain_ledger,
        env.provider_source_token,
        env.provider_destination_token,
        env.market_admin_destination_token,
        env.foreign_actor.portfolio,
        env.foreign_actor.source_token,
        env.foreign_actor.destination_token,
    ];
    for actor in &env.actors {
        keys.extend([
            actor.portfolio,
            actor.source_token,
            actor.destination_token,
            actor.matcher_context,
        ]);
    }
    keys.sort_unstable();
    keys.dedup();
    keys.into_iter()
        .map(|key| (key, env.svm.get_account(&key)))
        .collect()
}

pub fn reproduce_post_expiry_backing_fee(
    mut seed: [u8; 32],
    case: PostExpiryBackingCase,
) -> Result<PostExpiryBackingReproduction, String> {
    seed[0] ^= 0x67;
    let price = 100u64;
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
                super::v16_svm::USER_DEPOSIT,
                super::v16_svm::USER_DEPOSIT,
                super::v16_svm::EXIT_MAKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    let fee_bps = case.fee_bps.clamp(1, 10_000);
    let expiry_offset = u64::from(case.expiry_offset.clamp(2, 8));
    let mark_move_bps = u64::from(case.mark_move_bps.clamp(100, 1_000));
    let increase_divisor = i128::from(case.increase_divisor.clamp(10, 100));
    let domain = 1u16;
    let bucket_amount = 100_000u128;
    let open_q = 1_000 * POS_SCALE as i128;
    let increase_q = open_q
        .checked_div(increase_divisor)
        .ok_or("post-expiry increase divisor is zero")?;

    env.configure_auth_mark(false, 0, env.current_slot(), price)
        .map_err(|error| format!("configure low-price AuthMark: {error}"))?;
    env.update_backing_fee_policy(domain, fee_bps, 0)
        .map_err(|error| format!("configure backing fee policy: {error}"))?;
    let expiry_slot = env
        .current_slot()
        .checked_add(expiry_offset)
        .ok_or("post-expiry slot overflow")?;
    env.top_up_backing_bucket(domain, bucket_amount, expiry_slot)
        .map_err(|error| format!("top up backing bucket: {error}"))?;
    env.trade_no_cpi(0, 1, 0, open_q, price, 0)
        .map_err(|error| format!("open source-backed position: {error}"))?;

    let mark_slot = env
        .current_slot()
        .checked_add(1)
        .ok_or("post-expiry mark slot overflow")?;
    env.warp_to_slot(mark_slot);
    let winning_mark = price
        .checked_mul(
            10_000u64
                .checked_add(mark_move_bps)
                .ok_or("post-expiry mark bps overflow")?,
        )
        .and_then(|value| value.checked_div(10_000))
        .ok_or("post-expiry winning mark overflow")?;
    env.push_auth_mark(0, mark_slot, winning_mark)
        .map_err(|error| format!("push winning mark: {error}"))?;
    let oracle_accounts = env.primary_profile(0).oracle_leg_count;
    let observation = || {
        vec![CrankObservationHint {
            asset_index: 0,
            oracle_accounts,
        }]
    };
    env.crank(1, mark_slot, observation())
        .map_err(|error| format!("refresh counterparty at winning mark: {error}"))?;
    env.crank(0, mark_slot, observation())
        .map_err(|error| format!("refresh trader at winning mark: {error}"))?;

    let (_, before_group) = env.primary_market_state();
    let before_bucket = before_group.source_backing_buckets[domain as usize];
    if before_bucket.status != BackingBucketStatusV16::Fresh
        || before_bucket.expiry_slot != expiry_slot
    {
        return Err("backing bucket was not Fresh immediately before retained trade".into());
    }
    let capital_before = env.primary_portfolio(0).capital.get();
    let provider_before = env.token_amount(env.provider_destination_token);
    let supply_before = env.token_supply_observed();
    let retained = env.build_retained_no_cpi_trade(0, 1, 0, increase_q, winning_mark);

    env.warp_to_slot(
        expiry_slot
            .checked_add(1)
            .ok_or("post-expiry landing slot overflow")?,
    );
    if env.current_slot() <= expiry_slot {
        return Err("authenticated Clock did not pass backing expiry".into());
    }
    let before_rejection = tracked_economic_accounts(&env);
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
    let rejected_exact_rollback =
        retained_result.is_err() && tracked_economic_accounts(&env) == before_rejection;

    let (_, after_group) = env.primary_market_state();
    if after_group.current_slot >= env.current_slot() {
        return Err(
            "post-expiry reproducer did not preserve the authenticated-Clock/engine-slot gap"
                .into(),
        );
    }
    let after_bucket = after_group.source_backing_buckets[domain as usize];
    let provider_earnings = after_bucket
        .utilization_fee_earnings
        .checked_sub(before_bucket.utilization_fee_earnings)
        .ok_or("post-expiry provider earnings decreased")?;
    let capital_after = env.primary_portfolio(0).capital.get();
    let victim_capital_loss = capital_before
        .checked_sub(capital_after)
        .ok_or("post-expiry trade increased victim capital")?;
    if provider_earnings != 0 {
        env.withdraw_backing_bucket_earnings(domain, provider_earnings)
            .map_err(|error| format!("withdraw unexpected post-expiry earnings: {error}"))?;
    }
    let extracted_tokens = env
        .token_amount(env.provider_destination_token)
        .checked_sub(provider_before)
        .ok_or("provider destination token balance decreased")?;
    let position_before_reduction_q = position_abs_for_asset(&env.primary_portfolio(0), 0)?;
    let risk_reduction_landed = env
        .trade_no_cpi(0, 1, 0, -increase_q, winning_mark, 0)
        .is_ok();
    let position_after_reduction_q = position_abs_for_asset(&env.primary_portfolio(0), 0)?;
    Ok(PostExpiryBackingReproduction {
        blocker: KnownBlocker::PostExpiryBackingFee,
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

pub fn reproduce_omitted_rescue_liquidation(
    mut seed: [u8; 32],
) -> Result<OmittedRescueReproduction, String> {
    seed[0] ^= 0x22;
    let (mut omitted, position_before_q, insurance_before) = build_omitted_rescue_world(seed)?;
    let omitted_position_before_q = position_abs_for_asset(&omitted.primary_portfolio(0), 1)?;
    let first_before = tracked_economic_accounts(&omitted);
    let first_result = omitted.crank(0, 3, Vec::new());
    let (omitted_rejected_nonprogress, omitted_exact_rollback) = match first_result {
        Err(error) if error.contains("Custom(22)") => {
            (true, tracked_economic_accounts(&omitted) == first_before)
        }
        Err(error) => {
            return Err(format!(
                "omitted-observation first crank returned an unexpected error: {error}"
            ))
        }
        Ok(_) => {
            let second_before = tracked_economic_accounts(&omitted);
            match omitted.crank(0, 3, Vec::new()) {
                Err(error) if error.contains("Custom(22)") => {
                    (true, tracked_economic_accounts(&omitted) == second_before)
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
    let omitted_position_after_q = position_abs_for_asset(&omitted.primary_portfolio(0), 1)?;
    let omitted_insurance_after = omitted.primary_market_state().1.insurance;
    let omitted_insurance_delta = omitted_insurance_after
        .checked_sub(insurance_before)
        .ok_or("omitted-observation liquidation decreased insurance")?;

    let (mut complete, complete_position_before_q, complete_insurance_before) =
        build_omitted_rescue_world(seed)?;
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
        return Err("complete-world rescue observation booked no long funding".into());
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
    let complete_position_after_q = position_abs_for_asset(&complete_account, 1)?;
    let complete_cert = complete_account
        .health_cert
        .try_to_runtime()
        .map_err(|error| format!("decode complete-world certificate: {error:?}"))?;
    let complete_insurance_after = complete.primary_market_state().1.insurance;
    let complete_insurance_delta = complete_insurance_after
        .checked_sub(complete_insurance_before)
        .ok_or("complete-world insurance decreased")?;
    if complete_position_before_q != position_before_q
        || complete_position_after_q != complete_position_before_q
        || complete_cert.certified_liq_deficit != 0
        || complete_insurance_delta != 0
    {
        return Err(format!(
            "complete observation did not preserve the healthy control: position {complete_position_before_q}->{complete_position_after_q}, deficit {}, insurance delta {complete_insurance_delta}",
            complete_cert.certified_liq_deficit
        ));
    }

    Ok(OmittedRescueReproduction {
        blocker: KnownBlocker::OmittedRescueAccrualLiquidation,
        omitted_rejected_nonprogress,
        omitted_exact_rollback,
        omitted_position_before_q,
        omitted_position_after_q,
        omitted_insurance_delta,
        complete_position_after_q,
        complete_liquidation_deficit: complete_cert.certified_liq_deficit,
        complete_insurance_delta,
    })
}

pub fn reproduce_trade_retry_replay(
    mut seed: [u8; 32],
    route: TradeRoute,
) -> Result<TradeRetryReplayReproduction, String> {
    seed[0] ^= 0x43;
    let control = run_trade_retry_world(seed, route, false)?;
    let replay = run_trade_retry_world(seed, route, true)?;
    let victim_extra_loss = control
        .0
        .checked_sub(replay.0)
        .ok_or("trade retry replay increased victim payout")?;
    let attacker_extra_payout = replay
        .1
        .checked_sub(control.1)
        .ok_or("trade retry replay decreased attacker payout")?;
    if victim_extra_loss == 0 || victim_extra_loss != attacker_extra_payout {
        return Err(format!(
            "{route:?} retry variants did not transfer equal extractable value: victim {}/{}; attacker {}/{}",
            control.0, replay.0, control.1, replay.1
        ));
    }
    if control.2 != replay.2 {
        return Err(format!(
            "{route:?} retry replay changed total withdrawn value: control {}, replay {}",
            control.2, replay.2
        ));
    }
    Ok(TradeRetryReplayReproduction {
        blocker: KnownBlocker::TradeRetryReplay,
        route,
        victim_extra_loss,
        attacker_extra_payout,
        control_total_payout: control.2,
        replay_total_payout: replay.2,
    })
}

pub fn reproduce_asset_generation_trade_replay(
    mut seed: [u8; 32],
    route: TradeRoute,
) -> Result<AssetGenerationReplayReproduction, String> {
    seed[0] ^= 0x31;
    let control = run_asset_generation_trade_world(seed, route, false)?;
    let replay = run_asset_generation_trade_world(seed, route, true)?;
    if control.2 != replay.2 || control.3 != replay.3 {
        return Err(format!(
            "{route:?} lifecycle controls used different generations: control {:?}, replay {:?}",
            (control.2, control.3),
            (replay.2, replay.3)
        ));
    }
    let victim_loss = control
        .0
        .checked_sub(replay.0)
        .ok_or("asset-generation replay increased victim payout")?;
    let attacker_gain = replay
        .1
        .checked_sub(control.1)
        .ok_or("asset-generation replay decreased attacker payout")?;
    if victim_loss == 0 || victim_loss != attacker_gain {
        return Err(format!(
            "{route:?} stale generation did not transfer extractable value: victim {}/{}; attacker {}/{}",
            control.0, replay.0, control.1, replay.1
        ));
    }
    let total_payout = u128::from(replay.0) + u128::from(replay.1);
    if total_payout != u128::from(control.0) + u128::from(control.1) {
        return Err(format!(
            "{route:?} stale-generation replay changed total payout"
        ));
    }
    Ok(AssetGenerationReplayReproduction {
        blocker: KnownBlocker::AssetGenerationTradeReplay,
        route,
        old_market_id: replay.2,
        new_market_id: replay.3,
        victim_loss,
        attacker_payout: replay.1,
        total_payout,
    })
}

pub fn reproduce_asset_generation_mark_replay(
    mut seed: [u8; 32],
    path: AssetGenerationMarkPath,
) -> Result<AssetGenerationMarkReplayReproduction, String> {
    seed[0] ^= 0x75;
    let control = run_asset_generation_mark_world(seed, path, false)?;
    let replay = run_asset_generation_mark_world(seed, path, true)?;
    if control.old_market_id != replay.old_market_id
        || control.new_market_id != replay.new_market_id
        || control.old_market_id == control.new_market_id
    {
        return Err(format!(
            "{path:?} mark-replay worlds did not use the same distinct generations: control {}/{}, replay {}/{}",
            control.old_market_id,
            control.new_market_id,
            replay.old_market_id,
            replay.new_market_id
        ));
    }
    let victim_equity_loss = control
        .victim_equity
        .checked_sub(replay.victim_equity)
        .ok_or("stale mark replay increased victim equity")?;
    let beneficiary_extra_payout = replay
        .beneficiary_payout
        .checked_sub(control.beneficiary_payout)
        .ok_or("stale mark replay decreased beneficiary payout")?;
    if victim_equity_loss == 0
        || victim_equity_loss != u128::from(beneficiary_extra_payout)
        || replay.landed_mark >= control.landed_mark
    {
        return Err(format!(
            "{path:?} stale report did not transfer equal extractable value: victim equity {}/{}, beneficiary payout {}/{}, mark {}/{}",
            control.victim_equity,
            replay.victim_equity,
            control.beneficiary_payout,
            replay.beneficiary_payout,
            control.landed_mark,
            replay.landed_mark
        ));
    }
    if control.observed_token_supply != replay.observed_token_supply {
        return Err(format!(
            "{path:?} stale mark replay changed observed SPL supply: control {}, replay {}",
            control.observed_token_supply, replay.observed_token_supply
        ));
    }
    Ok(AssetGenerationMarkReplayReproduction {
        blocker: KnownBlocker::AssetGenerationMarkReplay,
        path,
        old_market_id: replay.old_market_id,
        new_market_id: replay.new_market_id,
        landed_mark: replay.landed_mark,
        victim_equity_loss,
        beneficiary_extra_payout,
        observed_token_supply: replay.observed_token_supply,
    })
}

pub fn reproduce_asset_generation_config_replay(
    mut seed: [u8; 32],
    path: AssetGenerationConfigPath,
) -> Result<AssetGenerationConfigReplayReproduction, String> {
    seed[0] ^= 0x77;
    if path == AssetGenerationConfigPath::Hybrid {
        let control = run_asset_generation_hybrid_config_world(seed, false)?;
        let replay = run_asset_generation_hybrid_config_world(seed, true)?;
        if control.old_market_id != replay.old_market_id
            || control.new_market_id != replay.new_market_id
            || control.old_market_id == control.new_market_id
        {
            return Err(format!(
                "{path:?} config-replay worlds did not use the same distinct generations: control {}/{}, replay {}/{}",
                control.old_market_id,
                control.new_market_id,
                replay.old_market_id,
                replay.new_market_id
            ));
        }
        let victim_equity_loss = control
            .victim_equity
            .checked_sub(replay.victim_equity)
            .ok_or("stale Hybrid config increased victim payout")?;
        let beneficiary_extra_payout = replay
            .beneficiary_payout
            .checked_sub(control.beneficiary_payout)
            .ok_or("stale Hybrid config decreased beneficiary payout")?;
        if replay.entry_price != control.entry_price
            || replay.restored_mark <= control.restored_mark
            || victim_equity_loss == 0
            || victim_equity_loss != u128::from(beneficiary_extra_payout)
            || control.victim_equity + u128::from(control.beneficiary_payout)
                != replay.victim_equity + u128::from(replay.beneficiary_payout)
        {
            return Err(format!(
                "{path:?} stale config did not create a terminal transfer: mark {}/{}, victim {}/{}, beneficiary {}/{}",
                control.entry_price,
                replay.entry_price,
                control.victim_equity,
                replay.victim_equity,
                control.beneficiary_payout,
                replay.beneficiary_payout
            ));
        }
        if control.observed_token_supply != replay.observed_token_supply {
            return Err(format!(
                "{path:?} stale config replay changed observed SPL supply: control {}, replay {}",
                control.observed_token_supply, replay.observed_token_supply
            ));
        }
        return Ok(AssetGenerationConfigReplayReproduction {
            blocker: KnownBlocker::AssetGenerationConfigReplay,
            path,
            old_market_id: replay.old_market_id,
            new_market_id: replay.new_market_id,
            stale_entry_price: replay.entry_price,
            restored_mark: replay.restored_mark,
            victim_equity_loss,
            beneficiary_extra_payout,
            observed_token_supply: replay.observed_token_supply,
        });
    }
    let control = run_asset_generation_config_world(seed, path, false)?;
    let replay = run_asset_generation_config_world(seed, path, true)?;
    if control.old_market_id != replay.old_market_id
        || control.new_market_id != replay.new_market_id
        || control.old_market_id == control.new_market_id
    {
        return Err(format!(
            "{path:?} config-replay worlds did not use the same distinct generations: control {}/{}, replay {}/{}",
            control.old_market_id,
            control.new_market_id,
            replay.old_market_id,
            replay.new_market_id
        ));
    }
    let victim_equity_loss = control
        .victim_equity
        .checked_sub(replay.victim_equity)
        .ok_or("stale config replay increased victim equity")?;
    let beneficiary_extra_payout = replay
        .beneficiary_payout
        .checked_sub(control.beneficiary_payout)
        .ok_or("stale config replay decreased beneficiary payout")?;
    if replay.entry_price >= control.entry_price
        || replay.restored_mark <= replay.entry_price
        || victim_equity_loss == 0
        || victim_equity_loss != u128::from(beneficiary_extra_payout)
    {
        return Err(format!(
            "{path:?} stale config did not create an extractable entry-anchor transfer: entry {}/{}, restored {}, victim equity {}/{}, beneficiary payout {}/{}",
            control.entry_price,
            replay.entry_price,
            replay.restored_mark,
            control.victim_equity,
            replay.victim_equity,
            control.beneficiary_payout,
            replay.beneficiary_payout
        ));
    }
    if control.observed_token_supply != replay.observed_token_supply {
        return Err(format!(
            "{path:?} stale config replay changed observed SPL supply: control {}, replay {}",
            control.observed_token_supply, replay.observed_token_supply
        ));
    }
    Ok(AssetGenerationConfigReplayReproduction {
        blocker: KnownBlocker::AssetGenerationConfigReplay,
        path,
        old_market_id: replay.old_market_id,
        new_market_id: replay.new_market_id,
        stale_entry_price: replay.entry_price,
        restored_mark: replay.restored_mark,
        victim_equity_loss,
        beneficiary_extra_payout,
        observed_token_supply: replay.observed_token_supply,
    })
}

pub fn verify_cpi_caller_fee_protection(
    mut seed: [u8; 32],
    route: TradeRoute,
) -> Result<CpiCallerFeeProtection, String> {
    if !matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi) {
        return Err(format!("{route:?} is not an unsigned-LP CPI route"));
    }
    seed[0] ^= 0x24;
    const DEPOSIT: u128 = 1_000_000;
    const ASSET: u16 = 1;
    const PRICE: u64 = 100;
    const SIZE_Q: i128 = 100 * POS_SCALE as i128;
    const CALLER_FEE_BPS: u64 = 10_000;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            actor_deposits: [
                DEPOSIT,
                DEPOSIT,
                super::v16_svm::USER_DEPOSIT,
                super::v16_svm::USER_DEPOSIT,
                super::v16_svm::EXIT_MAKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.update_market_init_fee_policy(1)
        .map_err(|error| format!("{route:?} configure permissionless init fee: {error}"))?;
    env.warp_to_slot(2);
    env.retire_asset(ASSET, 2)
        .map_err(|error| format!("{route:?} retire asset before attacker creation: {error}"))?;
    env.warp_to_slot(3);
    env.activate_permissionless_asset_for_actor(0, ASSET, 3, PRICE, 0, 1)
        .map_err(|error| format!("{route:?} attacker asset activation: {error}"))?;

    let market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    let mut max_trade_cu = 0;
    for size_q in [SIZE_Q, -SIZE_Q] {
        let success = match route {
            TradeRoute::Cpi => env
                .trade_cpi(0, 1, ASSET, size_q, CALLER_FEE_BPS, 0)
                .map_err(|error| format!("single CPI caller-fee leg {size_q}: {error}"))?,
            TradeRoute::BatchCpi => env
                .batch_trade_cpi(
                    0,
                    1,
                    vec![BatchTradeCpiLeg {
                        asset_index: ASSET,
                        market_id,
                        size_q,
                        fee_bps: CALLER_FEE_BPS,
                        limit_price: 0,
                    }],
                )
                .map_err(|error| format!("batch CPI caller-fee leg {size_q}: {error}"))?,
            _ => unreachable!(),
        };
        max_trade_cu = max_trade_cu.max(success.compute_units);
    }
    let attacker_capital = env.primary_portfolio(0).capital.get();
    let lp_capital = env.primary_portfolio(1).capital.get();
    let (_, group) = env.primary_market_state();
    if group.assets[ASSET as usize].oi_eff_long_q != 0
        || group.assets[ASSET as usize].oi_eff_short_q != 0
    {
        return Err(format!(
            "{route:?} caller-fee round trip did not close flat"
        ));
    }
    let withdrawable_insurance = group.insurance_domain_budget[2]
        .checked_add(group.insurance_domain_budget[3])
        .ok_or("CPI caller-fee insurance budget overflow")?;
    let attacker_profit = attacker_capital.saturating_sub(DEPOSIT) as u64;
    let lp_loss = DEPOSIT.saturating_sub(lp_capital) as u64;
    if attacker_capital != DEPOSIT
        || lp_capital != DEPOSIT
        || attacker_profit != 0
        || lp_loss != 0
        || withdrawable_insurance != 0
    {
        return Err(format!(
            "{route:?} caller fee remained economically authoritative: attacker capital {attacker_capital}, LP capital {lp_capital}, attacker profit {attacker_profit}, LP loss {lp_loss}, asset insurance {withdrawable_insurance}"
        ));
    }
    let before_rejection = tracked_economic_accounts(&env);
    let withdraw_error = match env.withdraw_insurance_asset(0, ASSET, 1) {
        Ok(_) => return Err(format!("{route:?} caller withdrew nonexistent insurance")),
        Err(error) => error,
    };
    let insurance_withdraw_rejected = withdraw_error.contains("Custom(21)")
        || withdraw_error.contains("custom program error: 0x15");
    let rejected_exact_rollback = tracked_economic_accounts(&env) == before_rejection;
    if !insurance_withdraw_rejected || !rejected_exact_rollback {
        return Err(format!(
            "{route:?} rejected insurance extraction was not a locked, exact rollback: {withdraw_error}"
        ));
    }
    env.withdraw_primary(0, attacker_capital)
        .map_err(|error| format!("{route:?} attacker capital withdrawal: {error}"))?;
    env.withdraw_primary(1, lp_capital)
        .map_err(|error| format!("{route:?} LP capital withdrawal: {error}"))?;
    let attacker_payout = env.token_amount(env.actors[0].destination_token);
    let lp_payout = env.token_amount(env.actors[1].destination_token);
    if attacker_payout != DEPOSIT as u64 || lp_payout != DEPOSIT as u64 {
        return Err(format!(
            "{route:?} caller fee changed terminal principal: attacker {attacker_payout}, LP {lp_payout}"
        ));
    }
    let total_payout = u128::from(attacker_payout) + u128::from(lp_payout);
    if total_payout != DEPOSIT * 2 {
        return Err(format!(
            "{route:?} caller-fee protection changed total payout to {total_payout}"
        ));
    }
    let token_supply_conserved = env.token_supply_observed() == supply_before;
    if !token_supply_conserved {
        return Err(format!(
            "{route:?} caller-fee protection changed SPL supply"
        ));
    }
    Ok(CpiCallerFeeProtection {
        blocker: KnownBlocker::CpiCallerFeeSiphon,
        route,
        requested_fee_bps: CALLER_FEE_BPS,
        max_trade_cu,
        attacker_profit,
        lp_loss,
        withdrawable_insurance,
        insurance_withdraw_rejected,
        rejected_exact_rollback,
        total_payout,
        token_supply_conserved,
    })
}

pub fn verify_cpi_base_fee_consent(
    mut seed: [u8; 32],
    route: TradeRoute,
) -> Result<CpiBaseFeeConsentProtection, String> {
    if !matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi) {
        return Err(format!(
            "PR 313 requires an unsigned-LP CPI route: {route:?}"
        ));
    }
    seed[0] ^= 0x13;
    const ASSET: u16 = 0;
    const BENEFICIARY: usize = 0;
    const LP: usize = 1;
    const DEPOSIT: u128 = 100_000_000;
    const SIZE_Q: i128 = POS_SCALE as i128;
    const REJECTING_CAP_BPS: u16 = 499;
    const INSTALLED_FEE_BPS: u64 = 500;
    const CONSENTED_CAP_BPS: u16 = 500;
    const FEE_PER_SIDE_PER_TRADE: u64 = 50_000;
    const TOTAL_INSURANCE_FEE: u128 = 4 * FEE_PER_SIDE_PER_TRADE as u128;

    let mut env = V16Svm::new(seed, MarketConfig::default());
    let supply_before = env.token_supply_observed();
    env.update_asset_authority_from_admin(
        ASSET,
        percolator_prog::processor::ASSET_AUTH_INSURANCE_OPERATOR,
        BENEFICIARY,
    )
    .map_err(|error| format!("PR 313 install independent fee beneficiary: {error}"))?;

    let epoch_before_consent = env.primary_portfolio_position_epoch(LP);
    env.set_matcher_config_with_trade_fee_cap(LP, 1, REJECTING_CAP_BPS)
        .map_err(|error| format!("PR 313 bind rejecting LP fee cap: {error}"))?;
    let cap_state = tracked_economic_accounts(&env);
    let invalid_cap_error = match env.set_matcher_config_with_trade_fee_cap(LP, 1, 10_001) {
        Ok(_) => return Err("PR 313 matcher accepted a fee cap above 100%".into()),
        Err(error) => error,
    };
    let invalid_cap_rejected = invalid_cap_error.contains("Custom(9)")
        || invalid_cap_error.contains("custom program error: 0x9");
    let invalid_cap_exact_rollback = tracked_economic_accounts(&env) == cap_state;
    let position_epoch_preserved = env.primary_portfolio_position_epoch(LP) == epoch_before_consent;
    if !invalid_cap_rejected || !invalid_cap_exact_rollback || !position_epoch_preserved {
        return Err(format!(
            "PR 313 invalid cap did not reject atomically: error={invalid_cap_error}, \
             rollback={invalid_cap_exact_rollback}, epoch={epoch_before_consent}/{}",
            env.primary_portfolio_position_epoch(LP)
        ));
    }

    env.update_trade_fee_policy(INSTALLED_FEE_BPS)
        .map_err(|error| format!("PR 313 raise live base fee after LP consent: {error}"))?;
    let send_fill = |env: &mut V16Svm, size_q: i128| {
        let market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
        match route {
            TradeRoute::Cpi => env.trade_cpi(BENEFICIARY, LP, ASSET, size_q, 0, 0),
            TradeRoute::BatchCpi => env.batch_trade_cpi(
                BENEFICIARY,
                LP,
                vec![BatchTradeCpiLeg {
                    asset_index: ASSET,
                    market_id,
                    size_q,
                    fee_bps: 0,
                    limit_price: 0,
                }],
            ),
            _ => unreachable!(),
        }
    };

    let state_after_policy = tracked_economic_accounts(&env);
    let lp_before_rejection = env.primary_portfolio(LP).capital.get();
    let insurance_before_rejection = env.primary_market_state().1.insurance_domain_budget[0]
        .checked_add(env.primary_market_state().1.insurance_domain_budget[1])
        .ok_or("PR 313 pre-rejection insurance total overflow")?;
    let stale_fill_error = match send_fill(&mut env, SIZE_Q) {
        Ok(_) => return Err("PR 313 CPI fill above the LP fee cap landed".into()),
        Err(error) => error,
    };
    let stale_fill_rejected = stale_fill_error.contains("Custom(9)")
        || stale_fill_error.contains("custom program error: 0x9");
    let stale_fill_exact_rollback = tracked_economic_accounts(&env) == state_after_policy;
    let unconsented_lp_loss = u64::try_from(
        lp_before_rejection
            .checked_sub(env.primary_portfolio(LP).capital.get())
            .ok_or("PR 313 rejected CPI fill increased LP capital")?,
    )
    .map_err(|_| "PR 313 unconsented LP loss does not fit u64")?;
    let insurance_after_rejection = env.primary_market_state().1.insurance_domain_budget[0]
        .checked_add(env.primary_market_state().1.insurance_domain_budget[1])
        .ok_or("PR 313 post-rejection insurance total overflow")?;
    let unconsented_insurance_delta = insurance_after_rejection
        .checked_sub(insurance_before_rejection)
        .ok_or("PR 313 rejected CPI fill decreased insurance")?;
    if !stale_fill_rejected
        || !stale_fill_exact_rollback
        || unconsented_lp_loss != 0
        || unconsented_insurance_delta != 0
    {
        return Err(format!(
            "PR 313 stale fee consent failed: error={stale_fill_error}, \
             rollback={stale_fill_exact_rollback}, lp_loss={unconsented_lp_loss}, \
             insurance_delta={unconsented_insurance_delta}"
        ));
    }

    let cap_update = env
        .set_matcher_config_with_trade_fee_cap(LP, 1, CONSENTED_CAP_BPS)
        .map_err(|error| format!("PR 313 install fresh LP fee consent: {error}"))?;
    if env.primary_portfolio_position_epoch(LP) != epoch_before_consent {
        return Err("PR 313 matcher fee consent changed the position episode".into());
    }
    let open = send_fill(&mut env, SIZE_Q)
        .map_err(|error| format!("PR 313 freshly consented CPI open failed: {error}"))?;
    let close = send_fill(&mut env, -SIZE_Q)
        .map_err(|error| format!("PR 313 freshly consented CPI close failed: {error}"))?;

    let beneficiary_capital = env.primary_portfolio(BENEFICIARY).capital.get();
    let lp_capital = env.primary_portfolio(LP).capital.get();
    let insurance = env.primary_market_state().1.insurance_domain_budget[0]
        .checked_add(env.primary_market_state().1.insurance_domain_budget[1])
        .ok_or("PR 313 consented insurance total overflow")?;
    let expected_capital = DEPOSIT
        .checked_sub(2 * FEE_PER_SIDE_PER_TRADE as u128)
        .ok_or("PR 313 expected capital underflow")?;
    if beneficiary_capital != expected_capital
        || lp_capital != expected_capital
        || insurance != TOTAL_INSURANCE_FEE
    {
        return Err(format!(
            "PR 313 consented CPI accounting mismatch: capital={beneficiary_capital}/\
             {lp_capital}, insurance={insurance}"
        ));
    }

    let insurance_withdrawal = env
        .withdraw_insurance_asset(BENEFICIARY, ASSET, TOTAL_INSURANCE_FEE)
        .map_err(|error| format!("PR 313 beneficiary insurance withdrawal failed: {error}"))?;
    let beneficiary_withdrawal = env
        .withdraw_primary(BENEFICIARY, beneficiary_capital)
        .map_err(|error| format!("PR 313 beneficiary exit failed: {error}"))?;
    let lp_withdrawal = env
        .withdraw_primary(LP, lp_capital)
        .map_err(|error| format!("PR 313 LP exit failed: {error}"))?;
    let beneficiary_payout = env.token_amount(env.actors[BENEFICIARY].destination_token);
    let lp_payout = env.token_amount(env.actors[LP].destination_token);
    let consented_lp_fee = (DEPOSIT as u64)
        .checked_sub(lp_payout)
        .ok_or("PR 313 LP payout exceeded its deposit")?;
    let total_payout = u128::from(beneficiary_payout) + u128::from(lp_payout);
    let max_route_cu = cap_update
        .compute_units
        .max(open.compute_units)
        .max(close.compute_units)
        .max(insurance_withdrawal.compute_units)
        .max(beneficiary_withdrawal.compute_units)
        .max(lp_withdrawal.compute_units);
    let token_supply_conserved = env.token_supply_observed() == supply_before;
    if beneficiary_payout != 100_100_000
        || lp_payout != 99_900_000
        || consented_lp_fee != 100_000
        || total_payout != 2 * DEPOSIT
        || max_route_cu >= TX_CU_LIMIT
        || !token_supply_conserved
    {
        return Err(format!(
            "PR 313 terminal mismatch: payouts={beneficiary_payout}/{lp_payout}, \
             lp_fee={consented_lp_fee}, total={total_payout}, max_cu={max_route_cu}, \
             supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }

    Ok(CpiBaseFeeConsentProtection {
        blocker: KnownBlocker::BilateralBaseFeeConsent,
        route,
        rejecting_cap_bps: REJECTING_CAP_BPS,
        installed_fee_bps: INSTALLED_FEE_BPS,
        invalid_cap_rejected,
        invalid_cap_exact_rollback,
        stale_fill_rejected,
        stale_fill_exact_rollback,
        position_epoch_preserved,
        unconsented_lp_loss,
        unconsented_insurance_delta,
        consented_cap_bps: CONSENTED_CAP_BPS,
        consented_lp_fee,
        consented_insurance_fee: TOTAL_INSURANCE_FEE as u64,
        total_payout,
        open_cu: open.compute_units,
        close_cu: close.compute_units,
        max_route_cu,
        token_supply_conserved,
    })
}

pub fn verify_cpi_backing_fee_consent(
    mut seed: [u8; 32],
) -> Result<CpiBackingFeeProtection, String> {
    seed[0] ^= 0x23;
    const PRICE: u64 = 100;
    const LP_DEPOSIT: u128 = 4_100;
    const ATTACKER_DEPOSIT: u128 = 10_000;
    const WINNING_SIZE_Q: i128 = 300 * POS_SCALE as i128;
    const LOSING_SIZE_Q: i128 = 100 * POS_SCALE as i128;
    const INCREASE_Q: i128 = 20 * POS_SCALE as i128;
    const WINNING_DOMAIN: u16 = 3;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            maintenance_fee_per_slot: 30,
            actor_deposits: [
                ATTACKER_DEPOSIT,
                ATTACKER_DEPOSIT,
                LP_DEPOSIT,
                super::v16_svm::USER_DEPOSIT,
                super::v16_svm::EXIT_MAKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    env.update_market_init_fee_policy(1)
        .map_err(|error| format!("configure permissionless init fee: {error}"))?;
    env.configure_auth_mark(false, 0, 1, PRICE)
        .map_err(|error| format!("configure base AuthMark: {error}"))?;
    env.warp_to_slot(2);
    env.retire_asset(1, 2)
        .map_err(|error| format!("retire attacker asset slot: {error}"))?;
    env.warp_to_slot(3);
    env.activate_permissionless_asset_for_actor(0, 1, 3, PRICE, 0, 1)
        .map_err(|error| format!("activate attacker asset: {error}"))?;
    env.configure_auth_mark_for_actor(0, 1, 3, PRICE)
        .map_err(|error| format!("configure attacker AuthMark: {error}"))?;
    env.top_up_backing_bucket_for_actor(0, WINNING_DOMAIN, 5_000, 100)
        .map_err(|error| format!("fund attacker backing domain: {error}"))?;

    env.trade_cpi(1, 2, 1, -WINNING_SIZE_Q, 0, 0)
        .map_err(|error| format!("open LP winning leg: {error}"))?;
    env.trade_cpi(1, 2, 0, -LOSING_SIZE_Q, 0, 0)
        .map_err(|error| format!("open LP losing leg: {error}"))?;

    env.warp_to_slot(4);
    env.push_auth_mark_for_actor(0, 1, 4, PRICE)
        .map_err(|error| format!("prime attacker asset: {error}"))?;
    env.push_auth_mark(0, 4, PRICE)
        .map_err(|error| format!("prime base asset: {error}"))?;
    let complete_prime_observations = vec![
        CrankObservationHint {
            asset_index: 1,
            oracle_accounts: env.primary_profile(1).oracle_leg_count,
        },
        CrankObservationHint {
            asset_index: 0,
            oracle_accounts: env.primary_profile(0).oracle_leg_count,
        },
    ];
    for actor in [1, 2] {
        crank_adapter_steps_with_observations(
            &mut env,
            actor,
            4,
            complete_prime_observations.clone(),
            8,
        )?;
    }
    env.sync_maintenance_fee(2, 4)
        .map_err(|error| format!("sync LP maintenance fee: {error}"))?;
    let expected_lp_capital = LP_DEPOSIT
        .checked_sub(90)
        .ok_or("LP maintenance setup underflow")?;
    if env.primary_portfolio(2).capital.get() != expected_lp_capital {
        return Err(format!(
            "LP maintenance setup reached capital {}, expected {expected_lp_capital}",
            env.primary_portfolio(2).capital.get(),
        ));
    }

    env.warp_to_slot(5);
    env.push_auth_mark_for_actor(0, 1, 5, 105)
        .map_err(|error| format!("push LP winning mark: {error}"))?;
    env.push_auth_mark(0, 5, 95)
        .map_err(|error| format!("push LP losing mark: {error}"))?;
    let complete_lp_observations = vec![
        CrankObservationHint {
            asset_index: 1,
            oracle_accounts: env.primary_profile(1).oracle_leg_count,
        },
        CrankObservationHint {
            asset_index: 0,
            oracle_accounts: env.primary_profile(0).oracle_leg_count,
        },
    ];
    for actor in [1, 2] {
        env.crank(actor, 5, complete_lp_observations.clone())
            .map_err(|error| format!("fully refresh LP source-PnL actor {actor}: {error}"))?;
    }
    if env.primary_portfolio(2).pnl.get() != 1_000 {
        return Err(format!(
            "LP source-PnL setup reached {}, expected 1000",
            env.primary_portfolio(2).pnl.get()
        ));
    }

    env.update_backing_fee_policy_for_actor(0, WINNING_DOMAIN, 5_000, 0)
        .map_err(|error| format!("install post-consent backing fee: {error}"))?;
    let lp_before = env.primary_portfolio(2).capital.get();
    let attacker_before = env.primary_portfolio(0).capital.get();
    let provider_before = env.primary_market_state().1.source_backing_buckets
        [WINNING_DOMAIN as usize]
        .utilization_fee_earnings;
    let provider_destination_before = env.token_amount(env.actors[0].destination_token);
    let supply_before = env.token_supply_observed();

    env.warp_to_slot(6);
    let before_rejection = tracked_economic_accounts(&env);
    let rejection = match env.trade_cpi(0, 2, 0, -INCREASE_Q, 0, 0) {
        Ok(_) => return Err("zero-cap matcher accepted an LP backing fee".into()),
        Err(error) => error,
    };
    let rejected_without_consent =
        rejection.contains("Custom(8)") || rejection.contains("custom program error: 0x8");
    let rejected_exact_rollback = tracked_economic_accounts(&env) == before_rejection;
    let unconsented_provider_earnings = env.primary_market_state().1.source_backing_buckets
        [WINNING_DOMAIN as usize]
        .utilization_fee_earnings
        .checked_sub(provider_before)
        .ok_or("rejected CPI backing fee decreased provider earnings")?;
    if !rejected_without_consent
        || !rejected_exact_rollback
        || env.primary_portfolio(2).capital.get() != lp_before
        || env.primary_portfolio(0).capital.get() != attacker_before
        || unconsented_provider_earnings != 0
    {
        return Err(format!(
            "unconsented CPI backing fee did not reject atomically: {rejection}"
        ));
    }

    const MATCHER_CAP_BPS: u16 = 5_000;
    let cap_update = env
        .set_matcher_backing_fee_cap(2, MATCHER_CAP_BPS)
        .map_err(|error| format!("LP backing-fee consent: {error}"))?;
    let consented_trade = env
        .trade_cpi(0, 2, 0, -INCREASE_Q, 0, 0)
        .map_err(|error| format!("consented fee-bearing CPI increase: {error}"))?;
    let lp_after = env.primary_portfolio(2).capital.get();
    let provider_after = env.primary_market_state().1.source_backing_buckets
        [WINNING_DOMAIN as usize]
        .utilization_fee_earnings;
    let provider_earnings = provider_after
        .checked_sub(provider_before)
        .ok_or("CPI backing provider earnings decreased")?;
    let lp_capital_loss = lp_before
        .checked_sub(lp_after)
        .ok_or("CPI backing fee increased LP capital")?;
    if provider_earnings == 0 || lp_capital_loss != provider_earnings {
        return Err(format!(
            "consented CPI backing fee did not transfer LP capital to provider: LP {lp_before}->{lp_after}, earnings {provider_before}->{provider_after}"
        ));
    }
    let zero_cap_update = env
        .set_matcher_backing_fee_cap(2, 0)
        .map_err(|error| format!("restore zero matcher cap: {error}"))?;
    let earnings_before_reduction = env.primary_market_state().1.source_backing_buckets
        [WINNING_DOMAIN as usize]
        .utilization_fee_earnings;
    let risk_reduction = env
        .trade_cpi(0, 2, 0, INCREASE_Q, 0, 0)
        .map_err(|error| format!("reverse fee-bearing CPI increase: {error}"))?;
    let zero_cap_risk_reduction_landed = env.primary_market_state().1.source_backing_buckets
        [WINNING_DOMAIN as usize]
        .utilization_fee_earnings
        == earnings_before_reduction;
    let attacker_after = env.primary_portfolio(0).capital.get();
    let attacker_capital_delta = i128::try_from(attacker_after)
        .and_then(|after| i128::try_from(attacker_before).map(|before| after - before))
        .map_err(|_| "attacker capital does not fit i128")?;
    if !zero_cap_risk_reduction_landed
        || attacker_capital_delta != 0
        || observed_positions(&env.primary_portfolio(0))?[0] != 0
    {
        return Err(format!(
            "zero-cap CPI risk reduction failed: landed {zero_cap_risk_reduction_landed}, capital delta {attacker_capital_delta}"
        ));
    }

    env.withdraw_backing_bucket_earnings_for_actor(0, WINNING_DOMAIN, provider_earnings)
        .map_err(|error| format!("withdraw LP-funded provider earnings: {error}"))?;
    let provider_destination_after = env.token_amount(env.actors[0].destination_token);
    let extracted_tokens = provider_destination_after
        .checked_sub(provider_destination_before)
        .ok_or("provider destination balance decreased")?;
    if u128::from(extracted_tokens) != provider_earnings {
        return Err(format!(
            "CPI backing fee ledger/SPL extraction mismatch: {provider_earnings} vs {extracted_tokens}"
        ));
    }
    let token_supply_conserved = env.token_supply_observed() == supply_before;
    if !token_supply_conserved {
        return Err("CPI backing-fee consent path changed total SPL supply".into());
    }
    let max_route_cu = cap_update
        .compute_units
        .max(consented_trade.compute_units)
        .max(zero_cap_update.compute_units)
        .max(risk_reduction.compute_units);
    Ok(CpiBackingFeeProtection {
        blocker: KnownBlocker::CpiBackingFeeSiphon,
        matcher_cap_bps: MATCHER_CAP_BPS,
        rejected_without_consent,
        rejected_exact_rollback,
        unconsented_provider_earnings,
        lp_capital_loss,
        provider_earnings,
        extracted_tokens,
        attacker_capital_delta,
        zero_cap_risk_reduction_landed,
        max_route_cu,
        token_supply_conserved,
    })
}

pub fn reproduce_composite_oracle_rounding(
    mut seed: [u8; 32],
    case: CompositeRoundingCase,
) -> Result<CompositeRoundingReproduction, String> {
    seed[0] ^= match case {
        CompositeRoundingCase::Pr329LargeMove => 0x29,
        CompositeRoundingCase::Pr381MicroMove => 0x81,
    };
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
    ) = match case {
        CompositeRoundingCase::Pr329LargeMove => (
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
        CompositeRoundingCase::Pr381MicroMove => (
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
            .ok_or("exact composite arithmetic failed".into())
    };
    if exact_composite(initial_prices)? != u128::from(exact_mark)
        || exact_composite(fresh_prices)? != u128::from(exact_mark)
    {
        return Err(format!(
            "{case:?} fixture does not preserve exact mark {exact_mark}"
        ));
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
                super::v16_svm::USER_DEPOSIT,
                super::v16_svm::EXIT_MAKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    env.update_liquidation_fee_policy(5_000)
        .map_err(|error| format!("{case:?} configure cranker share: {error}"))?;
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
    .map_err(|error| format!("{case:?} configure exact composite: {error}"))?;
    if env.primary_market_state().0.oracle_target_price_e6 != exact_mark {
        return Err(format!(
            "{case:?} initial composite target is {}, expected {exact_mark}",
            env.primary_market_state().0.oracle_target_price_e6
        ));
    }

    let size_q = size_units
        .checked_mul(POS_SCALE)
        .and_then(|value| i128::try_from(value).ok())
        .ok_or("composite victim size overflow")?;
    env.trade_no_cpi(0, 1, 0, size_q, exact_mark, 0)
        .map_err(|error| format!("{case:?} open victim long: {error}"))?;
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
            .ok_or("composite catch-up slot overflow")?;
        env.set_clock(slot, 101);
        env.crank_with_oracles(2, slot, observations(), &fresh_oracles)
            .map_err(|error| format!("{case:?} composite catch-up at slot {slot}: {error}"))?;
    }
    env.crank_with_oracles(0, slot, observations(), &fresh_oracles)
        .map_err(|error| format!("{case:?} refresh victim at rounded mark: {error}"))?;
    let victim_cert = env
        .primary_portfolio(0)
        .health_cert
        .try_to_runtime()
        .map_err(|error| format!("{case:?} decode victim certificate: {error:?}"))?;
    let certified_liq_deficit = victim_cert.certified_liq_deficit;
    if certified_liq_deficit != 0 {
        env.crank_with_reward(2, 0, slot, observations(), &fresh_oracles)
            .map_err(|error| format!("{case:?} false-price liquidation failed: {error}"))?;
    }
    let (wrapper_after, group_after) = env.primary_market_state();
    let victim_capital_after = env.primary_portfolio(0).capital.get();
    let oi_after = group_after.assets[0].oi_eff_long_q;
    let cranker_capital_after = env.primary_portfolio(2).capital.get();
    let victim_capital_loss = victim_capital_before
        .checked_sub(victim_capital_after)
        .ok_or("composite liquidation increased victim capital")?;
    let oi_reduction_q = oi_before
        .checked_sub(oi_after)
        .ok_or("composite liquidation increased victim OI")?;
    let cranker_reward = cranker_capital_after
        .checked_sub(cranker_capital_before)
        .ok_or("composite liquidation decreased cranker capital")?;
    if cranker_reward != 0 {
        env.withdraw_primary(2, cranker_reward)
            .map_err(|error| format!("{case:?} withdraw false-liquidation reward: {error}"))?;
    }
    let extracted_tokens = env.token_amount(env.actors[2].destination_token);
    if u128::from(extracted_tokens) != cranker_reward {
        return Err(format!(
            "{case:?} cranker reward/SPL mismatch: {cranker_reward} vs {extracted_tokens}"
        ));
    }
    if env.token_supply_observed() != supply_before {
        return Err(format!(
            "{case:?} false liquidation changed total SPL supply"
        ));
    }
    Ok(CompositeRoundingReproduction {
        blocker: KnownBlocker::CompositeOracleRounding,
        case,
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

pub fn reproduce_composite_oracle_time_skew(
    mut seed: [u8; 32],
) -> Result<CompositeOracleTimeSkewReproduction, String> {
    seed[0] ^= 0x31;
    const COHERENT_PRICE: u64 = 1_500_000;
    const INITIAL_A: i64 = 3_000_000;
    const INITIAL_B: i64 = 2_000_000;
    const FRESH_A: i64 = 6_000_000;
    const FRESH_B: i64 = 4_000_000;

    let coherent_initial = i128::from(INITIAL_A)
        .checked_mul(1_000_000)
        .and_then(|value| value.checked_div(i128::from(INITIAL_B)))
        .ok_or("PR 331 initial cross-rate arithmetic failed")?;
    let coherent_fresh = i128::from(FRESH_A)
        .checked_mul(1_000_000)
        .and_then(|value| value.checked_div(i128::from(FRESH_B)))
        .ok_or("PR 331 fresh cross-rate arithmetic failed")?;
    if coherent_initial != i128::from(COHERENT_PRICE)
        || coherent_fresh != i128::from(COHERENT_PRICE)
    {
        return Err("PR 331 fixture does not preserve its coherent cross-rate".into());
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
            actor_deposits: [
                540_000,
                540_000,
                1_000,
                super::v16_svm::USER_DEPOSIT,
                super::v16_svm::EXIT_MAKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    env.update_liquidation_fee_policy(5_000)
        .map_err(|error| format!("PR 331 configure cranker share: {error}"))?;
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
    .map_err(|error| format!("PR 331 configure coherent cross-rate: {error}"))?;
    if env.primary_market_state().0.oracle_target_price_e6 != COHERENT_PRICE {
        return Err(format!(
            "PR 331 initial target is {}, expected {COHERENT_PRICE}",
            env.primary_market_state().0.oracle_target_price_e6
        ));
    }

    let size_q = (POS_SCALE as i128)
        .checked_mul(35)
        .and_then(|value| value.checked_div(100))
        .ok_or("PR 331 victim size overflow")?;
    env.trade_no_cpi(1, 0, 0, size_q, COHERENT_PRICE, 0)
        .map_err(|error| format!("PR 331 open independent victim short: {error}"))?;
    if observed_positions(&env.primary_portfolio(0))?[0] >= 0 {
        return Err("PR 331 fixture did not put the victim on the short side".into());
    }
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
    let mut max_crank_cu = 0u64;
    for _ in 0..12 {
        slot = slot
            .checked_add(20)
            .ok_or("PR 331 catch-up slot overflow")?;
        env.set_clock(slot, 101);
        match env.crank_with_oracles(2, slot, observations(), &skewed) {
            Ok(success) => max_crank_cu = max_crank_cu.max(success.compute_units),
            Err(error) => {
                return Err(format!(
                    "PR 331 skewed composite no longer advances at slot {slot}: {error}"
                ))
            }
        }
    }
    let refresh = env
        .crank_with_oracles(0, slot, observations(), &skewed)
        .map_err(|error| format!("PR 331 refresh false-price victim: {error}"))?;
    max_crank_cu = max_crank_cu.max(refresh.compute_units);
    let victim_cert = env
        .primary_portfolio(0)
        .health_cert
        .try_to_runtime()
        .map_err(|error| format!("PR 331 decode victim certificate: {error:?}"))?;
    if victim_cert.certified_liq_deficit == 0 {
        return Err("PR 331 skewed cross-rate no longer certifies liquidation".into());
    }

    let liquidation = env
        .crank_with_reward(2, 0, slot, observations(), &skewed)
        .map_err(|error| format!("PR 331 false-price liquidation no longer lands: {error}"))?;
    max_crank_cu = max_crank_cu.max(liquidation.compute_units);
    let (wrapper_after, group_after) = env.primary_market_state();
    let victim_capital_after = env.primary_portfolio(0).capital.get();
    let oi_after = group_after.assets[0].oi_eff_short_q;
    let cranker_capital_after = env.primary_portfolio(2).capital.get();
    let victim_capital_loss = victim_capital_before
        .checked_sub(victim_capital_after)
        .ok_or("PR 331 false liquidation increased victim capital")?;
    let oi_reduction_q = oi_before
        .checked_sub(oi_after)
        .ok_or("PR 331 false liquidation increased short OI")?;
    let cranker_reward = cranker_capital_after
        .checked_sub(cranker_capital_before)
        .ok_or("PR 331 false liquidation decreased cranker capital")?;
    if victim_capital_loss == 0 || oi_reduction_q == 0 || cranker_reward == 0 {
        return Err(format!(
            "PR 331 skewed reports were not economically committed: victim loss \
             {victim_capital_loss}, OI reduction {oi_reduction_q}, reward {cranker_reward}"
        ));
    }
    env.withdraw_primary(2, cranker_reward)
        .map_err(|error| format!("PR 331 withdraw liquidation reward: {error}"))?;
    let extracted_tokens = env.token_amount(env.actors[2].destination_token);
    if u128::from(extracted_tokens) != cranker_reward {
        return Err(format!(
            "PR 331 reward/SPL mismatch: {cranker_reward} vs {extracted_tokens}"
        ));
    }
    if wrapper_after.oracle_target_price_e6 <= COHERENT_PRICE
        || group_after.assets[0].effective_price <= COHERENT_PRICE
        || env.token_supply_observed() != supply_before
        || max_crank_cu >= TX_CU_LIMIT
    {
        return Err(format!(
            "PR 331 witness did not preserve its public value/CU conditions: target={}, mark={}, \
             supply={}/{}, max_cu={max_crank_cu}",
            wrapper_after.oracle_target_price_e6,
            group_after.assets[0].effective_price,
            env.token_supply_observed(),
            supply_before
        ));
    }
    Ok(CompositeOracleTimeSkewReproduction {
        blocker: KnownBlocker::CompositeOracleTimeSkew,
        coherent_price: COHERENT_PRICE,
        skewed_target: wrapper_after.oracle_target_price_e6,
        skewed_mark: group_after.assets[0].effective_price,
        victim_capital_loss,
        oi_reduction_q,
        cranker_reward,
        extracted_tokens,
        max_crank_cu,
    })
}

pub fn reproduce_unstaged_mark_target(
    mut seed: [u8; 32],
    case: TargetStagingCase,
) -> Result<TargetStagingReproduction, String> {
    seed[0] ^= match case {
        TargetStagingCase::AuthMarkPush => 0x32,
        TargetStagingCase::EwmaMarkPush => 0xa2,
        TargetStagingCase::EwmaSingleTrade => 0x33,
        TargetStagingCase::EwmaBatchTrade => 0xb3,
    };
    const OLD_MARK: u64 = 100;
    const AUTH_TARGET: u64 = 200;
    const EWMA_TARGET: u64 = 150;
    const ATTACK_SIZE_Q: i128 = 10_000 * POS_SCALE as i128;
    const EXISTING_SIZE_Q: i128 = POS_SCALE as i128;

    let (mut env, attacker, lp, attack_size_q, wrapper_target, engine_epoch_before) = match case {
        TargetStagingCase::AuthMarkPush | TargetStagingCase::EwmaMarkPush => {
            let mut env = V16Svm::new(
                seed,
                MarketConfig {
                    initial_price: OLD_MARK,
                    max_price_move_bps_per_slot: 10_000,
                    max_accrual_dt_slots: 1,
                    actor_deposits: [
                        1_000_100,
                        4_000_000,
                        super::v16_svm::USER_DEPOSIT,
                        super::v16_svm::USER_DEPOSIT,
                        super::v16_svm::EXIT_MAKER_DEPOSIT,
                    ],
                    ..MarketConfig::default()
                },
            );
            match case {
                TargetStagingCase::AuthMarkPush => {
                    env.configure_auth_mark(false, 0, 0, OLD_MARK)
                        .map_err(|error| format!("PR 264/332 configure AuthMark: {error}"))?
                }
                TargetStagingCase::EwmaMarkPush => env
                    .configure_ewma_mark(0, 0, OLD_MARK, 1, 0)
                    .map_err(|error| format!("PR 265/332 configure EWMA mark: {error}"))?,
                TargetStagingCase::EwmaSingleTrade | TargetStagingCase::EwmaBatchTrade => {
                    unreachable!()
                }
            };
            env.trade_cpi(0, 1, 0, EXISTING_SIZE_Q, 0, 0)
                .map_err(|error| format!("PR 332 open liveness-control position: {error}"))?;
            let epoch_before = env.primary_market_state().1.oracle_epoch;
            env.warp_to_slot(2);
            let wrapper_target = match case {
                TargetStagingCase::AuthMarkPush => {
                    env.push_auth_mark(0, 2, AUTH_TARGET)
                        .map_err(|error| format!("PR 264/332 push honest AuthMark: {error}"))?;
                    AUTH_TARGET
                }
                TargetStagingCase::EwmaMarkPush => {
                    env.push_ewma_mark(0, 2, AUTH_TARGET)
                        .map_err(|error| format!("PR 265/332 push honest EWMA mark: {error}"))?;
                    EWMA_TARGET
                }
                TargetStagingCase::EwmaSingleTrade | TargetStagingCase::EwmaBatchTrade => {
                    unreachable!()
                }
            };
            (
                env,
                0usize,
                1usize,
                ATTACK_SIZE_Q
                    .checked_add(EXISTING_SIZE_Q)
                    .ok_or("PR 332 total attack size overflow")?,
                wrapper_target,
                epoch_before,
            )
        }
        TargetStagingCase::EwmaSingleTrade | TargetStagingCase::EwmaBatchTrade => {
            let mut env = V16Svm::new(
                seed,
                MarketConfig {
                    initial_price: OLD_MARK,
                    max_price_move_bps_per_slot: 10_000,
                    max_accrual_dt_slots: 1,
                    actor_deposits: [
                        1_000,
                        1_000,
                        1_000_000,
                        4_000_000,
                        super::v16_svm::EXIT_MAKER_DEPOSIT,
                    ],
                    ..MarketConfig::default()
                },
            );
            env.configure_ewma_mark(0, 0, OLD_MARK, 1, 0)
                .map_err(|error| format!("PR 333 configure EWMA mark: {error}"))?;
            env.warp_to_slot(2);
            let epoch_before = env.primary_market_state().1.oracle_epoch;
            let route = match case {
                TargetStagingCase::EwmaSingleTrade => TradeRoute::NoCpi,
                TargetStagingCase::EwmaBatchTrade => TradeRoute::BatchNoCpi,
                TargetStagingCase::AuthMarkPush | TargetStagingCase::EwmaMarkPush => unreachable!(),
            };
            execute_trade_route(&mut env, route, 0, 1, 0, EXISTING_SIZE_Q, 200, 0)
                .map_err(|error| format!("PR 333 publish trade-driven EWMA target: {error}"))?;
            let discovery_profile = env.primary_profile(0);
            if discovery_profile.mark_ewma_e6 != EWMA_TARGET {
                return Err(format!(
                    "PR 333 discovery trade did not move EWMA: mark={}, target={}, insurance={}",
                    discovery_profile.mark_ewma_e6,
                    discovery_profile.oracle_target_price_e6,
                    env.primary_market_state().1.insurance
                ));
            }
            execute_trade_route(&mut env, route, 0, 1, 0, -EXISTING_SIZE_Q, OLD_MARK, 0)
                .map_err(|error| format!("PR 333 flatten discovery portfolios: {error}"))?;
            if portfolio_has_active_asset(&env.primary_portfolio(0), 0)
                || portfolio_has_active_asset(&env.primary_portfolio(1), 0)
            {
                return Err("PR 333 discovery positions did not flatten".into());
            }
            (
                env,
                2usize,
                3usize,
                ATTACK_SIZE_Q,
                EWMA_TARGET,
                epoch_before,
            )
        }
    };

    let supply_before = env.token_supply_observed();
    let profile = env.primary_profile(0);
    let (_, staged_group) = env.primary_market_state();
    let stale_engine_target = staged_group.assets[0].raw_oracle_target_price;
    let expected_profile_target = match case {
        TargetStagingCase::AuthMarkPush | TargetStagingCase::EwmaMarkPush => wrapper_target,
        TargetStagingCase::EwmaSingleTrade | TargetStagingCase::EwmaBatchTrade => OLD_MARK,
    };
    if profile.mark_ewma_e6 != wrapper_target
        || profile.oracle_target_price_e6 != expected_profile_target
        || staged_group.assets[0].effective_price != OLD_MARK
        || stale_engine_target != OLD_MARK
        || staged_group.oracle_epoch != engine_epoch_before
    {
        return Err(format!(
            "{case:?} no longer exposes an unstaged target: profile={}/{}, raw={}, effective={}, \
             epoch={}/{}",
            profile.mark_ewma_e6,
            profile.oracle_target_price_e6,
            stale_engine_target,
            staged_group.assets[0].effective_price,
            staged_group.oracle_epoch,
            engine_epoch_before
        ));
    }

    let attacker_capital_before = env.primary_portfolio(attacker).capital.get();
    let victim_capital_before = env.primary_portfolio(lp).capital.get();
    let stale_increase = env
        .trade_cpi(attacker, lp, 0, ATTACK_SIZE_Q, 0, 0)
        .map_err(|error| format!("{case:?} stale-price CPI risk increase rejected: {error}"))?;
    if stale_increase.compute_units >= TX_CU_LIMIT {
        return Err(format!(
            "{case:?} stale-price risk increase consumed {} CU",
            stale_increase.compute_units
        ));
    }

    env.warp_to_slot(3);
    let mut moved_engine_mark = OLD_MARK;
    let mut crank_errors = Vec::new();
    for _ in 0..8 {
        for actor in [attacker, lp] {
            if let Err(error) = env.crank(
                actor,
                3,
                vec![CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                }],
            ) {
                crank_errors.push(format!("actor {actor}: {error}"));
            }
        }
        moved_engine_mark = env.primary_market_state().1.assets[0].effective_price;
        if moved_engine_mark == wrapper_target {
            break;
        }
    }
    if moved_engine_mark != wrapper_target {
        return Err(format!(
            "{case:?} public crank did not commit stale target: \
             {moved_engine_mark}/{wrapper_target}; {}",
            crank_errors.join(" | ")
        ));
    }

    env.trade_cpi(attacker, lp, 0, -attack_size_q, 0, 0)
        .map_err(|error| format!("{case:?} close stale-price exposure: {error}"))?;
    let attacker_flat = env.primary_portfolio(attacker);
    let victim_flat = env.primary_portfolio(lp);
    if portfolio_has_active_asset(&attacker_flat, 0) || portfolio_has_active_asset(&victim_flat, 0)
    {
        return Err(format!("{case:?} stale-price positions did not flatten"));
    }
    let attacker_profit = u128::try_from(attacker_flat.pnl.get())
        .map_err(|_| format!("{case:?} attacker did not realize positive PnL"))?;
    let victim_capital_loss = victim_capital_before
        .checked_sub(victim_flat.capital.get())
        .ok_or_else(|| format!("{case:?} LP capital increased"))?;
    let expected_profit = u128::from(wrapper_target - OLD_MARK)
        .checked_mul(attack_size_q.unsigned_abs())
        .and_then(|value| value.checked_div(POS_SCALE))
        .ok_or_else(|| format!("{case:?} expected profit overflow"))?;
    if attacker_profit != expected_profit || victim_capital_loss != expected_profit {
        return Err(format!(
            "{case:?} stale-window value transfer mismatch: attacker={attacker_profit}, \
             victim={victim_capital_loss}, expected={expected_profit}"
        ));
    }

    env.convert_released_pnl(attacker, attacker_profit)
        .map_err(|error| format!("{case:?} convert stale-window PnL: {error}"))?;
    let attacker_capital_after = env.primary_portfolio(attacker).capital.get();
    env.withdraw_primary(attacker, attacker_capital_after)
        .map_err(|error| format!("{case:?} withdraw stale-window proceeds: {error}"))?;
    let attacker_withdrawn = env.token_amount(env.actors[attacker].destination_token);
    let expected_withdrawal = attacker_capital_before
        .checked_add(attacker_profit)
        .ok_or_else(|| format!("{case:?} expected withdrawal overflow"))?;
    if u128::from(attacker_withdrawn) != expected_withdrawal
        || env.token_supply_observed() != supply_before
    {
        return Err(format!(
            "{case:?} stale-window profit was not publicly extractable: withdrew \
             {attacker_withdrawn}/{expected_withdrawal}, supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }
    Ok(TargetStagingReproduction {
        blocker: KnownBlocker::UnstagedMarkTarget,
        case,
        wrapper_target,
        stale_engine_target,
        moved_engine_mark,
        attacker_profit,
        victim_capital_loss,
        attacker_withdrawn,
        attack_cu: stale_increase.compute_units,
    })
}

pub fn reproduce_pending_mark_fee_reward(
    mut seed: [u8; 32],
) -> Result<PendingMarkFeeRewardReproduction, String> {
    seed[0] ^= 0x56;
    let control = run_pending_mark_fee_reward_world(seed, false)?;
    let reordered = run_pending_mark_fee_reward_world(seed, true)?;
    if !reordered.pending_sync_rejected_lock
        || !reordered.pending_sync_exact_rollback
        || control.reward != reordered.reward
        || control.victim_payout != reordered.victim_payout
        || control.winner_payout != reordered.winner_payout
        || control.extracted_reward != reordered.extracted_reward
    {
        return Err(format!(
            "PR 356 fixed route did not reject and converge after mark commitment: \
             control={control:?}, reordered={reordered:?}"
        ));
    }
    Ok(PendingMarkFeeRewardReproduction {
        blocker: KnownBlocker::PendingMarkFeeReward,
        pending_sync_rejected_lock: reordered.pending_sync_rejected_lock,
        pending_sync_exact_rollback: reordered.pending_sync_exact_rollback,
        control_reward: control.reward,
        reordered_reward: reordered.reward,
        control_winner_payout: control.winner_payout,
        reordered_winner_payout: reordered.winner_payout,
        victim_payout: reordered.victim_payout,
        extracted_reward: reordered.extracted_reward,
    })
}

#[derive(Clone, Copy, Debug)]
struct PendingMarkFeeWorld {
    pending_sync_rejected_lock: bool,
    pending_sync_exact_rollback: bool,
    reward: u64,
    victim_payout: u64,
    winner_payout: u64,
    extracted_reward: u64,
}

fn run_pending_mark_fee_reward_world(
    seed: [u8; 32],
    fee_first: bool,
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
        .map_err(|error| format!("PR 356 configure AuthMark: {error}"))?;
    env.update_maintenance_fee_policy(10_000)
        .map_err(|error| format!("PR 356 configure maintenance reward: {error}"))?;
    env.trade_no_cpi(0, 1, 0, SIZE_Q, OPEN_PRICE, 0)
        .map_err(|error| format!("PR 356 open victim/winner positions: {error}"))?;

    env.warp_to_slot(9);
    env.push_auth_mark(0, 9, OPEN_PRICE)
        .map_err(|error| format!("PR 356 advance authenticated fee clock: {error}"))?;
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
    let victim_before = env.primary_portfolio(0);
    let (_, current_group) = env.primary_market_state();
    if victim_before.last_fee_slot.get() != 1 || current_group.assets[0].slot_last != 9 {
        return Err(format!(
            "PR 356 setup failed to isolate fee debt: fee_slot={}, asset_slot={}",
            victim_before.last_fee_slot.get(),
            current_group.assets[0].slot_last
        ));
    }

    env.warp_to_slot(10);
    env.push_auth_mark(0, 10, ADVERSE_PRICE)
        .map_err(|error| format!("PR 356 publish adverse mark: {error}"))?;
    let (pending_profile, pending_group) = env.primary_market_state();
    if pending_profile.oracle_target_price_e6 != ADVERSE_PRICE
        || pending_group.assets[0].raw_oracle_target_price != OPEN_PRICE
        || pending_group.assets[0].effective_price != OPEN_PRICE
    {
        return Err("PR 356 fixture did not create wrapper/engine target lag".into());
    }

    let mut pending_sync_rejected_lock = false;
    let mut pending_sync_exact_rollback = true;
    if fee_first {
        let before = (
            env.svm.get_account(&env.market),
            env.svm.get_account(&env.actors[0].portfolio),
            env.svm.get_account(&env.actors[2].portfolio),
        );
        match env.sync_maintenance_fee_with_reward(0, 2, 10) {
            Ok(_) => return Err("PR 356 pending-mark fee sync still landed".into()),
            Err(error)
                if error.contains("Custom(21)") || error.contains("custom program error: 0x15") =>
            {
                pending_sync_rejected_lock = true;
                pending_sync_exact_rollback = before
                    == (
                        env.svm.get_account(&env.market),
                        env.svm.get_account(&env.actors[0].portfolio),
                        env.svm.get_account(&env.actors[2].portfolio),
                    );
            }
            Err(error) => {
                return Err(format!(
                    "PR 356 pending-mark fee sync returned an unexpected error: {error}"
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
        return Err("PR 356 public crank did not commit the adverse mark".into());
    }
    env.sync_maintenance_fee_with_reward(0, 2, 10)
        .map_err(|error| format!("PR 356 post-commit fee sync: {error}"))?;

    let cranker_capital = env.primary_portfolio(2).capital.get();
    let reward = cranker_capital
        .checked_sub(1)
        .ok_or("PR 356 cranker capital fell below its fixture deposit")?;
    if reward == 0 {
        return Err("PR 356 maintenance sync paid no public reward".into());
    }
    env.withdraw_primary(2, cranker_capital)
        .map_err(|error| format!("PR 356 withdraw cranker reward: {error}"))?;
    let cranker_withdrawn = env.token_amount(env.actors[2].destination_token);
    let extracted_reward = cranker_withdrawn
        .checked_sub(1)
        .ok_or("PR 356 cranker withdrawal lost fixture deposit")?;
    if u128::from(extracted_reward) != reward {
        return Err(format!(
            "PR 356 cranker reward/SPL mismatch: {reward}/{extracted_reward}"
        ));
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
        .map_err(|error| format!("PR 356 resolve terminal world: {error}"))?;
    let (victim_payout, _) = drain_resolved_actor(&mut env, 0)?;
    let (winner_payout, _) = drain_resolved_actor(&mut env, 1)?;
    if env.token_supply_observed() != supply_before {
        return Err("PR 356 terminal payout changed SPL supply".into());
    }
    Ok(PendingMarkFeeWorld {
        pending_sync_rejected_lock,
        pending_sync_exact_rollback,
        reward: u64::try_from(reward).map_err(|_| "PR 356 reward exceeds SPL range")?,
        victim_payout: u64::try_from(victim_payout)
            .map_err(|_| "PR 356 victim payout exceeds SPL range")?,
        winner_payout: u64::try_from(winner_payout)
            .map_err(|_| "PR 356 winner payout exceeds SPL range")?,
        extracted_reward,
    })
}

pub fn reproduce_fractional_cap_settlement(
    mut seed: [u8; 32],
) -> Result<FractionalCapSettlementReproduction, String> {
    seed[0] ^= 0x65;
    const OPEN_PRICE: u64 = 100;
    const TARGET_PRICE: u64 = 1;
    const CAP_BPS: u64 = 24;
    const MAX_DT: u64 = 20;
    const DEPOSIT: u128 = 1_000_000;

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
        .map_err(|error| format!("PR 365 configure permissionless resolve: {error}"))?;
    env.configure_auth_mark(false, 0, 1, OPEN_PRICE)
        .map_err(|error| format!("PR 365 configure AuthMark: {error}"))?;
    env.trade_no_cpi(0, 1, 0, POS_SCALE as i128, OPEN_PRICE, 0)
        .map_err(|error| format!("PR 365 open independent long/short: {error}"))?;
    env.warp_to_slot(2);
    env.push_auth_mark(0, 2, TARGET_PRICE)
        .map_err(|error| format!("PR 365 publish honest micro-price target: {error}"))?;

    let mut slot = 2u64
        .checked_add(MAX_DT)
        .ok_or("PR 365 initial crank slot overflow")?;
    let mut successful_cranks = 0u16;
    let mut nonmoving_attempts = 0u8;
    let mut rollback_stalls = 0u8;
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
                    .ok_or("PR 365 successful crank count overflow")?;
                let price_after = env.primary_market_state().1.assets[0].effective_price;
                if price_after == price_before {
                    nonmoving_attempts = nonmoving_attempts.saturating_add(1);
                } else {
                    nonmoving_attempts = 0;
                    rollback_stalls = 0;
                }
            }
            Err(_) => {
                if env.market_data(false) != market_before
                    || env.primary_portfolio_data(0) != long_before
                    || env.primary_portfolio_data(1) != short_before
                {
                    return Err("PR 365 rejected crank did not roll back economic state".into());
                }
                rollback_stalls = rollback_stalls.saturating_add(1);
            }
        }
        let price_after = env.primary_market_state().1.assets[0].effective_price;
        if price_after == TARGET_PRICE {
            return Err("PR 365 fractional cap now reaches its target; remove quarantine".into());
        }
        if (nonmoving_attempts >= 3 || rollback_stalls >= 3) && price_after > TARGET_PRICE {
            break;
        }
        slot = slot
            .checked_add(MAX_DT)
            .ok_or("PR 365 crank slot overflow")?;
    }
    let stalled = env.primary_market_state().1.assets[0];
    if successful_cranks == 0
        || (nonmoving_attempts < 3 && rollback_stalls < 3)
        || stalled.effective_price <= TARGET_PRICE
        || stalled.raw_oracle_target_price != TARGET_PRICE
    {
        return Err(format!(
            "PR 365 did not reach a persistent max-dt floor: price={}, raw={}, successes={}, \
             nonmoving={}, rejected={rollback_stalls}",
            stalled.effective_price,
            stalled.raw_oracle_target_price,
            successful_cranks,
            nonmoving_attempts
        ));
    }

    let resolve_slot = slot
        .checked_add(10_001)
        .ok_or("PR 365 resolve slot overflow")?;
    env.resolve_stale_permissionless(resolve_slot)
        .map_err(|error| format!("PR 365 permissionless stale resolve: {error}"))?;
    env.warp_to_slot(
        resolve_slot
            .checked_add(1)
            .ok_or("PR 365 close slot overflow")?,
    );
    let (long_payout_u128, _) = drain_resolved_actor(&mut env, 0)?;
    let (short_payout_u128, _) = drain_resolved_actor(&mut env, 1)?;
    let long_payout =
        u64::try_from(long_payout_u128).map_err(|_| "PR 365 long payout exceeds SPL range")?;
    let short_payout =
        u64::try_from(short_payout_u128).map_err(|_| "PR 365 short payout exceeds SPL range")?;
    let target_long_payout = u64::try_from(DEPOSIT)
        .ok()
        .and_then(|deposit| deposit.checked_sub(OPEN_PRICE - TARGET_PRICE))
        .ok_or("PR 365 target long payout arithmetic failed")?;
    let target_short_payout = u64::try_from(DEPOSIT)
        .ok()
        .and_then(|deposit| deposit.checked_add(OPEN_PRICE - TARGET_PRICE))
        .ok_or("PR 365 target short payout arithmetic failed")?;
    let long_overpayment = long_payout.checked_sub(target_long_payout).ok_or_else(|| {
        format!(
            "PR 365 stalled settlement underpaid the long: actual={long_payout}, target={target_long_payout}, short={short_payout}/{target_short_payout}, stalled={}",
            stalled.effective_price
        )
    })?;
    let short_underpayment = target_short_payout.checked_sub(short_payout).ok_or_else(|| {
        format!(
            "PR 365 stalled settlement overpaid the short: actual={short_payout}, target={target_short_payout}, long={long_payout}/{target_long_payout}, stalled={}",
            stalled.effective_price
        )
    })?;
    if long_overpayment == 0
        || long_overpayment != short_underpayment
        || u128::from(long_payout) + u128::from(short_payout) != DEPOSIT * 2
        || env.token_supply_observed() != supply_before
    {
        return Err(format!(
            "PR 365 stalled mark did not transfer conserved terminal value: payouts={long_payout}/{short_payout}, target={target_long_payout}/{target_short_payout}, supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }
    Ok(FractionalCapSettlementReproduction {
        blocker: KnownBlocker::FractionalCapSettlement,
        target_price: TARGET_PRICE,
        stalled_price: stalled.effective_price,
        successful_cranks,
        rollback_stalls,
        long_payout,
        short_payout,
        long_overpayment,
        short_underpayment,
    })
}

pub fn reproduce_prospective_funding_rewrite(
    mut seed: [u8; 32],
    route: TradeRoute,
) -> Result<ProspectiveFundingRewriteReproduction, String> {
    seed[0] ^= match route {
        TradeRoute::NoCpi => 0x80,
        TradeRoute::Cpi => 0xc8,
        TradeRoute::BatchNoCpi => 0xb8,
        TradeRoute::BatchCpi => 0xd8,
    };
    let control = run_prospective_funding_world(seed, route, false)?;
    let attack = run_prospective_funding_world(seed, route, true)?;
    let fixed_execution_price = matches!(route, TradeRoute::NoCpi | TradeRoute::BatchNoCpi);
    if control.f_short_num <= 0
        || (fixed_execution_price
            && (control.stamp_fee != attack.stamp_fee
                || control.final_mark != attack.final_mark
                || control.final_effective_price != attack.final_effective_price))
    {
        return Err(format!(
            "PR 380 worlds do not isolate funding timestamp order: control={control:?}, \
             attack={attack:?}"
        ));
    }
    let victim_payout_loss = control
        .victim_payout
        .checked_sub(attack.victim_payout)
        .ok_or("PR 380 trade-first ordering increased victim payout")?;
    let attacker_coalition_gain = attack
        .coalition_payout
        .checked_sub(control.coalition_payout)
        .ok_or("PR 380 trade-first ordering decreased coalition payout")?;
    if fixed_execution_price
        && (victim_payout_loss != attacker_coalition_gain
            || control.total_payout != attack.total_payout)
    {
        return Err(format!(
            "PR 380 prospective worlds do not conserve order-independent SPL value: \
             control={control:?}, attack={attack:?}"
        ));
    }
    Ok(ProspectiveFundingRewriteReproduction {
        blocker: KnownBlocker::ProspectiveFundingRewrite,
        route,
        control_f_short_num: control.f_short_num,
        attack_f_short_num: attack.f_short_num,
        stamp_fee: attack.stamp_fee,
        final_mark: attack.final_mark,
        final_effective_price: attack.final_effective_price,
        victim_payout_loss,
        attacker_coalition_gain,
        control_total_payout: control.total_payout,
        attack_total_payout: attack.total_payout,
    })
}

pub fn reproduce_resolve_before_committed_accrual(
    mut seed: [u8; 32],
) -> Result<ResolveBeforeCommittedAccrualReproduction, String> {
    seed[0] ^= 0x55;
    let control = run_pending_mark_resolve_world(seed, true)?;
    let attack = run_pending_mark_resolve_world(seed, false)?;
    let victim_payout_loss = control
        .long_payout
        .checked_sub(attack.long_payout)
        .ok_or("PR 255 resolve-first ordering increased victim payout")?;
    let attacker_payout_gain = attack
        .short_payout
        .checked_sub(control.short_payout)
        .ok_or("PR 255 resolve-first ordering decreased attacker payout")?;
    if control.effective_mark != attack.effective_mark
        || !attack.unsafe_resolve_rejected
        || !attack.rejected_exact_rollback
        || victim_payout_loss != 0
        || attacker_payout_gain != 0
        || control.total_payout != attack.total_payout
        || attack.catchup_steps == 0
        || attack.catchup_cu >= TX_CU_LIMIT
        || attack.resolve_cu >= TX_CU_LIMIT
    {
        return Err(format!(
            "PR 255 stale resolve did not reject, catch up, and preserve terminal value: \
             control={control:?}, attack={attack:?}"
        ));
    }
    Ok(ResolveBeforeCommittedAccrualReproduction {
        blocker: KnownBlocker::ResolveBeforeCommittedAccrual,
        control_mark: control.effective_mark,
        attack_mark: attack.effective_mark,
        unsafe_resolve_rejected: attack.unsafe_resolve_rejected,
        rejected_exact_rollback: attack.rejected_exact_rollback,
        victim_payout_loss,
        attacker_payout_gain,
        control_total_payout: control.total_payout,
        attack_total_payout: attack.total_payout,
        catchup_steps: attack.catchup_steps,
        catchup_cu: attack.catchup_cu,
        attack_resolve_cu: attack.resolve_cu,
    })
}

#[derive(Clone, Copy, Debug)]
struct PendingMarkResolveWorld {
    effective_mark: u64,
    unsafe_resolve_rejected: bool,
    rejected_exact_rollback: bool,
    long_payout: u64,
    short_payout: u64,
    total_payout: u128,
    catchup_steps: u16,
    catchup_cu: u64,
    resolve_cu: u64,
}

fn run_pending_mark_resolve_world(
    seed: [u8; 32],
    commit_mark_before_resolve: bool,
) -> Result<PendingMarkResolveWorld, String> {
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
            actor_deposits: [
                DEPOSIT,
                DEPOSIT,
                super::v16_svm::USER_DEPOSIT,
                super::v16_svm::USER_DEPOSIT,
                super::v16_svm::EXIT_MAKER_DEPOSIT,
            ],
            actor_token_balances: [
                super::v16_svm::EXIT_MAKER_TOKEN_BALANCE,
                super::v16_svm::EXIT_MAKER_TOKEN_BALANCE,
                200_000_000,
                200_000_000,
                super::v16_svm::EXIT_MAKER_TOKEN_BALANCE,
            ],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.configure_permissionless_resolve(3, 1)
        .map_err(|error| format!("PR 255 configure permissionless resolve: {error}"))?;
    env.trade_no_cpi(0, 1, 0, SIZE_Q, PRICE, 0)
        .map_err(|error| format!("PR 255 open independent long/short pair: {error}"))?;
    env.warp_to_slot(PUSH_SLOT);
    env.push_auth_mark(0, PUSH_SLOT, MARK)
        .map_err(|error| format!("PR 255 publish honest pending AuthMark: {error}"))?;
    let (_, pending) = env.primary_market_state();
    if env.primary_profile(0).mark_ewma_e6 != MARK || pending.assets[0].effective_price != PRICE {
        return Err(format!(
            "PR 255 fixture did not retain a pending authenticated mark: profile={}, engine={}",
            env.primary_profile(0).mark_ewma_e6,
            pending.assets[0].effective_price
        ));
    }
    if commit_mark_before_resolve {
        env.crank(
            0,
            PUSH_SLOT,
            vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: 0,
            }],
        )
        .map_err(|error| format!("PR 255 control mark accrual: {error}"))?;
    }

    let mut unsafe_resolve_rejected = false;
    let mut rejected_exact_rollback = false;
    let mut catchup_steps = 0u16;
    let mut catchup_cu = 0;
    let mut resolve_cu = None;
    if !commit_mark_before_resolve {
        let before_rejection = tracked_economic_accounts(&env);
        let resolve = env.resolve_stale_permissionless(RESOLVE_SLOT);
        unsafe_resolve_rejected = matches!(
            &resolve,
            Err(error)
                if error.contains("Custom(19)") || error.contains("custom program error: 0x13")
        );
        if !unsafe_resolve_rejected {
            return Err(format!(
                "PR 255 unsafe stale resolve returned an unexpected result: {resolve:?}"
            ));
        }
        rejected_exact_rollback = tracked_economic_accounts(&env) == before_rejection;
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
                .map_err(|error| format!("PR 255 stored-state catch-up crank: {error}"))?;
            catchup_steps = catchup_steps
                .checked_add(1)
                .ok_or("PR 255 catch-up step overflow")?;
            catchup_cu = catchup_cu.max(catchup.compute_units);

            let before_retry = tracked_economic_accounts(&env);
            match env.resolve_stale_permissionless(RESOLVE_SLOT) {
                Ok(resolve) => {
                    resolve_cu = Some(resolve.compute_units);
                    break;
                }
                Err(error)
                    if error.contains("Custom(19)")
                        || error.contains("custom program error: 0x13") =>
                {
                    rejected_exact_rollback &= tracked_economic_accounts(&env) == before_retry;
                }
                Err(error) => {
                    return Err(format!(
                        "PR 255 public stale resolve retry returned unexpected error: {error}"
                    ));
                }
            }
        }
        if resolve_cu.is_none() {
            return Err("PR 255 did not resolve within 16 bounded catch-up calls".into());
        }
    } else {
        let resolve = env
            .resolve_stale_permissionless(RESOLVE_SLOT)
            .map_err(|error| format!("PR 255 control stale resolve: {error}"))?;
        resolve_cu = Some(resolve.compute_units);
    }
    let (_, resolved) = env.primary_market_state();
    if resolved.mode != MarketModeV16::Resolved {
        return Err("PR 255 stale resolver did not terminalize the market".into());
    }
    let effective_mark = resolved.assets[0].effective_price;
    env.warp_to_slot(RESOLVE_SLOT + 1);
    let (short_payout, _) = drain_resolved_actor(&mut env, 1)?;
    let (long_payout, _) = drain_resolved_actor(&mut env, 0)?;
    let long_payout =
        u64::try_from(long_payout).map_err(|_| "PR 255 long payout exceeds SPL range")?;
    let short_payout =
        u64::try_from(short_payout).map_err(|_| "PR 255 short payout exceeds SPL range")?;
    let total_payout = u128::from(long_payout)
        .checked_add(u128::from(short_payout))
        .ok_or("PR 255 terminal payout overflow")?;
    if env.token_supply_observed() != supply_before {
        return Err("PR 255 terminal world changed SPL supply".into());
    }
    Ok(PendingMarkResolveWorld {
        effective_mark,
        unsafe_resolve_rejected,
        rejected_exact_rollback,
        long_payout,
        short_payout,
        total_payout,
        catchup_steps,
        catchup_cu,
        resolve_cu: resolve_cu.ok_or("PR 255 missing successful resolve CU")?,
    })
}

pub fn reproduce_bilateral_fee_support(
    mut seed: [u8; 32],
    mode: BilateralFeeMode,
    route: TradeRoute,
) -> Result<BilateralFeeSupportReproduction, String> {
    if !matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi) {
        return Err(format!(
            "PR 369 requires a CPI matcher route, got {route:?}"
        ));
    }
    seed[0] ^= match (mode, route) {
        (BilateralFeeMode::Ewma, TradeRoute::Cpi) => 0x69,
        (BilateralFeeMode::Ewma, TradeRoute::BatchCpi) => 0xb9,
        (BilateralFeeMode::HybridAfterHours, TradeRoute::Cpi) => 0xe9,
        (BilateralFeeMode::HybridAfterHours, TradeRoute::BatchCpi) => 0xf9,
        (_, TradeRoute::NoCpi | TradeRoute::BatchNoCpi) => unreachable!(),
    };
    const MARK: u64 = 1_000_000;
    const ADVERSE_MARK: u64 = 1_999_999;
    const MOVER_Q: i128 = POS_SCALE as i128;
    const BENEFICIARY_Q: i128 = 10 * POS_SCALE as i128;
    const LARGE_DEPOSIT: u128 = 50_000_000;

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
    if close_lp != 5 {
        return Err(format!("PR 369 unexpected close-LP index {close_lp}"));
    }
    let supply_before = env.token_supply_observed();
    let hybrid_feed = match mode {
        BilateralFeeMode::Ewma => {
            env.configure_ewma_mark(0, 1, MARK, 1, 0)
                .map_err(|error| format!("PR 369 configure EWMA mark: {error}"))?;
            None
        }
        BilateralFeeMode::HybridAfterHours => {
            env.set_clock(1, 100);
            let feed = [0xceu8; 32];
            let initial_oracle = env.set_pyth_price(&feed, MARK as i64, -6, 100, 100);
            env.configure_hybrid_oracle(
                0,
                1,
                100,
                0,
                [feed, [0u8; 32], [0u8; 32]],
                &[initial_oracle],
                1,
                100,
            )
            .map_err(|error| format!("PR 369 configure hybrid oracle: {error}"))?;
            Some(feed)
        }
    };

    env.set_matcher_spreads(1, 0, 9_000)
        .map_err(|error| format!("PR 369 configure opening passive matcher: {error}"))?;
    env.trade_cpi(0, 1, 0, -MOVER_Q, 0, 0)
        .map_err(|error| format!("PR 369 open future distressed short: {error}"))?;
    env.trade_no_cpi(2, 3, 0, BENEFICIARY_Q, MARK, 0)
        .map_err(|error| format!("PR 369 open independent beneficiary/victim book: {error}"))?;
    env.trade_no_cpi(4, close_lp, 0, BENEFICIARY_Q, MARK, 0)
        .map_err(|error| format!("PR 369 open independent extraction pair: {error}"))?;
    env.set_matcher_spreads(close_lp, 0, 9_000)
        .map_err(|error| format!("PR 369 configure extraction passive matcher: {error}"))?;

    let hybrid_oracle_tail = match mode {
        BilateralFeeMode::Ewma => {
            env.warp_to_slot(10);
            env.push_ewma_mark(0, 10, ADVERSE_MARK)
                .map_err(|error| format!("PR 369 publish honest EWMA mark: {error}"))?;
            None
        }
        BilateralFeeMode::HybridAfterHours => {
            env.set_clock(10, 110);
            Some(env.set_pyth_price(
                &hybrid_feed.ok_or("PR 369 hybrid feed missing")?,
                ADVERSE_MARK as i64,
                -6,
                100,
                110,
            ))
        }
    };
    let mut setup_max_cu = 0;
    for step in 0..16 {
        if env.primary_market_state().1.assets[0].slot_last >= 10 {
            break;
        }
        let observations = vec![CrankObservationHint {
            asset_index: 0,
            oracle_accounts: usize::from(hybrid_oracle_tail.is_some()) as u8,
        }];
        let success = if let Some(oracle) = hybrid_oracle_tail {
            env.crank_with_oracles(0, 10, observations, &[oracle])
        } else {
            env.crank(0, 10, observations)
        }
        .map_err(|error| format!("PR 369 setup market crank {step}: {error}"))?;
        setup_max_cu = setup_max_cu.max(success.compute_units);
    }
    if env.primary_market_state().1.assets[0].slot_last < 10 {
        return Err("PR 369 setup market did not reach the authenticated slot".into());
    }
    // The final market-catchup call dispatches actor 0. Give every other portfolio the same one
    // account-level selector step that the pre-catchup-only wrapper performed in its setup loop.
    for actor in 1..env.actors.len() {
        match env.crank(actor, 10, Vec::new()) {
            Ok(success) => setup_max_cu = setup_max_cu.max(success.compute_units),
            Err(error) if error.contains("Custom(22)") => {}
            Err(error) => return Err(format!("PR 369 setup account crank {actor}: {error}")),
        }
    }
    let setup_mark = env.primary_market_state().1.assets[0].effective_price;
    let mover_at_setup = env.primary_portfolio(0);
    if mover_at_setup.capital.get() == 0 || mover_at_setup.capital.get() >= u128::from(setup_mark) {
        return Err(format!(
            "PR 369 fixture did not leave a live underfunded mover: capital={}, mark={setup_mark}",
            mover_at_setup.capital.get()
        ));
    }

    let attacker_before = portfolio_equity(&env, 0)?
        .checked_add(portfolio_equity(&env, 2)?)
        .ok_or("PR 369 attacker pre-equity overflow")?;
    let victim_before = portfolio_equity(&env, 3)?;
    let fee_lp_before = portfolio_equity(&env, 1)?;
    let insurance_before = env.primary_market_state().1.insurance;

    env.set_matcher_spreads(1, 9_000, 9_000)
        .map_err(|error| format!("PR 369 configure exit passive matcher: {error}"))?;
    match mode {
        BilateralFeeMode::Ewma => env.warp_to_slot(20),
        BilateralFeeMode::HybridAfterHours => env.set_clock(20, 1_000),
    }
    let market_id = env.primary_market_state().1.assets[0].market_id;
    let exit = match route {
        TradeRoute::Cpi => env.trade_cpi(0, 1, 0, MOVER_Q, 0, 0),
        TradeRoute::BatchCpi => env.batch_trade_cpi(
            0,
            1,
            vec![BatchTradeCpiLeg {
                asset_index: 0,
                market_id,
                size_q: MOVER_Q,
                fee_bps: 0,
                limit_price: 0,
            }],
        ),
        TradeRoute::NoCpi | TradeRoute::BatchNoCpi => unreachable!(),
    }
    .map_err(|error| format!("PR 369 underfunded risk-reducing {route:?} exit: {error}"))?;
    let mut max_cu = setup_max_cu.max(exit.compute_units);
    let queued_mark = env.primary_profile(0).mark_ewma_e6;
    if queued_mark < setup_mark {
        return Err(format!(
            "PR 369 accepted upward print reversed mark: {setup_mark} -> {queued_mark}"
        ));
    }

    for actor in [2usize, 3usize] {
        let observations = if actor == 2 || hybrid_oracle_tail.is_some() {
            vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: usize::from(hybrid_oracle_tail.is_some()) as u8,
            }]
        } else {
            vec![]
        };
        let success = if let Some(oracle) = hybrid_oracle_tail {
            env.crank_with_oracles(actor, 20, observations, &[oracle])
        } else {
            env.crank(actor, 20, observations)
        }
        .map_err(|error| format!("PR 369 apply subsidized mark to actor {actor}: {error}"))?;
        max_cu = max_cu.max(success.compute_units);
    }

    let attacker_after = portfolio_equity(&env, 0)?
        .checked_add(portfolio_equity(&env, 2)?)
        .ok_or("PR 369 attacker post-equity overflow")?;
    let victim_after = portfolio_equity(&env, 3)?;
    let fee_lp_after = portfolio_equity(&env, 1)?;
    let insurance_after = env.primary_market_state().1.insurance;
    let victim_loss = u128::try_from(
        victim_before
            .checked_sub(victim_after)
            .ok_or("PR 369 victim equity increased")?,
    )
    .map_err(|_| "PR 369 victim loss is negative")?;
    let fee_lp_loss = u128::try_from(
        fee_lp_before
            .checked_sub(fee_lp_after)
            .ok_or("PR 369 fee LP equity increased")?,
    )
    .map_err(|_| "PR 369 fee LP loss is negative")?;
    let insurance_gain = insurance_after
        .checked_sub(insurance_before)
        .ok_or("PR 369 insurance decreased")?;

    let close = env
        .trade_cpi(2, close_lp, 0, -BENEFICIARY_Q, 0, 0)
        .map_err(|error| format!("PR 369 close beneficiary through independent LP: {error}"))?;
    max_cu = max_cu.max(close.compute_units);
    let released = env.primary_portfolio(2).pnl.get().max(0) as u128;
    if released == 0 {
        return Err("PR 369 subsidized mark produced no releasable attacker PnL".into());
    }
    env.convert_released_pnl(2, released)
        .map_err(|error| format!("PR 369 convert attacker PnL: {error}"))?;
    for actor in [2usize, 0usize] {
        let capital = env.primary_portfolio(actor).capital.get();
        if capital != 0 {
            let withdrawal = env
                .withdraw_primary(actor, capital)
                .map_err(|error| format!("PR 369 withdraw attacker actor {actor}: {error}"))?;
            max_cu = max_cu.max(withdrawal.compute_units);
        }
    }
    let extracted_tokens = u128::from(env.token_amount(env.actors[0].destination_token))
        .checked_add(u128::from(
            env.token_amount(env.actors[2].destination_token),
        ))
        .ok_or("PR 369 extracted SPL overflow")?;
    let attacker_before =
        u128::try_from(attacker_before).map_err(|_| "PR 369 attacker began insolvent")?;
    let coalition_excess = extracted_tokens.saturating_sub(attacker_before);
    if fee_lp_loss == 0
        || insurance_gain == 0
        || max_cu >= TX_CU_LIMIT
        || env.token_supply_observed() != supply_before
    {
        return Err(format!(
            "PR 369 public safety fixture failed: excess={coalition_excess}, \
             victim={victim_loss}, fee_lp={fee_lp_loss}, insurance={insurance_gain}, \
             internal_after={attacker_after}, max_cu={max_cu}, supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }
    Ok(BilateralFeeSupportReproduction {
        blocker: KnownBlocker::BilateralFeeSupport,
        mode,
        route,
        setup_mark,
        queued_mark,
        coalition_equity_before: attacker_before,
        coalition_excess,
        victim_loss,
        fee_lp_loss,
        insurance_gain,
        extracted_tokens,
        max_cu,
    })
}

pub fn reproduce_delayed_asset_authority_revival(
    seed: [u8; 32],
) -> Result<DelayedAssetAuthorityRevivalReproduction, String> {
    const ASSET: u16 = 0;
    const DOMAIN: u16 = 0;
    const DISPLACED_OPERATOR: usize = 0;
    const REPLACEMENT_OPERATOR: usize = 1;
    const PROVIDER: usize = 2;
    const AMOUNT: u128 = 50_000;

    let mut env = V16Svm::new(seed, MarketConfig::default());
    let supply_before = env.token_supply_observed();
    let provider_source = env.actors[PROVIDER].source_token;
    let attacker_destination = env.actors[DISPLACED_OPERATOR].destination_token;
    let provider_source_before = env.token_amount(provider_source);
    let attacker_destination_before = env.token_amount(attacker_destination);

    let retained_handoff = env.build_retained_asset_authority_handoff_from_admin(
        ASSET,
        percolator_prog::processor::ASSET_AUTH_INSURANCE_OPERATOR,
        DISPLACED_OPERATOR,
    );
    env.update_asset_authority_from_admin(
        ASSET,
        percolator_prog::processor::ASSET_AUTH_INSURANCE_OPERATOR,
        REPLACEMENT_OPERATOR,
    )
    .map_err(|error| format!("PR 251 install replacement operator: {error}"))?;
    env.update_asset_authority_from_admin(
        ASSET,
        percolator_prog::processor::ASSET_AUTH_INSURANCE,
        PROVIDER,
    )
    .map_err(|error| format!("PR 251 install independent insurance provider: {error}"))?;

    let profile = env.primary_profile(ASSET as usize);
    if profile.insurance_operator != env.actors[REPLACEMENT_OPERATOR].signer.pubkey().to_bytes()
        || profile.insurance_authority != env.actors[PROVIDER].signer.pubkey().to_bytes()
    {
        return Err("PR 251 replacement authority handoffs did not commit".into());
    }

    let top_up = env
        .top_up_insurance_domain_for_actor(PROVIDER, DOMAIN, AMOUNT)
        .map_err(|error| format!("PR 251 independent provider top-up: {error}"))?;
    let funded_reserve = env.primary_market_state().1.insurance_domain_budget[DOMAIN as usize];
    let provider_debit = provider_source_before
        .checked_sub(env.token_amount(provider_source))
        .ok_or("PR 251 provider source increased")?;
    if funded_reserve != AMOUNT || provider_debit != AMOUNT as u64 {
        return Err(format!(
            "PR 251 provider funding did not reach the live reserve: debit={provider_debit}, \
             reserve={funded_reserve}"
        ));
    }

    let handoff = env
        .land_retained(retained_handoff)
        .map_err(|error| format!("PR 251 stale admin handoff no longer lands: {error}"))?;
    if env.primary_profile(ASSET as usize).insurance_operator
        != env.actors[DISPLACED_OPERATOR].signer.pubkey().to_bytes()
    {
        return Err(
            "PR 251 retained handoff landed without reviving the displaced operator".into(),
        );
    }

    let withdrawal = env
        .withdraw_insurance_asset(DISPLACED_OPERATOR, ASSET, AMOUNT)
        .map_err(|error| format!("PR 251 revived operator could not drain reserve: {error}"))?;
    let attacker_extraction = env
        .token_amount(attacker_destination)
        .checked_sub(attacker_destination_before)
        .ok_or("PR 251 attacker destination decreased")?;
    let provider_loss = provider_source_before
        .checked_sub(env.token_amount(provider_source))
        .ok_or("PR 251 provider source increased after withdrawal")?;
    let reserve_after = env.primary_market_state().1.insurance_domain_budget[DOMAIN as usize];
    let max_cu = top_up
        .compute_units
        .max(handoff.compute_units)
        .max(withdrawal.compute_units);
    if provider_loss != AMOUNT as u64
        || attacker_extraction != AMOUNT as u64
        || reserve_after != 0
        || max_cu >= TX_CU_LIMIT
        || env.token_supply_observed() != supply_before
    {
        return Err(format!(
            "PR 251 public extraction conditions failed: provider_loss={provider_loss}, \
             attacker_extraction={attacker_extraction}, reserve={funded_reserve}->{reserve_after}, \
             max_cu={max_cu}, supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }

    Ok(DelayedAssetAuthorityRevivalReproduction {
        blocker: KnownBlocker::DelayedAssetAuthorityRevival,
        provider_loss,
        attacker_extraction,
        funded_reserve,
        reserve_after,
        handoff_cu: handoff.compute_units,
        withdrawal_cu: withdrawal.compute_units,
    })
}

pub fn reproduce_collateral_top_up_generation_replay(
    seed: [u8; 32],
) -> Result<CollateralTopUpGenerationReplayReproduction, String> {
    const ASSET: u16 = 1;
    const DOMAIN: u16 = ASSET * 2;
    const VICTIM: usize = 0;
    const ATTACKER: usize = 1;
    const AMOUNT: u128 = 250_000;

    let mut env = V16Svm::new(seed, MarketConfig::default());
    let supply_before = env.token_supply_observed();
    let victim_source = env.actors[VICTIM].source_token;
    let attacker_destination = env.actors[ATTACKER].destination_token;
    let victim_source_before = env.token_amount(victim_source);
    let attacker_destination_before = env.token_amount(attacker_destination);

    env.update_asset_authority_from_admin(
        ASSET,
        percolator_prog::processor::ASSET_AUTH_INSURANCE,
        VICTIM,
    )
    .map_err(|error| format!("PR 279 install old-generation insurance authority: {error}"))?;
    let old_market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    let retained_top_up =
        env.build_retained_insurance_domain_top_up_for_actor(VICTIM, DOMAIN, AMOUNT);

    env.update_market_init_fee_policy(1)
        .map_err(|error| format!("PR 279 configure permissionless init fee: {error}"))?;
    env.warp_to_slot(2);
    env.retire_asset(ASSET, 2)
        .map_err(|error| format!("PR 279 retire old asset generation: {error}"))?;
    env.warp_to_slot(3);
    env.activate_permissionless_asset_with_actor_authorities(
        ATTACKER, ASSET, 3, 2_000_000, VICTIM, ATTACKER, ATTACKER, ATTACKER, 1,
    )
    .map_err(|error| format!("PR 279 activate attacker-controlled replacement: {error}"))?;
    let new_market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    if new_market_id == old_market_id {
        return Err(format!(
            "PR 279 replacement reused asset market ID {old_market_id}"
        ));
    }
    let replacement_profile = env.primary_profile(ASSET as usize);
    if replacement_profile.insurance_authority != env.actors[VICTIM].signer.pubkey().to_bytes()
        || replacement_profile.insurance_operator != env.actors[ATTACKER].signer.pubkey().to_bytes()
    {
        return Err(
            "PR 279 replacement authorities did not match the signed replay premise".into(),
        );
    }

    let replay = env
        .land_retained(retained_top_up)
        .map_err(|error| format!("PR 279 stale collateral top-up no longer lands: {error}"))?;
    let victim_loss = victim_source_before
        .checked_sub(env.token_amount(victim_source))
        .ok_or("PR 279 victim source increased")?;
    let replayed_reserve = env.primary_market_state().1.insurance_domain_budget[DOMAIN as usize];
    if victim_loss != AMOUNT as u64 || replayed_reserve != AMOUNT {
        return Err(format!(
            "PR 279 retained top-up did not fund replacement: victim_loss={victim_loss}, \
             reserve={replayed_reserve}"
        ));
    }

    let withdrawal = env
        .withdraw_insurance_asset(ATTACKER, ASSET, AMOUNT)
        .map_err(|error| {
            format!("PR 279 replacement operator could not extract replay: {error}")
        })?;
    let attacker_extraction = env
        .token_amount(attacker_destination)
        .checked_sub(attacker_destination_before)
        .ok_or("PR 279 attacker destination decreased")?;
    let reserve_after = env.primary_market_state().1.insurance_domain_budget[DOMAIN as usize];
    if attacker_extraction != AMOUNT as u64
        || reserve_after != 0
        || replay.compute_units >= TX_CU_LIMIT
        || withdrawal.compute_units >= TX_CU_LIMIT
        || env.token_supply_observed() != supply_before
    {
        return Err(format!(
            "PR 279 public extraction conditions failed: victim_loss={victim_loss}, \
             attacker_extraction={attacker_extraction}, reserve={replayed_reserve}->{reserve_after}, \
             replay_cu={}, withdrawal_cu={}, supply={}/{}",
            replay.compute_units,
            withdrawal.compute_units,
            env.token_supply_observed(),
            supply_before
        ));
    }

    Ok(CollateralTopUpGenerationReplayReproduction {
        blocker: KnownBlocker::CollateralTopUpGenerationReplay,
        old_market_id,
        new_market_id,
        victim_loss,
        attacker_extraction,
        replay_cu: replay.compute_units,
        withdrawal_cu: withdrawal.compute_units,
    })
}

pub fn reproduce_backing_top_up_generation_replay(
    seed: [u8; 32],
) -> Result<BackingTopUpGenerationReplayReproduction, String> {
    const ASSET: u16 = 1;
    const DOMAIN: u16 = ASSET * 2 + 1;
    const WINNER: usize = 0;
    const LOSER: usize = 1;
    const PROVIDER: usize = 2;
    const REPLACEMENT_CREATOR: usize = 3;
    const TOP_UP: u128 = 150;
    const EXPIRY_SLOT: u64 = 8;
    const INITIAL_PRICE: u64 = 100;
    const SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const WINNER_DEPOSIT: u128 = 2_000;
    const LOSER_DEPOSIT: u128 = 250;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: INITIAL_PRICE,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [WINNER_DEPOSIT, LOSER_DEPOSIT, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.configure_permissionless_resolve(20, 5)
        .map_err(|error| format!("PR 321 configure permissionless resolve: {error}"))?;
    env.update_market_init_fee_policy(1)
        .map_err(|error| format!("PR 321 configure permissionless init fee: {error}"))?;
    env.update_asset_authority_from_admin(
        ASSET,
        percolator_prog::processor::ASSET_AUTH_BACKING_BUCKET,
        PROVIDER,
    )
    .map_err(|error| format!("PR 321 install old-generation backing authority: {error}"))?;

    let old_market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    let provider_source = env.actors[PROVIDER].source_token;
    let provider_source_before = env.token_amount(provider_source);
    let retained_top_up =
        env.build_retained_backing_bucket_top_up_for_actor(PROVIDER, DOMAIN, TOP_UP, EXPIRY_SLOT);

    env.warp_to_slot(2);
    env.retire_asset(ASSET, 2)
        .map_err(|error| format!("PR 321 retire old asset generation: {error}"))?;
    env.warp_to_slot(3);
    env.activate_permissionless_asset_with_actor_authorities(
        REPLACEMENT_CREATOR,
        ASSET,
        3,
        INITIAL_PRICE,
        REPLACEMENT_CREATOR,
        REPLACEMENT_CREATOR,
        PROVIDER,
        REPLACEMENT_CREATOR,
        1,
    )
    .map_err(|error| format!("PR 321 activate replacement generation: {error}"))?;
    let new_market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    if new_market_id == old_market_id {
        return Err(format!(
            "PR 321 replacement reused asset market ID {old_market_id}"
        ));
    }
    if env.primary_profile(ASSET as usize).backing_bucket_authority
        != env.actors[PROVIDER].signer.pubkey().to_bytes()
    {
        return Err("PR 321 replacement did not reuse the signed backing authority".into());
    }

    let replay = env
        .land_retained(retained_top_up)
        .map_err(|error| format!("PR 321 stale backing top-up no longer lands: {error}"))?;
    let provider_debit = provider_source_before
        .checked_sub(env.token_amount(provider_source))
        .ok_or("PR 321 provider source increased")?;
    let replayed_bucket = env.primary_market_state().1.source_backing_buckets[DOMAIN as usize];
    if provider_debit != TOP_UP as u64
        || replayed_bucket.fresh_unliened_backing_num != TOP_UP * percolator::BOUND_SCALE
    {
        return Err(format!(
            "PR 321 stale top-up did not fund replacement: provider_debit={provider_debit}, \
             fresh_backing_num={}",
            replayed_bucket.fresh_unliened_backing_num
        ));
    }

    let mut max_cu = replay.compute_units;
    let configure = env
        .configure_auth_mark_for_actor(REPLACEMENT_CREATOR, ASSET, 3, INITIAL_PRICE)
        .map_err(|error| format!("PR 321 configure replacement AuthMark: {error}"))?;
    max_cu = max_cu.max(configure.compute_units);
    let trade = env
        .trade_no_cpi(WINNER, LOSER, ASSET, SIZE_Q, INITIAL_PRICE, 0)
        .map_err(|error| format!("PR 321 open replacement-market positions: {error}"))?;
    max_cu = max_cu.max(trade.compute_units);

    for (slot, mark) in [(4, 105), (5, 110), (6, 115), (7, 120)] {
        env.warp_to_slot(slot);
        let push = env
            .push_auth_mark_for_actor(REPLACEMENT_CREATOR, ASSET, slot, mark)
            .map_err(|error| format!("PR 321 publish mark {mark} at slot {slot}: {error}"))?;
        max_cu = max_cu.max(push.compute_units);
        let observation = vec![CrankObservationHint {
            asset_index: ASSET,
            oracle_accounts: 0,
        }];
        let mut successful_cranks = 0u8;
        for actor in [WINNER, LOSER] {
            for _ in 0..8 {
                match env.crank(actor, slot, observation.clone()) {
                    Ok(crank) => {
                        max_cu = max_cu.max(crank.compute_units);
                        successful_cranks = successful_cranks.saturating_add(1);
                    }
                    Err(_) => break,
                }
                if env.primary_market_state().1.assets[ASSET as usize].effective_price == mark {
                    break;
                }
            }
        }
        if successful_cranks == 0
            || env.primary_market_state().1.assets[ASSET as usize].effective_price != mark
        {
            return Err(format!(
                "PR 321 public crank did not commit mark {mark} at slot {slot}"
            ));
        }
    }

    let winner = env.primary_portfolio(WINNER);
    let loser = env.primary_portfolio(LOSER);
    if winner.pnl.get() != 400 || loser.capital.get() != 0 || loser.pnl.get() >= 0 {
        return Err(format!(
            "PR 321 replacement positions did not consume backing: winner_pnl={}, \
             loser_capital={}, loser_pnl={}",
            winner.pnl.get(),
            loser.capital.get(),
            loser.pnl.get()
        ));
    }

    let resolve = env
        .resolve_stale_permissionless(30)
        .map_err(|error| format!("PR 321 permissionless terminal resolve: {error}"))?;
    max_cu = max_cu.max(resolve.compute_units);
    env.warp_to_slot(36);
    let (first_winner_payout, _) = drain_resolved_actor(&mut env, WINNER)?;
    let (loser_payout, _) = drain_resolved_actor(&mut env, LOSER)?;
    let (winner_top_up, _) = drain_resolved_actor(&mut env, WINNER)?;
    let attacker_payout = first_winner_payout
        .checked_add(winner_top_up)
        .and_then(|payout| payout.checked_add(loser_payout))
        .ok_or("PR 321 attacker payout overflow")?;
    let attacker_profit = attacker_payout
        .checked_sub(WINNER_DEPOSIT + LOSER_DEPOSIT)
        .ok_or("PR 321 attacker coalition did not recover its deposits")?;
    let recoverable = env.primary_market_state().1.source_backing_buckets[DOMAIN as usize]
        .fresh_unliened_backing_num
        / percolator::BOUND_SCALE;
    let provider_loss = TOP_UP
        .checked_sub(recoverable)
        .ok_or("PR 321 recoverable backing exceeds provider top-up")?;
    if provider_loss == 0
        || attacker_profit != provider_loss
        || provider_debit != TOP_UP as u64
        || max_cu >= TX_CU_LIMIT
        || env.token_supply_observed() != supply_before
    {
        return Err(format!(
            "PR 321 terminal extraction mismatch: provider_debit={provider_debit}, \
             provider_loss={provider_loss}, attacker_payout={attacker_payout}, \
             attacker_profit={attacker_profit}, max_cu={max_cu}, supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }

    Ok(BackingTopUpGenerationReplayReproduction {
        blocker: KnownBlocker::BackingTopUpGenerationReplay,
        old_market_id,
        new_market_id,
        provider_loss: u64::try_from(provider_loss)
            .map_err(|_| "PR 321 provider loss exceeds SPL range")?,
        attacker_profit,
        attacker_payout,
        replay_cu: replay.compute_units,
        max_cu,
    })
}

pub fn reproduce_insurance_withdrawal_generation_replay(
    seed: [u8; 32],
) -> Result<InsuranceWithdrawalGenerationReplayReproduction, String> {
    const ASSET: u16 = 1;
    const DOMAIN: u16 = ASSET * 2;
    const OPERATOR: usize = 0;
    const REPLACEMENT_CREATOR: usize = 1;
    const PROVIDER: usize = 2;
    const AMOUNT: u128 = 50_000;

    let mut env = V16Svm::new(seed, MarketConfig::default());
    let supply_before = env.token_supply_observed();

    env.update_asset_authority_from_admin(
        ASSET,
        percolator_prog::processor::ASSET_AUTH_INSURANCE_OPERATOR,
        OPERATOR,
    )
    .map_err(|error| format!("PR 328 install reusable insurance operator: {error}"))?;
    env.update_asset_authority_from_admin(
        ASSET,
        percolator_prog::processor::ASSET_AUTH_INSURANCE,
        PROVIDER,
    )
    .map_err(|error| format!("PR 328 install independent insurance provider: {error}"))?;
    env.top_up_insurance_domain_for_actor(PROVIDER, DOMAIN, AMOUNT)
        .map_err(|error| format!("PR 328 fund old asset generation: {error}"))?;
    let old_market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    let retained_withdrawal =
        env.build_retained_insurance_withdrawal_for_actor(OPERATOR, ASSET, AMOUNT);

    env.withdraw_insurance_asset(OPERATOR, ASSET, AMOUNT)
        .map_err(|error| format!("PR 328 clear old-generation reserve: {error}"))?;
    if env.primary_market_state().1.insurance_domain_budget[DOMAIN as usize] != 0 {
        return Err("PR 328 old-generation reserve did not clear before retirement".into());
    }
    env.update_market_init_fee_policy(1)
        .map_err(|error| format!("PR 328 configure permissionless init fee: {error}"))?;
    env.warp_to_slot(2);
    env.retire_asset(ASSET, 2)
        .map_err(|error| format!("PR 328 retire old asset generation: {error}"))?;
    env.warp_to_slot(3);
    env.activate_permissionless_asset_with_actor_authorities(
        REPLACEMENT_CREATOR,
        ASSET,
        3,
        2_000_000,
        PROVIDER,
        OPERATOR,
        REPLACEMENT_CREATOR,
        REPLACEMENT_CREATOR,
        1,
    )
    .map_err(|error| format!("PR 328 activate replacement generation: {error}"))?;
    let new_market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    if new_market_id == old_market_id {
        return Err(format!(
            "PR 328 replacement reused asset market ID {old_market_id}"
        ));
    }

    let provider_source = env.actors[PROVIDER].source_token;
    let provider_source_before = env.token_amount(provider_source);
    env.top_up_insurance_domain_for_actor(PROVIDER, DOMAIN, AMOUNT)
        .map_err(|error| format!("PR 328 fund replacement reserve: {error}"))?;
    let replacement_provider_loss = provider_source_before
        .checked_sub(env.token_amount(provider_source))
        .ok_or("PR 328 replacement provider source increased")?;
    let attacker_destination = env.actors[OPERATOR].destination_token;
    let attacker_destination_before = env.token_amount(attacker_destination);
    let replacement_reserve = env.primary_market_state().1.insurance_domain_budget[DOMAIN as usize];
    if replacement_provider_loss != AMOUNT as u64 || replacement_reserve != AMOUNT {
        return Err(format!(
            "PR 328 replacement funding mismatch: provider_loss={replacement_provider_loss}, \
             reserve={replacement_reserve}"
        ));
    }

    let replay = env
        .land_retained(retained_withdrawal)
        .map_err(|error| format!("PR 328 stale withdrawal no longer lands: {error}"))?;
    let attacker_extraction = env
        .token_amount(attacker_destination)
        .checked_sub(attacker_destination_before)
        .ok_or("PR 328 attacker destination decreased")?;
    let reserve_after = env.primary_market_state().1.insurance_domain_budget[DOMAIN as usize];
    if attacker_extraction != AMOUNT as u64
        || reserve_after != 0
        || replay.compute_units >= TX_CU_LIMIT
        || env.token_supply_observed() != supply_before
    {
        return Err(format!(
            "PR 328 public extraction conditions failed: replacement_provider_loss={replacement_provider_loss}, \
             attacker_extraction={attacker_extraction}, reserve={replacement_reserve}->{reserve_after}, \
             replay_cu={}, supply={}/{}",
            replay.compute_units,
            env.token_supply_observed(),
            supply_before
        ));
    }

    Ok(InsuranceWithdrawalGenerationReplayReproduction {
        blocker: KnownBlocker::InsuranceWithdrawalGenerationReplay,
        old_market_id,
        new_market_id,
        replacement_provider_loss,
        attacker_extraction,
        replay_cu: replay.compute_units,
    })
}

pub fn reproduce_insurance_top_up_retry_replay(
    seed: [u8; 32],
) -> Result<InsuranceTopUpRetryReplayReproduction, String> {
    const ASSET: u16 = 0;
    const DOMAIN: u16 = 0;
    const AUTHORITY: usize = 0;
    const OPERATOR: usize = 1;
    const AMOUNT: u128 = 50_000;

    let mut env = V16Svm::new(seed, MarketConfig::default());
    let supply_before = env.token_supply_observed();
    env.update_asset_authority_from_admin(
        ASSET,
        percolator_prog::processor::ASSET_AUTH_INSURANCE,
        AUTHORITY,
    )
    .map_err(|error| format!("PR 344 install insurance authority: {error}"))?;
    env.update_asset_authority_from_admin(
        ASSET,
        percolator_prog::processor::ASSET_AUTH_INSURANCE_OPERATOR,
        OPERATOR,
    )
    .map_err(|error| format!("PR 344 install distinct insurance operator: {error}"))?;

    let source = env.actors[AUTHORITY].source_token;
    let destination = env.actors[OPERATOR].destination_token;
    let source_before = env.token_amount(source);
    let destination_before = env.token_amount(destination);
    let intended = env.build_retained_insurance_domain_top_up_for_actor(AUTHORITY, DOMAIN, AMOUNT);
    let retry_variant =
        env.build_retained_insurance_domain_top_up_for_actor(AUTHORITY, DOMAIN, AMOUNT);

    let first = env
        .land_retained(intended)
        .map_err(|error| format!("PR 344 intended top-up rejected: {error}"))?;
    let intended_reserve = env.primary_market_state().1.insurance_domain_budget[DOMAIN as usize];
    if intended_reserve != AMOUNT
        || source_before
            .checked_sub(env.token_amount(source))
            .ok_or("PR 344 source increased after first top-up")?
            != AMOUNT as u64
    {
        return Err(format!(
            "PR 344 intended contribution mismatch: reserve={intended_reserve}, source={source_before}->{}",
            env.token_amount(source)
        ));
    }

    let replay = env
        .land_retained(retry_variant)
        .map_err(|error| format!("PR 344 retry variant no longer lands: {error}"))?;
    let total_debit = source_before
        .checked_sub(env.token_amount(source))
        .ok_or("PR 344 source increased after retry")?;
    let duplicate_loss = total_debit
        .checked_sub(AMOUNT as u64)
        .ok_or("PR 344 total debit below intended contribution")?;
    let doubled_reserve = env.primary_market_state().1.insurance_domain_budget[DOMAIN as usize];
    if duplicate_loss != AMOUNT as u64 || doubled_reserve != AMOUNT * 2 {
        return Err(format!(
            "PR 344 retry did not duplicate contribution: duplicate={duplicate_loss}, \
             reserve={doubled_reserve}"
        ));
    }

    let withdrawal = env
        .withdraw_insurance_asset(OPERATOR, ASSET, AMOUNT)
        .map_err(|error| format!("PR 344 operator could not extract duplicate: {error}"))?;
    let operator_extraction = env
        .token_amount(destination)
        .checked_sub(destination_before)
        .ok_or("PR 344 operator destination decreased")?;
    let insured_remainder = env.primary_market_state().1.insurance_domain_budget[DOMAIN as usize];
    let max_cu = first
        .compute_units
        .max(replay.compute_units)
        .max(withdrawal.compute_units);
    if operator_extraction != AMOUNT as u64
        || insured_remainder != AMOUNT
        || max_cu >= TX_CU_LIMIT
        || env.token_supply_observed() != supply_before
    {
        return Err(format!(
            "PR 344 public extraction conditions failed: duplicate={duplicate_loss}, \
             operator={operator_extraction}, insured={insured_remainder}, max_cu={max_cu}, \
             supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }

    Ok(InsuranceTopUpRetryReplayReproduction {
        blocker: KnownBlocker::InsuranceTopUpRetryReplay,
        intended_contribution: AMOUNT as u64,
        duplicate_loss,
        operator_extraction,
        insured_remainder,
        first_cu: first.compute_units,
        replay_cu: replay.compute_units,
    })
}

pub fn reproduce_activation_retry_replay(
    seed: [u8; 32],
) -> Result<ActivationRetryReplayReproduction, String> {
    const ASSET: u16 = 1;
    const CREATOR: usize = 0;
    const BENEFICIARY: usize = 1;
    const FEE: u128 = 500;

    let mut env = V16Svm::new(seed, MarketConfig::default());
    let supply_before = env.token_supply_observed();
    env.update_market_init_fee_policy(FEE)
        .map_err(|error| format!("PR 362 configure permissionless init fee: {error}"))?;
    env.update_asset_authority_from_admin(
        0,
        percolator_prog::processor::ASSET_AUTH_INSURANCE_OPERATOR,
        BENEFICIARY,
    )
    .map_err(|error| format!("PR 362 install independent fee beneficiary: {error}"))?;

    env.warp_to_slot(2);
    env.retire_asset(ASSET, 2)
        .map_err(|error| format!("PR 362 retire reusable slot: {error}"))?;
    env.warp_to_slot(3);
    let creator_source = env.actors[CREATOR].source_token;
    let source_before = env.token_amount(creator_source);
    let intended = env.build_retained_permissionless_asset_activation(
        CREATOR, ASSET, 3, 100, FEE, CREATOR, CREATOR, CREATOR, CREATOR,
    );
    let retry_variant = env.build_retained_permissionless_asset_activation(
        CREATOR, ASSET, 3, 100, FEE, CREATOR, CREATOR, CREATOR, CREATOR,
    );

    let first = env
        .land_retained(intended)
        .map_err(|error| format!("PR 362 intended activation rejected: {error}"))?;
    let first_market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    let first_debit = source_before
        .checked_sub(env.token_amount(creator_source))
        .ok_or("PR 362 creator source increased after intended activation")?;
    let first_insurance = env.primary_market_state().1.insurance_domain_budget[0]
        .checked_add(env.primary_market_state().1.insurance_domain_budget[1])
        .ok_or("PR 362 first insurance total overflow")?;
    if first_debit != FEE as u64 || first_insurance != FEE {
        return Err(format!(
            "PR 362 intended activation accounting mismatch: debit={first_debit}, \
             insurance={first_insurance}"
        ));
    }

    env.warp_to_slot(4);
    env.retire_asset(ASSET, 4)
        .map_err(|error| format!("PR 362 retire first activated generation: {error}"))?;
    env.warp_to_slot(5);
    let replay = env
        .land_retained(retry_variant)
        .map_err(|error| format!("PR 362 retained activation no longer lands: {error}"))?;
    let replay_market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    let total_debit = source_before
        .checked_sub(env.token_amount(creator_source))
        .ok_or("PR 362 creator source increased after retry")?;
    let duplicate_loss = total_debit
        .checked_sub(FEE as u64)
        .ok_or("PR 362 total debit below intended fee")?;
    let doubled_insurance = env.primary_market_state().1.insurance_domain_budget[0]
        .checked_add(env.primary_market_state().1.insurance_domain_budget[1])
        .ok_or("PR 362 doubled insurance total overflow")?;
    if replay_market_id == first_market_id
        || duplicate_loss != FEE as u64
        || doubled_insurance != FEE * 2
    {
        return Err(format!(
            "PR 362 replay did not create and fund a new generation: ids={first_market_id}/\
             {replay_market_id}, duplicate={duplicate_loss}, insurance={doubled_insurance}"
        ));
    }

    let beneficiary_destination = env.actors[BENEFICIARY].destination_token;
    let destination_before = env.token_amount(beneficiary_destination);
    let withdrawal = env
        .withdraw_insurance_asset(BENEFICIARY, 0, FEE)
        .map_err(|error| format!("PR 362 beneficiary could not extract duplicate fee: {error}"))?;
    let beneficiary_extraction = env
        .token_amount(beneficiary_destination)
        .checked_sub(destination_before)
        .ok_or("PR 362 beneficiary destination decreased")?;
    let insured_remainder = env.primary_market_state().1.insurance_domain_budget[0]
        .checked_add(env.primary_market_state().1.insurance_domain_budget[1])
        .ok_or("PR 362 remaining insurance total overflow")?;
    let max_cu = first
        .compute_units
        .max(replay.compute_units)
        .max(withdrawal.compute_units);
    if beneficiary_extraction != FEE as u64
        || insured_remainder != FEE
        || max_cu >= TX_CU_LIMIT
        || env.token_supply_observed() != supply_before
    {
        return Err(format!(
            "PR 362 public extraction mismatch: duplicate={duplicate_loss}, \
             beneficiary={beneficiary_extraction}, insured={insured_remainder}, max_cu={max_cu}, \
             supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }

    Ok(ActivationRetryReplayReproduction {
        blocker: KnownBlocker::ActivationRetryReplay,
        first_market_id,
        replay_market_id,
        intended_fee: FEE as u64,
        duplicate_loss,
        beneficiary_extraction,
        insured_remainder,
        replay_cu: replay.compute_units,
    })
}

pub fn verify_activation_fee_consent(
    seed: [u8; 32],
) -> Result<ActivationFeeConsentProtection, String> {
    const ASSET: u16 = 1;
    const CREATOR: usize = 0;
    const BENEFICIARY: usize = 1;
    const SIGNED_MAX_FEE: u128 = 1;
    const INSTALLED_UNAUTHORIZED_FEE: u128 = 1_000;
    const CONSENTED_MAX_FEE: u128 = SIGNED_MAX_FEE;

    let mut env = V16Svm::new(seed, MarketConfig::default());
    let supply_before = env.token_supply_observed();
    env.update_asset_authority_from_admin(
        0,
        percolator_prog::processor::ASSET_AUTH_INSURANCE_OPERATOR,
        BENEFICIARY,
    )
    .map_err(|error| format!("PR 314 install independent fee beneficiary: {error}"))?;

    env.warp_to_slot(2);
    env.retire_asset(ASSET, 2)
        .map_err(|error| format!("PR 314 retire reusable slot: {error}"))?;
    let delayed_policy = env.build_retained_market_init_fee_policy(INSTALLED_UNAUTHORIZED_FEE);
    env.update_market_init_fee_policy(SIGNED_MAX_FEE)
        .map_err(|error| format!("PR 314 publish creator-visible fee: {error}"))?;
    let visible_fee = env.primary_market_state().0.permissionless_market_init_fee;
    if visible_fee != SIGNED_MAX_FEE {
        return Err(format!(
            "PR 314 creator did not observe the advertised fee: {visible_fee}"
        ));
    }

    env.warp_to_slot(3);
    let creator_source = env.actors[CREATOR].source_token;
    let source_before = env.token_amount(creator_source);
    let activation = env.build_retained_permissionless_asset_activation(
        CREATOR,
        ASSET,
        3,
        100,
        SIGNED_MAX_FEE,
        CREATOR,
        CREATOR,
        CREATOR,
        CREATOR,
    );
    let insurance_before_rejection = env.primary_market_state().1.insurance_domain_budget[0]
        .checked_add(env.primary_market_state().1.insurance_domain_budget[1])
        .ok_or("PR 314 pre-rejection insurance total overflow")?;
    let state_before_policy = tracked_economic_accounts(&env);
    let stale_error = match env.land_retained(delayed_policy) {
        Ok(_) => return Err("PR 314 delayed high-fee policy unexpectedly landed".into()),
        Err(error) => error,
    };
    let stale_policy_rejected =
        stale_error.contains("Custom(19)") || stale_error.contains("custom program error: 0x13");
    let rejected_exact_rollback = tracked_economic_accounts(&env) == state_before_policy;
    let unconsented_creator_loss = source_before
        .checked_sub(env.token_amount(creator_source))
        .ok_or("PR 314 rejected activation increased creator source")?;
    let insurance_after_rejection = env.primary_market_state().1.insurance_domain_budget[0]
        .checked_add(env.primary_market_state().1.insurance_domain_budget[1])
        .ok_or("PR 314 post-rejection insurance total overflow")?;
    let unconsented_insurance_delta = insurance_after_rejection
        .checked_sub(insurance_before_rejection)
        .ok_or("PR 314 rejected activation decreased insurance")?;
    if !stale_policy_rejected
        || !rejected_exact_rollback
        || unconsented_creator_loss != 0
        || unconsented_insurance_delta != 0
        || env.primary_market_state().0.permissionless_market_init_fee != SIGNED_MAX_FEE
        || env.primary_market_state().1.assets[ASSET as usize].lifecycle
            == AssetLifecycleV16::Active
    {
        return Err(format!(
            "PR 314 stale policy protection failed: error={stale_error}, \
             rollback={rejected_exact_rollback}, creator_loss={unconsented_creator_loss}, \
             insurance_delta={unconsented_insurance_delta}"
        ));
    }

    let source_before_consent = env.token_amount(creator_source);
    let insurance_before_consent = env.primary_market_state().1.insurance_domain_budget[0]
        .checked_add(env.primary_market_state().1.insurance_domain_budget[1])
        .ok_or("PR 314 pre-consent insurance total overflow")?;
    let consented_activation = env
        .land_retained(activation)
        .map_err(|error| format!("PR 314 activation within signed cap rejected: {error}"))?;
    let charged_fee = source_before_consent
        .checked_sub(env.token_amount(creator_source))
        .ok_or("PR 314 consented activation increased creator source")?;
    let insurance_after_consent = env.primary_market_state().1.insurance_domain_budget[0]
        .checked_add(env.primary_market_state().1.insurance_domain_budget[1])
        .ok_or("PR 314 post-consent insurance total overflow")?;
    let insured_fee = insurance_after_consent
        .checked_sub(insurance_before_consent)
        .ok_or("PR 314 consented activation decreased insurance")?;
    let asset_active =
        env.primary_market_state().1.assets[ASSET as usize].lifecycle == AssetLifecycleV16::Active;
    let token_supply_conserved = env.token_supply_observed() == supply_before;
    if charged_fee != SIGNED_MAX_FEE as u64
        || insured_fee != SIGNED_MAX_FEE
        || !asset_active
        || consented_activation.compute_units >= TX_CU_LIMIT
        || !token_supply_conserved
    {
        return Err(format!(
            "PR 314 consented activation mismatch: charged={charged_fee}, insured={insured_fee}, \
             active={asset_active}, activation_cu={}, supply={}/{}",
            consented_activation.compute_units,
            env.token_supply_observed(),
            supply_before
        ));
    }

    Ok(ActivationFeeConsentProtection {
        blocker: KnownBlocker::ActivationFeeConsent,
        signed_max_fee: SIGNED_MAX_FEE as u64,
        installed_unauthorized_fee: INSTALLED_UNAUTHORIZED_FEE as u64,
        stale_policy_rejected,
        rejected_exact_rollback,
        unconsented_creator_loss,
        unconsented_insurance_delta,
        consented_max_fee: CONSENTED_MAX_FEE as u64,
        current_fee: SIGNED_MAX_FEE as u64,
        charged_fee,
        insured_fee,
        asset_active,
        activation_cu: consented_activation.compute_units,
        token_supply_conserved,
    })
}

pub fn verify_bilateral_base_fee_consent(
    seed: [u8; 32],
    route: TradeRoute,
) -> Result<BilateralBaseFeeConsentProtection, String> {
    if !matches!(route, TradeRoute::NoCpi | TradeRoute::BatchNoCpi) {
        return Err(format!(
            "PR 310 requires a bilateral no-CPI route: {route:?}"
        ));
    }

    const ASSET: u16 = 0;
    const BENEFICIARY: usize = 0;
    const VICTIM: usize = 1;
    const DEPOSIT: u128 = 100_000_000;
    const PRICE: u64 = 1_000_000;
    const SIZE_Q: i128 = POS_SCALE as i128;
    const SIGNED_FEE_BPS: u64 = 0;
    const INSTALLED_FEE_BPS: u64 = 500;
    const FEE_PER_SIDE_PER_TRADE: u64 = 50_000;
    const TOTAL_INSURANCE_FEE: u128 = 4 * FEE_PER_SIDE_PER_TRADE as u128;

    let mut env = V16Svm::new(seed, MarketConfig::default());
    let supply_before = env.token_supply_observed();
    env.update_asset_authority_from_admin(
        ASSET,
        percolator_prog::processor::ASSET_AUTH_INSURANCE_OPERATOR,
        BENEFICIARY,
    )
    .map_err(|error| format!("PR 310 install independent fee beneficiary: {error}"))?;

    let build_trade = |env: &mut V16Svm, size_q: i128, fee_bps: u64| match route {
        TradeRoute::NoCpi => env.build_retained_no_cpi_trade_with_fee(
            BENEFICIARY,
            VICTIM,
            ASSET,
            size_q,
            PRICE,
            fee_bps,
        ),
        TradeRoute::BatchNoCpi => env.build_retained_batch_no_cpi_trade_with_fee(
            BENEFICIARY,
            VICTIM,
            ASSET,
            size_q,
            PRICE,
            fee_bps,
        ),
        TradeRoute::Cpi | TradeRoute::BatchCpi => unreachable!(),
    };
    let retained_open = build_trade(&mut env, SIZE_Q, SIGNED_FEE_BPS);
    let retained_close = build_trade(&mut env, -SIZE_Q, SIGNED_FEE_BPS);

    env.update_trade_fee_policy(INSTALLED_FEE_BPS)
        .map_err(|error| format!("PR 310 raise live base fee after signing: {error}"))?;
    let state_after_policy = tracked_economic_accounts(&env);
    let victim_before_rejections = env.primary_portfolio(VICTIM).capital.get();
    let insurance_before_rejections = env.primary_market_state().1.insurance_domain_budget[0]
        .checked_add(env.primary_market_state().1.insurance_domain_budget[1])
        .ok_or("PR 310 pre-rejection insurance total overflow")?;
    let stale_open_error = match env.land_retained(retained_open) {
        Ok(_) => return Err("PR 310 stale-fee retained open unexpectedly landed".into()),
        Err(error) => error,
    };
    let stale_open_rejected = stale_open_error.contains("Custom(9)")
        || stale_open_error.contains("custom program error: 0x9");
    let rollback_after_open = tracked_economic_accounts(&env) == state_after_policy;
    let stale_close_error = match env.land_retained(retained_close) {
        Ok(_) => return Err("PR 310 stale-fee retained close unexpectedly landed".into()),
        Err(error) => error,
    };
    let stale_close_rejected = stale_close_error.contains("Custom(9)")
        || stale_close_error.contains("custom program error: 0x9");
    let rejected_exact_rollback =
        rollback_after_open && tracked_economic_accounts(&env) == state_after_policy;
    let unconsented_victim_loss = u64::try_from(
        victim_before_rejections
            .checked_sub(env.primary_portfolio(VICTIM).capital.get())
            .ok_or("PR 310 rejected retained trades increased victim capital")?,
    )
    .map_err(|_| "PR 310 unconsented victim loss does not fit u64")?;
    let insurance_after_rejections = env.primary_market_state().1.insurance_domain_budget[0]
        .checked_add(env.primary_market_state().1.insurance_domain_budget[1])
        .ok_or("PR 310 post-rejection insurance total overflow")?;
    let unconsented_insurance_delta = insurance_after_rejections
        .checked_sub(insurance_before_rejections)
        .ok_or("PR 310 rejected retained trades decreased insurance")?;
    if !stale_open_rejected
        || !stale_close_rejected
        || !rejected_exact_rollback
        || unconsented_victim_loss != 0
        || unconsented_insurance_delta != 0
    {
        return Err(format!(
            "PR 310 stale-fee rejection failed: open={stale_open_error}, close={stale_close_error}, \
             rollback={rejected_exact_rollback}, victim_loss={unconsented_victim_loss}, \
             insurance_delta={unconsented_insurance_delta}"
        ));
    }

    let freshly_consented_open = build_trade(&mut env, SIZE_Q, INSTALLED_FEE_BPS);
    let freshly_consented_close = build_trade(&mut env, -SIZE_Q, INSTALLED_FEE_BPS);
    let open = env
        .land_retained(freshly_consented_open)
        .map_err(|error| format!("PR 310 freshly consented open failed: {error}"))?;
    let close = env
        .land_retained(freshly_consented_close)
        .map_err(|error| format!("PR 310 freshly consented close failed: {error}"))?;

    let beneficiary_capital = env.primary_portfolio(BENEFICIARY).capital.get();
    let victim_capital = env.primary_portfolio(VICTIM).capital.get();
    let insurance = env.primary_market_state().1.insurance_domain_budget[0]
        .checked_add(env.primary_market_state().1.insurance_domain_budget[1])
        .ok_or("PR 310 insurance total overflow")?;
    let expected_capital = DEPOSIT
        .checked_sub(2 * FEE_PER_SIDE_PER_TRADE as u128)
        .ok_or("PR 310 expected capital underflow")?;
    if beneficiary_capital != expected_capital
        || victim_capital != expected_capital
        || insurance != TOTAL_INSURANCE_FEE
    {
        return Err(format!(
            "PR 310 retained trades did not charge the live fee: capital={beneficiary_capital}/\
             {victim_capital}, insurance={insurance}"
        ));
    }

    let beneficiary_destination = env.actors[BENEFICIARY].destination_token;
    let victim_destination = env.actors[VICTIM].destination_token;
    let insurance_withdrawal = env
        .withdraw_insurance_asset(BENEFICIARY, ASSET, TOTAL_INSURANCE_FEE)
        .map_err(|error| format!("PR 310 beneficiary could not extract insurance: {error}"))?;
    let beneficiary_withdrawal = env
        .withdraw_primary(BENEFICIARY, beneficiary_capital)
        .map_err(|error| format!("PR 310 beneficiary could not exit: {error}"))?;
    let victim_withdrawal = env
        .withdraw_primary(VICTIM, victim_capital)
        .map_err(|error| format!("PR 310 victim could not exit: {error}"))?;

    let beneficiary_payout = env.token_amount(beneficiary_destination);
    let victim_payout = env.token_amount(victim_destination);
    let consented_victim_fee = (DEPOSIT as u64)
        .checked_sub(victim_payout)
        .ok_or("PR 310 victim payout exceeded its deposit")?;
    let total_payout = u128::from(beneficiary_payout) + u128::from(victim_payout);
    let max_cu = open
        .compute_units
        .max(close.compute_units)
        .max(insurance_withdrawal.compute_units)
        .max(beneficiary_withdrawal.compute_units)
        .max(victim_withdrawal.compute_units);
    let token_supply_conserved = env.token_supply_observed() == supply_before;
    if consented_victim_fee != 100_000
        || beneficiary_payout != 100_100_000
        || victim_payout != 99_900_000
        || total_payout != 2 * DEPOSIT
        || env.primary_market_state().1.insurance_domain_budget[0]
            + env.primary_market_state().1.insurance_domain_budget[1]
            != 0
        || max_cu >= TX_CU_LIMIT
        || !token_supply_conserved
    {
        return Err(format!(
            "PR 310 terminal extraction mismatch: payouts={beneficiary_payout}/{victim_payout}, \
             consented_victim_fee={consented_victim_fee}, total={total_payout}, max_cu={max_cu}, \
             supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }

    Ok(BilateralBaseFeeConsentProtection {
        blocker: KnownBlocker::BilateralBaseFeeConsent,
        route,
        signed_fee_bps: SIGNED_FEE_BPS,
        installed_fee_bps: INSTALLED_FEE_BPS,
        stale_open_rejected,
        stale_close_rejected,
        rejected_exact_rollback,
        unconsented_victim_loss,
        unconsented_insurance_delta,
        consented_victim_fee,
        consented_insurance_fee: TOTAL_INSURANCE_FEE as u64,
        total_payout,
        open_cu: open.compute_units,
        close_cu: close.compute_units,
        token_supply_conserved,
    })
}

fn publicly_recreate_primary_market(
    env: &mut V16Svm,
    config: MarketConfig,
    reinit_slot: u64,
    context: &str,
) -> Result<(), String> {
    for actor in 0..PRIMARY_ACTOR_COUNT {
        env.withdraw_primary(actor, 1)
            .map_err(|error| format!("{context} empty generation-A portfolio {actor}: {error}"))?;
        env.close_primary_portfolio(actor)
            .map_err(|error| format!("{context} close generation-A portfolio {actor}: {error}"))?;
    }
    env.resolve_market()
        .map_err(|error| format!("{context} resolve generation A: {error}"))?;
    env.close_primary_slab()
        .map_err(|error| format!("{context} close generation-A slab: {error}"))?;

    env.warp_to_slot(reinit_slot);
    env.fund_closed_primary_market()
        .map_err(|error| format!("{context} re-fund closed market: {error}"))?;
    env.recreate_primary_vault()
        .map_err(|error| format!("{context} recreate canonical vault: {error}"))?;
    env.reinitialize_primary_market(config)
        .map_err(|error| format!("{context} initialize generation B: {error}"))?;
    Ok(())
}

pub fn reproduce_maintenance_policy_generation_replay(
    seed: [u8; 32],
) -> Result<MaintenancePolicyGenerationReplayReproduction, String> {
    const TRADER_LONG: usize = 0;
    const TRADER_SHORT: usize = 1;
    const FEE_PAYER: usize = 2;
    const ATTACKER: usize = 3;
    const PRICE: u64 = 100;
    const TRADE_SIZE_Q: i128 = POS_SCALE as i128;
    const DEPOSIT: u128 = 100_000;
    const MAINTENANCE_FEE_PER_SLOT: u128 = 58;
    const CRANKER_SHARE_BPS: u16 = 10_000;
    const REINIT_SLOT: u64 = 10;
    const SYNC_SLOT: u64 = 20;
    const EXPECTED_FEE: u64 = 580;

    let config = MarketConfig {
        initial_price: PRICE,
        maintenance_fee_per_slot: MAINTENANCE_FEE_PER_SLOT,
        actor_deposits: [1, 1, 1, 1, 1],
        ..MarketConfig::default()
    };
    let mut env = V16Svm::new(seed, config);
    let supply_before = env.token_supply_observed();
    let old_asset_market_id = env.primary_market_state().1.assets[0].market_id;
    let retained_policy = env.build_retained_maintenance_fee_policy(CRANKER_SHARE_BPS);

    publicly_recreate_primary_market(&mut env, config, REINIT_SLOT, "PR 325")?;
    env.configure_auth_mark(false, 0, REINIT_SLOT, PRICE)
        .map_err(|error| format!("PR 325 configure generation-B AuthMark: {error}"))?;
    let new_asset_market_id = env.primary_market_state().1.assets[0].market_id;
    if env
        .primary_market_state()
        .0
        .maintenance_cranker_fee_share_bps
        != 0
    {
        return Err("PR 325 generation B did not start with zero cranker share".into());
    }

    for actor in [TRADER_LONG, TRADER_SHORT, FEE_PAYER, ATTACKER] {
        env.fund_closed_primary_portfolio(actor, 1_000_000_000)
            .map_err(|error| format!("PR 325 re-fund portfolio {actor}: {error}"))?;
        env.reinitialize_primary_portfolio(actor)
            .map_err(|error| format!("PR 325 initialize portfolio {actor}: {error}"))?;
    }
    for actor in [TRADER_LONG, TRADER_SHORT, FEE_PAYER] {
        env.deposit_primary(actor, DEPOSIT)
            .map_err(|error| format!("PR 325 deposit actor {actor}: {error}"))?;
    }
    env.trade_no_cpi(TRADER_LONG, TRADER_SHORT, 0, TRADE_SIZE_Q, PRICE, 0)
        .map_err(|error| format!("PR 325 establish generation-B live OI: {error}"))?;
    let live_oi_q = env.primary_market_state().1.assets[0].oi_eff_long_q;
    if live_oi_q == 0 {
        return Err("PR 325 generation-B market has no live OI".into());
    }

    let replay = env
        .land_retained(retained_policy)
        .map_err(|error| format!("PR 325 stale maintenance policy no longer lands: {error}"))?;
    if env
        .primary_market_state()
        .0
        .maintenance_cranker_fee_share_bps
        != CRANKER_SHARE_BPS
    {
        return Err("PR 325 stale policy did not install the 100% cranker split".into());
    }

    env.warp_to_slot(SYNC_SLOT);
    let payer_before = env.primary_portfolio(FEE_PAYER).capital.get();
    let attacker_before = env.primary_portfolio(ATTACKER).capital.get();
    let sync = env
        .sync_maintenance_fee_with_reward(FEE_PAYER, ATTACKER, SYNC_SLOT)
        .map_err(|error| format!("PR 325 sync victim maintenance fee: {error}"))?;
    let victim_loss = payer_before
        .checked_sub(env.primary_portfolio(FEE_PAYER).capital.get())
        .ok_or("PR 325 fee payer capital increased")?;
    let attacker_reward = env
        .primary_portfolio(ATTACKER)
        .capital
        .get()
        .checked_sub(attacker_before)
        .ok_or("PR 325 attacker capital decreased")?;
    if victim_loss != EXPECTED_FEE as u128
        || attacker_reward != victim_loss
        || env.primary_market_state().1.insurance != 0
    {
        return Err(format!(
            "PR 325 stale split did not transfer the user fee: victim={victim_loss}, \
             attacker={attacker_reward}, insurance={}",
            env.primary_market_state().1.insurance
        ));
    }

    let destination = env.actors[ATTACKER].destination_token;
    let destination_before = env.token_amount(destination);
    let withdrawal = env
        .withdraw_primary(ATTACKER, attacker_reward)
        .map_err(|error| format!("PR 325 attacker could not withdraw reward: {error}"))?;
    let attacker_extraction = env
        .token_amount(destination)
        .checked_sub(destination_before)
        .ok_or("PR 325 attacker destination decreased")?;
    let max_cu = replay
        .compute_units
        .max(sync.compute_units)
        .max(withdrawal.compute_units);
    if attacker_extraction != EXPECTED_FEE
        || u128::from(attacker_extraction) != victim_loss
        || max_cu >= TX_CU_LIMIT
        || env.token_supply_observed() != supply_before
    {
        return Err(format!(
            "PR 325 public extraction mismatch: victim={victim_loss}, \
             extraction={attacker_extraction}, max_cu={max_cu}, supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }

    Ok(MaintenancePolicyGenerationReplayReproduction {
        blocker: KnownBlocker::MaintenancePolicyGenerationReplay,
        old_asset_market_id,
        new_asset_market_id,
        victim_loss: victim_loss as u64,
        attacker_extraction,
        live_oi_q,
        replay_cu: replay.compute_units,
        sync_cu: sync.compute_units,
    })
}

pub fn reproduce_delayed_maintenance_policy_replay(
    seed: [u8; 32],
) -> Result<DelayedMaintenancePolicyReplayReproduction, String> {
    const TRADER_LONG: usize = 0;
    const TRADER_SHORT: usize = 1;
    const FEE_PAYER: usize = 2;
    const ATTACKER: usize = 3;
    const PRICE: u64 = 100;
    const TRADE_SIZE_Q: i128 = POS_SCALE as i128;
    const DEPOSIT: u128 = 100_000;
    const MAINTENANCE_FEE_PER_SLOT: u128 = 58;
    const STALE_CRANKER_SHARE_BPS: u16 = 10_000;
    const CURRENT_CRANKER_SHARE_BPS: u16 = 0;
    const SYNC_SLOT: u64 = 11;
    const EXPECTED_FEE: u64 = 580;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            maintenance_fee_per_slot: MAINTENANCE_FEE_PER_SLOT,
            actor_deposits: [DEPOSIT, DEPOSIT, DEPOSIT, 1, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    let retained_policy = env.build_retained_maintenance_fee_policy(STALE_CRANKER_SHARE_BPS);
    let correction = env
        .update_maintenance_fee_policy(CURRENT_CRANKER_SHARE_BPS)
        .map_err(|error| format!("PR 337 land current zero-share correction: {error}"))?;
    if env
        .primary_market_state()
        .0
        .maintenance_cranker_fee_share_bps
        != CURRENT_CRANKER_SHARE_BPS
    {
        return Err("PR 337 current maintenance policy did not land".into());
    }

    env.trade_no_cpi(TRADER_LONG, TRADER_SHORT, 0, TRADE_SIZE_Q, PRICE, 0)
        .map_err(|error| format!("PR 337 establish independent live OI: {error}"))?;
    let live_oi_q = env.primary_market_state().1.assets[0].oi_eff_long_q;
    if live_oi_q == 0 {
        return Err("PR 337 market has no live OI".into());
    }
    let replay = env
        .land_retained(retained_policy)
        .map_err(|error| format!("PR 337 delayed maintenance policy no longer lands: {error}"))?;
    if env
        .primary_market_state()
        .0
        .maintenance_cranker_fee_share_bps
        != STALE_CRANKER_SHARE_BPS
    {
        return Err("PR 337 delayed policy did not overwrite the correction".into());
    }

    env.warp_to_slot(SYNC_SLOT);
    let victim_capital_before = env.primary_portfolio(FEE_PAYER).capital.get();
    let attacker_capital_before = env.primary_portfolio(ATTACKER).capital.get();
    let insurance_before = env.primary_market_state().1.insurance;
    let sync = env
        .sync_maintenance_fee_with_reward(FEE_PAYER, ATTACKER, SYNC_SLOT)
        .map_err(|error| format!("PR 337 sync victim maintenance fee: {error}"))?;
    let victim_loss = victim_capital_before
        .checked_sub(env.primary_portfolio(FEE_PAYER).capital.get())
        .ok_or("PR 337 fee payer capital increased")?;
    let attacker_reward = env
        .primary_portfolio(ATTACKER)
        .capital
        .get()
        .checked_sub(attacker_capital_before)
        .ok_or("PR 337 attacker capital decreased")?;
    let insurance_delta = env
        .primary_market_state()
        .1
        .insurance
        .checked_sub(insurance_before)
        .ok_or("PR 337 insurance decreased")?;
    if victim_loss != EXPECTED_FEE as u128 || attacker_reward != victim_loss || insurance_delta != 0
    {
        return Err(format!(
            "PR 337 delayed split mismatch: victim={victim_loss}, reward={attacker_reward}, \
             insurance={insurance_delta}"
        ));
    }

    let destination = env.actors[ATTACKER].destination_token;
    let destination_before = env.token_amount(destination);
    let withdrawal = env
        .withdraw_primary(ATTACKER, attacker_reward)
        .map_err(|error| format!("PR 337 attacker could not withdraw reward: {error}"))?;
    let attacker_extraction = env
        .token_amount(destination)
        .checked_sub(destination_before)
        .ok_or("PR 337 attacker destination decreased")?;
    let max_cu = correction
        .compute_units
        .max(replay.compute_units)
        .max(sync.compute_units)
        .max(withdrawal.compute_units);
    if attacker_extraction != EXPECTED_FEE
        || u128::from(attacker_extraction) != victim_loss
        || max_cu >= TX_CU_LIMIT
        || env.token_supply_observed() != supply_before
    {
        return Err(format!(
            "PR 337 terminal extraction mismatch: victim={victim_loss}, \
             extraction={attacker_extraction}, max_cu={max_cu}, supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }

    Ok(DelayedMaintenancePolicyReplayReproduction {
        blocker: KnownBlocker::DelayedMaintenancePolicyReplay,
        victim_loss: victim_loss as u64,
        attacker_extraction,
        insurance_delta,
        live_oi_q,
        correction_cu: correction.compute_units,
        replay_cu: replay.compute_units,
        sync_cu: sync.compute_units,
    })
}

#[derive(Clone, Copy, Debug)]
struct LiquidationPolicyExtraction {
    victim_capital_loss: u64,
    attacker_extraction: u64,
    insurance_delta: u128,
    liquidation_cu: u64,
}

fn finish_liquidation_policy_extraction(
    env: &mut V16Svm,
    supply_before: u128,
    victim: usize,
    attacker: usize,
    position_q: i128,
    liquidation_target: u64,
    first_slot: u64,
    last_slot: u64,
    context: &str,
) -> Result<LiquidationPolicyExtraction, String> {
    let observations = vec![CrankObservationHint {
        asset_index: 0,
        oracle_accounts: 0,
    }];
    let mut liquidation_slot = None;
    for slot in first_slot..=last_slot {
        env.warp_to_slot(slot);
        env.push_auth_mark(0, slot, liquidation_target)
            .map_err(|error| {
                format!("{context} publish liquidation mark at slot {slot}: {error}")
            })?;
        env.crank(victim, slot, observations.clone())
            .map_err(|error| format!("{context} refresh victim at slot {slot}: {error}"))?;
        if position_for_asset(&env.primary_portfolio(victim), 0)? != -position_q {
            return Err(format!(
                "{context} victim liquidated before reward-bearing crank at slot {slot}"
            ));
        }
        let cert = env
            .primary_portfolio(victim)
            .health_cert
            .try_to_runtime()
            .map_err(|error| format!("{context} decode health certificate: {error:?}"))?;
        let maintenance = i128::try_from(cert.certified_maintenance_req)
            .map_err(|_| format!("{context} maintenance requirement exceeds signed range"))?;
        if cert.certified_equity < maintenance {
            liquidation_slot = Some(slot);
            break;
        }
    }
    let liquidation_slot =
        liquidation_slot.ok_or_else(|| format!("{context} victim never became liquidatable"))?;
    let victim_capital_before = env.primary_portfolio(victim).capital.get();
    let attacker_capital_before = env.primary_portfolio(attacker).capital.get();
    let insurance_before = env.primary_market_state().1.insurance;
    let vault_before = env.token_amount(env.vault);
    let mut liquidation_cu = 0;
    for attempt in 0..8 {
        let crank = env
            .crank_with_reward(
                attacker,
                victim,
                liquidation_slot,
                if attempt == 0 {
                    observations.clone()
                } else {
                    Vec::new()
                },
                &[],
            )
            .map_err(|error| format!("{context} reward crank {attempt}: {error}"))?;
        liquidation_cu = liquidation_cu.max(crank.compute_units);
        if position_for_asset(&env.primary_portfolio(victim), 0)?.unsigned_abs()
            < position_q.unsigned_abs()
        {
            break;
        }
    }

    let victim_capital_loss = victim_capital_before
        .checked_sub(env.primary_portfolio(victim).capital.get())
        .ok_or_else(|| format!("{context} liquidation increased victim capital"))?;
    let attacker_reward = env
        .primary_portfolio(attacker)
        .capital
        .get()
        .checked_sub(attacker_capital_before)
        .ok_or_else(|| format!("{context} liquidation reduced attacker capital"))?;
    let insurance_delta = env
        .primary_market_state()
        .1
        .insurance
        .checked_sub(insurance_before)
        .ok_or_else(|| format!("{context} liquidation reduced insurance"))?;
    if attacker_reward == 0
        || victim_capital_loss != attacker_reward
        || insurance_delta != 0
        || position_for_asset(&env.primary_portfolio(victim), 0)?.unsigned_abs()
            >= position_q.unsigned_abs()
    {
        return Err(format!(
            "{context} stale split did not redirect a real liquidation fee: \
             victim={victim_capital_loss}, reward={attacker_reward}, insurance={insurance_delta}, \
             position={}",
            position_for_asset(&env.primary_portfolio(victim), 0)?
        ));
    }

    let destination = env.actors[attacker].destination_token;
    let destination_before = env.token_amount(destination);
    let withdrawal = env
        .withdraw_primary(attacker, attacker_reward)
        .map_err(|error| format!("{context} attacker could not withdraw reward: {error}"))?;
    let attacker_extraction = env
        .token_amount(destination)
        .checked_sub(destination_before)
        .ok_or_else(|| format!("{context} attacker destination decreased"))?;
    let expected_vault = vault_before
        .checked_sub(attacker_extraction)
        .ok_or_else(|| format!("{context} extraction exceeded canonical vault"))?;
    let max_cu = liquidation_cu.max(withdrawal.compute_units);
    if u128::from(attacker_extraction) != attacker_reward
        || env.token_amount(env.vault) != expected_vault
        || max_cu >= TX_CU_LIMIT
        || env.token_supply_observed() != supply_before
    {
        return Err(format!(
            "{context} public extraction mismatch: reward={attacker_reward}, \
             extraction={attacker_extraction}, vault={}/{vault_before}, max_cu={max_cu}, \
             supply={}/{}",
            env.token_amount(env.vault),
            env.token_supply_observed(),
            supply_before
        ));
    }

    Ok(LiquidationPolicyExtraction {
        victim_capital_loss: victim_capital_loss as u64,
        attacker_extraction,
        insurance_delta,
        liquidation_cu,
    })
}

pub fn reproduce_liquidation_policy_generation_replay(
    seed: [u8; 32],
) -> Result<LiquidationPolicyGenerationReplayReproduction, String> {
    const LONG: usize = 0;
    const VICTIM: usize = 1;
    const ATTACKER: usize = 2;
    const INITIAL_PRICE: u64 = 1_000_000;
    const LIQUIDATION_TARGET: u64 = 2_000_000;
    const LONG_DEPOSIT: u128 = 100_000_000;
    const VICTIM_DEPOSIT: u128 = 100_000;
    const POSITION_Q: i128 = POS_SCALE as i128;
    const CRANKER_SHARE_BPS: u16 = 10_000;
    const REINIT_SLOT: u64 = 10;

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
        actor_deposits: [1, 1, 1, 1, 1],
        ..MarketConfig::default()
    };
    let mut env = V16Svm::new(seed, config);
    let supply_before = env.token_supply_observed();
    let old_asset_market_id = env.primary_market_state().1.assets[0].market_id;
    let retained_policy = env.build_retained_liquidation_fee_policy(CRANKER_SHARE_BPS);

    publicly_recreate_primary_market(&mut env, config, REINIT_SLOT, "PR 326")?;
    env.configure_auth_mark(false, 0, REINIT_SLOT, INITIAL_PRICE)
        .map_err(|error| format!("PR 326 configure generation-B AuthMark: {error}"))?;
    let new_asset_market_id = env.primary_market_state().1.assets[0].market_id;
    if env
        .primary_market_state()
        .0
        .liquidation_cranker_fee_share_bps
        != 0
    {
        return Err("PR 326 generation B did not start with zero cranker share".into());
    }

    for actor in [LONG, VICTIM, ATTACKER] {
        env.fund_closed_primary_portfolio(actor, 1_000_000_000)
            .map_err(|error| format!("PR 326 re-fund portfolio {actor}: {error}"))?;
        env.reinitialize_primary_portfolio(actor)
            .map_err(|error| format!("PR 326 initialize portfolio {actor}: {error}"))?;
    }
    env.deposit_primary(LONG, LONG_DEPOSIT)
        .map_err(|error| format!("PR 326 deposit long: {error}"))?;
    env.deposit_primary(VICTIM, VICTIM_DEPOSIT)
        .map_err(|error| format!("PR 326 deposit victim: {error}"))?;
    env.trade_no_cpi(LONG, VICTIM, 0, POSITION_Q, INITIAL_PRICE, 0)
        .map_err(|error| format!("PR 326 establish generation-B live OI: {error}"))?;
    let live_oi_q = env.primary_market_state().1.assets[0].oi_eff_long_q;
    if live_oi_q == 0 || position_for_asset(&env.primary_portfolio(VICTIM), 0)? != -POSITION_Q {
        return Err("PR 326 generation-B victim position was not live".into());
    }

    let replay = env
        .land_retained(retained_policy)
        .map_err(|error| format!("PR 326 stale liquidation policy no longer lands: {error}"))?;
    if env
        .primary_market_state()
        .0
        .liquidation_cranker_fee_share_bps
        != CRANKER_SHARE_BPS
    {
        return Err("PR 326 stale policy did not install the 100% cranker split".into());
    }

    let extraction = finish_liquidation_policy_extraction(
        &mut env,
        supply_before,
        VICTIM,
        ATTACKER,
        POSITION_Q,
        LIQUIDATION_TARGET,
        11,
        40,
        "PR 326",
    )?;

    Ok(LiquidationPolicyGenerationReplayReproduction {
        blocker: KnownBlocker::LiquidationPolicyGenerationReplay,
        old_asset_market_id,
        new_asset_market_id,
        victim_capital_loss: extraction.victim_capital_loss,
        attacker_extraction: extraction.attacker_extraction,
        insurance_delta: extraction.insurance_delta,
        live_oi_q,
        replay_cu: replay.compute_units,
        liquidation_cu: extraction.liquidation_cu,
    })
}

pub fn reproduce_delayed_liquidation_policy_replay(
    seed: [u8; 32],
) -> Result<DelayedLiquidationPolicyReplayReproduction, String> {
    const LONG: usize = 0;
    const VICTIM: usize = 1;
    const ATTACKER: usize = 2;
    const INITIAL_PRICE: u64 = 1_000_000;
    const LIQUIDATION_TARGET: u64 = 2_000_000;
    const LONG_DEPOSIT: u128 = 100_000_000;
    const VICTIM_DEPOSIT: u128 = 100_000;
    const POSITION_Q: i128 = POS_SCALE as i128;
    const STALE_CRANKER_SHARE_BPS: u16 = 10_000;
    const CURRENT_CRANKER_SHARE_BPS: u16 = 0;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
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
            actor_deposits: [LONG_DEPOSIT, VICTIM_DEPOSIT, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.configure_auth_mark(false, 0, 1, INITIAL_PRICE)
        .map_err(|error| format!("PR 336 configure AuthMark: {error}"))?;
    let retained_policy = env.build_retained_liquidation_fee_policy(STALE_CRANKER_SHARE_BPS);
    let correction = env
        .update_liquidation_fee_policy(CURRENT_CRANKER_SHARE_BPS)
        .map_err(|error| format!("PR 336 land current zero-share correction: {error}"))?;
    if env
        .primary_market_state()
        .0
        .liquidation_cranker_fee_share_bps
        != CURRENT_CRANKER_SHARE_BPS
    {
        return Err("PR 336 current liquidation policy did not land".into());
    }

    env.trade_no_cpi(LONG, VICTIM, 0, POSITION_Q, INITIAL_PRICE, 0)
        .map_err(|error| format!("PR 336 establish independent live OI: {error}"))?;
    let live_oi_q = env.primary_market_state().1.assets[0].oi_eff_long_q;
    if live_oi_q == 0 || position_for_asset(&env.primary_portfolio(VICTIM), 0)? != -POSITION_Q {
        return Err("PR 336 victim position was not live".into());
    }
    let replay = env
        .land_retained(retained_policy)
        .map_err(|error| format!("PR 336 delayed liquidation policy no longer lands: {error}"))?;
    if env
        .primary_market_state()
        .0
        .liquidation_cranker_fee_share_bps
        != STALE_CRANKER_SHARE_BPS
    {
        return Err("PR 336 delayed policy did not overwrite the correction".into());
    }

    let extraction = finish_liquidation_policy_extraction(
        &mut env,
        supply_before,
        VICTIM,
        ATTACKER,
        POSITION_Q,
        LIQUIDATION_TARGET,
        2,
        31,
        "PR 336",
    )?;
    let max_cu = correction
        .compute_units
        .max(replay.compute_units)
        .max(extraction.liquidation_cu);
    if max_cu >= TX_CU_LIMIT {
        return Err(format!("PR 336 instruction exceeded CU limit: {max_cu}"));
    }

    Ok(DelayedLiquidationPolicyReplayReproduction {
        blocker: KnownBlocker::DelayedLiquidationPolicyReplay,
        victim_capital_loss: extraction.victim_capital_loss,
        attacker_extraction: extraction.attacker_extraction,
        insurance_delta: extraction.insurance_delta,
        live_oi_q,
        correction_cu: correction.compute_units,
        replay_cu: replay.compute_units,
        liquidation_cu: extraction.liquidation_cu,
    })
}

#[derive(Clone, Copy, Debug)]
struct DelayedAssetFeeExtraction {
    victim_loss: u64,
    attacker_profit: u64,
    extracted_fee: u64,
    withdrawal_cu: u64,
}

#[allow(clippy::too_many_arguments)]
fn finish_delayed_asset_fee_extraction(
    env: &mut V16Svm,
    supply_before: u128,
    victim: usize,
    attacker: usize,
    asset: u16,
    size_q: i128,
    price: u64,
    deposit: u128,
    fee_per_side: u64,
    total_fee: u128,
    context: &str,
) -> Result<DelayedAssetFeeExtraction, String> {
    let group_after_trade = env.primary_market_state().1;
    let attacker_domain_fee = group_after_trade.insurance_domain_budget[asset as usize * 2]
        .checked_add(group_after_trade.insurance_domain_budget[asset as usize * 2 + 1])
        .ok_or_else(|| format!("{context} attacker domain fee overflow"))?;
    if attacker_domain_fee != total_fee {
        return Err(format!(
            "{context} delayed policy credited {attacker_domain_fee}, expected {total_fee}"
        ));
    }

    let victim_destination = env.actors[victim].destination_token;
    let attacker_destination = env.actors[attacker].destination_token;
    let victim_destination_before = env.token_amount(victim_destination);
    let attacker_destination_before = env.token_amount(attacker_destination);
    let withdrawal = env
        .withdraw_insurance_asset(attacker, asset, total_fee)
        .map_err(|error| format!("{context} attacker could not withdraw trade fees: {error}"))?;
    env.update_trade_fee_policy(0)
        .map_err(|error| format!("{context} restore zero fee before neutral close: {error}"))?;
    env.trade_no_cpi(victim, attacker, asset, -size_q, price, 0)
        .map_err(|error| format!("{context} close user risk: {error}"))?;
    let victim_capital = env.primary_portfolio(victim).capital.get();
    let attacker_capital = env.primary_portfolio(attacker).capital.get();
    env.withdraw_primary(victim, victim_capital)
        .map_err(|error| format!("{context} victim terminal withdrawal: {error}"))?;
    env.withdraw_primary(attacker, attacker_capital)
        .map_err(|error| format!("{context} attacker terminal withdrawal: {error}"))?;

    let victim_return = env
        .token_amount(victim_destination)
        .checked_sub(victim_destination_before)
        .ok_or_else(|| format!("{context} victim destination decreased"))?;
    let attacker_return = env
        .token_amount(attacker_destination)
        .checked_sub(attacker_destination_before)
        .ok_or_else(|| format!("{context} attacker destination decreased"))?;
    let deposit_u64 =
        u64::try_from(deposit).map_err(|_| format!("{context} deposit does not fit u64"))?;
    let victim_loss = deposit_u64
        .checked_sub(victim_return)
        .ok_or_else(|| format!("{context} victim returned more than deposited"))?;
    let attacker_profit = attacker_return
        .checked_sub(deposit_u64)
        .ok_or_else(|| format!("{context} attacker did not recover its deposit"))?;
    if victim_loss != fee_per_side
        || attacker_profit != victim_loss
        || attacker_return != deposit_u64 + fee_per_side
        || withdrawal.compute_units >= TX_CU_LIMIT
        || env.token_supply_observed() != supply_before
    {
        return Err(format!(
            "{context} terminal extraction mismatch: victim={victim_loss}, \
             profit={attacker_profit}, attacker_return={attacker_return}, \
             withdrawal_cu={}, supply={}/{}",
            withdrawal.compute_units,
            env.token_supply_observed(),
            supply_before
        ));
    }

    Ok(DelayedAssetFeeExtraction {
        victim_loss,
        attacker_profit,
        extracted_fee: attacker_domain_fee as u64,
        withdrawal_cu: withdrawal.compute_units,
    })
}

pub fn verify_delayed_trade_fee_policy_nonextraction(
    seed: [u8; 32],
) -> Result<DelayedTradeFeePolicyReplayProtection, String> {
    const VICTIM: usize = 0;
    const ATTACKER: usize = 1;
    const ACTIVATION_PAYER: usize = 2;
    const ASSET: u16 = 1;
    const PRICE: u64 = 100;
    const DEPOSIT: u128 = 10_000;
    const SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const STALE_TRADE_FEE_BPS: u64 = 10_000;
    const CURRENT_TRADE_FEE_BPS: u64 = 0;
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            max_trading_fee_bps: STALE_TRADE_FEE_BPS,
            actor_deposits: [DEPOSIT, DEPOSIT, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    let retained_policy = env.build_retained_trade_fee_policy(STALE_TRADE_FEE_BPS);
    let correction = env
        .update_trade_fee_policy(CURRENT_TRADE_FEE_BPS)
        .map_err(|error| format!("PR 338 land current zero-fee correction: {error}"))?;
    if env.primary_market_state().0.trade_fee_base_bps != CURRENT_TRADE_FEE_BPS {
        return Err("PR 338 current trade-fee correction did not land".into());
    }

    env.update_market_init_fee_policy(1)
        .map_err(|error| format!("PR 338 configure permissionless init fee: {error}"))?;
    env.warp_to_slot(2);
    env.retire_asset(ASSET, 2)
        .map_err(|error| format!("PR 338 retire empty asset slot: {error}"))?;
    env.warp_to_slot(3);
    env.activate_permissionless_asset_with_actor_authorities(
        ACTIVATION_PAYER,
        ASSET,
        3,
        PRICE,
        ATTACKER,
        ATTACKER,
        ATTACKER,
        ATTACKER,
        1,
    )
    .map_err(|error| format!("PR 338 activate attacker-operated asset: {error}"))?;

    let retained_trade = env.build_retained_no_cpi_trade(VICTIM, ATTACKER, ASSET, SIZE_Q, PRICE);
    let replay = env
        .land_retained(retained_policy)
        .map_err(|error| format!("PR 338 delayed trade-fee policy no longer lands: {error}"))?;
    if env.primary_market_state().0.trade_fee_base_bps != STALE_TRADE_FEE_BPS {
        return Err("PR 338 delayed policy did not overwrite the correction".into());
    }
    let stale_policy_landed = true;
    let before_rejection = tracked_economic_accounts(&env);
    let victim_capital_before = env.primary_portfolio(VICTIM).capital.get();
    let attacker_capital_before = env.primary_portfolio(ATTACKER).capital.get();
    let stale_trade_error = match env.land_retained(retained_trade) {
        Ok(_) => return Err("PR 338 stale zero-fee trade unexpectedly landed".into()),
        Err(error) => error,
    };
    let stale_trade_rejected = stale_trade_error.contains("Custom(9)")
        || stale_trade_error.contains("custom program error: 0x9");
    let rejected_exact_rollback = tracked_economic_accounts(&env) == before_rejection;
    if !stale_trade_rejected
        || !rejected_exact_rollback
        || env.primary_portfolio(VICTIM).capital.get() != victim_capital_before
        || env.primary_portfolio(ATTACKER).capital.get() != attacker_capital_before
    {
        return Err(format!(
            "PR 338 stale trade did not reject atomically: {stale_trade_error}"
        ));
    }

    let recovery_correction = env
        .update_trade_fee_policy(CURRENT_TRADE_FEE_BPS)
        .map_err(|error| format!("PR 338 restore visible zero-fee policy: {error}"))?;
    let open = env
        .trade_no_cpi(
            VICTIM,
            ATTACKER,
            ASSET,
            SIZE_Q,
            PRICE,
            CURRENT_TRADE_FEE_BPS,
        )
        .map_err(|error| format!("PR 338 freshly signed zero-fee open failed: {error}"))?;
    let close = env
        .trade_no_cpi(
            VICTIM,
            ATTACKER,
            ASSET,
            -SIZE_Q,
            PRICE,
            CURRENT_TRADE_FEE_BPS,
        )
        .map_err(|error| format!("PR 338 freshly signed zero-fee close failed: {error}"))?;

    let victim_destination = env.actors[VICTIM].destination_token;
    let attacker_destination = env.actors[ATTACKER].destination_token;
    let victim_destination_before = env.token_amount(victim_destination);
    let attacker_destination_before = env.token_amount(attacker_destination);
    let victim_capital = env.primary_portfolio(VICTIM).capital.get();
    let attacker_capital = env.primary_portfolio(ATTACKER).capital.get();
    let victim_withdrawal = env
        .withdraw_primary(VICTIM, victim_capital)
        .map_err(|error| format!("PR 338 victim terminal withdrawal: {error}"))?;
    let attacker_withdrawal = env
        .withdraw_primary(ATTACKER, attacker_capital)
        .map_err(|error| format!("PR 338 attacker terminal withdrawal: {error}"))?;
    let victim_return = env
        .token_amount(victim_destination)
        .checked_sub(victim_destination_before)
        .ok_or("PR 338 victim destination decreased")?;
    let attacker_return = env
        .token_amount(attacker_destination)
        .checked_sub(attacker_destination_before)
        .ok_or("PR 338 attacker destination decreased")?;
    let deposit_u64 = u64::try_from(DEPOSIT).map_err(|_| "PR 338 deposit exceeds u64")?;
    let victim_loss = deposit_u64
        .checked_sub(victim_return)
        .ok_or("PR 338 victim returned more than deposited")?;
    let attacker_profit = attacker_return
        .checked_sub(deposit_u64)
        .ok_or("PR 338 attacker did not recover its deposit")?;
    let extracted_fee = env.primary_market_state().1.insurance_domain_budget[ASSET as usize * 2]
        .checked_add(env.primary_market_state().1.insurance_domain_budget[ASSET as usize * 2 + 1])
        .ok_or("PR 338 fee total overflow")?;
    let correction_cu = correction
        .compute_units
        .max(recovery_correction.compute_units);
    let trade_cu = open.compute_units.max(close.compute_units);
    let withdrawal_cu = victim_withdrawal
        .compute_units
        .max(attacker_withdrawal.compute_units);
    let token_supply_conserved = env.token_supply_observed() == supply_before;
    let max_cu = correction_cu
        .max(replay.compute_units)
        .max(trade_cu)
        .max(withdrawal_cu);
    if max_cu >= TX_CU_LIMIT {
        return Err(format!("PR 338 instruction exceeded CU limit: {max_cu}"));
    }
    if victim_loss != 0 || attacker_profit != 0 || extracted_fee != 0 || !token_supply_conserved {
        return Err(format!(
            "PR 338 stale policy still extracted value: victim={victim_loss}, \
             attacker={attacker_profit}, fee={extracted_fee}, supply={}/{supply_before}",
            env.token_supply_observed()
        ));
    }

    Ok(DelayedTradeFeePolicyReplayProtection {
        blocker: KnownBlocker::DelayedTradeFeePolicyReplay,
        stale_policy_landed,
        stale_trade_rejected,
        rejected_exact_rollback,
        victim_loss,
        attacker_profit,
        extracted_fee: u64::try_from(extracted_fee).map_err(|_| "PR 338 fee exceeds u64")?,
        correction_cu,
        replay_cu: replay.compute_units,
        trade_cu,
        withdrawal_cu,
        token_supply_conserved,
    })
}

pub fn reproduce_delayed_fee_redirect_policy_replay(
    seed: [u8; 32],
) -> Result<DelayedFeeRedirectPolicyReplayReproduction, String> {
    const VICTIM: usize = 0;
    const ATTACKER: usize = 1;
    const ACTIVATION_PAYER: usize = 2;
    const ASSET: u16 = 1;
    const PRICE: u64 = 100;
    const DEPOSIT: u128 = 10_000;
    const SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const TRADE_FEE_BPS: u64 = 10_000;
    const STALE_REDIRECT_BPS: u16 = 0;
    const CURRENT_REDIRECT_BPS: u16 = 10_000;
    const FEE_PER_SIDE: u64 = 1_000;
    const TOTAL_FEE: u128 = 2 * FEE_PER_SIDE as u128;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            max_trading_fee_bps: TRADE_FEE_BPS,
            actor_deposits: [DEPOSIT, DEPOSIT, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.update_trade_fee_policy(TRADE_FEE_BPS)
        .map_err(|error| format!("PR 340 install fixed trade fee: {error}"))?;
    let retained_policy = env.build_retained_fee_redirect_policy(STALE_REDIRECT_BPS);
    let correction = env
        .update_fee_redirect_policy(CURRENT_REDIRECT_BPS)
        .map_err(|error| format!("PR 340 land current protected redirect: {error}"))?;
    if env.primary_market_state().0.fee_redirect_to_market_0_bps != CURRENT_REDIRECT_BPS {
        return Err("PR 340 current fee redirect did not land".into());
    }

    env.update_market_init_fee_policy(1)
        .map_err(|error| format!("PR 340 configure permissionless init fee: {error}"))?;
    env.warp_to_slot(2);
    env.retire_asset(ASSET, 2)
        .map_err(|error| format!("PR 340 retire empty asset slot: {error}"))?;
    env.warp_to_slot(3);
    env.activate_permissionless_asset_with_actor_authorities(
        ACTIVATION_PAYER,
        ASSET,
        3,
        PRICE,
        ATTACKER,
        ATTACKER,
        ATTACKER,
        ATTACKER,
        1,
    )
    .map_err(|error| format!("PR 340 activate attacker-operated asset: {error}"))?;

    let retained_trade = env.build_retained_no_cpi_trade_with_fee(
        VICTIM,
        ATTACKER,
        ASSET,
        SIZE_Q,
        PRICE,
        TRADE_FEE_BPS,
    );
    let replay = env
        .land_retained(retained_policy)
        .map_err(|error| format!("PR 340 delayed redirect policy no longer lands: {error}"))?;
    if env.primary_market_state().0.fee_redirect_to_market_0_bps != STALE_REDIRECT_BPS {
        return Err("PR 340 delayed redirect did not overwrite the correction".into());
    }
    let trade = env
        .land_retained(retained_trade)
        .map_err(|error| format!("PR 340 victim's fee-bearing trade rejected: {error}"))?;
    let extraction = finish_delayed_asset_fee_extraction(
        &mut env,
        supply_before,
        VICTIM,
        ATTACKER,
        ASSET,
        SIZE_Q,
        PRICE,
        DEPOSIT,
        FEE_PER_SIDE,
        TOTAL_FEE,
        "PR 340",
    )?;
    let max_cu = correction
        .compute_units
        .max(replay.compute_units)
        .max(trade.compute_units)
        .max(extraction.withdrawal_cu);
    if max_cu >= TX_CU_LIMIT {
        return Err(format!("PR 340 instruction exceeded CU limit: {max_cu}"));
    }

    Ok(DelayedFeeRedirectPolicyReplayReproduction {
        blocker: KnownBlocker::DelayedFeeRedirectPolicyReplay,
        victim_loss: extraction.victim_loss,
        attacker_profit: extraction.attacker_profit,
        extracted_fee: extraction.extracted_fee,
        correction_cu: correction.compute_units,
        replay_cu: replay.compute_units,
        trade_cu: trade.compute_units,
        withdrawal_cu: extraction.withdrawal_cu,
    })
}

pub fn reproduce_delayed_backing_fee_policy_replay(
    seed: [u8; 32],
) -> Result<DelayedBackingFeePolicyReplayReproduction, String> {
    const VICTIM: usize = 0;
    const PROVIDER: usize = 1;
    const POLICY_AUTHORITY: usize = 2;
    const ASSET: u16 = 1;
    const WINNING_DOMAIN: u16 = ASSET * 2 + 1;
    const INITIAL_PRICE: u64 = 100;
    const ASSET_WIN_MARK: u64 = 105;
    const BASE_LOSS_MARK: u64 = 95;
    const ASSET_SIZE_Q: i128 = 2_000 * POS_SCALE as i128;
    const BASE_SIZE_Q: i128 = 1_000 * POS_SCALE as i128;
    const SAFE_INCREASE_Q: i128 = 100 * POS_SCALE as i128;
    const VICTIM_DEPOSIT: u128 = 31_300;
    const PROVIDER_DEPOSIT: u128 = 100_000;
    const BACKING_PRINCIPAL: u128 = 15_000;
    const STALE_FEE_BPS: u16 = 5_000;
    const CURRENT_FEE_BPS: u16 = 0;
    const EXPECTED_FEE: u64 = 75;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: INITIAL_PRICE,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [VICTIM_DEPOSIT, PROVIDER_DEPOSIT, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.update_asset_authority_from_admin(
        ASSET,
        percolator_prog::processor::ASSET_AUTH_INSURANCE,
        POLICY_AUTHORITY,
    )
    .map_err(|error| format!("PR 349 install fee-policy authority: {error}"))?;
    env.update_asset_authority_from_admin(
        ASSET,
        percolator_prog::processor::ASSET_AUTH_BACKING_BUCKET,
        PROVIDER,
    )
    .map_err(|error| format!("PR 349 install backing provider: {error}"))?;
    let retained_policy = env.build_retained_backing_fee_policy_for_actor(
        POLICY_AUTHORITY,
        WINNING_DOMAIN,
        STALE_FEE_BPS,
        0,
    );
    let correction = env
        .update_backing_fee_policy_for_actor(POLICY_AUTHORITY, WINNING_DOMAIN, CURRENT_FEE_BPS, 0)
        .map_err(|error| format!("PR 349 land current zero-fee correction: {error}"))?;
    env.configure_auth_mark(false, ASSET, 1, INITIAL_PRICE)
        .map_err(|error| format!("PR 349 configure asset AuthMark: {error}"))?;
    env.top_up_backing_bucket_for_actor(PROVIDER, WINNING_DOMAIN, BACKING_PRINCIPAL, 100)
        .map_err(|error| format!("PR 349 fund provider backing: {error}"))?;

    env.trade_no_cpi(VICTIM, PROVIDER, ASSET, ASSET_SIZE_Q, INITIAL_PRICE, 0)
        .map_err(|error| format!("PR 349 establish source-backed winning leg: {error}"))?;
    env.trade_no_cpi(VICTIM, PROVIDER, 0, BASE_SIZE_Q, INITIAL_PRICE, 0)
        .map_err(|error| format!("PR 349 establish offsetting losing leg: {error}"))?;
    env.warp_to_slot(2);
    env.push_auth_mark(ASSET, 2, ASSET_WIN_MARK)
        .map_err(|error| format!("PR 349 push winning mark: {error}"))?;
    env.push_auth_mark(0, 2, BASE_LOSS_MARK)
        .map_err(|error| format!("PR 349 push losing mark: {error}"))?;
    for (actor, asset_index) in [(PROVIDER, ASSET), (VICTIM, ASSET), (PROVIDER, 0)] {
        let oracle_accounts = env.primary_profile(asset_index as usize).oracle_leg_count;
        env.crank(
            actor,
            2,
            vec![CrankObservationHint {
                asset_index,
                oracle_accounts,
            }],
        )
        .map_err(|error| format!("PR 349 crank actor {actor} asset {asset_index}: {error}"))?;
    }
    if env.primary_portfolio(VICTIM).pnl.get() != 10_000 {
        return Err(format!(
            "PR 349 source-backed claim mismatch: {}",
            env.primary_portfolio(VICTIM).pnl.get()
        ));
    }

    let retained_trade =
        env.build_retained_no_cpi_trade(VICTIM, PROVIDER, 0, SAFE_INCREASE_Q, BASE_LOSS_MARK);
    let victim_capital_before = env.primary_portfolio(VICTIM).capital.get();
    let provider_capital_before = env.primary_portfolio(PROVIDER).capital.get();
    let earnings_before = env.primary_market_state().1.source_backing_buckets
        [WINNING_DOMAIN as usize]
        .utilization_fee_earnings;
    let replay = env
        .land_retained(retained_policy)
        .map_err(|error| format!("PR 349 delayed backing policy no longer lands: {error}"))?;
    let profile = env.primary_profile(ASSET as usize);
    if profile.backing_trade_fee_bps_short != STALE_FEE_BPS
        || profile.backing_trade_fee_insurance_share_bps_short != 0
    {
        return Err("PR 349 delayed policy did not overwrite the correction".into());
    }
    let trade = env
        .land_retained(retained_trade)
        .map_err(|error| format!("PR 349 victim's zero-fee increase rejected: {error}"))?;
    let victim_loss = victim_capital_before
        .checked_sub(env.primary_portfolio(VICTIM).capital.get())
        .ok_or("PR 349 victim capital increased")?;
    let earnings_after = env.primary_market_state().1.source_backing_buckets
        [WINNING_DOMAIN as usize]
        .utilization_fee_earnings;
    let backing_earnings = earnings_after
        .checked_sub(earnings_before)
        .ok_or("PR 349 backing earnings decreased")?;
    if victim_loss != EXPECTED_FEE as u128
        || backing_earnings != victim_loss
        || env.primary_portfolio(PROVIDER).capital.get() != provider_capital_before
    {
        return Err(format!(
            "PR 349 delayed fee mismatch: victim={victim_loss}, earnings={backing_earnings}, \
             provider_capital={provider_capital_before}/{}",
            env.primary_portfolio(PROVIDER).capital.get()
        ));
    }

    let destination = env.actors[PROVIDER].destination_token;
    let destination_before = env.token_amount(destination);
    let withdrawal = env
        .withdraw_backing_bucket_earnings_for_actor(PROVIDER, WINNING_DOMAIN, backing_earnings)
        .map_err(|error| format!("PR 349 provider could not withdraw victim fee: {error}"))?;
    let provider_extraction = env
        .token_amount(destination)
        .checked_sub(destination_before)
        .ok_or("PR 349 provider destination decreased")?;
    let max_cu = correction
        .compute_units
        .max(replay.compute_units)
        .max(trade.compute_units)
        .max(withdrawal.compute_units);
    if provider_extraction != EXPECTED_FEE
        || u128::from(provider_extraction) != victim_loss
        || max_cu >= TX_CU_LIMIT
        || env.token_supply_observed() != supply_before
    {
        return Err(format!(
            "PR 349 terminal extraction mismatch: victim={victim_loss}, \
             extraction={provider_extraction}, max_cu={max_cu}, supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }

    Ok(DelayedBackingFeePolicyReplayReproduction {
        blocker: KnownBlocker::DelayedBackingFeePolicyReplay,
        victim_loss: victim_loss as u64,
        provider_extraction,
        backing_earnings,
        correction_cu: correction.compute_units,
        replay_cu: replay.compute_units,
        trade_cu: trade.compute_units,
        withdrawal_cu: withdrawal.compute_units,
    })
}

#[derive(Clone, Copy, Debug)]
struct DelayedOracleIntentWorld {
    stale_mark: u64,
    restored_mark: u64,
    victim_payout: u64,
    beneficiary_payout: u64,
    replay_cu: u64,
    max_crank_cu: u64,
}

fn crank_delayed_oracle_actor(
    env: &mut V16Svm,
    actor: usize,
    slot: u64,
    asset: u16,
    context: &str,
) -> Result<u64, String> {
    let observations = vec![CrankObservationHint {
        asset_index: asset,
        oracle_accounts: 0,
    }];
    let mut max_cu = 0;
    let mut progressed = false;
    for _ in 0..4 {
        match env.crank(actor, slot, observations.clone()) {
            Ok(crank) => {
                progressed = true;
                max_cu = max_cu.max(crank.compute_units);
            }
            Err(error) if progressed && error.contains("Custom(22)") => break,
            Err(error) => return Err(format!("{context} crank actor {actor}: {error}")),
        }
    }
    if !progressed {
        return Err(format!("{context} crank actor {actor} made no progress"));
    }
    Ok(max_cu)
}

fn run_delayed_oracle_intent_world(
    seed: [u8; 32],
    path: DelayedOracleIntentPath,
    land_replay: bool,
) -> Result<DelayedOracleIntentWorld, String> {
    const VICTIM: usize = 0;
    const BENEFICIARY: usize = 1;
    const ASSET: u16 = 1;
    const HONEST_PRICE: u64 = 100;
    const STALE_PRICE: u64 = 50;
    const SIZE_Q: i128 = 5_000 * POS_SCALE as i128;
    const DEPOSIT: u128 = 1_000_000;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: HONEST_PRICE,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [DEPOSIT, DEPOSIT, 1, 1, super::v16_svm::EXIT_MAKER_DEPOSIT],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.configure_auth_mark(false, ASSET, 1, HONEST_PRICE)
        .map_err(|error| format!("PR 335 {path:?} configure initial AuthMark: {error}"))?;
    let retained = match path {
        DelayedOracleIntentPath::PushAuth => env.build_retained_auth_mark(ASSET, STALE_PRICE),
        DelayedOracleIntentPath::ConfigureAuth => {
            env.build_retained_auth_config(ASSET, STALE_PRICE)
        }
    };
    let victim_position_q = match path {
        DelayedOracleIntentPath::PushAuth => SIZE_Q,
        DelayedOracleIntentPath::ConfigureAuth => -SIZE_Q,
    };

    let mut replay_cu = 0;
    if path == DelayedOracleIntentPath::PushAuth {
        env.trade_no_cpi(
            VICTIM,
            BENEFICIARY,
            ASSET,
            victim_position_q,
            HONEST_PRICE,
            0,
        )
        .map_err(|error| format!("PR 335 {path:?} open independent positions: {error}"))?;
        env.warp_to_slot(2);
        env.push_auth_mark(ASSET, 2, HONEST_PRICE)
            .map_err(|error| format!("PR 335 {path:?} land newer correction: {error}"))?;
        if land_replay {
            replay_cu = env
                .land_retained(retained)
                .map_err(|error| format!("PR 335 {path:?} delayed push no longer lands: {error}"))?
                .compute_units;
        }
    } else {
        env.configure_auth_mark(false, ASSET, 1, HONEST_PRICE)
            .map_err(|error| format!("PR 335 {path:?} land newer correction: {error}"))?;
        if land_replay {
            replay_cu = env
                .land_retained(retained)
                .map_err(|error| {
                    format!("PR 335 {path:?} delayed configuration no longer lands: {error}")
                })?
                .compute_units;
        }
    }
    let stale_mark = env.primary_market_state().1.assets[ASSET as usize].effective_price;
    let expected_stale_mark = if land_replay && path == DelayedOracleIntentPath::ConfigureAuth {
        STALE_PRICE
    } else {
        HONEST_PRICE
    };
    if stale_mark != expected_stale_mark {
        return Err(format!(
            "PR 335 {path:?} pre-settlement mark {stale_mark}, expected {expected_stale_mark}"
        ));
    }

    if path == DelayedOracleIntentPath::ConfigureAuth {
        env.trade_no_cpi(
            VICTIM,
            BENEFICIARY,
            ASSET,
            victim_position_q,
            HONEST_PRICE,
            0,
        )
        .map_err(|error| format!("PR 335 {path:?} open independent positions: {error}"))?;
        env.warp_to_slot(2);
        env.push_auth_mark(ASSET, 2, HONEST_PRICE)
            .map_err(|error| format!("PR 335 {path:?} restore honest mark: {error}"))?;
    }

    let mut max_crank_cu = 0;
    for actor in [VICTIM, BENEFICIARY] {
        max_crank_cu = max_crank_cu.max(crank_delayed_oracle_actor(
            &mut env,
            actor,
            2,
            ASSET,
            &format!("PR 335 {path:?}"),
        )?);
    }
    let restored_mark = env.primary_market_state().1.assets[ASSET as usize].effective_price;
    let expected_restored_mark = if land_replay && path == DelayedOracleIntentPath::PushAuth {
        STALE_PRICE
    } else {
        HONEST_PRICE
    };
    if restored_mark != expected_restored_mark {
        return Err(format!(
            "PR 335 {path:?} settled mark {restored_mark}, expected {expected_restored_mark}"
        ));
    }

    env.trade_no_cpi(
        VICTIM,
        EXIT_MAKER_INDEX,
        ASSET,
        -victim_position_q,
        restored_mark,
        0,
    )
    .map_err(|error| format!("PR 335 {path:?} victim close: {error}"))?;
    env.trade_no_cpi(
        BENEFICIARY,
        EXIT_MAKER_INDEX,
        ASSET,
        victim_position_q,
        restored_mark,
        0,
    )
    .map_err(|error| format!("PR 335 {path:?} beneficiary close: {error}"))?;
    for actor in [VICTIM, BENEFICIARY] {
        let pnl = env.primary_portfolio(actor).pnl.get();
        if pnl > 0 {
            env.convert_released_pnl(actor, pnl as u128)
                .map_err(|error| format!("PR 335 {path:?} convert actor {actor}: {error}"))?;
        }
        let capital = env.primary_portfolio(actor).capital.get();
        env.withdraw_primary(actor, capital)
            .map_err(|error| format!("PR 335 {path:?} withdraw actor {actor}: {error}"))?;
    }
    let victim_payout = env.token_amount(env.actors[VICTIM].destination_token);
    let beneficiary_payout = env.token_amount(env.actors[BENEFICIARY].destination_token);
    if u128::from(victim_payout) + u128::from(beneficiary_payout) != 2 * DEPOSIT
        || env.token_supply_observed() != supply_before
        || replay_cu >= TX_CU_LIMIT
        || max_crank_cu >= TX_CU_LIMIT
    {
        return Err(format!(
            "PR 335 {path:?} terminal mismatch: payout={victim_payout}/{beneficiary_payout}, \
             replay_cu={replay_cu}, crank_cu={max_crank_cu}, supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }

    Ok(DelayedOracleIntentWorld {
        stale_mark,
        restored_mark,
        victim_payout,
        beneficiary_payout,
        replay_cu,
        max_crank_cu,
    })
}

pub fn reproduce_delayed_oracle_intent_replay(
    mut seed: [u8; 32],
    path: DelayedOracleIntentPath,
) -> Result<DelayedOracleIntentReplayReproduction, String> {
    seed[0] ^= match path {
        DelayedOracleIntentPath::PushAuth => 0x35,
        DelayedOracleIntentPath::ConfigureAuth => 0x53,
    };
    let control = run_delayed_oracle_intent_world(seed, path, false)?;
    let replay = run_delayed_oracle_intent_world(seed, path, true)?;
    let victim_loss = control
        .victim_payout
        .checked_sub(replay.victim_payout)
        .ok_or("PR 335 replay increased victim payout")?;
    let beneficiary_gain = replay
        .beneficiary_payout
        .checked_sub(control.beneficiary_payout)
        .ok_or("PR 335 replay decreased beneficiary payout")?;
    if control.victim_payout != 1_000_000
        || control.beneficiary_payout != 1_000_000
        || victim_loss == 0
        || victim_loss != beneficiary_gain
        || replay.replay_cu == 0
    {
        return Err(format!(
            "PR 335 {path:?} paired-world mismatch: control={control:?}, replay={replay:?}, \
             victim_loss={victim_loss}, beneficiary_gain={beneficiary_gain}"
        ));
    }
    Ok(DelayedOracleIntentReplayReproduction {
        blocker: KnownBlocker::DelayedOracleIntentReplay,
        path,
        stale_mark: replay.stale_mark,
        restored_mark: replay.restored_mark,
        victim_loss,
        beneficiary_gain,
        replay_cu: replay.replay_cu,
        max_crank_cu: replay.max_crank_cu,
    })
}

#[derive(Clone, Copy, Debug)]
struct BackingFeeConsentWorld {
    provider_withdrawn: u64,
    operator_withdrawn: u64,
    charged_fee: u64,
    replay_cu: u64,
    trade_cu: u64,
    max_cu: u64,
}

fn run_backing_fee_consent_world(
    seed: [u8; 32],
    order: BackingFeeConsentOrder,
    land_replay: bool,
) -> Result<BackingFeeConsentWorld, String> {
    const MARKET_TRADER: usize = 0;
    const PROVIDER: usize = 1;
    const POLICY_AUTHORITY: usize = 2;
    const OPERATOR: usize = 3;
    const LP: usize = EXIT_MAKER_INDEX;
    const ASSET: u16 = 1;
    const WINNING_DOMAIN: u16 = ASSET * 2 + 1;
    const INITIAL_PRICE: u64 = 100;
    const BACKING_FEE_BPS: u16 = 5_000;
    const WINNING_SIZE_Q: i128 = 200 * POS_SCALE as i128;
    const LOSING_SIZE_Q: i128 = 100 * POS_SCALE as i128;
    const INCREASE_Q: i128 = 20 * POS_SCALE as i128;
    const BACKING_PRINCIPAL: u128 = 5_000;
    const EXPECTED_FEE: u64 = 70;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: INITIAL_PRICE,
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
    let mut max_cu = 0;
    for (kind, actor) in [
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
        let handoff = env
            .update_asset_authority_from_admin(ASSET, kind, actor)
            .map_err(|error| format!("PR 339 authority handoff {kind}: {error}"))?;
        max_cu = max_cu.max(handoff.compute_units);
    }

    let mut stale_policy = Some(env.build_retained_backing_fee_policy_for_actor(
        POLICY_AUTHORITY,
        WINNING_DOMAIN,
        BACKING_FEE_BPS,
        10_000,
    ));
    let correction = env
        .update_backing_fee_policy_for_actor(POLICY_AUTHORITY, WINNING_DOMAIN, BACKING_FEE_BPS, 0)
        .map_err(|error| format!("PR 339 accepted provider split: {error}"))?;
    max_cu = max_cu.max(correction.compute_units);
    let retained_top_up = env.build_retained_backing_bucket_top_up_for_actor(
        PROVIDER,
        WINNING_DOMAIN,
        BACKING_PRINCIPAL,
        100,
    );
    let retained_trade = env.build_retained_no_cpi_trade(OPERATOR, LP, 0, -INCREASE_Q, 95);
    let mut replay_cu = 0;

    match order {
        BackingFeeConsentOrder::FundedThenPolicy => {
            let top_up = env
                .top_up_backing_bucket_for_actor(PROVIDER, WINNING_DOMAIN, BACKING_PRINCIPAL, 100)
                .map_err(|error| format!("PR 339 provider top-up: {error}"))?;
            max_cu = max_cu.max(top_up.compute_units);
        }
        BackingFeeConsentOrder::PolicyThenTopUp => {
            if land_replay {
                let replay = env
                    .land_retained(
                        stale_policy
                            .take()
                            .ok_or("PR 339 stale policy already consumed")?,
                    )
                    .map_err(|error| format!("PR 339 stale pre-top-up split: {error}"))?;
                replay_cu = replay.compute_units;
                max_cu = max_cu.max(replay.compute_units);
            }
            let top_up = env
                .land_retained(retained_top_up)
                .map_err(|error| format!("PR 339 retained provider top-up: {error}"))?;
            max_cu = max_cu.max(top_up.compute_units);
        }
    }

    env.trade_no_cpi(MARKET_TRADER, LP, ASSET, -WINNING_SIZE_Q, INITIAL_PRICE, 0)
        .map_err(|error| format!("PR 339 establish LP winning leg: {error}"))?;
    env.trade_no_cpi(MARKET_TRADER, LP, 0, -LOSING_SIZE_Q, INITIAL_PRICE, 0)
        .map_err(|error| format!("PR 339 establish LP losing leg: {error}"))?;
    env.warp_to_slot(2);
    env.push_auth_mark_for_actor(POLICY_AUTHORITY, ASSET, 2, INITIAL_PRICE)
        .map_err(|error| format!("PR 339 prime winning mark: {error}"))?;
    env.push_auth_mark(0, 2, INITIAL_PRICE)
        .map_err(|error| format!("PR 339 prime base mark: {error}"))?;
    for (actor, asset) in [
        (MARKET_TRADER, ASSET),
        (LP, ASSET),
        (MARKET_TRADER, 0),
        (LP, 0),
    ] {
        crank_adapter_steps(&mut env, actor, 2, asset, 4)
            .map_err(|error| format!("PR 339 prime settlement: {error}"))?;
    }
    env.sync_maintenance_fee(LP, 2)
        .map_err(|error| format!("PR 339 sync LP maintenance fee: {error}"))?;

    env.warp_to_slot(3);
    env.push_auth_mark_for_actor(POLICY_AUTHORITY, ASSET, 3, 105)
        .map_err(|error| format!("PR 339 push winning mark: {error}"))?;
    env.push_auth_mark(0, 3, 95)
        .map_err(|error| format!("PR 339 push losing mark: {error}"))?;
    for (actor, asset) in [(MARKET_TRADER, ASSET), (LP, ASSET), (MARKET_TRADER, 0)] {
        crank_adapter_steps(&mut env, actor, 3, asset, 4)
            .map_err(|error| format!("PR 339 source-backed settlement: {error}"))?;
    }
    if env.primary_portfolio(LP).pnl.get() != 1_000 {
        return Err(format!(
            "PR 339 LP source-backed PnL {}, expected 1000",
            env.primary_portfolio(LP).pnl.get()
        ));
    }

    if order == BackingFeeConsentOrder::FundedThenPolicy && land_replay {
        let replay = env
            .land_retained(
                stale_policy
                    .take()
                    .ok_or("PR 339 stale policy already consumed")?,
            )
            .map_err(|error| format!("PR 339 stale funded-bucket split: {error}"))?;
        replay_cu = replay.compute_units;
        max_cu = max_cu.max(replay.compute_units);
    }
    let before = env.primary_market_state().1;
    let provider_before =
        before.source_backing_buckets[WINNING_DOMAIN as usize].utilization_fee_earnings;
    let insurance_before = before.insurance_domain_budget[WINNING_DOMAIN as usize];
    let lp_before = env.primary_portfolio(LP).capital.get();
    let trade = env
        .land_retained(retained_trade)
        .map_err(|error| format!("PR 339 retained fee-bearing trade: {error}"))?;
    max_cu = max_cu.max(trade.compute_units);
    let after = env.primary_market_state().1;
    let provider_fee = after.source_backing_buckets[WINNING_DOMAIN as usize]
        .utilization_fee_earnings
        .checked_sub(provider_before)
        .ok_or("PR 339 provider earnings decreased")?;
    let insurance_fee = after.insurance_domain_budget[WINNING_DOMAIN as usize]
        .checked_sub(insurance_before)
        .ok_or("PR 339 insurance budget decreased")?;
    let charged_fee = lp_before
        .checked_sub(env.primary_portfolio(LP).capital.get())
        .ok_or("PR 339 trade increased LP capital")?;
    if provider_fee + insurance_fee != u128::from(EXPECTED_FEE)
        || charged_fee != u128::from(EXPECTED_FEE)
    {
        return Err(format!(
            "PR 339 fee mismatch: provider={provider_fee}, insurance={insurance_fee}, \
             charged={charged_fee}"
        ));
    }

    let provider_destination = env.actors[PROVIDER].destination_token;
    let operator_destination = env.actors[OPERATOR].destination_token;
    let provider_destination_before = env.token_amount(provider_destination);
    let operator_destination_before = env.token_amount(operator_destination);
    if provider_fee > 0 {
        let withdrawal = env
            .withdraw_backing_bucket_earnings_for_actor(PROVIDER, WINNING_DOMAIN, provider_fee)
            .map_err(|error| format!("PR 339 provider earnings withdrawal: {error}"))?;
        max_cu = max_cu.max(withdrawal.compute_units);
    }
    if insurance_fee > 0 {
        let withdrawal = env
            .withdraw_insurance_asset(OPERATOR, ASSET, insurance_fee)
            .map_err(|error| format!("PR 339 operator insurance withdrawal: {error}"))?;
        max_cu = max_cu.max(withdrawal.compute_units);
    }
    let provider_withdrawn = env
        .token_amount(provider_destination)
        .checked_sub(provider_destination_before)
        .ok_or("PR 339 provider destination decreased")?;
    let operator_withdrawn = env
        .token_amount(operator_destination)
        .checked_sub(operator_destination_before)
        .ok_or("PR 339 operator destination decreased")?;
    if u128::from(provider_withdrawn) != provider_fee
        || u128::from(operator_withdrawn) != insurance_fee
        || env.token_supply_observed() != supply_before
        || max_cu >= TX_CU_LIMIT
    {
        return Err(format!(
            "PR 339 extraction mismatch: provider={provider_withdrawn}/{provider_fee}, \
             operator={operator_withdrawn}/{insurance_fee}, max_cu={max_cu}, supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }

    Ok(BackingFeeConsentWorld {
        provider_withdrawn,
        operator_withdrawn,
        charged_fee: charged_fee as u64,
        replay_cu,
        trade_cu: trade.compute_units,
        max_cu,
    })
}

pub fn reproduce_backing_fee_consent_replay(
    mut seed: [u8; 32],
    order: BackingFeeConsentOrder,
) -> Result<BackingFeeConsentReplayReproduction, String> {
    seed[0] ^= match order {
        BackingFeeConsentOrder::FundedThenPolicy => 0x39,
        BackingFeeConsentOrder::PolicyThenTopUp => 0x93,
    };
    let control = run_backing_fee_consent_world(seed, order, false)?;
    let replay = run_backing_fee_consent_world(seed, order, true)?;
    let provider_loss = control
        .provider_withdrawn
        .checked_sub(replay.provider_withdrawn)
        .ok_or("PR 339 replay increased provider payout")?;
    let operator_gain = replay
        .operator_withdrawn
        .checked_sub(control.operator_withdrawn)
        .ok_or("PR 339 replay decreased operator payout")?;
    if control.provider_withdrawn != 70
        || control.operator_withdrawn != 0
        || replay.provider_withdrawn != 0
        || replay.operator_withdrawn != 70
        || provider_loss != operator_gain
        || replay.replay_cu == 0
        || control.charged_fee != replay.charged_fee
    {
        return Err(format!(
            "PR 339 {order:?} paired-world mismatch: control={control:?}, replay={replay:?}, \
             provider_loss={provider_loss}, operator_gain={operator_gain}"
        ));
    }
    Ok(BackingFeeConsentReplayReproduction {
        blocker: KnownBlocker::BackingFeeConsentReplay,
        order,
        provider_loss,
        operator_gain,
        charged_fee: replay.charged_fee,
        replay_cu: replay.replay_cu,
        trade_cu: replay.trade_cu,
        max_cu: replay.max_cu.max(control.max_cu),
    })
}

#[derive(Clone, Copy, Debug)]
struct AuthorityHandoffAbaWorld {
    attacker_extraction: u64,
    control_withdrawal_blocked: bool,
    reserve_before: u128,
    reserve_after: u128,
    replay_cu: u64,
    withdrawal_cu: u64,
}

fn authority_aba_reserve(env: &V16Svm, path: AuthorityHandoffAbaPath) -> u128 {
    let group = env.primary_market_state().1;
    match path {
        AuthorityHandoffAbaPath::Market => {
            group.insurance_domain_budget[0] + group.insurance_domain_budget[1]
        }
        AuthorityHandoffAbaPath::AssetInsuranceOperator => group.insurance_domain_budget[2],
    }
}

fn run_authority_handoff_aba_world(
    seed: [u8; 32],
    path: AuthorityHandoffAbaPath,
    land_replay: bool,
) -> Result<AuthorityHandoffAbaWorld, String> {
    const ATTACKER: usize = 0;
    const INTERIM: usize = 1;
    const PROVIDER: usize = 2;
    const ORIGINAL: usize = 3;
    const ASSET: u16 = 1;
    const AMOUNT: u128 = 50_000;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            actor_deposits: [1, 1, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    let retained = match path {
        AuthorityHandoffAbaPath::Market => {
            let retained = env.build_retained_market_authority_handoff_from_admin(ATTACKER);
            env.update_market_authority_from_admin(INTERIM)
                .map_err(|error| format!("PR 345 A-to-C handoff: {error}"))?;
            env.update_market_authority_to_admin(INTERIM)
                .map_err(|error| format!("PR 345 C-to-A handoff: {error}"))?;
            env.top_up_insurance_domain(0, AMOUNT)
                .map_err(|error| format!("PR 345 fresh base-insurance contribution: {error}"))?;
            retained
        }
        AuthorityHandoffAbaPath::AssetInsuranceOperator => {
            env.update_asset_authority_from_admin(
                ASSET,
                percolator_prog::processor::ASSET_AUTH_INSURANCE,
                PROVIDER,
            )
            .map_err(|error| format!("PR 346 install independent provider: {error}"))?;
            env.update_asset_authority_from_admin(
                ASSET,
                percolator_prog::processor::ASSET_AUTH_INSURANCE_OPERATOR,
                ORIGINAL,
            )
            .map_err(|error| format!("PR 346 install original operator: {error}"))?;
            env.burn_asset_admin(ASSET)
                .map_err(|error| format!("PR 346 burn asset admin: {error}"))?;
            if env.primary_profile(ASSET as usize).asset_admin != [0; 32] {
                return Err("PR 346 asset admin remained live".into());
            }
            let retained = env.build_retained_asset_authority_handoff_between_actors(
                ASSET,
                percolator_prog::processor::ASSET_AUTH_INSURANCE_OPERATOR,
                ORIGINAL,
                ATTACKER,
            );
            env.update_asset_authority_between_actors(
                ASSET,
                percolator_prog::processor::ASSET_AUTH_INSURANCE_OPERATOR,
                ORIGINAL,
                INTERIM,
            )
            .map_err(|error| format!("PR 346 A-to-C handoff: {error}"))?;
            env.update_asset_authority_between_actors(
                ASSET,
                percolator_prog::processor::ASSET_AUTH_INSURANCE_OPERATOR,
                INTERIM,
                ORIGINAL,
            )
            .map_err(|error| format!("PR 346 C-to-A handoff: {error}"))?;
            env.top_up_insurance_domain_for_actor(PROVIDER, 2, AMOUNT)
                .map_err(|error| format!("PR 346 independent provider contribution: {error}"))?;
            retained
        }
    };
    let reserve_before = authority_aba_reserve(&env, path);
    if reserve_before != AMOUNT {
        return Err(format!(
            "PR 345/346 {path:?} reserve {reserve_before}, expected {AMOUNT}"
        ));
    }
    let destination = env.actors[ATTACKER].destination_token;
    let destination_before = env.token_amount(destination);
    let (control_withdrawal_blocked, replay_cu, withdrawal_cu) = if land_replay {
        let replay = env
            .land_retained(retained)
            .map_err(|error| format!("PR 345/346 {path:?} retained handoff: {error}"))?;
        let withdrawal = env
            .withdraw_insurance_asset(
                ATTACKER,
                match path {
                    AuthorityHandoffAbaPath::Market => 0,
                    AuthorityHandoffAbaPath::AssetInsuranceOperator => ASSET,
                },
                AMOUNT,
            )
            .map_err(|error| format!("PR 345/346 {path:?} attacker withdrawal: {error}"))?;
        (false, replay.compute_units, withdrawal.compute_units)
    } else {
        let blocked = env
            .withdraw_insurance_asset(
                ATTACKER,
                match path {
                    AuthorityHandoffAbaPath::Market => 0,
                    AuthorityHandoffAbaPath::AssetInsuranceOperator => ASSET,
                },
                AMOUNT,
            )
            .is_err();
        if !blocked {
            return Err(format!(
                "PR 345/346 {path:?} control attacker withdrawal succeeded"
            ));
        }
        (true, 0, 0)
    };
    let attacker_extraction = env
        .token_amount(destination)
        .checked_sub(destination_before)
        .ok_or("PR 345/346 attacker destination decreased")?;
    let reserve_after = authority_aba_reserve(&env, path);
    if env.token_supply_observed() != supply_before {
        return Err(format!(
            "PR 345/346 {path:?} changed SPL supply: {}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }
    Ok(AuthorityHandoffAbaWorld {
        attacker_extraction,
        control_withdrawal_blocked,
        reserve_before,
        reserve_after,
        replay_cu,
        withdrawal_cu,
    })
}

pub fn reproduce_authority_handoff_aba_replay(
    mut seed: [u8; 32],
    path: AuthorityHandoffAbaPath,
) -> Result<AuthorityHandoffAbaReplayReproduction, String> {
    seed[0] ^= match path {
        AuthorityHandoffAbaPath::Market => 0x45,
        AuthorityHandoffAbaPath::AssetInsuranceOperator => 0x46,
    };
    let control = run_authority_handoff_aba_world(seed, path, false)?;
    let replay = run_authority_handoff_aba_world(seed, path, true)?;
    if !control.control_withdrawal_blocked
        || control.attacker_extraction != 0
        || control.reserve_after != 50_000
        || replay.attacker_extraction != 50_000
        || replay.reserve_after != 0
        || replay.replay_cu == 0
        || replay.withdrawal_cu == 0
        || control.reserve_before != replay.reserve_before
    {
        return Err(format!(
            "PR 345/346 {path:?} paired-world mismatch: control={control:?}, replay={replay:?}"
        ));
    }
    Ok(AuthorityHandoffAbaReplayReproduction {
        blocker: KnownBlocker::AuthorityHandoffAbaReplay,
        path,
        attacker_extraction: replay.attacker_extraction,
        control_withdrawal_blocked: control.control_withdrawal_blocked,
        reserve_before: replay.reserve_before,
        reserve_after: replay.reserve_after,
        replay_cu: replay.replay_cu,
        withdrawal_cu: replay.withdrawal_cu,
    })
}

#[derive(Clone, Copy, Debug)]
struct DelayedResolvePolicyWorld {
    victim_payout: u64,
    attacker_payout: u64,
    unsafe_resolve_rejected: bool,
    rejected_exact_rollback: bool,
    catchup_steps: u16,
    max_crank_cu: u64,
    settlement_price: u64,
    replay_cu: u64,
    resolve_cu: u64,
}

fn run_delayed_resolve_policy_world(
    seed: [u8; 32],
    land_replay: bool,
) -> Result<DelayedResolvePolicyWorld, String> {
    const VICTIM: usize = 0;
    const ATTACKER: usize = 1;
    const PRICE: u64 = 100;
    const TARGET: u64 = 110;
    const SIZE_Q: i128 = 10_000 * POS_SCALE as i128;
    const DEPOSIT: u128 = 1_000_000;
    const OLD_STALE_SLOTS: u64 = 2;
    const CURRENT_STALE_SLOTS: u64 = 100;
    const FORCE_CLOSE_DELAY: u64 = 5;
    const TARGET_SLOT: u64 = 2;
    const RESOLVE_SLOT: u64 = 4;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [DEPOSIT, DEPOSIT, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.trade_no_cpi(VICTIM, ATTACKER, 0, SIZE_Q, PRICE, 0)
        .map_err(|error| format!("PR 347 open independent long/short: {error}"))?;
    let retained_policy =
        env.build_retained_permissionless_resolve_policy(OLD_STALE_SLOTS, FORCE_CLOSE_DELAY);
    env.configure_permissionless_resolve(OLD_STALE_SLOTS, FORCE_CLOSE_DELAY)
        .map_err(|error| format!("PR 347 land initial short policy: {error}"))?;
    env.configure_permissionless_resolve(CURRENT_STALE_SLOTS, FORCE_CLOSE_DELAY)
        .map_err(|error| format!("PR 347 land long-window correction: {error}"))?;
    env.warp_to_slot(TARGET_SLOT);
    env.push_auth_mark(0, TARGET_SLOT, TARGET)
        .map_err(|error| format!("PR 347 commit authenticated target: {error}"))?;
    let before_replay = env.primary_market_state();
    if before_replay.0.mark_ewma_e6 != TARGET
        || before_replay.1.assets[0].effective_price != PRICE
        || before_replay.0.permissionless_resolve_stale_slots != CURRENT_STALE_SLOTS
    {
        return Err(format!(
            "PR 347 target staging mismatch: wrapper={}, effective={}, stale={}",
            before_replay.0.mark_ewma_e6,
            before_replay.1.assets[0].effective_price,
            before_replay.0.permissionless_resolve_stale_slots
        ));
    }
    env.warp_to_slot(RESOLVE_SLOT);

    let mut replay_cu = 0;
    let mut unsafe_resolve_rejected = false;
    let mut rejected_exact_rollback = false;
    let mut catchup_steps = 0u16;
    let mut max_crank_cu = 0;
    let resolve_cu;
    if land_replay {
        let replay = env
            .land_retained(retained_policy)
            .map_err(|error| format!("PR 347 delayed short policy no longer lands: {error}"))?;
        replay_cu = replay.compute_units;
        let before_rejection = tracked_economic_accounts(&env);
        let initial_resolve = env.resolve_stale_permissionless(RESOLVE_SLOT);
        unsafe_resolve_rejected = matches!(
            &initial_resolve,
            Err(error)
                if error.contains("Custom(19)")
                    || error.contains("custom program error: 0x13")
        );
        rejected_exact_rollback = tracked_economic_accounts(&env) == before_rejection;
        if !unsafe_resolve_rejected || !rejected_exact_rollback {
            return Err(format!(
                "PR 347 stale-policy resolve did not reject and roll back exactly: \
                 result={initial_resolve:?}"
            ));
        }
        let mut landed_resolve_cu = None;
        for step in 0..16 {
            let catchup = env
                .crank(
                    VICTIM,
                    RESOLVE_SLOT,
                    vec![CrankObservationHint {
                        asset_index: 0,
                        oracle_accounts: 0,
                    }],
                )
                .map_err(|error| format!("PR 347 public catch-up crank {step}: {error}"))?;
            catchup_steps = catchup_steps
                .checked_add(1)
                .ok_or("PR 347 catch-up step overflow")?;
            max_crank_cu = max_crank_cu.max(catchup.compute_units);
            let before_retry = tracked_economic_accounts(&env);
            match env.resolve_stale_permissionless(RESOLVE_SLOT) {
                Ok(resolve) => {
                    landed_resolve_cu = Some(resolve.compute_units);
                    break;
                }
                Err(error)
                    if error.contains("Custom(19)")
                        || error.contains("custom program error: 0x13") =>
                {
                    rejected_exact_rollback &= tracked_economic_accounts(&env) == before_retry;
                }
                Err(error) => {
                    return Err(format!(
                        "PR 347 stale-policy resolve retry returned unexpected error: {error}"
                    ));
                }
            }
        }
        resolve_cu = landed_resolve_cu
            .ok_or("PR 347 stale-policy resolve did not land after bounded public catch-up")?;
    } else {
        for actor in [VICTIM, ATTACKER] {
            let crank = env
                .crank(
                    actor,
                    RESOLVE_SLOT,
                    vec![CrankObservationHint {
                        asset_index: 0,
                        oracle_accounts: 0,
                    }],
                )
                .map_err(|error| format!("PR 347 control crank actor {actor}: {error}"))?;
            max_crank_cu = max_crank_cu.max(crank.compute_units);
        }
        resolve_cu = env
            .resolve_market()
            .map_err(|error| format!("PR 347 control terminal resolve: {error}"))?
            .compute_units;
    }
    let settlement_price = env.primary_market_state().1.assets[0].effective_price;
    env.warp_to_slot(RESOLVE_SLOT + FORCE_CLOSE_DELAY);
    let (attacker_payout, _) = drain_resolved_actor(&mut env, ATTACKER)?;
    let (victim_payout, _) = drain_resolved_actor(&mut env, VICTIM)?;
    let victim_payout =
        u64::try_from(victim_payout).map_err(|_| "PR 347 victim payout exceeds SPL range")?;
    let attacker_payout =
        u64::try_from(attacker_payout).map_err(|_| "PR 347 attacker payout exceeds SPL range")?;
    if u128::from(victim_payout) + u128::from(attacker_payout) != 2 * DEPOSIT
        || env.token_supply_observed() != supply_before
        || replay_cu >= TX_CU_LIMIT
        || resolve_cu >= TX_CU_LIMIT
    {
        return Err(format!(
            "PR 347 terminal mismatch: payout={victim_payout}/{attacker_payout}, \
             replay_cu={replay_cu}, resolve_cu={resolve_cu}, supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }
    Ok(DelayedResolvePolicyWorld {
        victim_payout,
        attacker_payout,
        unsafe_resolve_rejected,
        rejected_exact_rollback,
        catchup_steps,
        max_crank_cu,
        settlement_price,
        replay_cu,
        resolve_cu,
    })
}

pub fn reproduce_delayed_resolve_policy_replay(
    mut seed: [u8; 32],
) -> Result<DelayedResolvePolicyReplayReproduction, String> {
    seed[0] ^= 0x47;
    let control = run_delayed_resolve_policy_world(seed, false)?;
    let replay = run_delayed_resolve_policy_world(seed, true)?;
    let victim_loss = control
        .victim_payout
        .checked_sub(replay.victim_payout)
        .ok_or("PR 347 replay increased victim payout")?;
    let attacker_gain = replay
        .attacker_payout
        .checked_sub(control.attacker_payout)
        .ok_or("PR 347 replay decreased attacker payout")?;
    if control.victim_payout != 1_100_000
        || control.attacker_payout != 900_000
        || replay.victim_payout != control.victim_payout
        || replay.attacker_payout != control.attacker_payout
        || victim_loss != attacker_gain
        || victim_loss != 0
        || !replay.unsafe_resolve_rejected
        || !replay.rejected_exact_rollback
        || replay.catchup_steps == 0
        || replay.catchup_steps > 16
        || control.settlement_price != 110
        || replay.settlement_price != control.settlement_price
    {
        return Err(format!(
            "PR 347 paired-world mismatch: control={control:?}, replay={replay:?}, \
             victim_loss={victim_loss}, attacker_gain={attacker_gain}"
        ));
    }
    Ok(DelayedResolvePolicyReplayReproduction {
        blocker: KnownBlocker::DelayedResolvePolicyReplay,
        victim_loss,
        attacker_gain,
        unsafe_resolve_rejected: replay.unsafe_resolve_rejected,
        rejected_exact_rollback: replay.rejected_exact_rollback,
        catchup_steps: replay.catchup_steps,
        max_crank_cu: replay.max_crank_cu.max(control.max_crank_cu),
        replay_price: replay.settlement_price,
        control_price: control.settlement_price,
        replay_cu: replay.replay_cu,
        resolve_cu: replay.resolve_cu,
    })
}

#[derive(Clone, Copy, Debug)]
struct ResolveAuthorityIncarnationWorld {
    victim_payout: u64,
    winner_payout: u64,
    settlement_price: u64,
    replay_cu: u64,
    max_crank_cu: u64,
}

fn run_resolve_authority_incarnation_world(
    seed: [u8; 32],
    land_replay: bool,
) -> Result<ResolveAuthorityIncarnationWorld, String> {
    const WINNER: usize = 0;
    const VICTIM: usize = 1;
    const INTERIM: usize = 2;
    const PRICE: u64 = 100;
    const ADVERSE_PRICE: u64 = 110;
    const SIZE_Q: i128 = 10_000 * POS_SCALE as i128;
    const DEPOSIT: u128 = 1_000_000;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [DEPOSIT, DEPOSIT, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    let retained_resolve = env.build_retained_resolve_market();
    env.update_market_authority_from_admin(INTERIM)
        .map_err(|error| format!("PR 353 A-to-C handoff: {error}"))?;
    env.update_market_authority_to_admin(INTERIM)
        .map_err(|error| format!("PR 353 C-to-A handoff: {error}"))?;
    env.trade_no_cpi(WINNER, VICTIM, 0, SIZE_Q, PRICE, 0)
        .map_err(|error| format!("PR 353 open independent winner/victim OI: {error}"))?;
    env.warp_to_slot(10);
    env.push_auth_mark(0, 10, ADVERSE_PRICE)
        .map_err(|error| format!("PR 353 push temporary adverse mark: {error}"))?;
    let mut max_crank_cu =
        crank_market_then_accounts_once(&mut env, INTERIM, &[VICTIM, WINNER], 10, 0, 16)
            .map_err(|error| format!("PR 353 settle adverse mark: {error}"))?;
    if env.primary_market_state().1.assets[0].effective_price != ADVERSE_PRICE {
        return Err("PR 353 adverse mark did not become effective".into());
    }
    let replay_cu;
    let settlement_price;
    if land_replay {
        replay_cu = env
            .land_retained(retained_resolve)
            .map_err(|error| format!("PR 353 prior-incarnation resolve no longer lands: {error}"))?
            .compute_units;
        settlement_price = env.primary_market_state().1.assets[0].effective_price;
        env.warp_to_slot(11);
    } else {
        env.warp_to_slot(12);
        env.push_auth_mark(0, 12, PRICE)
            .map_err(|error| format!("PR 353 restore honest mark: {error}"))?;
        let boundary_cu =
            crank_market_then_accounts_once(&mut env, INTERIM, &[VICTIM, WINNER], 12, 0, 16)
                .map_err(|error| format!("PR 353 settle restored-mark boundary: {error}"))?;
        max_crank_cu = max_crank_cu.max(boundary_cu);
        env.warp_to_slot(13);
        let restored_cu =
            crank_market_then_accounts_once(&mut env, INTERIM, &[VICTIM, WINNER], 13, 0, 16)
                .map_err(|error| format!("PR 353 settle restored mark: {error}"))?;
        max_crank_cu = max_crank_cu.max(restored_cu);
        if env.primary_market_state().1.assets[0].effective_price != PRICE {
            return Err("PR 353 restored mark did not commit within bounded public cranks".into());
        }
        settlement_price = env.primary_market_state().1.assets[0].effective_price;
        env.resolve_market()
            .map_err(|error| format!("PR 353 current-incarnation resolve: {error}"))?;
        env.warp_to_slot(14);
        replay_cu = 0;
    }
    let (victim_payout, winner_payout) = if land_replay {
        let (victim_payout, _) = drain_resolved_actor(&mut env, VICTIM)?;
        let (winner_payout, _) = drain_resolved_actor(&mut env, WINNER)?;
        (victim_payout, winner_payout)
    } else {
        let (winner_payout, _) = drain_resolved_actor(&mut env, WINNER)?;
        let (victim_payout, _) = drain_resolved_actor(&mut env, VICTIM)?;
        (victim_payout, winner_payout)
    };
    let victim_payout =
        u64::try_from(victim_payout).map_err(|_| "PR 353 victim payout exceeds SPL range")?;
    let winner_payout =
        u64::try_from(winner_payout).map_err(|_| "PR 353 winner payout exceeds SPL range")?;
    if u128::from(victim_payout) + u128::from(winner_payout) != 2 * DEPOSIT
        || env.token_supply_observed() != supply_before
        || replay_cu >= TX_CU_LIMIT
        || max_crank_cu >= TX_CU_LIMIT
    {
        return Err(format!(
            "PR 353 terminal mismatch: payout={victim_payout}/{winner_payout}, \
             replay_cu={replay_cu}, crank_cu={max_crank_cu}, supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }
    Ok(ResolveAuthorityIncarnationWorld {
        victim_payout,
        winner_payout,
        settlement_price,
        replay_cu,
        max_crank_cu,
    })
}

pub fn reproduce_resolve_authority_incarnation_replay(
    mut seed: [u8; 32],
) -> Result<ResolveAuthorityIncarnationReplayReproduction, String> {
    seed[0] ^= 0x53;
    let control = run_resolve_authority_incarnation_world(seed, false)?;
    let replay = run_resolve_authority_incarnation_world(seed, true)?;
    let victim_loss = control
        .victim_payout
        .checked_sub(replay.victim_payout)
        .ok_or("PR 353 replay increased victim payout")?;
    let winner_gain = replay
        .winner_payout
        .checked_sub(control.winner_payout)
        .ok_or("PR 353 replay decreased winner payout")?;
    if control.victim_payout != 1_000_000
        || control.winner_payout != 1_000_000
        || replay.victim_payout != 900_000
        || replay.winner_payout != 1_100_000
        || victim_loss != winner_gain
        || control.settlement_price != 100
        || replay.settlement_price != 110
        || replay.replay_cu == 0
    {
        return Err(format!(
            "PR 353 paired-world mismatch: control={control:?}, replay={replay:?}, \
             victim_loss={victim_loss}, winner_gain={winner_gain}"
        ));
    }
    Ok(ResolveAuthorityIncarnationReplayReproduction {
        blocker: KnownBlocker::ResolveAuthorityIncarnationReplay,
        victim_loss,
        winner_gain,
        replay_price: replay.settlement_price,
        control_price: control.settlement_price,
        replay_cu: replay.replay_cu,
        max_crank_cu: replay.max_crank_cu.max(control.max_crank_cu),
    })
}

#[derive(Clone, Copy, Debug)]
struct PortfolioCloseIncarnationWorld {
    original_portfolio_id: u64,
    replacement_portfolio_id: u64,
    replacement_lamports_before: u64,
    replacement_lamports_after: u64,
    market_lamport_gain: u64,
    replay_cu: u64,
}

fn run_portfolio_close_incarnation_world(
    seed: [u8; 32],
    land_replay: bool,
) -> Result<PortfolioCloseIncarnationWorld, String> {
    const VICTIM: usize = 0;
    const REPLACEMENT_FUNDING: u64 = 1_000_000_000;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            actor_deposits: [1, 1, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.withdraw_primary(VICTIM, 1)
        .map_err(|error| format!("PR 309 empty incarnation A: {error}"))?;
    let original_portfolio_id = env.primary_portfolio_id(VICTIM);
    let retained_close = env.build_retained_close_primary_portfolio(VICTIM);
    env.close_primary_portfolio(VICTIM)
        .map_err(|error| format!("PR 309 close incarnation A: {error}"))?;
    env.fund_closed_primary_portfolio(VICTIM, REPLACEMENT_FUNDING)
        .map_err(|error| format!("PR 309 fund replacement account: {error}"))?;
    env.reinitialize_primary_portfolio(VICTIM)
        .map_err(|error| format!("PR 309 initialize incarnation B: {error}"))?;
    let replacement_portfolio_id = env.primary_portfolio_id(VICTIM);
    if replacement_portfolio_id <= original_portfolio_id {
        return Err(format!(
            "PR 309 portfolio ID did not advance: {original_portfolio_id}->{replacement_portfolio_id}"
        ));
    }
    let portfolio = env.actors[VICTIM].portfolio;
    let replacement_lamports_before = env.account_lamports(portfolio);
    let market_lamports_before = env.account_lamports(env.market);
    let replay_cu = if land_replay {
        env.land_retained(retained_close)
            .map_err(|error| format!("PR 309 stale close no longer lands: {error}"))?
            .compute_units
    } else {
        0
    };
    let replacement_lamports_after = env.account_lamports(portfolio);
    let market_lamport_gain = env
        .account_lamports(env.market)
        .checked_sub(market_lamports_before)
        .ok_or("PR 309 market lamports decreased")?;
    if replacement_lamports_before < REPLACEMENT_FUNDING
        || env.token_supply_observed() != supply_before
        || replay_cu >= TX_CU_LIMIT
    {
        return Err(format!(
            "PR 309 lifecycle mismatch: replacement_lamports={replacement_lamports_before}, \
             replay_cu={replay_cu}, supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }
    Ok(PortfolioCloseIncarnationWorld {
        original_portfolio_id,
        replacement_portfolio_id,
        replacement_lamports_before,
        replacement_lamports_after,
        market_lamport_gain,
        replay_cu,
    })
}

pub fn reproduce_portfolio_close_incarnation_replay(
    mut seed: [u8; 32],
) -> Result<PortfolioCloseIncarnationReplayReproduction, String> {
    seed[0] ^= 0x09;
    let control = run_portfolio_close_incarnation_world(seed, false)?;
    let replay = run_portfolio_close_incarnation_world(seed, true)?;
    let drained_lamports = control
        .replacement_lamports_after
        .checked_sub(replay.replacement_lamports_after)
        .ok_or("PR 309 replay increased replacement lamports")?;
    if control.original_portfolio_id != replay.original_portfolio_id
        || control.replacement_portfolio_id != replay.replacement_portfolio_id
        || control.replacement_lamports_before != control.replacement_lamports_after
        || replay.replacement_lamports_after != 0
        || drained_lamports != replay.replacement_lamports_before
        || replay.market_lamport_gain != drained_lamports
        || control.market_lamport_gain != 0
        || replay.replay_cu == 0
    {
        return Err(format!(
            "PR 309 paired-world mismatch: control={control:?}, replay={replay:?}, \
             drained={drained_lamports}"
        ));
    }
    Ok(PortfolioCloseIncarnationReplayReproduction {
        blocker: KnownBlocker::PortfolioCloseIncarnationReplay,
        original_portfolio_id: replay.original_portfolio_id,
        replacement_portfolio_id: replay.replacement_portfolio_id,
        drained_lamports,
        market_lamport_gain: replay.market_lamport_gain,
        replay_cu: replay.replay_cu,
    })
}

#[derive(Clone, Copy, Debug)]
struct MatcherGrantPortfolioIncarnationWorld {
    original_portfolio_id: u64,
    replacement_portfolio_id: u64,
    stale_replay_rejected: bool,
    rejected_exact_rollback: bool,
    control_trade_blocked: bool,
    fresh_grant_landed: bool,
    fresh_round_trip_landed: bool,
    owner_exit_landed: bool,
    max_cu: u64,
}

#[derive(Clone, Copy, Debug)]
struct MatcherGrantExploitOutcome {
    control_trade_blocked: bool,
    liquidation_slot: u64,
    cranker_reward: u128,
    extracted_reward: u64,
    replay_cu: u64,
    max_cu: u64,
}

fn exercise_matcher_grant_replay(
    env: &mut V16Svm,
    retained_grant: Transaction,
    land_replay: bool,
    first_mark_slot: u64,
    context: &str,
) -> Result<MatcherGrantExploitOutcome, String> {
    const LONG: usize = 0;
    const VICTIM: usize = 1;
    const CRANKER: usize = 2;
    const POSITION_Q: i128 = 1_000 * POS_SCALE as i128;

    let supply_before = env.token_supply_observed();
    let mut replay_cu = 0;
    let mut max_cu = 0;
    let control_trade_blocked;
    if land_replay {
        let replay = env
            .land_retained(retained_grant)
            .map_err(|error| format!("{context} stale matcher grant no longer lands: {error}"))?;
        replay_cu = replay.compute_units;
        max_cu = max_cu.max(replay.compute_units);
        let trade = env
            .trade_cpi(LONG, VICTIM, 0, POSITION_Q, 0, 0)
            .map_err(|error| format!("{context} fresh unsigned trade against B: {error}"))?;
        max_cu = max_cu.max(trade.compute_units);
        control_trade_blocked = false;
    } else {
        let market_before = env.market_data(false);
        let long_before = env.primary_portfolio_data(LONG);
        let victim_before = env.primary_portfolio_data(VICTIM);
        let matcher_before = env.all_matcher_context_data();
        control_trade_blocked = env.trade_cpi(LONG, VICTIM, 0, POSITION_Q, 0, 0).is_err();
        if !control_trade_blocked
            || env.market_data(false) != market_before
            || env.primary_portfolio_data(LONG) != long_before
            || env.primary_portfolio_data(VICTIM) != victim_before
            || env.all_matcher_context_data() != matcher_before
            || env.token_supply_observed() != supply_before
        {
            return Err(format!(
                "{context} disabled control matcher did not reject atomically"
            ));
        }
        return Ok(MatcherGrantExploitOutcome {
            control_trade_blocked,
            liquidation_slot: 0,
            cranker_reward: 0,
            extracted_reward: 0,
            replay_cu,
            max_cu,
        });
    }
    if position_for_asset(&env.primary_portfolio(VICTIM), 0)? != -POSITION_Q {
        return Err(format!(
            "{context} fresh CPI trade did not install B's short"
        ));
    }

    let mut liquidation_slot = 0;
    for offset in 0..30u64 {
        let slot = first_mark_slot
            .checked_add(offset)
            .ok_or_else(|| format!("{context} liquidation slot overflow"))?;
        env.warp_to_slot(slot);
        let current_mark = env.primary_market_state().1.assets[0].effective_price;
        let next_mark = current_mark
            .checked_add((current_mark / 500).max(1))
            .ok_or_else(|| format!("{context} mark overflow"))?;
        env.push_auth_mark(0, slot, next_mark)
            .map_err(|error| format!("{context} publish mark at slot {slot}: {error}"))?;
        let crank = env
            .crank(
                VICTIM,
                slot,
                vec![CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                }],
            )
            .map_err(|error| format!("{context} refresh B at slot {slot}: {error}"))?;
        max_cu = max_cu.max(crank.compute_units);
        let cert = env
            .primary_portfolio(VICTIM)
            .health_cert
            .try_to_runtime()
            .map_err(|error| format!("{context} decode health certificate: {error:?}"))?;
        let maintenance = i128::try_from(cert.certified_maintenance_req)
            .map_err(|_| format!("{context} maintenance requirement exceeds signed range"))?;
        if cert.certified_equity < maintenance {
            liquidation_slot = slot;
            break;
        }
    }
    if liquidation_slot == 0 {
        return Err(format!(
            "{context} replayed matcher trade never made B liquidatable"
        ));
    }
    let cranker_before = env.primary_portfolio(CRANKER).capital.get();
    let liquidation = env
        .crank_with_reward(CRANKER, VICTIM, liquidation_slot, Vec::new(), &[])
        .map_err(|error| format!("{context} independent liquidation: {error}"))?;
    max_cu = max_cu.max(liquidation.compute_units);
    let cranker_reward = env
        .primary_portfolio(CRANKER)
        .capital
        .get()
        .checked_sub(cranker_before)
        .ok_or_else(|| format!("{context} cranker capital decreased"))?;
    if cranker_reward == 0 {
        return Err(format!("{context} liquidation paid no reward"));
    }
    let destination = env.actors[CRANKER].destination_token;
    let destination_before = env.token_amount(destination);
    let withdrawal = env
        .withdraw_primary(CRANKER, cranker_reward)
        .map_err(|error| format!("{context} withdraw liquidation reward: {error}"))?;
    max_cu = max_cu.max(withdrawal.compute_units);
    let extracted_reward = env
        .token_amount(destination)
        .checked_sub(destination_before)
        .ok_or_else(|| format!("{context} cranker destination decreased"))?;
    if u128::from(extracted_reward) != cranker_reward
        || env.token_supply_observed() != supply_before
        || max_cu >= TX_CU_LIMIT
    {
        return Err(format!(
            "{context} extraction mismatch: reward={cranker_reward}, \
             extracted={extracted_reward}, max_cu={max_cu}, supply={}/{supply_before}",
            env.token_supply_observed()
        ));
    }
    Ok(MatcherGrantExploitOutcome {
        control_trade_blocked,
        liquidation_slot,
        cranker_reward,
        extracted_reward,
        replay_cu,
        max_cu,
    })
}

fn run_matcher_grant_portfolio_incarnation_world(
    seed: [u8; 32],
) -> Result<MatcherGrantPortfolioIncarnationWorld, String> {
    const LONG: usize = 0;
    const VICTIM: usize = 1;
    const PRICE: u64 = 1_000_000;
    const REPLACEMENT_CAPITAL: u128 = 100_000_000;
    const POSITION_Q: i128 = 1_000 * POS_SCALE as i128;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
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
            actor_deposits: [2_000_000_000, 1, 1_000, 1, 1],
            actor_token_balances: [2_100_000_000, 200_000_000, 10_000, 10, 10],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.update_liquidation_fee_policy(10_000)
        .map_err(|error| format!("PR 304 configure cranker reward: {error}"))?;
    let original_portfolio_id = env.primary_portfolio_id(VICTIM);
    let retained_grant = env.build_retained_matcher_config(VICTIM, 1);
    env.set_matcher_config(VICTIM, 0)
        .map_err(|error| format!("PR 304 disable incarnation-A matcher: {error}"))?;
    env.withdraw_primary(VICTIM, 1)
        .map_err(|error| format!("PR 304 empty incarnation A: {error}"))?;
    env.close_primary_portfolio(VICTIM)
        .map_err(|error| format!("PR 304 close incarnation A: {error}"))?;
    env.fund_closed_primary_portfolio(VICTIM, 1_000_000_000)
        .map_err(|error| format!("PR 304 fund replacement account: {error}"))?;
    env.reinitialize_primary_portfolio(VICTIM)
        .map_err(|error| format!("PR 304 initialize incarnation B: {error}"))?;
    let replacement_portfolio_id = env.primary_portfolio_id(VICTIM);
    if replacement_portfolio_id <= original_portfolio_id {
        return Err(format!(
            "PR 304 portfolio ID did not advance: {original_portfolio_id}->{replacement_portfolio_id}"
        ));
    }
    env.deposit_primary(VICTIM, REPLACEMENT_CAPITAL)
        .map_err(|error| format!("PR 304 fund incarnation B: {error}"))?;

    let before_replay = tracked_economic_accounts(&env);
    let stale_replay_rejected = env.land_retained(retained_grant).is_err();
    let rejected_exact_rollback = tracked_economic_accounts(&env) == before_replay;
    let before_control_trade = tracked_economic_accounts(&env);
    let control_trade_blocked = env.trade_cpi(LONG, VICTIM, 0, POSITION_Q, 0, 0).is_err()
        && tracked_economic_accounts(&env) == before_control_trade;
    if !stale_replay_rejected || !rejected_exact_rollback || !control_trade_blocked {
        return Err(format!(
            "PR 304 stale grant was not rejected atomically: rejected={stale_replay_rejected}, \
             rollback={rejected_exact_rollback}, trade_blocked={control_trade_blocked}"
        ));
    }

    let fresh = env
        .set_matcher_config(VICTIM, 1)
        .map_err(|error| format!("PR 304 fresh incarnation-B grant: {error}"))?;
    let open = env
        .trade_cpi(LONG, VICTIM, 0, POSITION_Q, 0, 0)
        .map_err(|error| format!("PR 304 fresh incarnation-B open: {error}"))?;
    let close = env
        .trade_cpi(LONG, VICTIM, 0, -POSITION_Q, 0, 0)
        .map_err(|error| format!("PR 304 fresh incarnation-B close: {error}"))?;
    let fresh_grant_landed = true;
    let fresh_round_trip_landed = observed_positions(&env.primary_portfolio(LONG))?[0] == 0
        && observed_positions(&env.primary_portfolio(VICTIM))?[0] == 0
        && env.primary_portfolio(VICTIM).capital.get() == REPLACEMENT_CAPITAL;
    let destination = env.actors[VICTIM].destination_token;
    let destination_before = env.token_amount(destination);
    let withdrawal = env
        .withdraw_primary(VICTIM, REPLACEMENT_CAPITAL)
        .map_err(|error| format!("PR 304 owner exit after fresh round trip: {error}"))?;
    let replacement_capital_u64 = u64::try_from(REPLACEMENT_CAPITAL)
        .map_err(|_| "PR 304 replacement capital exceeds SPL range")?;
    let owner_exit_landed = env
        .token_amount(destination)
        .checked_sub(destination_before)
        == Some(replacement_capital_u64);
    let max_cu = fresh
        .compute_units
        .max(open.compute_units)
        .max(close.compute_units)
        .max(withdrawal.compute_units);
    if !fresh_round_trip_landed
        || !owner_exit_landed
        || env.token_supply_observed() != supply_before
        || max_cu >= TX_CU_LIMIT
    {
        return Err(format!(
            "PR 304 fresh route mismatch: round_trip={fresh_round_trip_landed}, \
             exit={owner_exit_landed}, max_cu={max_cu}, supply={}/{supply_before}",
            env.token_supply_observed()
        ));
    }
    Ok(MatcherGrantPortfolioIncarnationWorld {
        original_portfolio_id,
        replacement_portfolio_id,
        stale_replay_rejected,
        rejected_exact_rollback,
        control_trade_blocked,
        fresh_grant_landed,
        fresh_round_trip_landed,
        owner_exit_landed,
        max_cu,
    })
}

pub fn verify_matcher_grant_portfolio_incarnation_protection(
    mut seed: [u8; 32],
) -> Result<MatcherGrantPortfolioIncarnationReplayProtection, String> {
    seed[0] ^= 0x04;
    let protection = run_matcher_grant_portfolio_incarnation_world(seed)?;
    if protection.replacement_portfolio_id <= protection.original_portfolio_id
        || !protection.stale_replay_rejected
        || !protection.rejected_exact_rollback
        || !protection.control_trade_blocked
        || !protection.fresh_grant_landed
        || !protection.fresh_round_trip_landed
        || !protection.owner_exit_landed
        || protection.max_cu >= TX_CU_LIMIT
    {
        return Err(format!("PR 304 protection mismatch: {protection:?}"));
    }
    Ok(MatcherGrantPortfolioIncarnationReplayProtection {
        blocker: KnownBlocker::MatcherGrantPortfolioIncarnationReplay,
        original_portfolio_id: protection.original_portfolio_id,
        replacement_portfolio_id: protection.replacement_portfolio_id,
        stale_replay_rejected: protection.stale_replay_rejected,
        rejected_exact_rollback: protection.rejected_exact_rollback,
        control_trade_blocked: protection.control_trade_blocked,
        fresh_grant_landed: protection.fresh_grant_landed,
        fresh_round_trip_landed: protection.fresh_round_trip_landed,
        owner_exit_landed: protection.owner_exit_landed,
        max_cu: protection.max_cu,
    })
}

#[derive(Clone, Copy, Debug)]
struct MatcherGrantMarketGenerationWorld {
    old_market_id: u64,
    new_market_id: u64,
    control_trade_blocked: bool,
    liquidation_slot: u64,
    cranker_reward: u128,
    extracted_reward: u64,
    replay_cu: u64,
    max_cu: u64,
}

fn run_matcher_grant_market_generation_world(
    seed: [u8; 32],
    land_replay: bool,
) -> Result<MatcherGrantMarketGenerationWorld, String> {
    const LONG: usize = 0;
    const VICTIM: usize = 1;
    const CRANKER: usize = 2;
    const PRICE: u64 = 1_000_000;
    const LONG_CAPITAL: u128 = 2_000_000_000;
    const VICTIM_CAPITAL: u128 = 100_000_000;
    const CRANKER_CAPITAL: u128 = 1_000;
    const REINIT_SLOT: u64 = 11;

    let config = MarketConfig {
        initial_price: PRICE,
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
        actor_deposits: [1, 1, 1, 1, 1],
        actor_token_balances: [2_100_000_000, 200_000_000, 10_000, 10, 10],
        ..MarketConfig::default()
    };
    let mut env = V16Svm::new(seed, config);
    let old_market_id = env.primary_market_state().1.assets[0].market_id;
    let retained_grant = env.build_retained_matcher_config(VICTIM, 1);

    publicly_recreate_primary_market(&mut env, config, REINIT_SLOT, "PR 294")?;
    env.configure_auth_mark(false, 0, REINIT_SLOT, PRICE)
        .map_err(|error| format!("PR 294 configure generation-B AuthMark: {error}"))?;
    let new_market_id = env.primary_market_state().1.assets[0].market_id;
    if old_market_id == 0 || new_market_id == 0 {
        return Err(format!(
            "PR 294 market generation was zero: {old_market_id}->{new_market_id}"
        ));
    }
    env.update_liquidation_fee_policy(10_000)
        .map_err(|error| format!("PR 294 configure cranker reward: {error}"))?;
    for (actor, capital) in [
        (LONG, LONG_CAPITAL),
        (VICTIM, VICTIM_CAPITAL),
        (CRANKER, CRANKER_CAPITAL),
    ] {
        env.fund_closed_primary_portfolio(actor, 1_000_000_000)
            .map_err(|error| format!("PR 294 re-fund portfolio {actor}: {error}"))?;
        env.reinitialize_primary_portfolio(actor)
            .map_err(|error| format!("PR 294 initialize portfolio {actor}: {error}"))?;
        env.deposit_primary(actor, capital)
            .map_err(|error| format!("PR 294 deposit portfolio {actor}: {error}"))?;
    }
    env.set_matcher_config(VICTIM, 0)
        .map_err(|error| format!("PR 294 align replacement matcher sequence: {error}"))?;

    let first_mark_slot = REINIT_SLOT
        .checked_add(1)
        .ok_or("PR 294 first mark slot overflow")?;
    let outcome = exercise_matcher_grant_replay(
        &mut env,
        retained_grant,
        land_replay,
        first_mark_slot,
        "PR 294",
    )?;
    Ok(MatcherGrantMarketGenerationWorld {
        old_market_id,
        new_market_id,
        control_trade_blocked: outcome.control_trade_blocked,
        liquidation_slot: outcome.liquidation_slot,
        cranker_reward: outcome.cranker_reward,
        extracted_reward: outcome.extracted_reward,
        replay_cu: outcome.replay_cu,
        max_cu: outcome.max_cu,
    })
}

pub fn reproduce_matcher_grant_market_generation_replay(
    mut seed: [u8; 32],
) -> Result<MatcherGrantMarketGenerationReplayReproduction, String> {
    seed[0] ^= 0x94;
    let control = run_matcher_grant_market_generation_world(seed, false)?;
    let replay = run_matcher_grant_market_generation_world(seed, true)?;
    if control.old_market_id != replay.old_market_id
        || control.new_market_id != replay.new_market_id
        || !control.control_trade_blocked
        || control.cranker_reward != 0
        || replay.cranker_reward == 0
        || u128::from(replay.extracted_reward) != replay.cranker_reward
        || replay.replay_cu == 0
        || replay.max_cu >= TX_CU_LIMIT
    {
        return Err(format!(
            "PR 294 paired-world mismatch: control={control:?}, replay={replay:?}"
        ));
    }
    Ok(MatcherGrantMarketGenerationReplayReproduction {
        blocker: KnownBlocker::MatcherGrantMarketGenerationReplay,
        old_market_id: replay.old_market_id,
        new_market_id: replay.new_market_id,
        control_trade_blocked: control.control_trade_blocked,
        liquidation_slot: replay.liquidation_slot,
        cranker_reward: replay.cranker_reward,
        extracted_reward: replay.extracted_reward,
        replay_cu: replay.replay_cu,
        max_cu: replay.max_cu,
    })
}

#[derive(Clone, Copy, Debug)]
struct TradeFeeMarketGenerationWorld {
    old_market_id: u64,
    new_market_id: u64,
    stale_policy_landed: bool,
    stale_trade_rejected: bool,
    rejected_exact_rollback: bool,
    recovery_trade_landed: bool,
    victim_loss: u64,
    attacker_profit: u64,
    extracted_fee: u64,
    replay_cu: u64,
    trade_cu: u64,
    max_cu: u64,
}

fn run_trade_fee_market_generation_world(
    seed: [u8; 32],
    land_replay: bool,
) -> Result<TradeFeeMarketGenerationWorld, String> {
    const VICTIM: usize = 0;
    const ATTACKER: usize = 1;
    const ACTIVATION_PAYER: usize = 2;
    const ASSET: u16 = 1;
    const PRICE: u64 = 100;
    const DEPOSIT: u128 = 10_000;
    const SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const STALE_TRADE_FEE_BPS: u64 = 10_000;
    const REINIT_SLOT: u64 = 10;
    const RETIRE_SLOT: u64 = 11;
    const ACTIVATE_SLOT: u64 = 12;

    let config = MarketConfig {
        initial_price: PRICE,
        max_trading_fee_bps: STALE_TRADE_FEE_BPS,
        actor_deposits: [1, 1, 1, 1, 1],
        actor_token_balances: [20_001, 20_001, 10, 1, 1],
        ..MarketConfig::default()
    };
    let mut env = V16Svm::new(seed, config);
    let supply_before = env.token_supply_observed();
    let old_market_id = env.primary_market_state().1.assets[0].market_id;
    let retained_policy = env.build_retained_trade_fee_policy(STALE_TRADE_FEE_BPS);

    publicly_recreate_primary_market(&mut env, config, REINIT_SLOT, "PR 296")?;
    env.configure_auth_mark(false, 0, REINIT_SLOT, PRICE)
        .map_err(|error| format!("PR 296 configure generation-B asset 0: {error}"))?;
    let new_market_id = env.primary_market_state().1.assets[0].market_id;
    env.update_market_init_fee_policy(1)
        .map_err(|error| format!("PR 296 configure permissionless init fee: {error}"))?;
    env.warp_to_slot(RETIRE_SLOT);
    env.retire_asset(ASSET, RETIRE_SLOT)
        .map_err(|error| format!("PR 296 retire generation-B asset slot: {error}"))?;
    env.warp_to_slot(ACTIVATE_SLOT);
    env.activate_permissionless_asset_with_actor_authorities(
        ACTIVATION_PAYER,
        ASSET,
        ACTIVATE_SLOT,
        PRICE,
        ATTACKER,
        ATTACKER,
        ATTACKER,
        ATTACKER,
        1,
    )
    .map_err(|error| format!("PR 296 activate attacker-operated asset: {error}"))?;
    for actor in [VICTIM, ATTACKER] {
        env.fund_closed_primary_portfolio(actor, 1_000_000_000)
            .map_err(|error| format!("PR 296 re-fund portfolio {actor}: {error}"))?;
        env.reinitialize_primary_portfolio(actor)
            .map_err(|error| format!("PR 296 initialize portfolio {actor}: {error}"))?;
    }
    if env.primary_market_state().0.trade_fee_base_bps != 0
        || env.primary_market_state().1.c_tot != 0
    {
        return Err("PR 296 replacement market did not start empty and zero-fee".into());
    }

    let victim_deposit = env.build_retained_deposit(VICTIM, DEPOSIT);
    let attacker_deposit = env.build_retained_deposit(ATTACKER, DEPOSIT);
    let retained_trade = env.build_retained_no_cpi_trade(VICTIM, ATTACKER, ASSET, SIZE_Q, PRICE);
    let mut max_cu = 0;
    let replay_cu = if land_replay {
        let replay = env
            .land_retained(retained_policy)
            .map_err(|error| format!("PR 296 stale trade-fee policy no longer lands: {error}"))?;
        max_cu = max_cu.max(replay.compute_units);
        replay.compute_units
    } else {
        0
    };
    let expected_policy = if land_replay { STALE_TRADE_FEE_BPS } else { 0 };
    if env.primary_market_state().0.trade_fee_base_bps != expected_policy {
        return Err(format!(
            "PR 296 policy mismatch: replay={land_replay}, expected={expected_policy}, got={}",
            env.primary_market_state().0.trade_fee_base_bps
        ));
    }
    for (label, retained) in [("victim", victim_deposit), ("attacker", attacker_deposit)] {
        let deposit = env
            .land_retained(retained)
            .map_err(|error| format!("PR 296 pre-signed {label} deposit rejected: {error}"))?;
        max_cu = max_cu.max(deposit.compute_units);
    }
    let (stale_trade_rejected, rejected_exact_rollback, recovery_trade_landed, trade_cu) =
        if land_replay {
            let before_rejection = tracked_economic_accounts(&env);
            let error = match env.land_retained(retained_trade) {
                Ok(_) => return Err("PR 296 stale-fee retained trade unexpectedly landed".into()),
                Err(error) => error,
            };
            let rejected =
                error.contains("Custom(9)") || error.contains("custom program error: 0x9");
            let rolled_back = tracked_economic_accounts(&env) == before_rejection;
            if !rejected || !rolled_back {
                return Err(format!(
                    "PR 296 stale-fee retained trade did not reject atomically: {error}"
                ));
            }
            let correction = env
                .update_trade_fee_policy(0)
                .map_err(|error| format!("PR 296 restore zero fee after rejection: {error}"))?;
            max_cu = max_cu.max(correction.compute_units);
            let recovery_trade = env
                .trade_no_cpi(VICTIM, ATTACKER, ASSET, SIZE_Q, PRICE, 0)
                .map_err(|error| format!("PR 296 fresh zero-fee trade failed: {error}"))?;
            max_cu = max_cu.max(recovery_trade.compute_units);
            (true, true, true, recovery_trade.compute_units)
        } else {
            let trade = env
                .land_retained(retained_trade)
                .map_err(|error| format!("PR 296 control zero-fee trade rejected: {error}"))?;
            max_cu = max_cu.max(trade.compute_units);
            (false, true, false, trade.compute_units)
        };

    let group_after_trade = env.primary_market_state().1;
    let attacker_domain_fee = group_after_trade.insurance_domain_budget[ASSET as usize * 2]
        .checked_add(group_after_trade.insurance_domain_budget[ASSET as usize * 2 + 1])
        .ok_or("PR 296 attacker domain fee overflow")?;
    if attacker_domain_fee != 0 {
        return Err(format!(
            "PR 296 rejected stale terms still created attacker-domain fee {attacker_domain_fee}"
        ));
    }

    let victim_destination = env.actors[VICTIM].destination_token;
    let attacker_destination = env.actors[ATTACKER].destination_token;
    let victim_destination_before = env.token_amount(victim_destination);
    let attacker_destination_before = env.token_amount(attacker_destination);
    let close = env
        .trade_no_cpi(VICTIM, ATTACKER, ASSET, -SIZE_Q, PRICE, 0)
        .map_err(|error| format!("PR 296 neutral close: {error}"))?;
    max_cu = max_cu.max(close.compute_units);
    for actor in [VICTIM, ATTACKER] {
        let capital = env.primary_portfolio(actor).capital.get();
        let withdrawal = env
            .withdraw_primary(actor, capital)
            .map_err(|error| format!("PR 296 terminal withdrawal for {actor}: {error}"))?;
        max_cu = max_cu.max(withdrawal.compute_units);
    }
    let victim_return = env
        .token_amount(victim_destination)
        .checked_sub(victim_destination_before)
        .ok_or("PR 296 victim destination decreased")?;
    let attacker_return = env
        .token_amount(attacker_destination)
        .checked_sub(attacker_destination_before)
        .ok_or("PR 296 attacker destination decreased")?;
    let deposit_u64 = u64::try_from(DEPOSIT).map_err(|_| "PR 296 deposit exceeds u64")?;
    let victim_loss = deposit_u64
        .checked_sub(victim_return)
        .ok_or("PR 296 victim returned more than deposited")?;
    let attacker_profit = attacker_return
        .checked_sub(deposit_u64)
        .ok_or("PR 296 attacker did not recover its deposit")?;
    if victim_loss != 0
        || attacker_profit != 0
        || env.token_supply_observed() != supply_before
        || max_cu >= TX_CU_LIMIT
    {
        return Err(format!(
            "PR 296 terminal mismatch: replay={land_replay}, victim={victim_loss}, \
             attacker={attacker_profit}, max_cu={max_cu}, supply={}/{supply_before}",
            env.token_supply_observed()
        ));
    }
    Ok(TradeFeeMarketGenerationWorld {
        old_market_id,
        new_market_id,
        stale_policy_landed: land_replay,
        stale_trade_rejected,
        rejected_exact_rollback,
        recovery_trade_landed,
        victim_loss,
        attacker_profit,
        extracted_fee: u64::try_from(attacker_domain_fee)
            .map_err(|_| "PR 296 extracted fee exceeds u64")?,
        replay_cu,
        trade_cu,
        max_cu,
    })
}

pub fn verify_trade_fee_market_generation_nonextraction(
    mut seed: [u8; 32],
) -> Result<TradeFeeMarketGenerationReplayProtection, String> {
    seed[0] ^= 0x96;
    let control = run_trade_fee_market_generation_world(seed, false)?;
    let replay = run_trade_fee_market_generation_world(seed, true)?;
    if control.old_market_id != replay.old_market_id
        || control.new_market_id != replay.new_market_id
        || control.victim_loss != 0
        || control.attacker_profit != 0
        || control.extracted_fee != 0
        || !replay.stale_policy_landed
        || !replay.stale_trade_rejected
        || !replay.rejected_exact_rollback
        || !replay.recovery_trade_landed
        || replay.victim_loss != 0
        || replay.attacker_profit != 0
        || replay.extracted_fee != 0
        || replay.replay_cu == 0
        || replay.max_cu >= TX_CU_LIMIT
    {
        return Err(format!(
            "PR 296 paired-world mismatch: control={control:?}, replay={replay:?}"
        ));
    }
    Ok(TradeFeeMarketGenerationReplayProtection {
        blocker: KnownBlocker::TradeFeeMarketGenerationReplay,
        old_market_id: replay.old_market_id,
        new_market_id: replay.new_market_id,
        stale_policy_landed: replay.stale_policy_landed,
        stale_trade_rejected: replay.stale_trade_rejected,
        rejected_exact_rollback: replay.rejected_exact_rollback,
        recovery_trade_landed: replay.recovery_trade_landed,
        victim_loss: replay.victim_loss,
        attacker_profit: replay.attacker_profit,
        extracted_fee: replay.extracted_fee,
        replay_cu: replay.replay_cu,
        trade_cu: replay.trade_cu,
        max_cu: replay.max_cu.max(control.max_cu),
    })
}

#[derive(Clone, Copy, Debug)]
struct TradePortfolioIncarnationWorld {
    original_portfolio_id: u64,
    replacement_portfolio_id: u64,
    position_after_landing_q: i128,
    liquidation_slot: u64,
    cranker_reward: u128,
    extracted_reward: u64,
    replay_cu: u64,
    max_cu: u64,
}

fn run_trade_portfolio_incarnation_world(
    seed: [u8; 32],
    route: TradeRoute,
    replacement_side: PortfolioIncarnationTradeSide,
    land_replay: bool,
) -> Result<TradePortfolioIncarnationWorld, String> {
    const COUNTERPARTY: usize = 0;
    const VICTIM: usize = 1;
    const CRANKER: usize = 2;
    const PRICE: u64 = 1_000_000;
    const REPLACEMENT_CAPITAL: u128 = 100_000;
    const POSITION_Q: i128 = POS_SCALE as i128;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
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
            actor_deposits: [100_000_000, 1, 1_000, 1, 1],
            actor_token_balances: [101_000_000, 200_000, 10_000, 10, 10],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.update_liquidation_fee_policy(10_000)
        .map_err(|error| format!("PR 303 configure cranker reward: {error}"))?;

    let original_portfolio_id = env.primary_portfolio_id(VICTIM);
    let (taker, maker, size_q) = match replacement_side {
        PortfolioIncarnationTradeSide::AccountA => (VICTIM, COUNTERPARTY, -POSITION_Q),
        PortfolioIncarnationTradeSide::AccountB => (COUNTERPARTY, VICTIM, POSITION_Q),
    };
    let retained = build_retained_trade(&mut env, route, taker, maker, 0, size_q, PRICE, 0);
    env.set_matcher_config(VICTIM, 0)
        .map_err(|error| format!("PR 303 disable incarnation-A matcher: {error}"))?;
    env.withdraw_primary(VICTIM, 1)
        .map_err(|error| format!("PR 303 empty incarnation A: {error}"))?;
    env.close_primary_portfolio(VICTIM)
        .map_err(|error| format!("PR 303 close incarnation A: {error}"))?;
    env.fund_closed_primary_portfolio(VICTIM, 1_000_000_000)
        .map_err(|error| format!("PR 303 fund replacement account: {error}"))?;
    env.reinitialize_primary_portfolio(VICTIM)
        .map_err(|error| format!("PR 303 initialize incarnation B: {error}"))?;
    let replacement_portfolio_id = env.primary_portfolio_id(VICTIM);
    if replacement_portfolio_id <= original_portfolio_id {
        return Err(format!(
            "PR 303 portfolio ID did not advance: {original_portfolio_id}->{replacement_portfolio_id}"
        ));
    }
    env.deposit_primary(VICTIM, REPLACEMENT_CAPITAL)
        .map_err(|error| format!("PR 303 fund incarnation B: {error}"))?;
    if matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi) && maker == VICTIM {
        env.set_matcher_config(VICTIM, 1)
            .map_err(|error| format!("PR 303 authorize B's current matcher: {error}"))?;
    }

    let mut replay_cu = 0;
    let mut max_cu = 0;
    if land_replay {
        let replay = env
            .land_retained(retained)
            .map_err(|error| format!("PR 303 stale {route:?} no longer lands: {error}"))?;
        replay_cu = replay.compute_units;
        max_cu = max_cu.max(replay.compute_units);
    }
    let position_after_landing_q = decoded_legs(&env.primary_portfolio(VICTIM))
        .into_iter()
        .find(|leg| leg.active && leg.asset_index == 0)
        .map(|leg| leg.basis_pos_q)
        .unwrap_or(0);
    let expected_position = if land_replay { -POSITION_Q } else { 0 };
    if position_after_landing_q != expected_position {
        return Err(format!(
            "PR 303 {route:?}/{replacement_side:?} replacement position mismatch: \
             expected={expected_position}, got={position_after_landing_q}"
        ));
    }

    let mut liquidation_slot = 0;
    for slot in 1..=30u64 {
        env.warp_to_slot(slot);
        env.push_auth_mark(0, slot, 2_000_000)
            .map_err(|error| format!("PR 303 publish mark at slot {slot}: {error}"))?;
        let crank = env
            .crank(
                VICTIM,
                slot,
                vec![CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                }],
            )
            .map_err(|error| format!("PR 303 refresh B at slot {slot}: {error}"))?;
        max_cu = max_cu.max(crank.compute_units);
        if !land_replay {
            continue;
        }
        let cert = env
            .primary_portfolio(VICTIM)
            .health_cert
            .try_to_runtime()
            .map_err(|error| format!("PR 303 decode health certificate: {error:?}"))?;
        let maintenance = i128::try_from(cert.certified_maintenance_req)
            .map_err(|_| "PR 303 maintenance requirement exceeds signed range")?;
        if cert.certified_equity < maintenance {
            liquidation_slot = slot;
            break;
        }
    }

    let cranker_before = env.primary_portfolio(CRANKER).capital.get();
    let mut cranker_reward = 0;
    let mut extracted_reward = 0;
    if land_replay {
        if liquidation_slot == 0 {
            return Err(format!(
                "PR 303 stale {route:?}/{replacement_side:?} never made B liquidatable"
            ));
        }
        let liquidation = env
            .crank_with_reward(CRANKER, VICTIM, liquidation_slot, Vec::new(), &[])
            .map_err(|error| format!("PR 303 independent liquidation: {error}"))?;
        max_cu = max_cu.max(liquidation.compute_units);
        cranker_reward = env
            .primary_portfolio(CRANKER)
            .capital
            .get()
            .checked_sub(cranker_before)
            .ok_or("PR 303 cranker capital decreased")?;
        if cranker_reward == 0 {
            return Err("PR 303 stale trade paid no liquidation reward".into());
        }
        let destination = env.actors[CRANKER].destination_token;
        let destination_before = env.token_amount(destination);
        let withdrawal = env
            .withdraw_primary(CRANKER, cranker_reward)
            .map_err(|error| format!("PR 303 withdraw liquidation reward: {error}"))?;
        max_cu = max_cu.max(withdrawal.compute_units);
        extracted_reward = env
            .token_amount(destination)
            .checked_sub(destination_before)
            .ok_or("PR 303 cranker destination decreased")?;
    }
    if u128::from(extracted_reward) != cranker_reward
        || (!land_replay && env.primary_portfolio(CRANKER).capital.get() != cranker_before)
        || env.token_supply_observed() != supply_before
        || max_cu >= TX_CU_LIMIT
    {
        return Err(format!(
            "PR 303 terminal mismatch: route={route:?}, side={replacement_side:?}, \
             reward={cranker_reward}, extracted={extracted_reward}, max_cu={max_cu}, \
             supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }
    Ok(TradePortfolioIncarnationWorld {
        original_portfolio_id,
        replacement_portfolio_id,
        position_after_landing_q,
        liquidation_slot,
        cranker_reward,
        extracted_reward,
        replay_cu,
        max_cu,
    })
}

pub fn reproduce_trade_portfolio_incarnation_replay(
    mut seed: [u8; 32],
    route: TradeRoute,
    replacement_side: PortfolioIncarnationTradeSide,
) -> Result<TradePortfolioIncarnationReplayReproduction, String> {
    seed[0] ^= 0x03 ^ route.index() as u8;
    if replacement_side == PortfolioIncarnationTradeSide::AccountB {
        seed[1] ^= 0xb0;
    }
    let control = run_trade_portfolio_incarnation_world(seed, route, replacement_side, false)?;
    let replay = run_trade_portfolio_incarnation_world(seed, route, replacement_side, true)?;
    if control.original_portfolio_id != replay.original_portfolio_id
        || control.replacement_portfolio_id != replay.replacement_portfolio_id
        || control.position_after_landing_q != 0
        || control.cranker_reward != 0
        || control.extracted_reward != 0
        || replay.position_after_landing_q != -(POS_SCALE as i128)
        || replay.cranker_reward == 0
        || u128::from(replay.extracted_reward) != replay.cranker_reward
        || replay.replay_cu == 0
    {
        return Err(format!(
            "PR 303 paired-world mismatch: route={route:?}, side={replacement_side:?}, \
             control={control:?}, replay={replay:?}"
        ));
    }
    Ok(TradePortfolioIncarnationReplayReproduction {
        blocker: KnownBlocker::TradePortfolioIncarnationReplay,
        route,
        replacement_side,
        original_portfolio_id: replay.original_portfolio_id,
        replacement_portfolio_id: replay.replacement_portfolio_id,
        control_position_q: control.position_after_landing_q,
        liquidation_slot: replay.liquidation_slot,
        cranker_reward: replay.cranker_reward,
        extracted_reward: replay.extracted_reward,
        replay_cu: replay.replay_cu,
        max_cu: replay.max_cu.max(control.max_cu),
    })
}

fn create_released_pnl_for_incarnation(
    env: &mut V16Svm,
    winner: usize,
    loser: usize,
    winner_deposit: u128,
    loser_deposit: u128,
    mark_slot: u64,
    start_price: u64,
    label: &str,
) -> Result<u128, String> {
    const POSITION_Q: i128 = 20 * POS_SCALE as i128;

    env.deposit_primary(winner, winner_deposit)
        .map_err(|error| format!("{label} fund winner: {error}"))?;
    env.deposit_primary(loser, loser_deposit)
        .map_err(|error| format!("{label} fund loser: {error}"))?;
    env.trade_no_cpi(winner, loser, 0, POSITION_Q, start_price, 0)
        .map_err(|error| format!("{label} open backed trade: {error}"))?;
    env.warp_to_slot(mark_slot);
    env.push_auth_mark(0, mark_slot, start_price + 5)
        .map_err(|error| format!("{label} publish winning mark: {error}"))?;
    for actor in [loser, winner] {
        env.crank(
            actor,
            mark_slot,
            vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: 0,
            }],
        )
        .map_err(|error| format!("{label} refresh actor {actor}: {error}"))?;
    }
    env.trade_no_cpi(winner, loser, 0, -POSITION_Q, start_price + 5, 0)
        .map_err(|error| format!("{label} close backed trade: {error}"))?;
    let released = env.primary_portfolio(winner).pnl.get();
    if released <= 0 {
        return Err(format!(
            "{label} produced no released winner PnL: {released}"
        ));
    }
    u128::try_from(released).map_err(|_| format!("{label} released PnL conversion overflow"))
}

#[derive(Clone, Copy, Debug)]
struct ConvertPortfolioIncarnationWorld {
    original_portfolio_id: u64,
    replacement_portfolio_id: u64,
    released_pnl: u128,
    victim_payout: u64,
    cranker_extraction: u64,
    replay_cu: u64,
    sync_cu: u64,
    max_cu: u64,
}

fn run_convert_portfolio_incarnation_world(
    seed: [u8; 32],
    land_replay: bool,
) -> Result<ConvertPortfolioIncarnationWorld, String> {
    const LOSER_A: usize = 0;
    const VICTIM: usize = 1;
    const CRANKER: usize = 2;
    const LOSER_B: usize = 3;
    const PRICE_A: u64 = 100;
    const PRICE_B: u64 = 105;
    const TARGET_CAPITAL: u128 = 1_000_000;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE_A,
            h_max: 10,
            min_nonzero_mm_req: 1,
            min_nonzero_im_req: 2,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            maintenance_fee_per_slot: 1,
            actor_deposits: [1, 1, 1, 1, 1],
            actor_token_balances: [2_000_000, 3_000_000, 10_000, 2_000_000, 10],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.update_maintenance_fee_policy(10_000)
        .map_err(|error| format!("PR 301 configure cranker reward: {error}"))?;
    env.top_up_backing_bucket(1, 300, 1_000)
        .map_err(|error| format!("PR 301 top up short backing: {error}"))?;

    let released_a = create_released_pnl_for_incarnation(
        &mut env,
        VICTIM,
        LOSER_A,
        TARGET_CAPITAL - 1,
        TARGET_CAPITAL - 1,
        2,
        PRICE_A,
        "PR 301 incarnation A",
    )?;
    let original_portfolio_id = env.primary_portfolio_id(VICTIM);
    let retained = env.build_retained_convert_released_pnl(VICTIM, u128::MAX);
    env.convert_released_pnl(VICTIM, released_a)
        .map_err(|error| format!("PR 301 convert incarnation-A PnL: {error}"))?;
    let capital_a = env.primary_portfolio(VICTIM).capital.get();
    env.withdraw_primary(VICTIM, capital_a)
        .map_err(|error| format!("PR 301 withdraw incarnation A: {error}"))?;
    env.close_primary_portfolio(VICTIM)
        .map_err(|error| format!("PR 301 close incarnation A: {error}"))?;
    env.fund_closed_primary_portfolio(VICTIM, 1_000_000_000)
        .map_err(|error| format!("PR 301 fund replacement account: {error}"))?;
    env.reinitialize_primary_portfolio(VICTIM)
        .map_err(|error| format!("PR 301 initialize incarnation B: {error}"))?;
    let replacement_portfolio_id = env.primary_portfolio_id(VICTIM);
    if replacement_portfolio_id <= original_portfolio_id {
        return Err(format!(
            "PR 301 portfolio ID did not advance: {original_portfolio_id}->{replacement_portfolio_id}"
        ));
    }

    let victim_destination = env.actors[VICTIM].destination_token;
    let victim_destination_before = env.token_amount(victim_destination);
    let released_pnl = create_released_pnl_for_incarnation(
        &mut env,
        VICTIM,
        LOSER_B,
        TARGET_CAPITAL,
        TARGET_CAPITAL - 1,
        3,
        PRICE_B,
        "PR 301 incarnation B",
    )?;
    let ordinary_capital = env.primary_portfolio(VICTIM).capital.get();
    env.withdraw_primary(VICTIM, ordinary_capital)
        .map_err(|error| format!("PR 301 withdraw B ordinary capital: {error}"))?;
    env.crank(
        VICTIM,
        3,
        vec![CrankObservationHint {
            asset_index: 0,
            oracle_accounts: 0,
        }],
    )
    .map_err(|error| format!("PR 301 settle empty-capital B: {error}"))?;
    if env.primary_portfolio(VICTIM).capital.get() != 0
        || env.primary_portfolio(VICTIM).pnl.get() != released_pnl as i128
    {
        return Err(format!(
            "PR 301 B precondition mismatch: capital={}, pnl={}, expected={released_pnl}",
            env.primary_portfolio(VICTIM).capital.get(),
            env.primary_portfolio(VICTIM).pnl.get()
        ));
    }

    let mut replay_cu = 0;
    let mut max_cu = 0;
    if land_replay {
        let replay = env
            .land_retained(retained)
            .map_err(|error| format!("PR 301 incarnation-A conversion no longer lands: {error}"))?;
        replay_cu = replay.compute_units;
        max_cu = max_cu.max(replay.compute_units);
    }
    env.warp_to_slot(10);
    let cranker_before = env.primary_portfolio(CRANKER).capital.get();
    let sync = env
        .sync_maintenance_fee_with_reward(VICTIM, CRANKER, 10)
        .map_err(|error| format!("PR 301 permissionless fee sync: {error}"))?;
    let sync_cu = sync.compute_units;
    max_cu = max_cu.max(sync_cu);
    let cranker_reward = env
        .primary_portfolio(CRANKER)
        .capital
        .get()
        .checked_sub(cranker_before)
        .ok_or("PR 301 cranker capital decreased")?;
    if !land_replay {
        if cranker_reward != 0 {
            return Err(format!(
                "PR 301 control paid a {cranker_reward}-atom maintenance reward"
            ));
        }
        env.convert_released_pnl(VICTIM, u128::MAX)
            .map_err(|error| format!("PR 301 fresh B conversion: {error}"))?;
    }

    let victim_capital = env.primary_portfolio(VICTIM).capital.get();
    let victim_withdrawal = env
        .withdraw_primary(VICTIM, victim_capital)
        .map_err(|error| format!("PR 301 withdraw B terminal capital: {error}"))?;
    max_cu = max_cu.max(victim_withdrawal.compute_units);
    let victim_payout = env
        .token_amount(victim_destination)
        .checked_sub(victim_destination_before)
        .ok_or("PR 301 victim destination decreased")?;

    let mut cranker_extraction = 0;
    if cranker_reward > 0 {
        let cranker_destination = env.actors[CRANKER].destination_token;
        let cranker_destination_before = env.token_amount(cranker_destination);
        let withdrawal = env
            .withdraw_primary(CRANKER, cranker_reward)
            .map_err(|error| format!("PR 301 withdraw cranker reward: {error}"))?;
        max_cu = max_cu.max(withdrawal.compute_units);
        cranker_extraction = env
            .token_amount(cranker_destination)
            .checked_sub(cranker_destination_before)
            .ok_or("PR 301 cranker destination decreased")?;
    }
    if u128::from(cranker_extraction) != cranker_reward
        || env.token_supply_observed() != supply_before
        || max_cu >= TX_CU_LIMIT
    {
        return Err(format!(
            "PR 301 terminal mismatch: replay={land_replay}, victim={victim_payout}, \
             reward={cranker_reward}/{cranker_extraction}, max_cu={max_cu}, supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }
    Ok(ConvertPortfolioIncarnationWorld {
        original_portfolio_id,
        replacement_portfolio_id,
        released_pnl,
        victim_payout,
        cranker_extraction,
        replay_cu,
        sync_cu,
        max_cu,
    })
}

pub fn reproduce_convert_portfolio_incarnation_replay(
    mut seed: [u8; 32],
) -> Result<ConvertPortfolioIncarnationReplayReproduction, String> {
    seed[0] ^= 0x01;
    let control = run_convert_portfolio_incarnation_world(seed, false)?;
    let replay = run_convert_portfolio_incarnation_world(seed, true)?;
    let victim_loss = control
        .victim_payout
        .checked_sub(replay.victim_payout)
        .ok_or("PR 301 replay increased victim payout")?;
    if control.original_portfolio_id != replay.original_portfolio_id
        || control.replacement_portfolio_id != replay.replacement_portfolio_id
        || control.released_pnl != replay.released_pnl
        || control.cranker_extraction != 0
        || replay.cranker_extraction == 0
        || victim_loss != replay.cranker_extraction
        || replay.replay_cu == 0
    {
        return Err(format!(
            "PR 301 paired-world mismatch: control={control:?}, replay={replay:?}, \
             victim_loss={victim_loss}"
        ));
    }
    Ok(ConvertPortfolioIncarnationReplayReproduction {
        blocker: KnownBlocker::ConvertPortfolioIncarnationReplay,
        original_portfolio_id: replay.original_portfolio_id,
        replacement_portfolio_id: replay.replacement_portfolio_id,
        released_pnl: replay.released_pnl,
        victim_loss,
        cranker_extraction: replay.cranker_extraction,
        replay_cu: replay.replay_cu,
        sync_cu: replay.sync_cu,
        max_cu: replay.max_cu.max(control.max_cu),
    })
}

#[derive(Clone, Copy, Debug)]
struct ForfeitPortfolioIncarnationWorld {
    original_portfolio_id: u64,
    replacement_portfolio_id: u64,
    victim_payout: u64,
    attacker_payout: u64,
    vault_remaining: u128,
    slab_closed: bool,
    stale_replay_rejected: bool,
    rejected_exact_rollback: bool,
    max_cu: u64,
}

#[derive(Clone, Copy, Debug)]
struct ForfeitReplayTerminalOutcome {
    victim_payout: u64,
    attacker_payout: u64,
    vault_remaining: u128,
    slab_closed: bool,
    stale_replay_rejected: bool,
    rejected_exact_rollback: bool,
    replay_cu: u64,
    max_cu: u64,
}

#[allow(clippy::too_many_arguments)]
fn finish_forfeit_replay_terminal(
    env: &mut V16Svm,
    retained: Transaction,
    land_replay: bool,
    expect_replay_rejection: bool,
    mark_slot: u64,
    shutdown_slot: u64,
    drain_slot: u64,
    supply_before: u128,
    context: &str,
) -> Result<ForfeitReplayTerminalOutcome, String> {
    const VICTIM: usize = 0;
    const ATTACKER: usize = 1;
    const PRICE: u64 = 100;
    const WIN_PRICE: u64 = 150;
    const SIZE_Q: i128 = 5_000 * POS_SCALE as i128;

    env.warp_to_slot(mark_slot);
    env.push_auth_mark(0, mark_slot, WIN_PRICE)
        .map_err(|error| format!("{context} publish honest winning mark: {error}"))?;
    let mut max_cu = 0;
    for step in 0..4 {
        let crank = env
            .crank(
                ATTACKER,
                mark_slot,
                vec![CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                }],
            )
            .map_err(|error| format!("{context} mark crank {step}: {error}"))?;
        max_cu = max_cu.max(crank.compute_units);
    }
    let effective_mark = env.primary_market_state().1.assets[0].effective_price;
    let pending_victim_profit = SIZE_Q
        .unsigned_abs()
        .checked_mul(u128::from(effective_mark - PRICE))
        .ok_or_else(|| format!("{context} pending winner PnL overflow"))?
        / POS_SCALE;
    if effective_mark != 120
        || pending_victim_profit != 100_000
        || env.primary_portfolio(VICTIM).pnl.get() != 0
    {
        return Err(format!(
            "{context} pending winner setup mismatch: mark={effective_mark}, \
             pending={pending_victim_profit}, pnl={}",
            env.primary_portfolio(VICTIM).pnl.get()
        ));
    }

    env.warp_to_slot(shutdown_slot);
    let mut shutdown_landed = false;
    for step in 0..16 {
        match env.shutdown_asset(0, shutdown_slot) {
            Ok(shutdown) => {
                max_cu = max_cu.max(shutdown.compute_units);
                shutdown_landed = true;
                break;
            }
            Err(error)
                if error.contains("Custom(19)") || error.contains("custom program error: 0x13") =>
            {
                let catchup = env
                    .crank(
                        ATTACKER,
                        shutdown_slot,
                        vec![CrankObservationHint {
                            asset_index: 0,
                            oracle_accounts: 0,
                        }],
                    )
                    .map_err(|crank_error| {
                        format!(
                            "{context} shutdown catch-up crank {step} after {error}: {crank_error}"
                        )
                    })?;
                max_cu = max_cu.max(catchup.compute_units);
            }
            Err(error) => {
                return Err(format!(
                    "{context} shutdown replacement generation returned unexpected error: {error}"
                ));
            }
        }
    }
    if !shutdown_landed {
        return Err(format!(
            "{context} shutdown replacement generation did not land after bounded public catch-up"
        ));
    }
    let mut replay_cu = 0;
    let (stale_replay_rejected, rejected_exact_rollback) = if land_replay && expect_replay_rejection
    {
        let before_rejection = tracked_economic_accounts(env);
        let replay = env.land_retained(retained);
        let rejected = matches!(
            &replay,
            Err(error)
                if error.contains("Custom(16)")
                    || error.contains("custom program error: 0x10")
        );
        if !rejected {
            return Err(format!(
                "{context} stale forfeit did not reject with the position-binding error: {replay:?}"
            ));
        }
        let exact_rollback = tracked_economic_accounts(env) == before_rejection;
        if !exact_rollback {
            return Err(format!(
                "{context} rejected stale forfeit did not roll back economic accounts exactly"
            ));
        }
        (true, true)
    } else if land_replay {
        let replay = env
            .land_retained(retained)
            .map_err(|error| format!("{context} stale forfeit no longer lands: {error}"))?;
        replay_cu = replay.compute_units;
        max_cu = max_cu.max(replay.compute_units);
        (false, true)
    } else {
        (false, true)
    };
    let resolve = env
        .resolve_market()
        .map_err(|error| format!("{context} resolve terminal world: {error}"))?;
    max_cu = max_cu.max(resolve.compute_units);
    env.warp_to_slot(drain_slot);
    let (attacker_payout, _) = drain_resolved_actor(env, ATTACKER)?;
    let (victim_payout, _) = drain_resolved_actor(env, VICTIM)?;
    let attacker_payout = u64::try_from(attacker_payout)
        .map_err(|_| format!("{context} attacker payout exceeds SPL amount"))?;
    let victim_payout = u64::try_from(victim_payout)
        .map_err(|_| format!("{context} victim payout exceeds SPL amount"))?;
    env.close_primary_portfolio(ATTACKER)
        .map_err(|error| format!("{context} close counterparty portfolio: {error}"))?;
    env.close_primary_portfolio(VICTIM)
        .map_err(|error| format!("{context} close victim portfolio: {error}"))?;
    let terminal = env.primary_market_state().1;
    let vault_remaining = terminal.vault;
    if terminal.c_tot != 0
        || terminal.insurance != 0
        || terminal.materialized_portfolio_count != 0
        || vault_remaining != u128::from(env.token_amount(env.vault))
        || env.token_supply_observed() != supply_before
        || max_cu >= TX_CU_LIMIT
    {
        return Err(format!(
            "{context} terminal state mismatch: replay={land_replay}, c_tot={}, insurance={}, \
             portfolios={}, vault={vault_remaining}/{}, supply={}/{supply_before}, max_cu={max_cu}",
            terminal.c_tot,
            terminal.insurance,
            terminal.materialized_portfolio_count,
            env.token_amount(env.vault),
            env.token_supply_observed()
        ));
    }
    let slab_closed = env.close_primary_slab().is_ok();
    Ok(ForfeitReplayTerminalOutcome {
        victim_payout,
        attacker_payout,
        vault_remaining,
        slab_closed,
        stale_replay_rejected,
        rejected_exact_rollback,
        replay_cu,
        max_cu,
    })
}

fn run_forfeit_portfolio_incarnation_world(
    seed: [u8; 32],
    land_replay: bool,
) -> Result<ForfeitPortfolioIncarnationWorld, String> {
    const VICTIM: usize = 0;
    const ATTACKER: usize = 1;
    const PRICE: u64 = 100;
    const DEPOSIT: u128 = 1_000_000;
    const SIZE_Q: i128 = 5_000 * POS_SCALE as i128;

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
            actor_token_balances: [2_000_000, 1_000_000, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.configure_permissionless_resolve(100, 1)
        .map_err(|error| format!("PR 278 configure permissionless resolve: {error}"))?;
    for actor in 2..PRIMARY_ACTOR_COUNT {
        env.withdraw_primary(actor, 1)
            .map_err(|error| format!("PR 278 empty unused actor {actor}: {error}"))?;
        env.close_primary_portfolio(actor)
            .map_err(|error| format!("PR 278 close unused actor {actor}: {error}"))?;
    }

    let original_portfolio_id = env.primary_portfolio_id(VICTIM);
    env.trade_no_cpi(VICTIM, ATTACKER, 0, SIZE_Q, PRICE, 0)
        .map_err(|error| format!("PR 278 open incarnation-A position: {error}"))?;
    env.warp_to_slot(2);
    env.shutdown_asset(0, 2)
        .map_err(|error| format!("PR 278 shutdown generation A: {error}"))?;
    let retained = env.build_retained_forfeit_recovery_leg(VICTIM, 0, 1);
    env.forfeit_recovery_leg(VICTIM, 0, 1)
        .map_err(|error| format!("PR 278 forfeit victim generation-A leg: {error}"))?;
    env.forfeit_recovery_leg(ATTACKER, 0, 1)
        .map_err(|error| format!("PR 278 forfeit counterparty generation-A leg: {error}"))?;
    env.withdraw_primary(VICTIM, DEPOSIT)
        .map_err(|error| format!("PR 278 withdraw incarnation A: {error}"))?;
    env.close_primary_portfolio(VICTIM)
        .map_err(|error| format!("PR 278 close incarnation A: {error}"))?;

    env.warp_to_slot(3);
    env.restart_asset_oracle(0, 3, PRICE)
        .map_err(|error| format!("PR 278 restart asset generation: {error}"))?;
    env.configure_auth_mark(false, 0, 3, PRICE)
        .map_err(|error| format!("PR 278 configure replacement AuthMark: {error}"))?;
    env.fund_closed_primary_portfolio(VICTIM, 1_000_000_000)
        .map_err(|error| format!("PR 278 fund replacement account: {error}"))?;
    env.reinitialize_primary_portfolio(VICTIM)
        .map_err(|error| format!("PR 278 initialize incarnation B: {error}"))?;
    let replacement_portfolio_id = env.primary_portfolio_id(VICTIM);
    if replacement_portfolio_id <= original_portfolio_id {
        return Err(format!(
            "PR 278 portfolio ID did not advance: {original_portfolio_id}->{replacement_portfolio_id}"
        ));
    }
    env.deposit_primary(VICTIM, DEPOSIT)
        .map_err(|error| format!("PR 278 fund incarnation B: {error}"))?;
    env.trade_no_cpi(VICTIM, ATTACKER, 0, SIZE_Q, PRICE, 0)
        .map_err(|error| format!("PR 278 open incarnation-B position: {error}"))?;
    let outcome = finish_forfeit_replay_terminal(
        &mut env,
        retained,
        land_replay,
        true,
        13,
        14,
        15,
        supply_before,
        "PR 278",
    )?;
    Ok(ForfeitPortfolioIncarnationWorld {
        original_portfolio_id,
        replacement_portfolio_id,
        victim_payout: outcome.victim_payout,
        attacker_payout: outcome.attacker_payout,
        vault_remaining: outcome.vault_remaining,
        slab_closed: outcome.slab_closed,
        stale_replay_rejected: outcome.stale_replay_rejected,
        rejected_exact_rollback: outcome.rejected_exact_rollback,
        max_cu: outcome.max_cu,
    })
}

pub fn reproduce_forfeit_portfolio_incarnation_replay(
    mut seed: [u8; 32],
) -> Result<ForfeitPortfolioIncarnationReplayReproduction, String> {
    seed[0] ^= 0x78;
    let control = run_forfeit_portfolio_incarnation_world(seed, false)?;
    let replay = run_forfeit_portfolio_incarnation_world(seed, true)?;
    if control.original_portfolio_id != replay.original_portfolio_id
        || control.replacement_portfolio_id != replay.replacement_portfolio_id
        || control.victim_payout.checked_add(control.attacker_payout) != Some(2_000_000)
        || replay.victim_payout != control.victim_payout
        || replay.attacker_payout != control.attacker_payout
        || control.vault_remaining != 0
        || replay.vault_remaining != 0
        || !control.slab_closed
        || !replay.slab_closed
        || !replay.stale_replay_rejected
        || !replay.rejected_exact_rollback
    {
        return Err(format!(
            "PR 278 fixed paired-world mismatch: control={control:?}, replay={replay:?}"
        ));
    }
    Ok(ForfeitPortfolioIncarnationReplayReproduction {
        blocker: KnownBlocker::ForfeitPortfolioIncarnationReplay,
        original_portfolio_id: replay.original_portfolio_id,
        replacement_portfolio_id: replay.replacement_portfolio_id,
        stale_replay_rejected: replay.stale_replay_rejected,
        rejected_exact_rollback: replay.rejected_exact_rollback,
        control_victim_payout: control.victim_payout,
        replay_victim_payout: replay.victim_payout,
        control_attacker_payout: control.attacker_payout,
        replay_attacker_payout: replay.attacker_payout,
        control_slab_closed: control.slab_closed,
        replay_slab_closed: replay.slab_closed,
        max_cu: replay.max_cu.max(control.max_cu),
    })
}

#[derive(Clone, Copy, Debug)]
struct ForfeitMarketGenerationWorld {
    old_market_id: u64,
    new_market_id: u64,
    victim_payout: u64,
    attacker_payout: u64,
    vault_remaining: u128,
    slab_closed: bool,
    replay_cu: u64,
    max_cu: u64,
}

fn run_forfeit_market_generation_world(
    seed: [u8; 32],
    land_replay: bool,
) -> Result<ForfeitMarketGenerationWorld, String> {
    const VICTIM: usize = 0;
    const ATTACKER: usize = 1;
    const PRICE: u64 = 100;
    const DEPOSIT: u128 = 1_000_000;
    const SIZE_Q: i128 = 5_000 * POS_SCALE as i128;
    const REINIT_SLOT: u64 = 10;

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
        .map_err(|error| format!("PR 295 configure generation-A resolve: {error}"))?;
    let old_market_id = env.primary_market_state().1.assets[0].market_id;
    env.trade_no_cpi(VICTIM, ATTACKER, 0, SIZE_Q, PRICE, 0)
        .map_err(|error| format!("PR 295 open generation-A position: {error}"))?;
    env.warp_to_slot(2);
    env.shutdown_asset(0, 2)
        .map_err(|error| format!("PR 295 shutdown generation A: {error}"))?;
    let retained = env.build_retained_forfeit_recovery_leg(VICTIM, 0, 1);
    for actor in [VICTIM, ATTACKER] {
        env.forfeit_recovery_leg(actor, 0, 1)
            .map_err(|error| format!("PR 295 forfeit generation-A actor {actor}: {error}"))?;
        env.withdraw_primary(actor, DEPOSIT)
            .map_err(|error| format!("PR 295 withdraw generation-A actor {actor}: {error}"))?;
        env.close_primary_portfolio(actor)
            .map_err(|error| format!("PR 295 close generation-A actor {actor}: {error}"))?;
    }
    for actor in 2..PRIMARY_ACTOR_COUNT {
        env.withdraw_primary(actor, 1)
            .map_err(|error| format!("PR 295 empty unused actor {actor}: {error}"))?;
        env.close_primary_portfolio(actor)
            .map_err(|error| format!("PR 295 close unused actor {actor}: {error}"))?;
    }
    env.resolve_market()
        .map_err(|error| format!("PR 295 resolve generation A: {error}"))?;
    env.close_primary_slab()
        .map_err(|error| format!("PR 295 close generation-A slab: {error}"))?;

    env.warp_to_slot(REINIT_SLOT);
    env.fund_closed_primary_market()
        .map_err(|error| format!("PR 295 System-fund replacement market: {error}"))?;
    env.recreate_primary_vault()
        .map_err(|error| format!("PR 295 recreate canonical vault: {error}"))?;
    env.reinitialize_primary_market(config)
        .map_err(|error| format!("PR 295 initialize generation B: {error}"))?;
    env.configure_permissionless_resolve(100, 1)
        .map_err(|error| format!("PR 295 configure generation-B resolve: {error}"))?;
    env.configure_auth_mark(false, 0, REINIT_SLOT, PRICE)
        .map_err(|error| format!("PR 295 configure generation-B AuthMark: {error}"))?;
    let new_market_id = env.primary_market_state().1.assets[0].market_id;
    for actor in [VICTIM, ATTACKER] {
        env.fund_closed_primary_portfolio(actor, 1_000_000_000)
            .map_err(|error| format!("PR 295 System-fund portfolio {actor}: {error}"))?;
        env.reinitialize_primary_portfolio(actor)
            .map_err(|error| format!("PR 295 initialize portfolio {actor}: {error}"))?;
        env.deposit_primary(actor, DEPOSIT)
            .map_err(|error| format!("PR 295 deposit generation-B actor {actor}: {error}"))?;
    }
    env.trade_no_cpi(VICTIM, ATTACKER, 0, SIZE_Q, PRICE, 0)
        .map_err(|error| format!("PR 295 open generation-B position: {error}"))?;

    let outcome = finish_forfeit_replay_terminal(
        &mut env,
        retained,
        land_replay,
        false,
        REINIT_SLOT + 10,
        REINIT_SLOT + 11,
        REINIT_SLOT + 12,
        supply_before,
        "PR 295",
    )?;
    Ok(ForfeitMarketGenerationWorld {
        old_market_id,
        new_market_id,
        victim_payout: outcome.victim_payout,
        attacker_payout: outcome.attacker_payout,
        vault_remaining: outcome.vault_remaining,
        slab_closed: outcome.slab_closed,
        replay_cu: outcome.replay_cu,
        max_cu: outcome.max_cu,
    })
}

pub fn reproduce_forfeit_market_generation_replay(
    mut seed: [u8; 32],
) -> Result<ForfeitMarketGenerationReplayReproduction, String> {
    seed[0] ^= 0x95;
    let control = run_forfeit_market_generation_world(seed, false)?;
    let replay = run_forfeit_market_generation_world(seed, true)?;
    let victim_loss = control
        .victim_payout
        .checked_sub(replay.victim_payout)
        .ok_or("PR 295 replay increased victim payout")?;
    if control.old_market_id != replay.old_market_id
        || control.new_market_id != replay.new_market_id
        || control.victim_payout.checked_add(control.attacker_payout) != Some(2_000_000)
        || replay.victim_payout != 1_000_000
        || control.attacker_payout != replay.attacker_payout
        || victim_loss == 0
        || control.vault_remaining != 0
        || replay.vault_remaining != u128::from(victim_loss)
        || !control.slab_closed
        || replay.slab_closed
        || replay.replay_cu == 0
    {
        return Err(format!(
            "PR 295 paired-world mismatch: control={control:?}, replay={replay:?}, \
             victim_loss={victim_loss}"
        ));
    }
    Ok(ForfeitMarketGenerationReplayReproduction {
        blocker: KnownBlocker::ForfeitMarketGenerationReplay,
        old_market_id: replay.old_market_id,
        new_market_id: replay.new_market_id,
        victim_loss,
        stranded_vault: replay.vault_remaining,
        control_slab_closed: control.slab_closed,
        replay_slab_blocked: !replay.slab_closed,
        replay_cu: replay.replay_cu,
        max_cu: replay.max_cu.max(control.max_cu),
    })
}

pub fn reproduce_fee_redirect_generation_replay(
    seed: [u8; 32],
) -> Result<FeeRedirectGenerationReplayReproduction, String> {
    const VICTIM: usize = 0;
    const ATTACKER: usize = 1;
    const ACTIVATION_PAYER: usize = 2;
    const ASSET: u16 = 1;
    const PRICE: u64 = 100;
    const DEPOSIT: u128 = 10_000;
    const SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const TRADE_FEE_BPS: u64 = 10_000;
    const FEE_PER_SIDE: u64 = 1_000;
    const TOTAL_FEE: u128 = 2 * FEE_PER_SIDE as u128;
    const REINIT_SLOT: u64 = 10;
    const RETIRE_SLOT: u64 = 11;
    const ACTIVATE_SLOT: u64 = 12;

    let config = MarketConfig {
        initial_price: PRICE,
        max_trading_fee_bps: TRADE_FEE_BPS,
        actor_deposits: [1, 1, 1, 1, 1],
        ..MarketConfig::default()
    };
    let mut env = V16Svm::new(seed, config);
    let supply_before = env.token_supply_observed();
    let old_market_id = env.primary_market_state().1.assets[0].market_id;
    env.update_fee_redirect_policy(10_000)
        .map_err(|error| format!("PR 317 seed generation-A redirect sequence: {error}"))?;
    let retained_policy = env.build_retained_fee_redirect_policy(0);

    for actor in 0..PRIMARY_ACTOR_COUNT {
        env.withdraw_primary(actor, 1)
            .map_err(|error| format!("PR 317 empty generation-A portfolio {actor}: {error}"))?;
        env.close_primary_portfolio(actor)
            .map_err(|error| format!("PR 317 close generation-A portfolio {actor}: {error}"))?;
    }
    env.resolve_market()
        .map_err(|error| format!("PR 317 resolve generation A: {error}"))?;
    env.close_primary_slab()
        .map_err(|error| format!("PR 317 close generation-A slab: {error}"))?;

    env.warp_to_slot(REINIT_SLOT);
    env.fund_closed_primary_market()
        .map_err(|error| format!("PR 317 re-fund closed market: {error}"))?;
    env.recreate_primary_vault()
        .map_err(|error| format!("PR 317 recreate canonical vault: {error}"))?;
    env.reinitialize_primary_market(config)
        .map_err(|error| format!("PR 317 initialize generation B: {error}"))?;
    env.configure_auth_mark(false, 0, REINIT_SLOT, PRICE)
        .map_err(|error| format!("PR 317 configure generation-B market 0: {error}"))?;
    let new_market_id = env.primary_market_state().1.assets[0].market_id;

    env.update_trade_fee_policy(TRADE_FEE_BPS)
        .map_err(|error| format!("PR 317 install generation-B trade fee: {error}"))?;
    env.update_fee_redirect_policy(10_000)
        .map_err(|error| format!("PR 317 protect generation-B fee revenue: {error}"))?;
    env.update_market_init_fee_policy(1)
        .map_err(|error| format!("PR 317 configure permissionless init fee: {error}"))?;
    env.warp_to_slot(RETIRE_SLOT);
    env.retire_asset(ASSET, RETIRE_SLOT)
        .map_err(|error| format!("PR 317 retire empty generation-B asset slot: {error}"))?;
    env.warp_to_slot(ACTIVATE_SLOT);
    env.activate_permissionless_asset_with_actor_authorities(
        ACTIVATION_PAYER,
        ASSET,
        ACTIVATE_SLOT,
        PRICE,
        ATTACKER,
        ATTACKER,
        ATTACKER,
        ATTACKER,
        1,
    )
    .map_err(|error| format!("PR 317 activate attacker-operated asset: {error}"))?;

    for actor in [VICTIM, ATTACKER] {
        env.fund_closed_primary_portfolio(actor, 1_000_000_000)
            .map_err(|error| format!("PR 317 re-fund portfolio {actor}: {error}"))?;
        env.reinitialize_primary_portfolio(actor)
            .map_err(|error| format!("PR 317 initialize portfolio {actor}: {error}"))?;
        env.deposit_primary(actor, DEPOSIT)
            .map_err(|error| format!("PR 317 deposit actor {actor}: {error}"))?;
    }
    if env.primary_market_state().0.fee_redirect_to_market_0_bps != 10_000 {
        return Err("PR 317 replacement fee redirect was not installed".into());
    }

    let replay = env
        .land_retained(retained_policy)
        .map_err(|error| format!("PR 317 stale redirect policy no longer lands: {error}"))?;
    if env.primary_market_state().0.fee_redirect_to_market_0_bps != 0 {
        return Err("PR 317 stale policy did not disable the replacement redirect".into());
    }

    let trade = env
        .trade_no_cpi(VICTIM, ATTACKER, ASSET, SIZE_Q, PRICE, TRADE_FEE_BPS)
        .map_err(|error| format!("PR 317 charge replacement users: {error}"))?;
    let (_, group_after_trade) = env.primary_market_state();
    let attacker_domain_fee = group_after_trade.insurance_domain_budget[ASSET as usize * 2]
        .checked_add(group_after_trade.insurance_domain_budget[ASSET as usize * 2 + 1])
        .ok_or("PR 317 attacker domain fee overflow")?;
    let protected_fee = group_after_trade.insurance_domain_budget[0]
        .checked_add(group_after_trade.insurance_domain_budget[1])
        .ok_or("PR 317 protected fee overflow")?;
    if attacker_domain_fee != TOTAL_FEE || protected_fee != 1 {
        return Err(format!(
            "PR 317 stale redirect credited wrong domains: attacker={attacker_domain_fee}, \
             protected={protected_fee}"
        ));
    }

    let victim_destination = env.actors[VICTIM].destination_token;
    let attacker_destination = env.actors[ATTACKER].destination_token;
    let victim_destination_before = env.token_amount(victim_destination);
    let attacker_destination_before = env.token_amount(attacker_destination);
    let withdrawal = env
        .withdraw_insurance_asset(ATTACKER, ASSET, TOTAL_FEE)
        .map_err(|error| format!("PR 317 attacker could not withdraw redirected fees: {error}"))?;

    env.update_trade_fee_policy(0)
        .map_err(|error| format!("PR 317 disable fee before neutral close: {error}"))?;
    env.trade_no_cpi(VICTIM, ATTACKER, ASSET, -SIZE_Q, PRICE, 0)
        .map_err(|error| format!("PR 317 close replacement risk: {error}"))?;
    let victim_capital = env.primary_portfolio(VICTIM).capital.get();
    let attacker_capital = env.primary_portfolio(ATTACKER).capital.get();
    env.withdraw_primary(VICTIM, victim_capital)
        .map_err(|error| format!("PR 317 victim terminal withdrawal: {error}"))?;
    env.withdraw_primary(ATTACKER, attacker_capital)
        .map_err(|error| format!("PR 317 attacker terminal withdrawal: {error}"))?;

    let victim_return = env
        .token_amount(victim_destination)
        .checked_sub(victim_destination_before)
        .ok_or("PR 317 victim destination decreased")?;
    let attacker_return = env
        .token_amount(attacker_destination)
        .checked_sub(attacker_destination_before)
        .ok_or("PR 317 attacker destination decreased")?;
    let victim_loss = u64::try_from(DEPOSIT)
        .map_err(|_| "PR 317 deposit does not fit u64")?
        .checked_sub(victim_return)
        .ok_or("PR 317 victim returned more than deposited")?;
    let attacker_profit = attacker_return
        .checked_sub(u64::try_from(DEPOSIT).map_err(|_| "PR 317 deposit does not fit u64")?)
        .ok_or("PR 317 attacker did not recover its deposit")?;
    let max_cu = replay
        .compute_units
        .max(trade.compute_units)
        .max(withdrawal.compute_units);
    if victim_loss != FEE_PER_SIDE
        || attacker_profit != victim_loss
        || attacker_return != DEPOSIT as u64 + FEE_PER_SIDE
        || max_cu >= TX_CU_LIMIT
        || env.token_supply_observed() != supply_before
    {
        return Err(format!(
            "PR 317 terminal extraction mismatch: victim_loss={victim_loss}, \
             attacker_profit={attacker_profit}, attacker_return={attacker_return}, \
             max_cu={max_cu}, supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }

    Ok(FeeRedirectGenerationReplayReproduction {
        blocker: KnownBlocker::FeeRedirectGenerationReplay,
        old_market_id,
        new_market_id,
        victim_loss,
        attacker_profit,
        redirected_fee: attacker_domain_fee,
        replay_cu: replay.compute_units,
        trade_cu: trade.compute_units,
        withdrawal_cu: withdrawal.compute_units,
    })
}

pub fn reproduce_backing_fee_generation_replay(
    seed: [u8; 32],
) -> Result<BackingFeeGenerationReplayReproduction, String> {
    const VICTIM: usize = 0;
    const ATTACKER: usize = 1;
    const OLD_POLICY_AUTHORITY: usize = 2;
    const ACTIVATION_PAYER: usize = 3;
    const ASSET: u16 = 1;
    const WINNING_DOMAIN: u16 = ASSET * 2 + 1;
    const INITIAL_PRICE: u64 = 100;
    const ASSET_WIN_MARK: u64 = 105;
    const BASE_LOSS_MARK: u64 = 95;
    const ASSET_SIZE_Q: i128 = 2_000 * POS_SCALE as i128;
    const BASE_SIZE_Q: i128 = 1_000 * POS_SCALE as i128;
    const SAFE_INCREASE_Q: i128 = 100 * POS_SCALE as i128;
    const VICTIM_DEPOSIT: u128 = 31_300;
    const ATTACKER_DEPOSIT: u128 = 100_000;
    const BACKING_PRINCIPAL: u128 = 15_000;
    const FORCED_FEE_BPS: u16 = 5_000;
    const EXPECTED_FEE: u64 = 75;

    let config = MarketConfig {
        initial_price: INITIAL_PRICE,
        maintenance_margin_bps: 1_000,
        initial_margin_bps: 1_000,
        max_price_move_bps_per_slot: 500,
        max_accrual_dt_slots: 1,
        min_funding_lifetime_slots: 1,
        actor_deposits: [VICTIM_DEPOSIT, ATTACKER_DEPOSIT, 1, 1, 1],
        ..MarketConfig::default()
    };
    let mut env = V16Svm::new(seed, config);
    let supply_before = env.token_supply_observed();
    env.update_asset_authority_from_admin(
        ASSET,
        percolator_prog::processor::ASSET_AUTH_INSURANCE,
        OLD_POLICY_AUTHORITY,
    )
    .map_err(|error| format!("PR 318 install generation-A policy authority: {error}"))?;
    let old_market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    let retained_policy = env.build_retained_backing_fee_policy_for_actor(
        OLD_POLICY_AUTHORITY,
        WINNING_DOMAIN,
        FORCED_FEE_BPS,
        0,
    );

    env.update_market_init_fee_policy(1)
        .map_err(|error| format!("PR 318 configure permissionless init fee: {error}"))?;
    env.warp_to_slot(2);
    env.retire_asset(ASSET, 2)
        .map_err(|error| format!("PR 318 retire generation-A asset: {error}"))?;
    env.warp_to_slot(3);
    env.activate_permissionless_asset_with_actor_authorities(
        ACTIVATION_PAYER,
        ASSET,
        3,
        INITIAL_PRICE,
        OLD_POLICY_AUTHORITY,
        ATTACKER,
        ATTACKER,
        ATTACKER,
        1,
    )
    .map_err(|error| format!("PR 318 activate attacker-backed replacement: {error}"))?;
    let new_market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    if new_market_id == old_market_id {
        return Err(format!(
            "PR 318 replacement reused asset market ID {old_market_id}"
        ));
    }
    env.configure_auth_mark(false, 0, 3, INITIAL_PRICE)
        .map_err(|error| format!("PR 318 configure base AuthMark: {error}"))?;
    env.configure_auth_mark_for_actor(ATTACKER, ASSET, 3, INITIAL_PRICE)
        .map_err(|error| format!("PR 318 configure replacement AuthMark: {error}"))?;
    env.top_up_backing_bucket_for_actor(ATTACKER, WINNING_DOMAIN, BACKING_PRINCIPAL, 100)
        .map_err(|error| format!("PR 318 fund replacement backing: {error}"))?;

    env.trade_no_cpi(VICTIM, ATTACKER, ASSET, ASSET_SIZE_Q, INITIAL_PRICE, 0)
        .map_err(|error| format!("PR 318 establish source-backed winning leg: {error}"))?;
    env.trade_no_cpi(VICTIM, ATTACKER, 0, BASE_SIZE_Q, INITIAL_PRICE, 0)
        .map_err(|error| format!("PR 318 establish offsetting losing leg: {error}"))?;

    env.warp_to_slot(4);
    env.push_auth_mark_for_actor(ATTACKER, ASSET, 4, ASSET_WIN_MARK)
        .map_err(|error| format!("PR 318 push replacement winning mark: {error}"))?;
    env.push_auth_mark(0, 4, BASE_LOSS_MARK)
        .map_err(|error| format!("PR 318 push base losing mark: {error}"))?;
    for (actor, asset_index) in [(ATTACKER, ASSET), (VICTIM, ASSET), (ATTACKER, 0)] {
        let oracle_accounts = env.primary_profile(asset_index as usize).oracle_leg_count;
        env.crank(
            actor,
            4,
            vec![CrankObservationHint {
                asset_index,
                oracle_accounts,
            }],
        )
        .map_err(|error| format!("PR 318 crank actor {actor} asset {asset_index}: {error}"))?;
    }
    if env.primary_portfolio(VICTIM).pnl.get() != 10_000 {
        return Err(format!(
            "PR 318 victim source-backed claim mismatch: {}",
            env.primary_portfolio(VICTIM).pnl.get()
        ));
    }

    let retained_trade =
        env.build_retained_no_cpi_trade(VICTIM, ATTACKER, 0, SAFE_INCREASE_Q, BASE_LOSS_MARK);
    let victim_capital_before = env.primary_portfolio(VICTIM).capital.get();
    let attacker_capital_before = env.primary_portfolio(ATTACKER).capital.get();
    let earnings_before = env.primary_market_state().1.source_backing_buckets
        [WINNING_DOMAIN as usize]
        .utilization_fee_earnings;

    let replay = env
        .land_retained(retained_policy)
        .map_err(|error| format!("PR 318 stale backing policy no longer lands: {error}"))?;
    let replacement_profile = env.primary_profile(ASSET as usize);
    if replacement_profile.backing_trade_fee_bps_short != FORCED_FEE_BPS
        || replacement_profile.backing_trade_fee_insurance_share_bps_short != 0
    {
        return Err("PR 318 stale policy did not mutate the replacement fee".into());
    }

    let trade = env
        .land_retained(retained_trade)
        .map_err(|error| format!("PR 318 victim pre-signed trade rejected: {error}"))?;
    let victim_loss = victim_capital_before
        .checked_sub(env.primary_portfolio(VICTIM).capital.get())
        .ok_or("PR 318 victim capital increased")?;
    let attacker_capital_after = env.primary_portfolio(ATTACKER).capital.get();
    let earnings_after = env.primary_market_state().1.source_backing_buckets
        [WINNING_DOMAIN as usize]
        .utilization_fee_earnings;
    let backing_earnings = earnings_after
        .checked_sub(earnings_before)
        .ok_or("PR 318 backing earnings decreased")?;
    if victim_loss != EXPECTED_FEE as u128
        || backing_earnings != victim_loss
        || attacker_capital_after != attacker_capital_before
    {
        return Err(format!(
            "PR 318 stale fee transfer mismatch: victim={victim_loss}, \
             earnings={backing_earnings}, attacker_capital={attacker_capital_before}/\
             {attacker_capital_after}"
        ));
    }

    let attacker_destination = env.actors[ATTACKER].destination_token;
    let destination_before = env.token_amount(attacker_destination);
    let withdrawal = env
        .withdraw_backing_bucket_earnings_for_actor(ATTACKER, WINNING_DOMAIN, backing_earnings)
        .map_err(|error| format!("PR 318 attacker could not withdraw victim fee: {error}"))?;
    let attacker_extraction = env
        .token_amount(attacker_destination)
        .checked_sub(destination_before)
        .ok_or("PR 318 attacker destination decreased")?;
    let max_cu = replay
        .compute_units
        .max(trade.compute_units)
        .max(withdrawal.compute_units);
    if attacker_extraction != EXPECTED_FEE
        || u128::from(attacker_extraction) != victim_loss
        || max_cu >= TX_CU_LIMIT
        || env.token_supply_observed() != supply_before
    {
        return Err(format!(
            "PR 318 public extraction mismatch: victim={victim_loss}, \
             extraction={attacker_extraction}, max_cu={max_cu}, supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }

    Ok(BackingFeeGenerationReplayReproduction {
        blocker: KnownBlocker::BackingFeeGenerationReplay,
        old_market_id,
        new_market_id,
        victim_loss: victim_loss as u64,
        attacker_extraction,
        backing_earnings,
        replay_cu: replay.compute_units,
        trade_cu: trade.compute_units,
        withdrawal_cu: withdrawal.compute_units,
    })
}

#[derive(Clone, Copy, Debug)]
struct BackingTopUpRetryWorld {
    provider_loss: u64,
    winner_payout: u128,
    loser_payout: u128,
    replay_cu: u64,
}

fn run_backing_top_up_retry_world(
    seed: [u8; 32],
    land_retry: bool,
) -> Result<BackingTopUpRetryWorld, String> {
    const ASSET: u16 = 0;
    const DOMAIN: u16 = 1;
    const WINNER: usize = 0;
    const LOSER: usize = 1;
    const PROVIDER: usize = 2;
    const PUBLISHER: usize = 3;
    const BACKING: u128 = 500;
    const SIZE_Q: i128 = 10 * POS_SCALE as i128;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: 100,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [1_000, 1_000, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.update_asset_authority_from_admin(
        ASSET,
        percolator_prog::processor::ASSET_AUTH_BACKING_BUCKET,
        PROVIDER,
    )
    .map_err(|error| format!("PR 351 install backing provider: {error}"))?;
    let provider_source = env.actors[PROVIDER].source_token;
    let source_before = env.token_amount(provider_source);
    let intended =
        env.build_retained_backing_bucket_top_up_for_actor(PROVIDER, DOMAIN, BACKING, 10_000);
    let retry_variant =
        env.build_retained_backing_bucket_top_up_for_actor(PROVIDER, DOMAIN, BACKING, 10_000);
    env.land_retained(intended)
        .map_err(|error| format!("PR 351 intended top-up rejected: {error}"))?;

    env.trade_no_cpi(WINNER, LOSER, ASSET, SIZE_Q, 100, 0)
        .map_err(|error| format!("PR 351 open winner/loser positions: {error}"))?;
    for (slot, mark) in [(2, 200), (3, 350)] {
        env.warp_to_slot(slot);
        env.push_auth_mark(ASSET, slot, mark)
            .map_err(|error| format!("PR 351 publish mark {mark}: {error}"))?;
        let observation = vec![CrankObservationHint {
            asset_index: ASSET,
            oracle_accounts: 0,
        }];
        for _ in 0..8 {
            if env.crank(PUBLISHER, slot, observation.clone()).is_err() {
                break;
            }
            if env.primary_market_state().1.assets[ASSET as usize].effective_price == mark {
                break;
            }
        }
        if env.primary_market_state().1.assets[ASSET as usize].effective_price != mark {
            return Err(format!(
                "PR 351 public crank did not commit mark {mark} at slot {slot}"
            ));
        }
    }

    let replay_cu = if land_retry {
        env.land_retained(retry_variant)
            .map_err(|error| format!("PR 351 retained top-up no longer lands: {error}"))?
            .compute_units
    } else {
        0
    };
    let provider_loss = source_before
        .checked_sub(env.token_amount(provider_source))
        .ok_or("PR 351 provider source increased")?;
    let expected_loss = if land_retry { BACKING * 2 } else { BACKING };
    if u128::from(provider_loss) != expected_loss {
        return Err(format!(
            "PR 351 provider debit mismatch: replay={land_retry}, loss={provider_loss}, \
             expected={expected_loss}"
        ));
    }

    env.resolve_market()
        .map_err(|error| format!("PR 351 resolve terminal world: {error}"))?;
    let (first_winner_payout, _) = drain_resolved_actor(&mut env, WINNER)?;
    let (loser_payout, _) = drain_resolved_actor(&mut env, LOSER)?;
    let (winner_top_up, _) = drain_resolved_actor(&mut env, WINNER)?;
    let winner_payout = first_winner_payout
        .checked_add(winner_top_up)
        .ok_or("PR 351 winner payout overflow")?;
    if loser_payout != 0 || env.token_supply_observed() != supply_before {
        return Err(format!(
            "PR 351 terminal world mismatch: replay={land_retry}, winner={winner_payout}, \
             loser={loser_payout}, supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }
    Ok(BackingTopUpRetryWorld {
        provider_loss,
        winner_payout,
        loser_payout,
        replay_cu,
    })
}

pub fn reproduce_backing_top_up_retry_replay(
    seed: [u8; 32],
) -> Result<BackingTopUpRetryReplayReproduction, String> {
    let control = run_backing_top_up_retry_world(seed, false)?;
    let replay = run_backing_top_up_retry_world(seed, true)?;
    let duplicate_loss = replay
        .provider_loss
        .checked_sub(control.provider_loss)
        .ok_or("PR 351 replay provider loss below control")?;
    let beneficiary_extra_payout = replay
        .winner_payout
        .checked_sub(control.winner_payout)
        .ok_or("PR 351 replay winner payout below control")?;
    if control.provider_loss != 500
        || control.winner_payout != 2_500
        || replay.provider_loss != 1_000
        || replay.winner_payout != 3_000
        || control.loser_payout != 0
        || replay.loser_payout != 0
        || u128::from(duplicate_loss) != beneficiary_extra_payout
        || replay.replay_cu >= TX_CU_LIMIT
    {
        return Err(format!(
            "PR 351 paired-world mismatch: control_loss={}, replay_loss={}, \
             control_winner={}, replay_winner={}, control_loser={}, replay_loser={}, replay_cu={}",
            control.provider_loss,
            replay.provider_loss,
            control.winner_payout,
            replay.winner_payout,
            control.loser_payout,
            replay.loser_payout,
            replay.replay_cu
        ));
    }
    Ok(BackingTopUpRetryReplayReproduction {
        blocker: KnownBlocker::BackingTopUpRetryReplay,
        intended_contribution: control.provider_loss,
        duplicate_loss,
        beneficiary_extra_payout,
        control_winner_payout: control.winner_payout,
        replay_winner_payout: replay.winner_payout,
        replay_cu: replay.replay_cu,
    })
}

#[derive(Clone, Copy, Debug)]
struct DepositRetryWorld {
    source_loss: u64,
    winner_payout: u128,
    victim_payout: u128,
    replay_cu: u64,
}

fn run_deposit_retry_world(seed: [u8; 32], land_retry: bool) -> Result<DepositRetryWorld, String> {
    const VICTIM: usize = 0;
    const WINNER: usize = 1;
    const PUBLISHER: usize = 2;
    const INITIAL_CAPITAL: u128 = 1_000;
    const TOP_UP: u128 = 500;
    const POSITION_Q: i128 = 10 * POS_SCALE as i128;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: 100,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [INITIAL_CAPITAL, INITIAL_CAPITAL, 1, 1, 1],
            actor_token_balances: [2_000, 1_000, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.configure_auth_mark(false, 0, 0, 100)
        .map_err(|error| format!("PR 350 configure AuthMark: {error}"))?;
    env.trade_no_cpi(VICTIM, WINNER, 0, -POSITION_Q, 100, 0)
        .map_err(|error| format!("PR 350 open victim/winner positions: {error}"))?;

    let source = env.actors[VICTIM].source_token;
    let source_before = env.token_amount(source);
    let intended = env.build_retained_deposit(VICTIM, TOP_UP);
    let retry_variant = env.build_retained_deposit(VICTIM, TOP_UP);
    env.land_retained(intended)
        .map_err(|error| format!("PR 350 intended deposit rejected: {error}"))?;

    for (slot, mark) in [(2, 200), (3, 300)] {
        env.warp_to_slot(slot);
        env.push_auth_mark(0, slot, mark)
            .map_err(|error| format!("PR 350 publish mark {mark}: {error}"))?;
        env.crank(
            PUBLISHER,
            slot,
            vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: 0,
            }],
        )
        .map_err(|error| format!("PR 350 commit mark {mark}: {error}"))?;
        if env.primary_market_state().1.assets[0].effective_price != mark {
            return Err(format!(
                "PR 350 mark did not commit: slot={slot}, expected={mark}, actual={}",
                env.primary_market_state().1.assets[0].effective_price
            ));
        }
    }

    let replay_cu = if land_retry {
        env.land_retained(retry_variant)
            .map_err(|error| format!("PR 350 retained deposit no longer lands: {error}"))?
            .compute_units
    } else {
        0
    };
    let source_loss = source_before
        .checked_sub(env.token_amount(source))
        .ok_or("PR 350 victim source increased")?;
    let expected_loss = if land_retry { TOP_UP * 2 } else { TOP_UP };
    if u128::from(source_loss) != expected_loss {
        return Err(format!(
            "PR 350 source debit mismatch: replay={land_retry}, loss={source_loss}, \
             expected={expected_loss}"
        ));
    }

    env.resolve_market()
        .map_err(|error| format!("PR 350 resolve terminal world: {error}"))?;
    let (first_winner_payout, _) = drain_resolved_actor(&mut env, WINNER)?;
    let (victim_payout, _) = drain_resolved_actor(&mut env, VICTIM)?;
    let (winner_top_up, _) = drain_resolved_actor(&mut env, WINNER)?;
    let winner_payout = first_winner_payout
        .checked_add(winner_top_up)
        .ok_or("PR 350 winner payout overflow")?;
    if victim_payout != 0 || env.token_supply_observed() != supply_before {
        return Err(format!(
            "PR 350 terminal world mismatch: replay={land_retry}, winner={winner_payout}, \
             victim={victim_payout}, supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }
    Ok(DepositRetryWorld {
        source_loss,
        winner_payout,
        victim_payout,
        replay_cu,
    })
}

pub fn reproduce_deposit_retry_replay(
    seed: [u8; 32],
) -> Result<DepositRetryReplayReproduction, String> {
    let control = run_deposit_retry_world(seed, false)?;
    let replay = run_deposit_retry_world(seed, true)?;
    let duplicate_loss = replay
        .source_loss
        .checked_sub(control.source_loss)
        .ok_or("PR 350 replay source loss below control")?;
    let beneficiary_extra_payout = replay
        .winner_payout
        .checked_sub(control.winner_payout)
        .ok_or("PR 350 replay winner payout below control")?;
    if control.source_loss != 500
        || control.winner_payout != 2_500
        || replay.source_loss != 1_000
        || replay.winner_payout != 3_000
        || control.victim_payout != 0
        || replay.victim_payout != 0
        || u128::from(duplicate_loss) != beneficiary_extra_payout
        || replay.replay_cu >= TX_CU_LIMIT
    {
        return Err(format!(
            "PR 350 paired-world mismatch: control_loss={}, replay_loss={}, \
             control_winner={}, replay_winner={}, control_victim={}, replay_victim={}, \
             replay_cu={}",
            control.source_loss,
            replay.source_loss,
            control.winner_payout,
            replay.winner_payout,
            control.victim_payout,
            replay.victim_payout,
            replay.replay_cu
        ));
    }
    Ok(DepositRetryReplayReproduction {
        blocker: KnownBlocker::DepositRetryReplay,
        intended_contribution: control.source_loss,
        duplicate_loss,
        beneficiary_extra_payout,
        control_winner_payout: control.winner_payout,
        replay_winner_payout: replay.winner_payout,
        replay_cu: replay.replay_cu,
    })
}

#[derive(Clone, Copy, Debug)]
struct PortfolioIncarnationDepositWorld {
    old_portfolio_id: u64,
    new_portfolio_id: u64,
    source_loss: u64,
    winner_payout: u128,
    victim_payout: u128,
    replay_cu: u64,
}

fn run_portfolio_incarnation_deposit_world(
    seed: [u8; 32],
    land_replay: bool,
) -> Result<PortfolioIncarnationDepositWorld, String> {
    const VICTIM: usize = 0;
    const WINNER: usize = 1;
    const PUBLISHER: usize = 2;
    const REPLACEMENT_CAPITAL: u128 = 150_000;
    const STALE_DEPOSIT: u128 = 100_000;
    const POSITION_Q: i128 = 1_000 * POS_SCALE as i128;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: 100,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [1, REPLACEMENT_CAPITAL, 1, 1, 1],
            actor_token_balances: [250_001, 150_000, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    let old_portfolio_id = env.primary_portfolio_id(VICTIM);
    let source = env.actors[VICTIM].source_token;
    let source_before = env.token_amount(source);
    let retained = env.build_retained_deposit(VICTIM, STALE_DEPOSIT);
    env.withdraw_primary(VICTIM, 1)
        .map_err(|error| format!("PR 305 empty incarnation A: {error}"))?;
    env.close_primary_portfolio(VICTIM)
        .map_err(|error| format!("PR 305 close incarnation A: {error}"))?;
    env.fund_closed_primary_portfolio(VICTIM, 1_000_000_000)
        .map_err(|error| format!("PR 305 re-fund closed portfolio: {error}"))?;
    env.reinitialize_primary_portfolio(VICTIM)
        .map_err(|error| format!("PR 305 initialize incarnation B: {error}"))?;
    let new_portfolio_id = env.primary_portfolio_id(VICTIM);
    if new_portfolio_id <= old_portfolio_id {
        return Err(format!(
            "PR 305 portfolio incarnation did not advance: {old_portfolio_id} -> \
             {new_portfolio_id}"
        ));
    }
    env.deposit_primary(VICTIM, REPLACEMENT_CAPITAL)
        .map_err(|error| format!("PR 305 fund incarnation B: {error}"))?;
    env.trade_no_cpi(VICTIM, WINNER, 0, -POSITION_Q, 100, 0)
        .map_err(|error| format!("PR 305 open replacement risk: {error}"))?;

    for (slot, mark) in [(2, 200), (3, 350)] {
        env.warp_to_slot(slot);
        env.push_auth_mark(0, slot, mark)
            .map_err(|error| format!("PR 305 publish mark {mark}: {error}"))?;
        env.crank(
            PUBLISHER,
            slot,
            vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: 0,
            }],
        )
        .map_err(|error| format!("PR 305 commit mark {mark}: {error}"))?;
        if env.primary_market_state().1.assets[0].effective_price != mark {
            return Err(format!(
                "PR 305 mark did not commit: slot={slot}, expected={mark}, actual={}",
                env.primary_market_state().1.assets[0].effective_price
            ));
        }
    }

    let replay_cu = if land_replay {
        env.land_retained(retained)
            .map_err(|error| format!("PR 305 incarnation-A deposit no longer lands: {error}"))?
            .compute_units
    } else {
        0
    };
    let source_loss = source_before
        .checked_sub(env.token_amount(source))
        .ok_or("PR 305 victim source increased")?;
    let expected_loss = if land_replay {
        REPLACEMENT_CAPITAL + STALE_DEPOSIT
    } else {
        REPLACEMENT_CAPITAL
    };
    if u128::from(source_loss) != expected_loss {
        return Err(format!(
            "PR 305 source debit mismatch: replay={land_replay}, loss={source_loss}, \
             expected={expected_loss}"
        ));
    }

    env.resolve_market()
        .map_err(|error| format!("PR 305 resolve terminal world: {error}"))?;
    let (first_winner_payout, _) = drain_resolved_actor(&mut env, WINNER)?;
    let (victim_payout, _) = drain_resolved_actor(&mut env, VICTIM)?;
    let (winner_top_up, _) = drain_resolved_actor(&mut env, WINNER)?;
    let winner_payout = first_winner_payout
        .checked_add(winner_top_up)
        .ok_or("PR 305 winner payout overflow")?;
    if victim_payout != 0 || env.token_supply_observed() != supply_before {
        return Err(format!(
            "PR 305 terminal world mismatch: replay={land_replay}, winner={winner_payout}, \
             victim={victim_payout}, supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }
    Ok(PortfolioIncarnationDepositWorld {
        old_portfolio_id,
        new_portfolio_id,
        source_loss,
        winner_payout,
        victim_payout,
        replay_cu,
    })
}

pub fn reproduce_portfolio_incarnation_deposit(
    seed: [u8; 32],
) -> Result<PortfolioIncarnationDepositReproduction, String> {
    let control = run_portfolio_incarnation_deposit_world(seed, false)?;
    let replay = run_portfolio_incarnation_deposit_world(seed, true)?;
    let stale_deposit = replay
        .source_loss
        .checked_sub(control.source_loss)
        .ok_or("PR 305 replay source loss below control")?;
    let beneficiary_extra_payout = replay
        .winner_payout
        .checked_sub(control.winner_payout)
        .ok_or("PR 305 replay winner payout below control")?;
    if control.old_portfolio_id != replay.old_portfolio_id
        || control.new_portfolio_id != replay.new_portfolio_id
        || control.source_loss != 150_000
        || replay.source_loss != 250_000
        || control.winner_payout != 300_000
        || replay.winner_payout != 400_000
        || control.victim_payout != 0
        || replay.victim_payout != 0
        || u128::from(stale_deposit) != beneficiary_extra_payout
        || replay.replay_cu >= TX_CU_LIMIT
    {
        return Err(format!(
            "PR 305 paired-world mismatch: control={control:?}, replay={replay:?}, \
             stale={stale_deposit}, beneficiary={beneficiary_extra_payout}"
        ));
    }
    Ok(PortfolioIncarnationDepositReproduction {
        blocker: KnownBlocker::PortfolioIncarnationDeposit,
        old_portfolio_id: replay.old_portfolio_id,
        new_portfolio_id: replay.new_portfolio_id,
        stale_deposit,
        beneficiary_extra_payout,
        control_winner_payout: control.winner_payout,
        replay_winner_payout: replay.winner_payout,
        replay_cu: replay.replay_cu,
    })
}

#[derive(Clone, Copy, Debug)]
struct MarketIncarnationDepositWorld {
    old_asset_market_id: u64,
    new_asset_market_id: u64,
    source_loss: u64,
    winner_payout: u128,
    victim_payout: u128,
    replay_cu: u64,
}

fn run_market_incarnation_deposit_world(
    seed: [u8; 32],
    land_replay: bool,
) -> Result<MarketIncarnationDepositWorld, String> {
    const VICTIM: usize = 0;
    const WINNER: usize = 1;
    const REPLACEMENT_CAPITAL: u128 = 150_000;
    const WINNER_CAPITAL: u128 = 100_000_000;
    const STALE_DEPOSIT: u128 = 100_000;
    const PRICE: u64 = 100;
    const ADVERSE_PRICE: u64 = 350;
    const REINIT_SLOT: u64 = 11;

    let config = MarketConfig {
        initial_price: PRICE,
        max_price_move_bps_per_slot: 10_000,
        max_accrual_dt_slots: 1,
        min_funding_lifetime_slots: 1,
        actor_deposits: [1, 1, 1, 1, 1],
        actor_token_balances: [250_001, 100_000_001, 1, 1, 1],
        ..MarketConfig::default()
    };
    let mut env = V16Svm::new(seed, config);
    let supply_before = env.token_supply_observed();
    let old_asset_market_id = env.primary_market_state().1.assets[0].market_id;
    let source = env.actors[VICTIM].source_token;
    let source_before = env.token_amount(source);
    let retained = env.build_retained_deposit(VICTIM, STALE_DEPOSIT);

    for actor in 0..PRIMARY_ACTOR_COUNT {
        env.withdraw_primary(actor, 1)
            .map_err(|error| format!("PR 307 empty generation-A portfolio {actor}: {error}"))?;
        env.close_primary_portfolio(actor)
            .map_err(|error| format!("PR 307 close generation-A portfolio {actor}: {error}"))?;
    }
    env.resolve_market()
        .map_err(|error| format!("PR 307 resolve generation A: {error}"))?;
    env.close_primary_slab()
        .map_err(|error| format!("PR 307 close generation-A slab: {error}"))?;

    env.warp_to_slot(REINIT_SLOT);
    env.fund_closed_primary_market()
        .map_err(|error| format!("PR 307 re-fund closed market account: {error}"))?;
    env.recreate_primary_vault()
        .map_err(|error| format!("PR 307 recreate canonical vault: {error}"))?;
    env.reinitialize_primary_market(config).map_err(|error| {
        let market = env.svm.get_account(&env.market);
        let mint = env.svm.get_account(&env.mint);
        format!(
            "PR 307 initialize generation-B market: {error}; market={:?}; mint={:?}",
            market
                .as_ref()
                .map(|account| (account.owner, account.lamports, account.data.len())),
            mint.as_ref()
                .map(|account| (account.owner, account.lamports, account.data.len()))
        )
    })?;
    env.configure_auth_mark(false, 0, REINIT_SLOT, PRICE)
        .map_err(|error| format!("PR 307 configure generation-B AuthMark: {error}"))?;
    let new_asset_market_id = env.primary_market_state().1.assets[0].market_id;

    for actor in [VICTIM, WINNER] {
        env.fund_closed_primary_portfolio(actor, 1_000_000_000)
            .map_err(|error| format!("PR 307 re-fund portfolio {actor}: {error}"))?;
        env.reinitialize_primary_portfolio(actor).map_err(|error| {
            format!("PR 307 initialize generation-B portfolio {actor}: {error}")
        })?;
    }
    env.deposit_primary(VICTIM, REPLACEMENT_CAPITAL)
        .map_err(|error| format!("PR 307 fund generation-B victim: {error}"))?;
    env.deposit_primary(WINNER, WINNER_CAPITAL)
        .map_err(|error| format!("PR 307 fund generation-B winner: {error}"))?;
    env.trade_no_cpi(WINNER, VICTIM, 0, 1_000 * POS_SCALE as i128, PRICE, 0)
        .map_err(|error| format!("PR 307 open generation-B risk: {error}"))?;

    let replay_cu = if land_replay {
        env.land_retained(retained)
            .map_err(|error| format!("PR 307 generation-A deposit no longer lands: {error}"))?
            .compute_units
    } else {
        0
    };
    let source_loss = source_before
        .checked_sub(env.token_amount(source))
        .ok_or("PR 307 victim source increased")?;
    let expected_loss = if land_replay {
        REPLACEMENT_CAPITAL + STALE_DEPOSIT
    } else {
        REPLACEMENT_CAPITAL
    };
    if u128::from(source_loss) != expected_loss {
        return Err(format!(
            "PR 307 source debit mismatch: replay={land_replay}, loss={source_loss}, \
             expected={expected_loss}"
        ));
    }

    let mut settlement_slot = REINIT_SLOT;
    for next in [200, ADVERSE_PRICE] {
        settlement_slot = settlement_slot
            .checked_add(1)
            .ok_or("PR 307 settlement slot overflow")?;
        env.warp_to_slot(settlement_slot);
        env.push_auth_mark(0, settlement_slot, next)
            .map_err(|error| format!("PR 307 publish adverse mark {next}: {error}"))?;
        env.crank(
            WINNER,
            settlement_slot,
            vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: 0,
            }],
        )
        .map_err(|error| format!("PR 307 commit adverse mark {next}: {error}"))?;
    }
    if env.primary_market_state().1.assets[0].effective_price != ADVERSE_PRICE {
        return Err(format!(
            "PR 307 adverse mark did not commit: {}",
            env.primary_market_state().1.assets[0].effective_price
        ));
    }

    env.resolve_market()
        .map_err(|error| format!("PR 307 resolve generation-B terminal world: {error}"))?;
    let (victim_payout, _) = drain_resolved_actor(&mut env, VICTIM)?;
    let (first_winner_payout, _) = drain_resolved_actor(&mut env, WINNER)?;
    let (winner_top_up, _) = drain_resolved_actor(&mut env, WINNER)?;
    let winner_payout = first_winner_payout
        .checked_add(winner_top_up)
        .ok_or("PR 307 winner payout overflow")?;
    if victim_payout != 0 || env.token_supply_observed() != supply_before {
        return Err(format!(
            "PR 307 terminal world mismatch: replay={land_replay}, winner={winner_payout}, \
             victim={victim_payout}, supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }
    Ok(MarketIncarnationDepositWorld {
        old_asset_market_id,
        new_asset_market_id,
        source_loss,
        winner_payout,
        victim_payout,
        replay_cu,
    })
}

pub fn reproduce_market_incarnation_deposit(
    seed: [u8; 32],
) -> Result<MarketIncarnationDepositReproduction, String> {
    let control = run_market_incarnation_deposit_world(seed, false)?;
    let replay = run_market_incarnation_deposit_world(seed, true)?;
    let stale_deposit = replay
        .source_loss
        .checked_sub(control.source_loss)
        .ok_or("PR 307 replay source loss below control")?;
    let beneficiary_extra_payout = replay
        .winner_payout
        .checked_sub(control.winner_payout)
        .ok_or("PR 307 replay winner payout below control")?;
    if control.old_asset_market_id != replay.old_asset_market_id
        || control.new_asset_market_id != replay.new_asset_market_id
        || replay.old_asset_market_id != replay.new_asset_market_id
        || control.source_loss != 150_000
        || replay.source_loss != 250_000
        || control.winner_payout != 100_150_000
        || replay.winner_payout != 100_250_000
        || control.victim_payout != 0
        || replay.victim_payout != 0
        || u128::from(stale_deposit) != beneficiary_extra_payout
        || replay.replay_cu >= TX_CU_LIMIT
    {
        return Err(format!(
            "PR 307 paired-world mismatch: control={control:?}, replay={replay:?}, \
             stale={stale_deposit}, beneficiary={beneficiary_extra_payout}"
        ));
    }
    Ok(MarketIncarnationDepositReproduction {
        blocker: KnownBlocker::MarketIncarnationDeposit,
        old_asset_market_id: replay.old_asset_market_id,
        new_asset_market_id: replay.new_asset_market_id,
        stale_deposit,
        beneficiary_extra_payout,
        control_winner_payout: control.winner_payout,
        replay_winner_payout: replay.winner_payout,
        replay_cu: replay.replay_cu,
    })
}

#[derive(Clone, Copy, Debug)]
struct ResolveGenerationReplayWorld {
    old_market_id: u64,
    new_market_id: u64,
    victim_payout: u128,
    winner_payout: u128,
    replay_cu: u64,
}

fn run_resolve_generation_replay_world(
    seed: [u8; 32],
    land_replay: bool,
) -> Result<ResolveGenerationReplayWorld, String> {
    const WINNER: usize = 0;
    const VICTIM: usize = 1;
    const PRICE: u64 = 100;
    const ADVERSE_PRICE: u64 = 110;
    const DEPOSIT: u128 = 1_000_000;
    const SIZE_Q: i128 = 10_000 * POS_SCALE as i128;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            max_price_move_bps_per_slot: 1_000,
            max_accrual_dt_slots: 4,
            min_funding_lifetime_slots: 4,
            actor_deposits: [1, 1, 1, 1, 1],
            actor_token_balances: [DEPOSIT as u64, DEPOSIT as u64, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.configure_permissionless_resolve(1_000_000, 1)
        .map_err(|error| format!("PR 311 configure shutdown delay: {error}"))?;
    let old_market_id = env.primary_market_state().1.assets[0].market_id;
    let retained = env.build_retained_resolve_market();

    env.warp_to_slot(2);
    env.shutdown_asset(0, 2)
        .map_err(|error| format!("PR 311 shut down generation A: {error}"))?;
    env.warp_to_slot(3);
    env.restart_asset_oracle(0, 3, PRICE)
        .map_err(|error| format!("PR 311 restart generation B: {error}"))?;
    env.configure_auth_mark(false, 0, 3, PRICE)
        .map_err(|error| format!("PR 311 configure generation-B AuthMark: {error}"))?;
    let new_market_id = env.primary_market_state().1.assets[0].market_id;
    if new_market_id <= old_market_id {
        return Err(format!(
            "PR 311 asset generation did not advance: {old_market_id} -> {new_market_id}"
        ));
    }

    for actor in [WINNER, VICTIM] {
        env.deposit_primary(actor, DEPOSIT - 1)
            .map_err(|error| format!("PR 311 fund generation-B actor {actor}: {error}"))?;
    }
    env.trade_no_cpi(WINNER, VICTIM, 0, SIZE_Q, PRICE, 0)
        .map_err(|error| format!("PR 311 open generation-B positions: {error}"))?;

    env.warp_to_slot(10);
    env.push_auth_mark(0, 10, ADVERSE_PRICE)
        .map_err(|error| format!("PR 311 publish temporary adverse mark: {error}"))?;
    crank_market_then_accounts_once(&mut env, 2, &[VICTIM, WINNER], 10, 0, 16)
        .map_err(|error| format!("PR 311 settle adverse mark: {error}"))?;
    if env.primary_market_state().1.assets[0].effective_price != ADVERSE_PRICE {
        return Err("PR 311 temporary adverse mark did not commit".into());
    }

    let replay_cu = if land_replay {
        env.land_retained(retained)
            .map_err(|error| format!("PR 311 generation-A resolve no longer lands: {error}"))?
            .compute_units
    } else {
        env.warp_to_slot(11);
        env.push_auth_mark(0, 11, PRICE)
            .map_err(|error| format!("PR 311 restore authenticated mark: {error}"))?;
        crank_market_then_accounts_once(&mut env, 2, &[VICTIM, WINNER], 11, 0, 16)
            .map_err(|error| format!("PR 311 settle restored mark: {error}"))?;
        if env.primary_market_state().1.assets[0].effective_price != PRICE {
            return Err("PR 311 control mark did not return to entry".into());
        }
        env.trade_no_cpi(WINNER, VICTIM, 0, -SIZE_Q, PRICE, 0)
            .map_err(|error| format!("PR 311 close control positions: {error}"))?;
        0
    };

    if !land_replay {
        env.resolve_market()
            .map_err(|error| format!("PR 311 resolve control world: {error}"))?;
    }
    env.warp_to_slot(12);
    let (victim_payout, _) = drain_resolved_actor(&mut env, VICTIM)?;
    let (winner_payout, _) = drain_resolved_actor(&mut env, WINNER)?;
    if env.token_supply_observed() != supply_before {
        return Err(format!(
            "PR 311 terminal supply mismatch: replay={land_replay}, supply={}/{supply_before}",
            env.token_supply_observed()
        ));
    }
    Ok(ResolveGenerationReplayWorld {
        old_market_id,
        new_market_id,
        victim_payout,
        winner_payout,
        replay_cu,
    })
}

pub fn reproduce_resolve_generation_replay(
    seed: [u8; 32],
) -> Result<ResolveGenerationReplayReproduction, String> {
    let control = run_resolve_generation_replay_world(seed, false)?;
    let replay = run_resolve_generation_replay_world(seed, true)?;
    let victim_loss = control
        .victim_payout
        .checked_sub(replay.victim_payout)
        .ok_or("PR 311 replay increased victim payout")?;
    let beneficiary_gain = replay
        .winner_payout
        .checked_sub(control.winner_payout)
        .ok_or("PR 311 replay reduced winner payout")?;
    if control.old_market_id != replay.old_market_id
        || control.new_market_id != replay.new_market_id
        || control.victim_payout != 1_000_000
        || replay.victim_payout != 900_000
        || control.winner_payout != 1_000_000
        || replay.winner_payout != 1_100_000
        || victim_loss != beneficiary_gain
        || victim_loss != 100_000
        || replay.replay_cu >= TX_CU_LIMIT
    {
        return Err(format!(
            "PR 311 paired-world mismatch: control={control:?}, replay={replay:?}, \
             victim_loss={victim_loss}, beneficiary_gain={beneficiary_gain}"
        ));
    }
    Ok(ResolveGenerationReplayReproduction {
        blocker: KnownBlocker::ResolveGenerationReplay,
        old_market_id: replay.old_market_id,
        new_market_id: replay.new_market_id,
        victim_loss: victim_loss as u64,
        beneficiary_gain: beneficiary_gain as u64,
        control_victim_payout: control.victim_payout,
        replay_victim_payout: replay.victim_payout,
        control_winner_payout: control.winner_payout,
        replay_winner_payout: replay.winner_payout,
        replay_cu: replay.replay_cu,
    })
}

#[derive(Clone, Copy, Debug)]
struct ShutdownGenerationReplayWorld {
    old_market_id: u64,
    new_market_id: u64,
    victim_payout: u128,
    winner_payout: u128,
    replay_cu: u64,
    force_close_cu: u64,
}

fn run_shutdown_generation_replay_world(
    seed: [u8; 32],
    land_replay: bool,
) -> Result<ShutdownGenerationReplayWorld, String> {
    const WINNER: usize = 0;
    const VICTIM: usize = 1;
    const CRANKER: usize = 2;
    const PRICE: u64 = 100;
    const ADVERSE_PRICE: u64 = 110;
    const DEPOSIT: u128 = 1_000_000;
    const SIZE_Q: i128 = 10_000 * POS_SCALE as i128;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
            max_price_move_bps_per_slot: 1_000,
            max_accrual_dt_slots: 4,
            min_funding_lifetime_slots: 4,
            actor_deposits: [1, 1, 1, 1, 1],
            actor_token_balances: [DEPOSIT as u64, DEPOSIT as u64, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.configure_permissionless_resolve(1_000_000, 1)
        .map_err(|error| format!("PR 315 configure shutdown delay: {error}"))?;
    let old_market_id = env.primary_market_state().1.assets[0].market_id;
    let retained = env.build_retained_shutdown_asset(0, 12);

    env.warp_to_slot(2);
    env.shutdown_asset(0, 2)
        .map_err(|error| format!("PR 315 shut down generation A: {error}"))?;
    env.warp_to_slot(3);
    env.restart_asset_oracle(0, 3, PRICE)
        .map_err(|error| format!("PR 315 restart generation B: {error}"))?;
    env.configure_auth_mark(false, 0, 3, PRICE)
        .map_err(|error| format!("PR 315 configure generation-B AuthMark: {error}"))?;
    let new_market_id = env.primary_market_state().1.assets[0].market_id;
    if new_market_id <= old_market_id {
        return Err(format!(
            "PR 315 asset generation did not advance: {old_market_id} -> {new_market_id}"
        ));
    }

    for actor in [WINNER, VICTIM] {
        env.deposit_primary(actor, DEPOSIT - 1)
            .map_err(|error| format!("PR 315 fund generation-B actor {actor}: {error}"))?;
    }
    env.trade_no_cpi(WINNER, VICTIM, 0, SIZE_Q, PRICE, 0)
        .map_err(|error| format!("PR 315 open generation-B positions: {error}"))?;
    env.warp_to_slot(10);
    env.push_auth_mark(0, 10, ADVERSE_PRICE)
        .map_err(|error| format!("PR 315 publish temporary adverse mark: {error}"))?;
    crank_market_then_accounts_once(&mut env, CRANKER, &[VICTIM, WINNER], 10, 0, 16)
        .map_err(|error| format!("PR 315 settle adverse mark: {error}"))?;
    if env.primary_market_state().1.assets[0].effective_price != ADVERSE_PRICE {
        return Err("PR 315 temporary adverse mark did not commit".into());
    }

    let (replay_cu, force_close_cu) = if land_replay {
        env.warp_to_slot(12);
        let replay = env
            .land_retained(retained)
            .map_err(|error| format!("PR 315 generation-A shutdown no longer lands: {error}"))?;
        if env.primary_market_state().1.assets[0].lifecycle != AssetLifecycleV16::Recovery {
            return Err("PR 315 stale shutdown did not freeze generation B".into());
        }
        env.warp_to_slot(13);
        let force_close = env
            .force_close_abandoned_asset(CRANKER, WINNER, VICTIM, 0, 13, SIZE_Q.unsigned_abs())
            .map_err(|error| format!("PR 315 permissionless force close failed: {error}"))?;
        (replay.compute_units, force_close.compute_units)
    } else {
        env.warp_to_slot(12);
        env.push_auth_mark(0, 12, PRICE)
            .map_err(|error| format!("PR 315 restore authenticated mark: {error}"))?;
        crank_market_then_accounts_once(&mut env, CRANKER, &[VICTIM, WINNER], 12, 0, 16)
            .map_err(|error| format!("PR 315 settle restored mark: {error}"))?;
        if env.primary_market_state().1.assets[0].effective_price != PRICE {
            return Err("PR 315 control mark did not return to entry".into());
        }
        env.trade_no_cpi(WINNER, VICTIM, 0, -SIZE_Q, PRICE, 0)
            .map_err(|error| format!("PR 315 close control positions: {error}"))?;
        (0, 0)
    };

    env.resolve_market()
        .map_err(|error| format!("PR 315 resolve terminal world: {error}"))?;
    env.warp_to_slot(14);
    let (victim_payout, _) = drain_resolved_actor(&mut env, VICTIM)?;
    let (winner_payout, _) = drain_resolved_actor(&mut env, WINNER)?;
    if env.token_supply_observed() != supply_before {
        return Err(format!(
            "PR 315 terminal supply mismatch: replay={land_replay}, supply={}/{supply_before}",
            env.token_supply_observed()
        ));
    }
    Ok(ShutdownGenerationReplayWorld {
        old_market_id,
        new_market_id,
        victim_payout,
        winner_payout,
        replay_cu,
        force_close_cu,
    })
}

pub fn reproduce_shutdown_generation_replay(
    seed: [u8; 32],
) -> Result<ShutdownGenerationReplayReproduction, String> {
    let control = run_shutdown_generation_replay_world(seed, false)?;
    let replay = run_shutdown_generation_replay_world(seed, true)?;
    let victim_loss = control
        .victim_payout
        .checked_sub(replay.victim_payout)
        .ok_or("PR 315 replay increased victim payout")?;
    let beneficiary_gain = replay
        .winner_payout
        .checked_sub(control.winner_payout)
        .ok_or("PR 315 replay reduced winner payout")?;
    if control.old_market_id != replay.old_market_id
        || control.new_market_id != replay.new_market_id
        || control.victim_payout != 1_000_000
        || replay.victim_payout != 900_000
        || control.winner_payout != 1_000_000
        || replay.winner_payout != 1_100_000
        || victim_loss != beneficiary_gain
        || victim_loss != 100_000
        || replay.replay_cu >= TX_CU_LIMIT
        || replay.force_close_cu >= TX_CU_LIMIT
    {
        return Err(format!(
            "PR 315 paired-world mismatch: control={control:?}, replay={replay:?}, \
             victim_loss={victim_loss}, beneficiary_gain={beneficiary_gain}"
        ));
    }
    Ok(ShutdownGenerationReplayReproduction {
        blocker: KnownBlocker::ShutdownGenerationReplay,
        old_market_id: replay.old_market_id,
        new_market_id: replay.new_market_id,
        victim_loss: victim_loss as u64,
        beneficiary_gain: beneficiary_gain as u64,
        control_victim_payout: control.victim_payout,
        replay_victim_payout: replay.victim_payout,
        control_winner_payout: control.winner_payout,
        replay_winner_payout: replay.winner_payout,
        replay_cu: replay.replay_cu,
        force_close_cu: replay.force_close_cu,
    })
}

pub fn reproduce_withdrawal_retry_liquidation(
    seed: [u8; 32],
) -> Result<WithdrawalRetryLiquidationReproduction, String> {
    const LONG: usize = 0;
    const VICTIM: usize = 1;
    const CRANKER: usize = 2;
    const STARTING_CAPITAL: u128 = 200_000_000;
    const WITHDRAWAL: u128 = 50_000_000;
    const POSITION_Q: i128 = 1_000 * POS_SCALE as i128;
    const PRICE: u64 = 1_000_000;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
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
            actor_deposits: [2_000_000_000, STARTING_CAPITAL, 1_000, 1, 1],
            actor_token_balances: [2_100_000_000, 300_000_000, 10_000, 10, 10],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.update_liquidation_fee_policy(5_000)
        .map_err(|error| format!("PR 355 configure cranker reward share: {error}"))?;

    let victim_destination = env.actors[VICTIM].destination_token;
    let destination_before = env.token_amount(victim_destination);
    let intended = env.build_retained_withdrawal(VICTIM, WITHDRAWAL);
    let retry_variant = env.build_retained_withdrawal(VICTIM, WITHDRAWAL);
    env.land_retained(intended)
        .map_err(|error| format!("PR 355 intended withdrawal rejected: {error}"))?;
    if env.primary_portfolio(VICTIM).capital.get() != STARTING_CAPITAL - WITHDRAWAL
        || env.token_amount(victim_destination) - destination_before != WITHDRAWAL as u64
    {
        return Err("PR 355 intended withdrawal accounting mismatch".into());
    }

    let fresh_trade = env.build_retained_no_cpi_trade(LONG, VICTIM, 0, POSITION_Q, PRICE);
    let replay = env
        .land_retained(retry_variant)
        .map_err(|error| format!("PR 355 retained withdrawal no longer lands: {error}"))?;
    let total_withdrawn = env
        .token_amount(victim_destination)
        .checked_sub(destination_before)
        .ok_or("PR 355 victim destination decreased")?;
    let duplicate_withdrawal = total_withdrawn
        .checked_sub(WITHDRAWAL as u64)
        .ok_or("PR 355 total withdrawal below intended amount")?;
    if duplicate_withdrawal != WITHDRAWAL as u64
        || env.primary_portfolio(VICTIM).capital.get() != STARTING_CAPITAL - 2 * WITHDRAWAL
    {
        return Err(format!(
            "PR 355 retained withdrawal mismatch: duplicate={duplicate_withdrawal}, capital={}",
            env.primary_portfolio(VICTIM).capital.get()
        ));
    }
    env.land_retained(fresh_trade)
        .map_err(|error| format!("PR 355 fresh signed trade did not land: {error}"))?;
    if position_for_asset(&env.primary_portfolio(VICTIM), 0)? != -POSITION_Q {
        return Err("PR 355 fresh trade did not install the victim short".into());
    }

    let mut liquidation = None;
    for slot in 2..=31u64 {
        env.warp_to_slot(slot);
        let current_mark = env.primary_market_state().1.assets[0].effective_price;
        let next_mark = current_mark
            .checked_add((current_mark / 500).max(1))
            .ok_or("PR 355 mark overflow")?;
        env.push_auth_mark(0, slot, next_mark)
            .map_err(|error| format!("PR 355 publish mark at slot {slot}: {error}"))?;
        env.crank(
            VICTIM,
            slot,
            vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: 0,
            }],
        )
        .map_err(|error| format!("PR 355 refresh victim at slot {slot}: {error}"))?;
        let cert = env
            .primary_portfolio(VICTIM)
            .health_cert
            .try_to_runtime()
            .map_err(|error| format!("PR 355 decode health certificate: {error:?}"))?;
        let maintenance = i128::try_from(cert.certified_maintenance_req)
            .map_err(|_| "PR 355 maintenance requirement exceeds signed range")?;
        if cert.certified_equity < maintenance {
            let restored_equity = cert
                .certified_equity
                .checked_add(WITHDRAWAL as i128)
                .ok_or("PR 355 restored equity overflow")?;
            if restored_equity <= maintenance {
                return Err(format!(
                    "PR 355 duplicate is not causal: equity={}, restored={restored_equity}, \
                     maintenance={maintenance}",
                    cert.certified_equity
                ));
            }
            liquidation = Some((slot, restored_equity - maintenance));
            break;
        }
    }
    let (liquidation_slot, restored_equity_surplus) =
        liquidation.ok_or("PR 355 duplicate withdrawal never caused liquidation")?;

    let cranker_before = env.primary_portfolio(CRANKER).capital.get();
    let liquidation_tx = env
        .crank_with_reward(CRANKER, VICTIM, liquidation_slot, Vec::new(), &[])
        .map_err(|error| format!("PR 355 independent liquidation failed: {error}"))?;
    let cranker_reward = env
        .primary_portfolio(CRANKER)
        .capital
        .get()
        .checked_sub(cranker_before)
        .ok_or("PR 355 cranker capital decreased")?;
    if cranker_reward == 0 {
        return Err("PR 355 replay-induced liquidation paid no reward".into());
    }
    let reward_destination = env.actors[CRANKER].destination_token;
    let reward_destination_before = env.token_amount(reward_destination);
    let withdrawal = env
        .withdraw_primary(CRANKER, cranker_reward)
        .map_err(|error| format!("PR 355 cranker reward withdrawal failed: {error}"))?;
    let extracted_reward = env
        .token_amount(reward_destination)
        .checked_sub(reward_destination_before)
        .ok_or("PR 355 cranker destination decreased")?;
    let max_cu = replay
        .compute_units
        .max(liquidation_tx.compute_units)
        .max(withdrawal.compute_units);
    if u128::from(extracted_reward) != cranker_reward
        || max_cu >= TX_CU_LIMIT
        || env.token_supply_observed() != supply_before
    {
        return Err(format!(
            "PR 355 extraction mismatch: reward={cranker_reward}, extracted={extracted_reward}, \
             max_cu={max_cu}, supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }

    Ok(WithdrawalRetryLiquidationReproduction {
        blocker: KnownBlocker::WithdrawalRetryLiquidation,
        intended_withdrawal: WITHDRAWAL as u64,
        duplicate_withdrawal,
        liquidation_slot,
        restored_equity_surplus,
        cranker_reward,
        extracted_reward,
        replay_cu: replay.compute_units,
    })
}

pub fn reproduce_portfolio_incarnation_withdrawal(
    seed: [u8; 32],
) -> Result<PortfolioIncarnationWithdrawalReproduction, String> {
    const LONG: usize = 0;
    const VICTIM: usize = 1;
    const CRANKER: usize = 2;
    const ORIGINAL_CAPITAL: u128 = 100_000_000;
    const REPLACEMENT_CAPITAL: u128 = 200_000_000;
    const STALE_WITHDRAWAL: u128 = 100_000_000;
    const POSITION_Q: i128 = 1_000 * POS_SCALE as i128;
    const PRICE: u64 = 1_000_000;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: PRICE,
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
            actor_deposits: [2_000_000_000, ORIGINAL_CAPITAL, 1_000, 1, 1],
            actor_token_balances: [2_100_000_000, 350_000_000, 10_000, 10, 10],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.update_liquidation_fee_policy(5_000)
        .map_err(|error| format!("PR 299 configure cranker reward share: {error}"))?;

    let old_portfolio_id = env.primary_portfolio_id(VICTIM);
    let retained = env.build_retained_withdrawal(VICTIM, STALE_WITHDRAWAL);
    env.withdraw_primary(VICTIM, ORIGINAL_CAPITAL)
        .map_err(|error| format!("PR 299 empty incarnation A: {error}"))?;
    env.close_primary_portfolio(VICTIM)
        .map_err(|error| format!("PR 299 close incarnation A: {error}"))?;
    env.fund_closed_primary_portfolio(VICTIM, 1_000_000_000)
        .map_err(|error| format!("PR 299 re-fund closed portfolio: {error}"))?;
    env.reinitialize_primary_portfolio(VICTIM)
        .map_err(|error| format!("PR 299 initialize incarnation B: {error}"))?;
    let new_portfolio_id = env.primary_portfolio_id(VICTIM);
    if new_portfolio_id <= old_portfolio_id {
        return Err(format!(
            "PR 299 portfolio incarnation did not advance: {old_portfolio_id} -> \
             {new_portfolio_id}"
        ));
    }
    env.deposit_primary(VICTIM, REPLACEMENT_CAPITAL)
        .map_err(|error| format!("PR 299 fund incarnation B: {error}"))?;

    let fresh_trade = env.build_retained_no_cpi_trade(LONG, VICTIM, 0, POSITION_Q, PRICE);
    let destination = env.actors[VICTIM].destination_token;
    let destination_before = env.token_amount(destination);
    let replay = env
        .land_retained(retained)
        .map_err(|error| format!("PR 299 incarnation-A withdrawal no longer lands: {error}"))?;
    let stale_withdrawal = env
        .token_amount(destination)
        .checked_sub(destination_before)
        .ok_or("PR 299 victim destination decreased")?;
    if u128::from(stale_withdrawal) != STALE_WITHDRAWAL
        || env.primary_portfolio(VICTIM).capital.get() != REPLACEMENT_CAPITAL - STALE_WITHDRAWAL
    {
        return Err(format!(
            "PR 299 stale withdrawal mismatch: withdrew={stale_withdrawal}, capital={}",
            env.primary_portfolio(VICTIM).capital.get()
        ));
    }
    env.land_retained(fresh_trade)
        .map_err(|error| format!("PR 299 incarnation-B trade did not land: {error}"))?;
    if position_for_asset(&env.primary_portfolio(VICTIM), 0)? != -POSITION_Q {
        return Err("PR 299 fresh trade did not install the replacement short".into());
    }

    let mut liquidation = None;
    for slot in 2..=31u64 {
        env.warp_to_slot(slot);
        let current_mark = env.primary_market_state().1.assets[0].effective_price;
        let next_mark = current_mark
            .checked_add((current_mark / 500).max(1))
            .ok_or("PR 299 mark overflow")?;
        env.push_auth_mark(0, slot, next_mark)
            .map_err(|error| format!("PR 299 publish mark at slot {slot}: {error}"))?;
        env.crank(
            VICTIM,
            slot,
            vec![CrankObservationHint {
                asset_index: 0,
                oracle_accounts: 0,
            }],
        )
        .map_err(|error| format!("PR 299 refresh victim at slot {slot}: {error}"))?;
        let cert = env
            .primary_portfolio(VICTIM)
            .health_cert
            .try_to_runtime()
            .map_err(|error| format!("PR 299 decode health certificate: {error:?}"))?;
        let maintenance = i128::try_from(cert.certified_maintenance_req)
            .map_err(|_| "PR 299 maintenance requirement exceeds signed range")?;
        if cert.certified_equity < maintenance {
            let restored_equity = cert
                .certified_equity
                .checked_add(STALE_WITHDRAWAL as i128)
                .ok_or("PR 299 restored equity overflow")?;
            if restored_equity <= maintenance {
                return Err(format!(
                    "PR 299 stale withdrawal is not causal: equity={}, restored={}, \
                     maintenance={maintenance}",
                    cert.certified_equity, restored_equity
                ));
            }
            liquidation = Some((slot, restored_equity - maintenance));
            break;
        }
    }
    let (liquidation_slot, restored_equity_surplus) =
        liquidation.ok_or("PR 299 stale withdrawal never caused liquidation")?;

    let cranker_before = env.primary_portfolio(CRANKER).capital.get();
    let liquidation_tx = env
        .crank_with_reward(CRANKER, VICTIM, liquidation_slot, Vec::new(), &[])
        .map_err(|error| format!("PR 299 independent liquidation failed: {error}"))?;
    let cranker_reward = env
        .primary_portfolio(CRANKER)
        .capital
        .get()
        .checked_sub(cranker_before)
        .ok_or("PR 299 cranker capital decreased")?;
    if cranker_reward == 0 {
        return Err("PR 299 stale-withdrawal liquidation paid no reward".into());
    }
    let reward_destination = env.actors[CRANKER].destination_token;
    let reward_destination_before = env.token_amount(reward_destination);
    let withdrawal = env
        .withdraw_primary(CRANKER, cranker_reward)
        .map_err(|error| format!("PR 299 cranker reward withdrawal failed: {error}"))?;
    let extracted_reward = env
        .token_amount(reward_destination)
        .checked_sub(reward_destination_before)
        .ok_or("PR 299 cranker destination decreased")?;
    let max_cu = replay
        .compute_units
        .max(liquidation_tx.compute_units)
        .max(withdrawal.compute_units);
    if u128::from(extracted_reward) != cranker_reward
        || max_cu >= TX_CU_LIMIT
        || env.token_supply_observed() != supply_before
    {
        return Err(format!(
            "PR 299 extraction mismatch: reward={cranker_reward}, extracted={extracted_reward}, \
             max_cu={max_cu}, supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }

    Ok(PortfolioIncarnationWithdrawalReproduction {
        blocker: KnownBlocker::PortfolioIncarnationWithdrawal,
        old_portfolio_id,
        new_portfolio_id,
        stale_withdrawal,
        liquidation_slot,
        restored_equity_surplus,
        cranker_reward,
        extracted_reward,
        replay_cu: replay.compute_units,
    })
}

fn portfolio_equity(env: &V16Svm, actor: usize) -> Result<i128, String> {
    let account = env.primary_portfolio(actor);
    i128::try_from(account.capital.get())
        .map_err(|_| "portfolio capital exceeds signed range")?
        .checked_add(account.pnl.get())
        .ok_or_else(|| "portfolio equity overflow".into())
}

#[derive(Clone, Copy, Debug)]
struct ProspectiveFundingWorld {
    coalition_payout: u128,
    victim_payout: u128,
    stamp_fee: u128,
    final_mark: u64,
    final_effective_price: u64,
    f_short_num: i128,
    total_payout: u128,
}

fn run_prospective_funding_world(
    seed: [u8; 32],
    route: TradeRoute,
    stamp_before_catchup: bool,
) -> Result<ProspectiveFundingWorld, String> {
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
        .map_err(|error| format!("PR 380 configure EWMA mark: {error}"))?;
    execute_trade_route(&mut env, route, 0, 1, 0, POS_SCALE as i128, PRICE, 0)
        .map_err(|error| format!("PR 380 open independent funding pair: {error}"))?;
    execute_trade_route(&mut env, route, 2, 3, 0, POS_SCALE as i128, PRICE, 0)
        .map_err(|error| format!("PR 380 open stamper pair: {error}"))?;

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
        .map_err(|error| format!("PR 380 prime funding clock: {error}"))?;
        if env.primary_market_state().1.assets[0].slot_last == PREP_SLOT {
            break;
        }
    }
    if env.primary_market_state().1.assets[0].slot_last != PREP_SLOT {
        return Err(format!(
            "PR 380 engine clock did not reach prep slot: {}",
            env.primary_market_state().1.assets[0].slot_last
        ));
    }
    env.push_ewma_mark(0, PREP_SLOT, PUSH_TARGET)
        .map_err(|error| format!("PR 380 publish honest premium: {error}"))?;
    let after_push = env.primary_profile(0);
    if after_push.mark_ewma_e6 != 1_500_000
        || after_push.funding_mark_e6 != after_push.mark_ewma_e6
        || after_push.funding_mark_pending_e6 != 0
        || after_push.funding_mark_pending_slot != 0
        || env.primary_market_state().1.assets[0].effective_price != PRICE
    {
        return Err(format!(
            "PR 380 premium setup drifted: mark={}, effective={}",
            after_push.mark_ewma_e6,
            env.primary_market_state().1.assets[0].effective_price
        ));
    }

    env.warp_to_slot(CATCHUP_SLOT);
    let insurance_before_stamp = env.primary_market_state().1.insurance;
    let stamp = |env: &mut V16Svm| {
        execute_trade_route(
            env,
            route,
            2,
            3,
            0,
            -(POS_SCALE as i128),
            STAMP_EXEC_PRICE,
            0,
        )
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
    };
    if stamp_before_catchup {
        stamp(&mut env).map_err(|error| format!("PR 380 trade-first stamp: {error}"))?;
        let staged = env.primary_profile(0);
        if staged.funding_mark_e6 != after_push.funding_mark_e6
            || staged.funding_mark_pending_e6 == 0
            || staged.funding_mark_pending_slot != CATCHUP_SLOT
        {
            return Err(format!(
                "PR 380 trade-first mark was not staged prospectively: {staged:?}"
            ));
        }
        catchup(&mut env).map_err(|error| format!("PR 380 trade-first catch-up: {error}"))?;
    } else {
        catchup(&mut env).map_err(|error| format!("PR 380 control catch-up: {error}"))?;
        stamp(&mut env).map_err(|error| format!("PR 380 control stamp: {error}"))?;
    }

    let (profile_after, group_after) = env.primary_market_state();
    let checkpoint_after = env.primary_profile(0);
    if checkpoint_after.funding_mark_e6 != checkpoint_after.mark_ewma_e6
        || checkpoint_after.funding_mark_pending_e6 != 0
        || checkpoint_after.funding_mark_pending_slot != 0
    {
        return Err(format!(
            "PR 380 funding checkpoint did not commit after catch-up: {checkpoint_after:?}"
        ));
    }
    let stamp_fee = group_after
        .insurance
        .checked_sub(insurance_before_stamp)
        .ok_or("PR 380 stamp decreased insurance")?;
    if group_after.assets[0].slot_last != CATCHUP_SLOT {
        return Err(format!(
            "PR 380 catch-up stopped at slot {}",
            group_after.assets[0].slot_last
        ));
    }
    env.resolve_market()
        .map_err(|error| format!("PR 380 resolve payout world: {error}"))?;
    let (stamper_short_payout, _) = drain_resolved_actor(&mut env, 3)?;
    let (victim_payout, _) = drain_resolved_actor(&mut env, 1)?;
    let (attacker_payout, _) = drain_resolved_actor(&mut env, 0)?;
    let (stamper_long_payout, _) = drain_resolved_actor(&mut env, 2)?;
    let coalition_payout = attacker_payout
        .checked_add(stamper_long_payout)
        .and_then(|value| value.checked_add(stamper_short_payout))
        .ok_or("PR 380 coalition payout overflow")?;
    let total_payout = coalition_payout
        .checked_add(victim_payout)
        .ok_or("PR 380 total payout overflow")?;
    if env.token_supply_observed() != supply_before {
        return Err("PR 380 terminal world changed SPL supply".into());
    }
    Ok(ProspectiveFundingWorld {
        coalition_payout,
        victim_payout,
        stamp_fee,
        final_mark: profile_after.mark_ewma_e6,
        final_effective_price: group_after.assets[0].effective_price,
        f_short_num: group_after.assets[0].f_short_num,
        total_payout,
    })
}

pub fn reproduce_rounded_funding_omission(
    mut seed: [u8; 32],
) -> Result<RoundedFundingOmissionReproduction, String> {
    seed[0] ^= 0x53;
    let control = run_rounded_funding_world(seed, false)?;
    let attack = run_rounded_funding_world(seed, true)?;
    let victim_payout_loss = control
        .victim_payout
        .checked_sub(attack.victim_payout)
        .ok_or("rounded-funding omission increased victim payout")?;
    let attacker_payout_gain = attack
        .counterparty_payout
        .checked_sub(control.counterparty_payout)
        .ok_or("rounded-funding omission decreased short payout")?;
    if u128::from(control.victim_payout) + u128::from(control.counterparty_payout)
        != u128::from(attack.victim_payout) + u128::from(attack.counterparty_payout)
    {
        return Err(format!(
            "rounded-funding indexes/payouts do not match the omission class: control={control:?}, attack={attack:?}"
        ));
    }
    Ok(RoundedFundingOmissionReproduction {
        blocker: KnownBlocker::RoundedFundingOmission,
        omitted_rejected_nonprogress: attack.omitted_rejected_nonprogress,
        omitted_exact_rollback: attack.omitted_exact_rollback,
        control_f_long_num: control.f_long_num,
        control_f_short_num: control.f_short_num,
        attack_f_long_num: attack.f_long_num,
        attack_f_short_num: attack.f_short_num,
        victim_payout_loss,
        attacker_payout_gain,
    })
}

pub fn reproduce_trade_funding_erasure(
    mut seed: [u8; 32],
    route: TradeRoute,
) -> Result<TradeFundingErasureReproduction, String> {
    if !matches!(route, TradeRoute::Cpi | TradeRoute::BatchCpi) {
        return Err(format!(
            "PR 271 requires an independently authorized CPI maker, got {route:?}"
        ));
    }
    seed[0] ^= 0x71;
    let control = run_trade_funding_order_world(seed, route, false)?;
    let attack = run_trade_funding_order_world(seed, route, true)?;
    let victim_payout_loss = control
        .1
        .checked_sub(attack.1)
        .ok_or("PR 271 close-first ordering increased victim payout")?;
    let attacker_payout_gain = attack
        .0
        .checked_sub(control.0)
        .ok_or("PR 271 close-first ordering decreased attacker payout")?;
    if control.2 <= 0
        || control.3 >= 0
        || attack.2 != control.2
        || attack.3 != control.3
        || victim_payout_loss != 0
        || attacker_payout_gain != 0
        || u128::from(control.0) + u128::from(control.1)
            != u128::from(attack.0) + u128::from(attack.1)
    {
        return Err(format!(
            "PR 271 did not erase a balanced public funding transfer: control={control:?}, attack={attack:?}"
        ));
    }
    Ok(TradeFundingErasureReproduction {
        blocker: KnownBlocker::TradeFundingErasure,
        route,
        control_f_long_num: control.2,
        control_f_short_num: control.3,
        attack_f_long_num: attack.2,
        attack_f_short_num: attack.3,
        victim_payout_loss,
        attacker_payout_gain,
    })
}

pub fn reproduce_rebalance_funding_erasure(
    mut seed: [u8; 32],
) -> Result<RebalanceFundingErasureReproduction, String> {
    seed[0] ^= 0x72;
    let control = run_rebalance_funding_order_world(seed, false)?;
    let attack = run_rebalance_funding_order_world(seed, true)?;
    let victim_claim_loss = control
        .1
        .checked_sub(attack.1)
        .ok_or("PR 272 reduce-first ordering increased victim claim")?;
    let attacker_payout_gain = attack
        .0
        .checked_sub(control.0)
        .ok_or("PR 272 reduce-first ordering decreased attacker payout")?;
    if control.2 == 0
        || control.2 != control.3
        || attack.2 != control.2
        || attack.3 != control.3
        || victim_claim_loss != 0
        || attacker_payout_gain != 0
        || u128::from(control.0) + control.1 != u128::from(attack.0) + attack.1
    {
        return Err(format!(
            "PR 272 did not erase a balanced unilateral-reduction funding transfer: control={control:?}, attack={attack:?}"
        ));
    }
    Ok(RebalanceFundingErasureReproduction {
        blocker: KnownBlocker::RebalanceFundingErasure,
        control_attacker_paid: control.2,
        control_victim_received: control.3,
        attack_attacker_paid: attack.2,
        attack_victim_received: attack.3,
        victim_claim_loss,
        attacker_payout_gain,
    })
}

pub fn reproduce_forfeit_funding_erasure(
    mut seed: [u8; 32],
) -> Result<ForfeitFundingErasureReproduction, String> {
    seed[0] ^= 0x73;
    let control = run_forfeit_funding_order_world(seed, false)?;
    let attack = run_forfeit_funding_order_world(seed, true)?;
    let victim_claim_loss = control
        .1
        .checked_sub(attack.1)
        .ok_or("PR 273 forfeit-first ordering overflowed victim claim delta")?;
    let attacker_payout_gain = attack
        .0
        .checked_sub(control.0)
        .ok_or("PR 273 forfeit-first ordering decreased attacker payout")?;
    if control.2 == 0
        || control.2 != control.3
        || attack.2 != control.2
        || attack.3 != control.3
        || victim_claim_loss != 0
        || attacker_payout_gain != 0
    {
        return Err(format!(
            "PR 273 did not erase a balanced recovery-forfeit funding transfer: control={control:?}, attack={attack:?}"
        ));
    }
    Ok(ForfeitFundingErasureReproduction {
        blocker: KnownBlocker::ForfeitFundingErasure,
        control_attacker_paid: control.2,
        control_victim_received: control.3,
        attack_attacker_paid: attack.2,
        attack_victim_received: attack.3,
        victim_claim_loss,
        attacker_payout_gain,
    })
}

pub fn reproduce_pending_ewma_inheritance(
    mut seed: [u8; 32],
    route: TradeRoute,
) -> Result<PendingEwmaInheritanceReproduction, String> {
    seed[0] ^= 0x60;
    const MARK: u64 = 1_000_000;
    const LARGE_Q: i128 = 50 * POS_SCALE as i128;
    const LARGE_DEPOSIT: u128 = 100_000_000;
    const SEED_DEPOSIT: u128 = 2_000_000;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: MARK,
            max_trading_fee_bps: 100,
            max_price_move_bps_per_slot: 100,
            max_accrual_dt_slots: 1,
            actor_deposits: [
                SEED_DEPOSIT,
                SEED_DEPOSIT,
                LARGE_DEPOSIT,
                LARGE_DEPOSIT,
                super::v16_svm::EXIT_MAKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    env.configure_ewma_mark(0, 1, MARK, 1, 0)
        .map_err(|error| format!("{route:?} configure EWMA mark: {error}"))?;
    env.top_up_backing_bucket(1, 10_000_000, 100)
        .map_err(|error| format!("{route:?} fund EWMA source backing: {error}"))?;
    env.warp_to_slot(2);
    let retained = build_retained_trade(&mut env, route, 2, 3, 0, LARGE_Q, MARK, 0);

    let seed_capital_before =
        env.primary_portfolio(0).capital.get() + env.primary_portfolio(1).capital.get();
    env.trade_no_cpi(0, 1, 0, POS_SCALE as i128, MARK * 2, 0)
        .map_err(|error| format!("{route:?} seed paid EWMA move: {error}"))?;
    let seed_capital_after =
        env.primary_portfolio(0).capital.get() + env.primary_portfolio(1).capital.get();
    let seed_cost = seed_capital_before
        .checked_sub(seed_capital_after)
        .ok_or("pending EWMA seed increased pair capital")?;
    let pending_mark = env.primary_profile(0).mark_ewma_e6;
    let effective_before = env.primary_market_state().1.assets[0].effective_price;
    if seed_cost == 0 || pending_mark <= MARK || effective_before != MARK {
        return Err(format!(
            "{route:?} seed did not create a paid pending mark: cost {seed_cost}, target {pending_mark}, effective {effective_before}"
        ));
    }

    env.land_retained(retained)
        .map_err(|error| format!("{route:?} pre-signed large trade no longer lands: {error}"))?;
    env.warp_to_slot(3);
    let oracle_accounts = env.primary_profile(0).oracle_leg_count;
    let observations = || {
        vec![CrankObservationHint {
            asset_index: 0,
            oracle_accounts,
        }]
    };
    env.crank(2, 3, observations())
        .map_err(|error| format!("{route:?} apply pending EWMA to attacker: {error}"))?;
    env.crank(3, 3, observations())
        .map_err(|error| format!("{route:?} apply pending EWMA to victim: {error}"))?;
    let applied_mark = env.primary_market_state().1.assets[0].effective_price;
    if applied_mark <= MARK {
        return Err(format!(
            "{route:?} queued EWMA mark did not apply: {applied_mark}"
        ));
    }

    execute_trade_route(&mut env, route, 2, 3, 0, -LARGE_Q, applied_mark, 0)
        .map_err(|error| format!("{route:?} close inherited EWMA exposure: {error}"))?;
    let victim_capital = env.primary_portfolio(3).capital.get();
    let victim_loss = LARGE_DEPOSIT
        .checked_sub(victim_capital)
        .ok_or("pending EWMA victim capital increased")?;
    let attacker_pnl = env.primary_portfolio(2).pnl.get();
    if attacker_pnl != victim_loss as i128 {
        return Err(format!(
            "{route:?} inherited EWMA PnL does not match victim loss: pnl {attacker_pnl}, loss {victim_loss}"
        ));
    }
    if victim_loss != 0 {
        env.convert_released_pnl(2, victim_loss)
            .map_err(|error| format!("{route:?} convert inherited EWMA PnL: {error}"))?;
    }
    let attacker_gain = env
        .primary_portfolio(2)
        .capital
        .get()
        .checked_sub(LARGE_DEPOSIT)
        .ok_or("pending EWMA attacker remained below principal")?;
    if attacker_gain != victim_loss || attacker_gain <= seed_cost {
        return Err(format!(
            "{route:?} pending EWMA inheritance is no longer profitable: gain {attacker_gain}, victim loss {victim_loss}, seed cost {seed_cost}"
        ));
    }
    let net_extracted = attacker_gain
        .checked_sub(seed_cost)
        .ok_or("pending EWMA net extraction underflow")?;
    env.withdraw_primary(2, net_extracted)
        .map_err(|error| format!("{route:?} withdraw inherited EWMA profit: {error}"))?;
    let net_extracted_tokens = env.token_amount(env.actors[2].destination_token);
    if u128::from(net_extracted_tokens) != net_extracted {
        return Err(format!(
            "{route:?} inherited EWMA SPL extraction mismatch: {net_extracted} vs {net_extracted_tokens}"
        ));
    }
    Ok(PendingEwmaInheritanceReproduction {
        blocker: KnownBlocker::PendingEwmaInheritance,
        route,
        seed_cost,
        victim_loss,
        attacker_gain,
        net_extracted_tokens,
        pending_mark,
        applied_mark,
    })
}

pub fn reproduce_reclaimable_ewma_fee(
    mut seed: [u8; 32],
    route: TradeRoute,
) -> Result<ReclaimableEwmaFeeReproduction, String> {
    seed[0] ^= 0x25;
    const ASSET: u16 = 1;
    const MARK: u64 = 1_000_000;
    const LOW_PRINT: u64 = 1;
    const POSITION_Q: i128 = 1_000 * POS_SCALE as i128;
    const DEPOSIT: u128 = 2_000_000_000;
    const INIT_FEE: u128 = 1;
    const TOKEN_BALANCE: u64 = 2_500_000_000;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: MARK,
            h_max: 20,
            max_trading_fee_bps: 100,
            max_price_move_bps_per_slot: 100,
            max_accrual_dt_slots: 20,
            min_funding_lifetime_slots: 20,
            actor_deposits: [
                DEPOSIT,
                DEPOSIT,
                DEPOSIT,
                DEPOSIT,
                super::v16_svm::EXIT_MAKER_DEPOSIT,
            ],
            actor_token_balances: [
                TOKEN_BALANCE,
                TOKEN_BALANCE,
                TOKEN_BALANCE,
                TOKEN_BALANCE,
                super::v16_svm::EXIT_MAKER_TOKEN_BALANCE,
            ],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    env.update_market_init_fee_policy(INIT_FEE)
        .map_err(|error| format!("{route:?} configure init fee: {error}"))?;
    env.warp_to_slot(2);
    env.retire_asset(ASSET, 2)
        .map_err(|error| format!("{route:?} retire creator asset: {error}"))?;
    env.warp_to_slot(3);
    env.activate_permissionless_asset_for_actor(0, ASSET, 3, MARK, 0, INIT_FEE)
        .map_err(|error| format!("{route:?} activate creator asset: {error}"))?;
    env.configure_ewma_mark_for_actor(0, ASSET, 3, MARK, 1, 0)
        .map_err(|error| format!("{route:?} configure creator EWMA: {error}"))?;

    execute_trade_route(&mut env, route, 0, 3, ASSET, -POSITION_Q, MARK, 0)
        .map_err(|error| format!("{route:?} open against independent LP: {error}"))?;
    env.warp_to_slot(4);
    let insurance_before = env.primary_market_state().1.insurance;
    env.trade_no_cpi(1, 2, ASSET, POSITION_Q, LOW_PRINT, 0)
        .map_err(|error| format!("{route:?} self-trade paid EWMA move: {error}"))?;
    env.trade_no_cpi(1, 2, ASSET, -POSITION_Q, LOW_PRINT, 0)
        .map_err(|error| format!("{route:?} flatten temporary EWMA pair: {error}"))?;
    let (_, after_move) = env.primary_market_state();
    let fee_paid = after_move
        .insurance
        .checked_sub(insurance_before)
        .ok_or("EWMA move decreased insurance")?;
    let queued_mark = env.primary_profile(ASSET as usize).mark_ewma_e6;
    if fee_paid == 0 || queued_mark >= MARK {
        return Err(format!(
            "{route:?} self-trade did not pay for a downward EWMA: fee {fee_paid}, target {queued_mark}"
        ));
    }

    let destination_before = env.token_amount(env.actors[0].destination_token);
    env.withdraw_insurance_asset(0, ASSET, fee_paid)
        .map_err(|error| format!("{route:?} EWMA movement fee no longer reclaimable: {error}"))?;
    let destination_after = env.token_amount(env.actors[0].destination_token);
    let fee_reclaimed = u128::from(
        destination_after
            .checked_sub(destination_before)
            .ok_or("EWMA reclaim destination decreased")?,
    );
    if fee_reclaimed != fee_paid {
        return Err(format!(
            "{route:?} EWMA fee reclaim mismatch: paid {fee_paid}, reclaimed {fee_reclaimed}"
        ));
    }

    let oracle_accounts = env.primary_profile(ASSET as usize).oracle_leg_count;
    let observations = || {
        vec![CrankObservationHint {
            asset_index: ASSET,
            oracle_accounts,
        }]
    };
    env.crank(0, 4, observations())
        .map_err(|error| format!("{route:?} apply EWMA to attacker: {error}"))?;
    env.crank(3, 4, observations())
        .map_err(|error| format!("{route:?} apply EWMA to victim LP: {error}"))?;
    let effective_mark = env.primary_market_state().1.assets[ASSET as usize].effective_price;
    if effective_mark >= MARK {
        return Err(format!(
            "{route:?} paid downward EWMA did not apply: {effective_mark}"
        ));
    }
    execute_trade_route(&mut env, route, 0, 3, ASSET, POSITION_Q, effective_mark, 0)
        .map_err(|error| format!("{route:?} close against independent LP: {error}"))?;
    env.crank(0, 4, Vec::new())
        .map_err(|error| format!("{route:?} settle attacker close: {error}"))?;
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
    let attacker_payout = (0..3).try_fold(0u128, |sum, actor| {
        sum.checked_add(u128::from(
            env.token_amount(env.actors[actor].destination_token),
        ))
        .ok_or("EWMA attacker payout overflow")
    })?;
    let victim_payout = u128::from(env.token_amount(env.actors[3].destination_token));
    let attacker_committed = DEPOSIT
        .checked_mul(3)
        .and_then(|value| value.checked_add(INIT_FEE))
        .ok_or("EWMA attacker committed-value overflow")?;
    let victim_loss = DEPOSIT.saturating_sub(victim_payout);
    let attacker_gain = attacker_payout.saturating_sub(attacker_committed);
    if victim_loss == 0
        || attacker_gain == 0
        || attacker_gain
            .checked_add(INIT_FEE)
            .ok_or("EWMA attacker gain overflow")?
            != victim_loss
    {
        return Err(format!(
            "{route:?} reclaimed EWMA fee did not extract independent LP value: gain {attacker_gain}, victim loss {victim_loss}, paid {fee_paid}, reclaimed {fee_reclaimed}"
        ));
    }
    if env.token_supply_observed() != supply_before {
        return Err(format!("{route:?} EWMA reclaim changed total SPL supply"));
    }
    Ok(ReclaimableEwmaFeeReproduction {
        blocker: KnownBlocker::ReclaimableEwmaFee,
        route,
        fee_paid,
        fee_reclaimed,
        victim_loss,
        attacker_gain,
        effective_mark,
    })
}

pub fn reproduce_trade_driven_liquidation_reward(
    mut seed: [u8; 32],
    mode: TradeDrivenLiquidationMode,
    route: TradeRoute,
) -> Result<TradeDrivenLiquidationRewardReproduction, String> {
    if !matches!(route, TradeRoute::NoCpi | TradeRoute::BatchNoCpi) {
        return Err(format!(
            "PR 280 needs a caller-reported no-CPI price, got {route:?}"
        ));
    }
    seed[0] ^= match mode {
        TradeDrivenLiquidationMode::Ewma => 0x80,
        TradeDrivenLiquidationMode::HybridAfterHours => 0x81,
    };
    const MARK: u64 = 1_000_000;
    const VICTIM_DEPOSIT: u128 = 50_000;
    const HONEST_DEPOSIT: u128 = 2_000_000;
    const ATTACK_DEPOSIT: u128 = 1_000;
    const CRANKER_DEPOSIT: u128 = 1;
    const TINY_Q: i128 = (POS_SCALE / 10_000) as i128;

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
        .map_err(|error| format!("{mode:?} {route:?} set full cranker share: {error}"))?;

    let mut oracle_accounts = Vec::new();
    let (trade_slot, reported_price) = match mode {
        TradeDrivenLiquidationMode::Ewma => {
            env.configure_ewma_mark(0, 1, MARK, 1, 0)
                .map_err(|error| format!("{route:?} configure EWMA: {error}"))?;
            env.warp_to_slot(2);
            (2, 999_800)
        }
        TradeDrivenLiquidationMode::HybridAfterHours => {
            env.set_clock(1, 100);
            let feed = [0xedu8; 32];
            let pyth = env.set_pyth_price(&feed, MARK as i64, -6, 0, 100);
            env.configure_hybrid_oracle(0, 1, 100, 0, [feed, [0; 32], [0; 32]], &[pyth], 1, 0)
                .map_err(|error| format!("{route:?} configure hybrid fallback: {error}"))?;
            oracle_accounts.push(pyth);
            env.set_clock(3, 1_000);
            (3, 999_850)
        }
    };

    env.trade_no_cpi(0, 1, 0, POS_SCALE as i128, MARK, 0)
        .map_err(|error| format!("{mode:?} {route:?} open independent victim: {error}"))?;
    let supply_before = env.token_supply_observed();
    let insurance_before_move = env.primary_market_state().1.insurance;
    execute_trade_route(&mut env, route, 2, 3, 0, TINY_Q, reported_price, 0)
        .map_err(|error| format!("{mode:?} {route:?} paid mark-moving wash trade: {error}"))?;
    let (profile_after_move, group_after_move) = env.primary_market_state();
    let movement_fee = group_after_move
        .insurance
        .checked_sub(insurance_before_move)
        .ok_or("PR 280 wash trade decreased insurance")?;
    let queued_mark = profile_after_move.mark_ewma_e6;
    if movement_fee == 0 || queued_mark >= MARK {
        return Err(format!(
            "{mode:?} {route:?} did not create a paid downward mark move: fee {movement_fee}, mark {queued_mark}"
        ));
    }

    let victim_capital_before = env.primary_portfolio(0).capital.get();
    let cranker_capital_before = env.primary_portfolio(4).capital.get();
    let observations = vec![CrankObservationHint {
        asset_index: 0,
        oracle_accounts: oracle_accounts.len() as u8,
    }];
    let mut liquidated = false;
    for attempt in 0..8 {
        let result = env.crank_with_reward(
            4,
            0,
            trade_slot,
            if attempt == 0 {
                observations.clone()
            } else {
                Vec::new()
            },
            if attempt == 0 { &oracle_accounts } else { &[] },
        );
        result.map_err(|error| {
            format!("{mode:?} {route:?} honest liquidation crank {attempt}: {error}")
        })?;
        let victim_q = decoded_legs(&env.primary_portfolio(0))
            .into_iter()
            .find(|leg| leg.active && leg.asset_index == 0)
            .map(|leg| leg.basis_pos_q.unsigned_abs())
            .unwrap_or(0);
        if victim_q < POS_SCALE {
            liquidated = true;
            break;
        }
    }
    if !liquidated {
        return Err(format!(
            "{mode:?} {route:?} paid mark move did not liquidate the independent victim"
        ));
    }
    let after_liquidation = env.primary_market_state().1;
    let cranker_reward = env
        .primary_portfolio(4)
        .capital
        .get()
        .checked_sub(cranker_capital_before)
        .ok_or("PR 280 liquidation reduced cranker capital")?;
    let retained_penalty = after_liquidation
        .insurance
        .checked_sub(group_after_move.insurance)
        .ok_or("PR 280 liquidation reduced insurance")?;
    let victim_penalty = cranker_reward
        .checked_add(retained_penalty)
        .ok_or("PR 280 victim penalty overflow")?;
    let victim_capital_loss = victim_capital_before
        .checked_sub(env.primary_portfolio(0).capital.get())
        .ok_or("PR 280 liquidation increased victim capital")?;
    if victim_penalty == 0
        || cranker_reward == 0
        || cranker_reward <= movement_fee
        || victim_capital_loss == 0
    {
        return Err(format!(
            "{mode:?} {route:?} liquidation reward is not a profitable victim transfer: movement {movement_fee}, penalty {victim_penalty}, reward {cranker_reward}, victim loss {victim_capital_loss}"
        ));
    }

    for actor in [2, 3] {
        env.crank(actor, trade_slot, Vec::new())
            .map_err(|error| format!("{mode:?} {route:?} refresh wash actor {actor}: {error}"))?;
    }
    execute_trade_route(&mut env, route, 2, 3, 0, -TINY_Q, queued_mark, 0)
        .map_err(|error| format!("{mode:?} {route:?} close wash pair: {error}"))?;
    for actor in [2, 3] {
        let pnl = env.primary_portfolio(actor).pnl.get();
        if pnl > 0 {
            env.convert_released_pnl(actor, pnl as u128)
                .map_err(|error| format!("{mode:?} {route:?} convert actor {actor}: {error}"))?;
        }
    }
    for actor in [2, 3, 4] {
        let capital = env.primary_portfolio(actor).capital.get();
        env.withdraw_primary(actor, capital)
            .map_err(|error| format!("{mode:?} {route:?} withdraw actor {actor}: {error}"))?;
    }
    let attacker_extracted = [2, 3, 4].into_iter().try_fold(0u128, |sum, actor| {
        sum.checked_add(u128::from(
            env.token_amount(env.actors[actor].destination_token),
        ))
        .ok_or("PR 280 attacker payout overflow")
    })?;
    let attacker_committed = ATTACK_DEPOSIT
        .checked_mul(2)
        .and_then(|value| value.checked_add(CRANKER_DEPOSIT))
        .ok_or("PR 280 attacker commitment overflow")?;
    let attacker_profit = attacker_extracted
        .checked_sub(attacker_committed)
        .ok_or("PR 280 attack remained unprofitable")?;
    if attacker_profit == 0 || env.token_supply_observed() != supply_before {
        return Err(format!(
            "{mode:?} {route:?} did not finish as conserved extractable profit: extracted {attacker_extracted}, committed {attacker_committed}, supply {}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }
    Ok(TradeDrivenLiquidationRewardReproduction {
        blocker: KnownBlocker::TradeDrivenLiquidationReward,
        mode,
        route,
        movement_fee,
        victim_penalty,
        cranker_reward,
        victim_capital_loss,
        attacker_extracted,
        attacker_profit,
    })
}

pub fn reproduce_cross_domain_backing_double_spend(
    mut seed: [u8; 32],
) -> Result<CrossDomainBackingDoubleSpendReproduction, String> {
    seed[0] ^= 0x67;
    const INITIAL_PRICE: u64 = 100;
    const MOVED_PRICE: u64 = 105;
    const SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const CLAIM_PER_ASSET: u128 = 100;
    const LOWER_SOURCE_DOMAIN: usize = 1;
    const FUNDED_SOURCE_DOMAIN: usize = 3;
    const DEPOSIT: u128 = 1_000;

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
        .map_err(|error| format!("PR 267 fund higher source domain: {error}"))?;
    if provider_source_before.checked_sub(env.token_amount(env.provider_source_token))
        != Some((2 * CLAIM_PER_ASSET) as u64)
    {
        return Err("PR 267 provider top-up did not debit exact SPL principal".into());
    }

    for asset in [0u16, 1u16] {
        env.trade_no_cpi(0, 1, asset, SIZE_Q, INITIAL_PRICE, 0)
            .map_err(|error| format!("PR 267 open winner asset {asset}: {error}"))?;
    }
    env.warp_to_slot(2);
    for asset in [0u16, 1u16] {
        env.push_auth_mark(asset, 2, MOVED_PRICE)
            .map_err(|error| format!("PR 267 move asset {asset}: {error}"))?;
        env.crank(
            0,
            2,
            vec![CrankObservationHint {
                asset_index: asset,
                oracle_accounts: env.primary_profile(asset as usize).oracle_leg_count,
            }],
        )
        .map_err(|error| format!("PR 267 refresh winner asset {asset}: {error}"))?;
    }
    let before = env.primary_market_state().1;
    let unfunded_claim_before_num =
        before.source_credit[LOWER_SOURCE_DOMAIN].positive_claim_bound_num;
    let funded_claim_before_num =
        before.source_credit[FUNDED_SOURCE_DOMAIN].positive_claim_bound_num;
    if env.primary_portfolio(0).pnl.get() != (2 * CLAIM_PER_ASSET) as i128
        || unfunded_claim_before_num != CLAIM_PER_ASSET * percolator::BOUND_SCALE
        || funded_claim_before_num != CLAIM_PER_ASSET * percolator::BOUND_SCALE
        || before.source_credit[LOWER_SOURCE_DOMAIN].fresh_reserved_backing_num != 0
        || before.source_credit[FUNDED_SOURCE_DOMAIN].fresh_reserved_backing_num
            != 2 * CLAIM_PER_ASSET * percolator::BOUND_SCALE
    {
        return Err(format!(
            "PR 267 setup did not create one unfunded and one overfunded claim: pnl {}, lower {:?}, funded {:?}",
            env.primary_portfolio(0).pnl.get(),
            before.source_credit[LOWER_SOURCE_DOMAIN],
            before.source_credit[FUNDED_SOURCE_DOMAIN]
        ));
    }

    for asset in [0u16, 1u16] {
        env.trade_no_cpi(0, 2, asset, -SIZE_Q, MOVED_PRICE, 0)
            .map_err(|error| format!("PR 267 flatten winner asset {asset}: {error}"))?;
    }
    if decoded_legs(&env.primary_portfolio(0))
        .into_iter()
        .any(|leg| leg.active)
    {
        return Err("PR 267 winner remained positioned before conversion".into());
    }

    env.convert_released_pnl(0, CLAIM_PER_ASSET)
        .map_err(|error| format!("PR 267 first aggregate conversion: {error}"))?;
    let after_first = env.primary_market_state().1;
    if after_first.source_credit[LOWER_SOURCE_DOMAIN].positive_claim_bound_num != 0
        || after_first.source_credit[FUNDED_SOURCE_DOMAIN].positive_claim_bound_num
            != CLAIM_PER_ASSET * percolator::BOUND_SCALE
        || after_first.source_backing_buckets[FUNDED_SOURCE_DOMAIN].consumed_liened_backing_num
            != CLAIM_PER_ASSET * percolator::BOUND_SCALE
    {
        return Err(format!(
            "PR 267 vulnerable first conversion no longer desynchronizes claim face/backing: lower {:?}, funded {:?}, bucket {:?}",
            after_first.source_credit[LOWER_SOURCE_DOMAIN],
            after_first.source_credit[FUNDED_SOURCE_DOMAIN],
            after_first.source_backing_buckets[FUNDED_SOURCE_DOMAIN]
        ));
    }

    env.trade_no_cpi(0, 2, 0, POS_SCALE as i128, MOVED_PRICE, 0)
        .map_err(|error| format!("PR 267 refresh-certificate open: {error}"))?;
    env.trade_no_cpi(0, 2, 0, -(POS_SCALE as i128), MOVED_PRICE, 0)
        .map_err(|error| format!("PR 267 refresh-certificate close: {error}"))?;
    env.convert_released_pnl(0, CLAIM_PER_ASSET)
        .map_err(|error| format!("PR 267 second cross-domain conversion: {error}"))?;
    let after_second = env.primary_market_state().1;
    let funded_backing_consumed_num =
        after_second.source_backing_buckets[FUNDED_SOURCE_DOMAIN].consumed_liened_backing_num;
    let winner_capital_gain = env
        .primary_portfolio(0)
        .capital
        .get()
        .checked_sub(DEPOSIT)
        .ok_or("PR 267 winner did not gain capital")?;
    if funded_backing_consumed_num != 2 * CLAIM_PER_ASSET * percolator::BOUND_SCALE
        || winner_capital_gain != 2 * CLAIM_PER_ASSET
        || env.primary_portfolio(0).pnl.get() != 0
    {
        return Err(format!(
            "PR 267 did not charge one provider twice: consumed {funded_backing_consumed_num}, winner gain {winner_capital_gain}, pnl {}",
            env.primary_portfolio(0).pnl.get()
        ));
    }
    let winner_capital = env.primary_portfolio(0).capital.get();
    env.withdraw_primary(0, winner_capital)
        .map_err(|error| format!("PR 267 withdraw double-funded winner: {error}"))?;
    let extracted_tokens = env.token_amount(env.actors[0].destination_token);
    if u128::from(extracted_tokens) != DEPOSIT + 2 * CLAIM_PER_ASSET
        || env.token_supply_observed() != supply_before
    {
        return Err(format!(
            "PR 267 terminal SPL extraction mismatch: tokens {extracted_tokens}, supply {}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }
    Ok(CrossDomainBackingDoubleSpendReproduction {
        blocker: KnownBlocker::CrossDomainBackingDoubleSpend,
        unfunded_claim_before_num,
        funded_claim_before_num,
        funded_backing_consumed_num,
        winner_capital_gain,
        extracted_tokens,
    })
}

pub fn reproduce_cross_domain_b_settlement(
    mut seed: [u8; 32],
) -> Result<CrossDomainBSettlementReproduction, String> {
    seed[0] ^= 0x81;
    const INITIAL_PRICE: u64 = 100;
    const FIRST_MARK: u64 = 105;
    const BANKRUPTCY_MARK: u64 = 500;
    const ATTACKER_Q: i128 = 20 * POS_SCALE as i128;
    const UNFUNDED_DOMAIN: usize = 1;
    const FUNDED_DOMAIN: usize = 3;
    const ATTACKER_DEPOSIT: u128 = 100_000;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: INITIAL_PRICE,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [
                ATTACKER_DEPOSIT,
                100_000,
                250,
                100_000,
                super::v16_svm::EXIT_MAKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    let supply_before = env.token_supply_observed();
    for asset_index in [0u16, 1u16] {
        env.configure_auth_mark(false, asset_index, 1, INITIAL_PRICE)
            .map_err(|error| {
                format!("PR 281 configure asset {asset_index} initial mark: {error}")
            })?;
    }
    env.top_up_backing_bucket(FUNDED_DOMAIN as u16, 20_000, 1_000)
        .map_err(|error| format!("PR 281 fund asset-1 source domain: {error}"))?;
    for asset_index in [0u16, 1u16] {
        env.trade_no_cpi(0, 1, asset_index, ATTACKER_Q, INITIAL_PRICE, 0)
            .map_err(|error| format!("PR 281 open asset {asset_index} claim pair: {error}"))?;
    }

    env.warp_to_slot(2);
    for asset_index in [0u16, 1u16] {
        env.push_auth_mark(asset_index, 2, FIRST_MARK)
            .map_err(|error| format!("PR 281 move asset {asset_index}: {error}"))?;
        crank_adapter_steps(&mut env, 0, 2, asset_index, 4)
            .map_err(|error| format!("PR 281 settle asset {asset_index} first claim: {error}"))?;
    }
    let first_claims = env.primary_portfolio(0);
    if source_claim_for_domain(&first_claims, UNFUNDED_DOMAIN) != 100 * percolator::BOUND_SCALE
        || source_claim_for_domain(&first_claims, FUNDED_DOMAIN) != 100 * percolator::BOUND_SCALE
    {
        return Err(format!(
            "PR 281 did not create independent 100-atom source claims: unfunded {}, funded {}",
            source_claim_for_domain(&first_claims, UNFUNDED_DOMAIN),
            source_claim_for_domain(&first_claims, FUNDED_DOMAIN)
        ));
    }

    env.trade_no_cpi(0, 2, 1, POS_SCALE as i128, FIRST_MARK, 0)
        .map_err(|error| format!("PR 281 open bankrupt asset-1 short: {error}"))?;
    env.warp_to_slot(7);
    env.push_auth_mark(1, 7, BANKRUPTCY_MARK)
        .map_err(|error| format!("PR 281 push bankruptcy mark: {error}"))?;
    crank_adapter_steps(&mut env, 0, 7, 1, 12)
        .map_err(|error| format!("PR 281 settle winner before B booking: {error}"))?;
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
        return Err("PR 281 public liquidation did not book asset-1 social loss".into());
    }

    let before_settle = env.primary_portfolio(0);
    let unfunded_claim_before_num = source_claim_for_domain(&before_settle, UNFUNDED_DOMAIN);
    let funded_claim_before_num = source_claim_for_domain(&before_settle, FUNDED_DOMAIN);
    if unfunded_claim_before_num == 0 || funded_claim_before_num <= unfunded_claim_before_num {
        return Err(format!(
            "PR 281 pre-settlement claims lack sparse-domain discriminator: unfunded {unfunded_claim_before_num}, funded {funded_claim_before_num}"
        ));
    }
    crank_adapter_steps(&mut env, 0, 7, 1, 8)
        .map_err(|error| format!("PR 281 settle winner B loss: {error}"))?;
    let after_settle = env.primary_portfolio(0);
    let settled_leg = decoded_legs(&after_settle)
        .into_iter()
        .find(|leg| leg.active && leg.asset_index == 1)
        .ok_or("PR 281 winner lost active asset-1 leg during B settlement")?;
    if settled_leg.b_snap != b_target_num {
        return Err(format!(
            "PR 281 winner B snapshot {} did not reach target {b_target_num}",
            settled_leg.b_snap
        ));
    }
    let pnl_loss = u128::try_from(
        before_settle
            .pnl
            .get()
            .checked_sub(after_settle.pnl.get())
            .ok_or("PR 281 B-settlement PnL subtraction overflow")?,
    )
    .map_err(|_| "PR 281 B settlement did not reduce winner PnL")?;
    let unfunded_claim_after_num = source_claim_for_domain(&after_settle, UNFUNDED_DOMAIN);
    let funded_claim_after_num = source_claim_for_domain(&after_settle, FUNDED_DOMAIN);
    let expected_total_reduction = pnl_loss
        .checked_mul(percolator::BOUND_SCALE)
        .ok_or("PR 281 claim reduction overflow")?;
    let wrong_domain_reduction = unfunded_claim_before_num
        .checked_sub(unfunded_claim_after_num)
        .ok_or("PR 281 unrelated claim increased during B settlement")?;
    let correct_domain_reduction = funded_claim_before_num
        .checked_sub(funded_claim_after_num)
        .ok_or("PR 281 affected claim increased during B settlement")?;
    if pnl_loss == 0
        || wrong_domain_reduction != 0
        || correct_domain_reduction != expected_total_reduction
    {
        return Err(format!(
            "PR 281 domain-local B settlement failed: loss {pnl_loss}, unfunded {unfunded_claim_before_num}->{unfunded_claim_after_num}, funded {funded_claim_before_num}->{funded_claim_after_num}"
        ));
    }

    let mut reduction_steps = 0u8;
    loop {
        let position = observed_positions(&env.primary_portfolio(0))?[1];
        if position == 0 {
            break;
        }
        if position < 0 {
            return Err(format!(
                "PR 281 affected position flipped while reducing: {position}"
            ));
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
                        .ok_or("PR 281 reduction step counter overflow")?;
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
                        return Err(format!("PR 281 failed reduction {reduce_q} mutated state"));
                    }
                    failures.push(format!("{reduce_q}: {error}"));
                }
            }
        }
        if progressed {
            if reduction_steps >= 64 {
                return Err("PR 281 reduction search exceeded 64 successful steps".into());
            }
            continue;
        }
        return Err(format!(
            "PR 281 no bounded public reduction progressed position {position_q}: {}",
            failures.join("; ")
        ));
    }
    let affected_position_after_q = observed_positions(&env.primary_portfolio(0))?[1];
    let asset_zero_position_q = observed_positions(&env.primary_portfolio(0))?[0];
    if asset_zero_position_q <= 0 {
        return Err(format!(
            "PR 281 unrelated asset position changed unexpectedly: {asset_zero_position_q}"
        ));
    }
    env.trade_no_cpi(0, 1, 0, -asset_zero_position_q, FIRST_MARK, 0)
        .map_err(|error| format!("PR 281 close unrelated asset after B settlement: {error}"))?;
    if observed_positions(&env.primary_portfolio(0))? != [0; ASSET_COUNT] {
        return Err("PR 281 owner did not reach a flat public state".into());
    }

    let winner_capital = env.primary_portfolio(0).capital.get();
    let destination_before = env.token_amount(env.actors[0].destination_token);
    env.withdraw_primary(0, winner_capital)
        .map_err(|error| format!("PR 281 withdraw flat owner principal: {error}"))?;
    let principal_withdrawn = u128::from(
        env.token_amount(env.actors[0].destination_token)
            .checked_sub(destination_before)
            .ok_or("PR 281 owner destination decreased")?,
    );
    let token_supply_conserved = env.token_supply_observed() == supply_before;
    if principal_withdrawn != winner_capital || !token_supply_conserved {
        return Err(format!(
            "PR 281 terminal principal reconciliation failed: withdrew {principal_withdrawn}/{winner_capital}, supply {}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }
    Ok(CrossDomainBSettlementReproduction {
        blocker: KnownBlocker::CrossDomainBSettlement,
        b_target_num,
        pnl_loss,
        unfunded_claim_before_num,
        unfunded_claim_after_num,
        funded_claim_before_num,
        funded_claim_after_num,
        wrong_domain_reduction_num: wrong_domain_reduction,
        correct_domain_reduction_num: correct_domain_reduction,
        reduction_steps,
        affected_position_after_q,
        principal_withdrawn,
        token_supply_conserved,
    })
}

#[derive(Clone, Copy, Debug)]
struct PendingEwmaTargetWorld {
    low_price: u64,
    target: u64,
    movement_fee: u128,
    victim_withdrawn: u64,
    attacker_withdrawn: u128,
    observed_token_supply: u128,
}

pub fn reproduce_pending_ewma_target_override(
    mut seed: [u8; 32],
    route: TradeRoute,
) -> Result<PendingEwmaTargetOverrideReproduction, String> {
    seed[0] ^= 0x82;
    const ATTACKER_DEPOSITS: u128 = 24_000_000_000;

    let control = run_pending_ewma_target_world(seed, route, false)?;
    let attack = run_pending_ewma_target_world(seed, route, true)?;
    if control.low_price != attack.low_price
        || control.target != 10_000_000
        || attack.target >= control.target
        || control.movement_fee != 0
        || attack.movement_fee == 0
        || control.observed_token_supply != attack.observed_token_supply
    {
        return Err(format!(
            "{route:?} PR 282 setup did not isolate a paid pending-target override: control={control:?}, attack={attack:?}"
        ));
    }
    let displaced_victim_pnl = u128::from(control.victim_withdrawn)
        .checked_sub(u128::from(attack.victim_withdrawn))
        .ok_or("PR 282 wash increased independent-victim payout")?;
    let attacker_profit = attack
        .attacker_withdrawn
        .checked_sub(ATTACKER_DEPOSITS)
        .ok_or("PR 282 coalition did not recover its public deposits")?;
    if displaced_victim_pnl == 0
        || attacker_profit == 0
        || attack.movement_fee >= displaced_victim_pnl
    {
        return Err(format!(
            "{route:?} PR 282 did not expose underpriced independent-victim PnL displacement: control={control:?}, attack={attack:?}, victim displacement={displaced_victim_pnl}, attacker profit={attacker_profit}"
        ));
    }
    Ok(PendingEwmaTargetOverrideReproduction {
        blocker: KnownBlocker::PendingEwmaTargetOverride,
        route,
        low_price: attack.low_price,
        control_target: control.target,
        attack_target: attack.target,
        movement_fee: attack.movement_fee,
        displaced_victim_pnl,
        attacker_profit,
        victim_withdrawn: attack.victim_withdrawn,
        attacker_withdrawn: attack.attacker_withdrawn,
    })
}

fn run_pending_ewma_target_world(
    seed: [u8; 32],
    route: TradeRoute,
    with_wash: bool,
) -> Result<PendingEwmaTargetWorld, String> {
    const BASIS: u64 = 10_000_000;
    const DIRECTIONAL_Q: i128 = 1_000 * POS_SCALE as i128;
    const WASH_Q: i128 = 100 * POS_SCALE as i128;
    const DIRECTIONAL_DEPOSIT: u128 = 20_000_000_000;
    const WASH_DEPOSIT: u128 = 2_000_000_000;

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
                WASH_DEPOSIT,
                WASH_DEPOSIT,
                super::v16_svm::EXIT_MAKER_DEPOSIT,
            ],
            actor_token_balances: [
                25_000_000_000,
                25_000_000_000,
                3_000_000_000,
                3_000_000_000,
                super::v16_svm::EXIT_MAKER_TOKEN_BALANCE,
            ],
            ..MarketConfig::default()
        },
    );
    env.configure_ewma_mark(0, 1, BASIS, 1, 0)
        .map_err(|error| format!("{route:?} configure pending-target EWMA: {error}"))?;
    env.trade_no_cpi(0, 1, 0, DIRECTIONAL_Q, BASIS, 0)
        .map_err(|error| format!("{route:?} open independent directional OI: {error}"))?;

    let observations = || {
        vec![CrankObservationHint {
            asset_index: 0,
            oracle_accounts: 0,
        }]
    };
    for slot in 2..=5 {
        env.warp_to_slot(slot);
        env.push_ewma_mark(0, slot, 1).map_err(|error| {
            format!("{route:?} publish honest low mark at slot {slot}: {error}")
        })?;
        for actor in [0, 1] {
            env.crank(actor, slot, observations()).map_err(|error| {
                format!("{route:?} accrue directional actor {actor} at slot {slot}: {error}")
            })?;
        }
    }
    let low_price = env.primary_market_state().1.assets[0].effective_price;
    if low_price >= BASIS / 5 {
        return Err(format!(
            "{route:?} honest bounded move did not reach a low mark: {low_price}"
        ));
    }

    env.warp_to_slot(6);
    for actor in [0, 1] {
        env.crank(actor, 6, observations()).map_err(|error| {
            format!("{route:?} advance directional actor {actor} before rebound: {error}")
        })?;
    }
    let rebound_input = BASIS
        .checked_mul(2)
        .and_then(|value| value.checked_sub(low_price))
        .ok_or("PR 282 rebound input overflow")?;
    env.push_ewma_mark(0, 6, rebound_input)
        .map_err(|error| format!("{route:?} publish honest pending rebound: {error}"))?;
    let pending_target = env.primary_profile(0).mark_ewma_e6;
    let pending_effective = env.primary_market_state().1.assets[0].effective_price;
    if pending_target != BASIS || pending_effective != low_price {
        return Err(format!(
            "{route:?} honest rebound did not remain circuit-breaker pending: target={pending_target}, effective={pending_effective}, expected={BASIS}/{low_price}"
        ));
    }

    env.warp_to_slot(7);
    let insurance_before = env.primary_market_state().1.insurance;
    if with_wash {
        execute_trade_route(&mut env, route, 2, 3, 0, WASH_Q, 1, 0)
            .map_err(|error| format!("{route:?} open target-override wash: {error}"))?;
        execute_trade_route(&mut env, route, 2, 3, 0, -WASH_Q, low_price, 0)
            .map_err(|error| format!("{route:?} close target-override wash: {error}"))?;
    }
    let movement_fee = env
        .primary_market_state()
        .1
        .insurance
        .checked_sub(insurance_before)
        .ok_or("PR 282 movement fee decreased insurance")?;
    let target = env.primary_profile(0).mark_ewma_e6;

    let mut slot = 7;
    loop {
        for actor in [0, 1] {
            env.crank(actor, slot, observations()).map_err(|error| {
                format!("{route:?} converge actor {actor} at slot {slot}: {error}")
            })?;
        }
        if env.primary_market_state().1.assets[0].effective_price == target {
            break;
        }
        slot = slot
            .checked_add(1)
            .ok_or("PR 282 convergence slot overflow")?;
        if slot >= 24 {
            return Err(format!(
                "{route:?} pending target did not converge by slot 23"
            ));
        }
        env.warp_to_slot(slot);
    }
    env.trade_no_cpi(0, 1, 0, -DIRECTIONAL_Q, target, 0)
        .map_err(|error| format!("{route:?} close independent directional OI: {error}"))?;
    env.resolve_market()
        .map_err(|error| format!("{route:?} resolve pending-target world: {error}"))?;
    for actor in 0..4 {
        env.close_resolved_primary(actor)
            .map_err(|error| format!("{route:?} close resolved actor {actor}: {error}"))?;
    }
    for actor in 0..4 {
        let _ = env.claim_resolved_payout_topup_primary(actor);
    }
    let victim_withdrawn = env.token_amount(env.actors[0].destination_token);
    let attacker_withdrawn = [1, 2, 3]
        .into_iter()
        .map(|actor| u128::from(env.token_amount(env.actors[actor].destination_token)))
        .sum();
    Ok(PendingEwmaTargetWorld {
        low_price,
        target,
        movement_fee,
        victim_withdrawn,
        attacker_withdrawn,
        observed_token_supply: env.token_supply_observed(),
    })
}

#[derive(Clone, Copy, Debug)]
struct TerminalDustPayoutWorld {
    low_price: u64,
    victim_withdrawn: u128,
    attacker_withdrawn: u128,
    vault_remaining: u128,
    observed_token_supply: u128,
}

pub fn reproduce_terminal_dust_payout_erasure(
    mut seed: [u8; 32],
    route: TradeRoute,
) -> Result<TerminalDustPayoutErasureReproduction, String> {
    seed[0] ^= 0x83;
    const ATTACKER_DEPOSITS: u128 = 20_000_002_000;
    const VICTIM_DEPOSIT: u128 = 20_000_000_000;

    if !matches!(route, TradeRoute::NoCpi | TradeRoute::BatchNoCpi) {
        return Err(format!("PR 283 unsupported trade route {route:?}"));
    }
    let control = run_terminal_dust_payout_world(seed, route, false)?;
    let attack = run_terminal_dust_payout_world(seed, route, true)?;
    let attacker_loss = ATTACKER_DEPOSITS
        .checked_sub(attack.attacker_withdrawn)
        .ok_or("PR 283 dust attack increased coalition payout")?;
    let victim_loss = VICTIM_DEPOSIT
        .checked_sub(attack.victim_withdrawn)
        .ok_or("PR 283 dust attack increased victim payout")?;
    if control.low_price != attack.low_price
        || control.attacker_withdrawn != ATTACKER_DEPOSITS
        || control.victim_withdrawn != VICTIM_DEPOSIT
        || control.vault_remaining != 0
        || attacker_loss != 1
        || victim_loss == 0
        || attack.vault_remaining != victim_loss + attacker_loss
        || control.observed_token_supply != attack.observed_token_supply
    {
        return Err(format!(
            "{route:?} PR 283 did not turn one atom into claimless terminal victim loss: control={control:?}, attack={attack:?}, attacker loss={attacker_loss}, victim loss={victim_loss}"
        ));
    }
    Ok(TerminalDustPayoutErasureReproduction {
        blocker: KnownBlocker::TerminalDustPayoutErasure,
        route,
        attacker_loss,
        victim_loss,
        vault_remaining: attack.vault_remaining,
        victim_withdrawn: attack.victim_withdrawn,
        attacker_withdrawn: attack.attacker_withdrawn,
    })
}

fn run_terminal_dust_payout_world(
    seed: [u8; 32],
    route: TradeRoute,
    with_dust: bool,
) -> Result<TerminalDustPayoutWorld, String> {
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
                super::v16_svm::EXIT_MAKER_DEPOSIT,
            ],
            actor_token_balances: [
                25_000_000_000,
                25_000_000_000,
                1_000_000,
                1_000_000,
                super::v16_svm::EXIT_MAKER_TOKEN_BALANCE,
            ],
            ..MarketConfig::default()
        },
    );
    env.configure_ewma_mark(0, 1, BASIS, 1, 0)
        .map_err(|error| format!("{route:?} configure terminal-dust EWMA: {error}"))?;
    env.trade_no_cpi(0, 1, 0, DIRECTIONAL_Q, BASIS, 0)
        .map_err(|error| format!("{route:?} open terminal-dust directional OI: {error}"))?;

    let observations = || {
        vec![CrankObservationHint {
            asset_index: 0,
            oracle_accounts: 0,
        }]
    };
    for slot in 2..=5 {
        env.warp_to_slot(slot);
        env.push_ewma_mark(0, slot, 1)
            .map_err(|error| format!("{route:?} publish terminal-dust low slot {slot}: {error}"))?;
        for actor in [0, 1] {
            env.crank(actor, slot, observations()).map_err(|error| {
                format!("{route:?} accrue terminal-dust actor {actor} at slot {slot}: {error}")
            })?;
        }
    }
    let low_price = env.primary_market_state().1.assets[0].effective_price;
    if low_price >= BASIS / 5 {
        return Err(format!(
            "{route:?} terminal-dust setup did not reach a low mark: {low_price}"
        ));
    }
    if with_dust {
        execute_trade_route(&mut env, route, 2, 3, 0, DUST_Q, low_price, 0)
            .map_err(|error| format!("{route:?} open one-quantum source position: {error}"))?;
    }

    env.warp_to_slot(6);
    for actor in [0, 1] {
        env.crank(actor, 6, observations()).map_err(|error| {
            format!("{route:?} advance terminal-dust actor {actor} before rebound: {error}")
        })?;
    }
    let rebound_input = BASIS
        .checked_mul(2)
        .and_then(|value| value.checked_sub(low_price))
        .ok_or("PR 283 rebound input overflow")?;
    env.push_ewma_mark(0, 6, rebound_input)
        .map_err(|error| format!("{route:?} publish terminal-dust rebound: {error}"))?;
    if env.primary_profile(0).mark_ewma_e6 != BASIS {
        return Err(format!(
            "{route:?} terminal-dust rebound missed basis: {}",
            env.primary_profile(0).mark_ewma_e6
        ));
    }

    env.warp_to_slot(7);
    let mut slot = 7;
    loop {
        for actor in [0, 1] {
            env.crank(actor, slot, observations()).map_err(|error| {
                format!("{route:?} converge terminal-dust actor {actor} at slot {slot}: {error}")
            })?;
        }
        if env.primary_market_state().1.assets[0].effective_price == BASIS {
            break;
        }
        slot = slot
            .checked_add(1)
            .ok_or("PR 283 convergence slot overflow")?;
        if slot >= 24 {
            return Err(format!(
                "{route:?} terminal-dust rebound did not converge by slot 23"
            ));
        }
        env.warp_to_slot(slot);
    }
    if with_dust {
        execute_trade_route(&mut env, route, 2, 3, 0, -DUST_Q, BASIS, 0)
            .map_err(|error| format!("{route:?} close one-quantum source position: {error}"))?;
    }
    env.trade_no_cpi(0, 1, 0, -DIRECTIONAL_Q, BASIS, 0)
        .map_err(|error| format!("{route:?} close terminal-dust directional OI: {error}"))?;
    env.resolve_market()
        .map_err(|error| format!("{route:?} resolve terminal-dust world: {error}"))?;
    for actor in 0..PRIMARY_ACTOR_COUNT {
        env.close_resolved_primary(actor)
            .map_err(|error| format!("{route:?} first close resolved actor {actor}: {error}"))?;
    }
    for actor in 0..PRIMARY_ACTOR_COUNT {
        for _ in 0..16 {
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
        return Err(format!(
            "{route:?} terminal payout still progressed after bounded close/top-up drain"
        ));
    }

    let victim_withdrawn = u128::from(env.token_amount(env.actors[0].destination_token));
    let attacker_withdrawn = [1, 2, 3]
        .into_iter()
        .map(|actor| u128::from(env.token_amount(env.actors[actor].destination_token)))
        .sum();
    let vault_remaining = env.primary_market_state().1.vault;
    if vault_remaining != u128::from(env.token_amount(env.vault)) {
        return Err(format!(
            "{route:?} terminal-dust engine/SPL vault mismatch: engine={vault_remaining}, SPL={}",
            env.token_amount(env.vault)
        ));
    }
    Ok(TerminalDustPayoutWorld {
        low_price,
        victim_withdrawn,
        attacker_withdrawn,
        vault_remaining,
        observed_token_supply: env.token_supply_observed(),
    })
}

pub fn reproduce_cross_margin_insurance_drain(
    mut seed: [u8; 32],
) -> Result<CrossMarginInsuranceDrainReproduction, String> {
    seed[0] ^= 0x90;
    const MARK: u64 = 100;
    const ATTACKER_DEPOSIT: u128 = 20_200;
    const INSURANCE_TOPUP: u128 = 100_000;

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
    env.top_up_insurance_domain(1, INSURANCE_TOPUP)
        .map_err(|error| format!("PR 290 fund unrelated asset-0 insurance: {error}"))?;
    env.trade_no_cpi(0, 2, 0, POS_SCALE as i128, MARK, 0)
        .map_err(|error| format!("PR 290 open unrelated asset-0 leg: {error}"))?;
    env.trade_no_cpi(0, 1, 1, -(POS_SCALE as i128), MARK, 0)
        .map_err(|error| format!("PR 290 open loss-bearing asset-1 leg: {error}"))?;

    env.warp_to_slot(2);
    for asset_index in [0, 1] {
        let observations = vec![CrankObservationHint {
            asset_index,
            oracle_accounts: 0,
        }];
        env.crank(3, 2, observations)
            .map_err(|error| format!("PR 290 accrue asset {asset_index}: {error}"))?;
    }
    env.sync_maintenance_fee(0, 2)
        .map_err(|error| format!("PR 290 exhaust loser capital: {error}"))?;
    let fee_drained = env.primary_portfolio(0);
    if fee_drained.capital.get() != 0
        || !portfolio_has_active_asset(&fee_drained, 0)
        || !portfolio_has_active_asset(&fee_drained, 1)
    {
        return Err(format!(
            "PR 290 maintenance setup did not retain both zero-capital legs: capital={}, asset0={}, asset1={}",
            fee_drained.capital.get(),
            portfolio_has_active_asset(&fee_drained, 0),
            portfolio_has_active_asset(&fee_drained, 1)
        ));
    }

    let mut mark = MARK;
    for slot in 3..=12 {
        mark = mark.checked_mul(2).ok_or("PR 290 adverse mark overflow")?;
        env.warp_to_slot(slot);
        env.push_auth_mark(1, slot, mark)
            .map_err(|error| format!("PR 290 publish asset-1 mark at slot {slot}: {error}"))?;
        env.crank(
            3,
            slot,
            vec![CrankObservationHint {
                asset_index: 1,
                oracle_accounts: 0,
            }],
        )
        .map_err(|error| format!("PR 290 advance asset-1 mark at slot {slot}: {error}"))?;
    }
    env.rebalance_reduce(0, 1, POS_SCALE)
        .map_err(|error| format!("PR 290 owner flattens loss-bearing asset-1 leg: {error}"))?;
    let flattened = env.primary_portfolio(0);
    if portfolio_has_active_asset(&flattened, 1)
        || !portfolio_has_active_asset(&flattened, 0)
        || flattened.pnl.get() >= 0
        || flattened.capital.get() != 0
    {
        return Err(format!(
            "PR 290 did not isolate account debt from the surviving asset-0 leg: capital={}, pnl={}, asset0={}, asset1={}",
            flattened.capital.get(),
            flattened.pnl.get(),
            portfolio_has_active_asset(&flattened, 0),
            portfolio_has_active_asset(&flattened, 1)
        ));
    }

    let spent_before = env.primary_market_state().1.insurance_domain_spent[1];
    let mut liquidation_calls = 0u16;
    for _ in 0..512 {
        let account = env.primary_portfolio(0);
        if account.pnl.get() >= 0 && !portfolio_has_active_asset(&account, 0) {
            break;
        }
        match env.crank(0, 12, vec![]) {
            Ok(_) => {
                liquidation_calls = liquidation_calls
                    .checked_add(1)
                    .ok_or("PR 290 liquidation call count overflow")?;
            }
            Err(error) if error.contains("Custom(23)") => break,
            Err(error) => return Err(format!("PR 290 public liquidation failed: {error}")),
        }
    }
    let unrelated_insurance_spent = env.primary_market_state().1.insurance_domain_spent[1]
        .checked_sub(spent_before)
        .ok_or("PR 290 insurance spent counter decreased")?;
    if liquidation_calls == 0 || unrelated_insurance_spent == 0 {
        return Err(format!(
            "PR 290 no longer drains unrelated insurance: calls={liquidation_calls}, spent={unrelated_insurance_spent}"
        ));
    }

    env.resolve_market()
        .map_err(|error| format!("PR 290 resolve drained market: {error}"))?;
    let (loser_payout, loser_close_calls) = drain_resolved_actor(&mut env, 0)?;
    let (counterparty_payout, counterparty_close_calls) = drain_resolved_actor(&mut env, 2)?;
    let (winner_payout, winner_close_calls) = drain_resolved_actor(&mut env, 1)?;
    let attacker_payout = loser_payout
        .checked_add(counterparty_payout)
        .and_then(|value| value.checked_add(winner_payout))
        .ok_or("PR 290 attacker payout overflow")?;
    let attacker_profit = attacker_payout
        .checked_sub(ATTACKER_DEPOSIT)
        .ok_or("PR 290 attacker coalition did not recover deposits")?;
    if attacker_profit == 0 || env.token_supply_observed() != supply_before {
        return Err(format!(
            "PR 290 cross-domain spend was not publicly extractable: payout={attacker_payout}, profit={attacker_profit}, supply={}/{}",
            env.token_supply_observed(),
            supply_before
        ));
    }
    Ok(CrossMarginInsuranceDrainReproduction {
        blocker: KnownBlocker::CrossMarginInsuranceDrain,
        unrelated_insurance_spent,
        attacker_payout,
        attacker_profit,
        liquidation_calls,
        loser_close_calls,
        counterparty_close_calls,
        winner_close_calls,
    })
}

fn drain_resolved_actor(env: &mut V16Svm, actor: usize) -> Result<(u128, u16), String> {
    let destination = env.actors[actor].destination_token;
    let payout_before = env.token_amount(destination);
    let mut calls = 0u16;
    for _ in 0..512 {
        let market_before = env.market_data(false);
        let portfolio_before = env.primary_portfolio_data(actor);
        let destination_before = env.token_amount(destination);
        if env.close_resolved_primary(actor).is_ok() {
            calls = calls
                .checked_add(1)
                .ok_or("resolved close call count overflow")?;
        }
        let _ = env.claim_resolved_payout_topup_primary(actor);
        if env.market_data(false) == market_before
            && env.primary_portfolio_data(actor) == portfolio_before
            && env.token_amount(destination) == destination_before
        {
            let payout = env
                .token_amount(destination)
                .checked_sub(payout_before)
                .ok_or("resolved payout destination decreased")?;
            return Ok((u128::from(payout), calls));
        }
    }
    Err(format!(
        "resolved actor {actor} did not reach a fixed point in 512 calls"
    ))
}

fn portfolio_has_active_asset(
    account: &percolator_prog::state::PortfolioAccountV16,
    asset_index: usize,
) -> bool {
    decoded_legs(account)
        .into_iter()
        .any(|leg| leg.active && leg.asset_index as usize == asset_index)
}

fn reset_pending_side_count(group: &percolator_prog::state::MarketGroupV16) -> usize {
    group
        .assets
        .iter()
        .take(ASSET_COUNT)
        .map(|asset| {
            usize::from(asset.mode_long == SideModeV16::ResetPending)
                + usize::from(asset.mode_short == SideModeV16::ResetPending)
        })
        .sum()
}

fn reset_pending_work_for_account(
    group: &percolator_prog::state::MarketGroupV16,
    account: &PortfolioAccountV16,
) -> Result<u128, String> {
    let mut work = 0u128;
    let legs = decoded_legs(account);
    for (asset_index, asset) in group.assets.iter().take(ASSET_COUNT).enumerate() {
        let (long_domain, short_domain) = v16_domain_pair_for_asset_index(asset_index)
            .map_err(|error| format!("reset-work domain mapping: {error:?}"))?;
        for (mode, stored, stale, pending, domain, side) in [
            (
                asset.mode_long,
                asset.stored_pos_count_long,
                asset.stale_account_count_long,
                asset.pending_obligation_count_long,
                long_domain,
                SideV16::Long,
            ),
            (
                asset.mode_short,
                asset.stored_pos_count_short,
                asset.stale_account_count_short,
                asset.pending_obligation_count_short,
                short_domain,
                SideV16::Short,
            ),
        ] {
            if mode != SideModeV16::ResetPending {
                continue;
            }
            let barrier = group
                .pending_domain_loss_barriers
                .get(domain)
                .copied()
                .ok_or_else(|| format!("reset-work missing domain {domain}"))?;
            let account_legs = legs
                .iter()
                .filter(|leg| {
                    leg.active && leg.asset_index as usize == asset_index && leg.side == side
                })
                .count() as u64;
            let already_finalizable = stored == 0 && stale == 0 && pending == 0 && barrier == 0;
            let owns_last_stored_legs = account_legs != 0 && account_legs == stored;
            let side_work = u128::from(account_legs)
                .checked_add(u128::from(already_finalizable || owns_last_stored_legs))
                .ok_or("reset-side progress rank overflow")?;
            work = work
                .checked_add(side_work)
                .ok_or("aggregate reset progress rank overflow")?;
        }
    }
    Ok(work)
}

fn reset_side_finalizable(
    group: &percolator_prog::state::MarketGroupV16,
    asset_index: usize,
    side: u8,
) -> bool {
    let Some(asset) = group.assets.get(asset_index) else {
        return false;
    };
    let Ok((long_domain, short_domain)) = v16_domain_pair_for_asset_index(asset_index) else {
        return false;
    };
    let (mode, stored, stale, pending, domain) = if side == 0 {
        (
            asset.mode_long,
            asset.stored_pos_count_long,
            asset.stale_account_count_long,
            asset.pending_obligation_count_long,
            long_domain,
        )
    } else {
        (
            asset.mode_short,
            asset.stored_pos_count_short,
            asset.stale_account_count_short,
            asset.pending_obligation_count_short,
            short_domain,
        )
    };
    mode == SideModeV16::ResetPending
        && stored == 0
        && stale == 0
        && pending == 0
        && group
            .pending_domain_loss_barriers
            .get(domain)
            .copied()
            .unwrap_or(u64::MAX)
            == 0
}

fn asset_contributes_to_loss_stale(asset: &percolator::AssetStateV16) -> bool {
    matches!(
        asset.lifecycle,
        AssetLifecycleV16::Active | AssetLifecycleV16::DrainOnly
    ) && (asset.oi_eff_long_q != 0
        || asset.oi_eff_short_q != 0
        || asset.stored_pos_count_long != 0
        || asset.stored_pos_count_short != 0
        || asset.stale_account_count_long != 0
        || asset.stale_account_count_short != 0
        || asset.pending_obligation_count_long != 0
        || asset.pending_obligation_count_short != 0
        || asset.loss_weight_sum_long != 0
        || asset.loss_weight_sum_short != 0)
}

pub(crate) fn execute_trade_route(
    env: &mut V16Svm,
    route: TradeRoute,
    taker: usize,
    maker: usize,
    asset_index: u16,
    size_q: i128,
    price: u64,
    fee_bps: u64,
) -> Result<TxSuccess, String> {
    let market_id = env.primary_market_state().1.assets[asset_index as usize].market_id;
    match route {
        TradeRoute::NoCpi => env.trade_no_cpi(taker, maker, asset_index, size_q, price, fee_bps),
        TradeRoute::Cpi => env.trade_cpi(taker, maker, asset_index, size_q, fee_bps, 0),
        TradeRoute::BatchNoCpi => env.batch_trade_no_cpi(
            taker,
            maker,
            vec![BatchTradeLeg {
                asset_index,
                market_id,
                size_q,
                exec_price: price,
                fee_bps,
            }],
        ),
        TradeRoute::BatchCpi => env.batch_trade_cpi(
            taker,
            maker,
            vec![BatchTradeCpiLeg {
                asset_index,
                market_id,
                size_q,
                fee_bps,
                limit_price: 0,
            }],
        ),
    }
}

#[allow(dead_code)]
pub fn verify_attributed_pnl_roundtrip(
    mut seed: [u8; 32],
    open_route: TradeRoute,
    close_route: TradeRoute,
    account_a_long: bool,
) -> Result<(), String> {
    const ACCOUNT_A: usize = 0;
    const ACCOUNT_B: usize = 1;
    const MARKET_CRANKER: usize = 4;
    const ASSET: u16 = 0;
    const START_PRICE: u64 = 1_000_000;
    const SETTLED_PRICE: u64 = 1_100_000;
    const DEPOSIT: u128 = 2_000_000;
    const EXPECTED_PNL: u128 = 100_000;
    const UNRELATED_DEPOSIT: u128 = 1;

    fn assert_token_frame(
        label: &str,
        before: &[(Pubkey, Vec<u8>)],
        after: &[(Pubkey, Vec<u8>)],
        mutable: &[Pubkey],
    ) -> Result<(), String> {
        if before.len() != after.len() {
            return Err(format!("{label}: tracked token-account set changed"));
        }
        for ((before_key, before_data), (after_key, after_data)) in before.iter().zip(after) {
            if before_key != after_key {
                return Err(format!("{label}: tracked token-account order changed"));
            }
            if !mutable.contains(before_key) && before_data != after_data {
                return Err(format!(
                    "{label}: unrelated token account {before_key} changed"
                ));
            }
        }
        Ok(())
    }

    fn assert_custody(
        label: &str,
        env: &V16Svm,
        expected_vault: u128,
        expected_capital: u128,
    ) -> Result<(), String> {
        let (_, group) = env.primary_market_state();
        if group.vault != expected_vault
            || u128::from(env.token_amount(env.vault)) != expected_vault
            || group.c_tot != expected_capital
        {
            return Err(format!(
                "{label}: custody/capital mismatch: accounting vault={}, SPL vault={}, c_tot={}, expected={expected_vault}/{expected_capital}",
                group.vault,
                env.token_amount(env.vault),
                group.c_tot,
            ));
        }
        Ok(())
    }

    seed[0] ^= (open_route.index() as u8) << 4;
    seed[0] ^= (close_route.index() as u8) << 1;
    seed[0] ^= u8::from(account_a_long);
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: START_PRICE,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            max_abs_funding_e9_per_slot: 0,
            min_funding_lifetime_slots: 1,
            maintenance_fee_per_slot: 0,
            actor_deposits: [
                DEPOSIT,
                DEPOSIT,
                UNRELATED_DEPOSIT,
                UNRELATED_DEPOSIT,
                UNRELATED_DEPOSIT,
            ],
            actor_token_balances: [
                DEPOSIT as u64,
                DEPOSIT as u64,
                UNRELATED_DEPOSIT as u64,
                UNRELATED_DEPOSIT as u64,
                UNRELATED_DEPOSIT as u64,
            ],
            ..MarketConfig::default()
        },
    );
    let baseline_vault = DEPOSIT
        .checked_mul(2)
        .and_then(|value| value.checked_add(UNRELATED_DEPOSIT * 3))
        .ok_or("INV-024 baseline vault overflow")?;
    let initial_token_frame = env.all_token_account_data();
    let unrelated_portfolios = [2usize, 3usize]
        .into_iter()
        .map(|actor| env.primary_portfolio_data(actor))
        .collect::<Vec<_>>();
    let initial_supply = env.token_supply_observed();
    let (_, initial_group) = env.primary_market_state();
    let initial_insurance = initial_group.insurance;
    let initial_insurance_budget = initial_group.insurance_domain_budget.clone();
    let initial_insurance_spent = initial_group.insurance_domain_spent.clone();
    let initial_insurance_reservations = initial_group.insurance_credit_reservations.clone();
    assert_custody("INV-024 initialized", &env, baseline_vault, baseline_vault)?;

    let open_size = if account_a_long {
        POS_SCALE as i128
    } else {
        -(POS_SCALE as i128)
    };
    let (winner, loser) = if account_a_long {
        (ACCOUNT_A, ACCOUNT_B)
    } else {
        (ACCOUNT_B, ACCOUNT_A)
    };
    execute_trade_route(
        &mut env,
        open_route,
        ACCOUNT_A,
        ACCOUNT_B,
        ASSET,
        open_size,
        START_PRICE,
        0,
    )
    .map_err(|error| format!("INV-024 {open_route:?} open: {error}"))?;
    let opened_a = env.primary_portfolio(ACCOUNT_A);
    let opened_b = env.primary_portfolio(ACCOUNT_B);
    if opened_a.capital.get() != DEPOSIT
        || opened_b.capital.get() != DEPOSIT
        || opened_a.pnl.get() != 0
        || opened_b.pnl.get() != 0
        || position_for_asset(&opened_a, ASSET as usize)? != open_size
        || position_for_asset(&opened_b, ASSET as usize)? != -open_size
    {
        return Err(format!(
            "INV-024 {open_route:?} open misattributed state: A cap/pnl={}/{}, B cap/pnl={}/{}, positions={}/{}",
            opened_a.capital.get(),
            opened_a.pnl.get(),
            opened_b.capital.get(),
            opened_b.pnl.get(),
            position_for_asset(&opened_a, ASSET as usize)?,
            position_for_asset(&opened_b, ASSET as usize)?,
        ));
    }
    assert_custody("INV-024 after open", &env, baseline_vault, baseline_vault)?;
    assert_token_frame(
        "INV-024 open",
        &initial_token_frame,
        &env.all_token_account_data(),
        &[],
    )?;

    env.warp_to_slot(2);
    env.push_auth_mark(ASSET, 2, SETTLED_PRICE)
        .map_err(|error| format!("INV-024 publish settled mark: {error}"))?;
    crank_market_then_accounts_once(&mut env, MARKET_CRANKER, &[loser, winner], 2, ASSET, 8)?;
    let settled_winner = env.primary_portfolio(winner);
    let settled_loser = env.primary_portfolio(loser);
    if settled_winner.capital.get() != DEPOSIT
        || settled_winner.pnl.get() != EXPECTED_PNL as i128
        || settled_loser.capital.get() != DEPOSIT - EXPECTED_PNL
        || settled_loser.pnl.get() != 0
    {
        return Err(format!(
            "INV-024 {open_route:?}/{close_route:?} settled PnL went to the wrong owner: winner {winner} cap/pnl={}/{}, loser {loser} cap/pnl={}/{}",
            settled_winner.capital.get(),
            settled_winner.pnl.get(),
            settled_loser.capital.get(),
            settled_loser.pnl.get(),
        ));
    }
    assert_custody(
        "INV-024 after settlement",
        &env,
        baseline_vault,
        baseline_vault - EXPECTED_PNL,
    )?;
    assert_token_frame(
        "INV-024 settlement",
        &initial_token_frame,
        &env.all_token_account_data(),
        &[],
    )?;

    execute_trade_route(
        &mut env,
        close_route,
        ACCOUNT_A,
        ACCOUNT_B,
        ASSET,
        -open_size,
        SETTLED_PRICE,
        0,
    )
    .map_err(|error| format!("INV-024 {close_route:?} close: {error}"))?;
    let flat_winner = env.primary_portfolio(winner);
    let flat_loser = env.primary_portfolio(loser);
    if decoded_legs(&flat_winner).iter().any(|leg| leg.active)
        || decoded_legs(&flat_loser).iter().any(|leg| leg.active)
        || flat_winner.capital.get() != DEPOSIT
        || flat_winner.pnl.get() != EXPECTED_PNL as i128
        || flat_loser.capital.get() != DEPOSIT - EXPECTED_PNL
        || flat_loser.pnl.get() != 0
    {
        return Err(format!(
            "INV-024 {open_route:?}/{close_route:?} close changed owner attribution"
        ));
    }
    assert_custody(
        "INV-024 after close",
        &env,
        baseline_vault,
        baseline_vault - EXPECTED_PNL,
    )?;
    assert_token_frame(
        "INV-024 close",
        &initial_token_frame,
        &env.all_token_account_data(),
        &[],
    )?;

    env.convert_released_pnl(winner, EXPECTED_PNL)
        .map_err(|error| format!("INV-024 winner conversion: {error}"))?;
    let converted_winner = env.primary_portfolio(winner);
    if converted_winner.pnl.get() != 0
        || converted_winner.capital.get() != DEPOSIT + EXPECTED_PNL
        || env.primary_portfolio(loser).capital.get() != DEPOSIT - EXPECTED_PNL
    {
        return Err(format!(
            "INV-024 conversion did not preserve winner/loser attribution: winner cap/pnl={}/{}, loser cap={}",
            converted_winner.capital.get(),
            converted_winner.pnl.get(),
            env.primary_portfolio(loser).capital.get(),
        ));
    }
    assert_custody(
        "INV-024 after conversion",
        &env,
        baseline_vault,
        baseline_vault,
    )?;
    assert_token_frame(
        "INV-024 conversion",
        &initial_token_frame,
        &env.all_token_account_data(),
        &[],
    )?;

    env.withdraw_primary(winner, DEPOSIT + EXPECTED_PNL)
        .map_err(|error| format!("INV-024 winner withdrawal: {error}"))?;
    env.withdraw_primary(loser, DEPOSIT - EXPECTED_PNL)
        .map_err(|error| format!("INV-024 loser withdrawal: {error}"))?;
    if env.token_amount(env.actors[winner].destination_token) != (DEPOSIT + EXPECTED_PNL) as u64
        || env.token_amount(env.actors[loser].destination_token) != (DEPOSIT - EXPECTED_PNL) as u64
    {
        return Err(format!(
            "INV-024 terminal SPL payouts were misattributed: winner={}, loser={}",
            env.token_amount(env.actors[winner].destination_token),
            env.token_amount(env.actors[loser].destination_token),
        ));
    }
    assert_custody(
        "INV-024 terminal",
        &env,
        UNRELATED_DEPOSIT * 3,
        UNRELATED_DEPOSIT * 3,
    )?;
    assert_token_frame(
        "INV-024 terminal",
        &initial_token_frame,
        &env.all_token_account_data(),
        &[
            env.vault,
            env.actors[ACCOUNT_A].destination_token,
            env.actors[ACCOUNT_B].destination_token,
        ],
    )?;

    let after_unrelated_portfolios = [2usize, 3usize]
        .into_iter()
        .map(|actor| env.primary_portfolio_data(actor))
        .collect::<Vec<_>>();
    if after_unrelated_portfolios != unrelated_portfolios {
        return Err("INV-024 route matrix mutated an unrelated portfolio".into());
    }
    let (_, terminal_group) = env.primary_market_state();
    if terminal_group.insurance != initial_insurance
        || terminal_group.insurance_domain_budget != initial_insurance_budget
        || terminal_group.insurance_domain_spent != initial_insurance_spent
        || terminal_group.insurance_credit_reservations != initial_insurance_reservations
        || terminal_group.source_claim_bound_total_num != 0
        || terminal_group.source_credit.iter().any(|source| {
            source.positive_claim_bound_num != 0
                || source.fresh_reserved_backing_num != 0
                || source.insurance_credit_reserved_num != 0
        })
        || env.token_supply_observed() != initial_supply
    {
        return Err(format!(
            "INV-024 terminal attribution left reserve, claim, or token-supply drift: claims={}, supply={}/{}",
            terminal_group.source_claim_bound_total_num,
            env.token_supply_observed(),
            initial_supply,
        ));
    }
    Ok(())
}

#[allow(dead_code)]
pub fn verify_exact_stock_reconciliation_lifecycle(seed: [u8; 32]) -> Result<(), String> {
    const LONG: usize = 0;
    const SHORT: usize = 1;
    const MARKET_CRANKER: usize = 4;
    const ASSET: u16 = 0;
    const SHORT_SOURCE_DOMAIN: u16 = 1;
    const START_PRICE: u64 = 1_000_000;
    const SETTLED_PRICE: u64 = 1_100_000;
    const DEPOSIT: u128 = 2_000_000;
    const BACKING: u128 = 125_003;
    const INSURANCE: u128 = 37_001;
    const EXPECTED_PNL: u128 = 100_000;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: START_PRICE,
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            max_abs_funding_e9_per_slot: 0,
            min_funding_lifetime_slots: 1,
            maintenance_fee_per_slot: 0,
            actor_deposits: [DEPOSIT, DEPOSIT, 1, 1, 1],
            actor_token_balances: [DEPOSIT as u64, DEPOSIT as u64, 1, 1, 1],
            ..MarketConfig::default()
        },
    );
    assert_public_stock_census("INV-025 initialized", &env)?;

    env.top_up_insurance_domain(0, INSURANCE)
        .map_err(|error| format!("INV-025 insurance top-up: {error}"))?;
    assert_public_stock_census("INV-025 after insurance top-up", &env)?;

    env.top_up_backing_bucket(SHORT_SOURCE_DOMAIN, BACKING, 100)
        .map_err(|error| format!("INV-025 backing top-up: {error}"))?;
    assert_public_stock_census("INV-025 after backing top-up", &env)?;

    execute_trade_route(
        &mut env,
        TradeRoute::NoCpi,
        LONG,
        SHORT,
        ASSET,
        POS_SCALE as i128,
        START_PRICE,
        0,
    )
    .map_err(|error| format!("INV-025 open: {error}"))?;
    assert_public_stock_census("INV-025 after open", &env)?;

    env.warp_to_slot(2);
    env.push_auth_mark(ASSET, 2, SETTLED_PRICE)
        .map_err(|error| format!("INV-025 mark publication: {error}"))?;
    crank_market_then_accounts_once(&mut env, MARKET_CRANKER, &[SHORT, LONG], 2, ASSET, 8)?;
    let winner = env.primary_portfolio(LONG);
    let loser = env.primary_portfolio(SHORT);
    if winner.pnl.get() != EXPECTED_PNL as i128
        || loser.pnl.get() != 0
        || loser.capital.get() != DEPOSIT - EXPECTED_PNL
    {
        return Err(format!(
            "INV-025 settlement fixture drifted: winner pnl={}, loser capital/pnl={}/{}",
            winner.pnl.get(),
            loser.capital.get(),
            loser.pnl.get(),
        ));
    }
    assert_public_stock_census("INV-025 after PnL settlement", &env)?;

    execute_trade_route(
        &mut env,
        TradeRoute::BatchCpi,
        LONG,
        SHORT,
        ASSET,
        -(POS_SCALE as i128),
        SETTLED_PRICE,
        0,
    )
    .map_err(|error| format!("INV-025 route-switched close: {error}"))?;
    assert_public_stock_census("INV-025 after route-switched close", &env)?;

    env.convert_released_pnl(LONG, EXPECTED_PNL)
        .map_err(|error| format!("INV-025 released-PnL conversion: {error}"))?;
    assert_public_stock_census("INV-025 after released-PnL conversion", &env)?;

    let (_, post_conversion) = env.primary_market_state();
    let remaining_backing_atoms = post_conversion.source_credit[SHORT_SOURCE_DOMAIN as usize]
        .fresh_reserved_backing_num
        / BOUND_SCALE;
    if remaining_backing_atoms != 0 {
        env.withdraw_backing_bucket(SHORT_SOURCE_DOMAIN, remaining_backing_atoms)
            .map_err(|error| format!("INV-025 backing withdrawal: {error}"))?;
        assert_public_stock_census("INV-025 after backing withdrawal", &env)?;
    }

    let long_capital = env.primary_portfolio(LONG).capital.get();
    let short_capital = env.primary_portfolio(SHORT).capital.get();
    env.withdraw_primary(LONG, long_capital)
        .map_err(|error| format!("INV-025 winner withdrawal: {error}"))?;
    assert_public_stock_census("INV-025 after winner withdrawal", &env)?;
    env.withdraw_primary(SHORT, short_capital)
        .map_err(|error| format!("INV-025 loser withdrawal: {error}"))?;
    assert_public_stock_census("INV-025 after loser withdrawal", &env)?;

    let (_, terminal) = env.primary_market_state();
    let terminal_fresh_backing_num = terminal
        .source_credit
        .iter()
        .try_fold(0u128, |sum, source| {
            sum.checked_add(source.fresh_reserved_backing_num)
        })
        .ok_or("INV-025 terminal backing sum overflow")?;
    if terminal.c_tot != 3
        || terminal.source_claim_bound_total_num != 0
        || terminal_fresh_backing_num != 0
        || terminal.insurance != INSURANCE
    {
        return Err(format!(
            "INV-025 terminal explicit stocks drifted: capital={}, claims={}, backing={}, insurance={}",
            terminal.c_tot,
            terminal.source_claim_bound_total_num,
            terminal_fresh_backing_num,
            terminal.insurance,
        ));
    }
    Ok(())
}

fn account_counterparty_lien_for_domain(account: &PortfolioAccountV16, domain: usize) -> u128 {
    account
        .source_domains
        .iter()
        .find(|source| source.is_occupied() && source.domain.get() as usize == domain)
        .map(|source| source.source_lien_counterparty_backing_num.get())
        .unwrap_or(0)
}

fn drain_resolved_actor_with_encumbrance_census(
    env: &mut V16Svm,
    actor: usize,
    label: &str,
) -> Result<u16, String> {
    let mut successful_calls = 0u16;
    for step in 0..512 {
        let market_before = env.market_data(false);
        let portfolio_before = env.primary_portfolio_data(actor);
        let destination_before = env.token_amount(env.actors[actor].destination_token);
        if env.close_resolved_primary(actor).is_ok() {
            successful_calls = successful_calls
                .checked_add(1)
                .ok_or_else(|| format!("{label}: resolved-close call count overflow"))?;
            assert_public_encumbrance_census(
                &format!("{label} actor {actor} close step {step}"),
                env,
            )?;
        }
        if env.claim_resolved_payout_topup_primary(actor).is_ok() {
            assert_public_encumbrance_census(
                &format!("{label} actor {actor} claim step {step}"),
                env,
            )?;
        }
        if env.market_data(false) == market_before
            && env.primary_portfolio_data(actor) == portfolio_before
            && env.token_amount(env.actors[actor].destination_token) == destination_before
        {
            return Ok(successful_calls);
        }
    }
    Err(format!(
        "{label}: resolved actor {actor} did not reach a fixed point in 512 calls"
    ))
}

#[allow(dead_code)]
pub fn verify_counterparty_encumbrance_route_matrix(mut seed: [u8; 32]) -> Result<(), String> {
    const WINNER: usize = 0;
    const COUNTERPARTY: usize = 1;
    const MARKET_CRANKER: usize = 4;
    const WINNING_ASSET: u16 = 0;
    const ADVERSE_ASSET: u16 = 1;
    const START_PRICE: u64 = 100;
    const WINNING_SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const ADVERSE_SIZE_Q: i128 = 10 * POS_SCALE as i128;
    const SAFE_INCREASE_Q: i128 = 2 * POS_SCALE as i128;
    const WINNER_DEPOSIT: u128 = 313;
    const BACKING_ATOMS: u128 = 150;

    for route in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ] {
        for winner_long in [false, true] {
            seed[0] ^= (route.index() as u8) << 3;
            seed[1] ^= u8::from(winner_long);
            let direction = if winner_long { 1i128 } else { -1i128 };
            let winning_mark = if winner_long { 105 } else { 95 };
            let adverse_mark = if winner_long { 95 } else { 105 };
            let source_domain = if winner_long { 1usize } else { 0usize };
            let label = format!("INV-026 {route:?} winner_long={winner_long}");
            let mut env = V16Svm::new(
                seed,
                MarketConfig {
                    initial_price: START_PRICE,
                    h_max: 4,
                    maintenance_margin_bps: 1_000,
                    initial_margin_bps: 1_000,
                    max_price_move_bps_per_slot: 500,
                    max_accrual_dt_slots: 1,
                    max_abs_funding_e9_per_slot: 0,
                    min_funding_lifetime_slots: 1,
                    maintenance_fee_per_slot: 0,
                    actor_deposits: [WINNER_DEPOSIT, 1_000, 1, 1, 1],
                    actor_token_balances: [WINNER_DEPOSIT as u64, 1_000, 1, 1, 1],
                    ..MarketConfig::default()
                },
            );
            let supply_before = env.token_supply_observed();
            env.top_up_backing_bucket(source_domain as u16, BACKING_ATOMS, 100)
                .map_err(|error| format!("{label} backing top-up: {error}"))?;
            assert_public_encumbrance_census(&format!("{label} after backing top-up"), &env)?;

            execute_trade_route(
                &mut env,
                route,
                WINNER,
                COUNTERPARTY,
                WINNING_ASSET,
                direction * WINNING_SIZE_Q,
                START_PRICE,
                0,
            )
            .map_err(|error| format!("{label} winning-leg open: {error}"))?;
            execute_trade_route(
                &mut env,
                route,
                WINNER,
                COUNTERPARTY,
                ADVERSE_ASSET,
                direction * ADVERSE_SIZE_Q,
                START_PRICE,
                0,
            )
            .map_err(|error| format!("{label} adverse-leg open: {error}"))?;
            assert_public_encumbrance_census(&format!("{label} after opens"), &env)?;

            env.warp_to_slot(2);
            env.push_auth_mark(WINNING_ASSET, 2, winning_mark)
                .map_err(|error| format!("{label} winning mark: {error}"))?;
            env.push_auth_mark(ADVERSE_ASSET, 2, adverse_mark)
                .map_err(|error| format!("{label} adverse mark: {error}"))?;
            let observations = [WINNING_ASSET, ADVERSE_ASSET]
                .into_iter()
                .map(|asset_index| CrankObservationHint {
                    asset_index,
                    oracle_accounts: env.primary_profile(asset_index as usize).oracle_leg_count,
                })
                .collect::<Vec<_>>();
            for actor in [MARKET_CRANKER, COUNTERPARTY, WINNER] {
                crank_adapter_steps_with_observations(&mut env, actor, 2, observations.clone(), 16)
                    .map_err(|error| format!("{label} settle actor {actor}: {error}"))?;
            }
            if env.primary_portfolio(WINNER).pnl.get() != 50 {
                return Err(format!(
                    "{label}: paired marks produced PnL {}, expected 50",
                    env.primary_portfolio(WINNER).pnl.get(),
                ));
            }
            assert_public_encumbrance_census(&format!("{label} after settlement"), &env)?;

            execute_trade_route(
                &mut env,
                route,
                WINNER,
                COUNTERPARTY,
                ADVERSE_ASSET,
                direction * SAFE_INCREASE_Q,
                adverse_mark,
                0,
            )
            .map_err(|error| format!("{label} source-backed risk increase: {error}"))?;
            let account_lien =
                account_counterparty_lien_for_domain(&env.primary_portfolio(WINNER), source_domain);
            let (_, liened_group) = env.primary_market_state();
            if account_lien == 0
                || account_lien
                    != liened_group.source_credit[source_domain].valid_liened_backing_num
            {
                return Err(format!(
                    "{label}: route did not create a real singly-attributed counterparty lien: account={account_lien}, market={}",
                    liened_group.source_credit[source_domain].valid_liened_backing_num,
                ));
            }
            assert_public_encumbrance_census(&format!("{label} after lien creation"), &env)?;

            env.resolve_market()
                .map_err(|error| format!("{label} resolve: {error}"))?;
            assert_public_encumbrance_census(&format!("{label} after resolve"), &env)?;
            let mut winner_calls = 0u16;
            let mut counterparty_calls = 0u16;
            let mut globally_fixed = false;
            for _ in 0..64 {
                let market_before = env.market_data(false);
                let winner_before = env.primary_portfolio_data(WINNER);
                let counterparty_before = env.primary_portfolio_data(COUNTERPARTY);
                let winner_destination_before =
                    env.token_amount(env.actors[WINNER].destination_token);
                let counterparty_destination_before =
                    env.token_amount(env.actors[COUNTERPARTY].destination_token);
                winner_calls = winner_calls
                    .checked_add(drain_resolved_actor_with_encumbrance_census(
                        &mut env, WINNER, &label,
                    )?)
                    .ok_or_else(|| format!("{label}: winner close count overflow"))?;
                counterparty_calls = counterparty_calls
                    .checked_add(drain_resolved_actor_with_encumbrance_census(
                        &mut env,
                        COUNTERPARTY,
                        &label,
                    )?)
                    .ok_or_else(|| format!("{label}: counterparty close count overflow"))?;
                if env.market_data(false) == market_before
                    && env.primary_portfolio_data(WINNER) == winner_before
                    && env.primary_portfolio_data(COUNTERPARTY) == counterparty_before
                    && env.token_amount(env.actors[WINNER].destination_token)
                        == winner_destination_before
                    && env.token_amount(env.actors[COUNTERPARTY].destination_token)
                        == counterparty_destination_before
                {
                    globally_fixed = true;
                    break;
                }
            }
            let (_, terminal) = env.primary_market_state();
            let terminal_source = terminal.source_credit[source_domain];
            let terminal_bucket = terminal.source_backing_buckets[source_domain];
            if !globally_fixed
                || winner_calls == 0
                || counterparty_calls == 0
                || account_counterparty_lien_for_domain(
                    &env.primary_portfolio(WINNER),
                    source_domain,
                ) != 0
                || terminal_source.valid_liened_backing_num != 0
                || terminal_source.impaired_liened_backing_num != 0
                || terminal_bucket.valid_liened_backing_num != 0
                || terminal_bucket.consumed_liened_backing_num == 0
                || terminal_source.provider_receivable_num
                    != terminal_bucket.consumed_liened_backing_num
                || env.token_supply_observed() != supply_before
            {
                return Err(format!(
                    "{label}: terminal lien lifecycle incomplete: calls={winner_calls}/{counterparty_calls}, source={terminal_source:?}, bucket={terminal_bucket:?}, supply={}/{}",
                    env.token_supply_observed(), supply_before,
                ));
            }
            assert_public_encumbrance_census(&format!("{label} terminal"), &env)?;
        }
    }
    Ok(())
}

fn zero_move_funding_world(seed: [u8; 32]) -> Result<V16Svm, String> {
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
            actor_deposits: [
                DEPOSIT,
                DEPOSIT,
                DEPOSIT,
                DEPOSIT,
                super::v16_svm::EXIT_MAKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    env.warp_to_slot(2);
    env.push_auth_mark(0, 2, TARGET)
        .map_err(|error| format!("stage zero-move funding mark: {error}"))?;
    Ok(env)
}

fn zero_move_observation(env: &V16Svm) -> Vec<CrankObservationHint> {
    vec![CrankObservationHint {
        asset_index: 0,
        oracle_accounts: env.primary_profile(0).oracle_leg_count,
    }]
}

fn prime_zero_move_funding(env: &mut V16Svm) -> Result<(), String> {
    for actor in [0, 1] {
        env.crank(actor, 2, zero_move_observation(env))
            .map_err(|error| format!("prime zero-move funding actor {actor}: {error}"))?;
    }
    let (_, group) = env.primary_market_state();
    if group.assets[0].effective_price != 2
        || group.assets[0].f_long_num != 0
        || group.assets[0].f_short_num != 0
    {
        return Err(format!(
            "zero-move prime unexpectedly accrued or moved price: price {}, F=({}, {})",
            group.assets[0].effective_price,
            group.assets[0].f_long_num,
            group.assets[0].f_short_num
        ));
    }
    env.warp_to_slot(3);
    Ok(())
}

fn run_trade_funding_order_world(
    seed: [u8; 32],
    route: TradeRoute,
    close_before_crank: bool,
) -> Result<(u64, u64, i128, i128), String> {
    const PRICE: u64 = 2;
    const Q: i128 = 100 * POS_SCALE as i128;

    let mut env = zero_move_funding_world(seed)?;
    execute_trade_route(&mut env, route, 0, 1, 0, -Q, PRICE, 0)
        .map_err(|error| format!("PR 271 {route:?} open: {error}"))?;
    prime_zero_move_funding(&mut env)?;

    if !close_before_crank {
        for actor in [0, 1] {
            env.crank(actor, 3, zero_move_observation(&env))
                .map_err(|error| format!("PR 271 control crank actor {actor}: {error}"))?;
        }
    }
    execute_trade_route(&mut env, route, 0, 1, 0, Q, PRICE, 0)
        .map_err(|error| format!("PR 271 {route:?} close: {error}"))?;
    if close_before_crank {
        for actor in [0, 1] {
            env.crank(actor, 3, zero_move_observation(&env))
                .map_err(|error| format!("PR 271 attack crank actor {actor}: {error}"))?;
        }
    }

    let (_, group) = env.primary_market_state();
    for actor in [0, 1] {
        let pnl = env.primary_portfolio(actor).pnl.get();
        if pnl > 0 {
            env.convert_released_pnl(actor, pnl as u128)
                .map_err(|error| format!("PR 271 convert actor {actor}: {error}"))?;
        }
        let capital = env.primary_portfolio(actor).capital.get();
        env.withdraw_primary(actor, capital)
            .map_err(|error| format!("PR 271 withdraw actor {actor}: {error}"))?;
    }
    Ok((
        env.token_amount(env.actors[0].destination_token),
        env.token_amount(env.actors[1].destination_token),
        group.assets[0].f_long_num,
        group.assets[0].f_short_num,
    ))
}

fn run_rebalance_funding_order_world(
    seed: [u8; 32],
    reduce_before_crank: bool,
) -> Result<(u64, u128, u128, u128), String> {
    const PRICE: u64 = 2;
    const Q: i128 = 100 * POS_SCALE as i128;

    let mut env = zero_move_funding_world(seed)?;
    env.trade_no_cpi(0, 1, 0, -Q, PRICE, 0)
        .map_err(|error| format!("PR 272 open: {error}"))?;
    prime_zero_move_funding(&mut env)?;

    if !reduce_before_crank {
        for actor in [0, 1] {
            env.crank(actor, 3, zero_move_observation(&env))
                .map_err(|error| format!("PR 272 control crank actor {actor}: {error}"))?;
        }
    }
    env.rebalance_reduce(0, 0, Q as u128)
        .map_err(|error| format!("PR 272 unilateral reduce: {error}"))?;
    env.crank(1, 3, zero_move_observation(&env))
        .map_err(|error| format!("PR 272 settle independent LP: {error}"))?;

    let attacker = env.primary_portfolio(0);
    let victim = env.primary_portfolio(1);
    let victim_claim = (victim.capital.get() as i128)
        .checked_add(victim.pnl.get())
        .ok_or("PR 272 victim claim overflow")?;
    let victim_claim = u128::try_from(victim_claim)
        .map_err(|_| "PR 272 victim claim became negative".to_string())?;
    let attacker_paid = attacker.funding_short_paid_atoms_total.get();
    let victim_received = victim.funding_long_received_atoms_total.get();
    let attacker_capital = attacker.capital.get();
    env.withdraw_primary(0, attacker_capital)
        .map_err(|error| format!("PR 272 withdraw attacker: {error}"))?;
    Ok((
        env.token_amount(env.actors[0].destination_token),
        victim_claim,
        attacker_paid,
        victim_received,
    ))
}

fn run_forfeit_funding_order_world(
    seed: [u8; 32],
    forfeit_before_crank: bool,
) -> Result<(u64, i128, u128, u128), String> {
    const PRICE: u64 = 2;
    const Q_ATTACKER: i128 = 5 * POS_SCALE as i128;
    const Q_WHALE: i128 = 95 * POS_SCALE as i128;

    let mut env = zero_move_funding_world(seed)?;
    env.trade_no_cpi(1, 2, 0, -Q_WHALE, PRICE, 0)
        .map_err(|error| format!("PR 273 open whale/reducer pair: {error}"))?;
    env.trade_no_cpi(0, 3, 0, -Q_ATTACKER, PRICE, 0)
        .map_err(|error| format!("PR 273 open attacker/LP pair: {error}"))?;
    prime_zero_move_funding(&mut env)?;

    if !forfeit_before_crank {
        for actor in 0..4 {
            env.crank(actor, 3, zero_move_observation(&env))
                .map_err(|error| format!("PR 273 control crank actor {actor}: {error}"))?;
        }
    }
    env.rebalance_reduce(2, 0, Q_WHALE as u128)
        .map_err(|error| format!("PR 273 force short side DrainOnly: {error}"))?;
    let (_, after_reduce) = env.primary_market_state();
    if after_reduce.assets[0].mode_short != SideModeV16::DrainOnly {
        return Err(format!(
            "PR 273 public reduction did not make short side DrainOnly: {:?}",
            after_reduce.assets[0].mode_short
        ));
    }
    env.forfeit_recovery_leg(0, 0, u128::from(u64::MAX))
        .map_err(|error| format!("PR 273 forfeit attacker recovery leg: {error}"))?;
    env.crank(3, 3, zero_move_observation(&env))
        .map_err(|error| format!("PR 273 settle independent LP: {error}"))?;

    let attacker = env.primary_portfolio(0);
    let victim = env.primary_portfolio(3);
    let attacker_paid = attacker.funding_short_paid_atoms_total.get();
    let victim_received = victim.funding_long_received_atoms_total.get();
    let victim_claim = (victim.capital.get() as i128)
        .checked_add(victim.pnl.get())
        .ok_or("PR 273 victim claim overflow")?;
    let attacker_capital = attacker.capital.get();
    env.withdraw_primary(0, attacker_capital)
        .map_err(|error| format!("PR 273 withdraw attacker: {error}"))?;
    Ok((
        env.token_amount(env.actors[0].destination_token),
        victim_claim,
        attacker_paid,
        victim_received,
    ))
}

#[derive(Clone, Copy, Debug)]
struct RoundedFundingWorld {
    omitted_rejected_nonprogress: bool,
    omitted_exact_rollback: bool,
    f_long_num: i128,
    f_short_num: i128,
    victim_payout: u64,
    counterparty_payout: u64,
}

fn run_rounded_funding_world(
    seed: [u8; 32],
    omit_selected_observation: bool,
) -> Result<RoundedFundingWorld, String> {
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
            actor_deposits: [
                DEPOSIT,
                DEPOSIT,
                DEPOSIT,
                DEPOSIT,
                super::v16_svm::EXIT_MAKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    env.trade_no_cpi(0, 1, 0, SIZE_Q, PRICE, 0)
        .map_err(|error| format!("open rounded-funding victim pair: {error}"))?;
    env.trade_no_cpi(2, 3, 1, POS_SCALE as i128, PRICE, 0)
        .map_err(|error| format!("open rounded-funding epoch pair: {error}"))?;

    env.warp_to_slot(2);
    env.push_auth_mark(0, 2, MARK)
        .map_err(|error| format!("stage rounded funding mark: {error}"))?;
    let asset0_observation = vec![CrankObservationHint {
        asset_index: 0,
        oracle_accounts: env.primary_profile(0).oracle_leg_count,
    }];
    env.crank(0, 2, asset0_observation.clone())
        .map_err(|error| format!("prime rounded funding checkpoint: {error}"))?;
    let (_, primed) = env.primary_market_state();
    if primed.assets[0].effective_price != PRICE
        || primed.assets[0].slot_last != 2
        || primed.assets[0].f_long_num != 0
    {
        return Err("rounded-funding prime state is not price-stationary and unfunded".into());
    }

    env.warp_to_slot(3);
    env.push_auth_mark(1, 3, MARK)
        .map_err(|error| format!("stage unrelated epoch mark: {error}"))?;
    env.crank(
        2,
        3,
        vec![CrankObservationHint {
            asset_index: 1,
            oracle_accounts: env.primary_profile(1).oracle_leg_count,
        }],
    )
    .map_err(|error| format!("advance unrelated market epoch: {error}"))?;
    let before_omission = tracked_economic_accounts(&env);
    let refresh = env.crank(
        0,
        3,
        if omit_selected_observation {
            Vec::new()
        } else {
            asset0_observation.clone()
        },
    );
    let (omitted_rejected_nonprogress, omitted_exact_rollback) = if omit_selected_observation {
        match refresh {
            Err(error) if error.contains("Custom(22)") => {
                let exact = tracked_economic_accounts(&env) == before_omission;
                env.crank(0, 3, asset0_observation)
                    .map_err(|error| format!("observed recovery after rejection: {error}"))?;
                (true, exact)
            }
            Err(error) => {
                return Err(format!(
                    "omitted rounded-funding observation returned an unexpected error: {error}"
                ))
            }
            Ok(_) => (false, false),
        }
    } else {
        refresh.map_err(|error| format!("observed rounded-funding refresh: {error}"))?;
        (false, false)
    };
    let (_, funded) = env.primary_market_state();

    env.crank(1, 3, Vec::new())
        .map_err(|error| format!("settle rounded-funding short: {error}"))?;
    for actor in [2usize, 3usize] {
        crank_adapter_steps(&mut env, actor, 3, 1, 8)
            .map_err(|error| format!("settle rounded-funding epoch actor {actor}: {error}"))?;
    }
    env.trade_no_cpi(0, 1, 0, -SIZE_Q, PRICE, 0)
        .map_err(|error| format!("close rounded-funding victim pair: {error}"))?;
    env.trade_no_cpi(2, 3, 1, -(POS_SCALE as i128), PRICE, 0)
        .map_err(|error| format!("close rounded-funding epoch pair: {error}"))?;
    if env.primary_portfolio(0).pnl.get() > 0 {
        env.convert_released_pnl(0, u128::MAX)
            .map_err(|error| format!("convert victim funding PnL: {error}"))?;
    }
    let victim_capital = env.primary_portfolio(0).capital.get();
    let short_capital = env.primary_portfolio(1).capital.get();
    env.withdraw_primary(0, victim_capital)
        .map_err(|error| format!("withdraw rounded-funding victim: {error}"))?;
    env.withdraw_primary(1, short_capital)
        .map_err(|error| format!("withdraw rounded-funding short: {error}"))?;
    Ok(RoundedFundingWorld {
        omitted_rejected_nonprogress,
        omitted_exact_rollback,
        f_long_num: funded.assets[0].f_long_num,
        f_short_num: funded.assets[0].f_short_num,
        victim_payout: env.token_amount(env.actors[0].destination_token),
        counterparty_payout: env.token_amount(env.actors[1].destination_token),
    })
}

fn crank_adapter_steps(
    env: &mut V16Svm,
    actor: usize,
    now_slot: u64,
    asset_index: u16,
    attempts: usize,
) -> Result<u64, String> {
    let observations = vec![CrankObservationHint {
        asset_index,
        oracle_accounts: env.primary_profile(asset_index as usize).oracle_leg_count,
    }];
    crank_adapter_steps_with_observations(env, actor, now_slot, observations, attempts)
}

fn crank_adapter_steps_with_observations(
    env: &mut V16Svm,
    actor: usize,
    now_slot: u64,
    observations: Vec<CrankObservationHint>,
    attempts: usize,
) -> Result<u64, String> {
    let observed_assets = observations
        .iter()
        .map(|observation| observation.asset_index)
        .collect::<Vec<_>>();
    let mut progressed = false;
    let mut max_cu = 0;
    for _ in 0..attempts {
        match env.crank(actor, now_slot, observations.clone()) {
            Ok(success) => {
                progressed = true;
                max_cu = max_cu.max(success.compute_units);
            }
            Err(error) if progressed && error.contains("Custom(22)") => break,
            Err(error) => {
                return Err(format!(
                    "actor {actor} assets {observed_assets:?} crank failed before progress: {error}"
                ));
            }
        }
    }
    if !progressed {
        return Err(format!(
            "actor {actor} assets {observed_assets:?} crank made no progress"
        ));
    }
    Ok(max_cu)
}

fn crank_market_then_accounts_once(
    env: &mut V16Svm,
    market_cranker: usize,
    accounts: &[usize],
    now_slot: u64,
    asset_index: u16,
    attempts: usize,
) -> Result<u64, String> {
    let observations = vec![CrankObservationHint {
        asset_index,
        oracle_accounts: env.primary_profile(asset_index as usize).oracle_leg_count,
    }];
    let mut max_cu = 0;
    for step in 0..attempts {
        if env.primary_market_state().1.assets[asset_index as usize].slot_last >= now_slot {
            break;
        }
        let success = env
            .crank(market_cranker, now_slot, observations.clone())
            .map_err(|error| {
                format!(
                    "market catch-up actor {market_cranker} asset {asset_index} step {step}: {error}"
                )
            })?;
        max_cu = max_cu.max(success.compute_units);
    }
    if env.primary_market_state().1.assets[asset_index as usize].slot_last < now_slot {
        return Err(format!(
            "market asset {asset_index} did not reach slot {now_slot} in {attempts} calls"
        ));
    }
    for &actor in accounts {
        match env.crank(actor, now_slot, Vec::new()) {
            Ok(success) => max_cu = max_cu.max(success.compute_units),
            Err(error) if error.contains("Custom(22)") => {}
            Err(error) => {
                return Err(format!(
                    "account selector actor {actor} asset {asset_index}: {error}"
                ))
            }
        }
    }
    Ok(max_cu)
}

#[derive(Clone, Copy, Debug)]
struct AssetGenerationMarkWorld {
    old_market_id: u64,
    new_market_id: u64,
    landed_mark: u64,
    victim_equity: u128,
    beneficiary_payout: u64,
    observed_token_supply: u128,
}

fn run_asset_generation_mark_world(
    seed: [u8; 32],
    path: AssetGenerationMarkPath,
    land_replay: bool,
) -> Result<AssetGenerationMarkWorld, String> {
    const ASSET: u16 = 1;
    const PRICE: u64 = 100;
    const STALE_ADVERSE_PRICE: u64 = 50;
    const SIZE_Q: i128 = 5_000 * POS_SCALE as i128;
    const DEPOSIT: u128 = 1_000_000;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [
                DEPOSIT,
                DEPOSIT,
                DEPOSIT,
                DEPOSIT,
                super::v16_svm::EXIT_MAKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    match path {
        AssetGenerationMarkPath::Auth => env
            .configure_auth_mark(false, ASSET, 1, PRICE)
            .map_err(|error| format!("{path:?} configure old AuthMark: {error}"))?,
        AssetGenerationMarkPath::Ewma => env
            .configure_ewma_mark(ASSET, 1, PRICE, 1, 0)
            .map_err(|error| format!("{path:?} configure old EwmaMark: {error}"))?,
    };
    let old_market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    let retained = match path {
        AssetGenerationMarkPath::Auth => env.build_retained_auth_mark(ASSET, STALE_ADVERSE_PRICE),
        AssetGenerationMarkPath::Ewma => env.build_retained_ewma_mark(ASSET, STALE_ADVERSE_PRICE),
    };
    env.update_market_init_fee_policy(1)
        .map_err(|error| format!("{path:?} configure permissionless init fee: {error}"))?;
    env.warp_to_slot(3);
    env.retire_asset(ASSET, 3)
        .map_err(|error| format!("{path:?} retire old asset generation: {error}"))?;
    env.warp_to_slot(4);
    env.activate_permissionless_asset(2, ASSET, 4, PRICE, 1)
        .map_err(|error| format!("{path:?} activate replacement generation: {error}"))?;
    match path {
        AssetGenerationMarkPath::Auth => env
            .configure_auth_mark(false, ASSET, 4, PRICE)
            .map_err(|error| format!("{path:?} configure replacement AuthMark: {error}"))?,
        AssetGenerationMarkPath::Ewma => env
            .configure_ewma_mark(ASSET, 4, PRICE, 1, 0)
            .map_err(|error| format!("{path:?} configure replacement EwmaMark: {error}"))?,
    };
    let new_market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    if new_market_id == old_market_id {
        return Err(format!(
            "{path:?} replacement reused market ID {old_market_id}"
        ));
    }

    env.trade_no_cpi(0, 1, ASSET, SIZE_Q, PRICE, 0)
        .map_err(|error| format!("{path:?} open fresh-generation victim exposure: {error}"))?;
    env.warp_to_slot(5);
    if land_replay {
        env.land_retained(retained)
            .map_err(|error| format!("{path:?} stale signed mark no longer lands: {error}"))?;
        if env.primary_profile(ASSET as usize).oracle_target_price_e6 >= PRICE {
            return Err(format!(
                "{path:?} stale signed mark landed without moving replacement target"
            ));
        }
        env.warp_to_slot(6);
        crank_adapter_steps(&mut env, 0, 6, ASSET, 4)
            .map_err(|error| format!("{path:?} settle stale mark against victim: {error}"))?;
        crank_adapter_steps(&mut env, 1, 6, ASSET, 4)
            .map_err(|error| format!("{path:?} settle stale mark against beneficiary: {error}"))?;
    }

    let landed_mark = env.primary_market_state().1.assets[ASSET as usize].effective_price;
    let victim = env.primary_portfolio(0);
    let victim_equity_i128 = (victim.capital.get() as i128)
        .checked_add(victim.pnl.get())
        .ok_or("victim equity overflow after stale mark")?;
    let victim_equity = u128::try_from(victim_equity_i128)
        .map_err(|_| format!("{path:?} stale mark made victim equity negative"))?;
    env.trade_no_cpi(1, EXIT_MAKER_INDEX, ASSET, SIZE_Q, landed_mark, 0)
        .map_err(|error| format!("{path:?} beneficiary public exit: {error}"))?;
    if env.primary_portfolio(1).pnl.get() > 0 {
        env.convert_released_pnl(1, u128::MAX)
            .map_err(|error| format!("{path:?} convert beneficiary PnL: {error}"))?;
    }
    let beneficiary_capital = env.primary_portfolio(1).capital.get();
    env.withdraw_primary(1, beneficiary_capital)
        .map_err(|error| format!("{path:?} withdraw beneficiary capital: {error}"))?;
    let beneficiary_payout = env.token_amount(env.actors[1].destination_token);
    if u128::from(beneficiary_payout) != beneficiary_capital {
        return Err(format!(
            "{path:?} beneficiary SPL payout {beneficiary_payout} did not equal capital {beneficiary_capital}"
        ));
    }
    if env.token_supply_observed() != env.initial_token_supply {
        return Err(format!(
            "{path:?} mark replay changed SPL supply: {}/{}",
            env.token_supply_observed(),
            env.initial_token_supply
        ));
    }
    Ok(AssetGenerationMarkWorld {
        old_market_id,
        new_market_id,
        landed_mark,
        victim_equity,
        beneficiary_payout,
        observed_token_supply: env.token_supply_observed(),
    })
}

#[derive(Clone, Copy, Debug)]
struct AssetGenerationConfigWorld {
    old_market_id: u64,
    new_market_id: u64,
    entry_price: u64,
    restored_mark: u64,
    victim_equity: u128,
    beneficiary_payout: u64,
    observed_token_supply: u128,
}

fn run_asset_generation_hybrid_config_world(
    seed: [u8; 32],
    land_replay: bool,
) -> Result<AssetGenerationConfigWorld, String> {
    const ASSET: u16 = 1;
    const HONEST_PRICE: u64 = 100;
    const REPLAY_PRICE: u64 = 105;
    const SIZE_Q: i128 = 20 * POS_SCALE as i128;
    const DEPOSIT: u128 = 1_000;
    const VICTIM: usize = 0;
    const BENEFICIARY: usize = 1;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            initial_price: HONEST_PRICE,
            maintenance_margin_bps: 1_000,
            initial_margin_bps: 1_000,
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [
                DEPOSIT,
                DEPOSIT,
                DEPOSIT,
                DEPOSIT,
                super::v16_svm::EXIT_MAKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    env.set_clock(1, 100);
    let feed = [0x22u8; 32];
    let replay_oracle = env.set_pyth_price(&feed, HONEST_PRICE as i64, -6, 0, 100);
    env.configure_hybrid_oracle(
        ASSET,
        1,
        100,
        0,
        [feed, [0; 32], [0; 32]],
        &[replay_oracle],
        1,
        0,
    )
    .map_err(|error| format!("Hybrid configure generation-A oracle: {error}"))?;
    let old_market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    let stale_config = env.build_retained_hybrid_oracle_config(
        ASSET,
        5,
        100,
        0,
        [feed, [0; 32], [0; 32]],
        &[replay_oracle],
        1,
        0,
    );

    env.update_market_init_fee_policy(1)
        .map_err(|error| format!("Hybrid configure permissionless init fee: {error}"))?;
    env.warp_to_slot(3);
    env.retire_asset(ASSET, 3)
        .map_err(|error| format!("Hybrid retire generation-A asset: {error}"))?;
    env.warp_to_slot(4);
    env.activate_permissionless_asset(2, ASSET, 4, HONEST_PRICE, 1)
        .map_err(|error| format!("Hybrid activate replacement generation: {error}"))?;
    let new_market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    if new_market_id == old_market_id {
        return Err(format!(
            "Hybrid replacement reused market ID {old_market_id}"
        ));
    }

    env.warp_to_slot(5);
    if land_replay {
        env.land_retained(stale_config)
            .map_err(|error| format!("Hybrid stale signed config no longer lands: {error}"))?;
    }
    let entry_price = env.primary_market_state().1.assets[ASSET as usize].effective_price;
    if entry_price != HONEST_PRICE {
        return Err(format!(
            "Hybrid entry mark {entry_price}, expected {HONEST_PRICE}"
        ));
    }

    env.trade_no_cpi(BENEFICIARY, VICTIM, ASSET, SIZE_Q, HONEST_PRICE, 0)
        .map_err(|error| format!("Hybrid open replacement user exposure: {error}"))?;
    env.set_clock(6, 101);
    let moved_oracle = env.set_pyth_price(&feed, REPLAY_PRICE as i64, -6, 0, 101);
    if land_replay {
        let observations = vec![CrankObservationHint {
            asset_index: ASSET,
            oracle_accounts: 1,
        }];
        for actor in [BENEFICIARY, VICTIM] {
            env.crank_with_oracles(actor, 6, observations.clone(), &[moved_oracle])
                .map_err(|error| format!("Hybrid settle actor {actor} at replay mark: {error}"))?;
        }
    } else {
        crank_adapter_steps(&mut env, BENEFICIARY, 6, ASSET, 2)
            .map_err(|error| format!("Hybrid settle control beneficiary: {error}"))?;
        crank_adapter_steps(&mut env, VICTIM, 6, ASSET, 2)
            .map_err(|error| format!("Hybrid settle control victim: {error}"))?;
    }
    let restored_mark = env.primary_market_state().1.assets[ASSET as usize].effective_price;
    let expected_settled_mark = if land_replay {
        REPLAY_PRICE
    } else {
        HONEST_PRICE
    };
    if restored_mark != expected_settled_mark {
        return Err(format!(
            "Hybrid settled mark {restored_mark}, expected {expected_settled_mark}"
        ));
    }

    env.trade_no_cpi(
        EXIT_MAKER_INDEX,
        BENEFICIARY,
        ASSET,
        SIZE_Q,
        restored_mark,
        0,
    )
    .map_err(|error| format!("Hybrid beneficiary public close: {error}"))?;
    env.trade_no_cpi(VICTIM, EXIT_MAKER_INDEX, ASSET, SIZE_Q, restored_mark, 0)
        .map_err(|error| format!("Hybrid victim public close: {error}"))?;
    if env.primary_portfolio(BENEFICIARY).pnl.get() > 0 {
        env.convert_released_pnl(BENEFICIARY, u128::MAX)
            .map_err(|error| format!("Hybrid convert beneficiary PnL: {error}"))?;
    }
    let beneficiary_capital = env.primary_portfolio(BENEFICIARY).capital.get();
    let victim_capital = env.primary_portfolio(VICTIM).capital.get();
    env.withdraw_primary(BENEFICIARY, beneficiary_capital)
        .map_err(|error| format!("Hybrid withdraw beneficiary capital: {error}"))?;
    env.withdraw_primary(VICTIM, victim_capital)
        .map_err(|error| format!("Hybrid withdraw victim capital: {error}"))?;
    let beneficiary_payout = env.token_amount(env.actors[BENEFICIARY].destination_token);
    let victim_payout = env.token_amount(env.actors[VICTIM].destination_token);
    if u128::from(beneficiary_payout) != beneficiary_capital
        || u128::from(victim_payout) != victim_capital
        || env.token_supply_observed() != env.initial_token_supply
    {
        return Err(format!(
            "Hybrid terminal payout mismatch: victim={victim_payout}/{victim_capital}, \
             beneficiary={beneficiary_payout}/{beneficiary_capital}, supply={}/{}",
            env.token_supply_observed(),
            env.initial_token_supply
        ));
    }

    Ok(AssetGenerationConfigWorld {
        old_market_id,
        new_market_id,
        entry_price,
        restored_mark,
        victim_equity: u128::from(victim_payout),
        beneficiary_payout,
        observed_token_supply: env.token_supply_observed(),
    })
}

fn run_asset_generation_config_world(
    seed: [u8; 32],
    path: AssetGenerationConfigPath,
    land_replay: bool,
) -> Result<AssetGenerationConfigWorld, String> {
    const ASSET: u16 = 1;
    const PRICE: u64 = 100;
    const STALE_ENTRY_PRICE: u64 = 50;
    const SIZE_Q: i128 = 5_000 * POS_SCALE as i128;
    const DEPOSIT: u128 = 1_000_000;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            min_funding_lifetime_slots: 1,
            actor_deposits: [
                DEPOSIT,
                DEPOSIT,
                DEPOSIT,
                DEPOSIT,
                super::v16_svm::EXIT_MAKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    match path {
        AssetGenerationConfigPath::Auth => env
            .configure_auth_mark(false, ASSET, 1, PRICE)
            .map_err(|error| format!("{path:?} configure old AuthMark: {error}"))?,
        AssetGenerationConfigPath::Ewma => env
            .configure_ewma_mark(ASSET, 1, PRICE, 1, 0)
            .map_err(|error| format!("{path:?} configure old EwmaMark: {error}"))?,
        AssetGenerationConfigPath::Hybrid => unreachable!("Hybrid uses its terminal world"),
    };
    let old_market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    let stale_config = match path {
        AssetGenerationConfigPath::Auth => env.build_retained_auth_config(ASSET, STALE_ENTRY_PRICE),
        AssetGenerationConfigPath::Ewma => {
            env.build_retained_ewma_config(ASSET, STALE_ENTRY_PRICE, 1, 0)
        }
        AssetGenerationConfigPath::Hybrid => unreachable!("Hybrid uses its terminal world"),
    };
    env.update_market_init_fee_policy(1)
        .map_err(|error| format!("{path:?} configure permissionless init fee: {error}"))?;
    env.warp_to_slot(3);
    env.retire_asset(ASSET, 3)
        .map_err(|error| format!("{path:?} retire old asset generation: {error}"))?;
    env.warp_to_slot(4);
    env.activate_permissionless_asset(2, ASSET, 4, PRICE, 1)
        .map_err(|error| format!("{path:?} activate replacement generation: {error}"))?;
    let new_market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    if new_market_id == old_market_id {
        return Err(format!(
            "{path:?} replacement reused market ID {old_market_id}"
        ));
    }
    let current_generation_trade = env.build_retained_no_cpi_trade(0, 1, ASSET, SIZE_Q, PRICE);

    env.warp_to_slot(5);
    if land_replay {
        env.land_retained(stale_config)
            .map_err(|error| format!("{path:?} stale signed config no longer lands: {error}"))?;
    } else {
        match path {
            AssetGenerationConfigPath::Auth => env
                .configure_auth_mark(false, ASSET, 5, PRICE)
                .map_err(|error| format!("{path:?} configure current AuthMark: {error}"))?,
            AssetGenerationConfigPath::Ewma => env
                .configure_ewma_mark(ASSET, 5, PRICE, 1, 0)
                .map_err(|error| format!("{path:?} configure current EwmaMark: {error}"))?,
            AssetGenerationConfigPath::Hybrid => unreachable!("Hybrid uses its terminal world"),
        };
    }
    let entry_price = env.primary_market_state().1.assets[ASSET as usize].effective_price;
    if land_replay && entry_price != STALE_ENTRY_PRICE {
        return Err(format!(
            "{path:?} stale config set entry anchor {entry_price}, expected {STALE_ENTRY_PRICE}"
        ));
    }
    env.land_retained(current_generation_trade)
        .map_err(|error| format!("{path:?} retained replacement-generation trade: {error}"))?;

    env.warp_to_slot(6);
    match path {
        AssetGenerationConfigPath::Auth => env
            .push_auth_mark(ASSET, 6, PRICE)
            .map_err(|error| format!("{path:?} restore honest AuthMark: {error}"))?,
        AssetGenerationConfigPath::Ewma => env
            .push_ewma_mark(ASSET, 6, PRICE)
            .map_err(|error| format!("{path:?} restore honest EwmaMark: {error}"))?,
        AssetGenerationConfigPath::Hybrid => unreachable!("Hybrid uses its terminal world"),
    };
    crank_adapter_steps(&mut env, 0, 6, ASSET, 4)
        .map_err(|error| format!("{path:?} settle beneficiary after restoration: {error}"))?;
    crank_adapter_steps(&mut env, 1, 6, ASSET, 4)
        .map_err(|error| format!("{path:?} settle victim after restoration: {error}"))?;
    let restored_mark = env.primary_market_state().1.assets[ASSET as usize].effective_price;
    let victim = env.primary_portfolio(1);
    let victim_equity_i128 = (victim.capital.get() as i128)
        .checked_add(victim.pnl.get())
        .ok_or("victim equity overflow after stale config")?;
    let victim_equity = u128::try_from(victim_equity_i128)
        .map_err(|_| format!("{path:?} stale config made victim equity negative"))?;

    env.trade_no_cpi(EXIT_MAKER_INDEX, 0, ASSET, SIZE_Q, restored_mark, 0)
        .map_err(|error| format!("{path:?} beneficiary public exit: {error}"))?;
    if env.primary_portfolio(0).pnl.get() > 0 {
        env.convert_released_pnl(0, u128::MAX)
            .map_err(|error| format!("{path:?} convert beneficiary PnL: {error}"))?;
    }
    let beneficiary_capital = env.primary_portfolio(0).capital.get();
    env.withdraw_primary(0, beneficiary_capital)
        .map_err(|error| format!("{path:?} withdraw beneficiary capital: {error}"))?;
    let beneficiary_payout = env.token_amount(env.actors[0].destination_token);
    if u128::from(beneficiary_payout) != beneficiary_capital {
        return Err(format!(
            "{path:?} beneficiary SPL payout {beneficiary_payout} did not equal capital {beneficiary_capital}"
        ));
    }
    if env.token_supply_observed() != env.initial_token_supply {
        return Err(format!(
            "{path:?} config replay changed SPL supply: {}/{}",
            env.token_supply_observed(),
            env.initial_token_supply
        ));
    }
    Ok(AssetGenerationConfigWorld {
        old_market_id,
        new_market_id,
        entry_price,
        restored_mark,
        victim_equity,
        beneficiary_payout,
        observed_token_supply: env.token_supply_observed(),
    })
}

fn run_asset_generation_trade_world(
    seed: [u8; 32],
    route: TradeRoute,
    land_replay: bool,
) -> Result<(u64, u64, u64, u64), String> {
    const ASSET: u16 = 1;
    const OLD_PRICE: u64 = 100;
    const NEW_PRICE: u64 = 250;
    const ADVERSE_PRICE: u64 = 200;
    const SIZE_Q: i128 = 1_000 * POS_SCALE as i128;

    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            actor_deposits: [
                1_000_000,
                1_000_000,
                super::v16_svm::USER_DEPOSIT,
                super::v16_svm::USER_DEPOSIT,
                super::v16_svm::EXIT_MAKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    env.configure_auth_mark(false, ASSET, 1, OLD_PRICE)
        .map_err(|error| format!("{route:?} configure old generation: {error}"))?;
    env.update_market_init_fee_policy(1)
        .map_err(|error| format!("{route:?} configure permissionless init fee: {error}"))?;
    let old_market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    let retained = build_retained_trade(&mut env, route, 0, 1, ASSET, SIZE_Q, OLD_PRICE, 0);

    env.warp_to_slot(3);
    env.retire_asset(ASSET, 3)
        .map_err(|error| format!("{route:?} retire old asset generation: {error}"))?;
    env.warp_to_slot(4);
    env.activate_permissionless_asset(2, ASSET, 4, NEW_PRICE, 1)
        .map_err(|error| format!("{route:?} activate replacement generation: {error}"))?;
    env.configure_auth_mark(false, ASSET, 4, NEW_PRICE)
        .map_err(|error| format!("{route:?} configure replacement AuthMark: {error}"))?;
    let new_market_id = env.primary_market_state().1.assets[ASSET as usize].market_id;
    if new_market_id == old_market_id {
        return Err(format!(
            "{route:?} asset slot reuse did not change market ID {old_market_id}"
        ));
    }
    if land_replay {
        env.land_retained(retained).map_err(|error| {
            format!("{route:?} stale-generation trade no longer lands: {error}")
        })?;
        let victim_position = observed_positions(&env.primary_portfolio(0))?[ASSET as usize];
        if victim_position != SIZE_Q {
            return Err(format!(
                "{route:?} stale trade created position {victim_position}, expected {SIZE_Q}"
            ));
        }
    }

    env.warp_to_slot(5);
    env.push_auth_mark(ASSET, 5, ADVERSE_PRICE)
        .map_err(|error| format!("{route:?} push replacement adverse mark: {error}"))?;
    let oracle_accounts = env.primary_profile(ASSET as usize).oracle_leg_count;
    let observation = || {
        vec![CrankObservationHint {
            asset_index: ASSET,
            oracle_accounts,
        }]
    };
    env.crank(1, 5, observation())
        .map_err(|error| format!("{route:?} refresh replacement attacker: {error}"))?;
    env.crank(0, 5, observation())
        .map_err(|error| format!("{route:?} refresh replacement victim: {error}"))?;
    if land_replay {
        let victim_position = observed_positions(&env.primary_portfolio(0))?[ASSET as usize];
        env.trade_no_cpi(0, 1, ASSET, -victim_position, ADVERSE_PRICE, 0)
            .map_err(|error| format!("{route:?} close stale-generation exposure: {error}"))?;
        env.convert_released_pnl(1, u128::MAX)
            .map_err(|error| format!("{route:?} convert stale-generation profit: {error}"))?;
    }
    let victim_capital = env.primary_portfolio(0).capital.get();
    let attacker_capital = env.primary_portfolio(1).capital.get();
    env.withdraw_primary(0, victim_capital)
        .map_err(|error| format!("{route:?} replacement victim withdrawal: {error}"))?;
    env.withdraw_primary(1, attacker_capital)
        .map_err(|error| format!("{route:?} replacement attacker withdrawal: {error}"))?;
    Ok((
        env.token_amount(env.actors[0].destination_token),
        env.token_amount(env.actors[1].destination_token),
        old_market_id,
        new_market_id,
    ))
}

fn run_trade_retry_world(
    seed: [u8; 32],
    route: TradeRoute,
    land_retry: bool,
) -> Result<(u64, u64, u128), String> {
    let mut env = V16Svm::new(
        seed,
        MarketConfig {
            max_price_move_bps_per_slot: 1_000,
            max_accrual_dt_slots: 1,
            actor_deposits: [
                4_000_000,
                4_000_000,
                super::v16_svm::USER_DEPOSIT,
                super::v16_svm::USER_DEPOSIT,
                super::v16_svm::EXIT_MAKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    let size_q = -(POS_SCALE as i128);
    let original = build_retained_trade(
        &mut env,
        route,
        0,
        1,
        0,
        size_q,
        super::v16_svm::INITIAL_PRICE,
        super::v16_svm::INITIAL_PRICE,
    );
    let retry = build_retained_trade(
        &mut env,
        route,
        0,
        1,
        0,
        size_q,
        super::v16_svm::INITIAL_PRICE,
        super::v16_svm::INITIAL_PRICE,
    );
    env.land_retained(original)
        .map_err(|error| format!("{route:?} original intent: {error}"))?;
    if land_retry {
        env.land_retained(retry)
            .map_err(|error| format!("{route:?} retry variant no longer lands: {error}"))?;
    }

    let expected_abs_q = (if land_retry { 2 } else { 1 }) * POS_SCALE;
    let victim_position = observed_positions(&env.primary_portfolio(0))?[0];
    if victim_position >= 0 || victim_position.unsigned_abs() != expected_abs_q {
        return Err(format!(
            "{route:?} retry setup produced unexpected victim position {victim_position}, expected -{expected_abs_q}"
        ));
    }

    env.warp_to_slot(2);
    let close_price = super::v16_svm::INITIAL_PRICE
        .checked_mul(11)
        .and_then(|value| value.checked_div(10))
        .ok_or("trade retry close-price overflow")?;
    env.push_auth_mark(0, 2, close_price)
        .map_err(|error| format!("{route:?} push adverse mark: {error}"))?;
    let oracle_accounts = env.primary_profile(0).oracle_leg_count;
    let observation = || {
        vec![CrankObservationHint {
            asset_index: 0,
            oracle_accounts,
        }]
    };
    env.crank(1, 2, observation())
        .map_err(|error| format!("{route:?} refresh attacker: {error}"))?;
    env.crank(0, 2, observation())
        .map_err(|error| format!("{route:?} refresh victim: {error}"))?;
    let victim_position = observed_positions(&env.primary_portfolio(0))?[0];
    env.trade_no_cpi(0, 1, 0, -victim_position, close_price, 0)
        .map_err(|error| format!("{route:?} close replayed exposure: {error}"))?;
    if observed_positions(&env.primary_portfolio(0))?[0] != 0
        || observed_positions(&env.primary_portfolio(1))?[0] != 0
    {
        return Err(format!("{route:?} replay world did not close flat"));
    }

    env.convert_released_pnl(1, u128::MAX)
        .map_err(|error| format!("{route:?} convert attacker released PnL: {error}"))?;
    let victim_capital = env.primary_portfolio(0).capital.get();
    let attacker_capital = env.primary_portfolio(1).capital.get();
    env.withdraw_primary(0, victim_capital)
        .map_err(|error| format!("{route:?} victim withdrawal: {error}"))?;
    env.withdraw_primary(1, attacker_capital)
        .map_err(|error| format!("{route:?} attacker withdrawal: {error}"))?;
    let victim_payout = env.token_amount(env.actors[0].destination_token);
    let attacker_payout = env.token_amount(env.actors[1].destination_token);
    Ok((
        victim_payout,
        attacker_payout,
        u128::from(victim_payout) + u128::from(attacker_payout),
    ))
}

fn build_retained_trade(
    env: &mut V16Svm,
    route: TradeRoute,
    taker: usize,
    maker: usize,
    asset_index: u16,
    size_q: i128,
    exec_price: u64,
    limit_price: u64,
) -> Transaction {
    match route {
        TradeRoute::NoCpi => {
            env.build_retained_no_cpi_trade(taker, maker, asset_index, size_q, exec_price)
        }
        TradeRoute::Cpi => {
            env.build_retained_cpi_trade(taker, maker, asset_index, size_q, limit_price)
        }
        TradeRoute::BatchNoCpi => {
            env.build_retained_batch_no_cpi_trade(taker, maker, asset_index, size_q, exec_price)
        }
        TradeRoute::BatchCpi => {
            env.build_retained_batch_cpi_trade(taker, maker, asset_index, size_q, limit_price)
        }
    }
}

fn build_omitted_rescue_world(seed: [u8; 32]) -> Result<(V16Svm, u128, u128), String> {
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
                super::v16_svm::USER_DEPOSIT,
                super::v16_svm::USER_DEPOSIT,
                super::v16_svm::EXIT_MAKER_DEPOSIT,
            ],
            ..MarketConfig::default()
        },
    );
    env.configure_auth_mark(false, 0, 1, RESCUE_PRICE)
        .map_err(|error| format!("configure rescue AuthMark: {error}"))?;
    env.configure_auth_mark(false, 1, 1, ADVERSE_PRICE)
        .map_err(|error| format!("configure adverse AuthMark: {error}"))?;
    env.top_up_backing_bucket(1, 200_000, 10)
        .map_err(|error| format!("top up rescue backing: {error}"))?;
    env.trade_no_cpi(0, 1, 1, ADVERSE_SIZE_Q, ADVERSE_PRICE, 0)
        .map_err(|error| format!("open adverse first leg: {error}"))?;
    env.trade_no_cpi(0, 1, 0, RESCUE_SIZE_Q, RESCUE_PRICE, 0)
        .map_err(|error| format!("open rescue later leg: {error}"))?;
    let opened = env.primary_portfolio(0);
    let active_assets: Vec<_> = decoded_legs(&opened)
        .into_iter()
        .filter(|leg| leg.active)
        .map(|leg| leg.asset_index)
        .collect();
    if active_assets != [1, 0] {
        return Err(format!(
            "rescue setup did not preserve first/later leg ordering: {active_assets:?}"
        ));
    }
    let position_before_q = position_abs_for_asset(&opened, 1)?;

    env.warp_to_slot(2);
    env.push_auth_mark(0, 2, RESCUE_MARK)
        .map_err(|error| format!("stage rounded rescue mark: {error}"))?;
    let rescue_oracle_accounts = env.primary_profile(0).oracle_leg_count;
    env.crank(
        2,
        2,
        vec![CrankObservationHint {
            asset_index: 0,
            oracle_accounts: rescue_oracle_accounts,
        }],
    )
    .map_err(|error| format!("prime rounded rescue mark: {error}"))?;
    let (_, primed) = env.primary_market_state();
    if primed.assets[0].effective_price != RESCUE_PRICE
        || primed.assets[0].slot_last != 2
        || primed.assets[0].f_long_num != 0
    {
        return Err("rescue leg did not reach the rounded, zero-funding prime state".into());
    }

    env.warp_to_slot(3);
    env.push_auth_mark(1, 3, ADVERSE_TARGET)
        .map_err(|error| format!("stage adverse mark: {error}"))?;
    let adverse_oracle_accounts = env.primary_profile(1).oracle_leg_count;
    env.crank(
        2,
        3,
        vec![CrankObservationHint {
            asset_index: 1,
            oracle_accounts: adverse_oracle_accounts,
        }],
    )
    .map_err(|error| format!("commit adverse mark: {error}"))?;
    let (_, stale_group) = env.primary_market_state();
    if stale_group.assets[0].effective_price != RESCUE_PRICE || stale_group.assets[0].slot_last != 2
    {
        return Err("adverse prefix accidentally consumed the later rescue leg".into());
    }
    let insurance_before = stale_group.insurance;
    Ok((env, position_before_q, insurance_before))
}

fn position_abs_for_asset(
    account: &percolator_prog::state::PortfolioAccountV16,
    asset_index: usize,
) -> Result<u128, String> {
    decoded_legs(account)
        .into_iter()
        .find(|leg| leg.active && leg.asset_index as usize == asset_index)
        .map(|leg| leg.basis_pos_q.unsigned_abs())
        .ok_or_else(|| format!("portfolio has no active asset {asset_index}"))
}

fn position_for_asset(
    account: &percolator_prog::state::PortfolioAccountV16,
    asset_index: usize,
) -> Result<i128, String> {
    decoded_legs(account)
        .into_iter()
        .find(|leg| leg.active && leg.asset_index as usize == asset_index)
        .map(|leg| leg.basis_pos_q)
        .ok_or_else(|| format!("portfolio has no active asset {asset_index}"))
}

#[allow(dead_code)]
pub fn post_expiry_backing_case_strategy(
) -> impl Strategy<Value = ([u8; 32], PostExpiryBackingCase)> {
    (
        any::<[u8; 32]>(),
        Just(5_000u16),
        prop::sample::select(vec![2u8, 3, 5, 8]),
        Just(500u16),
        Just(20u8),
    )
        .prop_map(
            |(seed, fee_bps, expiry_offset, mark_move_bps, increase_divisor)| {
                (
                    seed,
                    PostExpiryBackingCase {
                        fee_bps,
                        expiry_offset,
                        mark_move_bps,
                        increase_divisor,
                    },
                )
            },
        )
}

#[allow(dead_code)]
pub fn omitted_rescue_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn trade_retry_replay_strategy() -> impl Strategy<Value = ([u8; 32], TradeRoute)> {
    (
        any::<[u8; 32]>(),
        prop::sample::select(vec![
            TradeRoute::NoCpi,
            TradeRoute::Cpi,
            TradeRoute::BatchNoCpi,
            TradeRoute::BatchCpi,
        ]),
    )
}

#[allow(dead_code)]
pub fn asset_generation_replay_strategy() -> impl Strategy<Value = ([u8; 32], TradeRoute)> {
    (
        any::<[u8; 32]>(),
        prop::sample::select(vec![
            TradeRoute::NoCpi,
            TradeRoute::Cpi,
            TradeRoute::BatchNoCpi,
            TradeRoute::BatchCpi,
        ]),
    )
}

#[allow(dead_code)]
pub fn asset_generation_mark_replay_strategy(
) -> impl Strategy<Value = ([u8; 32], AssetGenerationMarkPath)> {
    (
        any::<[u8; 32]>(),
        prop::sample::select(vec![
            AssetGenerationMarkPath::Auth,
            AssetGenerationMarkPath::Ewma,
        ]),
    )
}

#[allow(dead_code)]
pub fn asset_generation_config_replay_strategy(
) -> impl Strategy<Value = ([u8; 32], AssetGenerationConfigPath)> {
    (
        any::<[u8; 32]>(),
        prop::sample::select(vec![
            AssetGenerationConfigPath::Auth,
            AssetGenerationConfigPath::Ewma,
            AssetGenerationConfigPath::Hybrid,
        ]),
    )
}

#[allow(dead_code)]
pub fn cpi_caller_fee_strategy() -> impl Strategy<Value = ([u8; 32], TradeRoute)> {
    (
        any::<[u8; 32]>(),
        prop::sample::select(vec![TradeRoute::Cpi, TradeRoute::BatchCpi]),
    )
}

#[allow(dead_code)]
pub fn cpi_base_fee_consent_strategy() -> impl Strategy<Value = ([u8; 32], TradeRoute)> {
    (
        any::<[u8; 32]>(),
        prop::sample::select(vec![TradeRoute::Cpi, TradeRoute::BatchCpi]),
    )
}

#[allow(dead_code)]
pub fn cpi_backing_fee_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn composite_rounding_strategy() -> impl Strategy<Value = ([u8; 32], CompositeRoundingCase)> {
    (
        any::<[u8; 32]>(),
        prop::sample::select(vec![
            CompositeRoundingCase::Pr329LargeMove,
            CompositeRoundingCase::Pr381MicroMove,
        ]),
    )
}

#[allow(dead_code)]
pub fn composite_time_skew_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn target_staging_strategy() -> impl Strategy<Value = ([u8; 32], TargetStagingCase)> {
    (
        any::<[u8; 32]>(),
        prop::sample::select(vec![
            TargetStagingCase::AuthMarkPush,
            TargetStagingCase::EwmaMarkPush,
            TargetStagingCase::EwmaSingleTrade,
            TargetStagingCase::EwmaBatchTrade,
        ]),
    )
}

#[allow(dead_code)]
pub fn pending_mark_fee_reward_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn fractional_cap_settlement_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn prospective_funding_rewrite_strategy() -> impl Strategy<Value = ([u8; 32], TradeRoute)> {
    (
        any::<[u8; 32]>(),
        prop::sample::select(vec![
            TradeRoute::NoCpi,
            TradeRoute::Cpi,
            TradeRoute::BatchNoCpi,
            TradeRoute::BatchCpi,
        ]),
    )
}

#[allow(dead_code)]
pub fn resolve_before_committed_accrual_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn bilateral_fee_support_strategy(
) -> impl Strategy<Value = ([u8; 32], BilateralFeeMode, TradeRoute)> {
    (
        any::<[u8; 32]>(),
        prop::sample::select(vec![
            BilateralFeeMode::Ewma,
            BilateralFeeMode::HybridAfterHours,
        ]),
        prop::sample::select(vec![TradeRoute::Cpi, TradeRoute::BatchCpi]),
    )
}

#[allow(dead_code)]
pub fn delayed_asset_authority_revival_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn collateral_top_up_generation_replay_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn backing_top_up_generation_replay_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn insurance_withdrawal_generation_replay_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn insurance_top_up_retry_replay_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn activation_retry_replay_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn activation_fee_consent_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn bilateral_base_fee_consent_strategy() -> impl Strategy<Value = ([u8; 32], TradeRoute)> {
    (
        any::<[u8; 32]>(),
        prop::sample::select(vec![TradeRoute::NoCpi, TradeRoute::BatchNoCpi]),
    )
}

#[allow(dead_code)]
pub fn maintenance_policy_generation_replay_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn liquidation_policy_generation_replay_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn delayed_maintenance_policy_replay_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn delayed_liquidation_policy_replay_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn delayed_trade_fee_policy_replay_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn delayed_fee_redirect_policy_replay_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn delayed_backing_fee_policy_replay_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn delayed_oracle_intent_replay_strategy(
) -> impl Strategy<Value = ([u8; 32], DelayedOracleIntentPath)> {
    (
        any::<[u8; 32]>(),
        prop::sample::select(vec![
            DelayedOracleIntentPath::PushAuth,
            DelayedOracleIntentPath::ConfigureAuth,
        ]),
    )
}

#[allow(dead_code)]
pub fn backing_fee_consent_replay_strategy(
) -> impl Strategy<Value = ([u8; 32], BackingFeeConsentOrder)> {
    (
        any::<[u8; 32]>(),
        prop::sample::select(vec![
            BackingFeeConsentOrder::FundedThenPolicy,
            BackingFeeConsentOrder::PolicyThenTopUp,
        ]),
    )
}

#[allow(dead_code)]
pub fn authority_handoff_aba_replay_strategy(
) -> impl Strategy<Value = ([u8; 32], AuthorityHandoffAbaPath)> {
    (
        any::<[u8; 32]>(),
        prop::sample::select(vec![
            AuthorityHandoffAbaPath::Market,
            AuthorityHandoffAbaPath::AssetInsuranceOperator,
        ]),
    )
}

#[allow(dead_code)]
pub fn delayed_resolve_policy_replay_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn resolve_authority_incarnation_replay_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn portfolio_close_incarnation_replay_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn matcher_grant_portfolio_incarnation_replay_seed_strategy() -> impl Strategy<Value = [u8; 32]>
{
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn matcher_grant_market_generation_replay_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn trade_fee_market_generation_replay_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn trade_portfolio_incarnation_replay_strategy(
) -> impl Strategy<Value = ([u8; 32], TradeRoute, PortfolioIncarnationTradeSide)> {
    (
        any::<[u8; 32]>(),
        prop::sample::select(vec![
            TradeRoute::NoCpi,
            TradeRoute::Cpi,
            TradeRoute::BatchNoCpi,
            TradeRoute::BatchCpi,
        ]),
        prop::sample::select(vec![
            PortfolioIncarnationTradeSide::AccountA,
            PortfolioIncarnationTradeSide::AccountB,
        ]),
    )
}

#[allow(dead_code)]
pub fn convert_portfolio_incarnation_replay_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn forfeit_portfolio_incarnation_replay_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn forfeit_market_generation_replay_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn fee_redirect_generation_replay_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn backing_fee_generation_replay_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn backing_top_up_retry_replay_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn withdrawal_retry_liquidation_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn deposit_retry_replay_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn portfolio_incarnation_withdrawal_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn portfolio_incarnation_deposit_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn market_incarnation_deposit_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn resolve_generation_replay_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn shutdown_generation_replay_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn rounded_funding_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn pending_ewma_inheritance_strategy() -> impl Strategy<Value = ([u8; 32], TradeRoute)> {
    (
        any::<[u8; 32]>(),
        prop::sample::select(vec![
            TradeRoute::NoCpi,
            TradeRoute::Cpi,
            TradeRoute::BatchNoCpi,
            TradeRoute::BatchCpi,
        ]),
    )
}

#[allow(dead_code)]
pub fn pending_ewma_target_override_strategy() -> impl Strategy<Value = ([u8; 32], TradeRoute)> {
    (
        any::<[u8; 32]>(),
        prop::sample::select(vec![
            TradeRoute::NoCpi,
            TradeRoute::Cpi,
            TradeRoute::BatchNoCpi,
            TradeRoute::BatchCpi,
        ]),
    )
}

#[allow(dead_code)]
pub fn terminal_dust_payout_erasure_strategy() -> impl Strategy<Value = ([u8; 32], TradeRoute)> {
    (
        any::<[u8; 32]>(),
        prop::sample::select(vec![TradeRoute::NoCpi, TradeRoute::BatchNoCpi]),
    )
}

#[allow(dead_code)]
pub fn cross_margin_insurance_drain_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn reclaimable_ewma_fee_strategy() -> impl Strategy<Value = ([u8; 32], TradeRoute)> {
    (
        any::<[u8; 32]>(),
        prop::sample::select(vec![
            TradeRoute::NoCpi,
            TradeRoute::Cpi,
            TradeRoute::BatchNoCpi,
            TradeRoute::BatchCpi,
        ]),
    )
}

#[allow(dead_code)]
pub fn trade_funding_erasure_strategy() -> impl Strategy<Value = ([u8; 32], TradeRoute)> {
    (
        any::<[u8; 32]>(),
        prop::sample::select(vec![TradeRoute::Cpi, TradeRoute::BatchCpi]),
    )
}

#[allow(dead_code)]
pub fn rebalance_funding_erasure_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn forfeit_funding_erasure_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn trade_driven_liquidation_reward_strategy(
) -> impl Strategy<Value = ([u8; 32], TradeDrivenLiquidationMode, TradeRoute)> {
    (
        any::<[u8; 32]>(),
        prop::sample::select(vec![
            TradeDrivenLiquidationMode::Ewma,
            TradeDrivenLiquidationMode::HybridAfterHours,
        ]),
        prop::sample::select(vec![TradeRoute::NoCpi, TradeRoute::BatchNoCpi]),
    )
}

#[allow(dead_code)]
pub fn cross_domain_backing_seed_strategy() -> impl Strategy<Value = [u8; 32]> {
    any::<[u8; 32]>()
}

#[allow(dead_code)]
pub fn scenario_strategy(max_actions: usize) -> impl Strategy<Value = Scenario> {
    (
        any::<[u8; 32]>(),
        small_market_config_strategy(),
        prop::collection::vec(action_strategy(), 1..=max_actions),
    )
        .prop_map(|(seed, config, actions)| Scenario {
            seed,
            config,
            actions,
        })
}

fn small_market_config_strategy() -> impl Strategy<Value = SmallMarketConfig> {
    prop::sample::select(vec![
        SmallMarketConfig::default(),
        SmallMarketConfig {
            max_price_move_bps_per_slot: 1,
            max_accrual_dt_slots: 4,
            max_abs_funding_e9_per_slot: 10_000,
            maintenance_fee_per_slot: 0,
        },
        SmallMarketConfig {
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 2,
            max_abs_funding_e9_per_slot: 0,
            maintenance_fee_per_slot: 1,
        },
        SmallMarketConfig {
            max_price_move_bps_per_slot: 10_000,
            max_accrual_dt_slots: 1,
            max_abs_funding_e9_per_slot: 0,
            maintenance_fee_per_slot: 10,
        },
    ])
}

#[allow(dead_code)]
fn action_strategy() -> impl Strategy<Value = Action> {
    let route = prop_oneof![
        Just(TradeRoute::NoCpi),
        Just(TradeRoute::Cpi),
        Just(TradeRoute::BatchNoCpi),
        Just(TradeRoute::BatchCpi),
    ];
    prop_oneof![
        12 => (
            route,
            any::<u8>(),
            any::<u8>(),
            any::<u8>(),
            -3i8..=3,
            0u16..=25,
            -1_000i16..=1_000,
            any::<bool>(),
        )
            .prop_map(
                |(
                    route,
                    taker,
                    maker,
                    asset,
                    units,
                    fee_bps,
                    price_move_bps,
                    prefer_reduce,
                )| {
                    Action::Trade {
                        route,
                        taker,
                        maker,
                        asset,
                        units,
                        fee_bps,
                        price_move_bps,
                        prefer_reduce,
                    }
                },
            ),
        2 => (any::<u8>(), 1u16..=2_000, 0u16..=100).prop_map(
            |(asset, halflife_slots, mark_min_fee)| Action::ConfigureEwma {
                asset,
                halflife_slots,
                mark_min_fee,
            },
        ),
        5 => (any::<u8>(), 1u8..=4, -500i16..=500).prop_map(
            |(asset, dt, move_bps)| Action::PushMark {
                asset,
                dt,
                move_bps,
            },
        ),
        6 => (
            any::<u8>(),
            prop_oneof![
                5 => Just(HintMode::Complete),
                2 => Just(HintMode::Reversed),
                1 => Just(HintMode::Empty),
                1 => Just(HintMode::Duplicate),
            ],
        )
            .prop_map(|(actor, hints)| Action::Crank { actor, hints }),
        2 => (any::<u8>(), 1u16..=500)
            .prop_map(|(actor, amount)| Action::Deposit { actor, amount }),
        2 => (any::<u8>(), 0u16..=500)
            .prop_map(|(actor, amount)| Action::Withdraw { actor, amount }),
        2 => (any::<u8>(), 1u8..=4)
            .prop_map(|(actor, dt)| Action::SyncMaintenanceFee { actor, dt }),
        2 => (any::<u8>(), any::<bool>(), 0u16..=10_000).prop_map(
            |(actor, enabled, trade_fee_cap_bps)| Action::SetMatcherConfig {
                actor,
                enabled,
                trade_fee_cap_bps,
            },
        ),
        2 => (any::<u8>(), 1u16..=500)
            .prop_map(|(domain, amount)| Action::TopUpInsurance { domain, amount }),
        2 => (any::<u8>(), 1u16..=500, 128u8..=u8::MAX).prop_map(
            |(domain, amount, expiry_delta)| Action::TopUpBacking {
                domain,
                amount,
                expiry_delta,
            },
        ),
        2 => (any::<u8>(), 1u16..=500)
            .prop_map(|(actor, amount)| Action::ConvertReleasedPnl { actor, amount }),
        2 => (any::<u8>(), any::<u8>())
            .prop_map(|(actor, asset)| Action::RebalanceReduce { actor, asset }),
        1 => (128u16..=u16::MAX, 1u16..=u16::MAX).prop_map(
            |(stale_slots, force_close_delay_slots)| Action::ConfigurePermissionlessResolve {
                stale_slots,
                force_close_delay_slots,
            },
        ),
        1 => (any::<u8>(), 0u8..=4)
            .prop_map(|(asset, dt)| Action::ShutdownAsset { asset, dt }),
        1 => (any::<u8>(), any::<u8>()).prop_map(|(asset, new_actor)| {
            Action::RotateOracleAuthority { asset, new_actor }
        }),
        2 => any::<u8>().prop_map(|actor| Action::CrossMarketSubstitution { actor }),
        2 => (
            any::<u8>(),
            prop::sample::select(SubstitutionKind::ALL.to_vec()),
        )
            .prop_map(|(actor, kind)| Action::AccountSubstitution { actor, kind }),
        3 => (any::<u8>(), any::<u8>(), any::<u8>(), -2i8..=2).prop_map(
            |(taker, maker, asset, units)| Action::RetainTrade {
                taker,
                maker,
                asset,
                units,
            },
        ),
        2 => Just(Action::LandRetained),
        1 => Just(Action::AdvanceBlockhash),
    ]
}

fn moved_price(price: u64, move_bps: i16) -> Result<u64, String> {
    let numerator = (price as i128)
        .checked_mul(10_000 + i128::from(move_bps.clamp(-9_999, 10_000)))
        .ok_or("reported-price multiplication overflow")?;
    u64::try_from((numerator / 10_000).max(1))
        .map_err(|_| "reported-price conversion overflow".into())
}

fn all_observations() -> Vec<CrankObservationHint> {
    (0..ASSET_COUNT)
        .map(|asset_index| CrankObservationHint {
            asset_index: asset_index as u16,
            oracle_accounts: 0,
        })
        .collect()
}

fn decoded_legs(account: &percolator_prog::state::PortfolioAccountV16) -> Vec<PortfolioLegV16> {
    account
        .legs
        .iter()
        .filter_map(|leg| leg.try_to_runtime().ok())
        .collect()
}

fn source_claim_for_domain(
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

fn observed_positions(
    account: &percolator_prog::state::PortfolioAccountV16,
) -> Result<[i128; ASSET_COUNT], String> {
    let mut out = [0i128; ASSET_COUNT];
    for leg in decoded_legs(account) {
        if !leg.active {
            continue;
        }
        let asset = leg.asset_index as usize;
        if asset >= ASSET_COUNT {
            return Err(format!("portfolio has out-of-world asset {asset}"));
        }
        out[asset] = out[asset]
            .checked_add(leg.basis_pos_q)
            .ok_or("observed position overflow")?;
    }
    Ok(out)
}
