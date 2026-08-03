//! INV-045 - No free mark movement.
//!
//! Normative obligation: Every mark movement remains elapsed-time bounded and economically paid across every trade route.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_probe_ewma_fee_covers_large_passive_oi_moved_by_small_wash_trades`, `v16_attack_accumulated_ewma_lag_remains_fee_covered_after_crank`, `v16_attack_nocpi_positive_mark_min_fee_does_not_dos_or_move_ewma_for_free`, `v16_attack_cpi_matcher_price_caps_ewma_move_without_dos`, `v16_attack_underfunded_exit_cannot_move_ewma_with_uncollectible_fee`, `v16_attack_underfunded_cpi_exit_cannot_move_ewma_with_uncollectible_fee`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_probe_ewma_fee_covers_large_passive_oi_moved_by_small_wash_trades() {
    const MARK: u64 = 100;
    const VICTIM_Q: i128 = 100 * POS_SCALE as i128;
    const WASH_Q: i128 = POS_SCALE as i128;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: MARK,
        h_max: 20,
        max_trading_fee_bps: 10_000,
        max_price_move_bps_per_slot: 500,
        max_accrual_dt_slots: 20,
        min_funding_lifetime_slots: 20,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(1);
    env.configure_ewma_mark_with_cu(1, MARK, 1, 0);

    let attacker = Keypair::new();
    let victim = Keypair::new();
    let wash_long = Keypair::new();
    let wash_short = Keypair::new();
    let attacker_account = env.create_portfolio(&attacker);
    let victim_account = env.create_portfolio(&victim);
    let wash_long_account = env.create_portfolio(&wash_long);
    let wash_short_account = env.create_portfolio(&wash_short);
    for (owner, portfolio) in [
        (&attacker, attacker_account),
        (&victim, victim_account),
        (&wash_long, wash_long_account),
        (&wash_short, wash_short_account),
    ] {
        env.deposit(owner, portfolio, 10_000_000_000);
    }
    env.trade_asset_with_cu(
        0,
        &attacker,
        attacker_account,
        &victim,
        victim_account,
        VICTIM_Q,
        MARK,
        0,
    );
    let insurance_before = env.market_state().1.insurance;

    for slot in 2..=20u64 {
        env.svm.warp_to_slot(slot);
        let size_q = if slot % 2 == 0 { WASH_Q } else { -WASH_Q };
        env.svm.expire_blockhash();
        env.try_trade_asset_with_cu(
            0,
            &wash_long,
            wash_long_account,
            &wash_short,
            wash_short_account,
            size_q,
            MARK.checked_mul(slot).unwrap(),
            0,
        )
        .unwrap_or_else(|err| panic!("small wash trade at slot {slot} failed: {err}"));
    }

    let (cfg_before_crank, group_before_crank) = env.market_state();
    let fees_paid = group_before_crank.insurance - insurance_before;
    assert_eq!(group_before_crank.assets[0].effective_price, MARK);
    assert!(cfg_before_crank.mark_ewma_e6 > MARK);
    for portfolio in [attacker_account, victim_account] {
        env.svm.expire_blockhash();
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 20,
                observations: crank_observations(0),
            },
        );
    }

    let attacker_pnl = env.portfolio_state(attacker_account).pnl.get();
    assert!(
        attacker_pnl > 0,
        "wash prints must actually move the large book"
    );
    assert!(
        attacker_pnl as u128 <= fees_paid,
        "small wash fills must pay for the full passive-book transfer: pnl={attacker_pnl}, fees={fees_paid}"
    );
}

#[test]
fn v16_attack_accumulated_ewma_lag_remains_fee_covered_after_crank() {
    const MARK: u64 = 100;
    const BASE_Q: i128 = POS_SCALE as i128;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: MARK,
        h_max: 20,
        max_trading_fee_bps: 10_000,
        max_price_move_bps_per_slot: 500,
        max_accrual_dt_slots: 20,
        min_funding_lifetime_slots: 20,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(1);
    env.configure_ewma_mark_with_cu(1, MARK, 1, 0);

    let attacker = Keypair::new();
    let victim = Keypair::new();
    let wash_long = Keypair::new();
    let wash_short = Keypair::new();
    let attacker_account = env.create_portfolio(&attacker);
    let victim_account = env.create_portfolio(&victim);
    let wash_long_account = env.create_portfolio(&wash_long);
    let wash_short_account = env.create_portfolio(&wash_short);
    for (owner, portfolio) in [
        (&attacker, attacker_account),
        (&victim, victim_account),
        (&wash_long, wash_long_account),
        (&wash_short, wash_short_account),
    ] {
        env.deposit(owner, portfolio, 10_000_000_000);
    }

    env.trade_asset_with_cu(
        0,
        &attacker,
        attacker_account,
        &victim,
        victim_account,
        BASE_Q,
        MARK,
        0,
    );
    let insurance_before = env.market_state().1.insurance;

    for slot in 2..=20u64 {
        env.svm.warp_to_slot(slot);
        let size_q = if slot % 2 == 0 { BASE_Q } else { -BASE_Q };
        env.svm.expire_blockhash();
        env.try_trade_asset_with_cu(
            0,
            &wash_long,
            wash_long_account,
            &wash_short,
            wash_short_account,
            size_q,
            MARK.checked_mul(slot).unwrap(),
            0,
        )
        .unwrap_or_else(|err| panic!("wash trade at slot {slot} failed: {err}"));
    }

    let (cfg_before_crank, group_before_crank) = env.market_state();
    let fees_paid = group_before_crank.insurance - insurance_before;
    let victim_capital_before = env.portfolio_state(victim_account).capital.get();
    assert_eq!(group_before_crank.assets[0].effective_price, MARK);
    assert!(cfg_before_crank.mark_ewma_e6 > MARK);

    for portfolio in [attacker_account, victim_account] {
        env.svm.expire_blockhash();
        env.crank(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 20,
                observations: crank_observations(0),
            },
        );
    }

    let attacker_after = env.portfolio_state(attacker_account);
    let victim_after = env.portfolio_state(victim_account);
    let attacker_pnl = attacker_after.pnl.get();
    assert!(attacker_pnl > 0);
    assert_eq!(
        victim_after.pnl.get(),
        0,
        "the losing short settles its negative PnL"
    );
    assert_eq!(
        victim_capital_before - victim_after.capital.get(),
        attacker_pnl as u128,
        "the attacker's mark-created claim is the victim's realized capital loss"
    );
    assert!(
        attacker_pnl as u128 <= fees_paid,
        "accumulated mark-created claim must remain covered by wash fees: pnl={}, fees={fees_paid}",
        attacker_pnl
    );
}

#[test]
fn v16_attack_nocpi_positive_mark_min_fee_does_not_dos_or_move_ewma_for_free() {
    const MARK: u64 = 1_000_000;
    const CAP_BPS: u64 = 50;
    const MAX_FEE_BPS: u64 = 37;
    const TRADE_SLOT: u64 = 5;
    const SIZE_Q: i128 = (1000u128 * POS_SCALE) as i128;
    const HIGH_PRINT: u64 = MARK * 19 / 10;
    const MARK_MIN_FEE: u64 = 100_000_000;

    let accepted_price = oracle_v16::clamp_toward_engine_dt(MARK, HIGH_PRINT, CAP_BPS, 4);
    let candidate_mark = percolator_prog::policy_v16::ewma_update(
        MARK,
        accepted_price,
        1,
        1,
        TRADE_SLOT,
        MARK_MIN_FEE,
        MARK_MIN_FEE,
    );
    let candidate_move_bps = percolator_prog::policy_v16::price_move_bps_ceil(MARK, candidate_mark)
        .expect("candidate move bps");
    assert!(
        candidate_move_bps > MAX_FEE_BPS,
        "setup must make the full mark-min-fee EWMA candidate exceed the market fee cap"
    );

    for path in [
        NoCpiReportedPricePath::Single,
        NoCpiReportedPricePath::Batch,
    ] {
        let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
            initial_price: MARK,
            h_max: 20,
            max_trading_fee_bps: MAX_FEE_BPS,
            max_price_move_bps_per_slot: CAP_BPS,
            max_accrual_dt_slots: 20,
            min_funding_lifetime_slots: 20,
            ..V16CuMarketParams::default()
        });
        env.svm.warp_to_slot(1);
        env.configure_ewma_mark_with_cu(1, MARK, 1, MARK_MIN_FEE);
        env.svm.warp_to_slot(TRADE_SLOT);
        let (owner_a, account_a, owner_b, account_b) =
            funded_no_cpi_reported_price_pair(&mut env, 4_000_000_000);
        let insurance_before = env.market_state().1.insurance;

        env.svm.expire_blockhash();
        let trade = try_no_cpi_reported_price_trade_with_cu(
            &mut env, path, &owner_a, account_a, &owner_b, account_b, SIZE_Q, HIGH_PRINT, 0,
        );
        assert!(
            trade.is_ok(),
            "{path:?}: positive mark_min_fee must not DoS a valid off-mark trade: {trade:?}"
        );

        let (cfg, group) = env.market_state();
        let fee_paid = group.insurance - insurance_before;
        assert!(
            fee_paid > 0 && fee_paid < MARK_MIN_FEE as u128,
            "{path:?}: setup must pay a nonzero fee below mark_min_fee"
        );
        let mark_move_bps =
            percolator_prog::policy_v16::price_move_bps_ceil(MARK, cfg.mark_ewma_e6)
                .expect("actual mark move bps");
        assert_eq!(
            mark_move_bps, MAX_FEE_BPS,
            "{path:?}: mark movement should bind at paid fee headroom, not at mark_min_fee"
        );
        let trade_notional = SIZE_Q.unsigned_abs() * accepted_price as u128 / POS_SCALE;
        let paid_move_bps = fee_paid * 10_000 / (trade_notional * 2);
        assert!(
            mark_move_bps <= paid_move_bps as u64,
            "{path:?}: positive mark_min_fee EWMA move ({mark_move_bps} bps) must be paid by fees ({paid_move_bps} bps)"
        );
        assert_eq!(group.assets[0].oi_eff_long_q, SIZE_Q.unsigned_abs());
        assert_eq!(group.assets[0].oi_eff_short_q, SIZE_Q.unsigned_abs());
    }
}

#[test]
fn v16_attack_cpi_matcher_price_caps_ewma_move_without_dos() {
    for path in [CpiEwmaTradePath::Single, CpiEwmaTradePath::Batch] {
        assert_cpi_matcher_price_caps_paid_ewma_move(path, (1000u128 * POS_SCALE) as i128);
        assert_cpi_matcher_price_caps_paid_ewma_move(path, -((1000u128 * POS_SCALE) as i128));
        assert_cpi_matcher_price_caps_paid_ewma_move(path, 1);
        assert_cpi_matcher_price_caps_paid_ewma_move(path, -1);
    }
}

#[test]
fn v16_attack_underfunded_exit_cannot_move_ewma_with_uncollectible_fee() {
    for path in [
        NoCpiReportedPricePath::Single,
        NoCpiReportedPricePath::Batch,
    ] {
        assert_underfunded_ewma_exit_uses_collected_fee(path);
    }
}

#[test]
fn v16_attack_underfunded_cpi_exit_cannot_move_ewma_with_uncollectible_fee() {
    for path in [CpiEwmaTradePath::Single, CpiEwmaTradePath::Batch] {
        assert_underfunded_cpi_ewma_exit_uses_collected_fee(path);
    }
}
