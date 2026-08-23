//! INV-040 - No fee seniority.
//!
//! Fees are junior to protected principal: an uncollectible protocol/trading
//! fee must be dropped or capped by available participant value, not charged
//! from another protected pool, minted into insurance, or allowed to block a
//! risk-reducing exit.
//!
//! This test uses public no-CPI trade routes to put one side at an adverse
//! maintenance boundary, then executes a full exit whose quoted fee exceeds
//! that side's remaining capital. The invariant checks the successful exit,
//! the actual insurance delta, exact aggregate-capital conservation, and zero
//! token-vault movement.

use super::*;

fn assert_underfunded_exit_drops_only_uncollectible_fee(path: NoCpiReportedPricePath) {
    const MARK: u64 = 1_000_000;
    const SIZE_Q: i128 = POS_SCALE as i128;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: MARK,
        max_trading_fee_bps: 10_000,
        max_price_move_bps_per_slot: 10_000,
        max_accrual_dt_slots: 1,
        min_funding_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.configure_ewma_mark_with_cu(0, MARK, 1, 0);
    let (long_owner, long, short_owner, short) =
        funded_no_cpi_reported_price_pair(&mut env, MARK as u128);

    try_no_cpi_reported_price_trade_with_cu(
        &mut env,
        path,
        &long_owner,
        long,
        &short_owner,
        short,
        SIZE_Q,
        MARK,
        0,
    )
    .unwrap_or_else(|err| panic!("{path:?}: setup open failed: {err}"));

    env.svm.warp_to_slot(10);
    env.push_ewma_mark_with_cu(10, 1);
    env.svm.expire_blockhash();
    env.crank_steps_after_market_catchup(
        long,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
        1,
    );
    env.svm.expire_blockhash();
    env.crank(
        short,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
    );

    env.svm.warp_to_slot(20);
    let (_, group_before) = env.market_state();
    let reported_exit_price = group_before.assets[0]
        .effective_price
        .checked_mul(2)
        .expect("one-slot upper price envelope");
    let requested_fee_per_side = reported_exit_price as u128;
    let long_before = env.portfolio_state(long);
    let short_before = env.portfolio_state(short);
    let aggregate_capital_before = long_before
        .capital
        .get()
        .checked_add(short_before.capital.get())
        .expect("aggregate capital before");
    assert!(
        0 < long_before.capital.get() && long_before.capital.get() < requested_fee_per_side,
        "{path:?}: setup must make one side's quoted fee partly uncollectible",
    );
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    env.svm.expire_blockhash();
    let exit = try_no_cpi_reported_price_trade_with_cu(
        &mut env,
        path,
        &long_owner,
        long,
        &short_owner,
        short,
        -SIZE_Q,
        reported_exit_price,
        0,
    );
    assert!(
        exit.is_ok(),
        "{path:?}: uncollectible fee must not DoS a risk-reducing full exit: {exit:?}",
    );

    let (_, group_after) = env.market_state();
    assert_eq!(group_after.assets[0].oi_eff_long_q, 0);
    assert_eq!(group_after.assets[0].oi_eff_short_q, 0);
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "{path:?}: fee collection is internal and must not mint or move vault tokens",
    );
    assert_eq!(
        group_after.vault, group_before.vault,
        "{path:?}: internal fee accounting must not change token-stock accounting",
    );

    let collected_fee = group_after.insurance - group_before.insurance;
    let quoted_two_sided_fee = requested_fee_per_side * 2;
    assert!(
        collected_fee < quoted_two_sided_fee,
        "{path:?}: the uncollectible part of the quoted fee must not be credited to insurance",
    );
    let aggregate_capital_after = env
        .portfolio_state(long)
        .capital
        .get()
        .checked_add(env.portfolio_state(short).capital.get())
        .expect("aggregate capital after");
    assert_eq!(
        aggregate_capital_before - aggregate_capital_after,
        collected_fee,
        "{path:?}: aggregate user capital may fall only by the actually collected fee",
    );
}

#[test]
fn v16_program_uncollectible_exit_fee_is_dropped_not_senioritized() {
    for path in [
        NoCpiReportedPricePath::Single,
        NoCpiReportedPricePath::Batch,
    ] {
        assert_underfunded_exit_drops_only_uncollectible_fee(path);
    }
}

fn assert_underfunded_cpi_exit_drops_only_uncollectible_fee(path: CpiEwmaTradePath) {
    const MARK: u64 = 1_000_000;
    const ADVERSE_MARK: u64 = 1_999_999;
    const SIZE_Q: i128 = POS_SCALE as i128;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: MARK,
        max_trading_fee_bps: 10_000,
        max_price_move_bps_per_slot: 10_000,
        max_accrual_dt_slots: 1,
        min_funding_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.configure_ewma_mark_with_cu(0, MARK, 1, 0);

    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(matcher_program_path()).expect("read matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);
    let taker_owner = Keypair::new();
    let taker = env.create_portfolio(&taker_owner);
    let lp_owner = Keypair::new();
    let lp = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker, MARK as u128);
    env.deposit(&lp_owner, lp, MARK as u128);
    let (open_ctx, open_delegate, _) = env.init_matcher_context_with_passive_spread_authorized(
        matcher_program,
        &lp_owner,
        lp,
        0,
        9_000,
    );

    env.svm.expire_blockhash();
    env.try_trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker,
        &lp_owner,
        lp,
        matcher_program,
        open_ctx,
        open_delegate,
        0,
        -SIZE_Q,
        0,
    )
    .unwrap_or_else(|err| panic!("{path:?}: setup short open failed: {err}"));

    env.svm.warp_to_slot(10);
    env.push_ewma_mark_with_cu(10, ADVERSE_MARK);
    env.svm.expire_blockhash();
    env.crank_steps_after_market_catchup(
        taker,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
        1,
    );
    env.svm.expire_blockhash();
    env.crank(
        lp,
        ProgInstruction::PermissionlessCrank {
            now_slot: 10,
            observations: crank_observations(0),
        },
    );

    let (exit_ctx, exit_delegate, _) = env.init_matcher_context_with_passive_spread_authorized(
        matcher_program,
        &lp_owner,
        lp,
        9_000,
        9_000,
    );
    env.svm.warp_to_slot(20);
    let (_, group_before) = env.market_state();
    let expected_matcher_price = group_before.assets[0]
        .effective_price
        .checked_mul(19)
        .expect("matcher ask numerator")
        / 10;
    let accepted_exit_price = oracle_v16::clamp_toward_engine_dt(
        group_before.assets[0].effective_price,
        expected_matcher_price,
        10_000,
        1,
    );
    assert_eq!(
        accepted_exit_price, expected_matcher_price,
        "{path:?}: wide matcher ask must remain inside the one-segment engine envelope"
    );
    let requested_fee_per_side = accepted_exit_price as u128;
    let taker_before = env.portfolio_state(taker);
    let lp_before = env.portfolio_state(lp);
    let aggregate_capital_before = taker_before
        .capital
        .get()
        .checked_add(lp_before.capital.get())
        .expect("aggregate capital before");
    assert!(
        0 < taker_before.capital.get() && taker_before.capital.get() < requested_fee_per_side,
        "{path:?}: setup must leave the adverse short unable to pay its quoted exit fee",
    );
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    env.svm.expire_blockhash();
    let exit = match path {
        CpiEwmaTradePath::Single => env.try_trade_cpi_with_cu_on_asset(
            &taker_owner,
            taker,
            &lp_owner,
            lp,
            matcher_program,
            exit_ctx,
            exit_delegate,
            0,
            SIZE_Q,
            0,
        ),
        CpiEwmaTradePath::Batch => env.send(
            env.batch_trade_cpi_ix(
                taker,
                lp,
                vec![BatchTradeCpiLeg {
                    asset_index: 0,
                    market_id: group_before.assets[0].market_id,
                    size_q: SIZE_Q,
                    fee_bps: 0,
                    limit_price: 0,
                }],
            ),
            vec![
                AccountMeta::new(taker_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(taker, false),
                AccountMeta::new(lp, false),
                AccountMeta::new_readonly(matcher_program, false),
                AccountMeta::new(exit_ctx, false),
                AccountMeta::new_readonly(exit_delegate, false),
            ],
            &[&taker_owner],
        ),
    };
    assert!(
        exit.is_ok(),
        "{path:?}: uncollectible CPI fee must not DoS a risk-reducing full exit: {exit:?}",
    );

    let (_, group_after) = env.market_state();
    assert_eq!(group_after.assets[0].oi_eff_long_q, 0);
    assert_eq!(group_after.assets[0].oi_eff_short_q, 0);
    assert_eq!(
        env.svm.get_account(&env.vault).unwrap(),
        vault_before,
        "{path:?}: CPI exit fee collection is internal and must not move vault tokens",
    );
    assert_eq!(
        group_after.vault, group_before.vault,
        "{path:?}: CPI internal fee accounting must not change token-stock accounting",
    );

    let collected_fee = group_after.insurance - group_before.insurance;
    let quoted_two_sided_fee = requested_fee_per_side * 2;
    assert!(
        collected_fee < quoted_two_sided_fee,
        "{path:?}: the uncollectible CPI fee must not be credited to insurance",
    );
    let aggregate_capital_after = env
        .portfolio_state(taker)
        .capital
        .get()
        .checked_add(env.portfolio_state(lp).capital.get())
        .expect("aggregate capital after");
    assert_eq!(
        aggregate_capital_before - aggregate_capital_after,
        collected_fee,
        "{path:?}: CPI aggregate user capital may fall only by the actually collected fee",
    );
}

#[test]
fn v16_program_cpi_uncollectible_exit_fee_is_dropped_not_senioritized() {
    for path in [CpiEwmaTradePath::Single, CpiEwmaTradePath::Batch] {
        assert_underfunded_cpi_exit_drops_only_uncollectible_fee(path);
    }
}

// DoS-resistance: SyncMaintenanceFee is permissionless, so an attacker could try to grief a victim by
// spamming it to over-drain their capital, or by passing a far-future now_slot to charge future time.
// The fee is time-based (charged on real elapsed slots, last_sync advanced to now) and uses the
// AUTHENTICATED clock -> spamming in one slot charges only once, and a future now_slot charges nothing
// extra. The victim pays exactly the elapsed-time fee they already owe, no more.
#[test]
fn v16_attack_maintenance_fee_spam_cannot_overdrain() {
    let mut env =
        V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(1, 1_000, 1_000, 500, 100);
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    let cap0 = env.portfolio_state(p).capital.get();
    env.svm.warp_to_slot(50);
    // first sync at slot 50: charges the elapsed-time maintenance fee.
    env.sync_maintenance_fee_with_cu(p, None, 50);
    let cap1 = env.portfolio_state(p).capital.get();
    assert!(
        cap1 < cap0,
        "first sync charges the accrued maintenance fee"
    );
    // SPAM: repeated syncs in the same slot charge nothing more (idempotent -> no grief over-drain).
    for _ in 0..5 {
        env.svm.expire_blockhash();
        env.sync_maintenance_fee_with_cu(p, None, 50);
    }
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        cap1,
        "spamming sync in one slot must not over-charge"
    );
    // FUTURE now_slot lie: real clock is still 50, so no future time can be charged.
    env.svm.expire_blockhash();
    env.sync_maintenance_fee_with_cu(p, None, 50 + 1_000_000);
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        cap1,
        "a future now_slot cannot charge future maintenance time"
    );
    // real time advancing DOES accrue more (fee is genuinely time-based).
    env.svm.warp_to_slot(100);
    env.svm.expire_blockhash();
    env.sync_maintenance_fee_with_cu(p, None, 100);
    assert!(
        env.portfolio_state(p).capital.get() < cap1,
        "advancing real time accrues additional fee"
    );
}
