//! INV-081 - Success-state validity over complete public routes.
//!
//! Normative obligation: every successful public wrapper instruction commits a globally valid
//! post-state and only the economic deltas authorized by the instruction's signer/account set.
//! Every rejected wrapper route must leave the complete tracked public state unchanged.
//!
//! Evidence in this file (I/F): this deterministic LiteSVM scenario uses the shared whole-route
//! public-interface oracle, not a hand-written state mutation. The scenario runs successful
//! deposits, withdrawals, all four trade routes, mark/crank progress, and liquidation coverage
//! through the runner's mandatory prefix, then adds a fixed mixed route with an over-policy CPI
//! trade rejection. After every successful wrapper instruction the oracle checks SPL supply,
//! vault/accounting equality, source-credit attribution, OI/current-leg shape, and authorized
//! token/account frames. Rejected routes are checked by byte-for-byte snapshots of market,
//! portfolio, backing-ledger, matcher-context, and SPL-token state.
//!
//! Guarantee boundary: this is a deterministic CU-owned whole-route witness for the deployed SBF
//! artifact. It complements, rather than replaces, the public-SBF blocker corpus and generated
//! stateful INV-081 coverage.

use crate::support::fuzz_model::{
    run_scenario, Action, HintMode, Scenario, SmallMarketConfig, TradeRoute,
};

#[test]
fn v16_program_public_route_oracle_checks_success_and_reject_frames_fixed_case() {
    let scenario = Scenario {
        seed: [0x81; 32],
        config: SmallMarketConfig::default(),
        actions: vec![
            Action::Deposit {
                actor: 0,
                amount: 17,
            },
            Action::Trade {
                route: TradeRoute::BatchCpi,
                taker: 0,
                maker: 2,
                asset: 2,
                units: 1,
                fee_bps: 17,
                price_move_bps: 4,
                prefer_reduce: false,
            },
            Action::PushMark {
                asset: 2,
                dt: 2,
                move_bps: -75,
            },
            Action::Crank {
                actor: 0,
                hints: HintMode::Complete,
            },
            Action::Trade {
                route: TradeRoute::Cpi,
                taker: 1,
                maker: 2,
                asset: 0,
                units: 1,
                fee_bps: u16::MAX,
                price_move_bps: 0,
                prefer_reduce: false,
            },
            Action::Withdraw {
                actor: 0,
                amount: 7,
            },
        ],
    };

    let coverage = run_scenario(&scenario).unwrap_or_else(|error| {
        panic!(
            "INV-081 deterministic public-route scenario failed\nscenario={}\n{error}",
            serde_json::to_string_pretty(&scenario).unwrap()
        )
    });

    assert_ne!(
        coverage.loaded_program_hash, [0; 32],
        "the deployed SBF artifact hash must be recorded"
    );
    assert!(
        coverage
            .route_success
            .iter()
            .all(|successes| *successes != 0),
        "all four public trade routes must have successful authorized deltas"
    );
    assert!(
        coverage.route_reject.iter().copied().sum::<u64>() != 0,
        "the over-policy trade rejection path must be exercised with exact rollback"
    );
    assert!(
        coverage.deposits != 0 && coverage.withdrawals != 0 && coverage.token_frame_checks != 0,
        "custody-changing routes must be checked against exact token frames"
    );
    assert!(
        coverage.crank_progress != 0,
        "permissionless crank progress must remain part of the complete public route oracle"
    );
    assert!(
        coverage.liquidation_steps != 0 && coverage.liquidated_abs_q != 0,
        "liquidation progress must remain part of the complete public route oracle"
    );
}
