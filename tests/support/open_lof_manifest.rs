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
    Pr279CollateralTopUpGenerationReplay,
    Pr328InsuranceWithdrawalGenerationReplay,
    Pr255ResolveBeforeCommittedAccrual,
    Pr260PendingEwmaInheritance,
    Pr267CrossDomainBackingDoubleSpend,
    Pr271TradeFundingErasure,
    Pr272RebalanceFundingErasure,
    Pr273ForfeitFundingErasure,
    Pr275AssetGenerationMarkReplay,
    Pr277AssetGenerationConfigReplay,
    Pr280TradeDrivenLiquidationReward,
    Pr281CrossDomainBSettlement,
    Pr282PendingEwmaTargetOverride,
    Pr283TerminalDustPayoutErasure,
    Pr290CrossMarginInsuranceDrain,
    Pr331CompositeOracleTimeSkew,
    TargetStaging,
    Pr356PendingMarkFeeReward,
    Pr365FractionalCapSettlement,
    Pr369BilateralFeeSupport,
    Pr380ProspectiveFundingRewrite,
    CompositeOracleRounding,
    Pr343TradeRetryReplay,
    Pr367PostExpiryBacking,
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
            Self::Pr279CollateralTopUpGenerationReplay => 279,
            Self::Pr328InsuranceWithdrawalGenerationReplay => 328,
            Self::Pr255ResolveBeforeCommittedAccrual => 255,
            Self::Pr260PendingEwmaInheritance => 260,
            Self::Pr267CrossDomainBackingDoubleSpend => 267,
            Self::Pr271TradeFundingErasure => 271,
            Self::Pr272RebalanceFundingErasure => 272,
            Self::Pr273ForfeitFundingErasure => 273,
            Self::Pr275AssetGenerationMarkReplay => 275,
            Self::Pr277AssetGenerationConfigReplay => 277,
            Self::Pr280TradeDrivenLiquidationReward => 280,
            Self::Pr281CrossDomainBSettlement => 281,
            Self::Pr282PendingEwmaTargetOverride => 282,
            Self::Pr283TerminalDustPayoutErasure => 283,
            Self::Pr290CrossMarginInsuranceDrain => 290,
            Self::Pr331CompositeOracleTimeSkew => 331,
            Self::TargetStaging => 332,
            Self::Pr356PendingMarkFeeReward => 356,
            Self::Pr365FractionalCapSettlement => 365,
            Self::Pr369BilateralFeeSupport => 369,
            Self::Pr380ProspectiveFundingRewrite => 380,
            Self::CompositeOracleRounding => 329,
            Self::Pr343TradeRetryReplay => 343,
            Self::Pr367PostExpiryBacking => 367,
        }
    }

    pub const fn supports(self, pr: u16) -> bool {
        match self {
            Self::Pr220OmittedRescue => pr == 220 || pr == 366,
            Self::CompositeOracleRounding => pr == 329 || pr == 381,
            Self::TargetStaging => matches!(pr, 264 | 265 | 332 | 333),
            Self::Pr279CollateralTopUpGenerationReplay => pr == 279 || pr == 320,
            _ => self.canonical_pr() == pr,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LofEvidence {
    Missing,
    Quarantined(BlockerAdapter),
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
}

use LofFamily::{
    DomainValue, FeeConsent, OracleAccrual, PrivilegedLifecycle, RecoveryTerminal,
    ReplayIncarnation,
};
use LofSeverity::{Blocker, Hardening, Privileged};

// Snapshot of every open [BLOCKER|PRIVILEGED|HARDENING LoF] PR on 2026-07-30.
// Assignment to a family is routing, not coverage. Only executable adapters may
// use Quarantined; a fixed-pin invariant will replace that state after merge.
pub const OPEN_LOFS: &[OpenLof] = &[
    OpenLof::quarantined(
        220,
        Blocker,
        OracleAccrual,
        BlockerAdapter::Pr220OmittedRescue,
    ),
    OpenLof::quarantined(
        223,
        Privileged,
        FeeConsent,
        BlockerAdapter::Pr223CpiBackingFeeSiphon,
    ),
    OpenLof::quarantined(
        224,
        Blocker,
        FeeConsent,
        BlockerAdapter::Pr224CpiCallerFeeSiphon,
    ),
    OpenLof::quarantined(
        225,
        Blocker,
        OracleAccrual,
        BlockerAdapter::Pr225ReclaimableEwmaFee,
    ),
    OpenLof::quarantined(
        231,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr231AssetGenerationTradeReplay,
    ),
    OpenLof::missing(237, Privileged, PrivilegedLifecycle),
    OpenLof::quarantined(
        251,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr251DelayedAssetAuthorityRevival,
    ),
    OpenLof::quarantined(
        253,
        Blocker,
        OracleAccrual,
        BlockerAdapter::Pr253RoundedFundingOmission,
    ),
    OpenLof::missing(254, Privileged, PrivilegedLifecycle),
    OpenLof::quarantined(
        255,
        Blocker,
        RecoveryTerminal,
        BlockerAdapter::Pr255ResolveBeforeCommittedAccrual,
    ),
    OpenLof::missing(256, Hardening, FeeConsent),
    OpenLof::missing(258, Privileged, PrivilegedLifecycle),
    OpenLof::missing(259, Privileged, FeeConsent),
    OpenLof::quarantined(
        260,
        Blocker,
        OracleAccrual,
        BlockerAdapter::Pr260PendingEwmaInheritance,
    ),
    OpenLof::quarantined(264, Blocker, OracleAccrual, BlockerAdapter::TargetStaging),
    OpenLof::quarantined(265, Blocker, OracleAccrual, BlockerAdapter::TargetStaging),
    OpenLof::quarantined(
        267,
        Blocker,
        DomainValue,
        BlockerAdapter::Pr267CrossDomainBackingDoubleSpend,
    ),
    OpenLof::quarantined(
        271,
        Blocker,
        OracleAccrual,
        BlockerAdapter::Pr271TradeFundingErasure,
    ),
    OpenLof::quarantined(
        272,
        Blocker,
        OracleAccrual,
        BlockerAdapter::Pr272RebalanceFundingErasure,
    ),
    OpenLof::quarantined(
        273,
        Blocker,
        RecoveryTerminal,
        BlockerAdapter::Pr273ForfeitFundingErasure,
    ),
    OpenLof::missing(274, Hardening, ReplayIncarnation),
    OpenLof::quarantined(
        275,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr275AssetGenerationMarkReplay,
    ),
    OpenLof::missing(276, Hardening, ReplayIncarnation),
    OpenLof::quarantined(
        277,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr277AssetGenerationConfigReplay,
    ),
    OpenLof::missing(278, Hardening, ReplayIncarnation),
    OpenLof::quarantined(
        279,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr279CollateralTopUpGenerationReplay,
    ),
    OpenLof::quarantined(
        280,
        Blocker,
        OracleAccrual,
        BlockerAdapter::Pr280TradeDrivenLiquidationReward,
    ),
    OpenLof::quarantined(
        281,
        Blocker,
        DomainValue,
        BlockerAdapter::Pr281CrossDomainBSettlement,
    ),
    OpenLof::quarantined(
        282,
        Blocker,
        OracleAccrual,
        BlockerAdapter::Pr282PendingEwmaTargetOverride,
    ),
    OpenLof::quarantined(
        283,
        Blocker,
        RecoveryTerminal,
        BlockerAdapter::Pr283TerminalDustPayoutErasure,
    ),
    OpenLof::missing(284, Blocker, DomainValue),
    OpenLof::missing(285, Hardening, ReplayIncarnation),
    OpenLof::missing(286, Blocker, RecoveryTerminal),
    OpenLof::missing(287, Blocker, RecoveryTerminal),
    OpenLof::quarantined(
        290,
        Blocker,
        DomainValue,
        BlockerAdapter::Pr290CrossMarginInsuranceDrain,
    ),
    OpenLof::missing(292, Hardening, ReplayIncarnation),
    OpenLof::missing(293, Hardening, ReplayIncarnation),
    OpenLof::missing(294, Hardening, ReplayIncarnation),
    OpenLof::missing(295, Hardening, ReplayIncarnation),
    OpenLof::missing(296, Blocker, ReplayIncarnation),
    OpenLof::missing(299, Hardening, ReplayIncarnation),
    OpenLof::missing(301, Hardening, ReplayIncarnation),
    OpenLof::missing(303, Hardening, ReplayIncarnation),
    OpenLof::missing(304, Hardening, ReplayIncarnation),
    OpenLof::missing(305, Hardening, ReplayIncarnation),
    OpenLof::missing(307, Hardening, ReplayIncarnation),
    OpenLof::missing(309, Hardening, ReplayIncarnation),
    OpenLof::missing(310, Privileged, FeeConsent),
    OpenLof::missing(311, Blocker, ReplayIncarnation),
    OpenLof::missing(312, Blocker, ReplayIncarnation),
    OpenLof::missing(313, Privileged, FeeConsent),
    OpenLof::missing(314, Blocker, FeeConsent),
    OpenLof::missing(315, Blocker, ReplayIncarnation),
    OpenLof::missing(316, Hardening, ReplayIncarnation),
    OpenLof::missing(317, Blocker, ReplayIncarnation),
    OpenLof::missing(318, Blocker, ReplayIncarnation),
    OpenLof::quarantined(
        320,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr279CollateralTopUpGenerationReplay,
    ),
    OpenLof::missing(321, Blocker, ReplayIncarnation),
    OpenLof::missing(322, Blocker, ReplayIncarnation),
    OpenLof::missing(325, Blocker, ReplayIncarnation),
    OpenLof::missing(326, Blocker, ReplayIncarnation),
    OpenLof::quarantined(
        328,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr328InsuranceWithdrawalGenerationReplay,
    ),
    OpenLof::quarantined(
        329,
        Blocker,
        OracleAccrual,
        BlockerAdapter::CompositeOracleRounding,
    ),
    OpenLof::missing(330, Blocker, RecoveryTerminal),
    OpenLof::quarantined(
        331,
        Blocker,
        OracleAccrual,
        BlockerAdapter::Pr331CompositeOracleTimeSkew,
    ),
    OpenLof::quarantined(332, Blocker, OracleAccrual, BlockerAdapter::TargetStaging),
    OpenLof::quarantined(333, Blocker, OracleAccrual, BlockerAdapter::TargetStaging),
    OpenLof::missing(334, Blocker, ReplayIncarnation),
    OpenLof::missing(335, Blocker, ReplayIncarnation),
    OpenLof::missing(336, Blocker, ReplayIncarnation),
    OpenLof::missing(337, Blocker, ReplayIncarnation),
    OpenLof::missing(338, Blocker, ReplayIncarnation),
    OpenLof::missing(339, Blocker, FeeConsent),
    OpenLof::missing(340, Blocker, ReplayIncarnation),
    OpenLof::quarantined(
        343,
        Blocker,
        ReplayIncarnation,
        BlockerAdapter::Pr343TradeRetryReplay,
    ),
    OpenLof::missing(344, Blocker, ReplayIncarnation),
    OpenLof::missing(345, Blocker, ReplayIncarnation),
    OpenLof::missing(346, Blocker, ReplayIncarnation),
    OpenLof::missing(347, Blocker, ReplayIncarnation),
    OpenLof::missing(349, Blocker, ReplayIncarnation),
    OpenLof::missing(350, Blocker, ReplayIncarnation),
    OpenLof::missing(351, Blocker, ReplayIncarnation),
    OpenLof::missing(353, Blocker, ReplayIncarnation),
    OpenLof::missing(355, Blocker, ReplayIncarnation),
    OpenLof::quarantined(
        356,
        Blocker,
        OracleAccrual,
        BlockerAdapter::Pr356PendingMarkFeeReward,
    ),
    OpenLof::missing(360, Blocker, DomainValue),
    OpenLof::missing(362, Blocker, ReplayIncarnation),
    OpenLof::missing(363, Blocker, DomainValue),
    OpenLof::quarantined(
        365,
        Blocker,
        OracleAccrual,
        BlockerAdapter::Pr365FractionalCapSettlement,
    ),
    OpenLof::quarantined(
        366,
        Blocker,
        OracleAccrual,
        BlockerAdapter::Pr220OmittedRescue,
    ),
    OpenLof::quarantined(
        367,
        Blocker,
        DomainValue,
        BlockerAdapter::Pr367PostExpiryBacking,
    ),
    OpenLof::quarantined(
        369,
        Blocker,
        FeeConsent,
        BlockerAdapter::Pr369BilateralFeeSupport,
    ),
    OpenLof::missing(370, Blocker, RecoveryTerminal),
    OpenLof::missing(372, Blocker, RecoveryTerminal),
    OpenLof::missing(373, Blocker, RecoveryTerminal),
    OpenLof::missing(374, Blocker, RecoveryTerminal),
    OpenLof::missing(375, Privileged, PrivilegedLifecycle),
    OpenLof::quarantined(
        380,
        Blocker,
        OracleAccrual,
        BlockerAdapter::Pr380ProspectiveFundingRewrite,
    ),
    OpenLof::quarantined(
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
        if let LofEvidence::Quarantined(adapter) = entry.evidence {
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
            LofEvidence::Quarantined(_) => None,
        })
        .collect()
}

pub fn quarantined_prs() -> Vec<u16> {
    OPEN_LOFS
        .iter()
        .filter_map(|entry| match entry.evidence {
            LofEvidence::Missing => None,
            LofEvidence::Quarantined(_) => Some(entry.pr),
        })
        .collect()
}
