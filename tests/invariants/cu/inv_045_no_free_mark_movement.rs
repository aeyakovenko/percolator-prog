//! INV-045 - No free mark movement.
//!
//! Normative obligation: Every mark movement remains elapsed-time bounded and economically paid across every trade route.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): wash-trade fee coverage,
//! no-CPI zero/epsilon/extreme reported-price normalization, CPI matcher quote pinning, same-slot
//! and per-slot EWMA circuit breakers, and underfunded exits that cannot move EWMA with an
//! uncollectible fee. A public 14-asset composition additionally pays the maximum elapsed-time
//! movement, refreshes both portfolios, enters DrainOnly, closes every leg at raw price one within
//! the SVM ceiling, converts the exact released PnL, and reconciles terminal custody. These tests
//! exercise the deployed public wrapper with real SBF/LiteSVM account construction and assert
//! economic state, token, rollback, liveness, or compute outcomes appropriate to the invariant.
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
        if slot > 2 {
            env.svm.expire_blockhash();
            env.crank(
                wash_long_account,
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
            );
            for portfolio in [attacker_account, victim_account] {
                let state = env.portfolio_state(portfolio);
                let leg = active_leg_for_asset(&state, 0);
                let asset = env.market_state().1.assets[0];
                let cohort_epoch = match leg.side {
                    SideV16::Long => asset.kf_epoch_long,
                    SideV16::Short => asset.kf_epoch_short,
                };
                if leg.kf_epoch_snap == cohort_epoch {
                    continue;
                }
                env.svm.expire_blockhash();
                env.crank(
                    portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: vec![],
                    },
                );
            }
        }
        env.svm.expire_blockhash();
        env.try_trade_asset_with_cu(
            0,
            &wash_long,
            wash_long_account,
            &wash_short,
            wash_short_account,
            WASH_Q,
            MARK.checked_mul(slot).unwrap(),
            0,
        )
        .unwrap_or_else(|err| panic!("small wash trade at slot {slot} failed: {err}"));
        // Target staging may block another risk increase, but cannot strand either side. Both
        // accounts can return to zero immediately while the just-published target is pending.
        env.svm.expire_blockhash();
        env.try_trade_asset_with_cu(
            0,
            &wash_long,
            wash_long_account,
            &wash_short,
            wash_short_account,
            -WASH_Q,
            MARK.checked_mul(slot).unwrap(),
            0,
        )
        .unwrap_or_else(|err| panic!("wash exit at slot {slot} failed: {err}"));
    }

    let (cfg_before_crank, group_before_crank) = env.market_state();
    let fees_paid = group_before_crank.insurance - insurance_before;
    assert!(group_before_crank.assets[0].effective_price > MARK);
    assert_eq!(
        cfg_before_crank.mark_ewma_e6, group_before_crank.assets[0].effective_price,
        "same-slot risk reduction must not leave a second uncommitted mark segment"
    );
    env.svm.warp_to_slot(21);
    for portfolio in [attacker_account, victim_account] {
        env.svm.expire_blockhash();
        env.crank_if_actionable(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 21,
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
fn v16_attack_repeated_ewma_moves_require_catchup_and_remain_fee_covered() {
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
        if slot > 2 {
            env.svm.expire_blockhash();
            env.crank(
                wash_long_account,
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: crank_observations(0),
                },
            );
            for portfolio in [attacker_account, victim_account] {
                let state = env.portfolio_state(portfolio);
                let leg = active_leg_for_asset(&state, 0);
                let asset = env.market_state().1.assets[0];
                let cohort_epoch = match leg.side {
                    SideV16::Long => asset.kf_epoch_long,
                    SideV16::Short => asset.kf_epoch_short,
                };
                if leg.kf_epoch_snap == cohort_epoch {
                    continue;
                }
                env.svm.expire_blockhash();
                env.crank(
                    portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: vec![],
                    },
                );
            }
        }
        env.svm.expire_blockhash();
        env.try_trade_asset_with_cu(
            0,
            &wash_long,
            wash_long_account,
            &wash_short,
            wash_short_account,
            BASE_Q,
            MARK.checked_mul(slot).unwrap(),
            0,
        )
        .unwrap_or_else(|err| panic!("wash trade at slot {slot} failed: {err}"));
        env.svm.expire_blockhash();
        env.try_trade_asset_with_cu(
            0,
            &wash_long,
            wash_long_account,
            &wash_short,
            wash_short_account,
            -BASE_Q,
            MARK.checked_mul(slot).unwrap(),
            0,
        )
        .unwrap_or_else(|err| panic!("wash exit at slot {slot} failed: {err}"));
    }

    let (cfg_before_crank, group_before_crank) = env.market_state();
    let fees_paid = group_before_crank.insurance - insurance_before;
    let victim_capital_before = env.portfolio_state(victim_account).capital.get();
    assert!(group_before_crank.assets[0].effective_price > MARK);
    assert_eq!(
        cfg_before_crank.mark_ewma_e6,
        group_before_crank.assets[0].effective_price
    );

    env.svm.warp_to_slot(21);
    for portfolio in [attacker_account, victim_account] {
        env.svm.expire_blockhash();
        env.crank_if_actionable(
            portfolio,
            ProgInstruction::PermissionlessCrank {
                now_slot: 21,
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
        victim_after.capital.get(),
        victim_capital_before,
        "fee-backed mark movement must not debit the passive counterparty's principal"
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

// Attacker criterion (must FAIL): declaring exec_price << mark charges a smaller fee than the mark.
#[test]
fn v16_program_tradenocpi_fee_cannot_be_evaded_via_exec_price() {
    // fee charged (insurance delta) for the SAME mark-valued position at different declared exec_prices.
    let fee_for = |exec_price: u64| -> u128 {
        let mut env = V16CuEnv::new();
        env.configure_auth_mark_with_cu(0, 100); // mark = 100
        let oa = Keypair::new();
        let a = env.create_portfolio(&oa);
        let ob = Keypair::new();
        let b = env.create_portfolio(&ob);
        env.deposit(&oa, a, 100_000_000_000);
        env.deposit(&ob, b, 100_000_000_000);
        let ins0 = env.market_state().1.insurance;
        env.trade_asset_with_cu(
            0,
            &oa,
            a,
            &ob,
            b,
            (1000 * POS_SCALE) as i128,
            exec_price,
            100,
        );
        env.market_state().1.insurance - ins0
    };
    let fee_at_mark = fee_for(100);
    assert!(
        fee_at_mark > 0,
        "non-vacuous: a real fee is charged at the mark"
    );
    // The whole point of the fix: a tiny declared exec_price does NOT reduce the fee anymore.
    assert_eq!(
        fee_for(1),
        fee_at_mark,
        "exec_price=1 is billed the SAME mark-based fee (no evasion)"
    );
    assert_eq!(
        fee_for(50),
        fee_at_mark,
        "exec_price below mark is billed the mark-based fee"
    );
    // Declaring a HIGH exec_price must not over-bill either — fee is pinned to the mark, not the caller.
    assert_eq!(
        fee_for(100_000),
        fee_at_mark,
        "exec_price above mark is also billed the mark-based fee"
    );
}

// reported price used to bypass the per-slot clamp and move the mark toward zero via a wash trade.
#[test]
fn v16_program_nocpi_zero_reported_price_cannot_drive_ewma_or_hybrid_mark() {
    for path in [
        NoCpiReportedPricePath::Single,
        NoCpiReportedPricePath::Batch,
    ] {
        assert_zero_reported_price_rejects_atomically(
            zero_reported_price_ewma_env(),
            path,
            "EWMA mark",
        );
        assert_zero_reported_price_rejects_atomically(
            zero_reported_price_hybrid_after_hours_env(),
            path,
            "Hybrid after-hours mark",
        );
    }
}

// different mark than the charged price justified.
#[test]
fn v16_program_nocpi_epsilon_reported_price_uses_dt_clamped_fee_and_ewma_price() {
    const MARK: u64 = 1_000_000;
    const CAP_BPS: u64 = 50;
    const DT: u64 = 4;
    let dt_clamped_epsilon = oracle_v16::clamp_toward_engine_dt(MARK, 1, CAP_BPS, DT);
    assert_eq!(
        dt_clamped_epsilon, 980_000,
        "test setup must exercise a multi-slot engine dt clamp"
    );

    for path in [
        NoCpiReportedPricePath::Single,
        NoCpiReportedPricePath::Batch,
    ] {
        let epsilon = ewma_no_cpi_fee_and_mark_for_reported_price(path, 1);
        let clamped = ewma_no_cpi_fee_and_mark_for_reported_price(path, dt_clamped_epsilon);
        let at_mark = ewma_no_cpi_fee_and_mark_for_reported_price(path, MARK);

        assert_eq!(
            epsilon, clamped,
            "{path:?}: epsilon report must be normalized to the accepted dt-clamped price for fee and EWMA"
        );
        assert_ne!(
            epsilon.0, at_mark.0,
            "{path:?}: non-vacuous fee assertion; the accepted below-mark print must not be billed as old mark"
        );
        assert_ne!(
            epsilon.1, at_mark.1,
            "{path:?}: non-vacuous EWMA assertion; the accepted below-mark print must move the mark"
        );
        assert_eq!(
            at_mark.1, MARK,
            "{path:?}: at-mark control should not move EWMA"
        );
        assert!(
            epsilon.1 < at_mark.1,
            "{path:?}: lower accepted print should move EWMA below at-mark control"
        );
    }
}

// unit against this price. The trade must still execute, but the unpaid print must move no EWMA.
#[test]
fn v16_program_nocpi_trade_not_dosed_by_extreme_reported_price() {
    for path in [
        NoCpiReportedPricePath::Single,
        NoCpiReportedPricePath::Batch,
    ] {
        for reported_price in [1, percolator::MAX_ORACLE_PRICE] {
            assert_no_cpi_tiny_exit_accepts_extreme_reported_price(path, reported_price);
            assert_no_cpi_tiny_open_accepts_extreme_reported_price(path, reported_price);
        }
    }
}

// is reduced to what the capped fee actually pays for.
#[test]
fn v16_program_nocpi_extreme_price_caps_ewma_move_without_dos() {
    for path in [
        NoCpiReportedPricePath::Single,
        NoCpiReportedPricePath::Batch,
    ] {
        for reported_price in [1, percolator::MAX_ORACLE_PRICE] {
            assert_no_cpi_extreme_reported_price_caps_paid_ewma_move(path, reported_price);
        }
    }
}

// the fee is computed on the mark, so the matcher's price-quoting knobs (spread) cannot change the fee.
#[test]
fn v16_program_tradecpi_fee_is_mark_pinned_not_matcher_quoted() {
    let fee_for_spread = |base_spread: u32, max_total: u32| -> u128 {
        let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
            trade_fee_base_bps: 100,
            ..V16CuMarketParams::default()
        });
        let matcher_program = Pubkey::new_unique();
        let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
        env.svm.add_program(matcher_program, &matcher_bytes);
        let to = Keypair::new();
        let t = env.create_portfolio(&to);
        let mo = Keypair::new();
        let m = env.create_portfolio(&mo);
        env.deposit(&to, t, 100_000_000);
        env.deposit(&mo, m, 100_000_000);
        let (ctx, del, _) = env.init_matcher_context_with_passive_spread_authorized(
            matcher_program,
            &mo,
            m,
            base_spread,
            max_total,
        );
        let ins0 = env.market_state().1.insurance;
        let _ = env.trade_cpi_with_cu_on_asset(
            &to,
            t,
            &mo,
            m,
            matcher_program,
            ctx,
            del,
            0,
            (10 * POS_SCALE) as i128,
            100,
        );
        // entry is at the mark (effective_price==100) regardless of the matcher's spread.
        assert_eq!(
            env.market_state().1.assets[0].effective_price,
            100,
            "position priced at the mark"
        );
        assert_eq!(
            env.portfolio_state(t).pnl.get(),
            0,
            "no off-market PnL from the matcher quote"
        );
        env.market_state().1.insurance - ins0
    };
    // fee at the mark (no spread): 10*POS_SCALE @ 100 = notional 1000 -> 100bps -> 10/side -> 20 total.
    let fee_no_spread = fee_for_spread(0, 100);
    assert_eq!(fee_no_spread, 20, "baseline CPI fee is the mark-based fee");
    // a WIDE matcher spread (20%) charges the IDENTICAL fee -> the fee is on the mark, NOT the matcher's
    // (potentially manipulated) exec_price. A malicious matcher cannot under-bill the CPI fee.
    assert_eq!(
        fee_for_spread(2_000, 4_000),
        fee_no_spread,
        "matcher spread does not change the CPI fee (mark-pinned)"
    );
    assert_eq!(
        fee_for_spread(500, 1_000),
        fee_no_spread,
        "fee invariant to a moderate spread too"
    );
}

// per-slot cap. Complements #10533 with the EWMA mode.
#[test]
fn v16_program_ewma_mark_respects_per_slot_circuit_breaker() {
    const INITIAL_PRICE: u64 = 1_000_000;
    const MOVE_BPS: u64 = 1_000; // 10% / slot circuit breaker
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: INITIAL_PRICE,
        max_price_move_bps_per_slot: MOVE_BPS,
        max_accrual_dt_slots: 1,
        max_abs_funding_e9_per_slot: 1_000,
        min_funding_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(0);
    // EWMA mark mode with an aggressive halflife (1 slot) so the smoothed target races toward any push.
    env.configure_ewma_mark_with_cu(0, INITIAL_PRICE, 1, 0);
    let lo_owner = Keypair::new();
    let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new();
    let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, 50_000_000);
    env.deposit(&sh_owner, sh, 50_000_000);
    // open matched OI so equity is active (the move gate at v16.rs:8060 only fires when equity_active).
    env.trade_asset_with_cu(
        0,
        &lo_owner,
        lo,
        &sh_owner,
        sh,
        POS_SCALE as i128,
        INITIAL_PRICE,
        0,
    );

    let mut prev_price = INITIAL_PRICE;
    // push the EWMA mark to 100x EVERY slot and crank; the effective price must never jump past the cap.
    for slot in 1..=4u64 {
        env.svm.warp_to_slot(slot);
        env.push_ewma_mark_with_cu(slot, INITIAL_PRICE * 100); // 100,000,000 — 100x
        env.svm.expire_blockhash();
        // crank may succeed (clamped move) or be refused (RecoveryRequired) — either way price is bounded.
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(lo, false),
            ],
            &[],
        );
        let g = env.market_state().1;
        let ep = g.assets[0].effective_price;
        // PER-SLOT CAP: the move from the previous settled price is at most MOVE_BPS (10%); a small
        // tolerance covers rounding in the EWMA target. The mark authority CANNOT jump the price 100x.
        let cap = prev_price + prev_price * (MOVE_BPS + 1) / 10_000;
        assert!(
            ep <= cap,
            "slot {}: effective price {} clamped to <= per-slot cap {} (prev {})",
            slot,
            ep,
            cap,
            prev_price
        );
        prev_price = ep;
        assert!(
            g.vault >= g.c_tot + g.insurance,
            "senior conservation under clamped EWMA move"
        );
        assert_eq!(
            g.vault as u64,
            env.token_amount(env.vault),
            "accounting vault == real on-chain vault"
        );
    }
    // NON-VACUITY + the headline: after FOUR slots of pushing to 100,000,000, the effective settlement
    // price is still nowhere near it — the EWMA mode did NOT bypass the circuit breaker.
    let final_price = env.market_state().1.assets[0].effective_price;
    assert!(final_price < INITIAL_PRICE * 2, "after 4 clamped slots the price ({}) is far below the 100x push (circuit breaker held across EWMA mode)", final_price);
    // NON-VACUITY: the price DID move toward the push (the clamp throttled a real move, it wasn't a no-op).
    assert!(
        final_price > INITIAL_PRICE,
        "the EWMA mark actually moved the price (clamped, not frozen): {} > {}",
        final_price,
        INITIAL_PRICE
    );
}

// hyperp-index Bug #9 fix, which has its own test.)
#[test]
fn v16_program_push_ewma_mark_same_slot_does_not_compound() {
    let mut env = V16CuEnv::new();
    env.configure_ewma_mark_with_cu(1, 100, 10, 0); // asset 0 EWMA, mark 100, halflife 10
    env.svm.warp_to_slot(5);
    env.svm.expire_blockhash();
    env.push_ewma_mark_with_cu(5, 1000); // push toward 1000 -> partial (alpha) move, not a jump
    let m1 = env.market_state().0.mark_ewma_e6;
    assert!(
        m1 > 100 && m1 < 1000,
        "first push moves EWMA partially toward target (no instant jump): {m1}"
    );
    // SECOND push, SAME slot, same target: dt==0 -> must NOT move (no compounding).
    env.svm.expire_blockhash();
    env.push_ewma_mark_with_cu(5, 1000);
    let m2 = env.market_state().0.mark_ewma_e6;
    assert_eq!(
        m2, m1,
        "same-slot repeated PushEwmaMark must not compound (dt==0 -> no movement)"
    );
    // a genuine slot advance resumes movement (time-gated, not frozen).
    env.svm.warp_to_slot(6);
    env.svm.expire_blockhash();
    env.push_ewma_mark_with_cu(6, 1000);
    let m3 = env.market_state().0.mark_ewma_e6;
    assert!(
        m3 > m2,
        "advancing a slot resumes EWMA movement: {m3} vs {m2}"
    );
}

// security.md sweep — oracle/mark bounds (#37/#39): the auth-mark push feeds settlement. An extreme
// mark (0 or u64::MAX) must be rejected/clamped, never corrupt pnl or panic the program.
#[test]
fn v16_attack_extreme_auth_mark_push_rejected_or_safe() {
    let mut env = V16CuEnv::new();
    env.configure_auth_mark_with_cu(0, 100);
    let lo_owner = Keypair::new();
    let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new();
    let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, 1_000_000);
    env.deposit(&sh_owner, sh, 1_000_000);
    env.trade_asset_with_cu(0, &lo_owner, lo, &sh_owner, sh, POS_SCALE as i128, 100, 0);
    env.svm.warp_to_slot(5);
    // push extreme marks; each must reject or be clamped — never panic, never corrupt state.
    for mark in [0u64, 1, u64::MAX, u64::MAX / 2] {
        env.svm.expire_blockhash();
        let _ = send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::PushAuthMark {
                market_id: 0,
                observation_sequence: u64::MAX,
                asset_index: 0,
                now_slot: 5,
                mark_e6: mark,
            },
            vec![
                AccountMeta::new(env.admin.pubkey(), true),
                AccountMeta::new(env.market, false),
            ],
            &[&env.admin],
        ); // ignore Err; we only require no panic + conservation
           // crank against whatever mark landed; must not corrupt conservation.
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: 5,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(lo, false),
            ],
            &[],
        );
        let (_, g) = env.market_state();
        assert_eq!(
            g.vault, 2_000_000,
            "vault intact under extreme mark {}",
            mark
        );
        assert!(
            g.vault >= g.c_tot + g.insurance,
            "senior conservation under extreme mark {}",
            mark
        );
        assert!(
            g.assets[0].effective_price > 0
                && g.assets[0].effective_price <= percolator::MAX_ORACLE_PRICE,
            "effective price stays in valid bounds under extreme mark {} (got {})",
            mark,
            g.assets[0].effective_price
        );
    }
    // state still decodes and positions intact (no corruption). Vault holds exactly the deposits
    // (checked per-iteration); accounted equity + insurance never EXCEEDS the vault (the small
    // difference is the in-vault §6.2 residual buffer, not lost value).
    let a = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    let b = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
    let accounted = (a.capital.get() as i128 + a.pnl.get())
        + (b.capital.get() as i128 + b.pnl.get())
        + env.market_state().1.insurance as i128;
    assert!(
        accounted <= 2_000_000,
        "no value created by extreme mark pushes (accounted {})",
        accounted
    );
    assert!(
        accounted >= 2_000_000 - 1_000,
        "value not materially destroyed; remainder is in-vault residual (accounted {})",
        accounted
    );
}

// security.md sweep — circuit breaker on mark push (#9 oracle manipulation): a push to a far-away
// mark must move the effective price by at most max_price_move_bps_per_slot per slot. An attacker
// (mark authority) cannot jump the settlement price arbitrarily in one slot.
#[test]
fn v16_attack_mark_push_clamped_per_slot() {
    let mut env = V16CuEnv::new(); // max_price_move_bps_per_slot = 10_000 (100%/slot)
    env.configure_auth_mark_with_cu(0, 100);
    let lo_owner = Keypair::new();
    let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new();
    let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, 1_000_000);
    env.deposit(&sh_owner, sh, 1_000_000);
    env.trade_asset_with_cu(0, &lo_owner, lo, &sh_owner, sh, POS_SCALE as i128, 100, 0);
    let mut prev_price = 100u64;
    for slot in [10u64, 11, 12] {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_with_cu(slot, 1_000_000); // push to a huge mark (10000x) every slot
        let _ = env.send_crank_if_actionable(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(lo, false),
            ],
            &[],
        );
        let (_, g) = env.market_state();
        let ep = g.assets[0].effective_price;
        // the per-slot move is clamped to <= 100% (price at most doubles per slot).
        assert!(
            ep <= prev_price * 2,
            "slot {}: effective price {} clamped to <= 2x prev {} (circuit breaker)",
            slot,
            ep,
            prev_price
        );
        assert!(
            ep > prev_price,
            "slot {}: price moved toward the pushed mark (non-vacuous)",
            slot
        );
        prev_price = ep;
        assert!(
            g.vault >= g.c_tot + g.insurance,
            "senior conservation under clamped move"
        );
    }
    // even after 3 slots of pushing to 1,000,000, the effective price is nowhere near it (clamped).
    let (_, g) = env.market_state();
    assert!(
        g.assets[0].effective_price <= 800,
        "after 3 clamped slots, price is far below the 1,000,000 push (got {})",
        g.assets[0].effective_price
    );
}

// security.md sweep — EWMA mark halflife edge (#37): configuring the EWMA mark with halflife 0
// (instant) must be handled cleanly — no div-by-zero/panic, no settlement corruption. The mark/price
// stays in valid bounds and conservation holds.
#[test]
fn v16_attack_ewma_mark_halflife_zero_safe() {
    let mut env = V16CuEnv::new();
    // configure with halflife = 0 (instant). If accepted, settlement must stay safe.
    let r = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureEwmaMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 0,
            initial_mark_e6: 100,
            mark_ewma_halflife_slots: 0,
            mark_min_fee: 0,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&env.admin],
    );
    let lo = Keypair::new();
    let plo = env.create_portfolio(&lo);
    let sh = Keypair::new();
    let psh = env.create_portfolio(&sh);
    env.deposit(&lo, plo, 1_000_000);
    env.deposit(&sh, psh, 1_000_000);
    env.trade_with_cu(&lo, plo, &sh, psh, POS_SCALE as i128, 100, 0);
    // if the halflife=0 config was accepted, push a mark and crank — must not panic/corrupt.
    if r.is_ok() {
        env.svm.warp_to_slot(1);
        env.push_ewma_mark_with_cu(1, 150);
        for slot in [1u64, 2] {
            env.svm.warp_to_slot(slot);
            for p in [psh, plo] {
                let _ = env.send_crank_if_actionable(
                    ProgInstruction::PermissionlessCrank {
                        now_slot: slot,
                        observations: crank_observations(0),
                    },
                    vec![
                        AccountMeta::new(env.payer.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(p, false),
                    ],
                    &[],
                );
            }
        }
    }
    // regardless of accept/reject: no corruption, state decodes, conservation holds, price in bounds.
    let (_, g) = env.market_state();
    assert_eq!(g.vault, 2_000_000, "vault intact");
    assert_eq!(
        g.vault as u64,
        env.token_amount(env.vault),
        "accounting == real vault"
    );
    assert!(g.vault >= g.c_tot + g.insurance, "senior conservation");
    assert!(
        g.assets[0].effective_price > 0
            && g.assets[0].effective_price <= percolator::MAX_ORACLE_PRICE,
        "price in valid bounds (no corruption)"
    );
    let _ = r;
}

// security.md sweep — EWMA-mark mode respects the per-slot circuit breaker (#6/#9/#37): the auth-mark
// test (#10533) proves AUTH_MARK clamps to <= max_price_move_bps_per_slot; EWMA_MARK is a SEPARATE
// pricing mode (halflife smoothing toward a pushed mark) and could, if the clamp lived only in the
// auth-mark path, let the mark authority move the effective settlement price arbitrarily fast in one
// slot. Attacker goal: as mark authority, push the EWMA mark to 100x and have the NEXT crank settle the
// effective price (used for liquidation/funding/PnL) at the full 100x in a single slot. Protection: the
// per-slot move gate is mode-INDEPENDENT — it lives in accrue_asset_to_not_atomic (percolator/src/v16.rs:
// 8052: price_diff*MAX_MARGIN_BPS <= max_price_move_bps_per_slot*segment_dt*old_price, else RecoveryRequired),
// so an EWMA push beyond the budget either clamps or is refused; the effective price cannot jump past the
// security.md sweep — crank dt-clamp prevents retroactive one-shot settlement after a long gap (#9/#22):
// the per-crank segment is clamped to max_accrual_dt_slots (percolator/src/v16.rs:8037), and a crank
// advances slot_last by only that clamped segment (v16.rs:8089), NOT all the way to now_slot. This dt is
// the multiplier on EVERY time-scaled accrual (funding: v16.rs:8072 funding_rate*segment_dt*price; and the
// price-move budget: v16.rs:8057 cap*segment_dt). Attacker goal: leave the market un-cranked for a long
// time so a large accrual window builds up, then fire ONE crank to retroactively settle the FULL elapsed
// window in a single shot — a one-block funding windfall to the favoured side, or a surprise margin blow-up
// of the other side that skips the per-slot price-move circuit breaker. Protection: each crank settles at
// most max_accrual_dt_slots; slot_last creeps forward by the clamp per crank, so catch-up is bounded and
// many cranks (each itself re-clamped) are required to close the gap — no one-shot retroactive settlement.
// (Not covered by the conservation tests, which all crank every slot so dt is always 1.)
#[test]
fn v16_attack_crank_dt_clamp_blocks_retroactive_settle() {
    const INITIAL_PRICE: u64 = 1_000_000;
    const DT_CLAMP: u64 = 3; // max_accrual_dt_slots
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: INITIAL_PRICE,
        max_price_move_bps_per_slot: 1_000,
        max_accrual_dt_slots: DT_CLAMP,
        max_abs_funding_e9_per_slot: 1_000,
        min_funding_lifetime_slots: DT_CLAMP, // lifetime >= accrual window (config validity)
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(0);
    env.configure_ewma_mark_with_cu(0, INITIAL_PRICE, 1, 0);
    let lo_owner = Keypair::new();
    let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new();
    let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, 50_000_000);
    env.deposit(&sh_owner, sh, 50_000_000);
    env.trade_asset_with_cu(
        0,
        &lo_owner,
        lo,
        &sh_owner,
        sh,
        POS_SCALE as i128,
        INITIAL_PRICE,
        0,
    );
    let slot_last_0 = env.market_state().1.assets[0].slot_last;
    assert_eq!(
        slot_last_0, 0,
        "asset starts settled at slot 0 (open position is live)"
    );

    // ATTACK: leave the market UNCRANKED for a huge window, then fire ONE crank that tries to settle it all.
    const GAP_SLOT: u64 = 500;
    env.svm.warp_to_slot(GAP_SLOT);
    env.svm.expire_blockhash();
    let r = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: GAP_SLOT,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(lo, false),
        ],
        &[],
    );
    assert!(r.is_ok(), "the catch-up crank itself succeeds: {:?}", r);

    let g1 = env.market_state().1;
    // ANTI-RETROACTIVITY (the headline): the settled segment is the dt CLAMP (3), NOT the 500-slot gap.
    // A naive retroactive settle would jump slot_last to 500 and apply ~166x the funding/move budget in one
    // block; the clamp holds the advance to exactly max_accrual_dt_slots.
    assert_eq!(
        g1.assets[0].slot_last,
        slot_last_0 + DT_CLAMP,
        "one crank settles only max_accrual_dt_slots ({}), NOT the full {}-slot gap (slot_last={})",
        DT_CLAMP,
        GAP_SLOT,
        g1.assets[0].slot_last
    );
    assert!(GAP_SLOT > 50 * DT_CLAMP, "non-vacuous: the elapsed gap ({}) dwarfs the clamp ({}) — the clamp is the binding constraint", GAP_SLOT, DT_CLAMP);
    assert_eq!(
        g1.current_slot, GAP_SLOT,
        "header current_slot advances to now (the asset is intentionally left 'behind' by design)"
    );
    // conservation: whatever was settled is internal redistribution; nothing minted.
    assert_eq!(
        g1.vault, 100_000_000,
        "vault unchanged (settlement mints nothing)"
    );
    assert_eq!(
        g1.vault as u64,
        env.token_amount(env.vault),
        "accounting vault == real on-chain vault"
    );
    assert!(
        g1.vault >= g1.c_tot + g1.insurance,
        "senior conservation under the clamped catch-up"
    );

    // A SECOND crank at the same now_slot advances slot_last by ANOTHER clamp window — catch-up is bounded
    // PER crank, so closing a 500-slot gap needs ~167 separately-clamped cranks, each re-subject to the
    // per-slot price-move circuit breaker. There is no single transaction that settles the whole window.
    env.svm.expire_blockhash();
    let r2 = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: GAP_SLOT,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(lo, false),
        ],
        &[],
    );
    assert!(r2.is_ok(), "second catch-up crank succeeds: {:?}", r2);
    let g2 = env.market_state().1;
    assert_eq!(
        g2.assets[0].slot_last,
        slot_last_0 + 2 * DT_CLAMP,
        "second crank advances another clamp window (bounded per-crank catch-up)"
    );
    assert!(g2.assets[0].slot_last < GAP_SLOT, "even after two cranks the asset is still far short of the gap (catch-up is throttled, not instant)");
    assert!(
        g2.vault >= g2.c_tot + g2.insurance,
        "senior conservation after second crank"
    );
}

fn configure_max_shape_ewma_asset(
    env: &mut V16CuEnv,
    asset_index: u16,
    now_slot: u64,
    mark_e6: u64,
) {
    let observation_sequence = next_control_sequence(
        env.control_sequences(asset_index as usize)
            .oracle_observation,
    );
    let market_id = env.asset_market_id(asset_index);
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureEwmaMark {
            market_id,
            asset_index,
            now_slot,
            initial_mark_e6: mark_e6,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            observation_sequence,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&env.admin],
    )
    .expect("configure maximum-shape EWMA asset");
}

#[test]
fn v16_program_max_shape_ewma_movement_is_paid_and_drain_only_exit_stays_bounded() {
    const ASSET_COUNT: u16 = percolator_prog::constants::WRAPPER_MAX_PORTFOLIO_ASSETS;
    const MARK: u64 = 1_000_000;
    const RAW_UP: u64 = 2_000_000;
    const DEPOSIT: u128 = 25_000_000;
    const EXPECTED_MARK: u64 = 1_005_000;
    const EXPECTED_FEE_PER_ASSET: u128 = 10_100;
    const EXPECTED_RELEASED_PNL_PER_ASSET: u128 = 5_000;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        max_portfolio_assets: ASSET_COUNT,
        initial_price: MARK,
        max_trading_fee_bps: 100,
        max_price_move_bps_per_slot: 100,
        max_accrual_dt_slots: 1,
        ..V16CuMarketParams::default()
    });
    for asset_index in 0..ASSET_COUNT {
        configure_max_shape_ewma_asset(&mut env, asset_index, 0, MARK);
    }

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, DEPOSIT);
    env.deposit(&short_owner, short, DEPOSIT);
    let vault_before = env.token_amount(env.vault);

    env.svm.warp_to_slot(1);
    let open_legs = (0..ASSET_COUNT)
        .map(|asset_index| BatchTradeLeg {
            asset_index,
            market_id: env.asset_market_id(asset_index),
            size_q: POS_SCALE as i128,
            exec_price: RAW_UP,
            fee_bps: 0,
        })
        .collect();
    env.svm.expire_blockhash();
    let open_cu = env
        .send(
            env.batch_trade_no_cpi_ix(long, short, open_legs),
            vec![
                AccountMeta::new(long_owner.pubkey(), true),
                AccountMeta::new(short_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(long, false),
                AccountMeta::new(short, false),
            ],
            &[&long_owner, &short_owner],
        )
        .expect("maximum-shape paid EWMA batch");

    let after_open = env.market_state().1;
    assert_eq!(after_open.vault as u64, vault_before);
    assert_eq!(
        after_open.insurance,
        EXPECTED_FEE_PER_ASSET * u128::from(ASSET_COUNT)
    );
    assert_eq!(
        after_open.insurance_domain_budget.iter().sum::<u128>(),
        0,
        "trade-driven mark fees remain nonwithdrawable"
    );
    assert_eq!(after_open.c_tot + after_open.insurance, after_open.vault);
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&env.portfolio_state(long))),
        u32::from(ASSET_COUNT)
    );
    for asset_index in 0..ASSET_COUNT {
        let profile = state::read_asset_oracle_profile(
            &env.svm.get_account(&env.market).unwrap().data,
            asset_index as usize,
        )
        .unwrap();
        assert_eq!(profile.mark_ewma_e6, EXPECTED_MARK);
        assert_eq!(profile.oracle_target_price_e6, EXPECTED_MARK);
        assert_eq!(
            after_open.assets[asset_index as usize].raw_oracle_target_price,
            EXPECTED_MARK
        );
        assert_eq!(
            after_open.assets[asset_index as usize].effective_price,
            MARK
        );
    }

    let asset_indices = (0..ASSET_COUNT).collect::<Vec<_>>();
    env.svm.expire_blockhash();
    let refresh_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 1,
                observations: crank_observations_for_assets(&asset_indices),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(long, false),
            ],
            &[],
        )
        .expect("maximum-shape paid EWMA refresh");
    let after_refresh = env.market_state().1;
    for asset_index in 0..ASSET_COUNT as usize {
        assert_eq!(
            after_refresh.assets[asset_index].effective_price,
            EXPECTED_MARK
        );
        assert_eq!(
            after_refresh.assets[asset_index].raw_oracle_target_price,
            EXPECTED_MARK
        );
    }

    for asset_index in 0..ASSET_COUNT {
        env.update_asset_lifecycle_as_admin_with_cu(
            processor::ASSET_ACTION_DRAIN_ONLY,
            asset_index,
            0,
            0,
        );
    }
    let cert_current = |env: &V16CuEnv, portfolio: Pubkey| {
        let group = env.market_state().1;
        let account = env.portfolio_state(portfolio);
        let cert = health_cert(&account);
        cert.valid
            && cert.cert_oracle_epoch == group.oracle_epoch
            && cert.cert_funding_epoch == group.funding_epoch
            && cert.cert_risk_epoch == group.risk_epoch
            && cert.cert_asset_set_epoch == group.asset_set_epoch
            && cert.active_bitmap_at_cert == active_bitmap(&account)
    };
    let mut drain_refresh_cu = 0;
    for _ in 0..(u32::from(ASSET_COUNT) * 2 + 4) {
        for portfolio in [long, short] {
            if !cert_current(&env, portfolio) {
                drain_refresh_cu = drain_refresh_cu.max(env.crank(
                    portfolio,
                    ProgInstruction::PermissionlessCrank {
                        now_slot: 1,
                        observations: vec![],
                    },
                ));
            }
        }
        if cert_current(&env, long) && cert_current(&env, short) {
            break;
        }
    }
    assert!(
        cert_current(&env, long) && cert_current(&env, short),
        "permissionless refresh must reach a two-account certificate fixed point"
    );
    let fee_stock_before_exit = env.market_state().1.insurance;
    let long_capital_before_exit = env.portfolio_state(long).capital.get();
    let short_capital_before_exit = env.portfolio_state(short).capital.get();
    let close_legs = (0..ASSET_COUNT)
        .map(|asset_index| BatchTradeLeg {
            asset_index,
            market_id: env.asset_market_id(asset_index),
            size_q: -(POS_SCALE as i128),
            exec_price: 1,
            fee_bps: 0,
        })
        .collect();
    env.svm.expire_blockhash();
    let close_cu = env
        .send(
            env.batch_trade_no_cpi_ix(long, short, close_legs),
            vec![
                AccountMeta::new(long_owner.pubkey(), true),
                AccountMeta::new(short_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(long, false),
                AccountMeta::new(short, false),
            ],
            &[&long_owner, &short_owner],
        )
        .expect("maximum-shape DrainOnly extreme-price reduction");

    let after_close = env.market_state().1;
    assert_eq!(after_close.insurance, fee_stock_before_exit);
    assert_eq!(
        env.portfolio_state(long).capital.get(),
        long_capital_before_exit
    );
    assert_eq!(
        env.portfolio_state(short).capital.get(),
        short_capital_before_exit
    );
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&env.portfolio_state(long))),
        0
    );
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&env.portfolio_state(short))),
        0
    );
    for asset_index in 0..ASSET_COUNT as usize {
        assert_eq!(after_close.assets[asset_index].oi_eff_long_q, 0);
        assert_eq!(after_close.assets[asset_index].oi_eff_short_q, 0);
        let profile = state::read_asset_oracle_profile(
            &env.svm.get_account(&env.market).unwrap().data,
            asset_index,
        )
        .unwrap();
        assert_eq!(profile.mark_ewma_e6, EXPECTED_MARK);
    }

    let released_pnl = EXPECTED_RELEASED_PNL_PER_ASSET * u128::from(ASSET_COUNT);
    assert_eq!(env.portfolio_state(long).pnl.get(), released_pnl as i128);
    assert_eq!(env.portfolio_state(short).pnl.get(), 0);
    let convert_cu = env.convert_released_pnl_with_cu(&long_owner, long, released_pnl);
    let long_after_convert = env.portfolio_state(long);
    let short_after_convert = env.portfolio_state(short);
    assert_eq!(long_after_convert.pnl.get(), 0);
    assert_eq!(short_after_convert.pnl.get(), 0);
    assert_eq!(
        long_after_convert.capital.get() + short_after_convert.capital.get(),
        2 * DEPOSIT - fee_stock_before_exit
    );

    let long_dest = env.withdraw(&long_owner, long, long_after_convert.capital.get());
    let short_dest = env.withdraw(&short_owner, short, short_after_convert.capital.get());
    assert_eq!(
        u128::from(env.token_amount(long_dest)) + u128::from(env.token_amount(short_dest)),
        2 * DEPOSIT - fee_stock_before_exit
    );
    env.close_portfolio_with_cu(&long_owner, long);
    env.close_portfolio_with_cu(&short_owner, short);
    let terminal = env.market_state().1;
    assert_eq!(terminal.vault, terminal.insurance);
    assert_eq!(terminal.vault as u64, env.token_amount(env.vault));
    assert_eq!(terminal.vault, fee_stock_before_exit);

    println!(
        "INV-045 max-shape paid EWMA open={open_cu} movement-refresh={refresh_cu} \
         drain-refresh={drain_refresh_cu} \
         DrainOnly-close={close_cu} convert={convert_cu}"
    );
    for (label, cu) in [
        ("paid EWMA open", open_cu),
        ("paid EWMA refresh", refresh_cu),
        ("DrainOnly refresh", drain_refresh_cu),
        ("DrainOnly extreme-price close", close_cu),
        ("released PnL conversion", convert_cu),
    ] {
        assert!(cu < 1_400_000, "{label} consumed {cu} CU");
    }
}
