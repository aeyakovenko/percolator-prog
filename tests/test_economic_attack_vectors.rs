//! Focused economic-attack probes inspired by public DEX/perp incidents.

mod common;
use common::*;

use solana_sdk::signature::Keypair;

fn attacker_account_equity(env: &TestEnv, idx: u16) -> i128 {
    env.read_account_capital(idx) as i128
        + env.read_account_pnl(idx)
        + env.read_account_fee_credits(idx)
}

fn assert_no_net_extraction(
    label: &str,
    env: &TestEnv,
    idx_a: u16,
    idx_b: u16,
    external_withdrawn: i128,
    initial_deposits: i128,
) {
    let wealth = external_withdrawn
        + attacker_account_equity(env, idx_a)
        + attacker_account_equity(env, idx_b);
    assert!(
        wealth <= initial_deposits + 10_000,
        "{} extracted net value: wealth={} deposits={} external={} a_eq={} b_eq={}",
        label,
        wealth,
        initial_deposits,
        external_withdrawn,
        attacker_account_equity(env, idx_a),
        attacker_account_equity(env, idx_b),
    );
}

fn residual(env: &TestEnv) -> u128 {
    let senior = env
        .read_c_tot()
        .checked_add(env.read_insurance_balance())
        .expect("senior accounting overflow in test");
    env.read_engine_vault().saturating_sub(senior)
}

fn withdraw_chunked(
    env: &mut TestEnv,
    owner: &Keypair,
    idx: u16,
    chunk: u64,
    rounds: usize,
) -> u64 {
    let mut withdrawn = 0u64;
    for _ in 0..rounds {
        if env.try_withdraw(owner, idx, chunk).is_ok() {
            withdrawn = withdrawn.saturating_add(chunk);
        }
    }
    withdrawn
}

/// dYdX/YFI-style profit recycling.
///
/// Attack model: the attacker controls both sides of a matched trade. They
/// push the long side into profit, repeatedly try to convert released PnL into
/// capital, withdraw that capital while the losing short leg remains open, and
/// treat every successful withdrawal as external attacker wealth that could be
/// redeployed into fresh accounts.
///
/// Success condition for the attacker would be:
///
/// ```text
/// withdrawn_external + remaining_user_equity + remaining_lp_equity
///   > initial_user_deposit + initial_lp_deposit
/// ```
///
/// A defended market may allow profit withdrawals, but only if the losing leg
/// carries the offsetting loss. Insurance must not subsidize the loop.
#[test]
fn test_attack_yfi_style_profit_recycling_no_net_extraction() {
    let mut env = TestEnv::new();
    env.init_market_with_trading_fee_and_warmup(0, 50);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    let lp_deposit: u64 = 30_000_000_000;
    env.deposit(&lp, lp_idx, lp_deposit);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    let user_deposit: u64 = 2_000_000_000;
    env.deposit(&user, user_idx, user_deposit);

    let initial_deposits = user_deposit as i128 + lp_deposit as i128;
    let insurance_before = env.read_insurance_balance();

    // Near-margin long for the user at the default 138.0 e6 price.
    let size: i128 = 100_000_000;
    env.trade(&user, &lp, lp_idx, user_idx, size);

    // Move price up far enough to create meaningful long-side PnL. The helper
    // walks/cranks through the configured envelope; this is not testing oracle
    // gap safety, only PnL recycling once profit exists.
    env.set_slot_and_price(800, 170_000_000);
    env.crank();
    for slot in (900..=1_300).step_by(50) {
        env.set_slot_and_price(slot, 170_000_000);
        env.crank();
    }

    let mut withdrawn_external: i128 = 0;
    let mut successful_withdrawals = 0u64;
    let mut successful_conversions = 0u64;

    for round in 0..12u64 {
        if env
            .try_convert_released_pnl(&user, user_idx, 250_000_000)
            .is_ok()
        {
            successful_conversions += 1;
        }

        if env.try_withdraw(&user, user_idx, 250_000_000).is_ok() {
            withdrawn_external += 250_000_000;
            successful_withdrawals += 1;
        }

        env.set_slot_and_price(1_350 + round * 25, 170_000_000);
        env.crank();

        let attacker_wealth = withdrawn_external
            + attacker_account_equity(&env, user_idx)
            + attacker_account_equity(&env, lp_idx);
        assert!(
            attacker_wealth <= initial_deposits + 10_000,
            "profit recycling extracted net value after round {}: wealth={} deposits={} withdrawn={} user_eq={} lp_eq={}",
            round,
            attacker_wealth,
            initial_deposits,
            withdrawn_external,
            attacker_account_equity(&env, user_idx),
            attacker_account_equity(&env, lp_idx),
        );
    }

    assert!(
        successful_withdrawals > 0,
        "probe did not exercise the withdrawal leg"
    );

    let insurance_after = env.read_insurance_balance();
    assert_eq!(
        insurance_after, insurance_before,
        "profit recycling loop must not draw from insurance"
    );

    println!(
        "YFI-style recycling defended: conversions={} withdrawals={} withdrawn={} user_eq={} lp_eq={} insurance={}",
        successful_conversions,
        successful_withdrawals,
        withdrawn_external,
        attacker_account_equity(&env, user_idx),
        attacker_account_equity(&env, lp_idx),
        insurance_after,
    );
}

/// Mars/JELLY-style self-liquidation into the backstop.
///
/// Attack model: the attacker controls the weak long and the strong LP short.
/// They create near-margin exposure, push the oracle toward a price that would
/// bankrupt the long if it landed immediately, then rely on crank/liquidation
/// processing to forgive the toxic leg while the LP leg keeps the gain.
///
/// A defended market should liquidate/risk-reduce through capped effective
/// prices before the bad leg can externalize losses to insurance.
#[test]
fn test_attack_self_liquidation_backstop_no_insurance_siphon() {
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 100);

    let admin = Keypair::from_bytes(&env.payer.to_bytes()).unwrap();
    env.top_up_insurance(&admin, 5_000_000_000);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    let lp_deposit: u64 = 60_000_000_000;
    env.deposit(&lp, lp_idx, lp_deposit);

    let weak_long = Keypair::new();
    let weak_idx = env.init_user(&weak_long);
    let weak_deposit: u64 = 16_000_000_000;
    env.deposit(&weak_long, weak_idx, weak_deposit);

    let insurance_before = env.read_insurance_balance();
    let initial_deposits = lp_deposit as i128 + weak_deposit as i128;

    let size: i128 = 1_000_000_000;
    env.trade(&weak_long, &lp, lp_idx, weak_idx, size);
    let weak_pos_before = env.read_account_position(weak_idx);

    // Raw target is a 25% adverse move for the long. The helper walks and
    // cranks through the envelope, modeling responsive keepers after the
    // attacker starts pushing the target.
    env.set_slot_and_price(900, 103_500_000);
    for slot in (950..=1_800).step_by(50) {
        env.set_slot_and_price(slot, 103_500_000);
        let _ = env.try_crank();
    }

    // If the account is still eligible for explicit liquidation, try it too.
    let _ = env.try_liquidate(weak_idx);
    for slot in (1_850..=2_100).step_by(50) {
        env.set_slot_and_price(slot, 103_500_000);
        let _ = env.try_crank();
    }

    let weak_pos_after = env.read_account_position(weak_idx);
    assert!(
        weak_pos_after.unsigned_abs() <= weak_pos_before.unsigned_abs(),
        "liquidation/risk processing must not increase toxic exposure: before={} after={}",
        weak_pos_before,
        weak_pos_after,
    );

    let attacker_wealth =
        attacker_account_equity(&env, weak_idx) + attacker_account_equity(&env, lp_idx);
    assert!(
        attacker_wealth <= initial_deposits + 10_000,
        "self-liquidation extracted net value: wealth={} deposits={} weak_eq={} lp_eq={}",
        attacker_wealth,
        initial_deposits,
        attacker_account_equity(&env, weak_idx),
        attacker_account_equity(&env, lp_idx),
    );

    let insurance_after = env.read_insurance_balance();
    assert!(
        insurance_before.saturating_sub(insurance_after) <= 10_000,
        "self-liquidation drew from insurance: before={} after={}",
        insurance_before,
        insurance_after,
    );

    assert_eq!(
        env.read_engine_vault() as u64,
        env.vault_balance(),
        "engine vault and SPL vault must stay synchronized"
    );

    println!(
        "Self-liquidation defended: weak_pos {} -> {}, weak_eq={}, lp_eq={}, insurance {} -> {}",
        weak_pos_before,
        weak_pos_after,
        attacker_account_equity(&env, weak_idx),
        attacker_account_equity(&env, lp_idx),
        insurance_before,
        insurance_after,
    );
}

/// Mirror of the YFI recycling probe with the LP as the winning leg.
///
/// The attacker shorts through the LP account, moves price down, converts any
/// released LP-side PnL, and withdraws it while the weak long remains open.
#[test]
fn test_attack_lp_side_profit_recycling_no_net_extraction() {
    let mut env = TestEnv::new();
    env.init_market_with_trading_fee_and_warmup(0, 50);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    let lp_deposit: u64 = 20_000_000_000;
    env.deposit(&lp, lp_idx, lp_deposit);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    let user_deposit: u64 = 16_000_000_000;
    env.deposit(&user, user_idx, user_deposit);

    let initial_deposits = user_deposit as i128 + lp_deposit as i128;
    let insurance_before = env.read_insurance_balance();

    env.trade(&user, &lp, lp_idx, user_idx, 1_000_000_000);
    env.set_slot_and_price(900, 120_000_000);
    for slot in (950..=1_350).step_by(50) {
        env.set_slot_and_price(slot, 120_000_000);
        env.crank();
    }

    let mut withdrawn_external = 0i128;
    let mut successful_withdrawals = 0u64;
    for round in 0..10u64 {
        let _ = env.try_convert_released_pnl(&lp, lp_idx, 250_000_000);
        if env.try_withdraw(&lp, lp_idx, 250_000_000).is_ok() {
            withdrawn_external += 250_000_000;
            successful_withdrawals += 1;
        }
        env.set_slot_and_price(1_400 + round * 25, 120_000_000);
        env.crank();
        assert_no_net_extraction(
            "LP-side profit recycling",
            &env,
            user_idx,
            lp_idx,
            withdrawn_external,
            initial_deposits,
        );
    }

    assert!(
        successful_withdrawals > 0,
        "probe did not exercise LP withdrawal"
    );
    assert!(
        env.read_insurance_balance() >= insurance_before,
        "LP recycling must not draw from insurance: before={} after={}",
        insurance_before,
        env.read_insurance_balance(),
    );
}

/// Extraction-sensitive operations must fail atomically while raw oracle target
/// and effective engine price diverge.
#[test]
fn test_attack_target_lag_withdraw_rejected_atomically() {
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 100);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 30_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.trade(&user, &lp, lp_idx, user_idx, 100_000_000);

    let base_slot = env.svm.get_sysvar::<Clock>().slot;
    env.set_slot_and_price_raw_no_walk(base_slot + 10, 200_000_000);

    let user_cap_before = env.read_account_capital(user_idx);
    let user_pos_before = env.read_account_position(user_idx);
    let vault_before = env.vault_balance();
    let engine_vault_before = env.read_engine_vault();

    let result = env.try_withdraw(&user, user_idx, 1);
    assert!(
        result.is_err(),
        "withdraw during target/effective lag must reject"
    );
    assert_eq!(env.read_account_capital(user_idx), user_cap_before);
    assert_eq!(env.read_account_position(user_idx), user_pos_before);
    assert_eq!(env.vault_balance(), vault_before);
    assert_eq!(env.read_engine_vault(), engine_vault_before);
}

/// Risk-increasing trades must fail atomically while raw oracle target and
/// effective engine price diverge.
#[test]
fn test_attack_target_lag_trade_rejected_atomically() {
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 100);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 30_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 5_000_000_000);

    env.trade(&user, &lp, lp_idx, user_idx, 100_000_000);

    let base_slot = env.svm.get_sysvar::<Clock>().slot;
    env.set_slot_and_price_raw_no_walk(base_slot + 10, 200_000_000);

    let user_pos_before = env.read_account_position(user_idx);
    let lp_pos_before = env.read_account_position(lp_idx);
    let user_cap_before = env.read_account_capital(user_idx);
    let lp_cap_before = env.read_account_capital(lp_idx);
    let vault_before = env.vault_balance();

    let result = env.try_trade(&user, &lp, lp_idx, user_idx, 1_000_000);
    assert!(
        result.is_err(),
        "trade during target/effective lag must reject"
    );
    assert_eq!(env.read_account_position(user_idx), user_pos_before);
    assert_eq!(env.read_account_position(lp_idx), lp_pos_before);
    assert_eq!(env.read_account_capital(user_idx), user_cap_before);
    assert_eq!(env.read_account_capital(lp_idx), lp_cap_before);
    assert_eq!(env.vault_balance(), vault_before);
}

/// Whipsaw version of profit recycling: attacker withdraws during an up-move,
/// then the oracle reverses. The previously withdrawn amount must remain backed
/// by an offsetting loss on the controlled LP leg.
#[test]
fn test_attack_whipsaw_profit_recycling_no_net_extraction() {
    let mut env = TestEnv::new();
    env.init_market_with_trading_fee_and_warmup(0, 50);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    let lp_deposit: u64 = 30_000_000_000;
    env.deposit(&lp, lp_idx, lp_deposit);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    let user_deposit: u64 = 2_000_000_000;
    env.deposit(&user, user_idx, user_deposit);

    let initial_deposits = user_deposit as i128 + lp_deposit as i128;
    let insurance_before = env.read_insurance_balance();

    env.trade(&user, &lp, lp_idx, user_idx, 100_000_000);
    env.set_slot_and_price(800, 170_000_000);
    for slot in (850..=1_150).step_by(50) {
        env.set_slot_and_price(slot, 170_000_000);
        env.crank();
    }

    let mut withdrawn_external = 0i128;
    for _ in 0..6 {
        let _ = env.try_convert_released_pnl(&user, user_idx, 250_000_000);
        if env.try_withdraw(&user, user_idx, 250_000_000).is_ok() {
            withdrawn_external += 250_000_000;
        }
    }

    env.set_slot_and_price(1_300, 110_000_000);
    for slot in (1_350..=1_800).step_by(50) {
        env.set_slot_and_price(slot, 110_000_000);
        let _ = env.try_crank();
    }

    assert_no_net_extraction(
        "whipsaw profit recycling",
        &env,
        user_idx,
        lp_idx,
        withdrawn_external,
        initial_deposits,
    );
    assert!(
        insurance_before.saturating_sub(env.read_insurance_balance()) <= 10_000,
        "whipsaw recycling must not drain insurance"
    );
}

/// Same self-liquidation shape as the backstop probe, but with no meaningful
/// insurance buffer. This stresses force-realize/ADL behavior instead of the
/// ordinary liquidation path.
#[test]
fn test_attack_zero_insurance_self_liquidation_no_net_extraction() {
    let mut env = TestEnv::new();
    env.init_market_with_cap(0, 100);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    let lp_deposit: u64 = 60_000_000_000;
    env.deposit(&lp, lp_idx, lp_deposit);

    let weak_long = Keypair::new();
    let weak_idx = env.init_user(&weak_long);
    let weak_deposit: u64 = 16_000_000_000;
    env.deposit(&weak_long, weak_idx, weak_deposit);

    let initial_deposits = lp_deposit as i128 + weak_deposit as i128;
    let insurance_before = env.read_insurance_balance();

    env.trade(&weak_long, &lp, lp_idx, weak_idx, 1_000_000_000);
    env.set_slot_and_price(900, 103_500_000);
    for slot in (950..=2_100).step_by(50) {
        env.set_slot_and_price(slot, 103_500_000);
        let _ = env.try_crank();
    }
    let _ = env.try_liquidate(weak_idx);

    assert_no_net_extraction(
        "zero-insurance self-liquidation",
        &env,
        weak_idx,
        lp_idx,
        0,
        initial_deposits,
    );
    assert!(
        insurance_before.saturating_sub(env.read_insurance_balance()) <= 10_000,
        "zero-insurance liquidation must not create an insurance siphon"
    );
    assert_eq!(env.read_engine_vault() as u64, env.vault_balance());
}

/// Minimum-size position and extreme whipsaw probe for rounding/precision
/// boundaries. This is Kyber-style in spirit: many bugs live at the smallest
/// nonzero quantity when state crosses price boundaries.
#[test]
fn test_attack_min_position_whipsaw_no_rounding_mint() {
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    let lp_deposit: u64 = 1_000_000_000;
    env.deposit(&lp, lp_idx, lp_deposit);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    let user_deposit: u64 = 1_000_000_000;
    env.deposit(&user, user_idx, user_deposit);

    let initial_deposits = user_deposit as i128 + lp_deposit as i128;
    let insurance_before = env.read_insurance_balance();

    env.trade(&user, &lp, lp_idx, user_idx, 1);
    for (slot, price) in [
        (200u64, 200_000_000i64),
        (400u64, 80_000_000i64),
        (600u64, 180_000_000i64),
        (800u64, 138_000_000i64),
    ] {
        env.set_slot_and_price(slot, price);
        let _ = env.try_crank();
    }
    let _ = env.try_trade(&user, &lp, lp_idx, user_idx, -1);
    env.set_slot_and_price(900, 138_000_000);
    let _ = env.try_crank();

    assert_no_net_extraction(
        "minimum-position whipsaw",
        &env,
        user_idx,
        lp_idx,
        0,
        initial_deposits,
    );
    assert!(
        env.read_insurance_balance() >= insurance_before,
        "minimum-position whipsaw should not drain insurance"
    );
    assert_eq!(env.read_engine_vault() as u64, env.vault_balance());
}

/// Fee-cycling wash trades should burn value into insurance, not create a
/// rebate-like extraction path for the two controlled accounts.
#[test]
fn test_attack_fee_cycling_wash_trades_no_rebate_siphon() {
    let mut env = TestEnv::new();
    env.init_market_with_trading_fee(100); // 1%

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    let lp_deposit: u64 = 20_000_000_000;
    env.deposit(&lp, lp_idx, lp_deposit);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    let user_deposit: u64 = 20_000_000_000;
    env.deposit(&user, user_idx, user_deposit);

    let initial_deposits = user_deposit as i128 + lp_deposit as i128;
    let insurance_before = env.read_insurance_balance();

    for round in 0..20u64 {
        let size = if round % 2 == 0 {
            10_000_000
        } else {
            -10_000_000
        };
        let _ = env.try_trade(&user, &lp, lp_idx, user_idx, size);
        env.set_slot_and_price(100 + round * 5, 138_000_000);
        let _ = env.try_crank();
        assert_no_net_extraction(
            "fee-cycling wash trades",
            &env,
            user_idx,
            lp_idx,
            0,
            initial_deposits,
        );
    }

    assert!(
        env.read_insurance_balance() >= insurance_before,
        "wash-trade fees must not reduce insurance"
    );
    assert_eq!(env.read_engine_vault() as u64, env.vault_balance());
}

/// Many one-unit trades are a precision/rounding stressor. The attacker should
/// not be able to accumulate sub-unit rounding into withdrawable net wealth.
#[test]
fn test_attack_many_one_unit_trades_no_rounding_accumulation() {
    let mut env = TestEnv::new();
    env.init_market_with_invert(0);

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    let lp_deposit: u64 = 1_000_000_000;
    env.deposit(&lp, lp_idx, lp_deposit);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    let user_deposit: u64 = 1_000_000_000;
    env.deposit(&user, user_idx, user_deposit);

    let initial_deposits = user_deposit as i128 + lp_deposit as i128;
    let insurance_before = env.read_insurance_balance();

    for round in 0..100u64 {
        let size = if round % 2 == 0 { 1 } else { -1 };
        let _ = env.try_trade(&user, &lp, lp_idx, user_idx, size);
        if round % 10 == 0 {
            env.set_slot_and_price(100 + round, 138_000_000);
            let _ = env.try_crank();
        }
        assert_no_net_extraction(
            "many one-unit trades",
            &env,
            user_idx,
            lp_idx,
            0,
            initial_deposits,
        );
    }

    env.set_slot_and_price(250, 138_000_000);
    let _ = env.try_crank();
    assert_no_net_extraction(
        "many one-unit trades final",
        &env,
        user_idx,
        lp_idx,
        0,
        initial_deposits,
    );
    assert!(env.read_insurance_balance() >= insurance_before);
    assert_eq!(env.read_engine_vault() as u64, env.vault_balance());
}

/// Fee revenue must not become trader-claimable residual.
///
/// Attack model: attacker controls both sides of repeated wash trades and pays
/// fees hoping the implementation lowers `c_tot` without increasing insurance,
/// creating residual that can later back positive PnL withdrawals.
///
/// Success criterion would be residual growth from fees alone.
#[test]
fn test_attack_fee_revenue_not_trader_residual() {
    let mut env = TestEnv::new();
    env.init_market_with_trading_fee(100); // 1%

    let lp = Keypair::new();
    let lp_idx = env.init_lp(&lp);
    env.deposit(&lp, lp_idx, 20_000_000_000);

    let user = Keypair::new();
    let user_idx = env.init_user(&user);
    env.deposit(&user, user_idx, 20_000_000_000);

    assert_eq!(residual(&env), 0, "fresh market should have no residual");
    let insurance_before = env.read_insurance_balance();

    for round in 0..24u64 {
        let size = if round % 2 == 0 {
            20_000_000
        } else {
            -20_000_000
        };
        let _ = env.try_trade(&user, &lp, lp_idx, user_idx, size);
        env.set_slot_and_price(100 + round * 3, 138_000_000);
        let _ = env.try_crank();

        assert_eq!(
            residual(&env),
            0,
            "fees must route to insurance, not residual, after round {}",
            round
        );
    }

    assert!(
        env.read_insurance_balance() > insurance_before,
        "probe must actually collect fees into insurance"
    );
    assert_eq!(env.read_engine_vault() as u64, env.vault_balance());
}

/// One winner exits, then the market reverses and the old losing side becomes
/// profitable. Both exits together must remain bounded by controlled deposits;
/// the second winner cannot withdraw against already-consumed residual.
#[test]
fn test_attack_sequential_winner_exits_after_whipsaw_no_double_spend() {
    let mut env = TestEnv::new();
    env.init_market_with_trading_fee_and_warmup(0, 1);

    let short_owner = Keypair::new();
    let short_idx = env.init_lp(&short_owner);
    let short_deposit: u64 = 40_000_000_000;
    env.deposit(&short_owner, short_idx, short_deposit);

    let first_long = Keypair::new();
    let first_long_idx = env.init_user(&first_long);
    let first_deposit: u64 = 6_000_000_000;
    env.deposit(&first_long, first_long_idx, first_deposit);

    let second_long = Keypair::new();
    let second_long_idx = env.init_lp(&second_long);
    let second_deposit: u64 = 40_000_000_000;
    env.deposit(&second_long, second_long_idx, second_deposit);

    let initial_deposits = short_deposit as i128 + first_deposit as i128 + second_deposit as i128;
    let insurance_before = env.read_insurance_balance();
    let size: i128 = 100_000_000;

    // First long opens against the original short.
    env.trade(&first_long, &short_owner, short_idx, first_long_idx, size);

    // Price rises: first long wins, original short loses. Settle the losing
    // short first to create real residual backing.
    env.set_slot_and_price(200, 170_000_000);
    env.try_settle_account(short_idx)
        .expect("settle original short loss");
    env.set_slot_and_price(202, 170_000_000);
    env.try_settle_account(first_long_idx)
        .expect("settle first long profit");

    // First long exits to a fresh long-side account, leaving the original
    // short open. Then withdraw whatever backed profit is actually available.
    env.set_slot_and_price(205, 170_000_000);
    env.trade(
        &first_long,
        &second_long,
        second_long_idx,
        first_long_idx,
        -size,
    );
    env.set_slot_and_price(207, 170_000_000);
    let _ = env.try_convert_released_pnl(&first_long, first_long_idx, 500_000_000);
    let first_external = withdraw_chunked(&mut env, &first_long, first_long_idx, 500_000_000, 20);

    // Price reverses: the old short is now the winner and the fresh long is
    // the loser. Close them against each other and try to withdraw the second
    // winner's profit too.
    env.set_slot_and_price(400, 110_000_000);
    env.try_settle_account(second_long_idx)
        .expect("settle second long loss");
    env.set_slot_and_price(402, 110_000_000);
    env.try_settle_account(short_idx)
        .expect("settle original short profit");
    env.set_slot_and_price(405, 110_000_000);
    env.trade(
        &second_long,
        &short_owner,
        short_idx,
        second_long_idx,
        -size,
    );
    env.set_slot_and_price(407, 110_000_000);
    let _ = env.try_convert_released_pnl(&short_owner, short_idx, 500_000_000);
    let second_external = withdraw_chunked(&mut env, &short_owner, short_idx, 500_000_000, 20);

    let total_external = first_external as i128 + second_external as i128;
    let attacker_wealth = total_external
        + attacker_account_equity(&env, short_idx)
        + attacker_account_equity(&env, first_long_idx)
        + attacker_account_equity(&env, second_long_idx);
    assert!(
        attacker_wealth <= initial_deposits + 10_000,
        "sequential winner exits double-spent residual: wealth={} deposits={} external={} first_external={} second_external={} short_eq={} first_eq={} second_eq={}",
        attacker_wealth,
        initial_deposits,
        total_external,
        first_external,
        second_external,
        attacker_account_equity(&env, short_idx),
        attacker_account_equity(&env, first_long_idx),
        attacker_account_equity(&env, second_long_idx),
    );
    assert!(
        insurance_before.saturating_sub(env.read_insurance_balance()) <= 10_000,
        "sequential winner exits must not drain insurance: before={} after={}",
        insurance_before,
        env.read_insurance_balance()
    );
    assert_eq!(env.read_engine_vault() as u64, env.vault_balance());
}
