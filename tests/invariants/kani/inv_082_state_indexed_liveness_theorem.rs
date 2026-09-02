//! INV-082 - State-indexed liveness theorem.
//!
//! The engine owns state classification and each continuation's concrete rank postcondition. This
//! wrapper composition executes the pinned engine's real selector over arbitrary class magnitudes
//! and proves that applying the named per-class postcondition strictly lowers one finite
//! lexicographic rank. A second proof exhausts every overlap of the eight actionable-summary flags
//! and proves that at most seven class-completion steps reach `NoAction`.
//!
//! This is not a duplicate model of engine state. INV-071 source-locks the classifier, selector,
//! dispatch, and rank contracts to all wrapper callsites and public witnesses; INV-077 owns the
//! maximum-shape CU bound for each step. The only environment exception is the engine's explicit
//! `RefreshAccount { asset_index: None }` case, which requires one authenticated observation and is
//! covered by the public oracle/recovery availability assumptions in INV-020/073/078.

use percolator::{
    auto_crank_plan_requires_caller_observation, kani_select_auto_crank_plan, ActionableSummaryV16,
    AutoCrankPlanV16, PermissionlessRecoveryReasonV16,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct AccountLivenessRank {
    recovery: u128,
    resolved: u128,
    close: u128,
    b_settlement: u128,
    liquidation: u128,
    source_lien: u128,
    refresh: u128,
}

fn inv082_summary_for_rank(
    rank: AccountLivenessRank,
    recovery_flag_shape: u8,
) -> ActionableSummaryV16 {
    let expired_close = rank.recovery != 0 && recovery_flag_shape & 1 != 0;
    let recovery_eligible = rank.recovery != 0 && (recovery_flag_shape & 2 != 0 || !expired_close);
    ActionableSummaryV16 {
        stale: rank.refresh != 0,
        b_stale: rank.b_settlement != 0,
        pending_close: rank.close != 0,
        expired_close,
        liquidatable: rank.liquidation != 0,
        source_liens_releasable: rank.source_lien != 0,
        recovery_eligible,
        resolved_winner: rank.resolved != 0,
    }
}

fn inv082_rank_for_summary(summary: ActionableSummaryV16) -> AccountLivenessRank {
    AccountLivenessRank {
        recovery: u128::from(summary.expired_close || summary.recovery_eligible),
        resolved: u128::from(summary.resolved_winner),
        close: u128::from(summary.pending_close),
        b_settlement: u128::from(summary.b_stale),
        liquidation: u128::from(summary.liquidatable),
        source_lien: u128::from(summary.source_liens_releasable),
        refresh: u128::from(summary.stale),
    }
}

fn inv082_select(summary: ActionableSummaryV16, refresh_asset: Option<usize>) -> AutoCrankPlanV16 {
    kani_select_auto_crank_plan(
        summary,
        3,
        5,
        refresh_asset,
        PermissionlessRecoveryReasonV16::ActiveBankruptCloseCannotProgress,
    )
}

fn inv082_apply_named_engine_progress_contract(
    mut rank: AccountLivenessRank,
    selected: AutoCrankPlanV16,
) -> AccountLivenessRank {
    match selected {
        AutoCrankPlanV16::NoAction => {}
        AutoCrankPlanV16::DeclareRecovery { .. } => {
            rank.recovery = rank
                .recovery
                .checked_sub(1)
                .expect("recovery contract requires active recovery work");
        }
        AutoCrankPlanV16::CloseResolved => {
            rank.resolved = rank
                .resolved
                .checked_sub(1)
                .expect("resolved-close contract requires active resolved work");
        }
        AutoCrankPlanV16::AdvanceClose => {
            rank.close = rank
                .close
                .checked_sub(1)
                .expect("close contract requires outstanding residual work");
        }
        AutoCrankPlanV16::SettleBChunk { .. } => {
            rank.b_settlement = rank
                .b_settlement
                .checked_sub(1)
                .expect("B contract requires a pending settlement chunk");
        }
        AutoCrankPlanV16::Liquidate { .. } => {
            rank.liquidation = rank
                .liquidation
                .checked_sub(1)
                .expect("liquidation contract requires reducible exposure");
        }
        AutoCrankPlanV16::ReleaseSourceLiens => {
            rank.source_lien = rank
                .source_lien
                .checked_sub(1)
                .expect("source-lien contract requires releasable encumbrance");
        }
        AutoCrankPlanV16::RefreshAccount { .. } => {
            rank.refresh = rank
                .refresh
                .checked_sub(1)
                .expect("refresh contract requires stale account work");
        }
        AutoCrankPlanV16::FinalizeRecovery => {
            panic!("the Live/Resolved actionable selector cannot emit FinalizeRecovery");
        }
    }
    rank
}

#[kani::proof]
fn kani_inv082_actual_engine_selector_composes_to_strict_rank_decrease() {
    let before = AccountLivenessRank {
        recovery: kani::any(),
        resolved: kani::any(),
        close: kani::any(),
        b_settlement: kani::any(),
        liquidation: kani::any(),
        source_lien: kani::any(),
        refresh: kani::any(),
    };
    let recovery_flag_shape: u8 = kani::any();
    let refresh_asset: Option<usize> = kani::any();
    let summary = inv082_summary_for_rank(before, recovery_flag_shape);
    let selected = inv082_select(summary, refresh_asset);
    let after = inv082_apply_named_engine_progress_contract(before, selected);

    kani::cover!(
        matches!(selected, AutoCrankPlanV16::DeclareRecovery { .. }),
        "recovery class selected"
    );
    kani::cover!(
        matches!(selected, AutoCrankPlanV16::CloseResolved),
        "resolved class selected"
    );
    kani::cover!(
        matches!(selected, AutoCrankPlanV16::AdvanceClose),
        "close class selected"
    );
    kani::cover!(
        matches!(selected, AutoCrankPlanV16::SettleBChunk { .. }),
        "B class selected"
    );
    kani::cover!(
        matches!(selected, AutoCrankPlanV16::Liquidate { .. }),
        "liquidation class selected"
    );
    kani::cover!(
        matches!(selected, AutoCrankPlanV16::ReleaseSourceLiens),
        "source-lien class selected"
    );
    kani::cover!(
        matches!(selected, AutoCrankPlanV16::RefreshAccount { .. }),
        "refresh class selected"
    );
    kani::cover!(
        matches!(selected, AutoCrankPlanV16::NoAction),
        "fixed point selected"
    );
    kani::cover!(
        auto_crank_plan_requires_caller_observation(&selected),
        "empty-account refresh exposes the authenticated-observation requirement"
    );

    assert_eq!(
        matches!(selected, AutoCrankPlanV16::NoAction),
        before == AccountLivenessRank::default()
    );
    assert!(before == AccountLivenessRank::default() || after < before);
    assert!(
        !auto_crank_plan_requires_caller_observation(&selected)
            || matches!(
                selected,
                AutoCrankPlanV16::RefreshAccount { asset_index: None }
            )
    );
}

#[kani::proof]
#[kani::unwind(8)]
fn kani_inv082_every_actionable_summary_overlap_reaches_fixed_point() {
    let original = ActionableSummaryV16 {
        stale: kani::any(),
        b_stale: kani::any(),
        pending_close: kani::any(),
        expired_close: kani::any(),
        liquidatable: kani::any(),
        source_liens_releasable: kani::any(),
        recovery_eligible: kani::any(),
        resolved_winner: kani::any(),
    };
    let mut rank = inv082_rank_for_summary(original);
    let initial_rank = rank;

    for _ in 0..7 {
        let summary = inv082_summary_for_rank(rank, 1);
        let selected = inv082_select(summary, Some(0));
        rank = inv082_apply_named_engine_progress_contract(rank, selected);
    }

    kani::cover!(
        original.expired_close && original.recovery_eligible,
        "overlapping recovery flags are exhausted as one terminal class"
    );
    kani::cover!(
        original.stale
            && original.b_stale
            && original.pending_close
            && original.liquidatable
            && original.source_liens_releasable
            && original.resolved_winner,
        "all nonterminal summary classes overlap"
    );
    assert_eq!(rank, AccountLivenessRank::default());
    assert!(initial_rank == AccountLivenessRank::default() || rank < initial_rank);
    assert!(matches!(
        inv082_select(inv082_summary_for_rank(rank, 1), Some(0)),
        AutoCrankPlanV16::NoAction
    ));
}
