//! INV-082 - State-indexed liveness theorem.
//!
//! Normative obligation: every publicly reachable nonterminal state in each
//! lifecycle mode either is terminal already or has a constructible bounded
//! public action that decreases the mode-specific rank.
//!
//! Evidence in this file (I/F/CU): this deterministic LiteSVM scenario first
//! lands ordinary public routes, then injects non-progressing discovery noise:
//! empty/duplicate crank hints, retained no-CPI execution, and account
//! substitution attempts. Rejected substitutions must roll back through the
//! shared scenario oracle. The same state must still admit complete-hint
//! permissionless crank progress, a normal user exit, and an independent
//! liquidation-progress probe under the compute ceiling.
//!
//! Guarantee boundary: this is a concrete whole-route witness for adversarial
//! landing order around the liveness theorem. The exhaustive quantification over
//! every reachable state remains the model/proof frontier.

use crate::support::fuzz_model::{
    run_scenario, Action, HintMode, Scenario, SmallMarketConfig, SubstitutionKind, TradeRoute,
};

#[test]
fn v16_program_public_liveness_survives_bad_hints_retained_route_and_substitutions() {
    let scenario = Scenario {
        seed: [0x82; 32],
        config: SmallMarketConfig {
            max_price_move_bps_per_slot: 500,
            max_accrual_dt_slots: 2,
            max_abs_funding_e9_per_slot: 0,
            maintenance_fee_per_slot: 1,
        },
        actions: vec![
            Action::Deposit {
                actor: 0,
                amount: 300,
            },
            Action::Deposit {
                actor: 1,
                amount: 300,
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
                route: TradeRoute::Cpi,
                taker: 2,
                maker: 3,
                asset: 1,
                units: 1,
                fee_bps: 0,
                price_move_bps: 0,
                prefer_reduce: false,
            },
            Action::RetainTrade {
                taker: 0,
                maker: 1,
                asset: 0,
                units: 1,
            },
            Action::LandRetained,
            Action::Trade {
                route: TradeRoute::BatchNoCpi,
                taker: 0,
                maker: 2,
                asset: 0,
                units: -1,
                fee_bps: 0,
                price_move_bps: 0,
                prefer_reduce: true,
            },
            Action::Trade {
                route: TradeRoute::BatchCpi,
                taker: 1,
                maker: 3,
                asset: 1,
                units: -1,
                fee_bps: 0,
                price_move_bps: 0,
                prefer_reduce: true,
            },
            Action::PushMark {
                asset: 0,
                dt: 2,
                move_bps: 500,
            },
            Action::Crank {
                actor: 0,
                hints: HintMode::Empty,
            },
            Action::Crank {
                actor: 0,
                hints: HintMode::Duplicate,
            },
            Action::Crank {
                actor: 0,
                hints: HintMode::Complete,
            },
            Action::SyncMaintenanceFee { actor: 0, dt: 2 },
            Action::AccountSubstitution {
                actor: 0,
                kind: SubstitutionKind::ForeignTradePortfolio,
            },
            Action::AccountSubstitution {
                actor: 1,
                kind: SubstitutionKind::ForeignDepositVault,
            },
            Action::AccountSubstitution {
                actor: 2,
                kind: SubstitutionKind::ForeignWithdrawVault,
            },
            Action::AccountSubstitution {
                actor: 3,
                kind: SubstitutionKind::ForeignCrankPortfolio,
            },
            Action::AccountSubstitution {
                actor: 0,
                kind: SubstitutionKind::MismatchedMatcherBinding,
            },
        ],
    };

    let coverage = run_scenario(&scenario).expect("INV-082 bad-hint liveness scenario");
    assert!(
        coverage.retained_landed > 0,
        "retained no-CPI route must execute under the same state oracle: {coverage:?}"
    );
    assert!(
        coverage
            .substitution_rejections
            .iter()
            .all(|rejections| *rejections > 0),
        "every account-substitution boundary must reject with exact rollback: {coverage:?}"
    );
    assert!(
        coverage.crank_progress > 0,
        "complete-hint public crank must still decrease rank: {coverage:?}"
    );
    assert!(
        coverage.user_positions_closed > 0,
        "normal-user exit campaign must still close a public position: {coverage:?}"
    );
    assert!(
        coverage.liquidation_steps > 0 && coverage.liquidated_abs_q > 0,
        "independent liquidation liveness probe must still progress: {coverage:?}"
    );
}
