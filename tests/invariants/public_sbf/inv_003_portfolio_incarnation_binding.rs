//! INV-003 - Portfolio incarnation binding.
//!
//! Normative obligation: Portfolio-scoped consent cannot cross close and same-pubkey recreation.
//!
//! Evidence in this file (I plus invariant-specific F/M assertions): `v16_program_pr309_stale_close_drains_replacement_account_lamports`, `v16_program_pr304_stale_matcher_grant_rejects_for_reinitialized_portfolio`, `v16_program_pr303_stale_trades_liquidate_reinitialized_portfolio`, `v16_program_pr301_stale_pnl_conversion_pays_cranker_from_replacement`, `v16_program_pr278_stale_forfeit_rejects_and_preserves_replacement_winner_payout`, `v16_program_pr299_stale_withdrawal_liquidates_reinitialized_portfolio`, `v16_program_pr305_stale_deposit_funds_reinitialized_portfolio_winner`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.
//! PR304 is fixed-pin coverage; the other named vulnerable routes retain their explicit status.

use super::*;

#[test]
fn v16_program_pr309_stale_close_drains_replacement_account_lamports() {
    let reproduction = reproduce_portfolio_close_incarnation_replay([0x09; 32])
        .unwrap_or_else(|error| panic!("PR 309 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::PortfolioCloseIncarnationReplay
    );
    assert!(reproduction.replacement_portfolio_id > reproduction.original_portfolio_id);
    assert_eq!(reproduction.drained_lamports, 1_000_000_000);
    assert_eq!(reproduction.market_lamport_gain, 1_000_000_000);
    assert!(reproduction.replay_cu < 1_400_000);
}

#[test]
fn v16_program_pr304_stale_matcher_grant_rejects_for_reinitialized_portfolio() {
    let protection = verify_matcher_grant_portfolio_incarnation_protection([0x04; 32])
        .unwrap_or_else(|error| panic!("PR 304 protection failed: {error}"));
    assert_eq!(
        protection.blocker,
        KnownBlocker::MatcherGrantPortfolioIncarnationReplay
    );
    assert!(protection.replacement_portfolio_id > protection.original_portfolio_id);
    assert!(protection.stale_replay_rejected);
    assert!(protection.rejected_exact_rollback);
    assert!(protection.control_trade_blocked);
    assert!(protection.fresh_grant_landed);
    assert!(protection.fresh_round_trip_landed);
    assert!(protection.owner_exit_landed);
    assert!(protection.max_cu < 1_400_000);
}

#[test]
fn v16_program_pr303_stale_trades_liquidate_reinitialized_portfolio() {
    for route in [
        TradeRoute::NoCpi,
        TradeRoute::Cpi,
        TradeRoute::BatchNoCpi,
        TradeRoute::BatchCpi,
    ] {
        for side in [
            PortfolioIncarnationTradeSide::AccountA,
            PortfolioIncarnationTradeSide::AccountB,
        ] {
            let reproduction =
                reproduce_trade_portfolio_incarnation_replay([0x03; 32], route, side)
                    .unwrap_or_else(|error| {
                        panic!("PR 303 {route:?}/{side:?} no longer reproduces: {error}")
                    });
            assert_eq!(
                reproduction.blocker,
                KnownBlocker::TradePortfolioIncarnationReplay
            );
            assert_eq!(reproduction.route, route);
            assert_eq!(reproduction.replacement_side, side);
            assert!(reproduction.replacement_portfolio_id > reproduction.original_portfolio_id);
            assert_eq!(reproduction.control_position_q, 0);
            assert!(reproduction.liquidation_slot > 0);
            assert_eq!(
                reproduction.cranker_reward,
                u128::from(reproduction.extracted_reward)
            );
            assert_eq!(reproduction.cranker_reward, 453);
            assert!(reproduction.replay_cu < 1_400_000);
            assert!(reproduction.max_cu < 1_400_000);
        }
    }
}

#[test]
fn v16_program_pr301_stale_pnl_conversion_pays_cranker_from_replacement() {
    let reproduction = reproduce_convert_portfolio_incarnation_replay([0x01; 32])
        .unwrap_or_else(|error| panic!("PR 301 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::ConvertPortfolioIncarnationReplay
    );
    assert!(reproduction.replacement_portfolio_id > reproduction.original_portfolio_id);
    assert_eq!(reproduction.released_pnl, 100);
    assert_eq!(reproduction.victim_loss, reproduction.cranker_extraction);
    assert_eq!(reproduction.victim_loss, 8);
    assert!(reproduction.replay_cu < 1_400_000);
    assert!(reproduction.sync_cu < 1_400_000);
    assert!(reproduction.max_cu < 1_400_000);
}

#[test]
fn v16_program_pr278_stale_forfeit_rejects_and_preserves_replacement_winner_payout() {
    let reproduction = reproduce_forfeit_portfolio_incarnation_replay([0x78; 32])
        .unwrap_or_else(|error| panic!("PR 278 fixed regression failed: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::ForfeitPortfolioIncarnationReplay
    );
    assert!(reproduction.replacement_portfolio_id > reproduction.original_portfolio_id);
    assert!(reproduction.stale_replay_rejected);
    assert!(reproduction.rejected_exact_rollback);
    assert_eq!(
        reproduction.replay_victim_payout,
        reproduction.control_victim_payout
    );
    assert_eq!(
        reproduction.replay_attacker_payout,
        reproduction.control_attacker_payout
    );
    assert_eq!(
        reproduction
            .replay_victim_payout
            .checked_add(reproduction.replay_attacker_payout),
        Some(2_000_000)
    );
    assert!(reproduction.control_slab_closed);
    assert!(reproduction.replay_slab_closed);
    assert!(reproduction.max_cu < 1_400_000);
}

#[test]
fn v16_program_pr299_stale_withdrawal_liquidates_reinitialized_portfolio() {
    let reproduction = reproduce_portfolio_incarnation_withdrawal([0x99; 32])
        .unwrap_or_else(|error| panic!("PR 299 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::PortfolioIncarnationWithdrawal
    );
    assert!(reproduction.new_portfolio_id > reproduction.old_portfolio_id);
    assert_eq!(reproduction.stale_withdrawal, 100_000_000);
    assert!(reproduction.restored_equity_surplus > 0);
    assert_eq!(
        reproduction.cranker_reward,
        u128::from(reproduction.extracted_reward)
    );
    assert!(reproduction.cranker_reward > 0);
    assert!(reproduction.replay_cu < 1_400_000);
}

#[test]
fn v16_program_pr305_stale_deposit_funds_reinitialized_portfolio_winner() {
    let reproduction = reproduce_portfolio_incarnation_deposit([0x05; 32])
        .unwrap_or_else(|error| panic!("PR 305 no longer reproduces: {error}"));
    assert_eq!(
        reproduction.blocker,
        KnownBlocker::PortfolioIncarnationDeposit
    );
    assert!(reproduction.new_portfolio_id > reproduction.old_portfolio_id);
    assert_eq!(reproduction.stale_deposit, 100_000);
    assert_eq!(reproduction.beneficiary_extra_payout, 100_000);
    assert_eq!(reproduction.control_winner_payout, 300_000);
    assert_eq!(reproduction.replay_winner_payout, 400_000);
    assert!(reproduction.replay_cu < 1_400_000);
}
