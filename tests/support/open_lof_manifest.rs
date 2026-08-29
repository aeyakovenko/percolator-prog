#![allow(dead_code)]

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LofSeverity {
    Blocker,
    Privileged,
    Hardening,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LofFamily {
    OracleAccrual,
    FeeConsent,
    ReplayIncarnation,
    DomainValue,
    RecoveryTerminal,
    PrivilegedLifecycle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockerAdapter {
    Pr220OmittedRescue,
    Pr223CpiBackingFeeSiphon,
    Pr224CpiCallerFeeSiphon,
    Pr225ReclaimableEwmaFee,
    Pr231AssetGenerationTradeReplay,
    Pr251DelayedAssetAuthorityRevival,
    Pr253RoundedFundingOmission,
    Pr254ShutdownCommittedFunding,
    Pr256LiveFeeConsent,
    Pr259DirectBackingFeeConsent,
    Pr279CollateralTopUpGenerationReplay,
    Pr321BackingTopUpGenerationReplay,
    Pr328InsuranceWithdrawalGenerationReplay,
    Pr344InsuranceTopUpRetryReplay,
    Pr351BackingTopUpRetryReplay,
    Pr355WithdrawalRetryLiquidation,
    Pr350DepositRetryReplay,
    Pr299PortfolioIncarnationWithdrawal,
    Pr305PortfolioIncarnationDeposit,
    Pr307MarketIncarnationDeposit,
    Pr311ResolveGenerationReplay,
    Pr315ShutdownGenerationReplay,
    Pr314ActivationFeeConsent,
    Pr310BilateralBaseFeeConsent,
    Pr325MaintenancePolicyGenerationReplay,
    Pr326LiquidationPolicyGenerationReplay,
    Pr336DelayedLiquidationPolicyReplay,
    Pr337DelayedMaintenancePolicyReplay,
    Pr338DelayedTradeFeePolicyReplay,
    Pr340DelayedFeeRedirectPolicyReplay,
    Pr349DelayedBackingFeePolicyReplay,
    Pr335DelayedOracleIntentReplay,
    Pr334DelayedMatcherEnableReplay,
    Pr339BackingFeeConsentReplay,
    AuthorityHandoffAbaReplayFamily,
    Pr347DelayedResolvePolicyReplay,
    Pr353ResolveAuthorityIncarnationReplay,
    Pr309PortfolioCloseIncarnationReplay,
    Pr304MatcherGrantPortfolioIncarnationReplay,
    Pr303TradePortfolioIncarnationReplay,
    Pr301ConvertPortfolioIncarnationReplay,
    Pr278ForfeitPortfolioIncarnationReplay,
    PortfolioAuthorityIncarnationReplayFamily,
    Pr294MatcherGrantMarketGenerationReplay,
    Pr295ForfeitMarketGenerationReplay,
    Pr296TradeFeeMarketGenerationReplay,
    Pr317FeeRedirectGenerationReplay,
    Pr318BackingFeeGenerationReplay,
    Pr362ActivationRetryReplay,
    Pr255ResolveBeforeCommittedAccrual,
    Pr260PendingEwmaInheritance,
    Pr267CrossDomainBackingDoubleSpend,
    Pr271TradeFundingErasure,
    Pr272RebalanceFundingErasure,
    Pr273ForfeitFundingErasure,
    OracleGenerationReplayFamily,
    Pr280TradeDrivenLiquidationReward,
    Pr281CrossDomainBSettlement,
    Pr282PendingEwmaTargetOverride,
    Pr283TerminalDustPayoutErasure,
    Pr284SignedDirectionSideAttribution,
    Pr292RebalancePortfolioIncarnation,
    Pr293RebalanceMarketIncarnation,
    Pr290CrossMarginInsuranceDrain,
    Pr331CompositeOracleTimeSkew,
    Pr312ResolvePolicyGenerationReplay,
    Pr313CpiBaseFeeConsent,
    Pr316RebalancePositionEpisode,
    Pr330TerminalBankruptcyResidual,
    TargetStaging,
    Pr356PendingMarkFeeReward,
    Pr365FractionalCapSettlement,
    Pr369BilateralFeeSupport,
    Pr380ProspectiveFundingRewrite,
    CompositeOracleRounding,
    Pr343TradeRetryReplay,
    Pr367PostExpiryBacking,
    Pr360StaleCohortNovation,
    Pr363ExpiredBackingConversion,
    Pr375FundedRoleAdminSeizure,
}

impl BlockerAdapter {
    pub const fn canonical_pr(self) -> u16 {
        match self {
            Self::Pr220OmittedRescue => 220,
            Self::Pr223CpiBackingFeeSiphon => 223,
            Self::Pr224CpiCallerFeeSiphon => 224,
            Self::Pr225ReclaimableEwmaFee => 225,
            Self::Pr231AssetGenerationTradeReplay => 231,
            Self::Pr251DelayedAssetAuthorityRevival => 251,
            Self::Pr253RoundedFundingOmission => 253,
            Self::Pr254ShutdownCommittedFunding => 254,
            Self::Pr256LiveFeeConsent => 256,
            Self::Pr259DirectBackingFeeConsent => 259,
            Self::Pr279CollateralTopUpGenerationReplay => 279,
            Self::Pr321BackingTopUpGenerationReplay => 321,
            Self::Pr328InsuranceWithdrawalGenerationReplay => 328,
            Self::Pr344InsuranceTopUpRetryReplay => 344,
            Self::Pr351BackingTopUpRetryReplay => 351,
            Self::Pr355WithdrawalRetryLiquidation => 355,
            Self::Pr350DepositRetryReplay => 350,
            Self::Pr299PortfolioIncarnationWithdrawal => 299,
            Self::Pr305PortfolioIncarnationDeposit => 305,
            Self::Pr307MarketIncarnationDeposit => 307,
            Self::Pr311ResolveGenerationReplay => 311,
            Self::Pr315ShutdownGenerationReplay => 315,
            Self::Pr314ActivationFeeConsent => 314,
            Self::Pr310BilateralBaseFeeConsent => 310,
            Self::Pr325MaintenancePolicyGenerationReplay => 325,
            Self::Pr326LiquidationPolicyGenerationReplay => 326,
            Self::Pr336DelayedLiquidationPolicyReplay => 336,
            Self::Pr337DelayedMaintenancePolicyReplay => 337,
            Self::Pr338DelayedTradeFeePolicyReplay => 338,
            Self::Pr340DelayedFeeRedirectPolicyReplay => 340,
            Self::Pr349DelayedBackingFeePolicyReplay => 349,
            Self::Pr335DelayedOracleIntentReplay => 335,
            Self::Pr334DelayedMatcherEnableReplay => 334,
            Self::Pr339BackingFeeConsentReplay => 339,
            Self::AuthorityHandoffAbaReplayFamily => 345,
            Self::Pr347DelayedResolvePolicyReplay => 347,
            Self::Pr353ResolveAuthorityIncarnationReplay => 353,
            Self::Pr309PortfolioCloseIncarnationReplay => 309,
            Self::Pr304MatcherGrantPortfolioIncarnationReplay => 304,
            Self::Pr303TradePortfolioIncarnationReplay => 303,
            Self::Pr301ConvertPortfolioIncarnationReplay => 301,
            Self::Pr278ForfeitPortfolioIncarnationReplay => 278,
            Self::PortfolioAuthorityIncarnationReplayFamily => 285,
            Self::Pr294MatcherGrantMarketGenerationReplay => 294,
            Self::Pr295ForfeitMarketGenerationReplay => 295,
            Self::Pr296TradeFeeMarketGenerationReplay => 296,
            Self::Pr317FeeRedirectGenerationReplay => 317,
            Self::Pr318BackingFeeGenerationReplay => 318,
            Self::Pr362ActivationRetryReplay => 362,
            Self::Pr255ResolveBeforeCommittedAccrual => 255,
            Self::Pr260PendingEwmaInheritance => 260,
            Self::Pr267CrossDomainBackingDoubleSpend => 267,
            Self::Pr271TradeFundingErasure => 271,
            Self::Pr272RebalanceFundingErasure => 272,
            Self::Pr273ForfeitFundingErasure => 273,
            Self::OracleGenerationReplayFamily => 322,
            Self::Pr280TradeDrivenLiquidationReward => 280,
            Self::Pr281CrossDomainBSettlement => 281,
            Self::Pr282PendingEwmaTargetOverride => 282,
            Self::Pr283TerminalDustPayoutErasure => 283,
            Self::Pr284SignedDirectionSideAttribution => 284,
            Self::Pr292RebalancePortfolioIncarnation => 292,
            Self::Pr293RebalanceMarketIncarnation => 293,
            Self::Pr290CrossMarginInsuranceDrain => 290,
            Self::Pr331CompositeOracleTimeSkew => 331,
            Self::Pr312ResolvePolicyGenerationReplay => 312,
            Self::Pr313CpiBaseFeeConsent => 313,
            Self::Pr316RebalancePositionEpisode => 316,
            Self::Pr330TerminalBankruptcyResidual => 330,
            Self::TargetStaging => 332,
            Self::Pr356PendingMarkFeeReward => 356,
            Self::Pr365FractionalCapSettlement => 365,
            Self::Pr369BilateralFeeSupport => 369,
            Self::Pr380ProspectiveFundingRewrite => 380,
            Self::CompositeOracleRounding => 329,
            Self::Pr343TradeRetryReplay => 343,
            Self::Pr367PostExpiryBacking => 367,
            Self::Pr360StaleCohortNovation => 360,
            Self::Pr363ExpiredBackingConversion => 363,
            Self::Pr375FundedRoleAdminSeizure => 375,
        }
    }

    pub const fn supports(self, pr: u16) -> bool {
        match self {
            Self::Pr220OmittedRescue => pr == 220 || pr == 366,
            Self::CompositeOracleRounding => pr == 329 || pr == 381,
            Self::TargetStaging => matches!(pr, 264 | 265 | 332 | 333),
            Self::Pr279CollateralTopUpGenerationReplay => pr == 279 || pr == 320,
            Self::OracleGenerationReplayFamily => matches!(pr, 275 | 277 | 322),
            Self::AuthorityHandoffAbaReplayFamily => pr == 345 || pr == 346,
            Self::Pr304MatcherGrantPortfolioIncarnationReplay => pr == 274 || pr == 304,
            Self::Pr303TradePortfolioIncarnationReplay => pr == 276 || pr == 303,
            _ => self.canonical_pr() == pr,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LofEvidence {
    Missing,
    Quarantined(BlockerAdapter),
    Certified(BlockerAdapter),
    Nonqualifying,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenLof {
    pub pr: u16,
    pub severity: LofSeverity,
    pub family: LofFamily,
    pub evidence: LofEvidence,
}

impl OpenLof {
    const fn missing(pr: u16, severity: LofSeverity, family: LofFamily) -> Self {
        Self {
            pr,
            severity,
            family,
            evidence: LofEvidence::Missing,
        }
    }

    const fn quarantined(
        pr: u16,
        severity: LofSeverity,
        family: LofFamily,
        adapter: BlockerAdapter,
    ) -> Self {
        Self {
            pr,
            severity,
            family,
            evidence: LofEvidence::Quarantined(adapter),
        }
    }

    const fn certified(
        pr: u16,
        severity: LofSeverity,
        family: LofFamily,
        adapter: BlockerAdapter,
    ) -> Self {
        Self {
            pr,
            severity,
            family,
            evidence: LofEvidence::Certified(adapter),
        }
    }

    const fn nonqualifying(pr: u16, severity: LofSeverity, family: LofFamily) -> Self {
        Self {
            pr,
            severity,
            family,
            evidence: LofEvidence::Nonqualifying,
        }
    }
}

use LofFamily::{
    DomainValue, FeeConsent, OracleAccrual, PrivilegedLifecycle, RecoveryTerminal,
    ReplayIncarnation,
};
use LofSeverity::{Blocker, Hardening, Privileged};

// Snapshot of every open [BLOCKER|PRIVILEGED|HARDENING LoF] PR on 2026-07-30.
// Assignment to a family is routing, not coverage. Only executable adapters may
// use Quarantined. Certified means the same public adapter now asserts the fixed-pin safety
// postcondition; it does not imply exhaustive proof outside that adapter's stated boundary.
pub const OPEN_LOFS: &[OpenLof] = &[
    OpenLof::certified(
        220,
        Blocker,
        OracleAccrual,
        BlockerAdapter::Pr220OmittedRescue,
    ),
    OpenLof::certified(
        223,
        Privileged,
        FeeConsent,
        BlockerAdapter::Pr223CpiBackingFeeSiphon,
    ),
    OpenLof::certified(
        224,
        Blocker,
        FeeConsent,
        BlockerAdapter::Pr224CpiCallerFeeSiphon,
    ),
    OpenLof::certified(
        225,
        Blocker,
        OracleAccrual,
        BlockerAdapter::Pr225ReclaimableEwmaFee,
    ),
    OpenLof::certified(
        231,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr231AssetGenerationTradeReplay,
    ),
    OpenLof::nonqualifying(237, Privileged, PrivilegedLifecycle),
    OpenLof::certified(
        251,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr251DelayedAssetAuthorityRevival,
    ),
    OpenLof::certified(
        253,
        Blocker,
        OracleAccrual,
        BlockerAdapter::Pr253RoundedFundingOmission,
    ),
    OpenLof::certified(
        254,
        Privileged,
        PrivilegedLifecycle,
        BlockerAdapter::Pr254ShutdownCommittedFunding,
    ),
    OpenLof::certified(
        255,
        Blocker,
        RecoveryTerminal,
        BlockerAdapter::Pr255ResolveBeforeCommittedAccrual,
    ),
    OpenLof::certified(
        256,
        Hardening,
        FeeConsent,
        BlockerAdapter::Pr256LiveFeeConsent,
    ),
    OpenLof::nonqualifying(258, Privileged, PrivilegedLifecycle),
    OpenLof::certified(
        259,
        Privileged,
        FeeConsent,
        BlockerAdapter::Pr259DirectBackingFeeConsent,
    ),
    OpenLof::certified(
        260,
        Blocker,
        OracleAccrual,
        BlockerAdapter::Pr260PendingEwmaInheritance,
    ),
    OpenLof::certified(264, Blocker, OracleAccrual, BlockerAdapter::TargetStaging),
    OpenLof::certified(265, Blocker, OracleAccrual, BlockerAdapter::TargetStaging),
    OpenLof::certified(
        267,
        Blocker,
        DomainValue,
        BlockerAdapter::Pr267CrossDomainBackingDoubleSpend,
    ),
    OpenLof::certified(
        271,
        Blocker,
        OracleAccrual,
        BlockerAdapter::Pr271TradeFundingErasure,
    ),
    OpenLof::certified(
        272,
        Blocker,
        OracleAccrual,
        BlockerAdapter::Pr272RebalanceFundingErasure,
    ),
    OpenLof::certified(
        273,
        Blocker,
        RecoveryTerminal,
        BlockerAdapter::Pr273ForfeitFundingErasure,
    ),
    OpenLof::certified(
        274,
        Hardening,
        ReplayIncarnation,
        BlockerAdapter::Pr304MatcherGrantPortfolioIncarnationReplay,
    ),
    OpenLof::certified(
        275,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::OracleGenerationReplayFamily,
    ),
    OpenLof::certified(
        276,
        Hardening,
        ReplayIncarnation,
        BlockerAdapter::Pr303TradePortfolioIncarnationReplay,
    ),
    OpenLof::certified(
        277,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::OracleGenerationReplayFamily,
    ),
    OpenLof::certified(
        278,
        Hardening,
        ReplayIncarnation,
        BlockerAdapter::Pr278ForfeitPortfolioIncarnationReplay,
    ),
    OpenLof::certified(
        279,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr279CollateralTopUpGenerationReplay,
    ),
    OpenLof::certified(
        280,
        Blocker,
        OracleAccrual,
        BlockerAdapter::Pr280TradeDrivenLiquidationReward,
    ),
    OpenLof::certified(
        281,
        Blocker,
        DomainValue,
        BlockerAdapter::Pr281CrossDomainBSettlement,
    ),
    OpenLof::certified(
        282,
        Blocker,
        OracleAccrual,
        BlockerAdapter::Pr282PendingEwmaTargetOverride,
    ),
    OpenLof::certified(
        283,
        Blocker,
        RecoveryTerminal,
        BlockerAdapter::Pr283TerminalDustPayoutErasure,
    ),
    OpenLof::certified(
        284,
        Blocker,
        DomainValue,
        BlockerAdapter::Pr284SignedDirectionSideAttribution,
    ),
    OpenLof::certified(
        285,
        Hardening,
        ReplayIncarnation,
        BlockerAdapter::PortfolioAuthorityIncarnationReplayFamily,
    ),
    OpenLof::nonqualifying(286, Blocker, RecoveryTerminal),
    OpenLof::nonqualifying(287, Blocker, RecoveryTerminal),
    OpenLof::certified(
        290,
        Blocker,
        DomainValue,
        BlockerAdapter::Pr290CrossMarginInsuranceDrain,
    ),
    OpenLof::certified(
        292,
        Hardening,
        ReplayIncarnation,
        BlockerAdapter::Pr292RebalancePortfolioIncarnation,
    ),
    OpenLof::quarantined(
        293,
        Hardening,
        ReplayIncarnation,
        BlockerAdapter::Pr293RebalanceMarketIncarnation,
    ),
    OpenLof::quarantined(
        294,
        Hardening,
        ReplayIncarnation,
        BlockerAdapter::Pr294MatcherGrantMarketGenerationReplay,
    ),
    OpenLof::quarantined(
        295,
        Hardening,
        ReplayIncarnation,
        BlockerAdapter::Pr295ForfeitMarketGenerationReplay,
    ),
    OpenLof::quarantined(
        296,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr296TradeFeeMarketGenerationReplay,
    ),
    OpenLof::certified(
        299,
        Hardening,
        ReplayIncarnation,
        BlockerAdapter::Pr299PortfolioIncarnationWithdrawal,
    ),
    OpenLof::certified(
        301,
        Hardening,
        ReplayIncarnation,
        BlockerAdapter::Pr301ConvertPortfolioIncarnationReplay,
    ),
    OpenLof::certified(
        303,
        Hardening,
        ReplayIncarnation,
        BlockerAdapter::Pr303TradePortfolioIncarnationReplay,
    ),
    OpenLof::certified(
        304,
        Hardening,
        ReplayIncarnation,
        BlockerAdapter::Pr304MatcherGrantPortfolioIncarnationReplay,
    ),
    OpenLof::certified(
        305,
        Hardening,
        ReplayIncarnation,
        BlockerAdapter::Pr305PortfolioIncarnationDeposit,
    ),
    OpenLof::quarantined(
        307,
        Hardening,
        ReplayIncarnation,
        BlockerAdapter::Pr307MarketIncarnationDeposit,
    ),
    OpenLof::certified(
        309,
        Hardening,
        ReplayIncarnation,
        BlockerAdapter::Pr309PortfolioCloseIncarnationReplay,
    ),
    OpenLof::certified(
        310,
        Privileged,
        FeeConsent,
        BlockerAdapter::Pr310BilateralBaseFeeConsent,
    ),
    OpenLof::certified(
        311,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr311ResolveGenerationReplay,
    ),
    OpenLof::certified(
        312,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr312ResolvePolicyGenerationReplay,
    ),
    OpenLof::certified(
        313,
        Privileged,
        FeeConsent,
        BlockerAdapter::Pr313CpiBaseFeeConsent,
    ),
    OpenLof::certified(
        314,
        Blocker,
        FeeConsent,
        BlockerAdapter::Pr314ActivationFeeConsent,
    ),
    OpenLof::certified(
        315,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr315ShutdownGenerationReplay,
    ),
    OpenLof::certified(
        316,
        Hardening,
        ReplayIncarnation,
        BlockerAdapter::Pr316RebalancePositionEpisode,
    ),
    OpenLof::quarantined(
        317,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr317FeeRedirectGenerationReplay,
    ),
    OpenLof::certified(
        318,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr318BackingFeeGenerationReplay,
    ),
    OpenLof::certified(
        320,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr279CollateralTopUpGenerationReplay,
    ),
    OpenLof::certified(
        321,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr321BackingTopUpGenerationReplay,
    ),
    OpenLof::certified(
        322,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::OracleGenerationReplayFamily,
    ),
    OpenLof::quarantined(
        325,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr325MaintenancePolicyGenerationReplay,
    ),
    OpenLof::quarantined(
        326,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr326LiquidationPolicyGenerationReplay,
    ),
    OpenLof::certified(
        328,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr328InsuranceWithdrawalGenerationReplay,
    ),
    OpenLof::certified(
        329,
        Blocker,
        OracleAccrual,
        BlockerAdapter::CompositeOracleRounding,
    ),
    OpenLof::certified(
        330,
        Blocker,
        RecoveryTerminal,
        BlockerAdapter::Pr330TerminalBankruptcyResidual,
    ),
    OpenLof::certified(
        331,
        Blocker,
        OracleAccrual,
        BlockerAdapter::Pr331CompositeOracleTimeSkew,
    ),
    OpenLof::certified(332, Blocker, OracleAccrual, BlockerAdapter::TargetStaging),
    OpenLof::certified(333, Blocker, OracleAccrual, BlockerAdapter::TargetStaging),
    OpenLof::certified(
        334,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr334DelayedMatcherEnableReplay,
    ),
    OpenLof::certified(
        335,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr335DelayedOracleIntentReplay,
    ),
    OpenLof::certified(
        336,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr336DelayedLiquidationPolicyReplay,
    ),
    OpenLof::certified(
        337,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr337DelayedMaintenancePolicyReplay,
    ),
    OpenLof::certified(
        338,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr338DelayedTradeFeePolicyReplay,
    ),
    OpenLof::quarantined(
        339,
        Blocker,
        FeeConsent,
        BlockerAdapter::Pr339BackingFeeConsentReplay,
    ),
    OpenLof::certified(
        340,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr340DelayedFeeRedirectPolicyReplay,
    ),
    OpenLof::certified(
        343,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr343TradeRetryReplay,
    ),
    OpenLof::certified(
        344,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr344InsuranceTopUpRetryReplay,
    ),
    OpenLof::certified(
        345,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::AuthorityHandoffAbaReplayFamily,
    ),
    OpenLof::certified(
        346,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::AuthorityHandoffAbaReplayFamily,
    ),
    OpenLof::certified(
        347,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr347DelayedResolvePolicyReplay,
    ),
    OpenLof::certified(
        349,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr349DelayedBackingFeePolicyReplay,
    ),
    OpenLof::certified(
        350,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr350DepositRetryReplay,
    ),
    OpenLof::certified(
        351,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr351BackingTopUpRetryReplay,
    ),
    OpenLof::certified(
        353,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr353ResolveAuthorityIncarnationReplay,
    ),
    OpenLof::certified(
        355,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr355WithdrawalRetryLiquidation,
    ),
    OpenLof::certified(
        356,
        Blocker,
        OracleAccrual,
        BlockerAdapter::Pr356PendingMarkFeeReward,
    ),
    OpenLof::certified(
        360,
        Blocker,
        DomainValue,
        BlockerAdapter::Pr360StaleCohortNovation,
    ),
    OpenLof::certified(
        362,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr362ActivationRetryReplay,
    ),
    OpenLof::certified(
        363,
        Blocker,
        DomainValue,
        BlockerAdapter::Pr363ExpiredBackingConversion,
    ),
    OpenLof::certified(
        365,
        Blocker,
        OracleAccrual,
        BlockerAdapter::Pr365FractionalCapSettlement,
    ),
    OpenLof::certified(
        366,
        Blocker,
        OracleAccrual,
        BlockerAdapter::Pr220OmittedRescue,
    ),
    OpenLof::certified(
        367,
        Blocker,
        DomainValue,
        BlockerAdapter::Pr367PostExpiryBacking,
    ),
    OpenLof::certified(
        369,
        Blocker,
        FeeConsent,
        BlockerAdapter::Pr369BilateralFeeSupport,
    ),
    OpenLof::nonqualifying(370, Blocker, RecoveryTerminal),
    OpenLof::nonqualifying(372, Blocker, RecoveryTerminal),
    OpenLof::nonqualifying(373, Blocker, RecoveryTerminal),
    OpenLof::nonqualifying(374, Blocker, RecoveryTerminal),
    OpenLof::certified(
        375,
        Privileged,
        PrivilegedLifecycle,
        BlockerAdapter::Pr375FundedRoleAdminSeizure,
    ),
    OpenLof::certified(
        380,
        Blocker,
        OracleAccrual,
        BlockerAdapter::Pr380ProspectiveFundingRewrite,
    ),
    OpenLof::certified(
        381,
        Blocker,
        OracleAccrual,
        BlockerAdapter::CompositeOracleRounding,
    ),
];

pub fn validate_manifest() -> Result<(), String> {
    if OPEN_LOFS.len() != 99 {
        return Err(format!(
            "open LoF snapshot changed size: expected 99, found {}",
            OPEN_LOFS.len()
        ));
    }
    for pair in OPEN_LOFS.windows(2) {
        if pair[0].pr >= pair[1].pr {
            return Err(format!(
                "open LoF PR IDs must be strictly increasing: {} then {}",
                pair[0].pr, pair[1].pr
            ));
        }
    }
    for entry in OPEN_LOFS {
        if let LofEvidence::Quarantined(adapter) | LofEvidence::Certified(adapter) = entry.evidence
        {
            if !adapter.supports(entry.pr) {
                return Err(format!(
                    "PR {} claims incompatible adapter {:?} (canonical PR {})",
                    entry.pr,
                    adapter,
                    adapter.canonical_pr()
                ));
            }
        }
    }
    let severity_counts = OPEN_LOFS.iter().fold([0usize; 3], |mut counts, entry| {
        let index = match entry.severity {
            LofSeverity::Blocker => 0,
            LofSeverity::Privileged => 1,
            LofSeverity::Hardening => 2,
        };
        counts[index] += 1;
        counts
    });
    if severity_counts != [74, 8, 17] {
        return Err(format!(
            "open LoF severity snapshot changed: expected [74, 8, 17], found {severity_counts:?}"
        ));
    }
    Ok(())
}

pub fn missing_prs() -> Vec<u16> {
    OPEN_LOFS
        .iter()
        .filter_map(|entry| match entry.evidence {
            LofEvidence::Missing => Some(entry.pr),
            LofEvidence::Quarantined(_)
            | LofEvidence::Certified(_)
            | LofEvidence::Nonqualifying => None,
        })
        .collect()
}

pub fn quarantined_prs() -> Vec<u16> {
    OPEN_LOFS
        .iter()
        .filter_map(|entry| match entry.evidence {
            LofEvidence::Missing | LofEvidence::Nonqualifying => None,
            LofEvidence::Quarantined(_) => Some(entry.pr),
            LofEvidence::Certified(_) => None,
        })
        .collect()
}

pub fn certified_prs() -> Vec<u16> {
    OPEN_LOFS
        .iter()
        .filter_map(|entry| match entry.evidence {
            LofEvidence::Certified(_) => Some(entry.pr),
            LofEvidence::Missing | LofEvidence::Quarantined(_) | LofEvidence::Nonqualifying => None,
        })
        .collect()
}

pub fn nonqualifying_prs() -> Vec<u16> {
    OPEN_LOFS
        .iter()
        .filter_map(|entry| match entry.evidence {
            LofEvidence::Nonqualifying => Some(entry.pr),
            LofEvidence::Missing | LofEvidence::Quarantined(_) | LofEvidence::Certified(_) => None,
        })
        .collect()
}
