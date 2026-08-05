//! INV-053 - Full-health recertification equivalence.
//!
//! Normative obligation: Fast or incremental certification is never more favorable than full recomputation.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_attack_pending_later_rounded_rescue_funding_requires_observation`, `v16_attack_no_observation_refresh_cannot_skip_premium_funding`, `v16_attack_no_observation_liquidation_cannot_skip_premium_funding`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: the first test certifies the minimized PR220/PR366 trace on the fixed
//! program and checks exact rollback plus the fully observed healthy control. The stateful matrix
//! supplies bounded coverage over all four trade routes and both active-leg orders.

use super::*;

#[test]
fn v16_attack_pending_later_rounded_rescue_funding_requires_observation() {
    const ADVERSE_PRICE: u64 = 1_000_000;
    const ADVERSE_TARGET: u64 = 997_600;
    const RESCUE_PRICE: u64 = 100;
    const RESCUE_MARK: u64 = 99;
    const OPEN_SLOT: u64 = 1;
    const PRIME_SLOT: u64 = 2;
    const ATTACK_SLOT: u64 = 3;
    const ADVERSE_SIZE_Q: i128 = (50 * POS_SCALE) as i128;
    const RESCUE_SIZE_Q: i128 = (100_000 * POS_SCALE) as i128;

    let mut params = production_risk_params();
    params.max_portfolio_assets = 2;
    params.max_accrual_dt_slots = 1;
    params.max_abs_funding_e9_per_slot = 10_000;
    params.min_funding_lifetime_slots = 1;
    let mut env = V16CuEnv::new_with_init_params(params);
    env.svm.warp_to_slot(OPEN_SLOT);
    env.configure_auth_mark_for_asset_as_admin(0, OPEN_SLOT, RESCUE_PRICE);
    env.configure_auth_mark_for_asset_as_admin(1, OPEN_SLOT, ADVERSE_PRICE);

    let user = Keypair::new();
    let counterparty = Keypair::new();
    let observer_owner = Keypair::new();
    let user_account = env.create_portfolio(&user);
    let counterparty_account = env.create_portfolio(&counterparty);
    let observer = env.create_portfolio(&observer_owner);
    env.deposit(&user, user_account, 3_115_000);
    env.deposit(&counterparty, counterparty_account, 50_000_000);
    env.top_up_backing_bucket(1, 200_000, 10);

    // The adverse long is first. The funding-receiving long deliberately
    // occupies a later slot, outside the wrapper's selected-leg check.
    env.trade_asset_with_cu(
        1,
        &user,
        user_account,
        &counterparty,
        counterparty_account,
        ADVERSE_SIZE_Q,
        ADVERSE_PRICE,
        0,
    );
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        0,
        &user,
        user_account,
        &counterparty,
        counterparty_account,
        RESCUE_SIZE_Q,
        RESCUE_PRICE,
        0,
    );
    let opened = env.portfolio_state(user_account);
    assert_eq!(leg(&opened, 0).asset_index, 1);
    assert_eq!(leg(&opened, 1).asset_index, 0);
    let adverse_position_before = active_leg_for_asset(&opened, 1).basis_pos_q.unsigned_abs();

    // Prime the later leg at the low price. Its 24-bps movement cap rounds to
    // zero, and the first observed segment intentionally has no funding.
    env.svm.warp_to_slot(PRIME_SLOT);
    env.push_auth_mark_for_asset_as_admin(0, PRIME_SLOT, RESCUE_MARK);
    env.crank(
        observer,
        ProgInstruction::PermissionlessCrank {
            now_slot: PRIME_SLOT,
            observations: crank_observations(0),
        },
    );
    let (_, primed) = env.market_state();
    assert_eq!(primed.assets[0].effective_price, RESCUE_PRICE);
    assert_eq!(primed.assets[0].slot_last, PRIME_SLOT);
    assert_eq!(primed.assets[0].f_long_num, 0);

    // Commit the first leg's adverse move. This makes the user's certificate
    // stale while the later leg has one segment of negative-premium long funding.
    env.svm.warp_to_slot(ATTACK_SLOT);
    env.push_auth_mark_for_asset_as_admin(1, ATTACK_SLOT, ADVERSE_TARGET);
    env.crank(
        observer,
        ProgInstruction::PermissionlessCrank {
            now_slot: ATTACK_SLOT,
            observations: crank_observations(1),
        },
    );
    let (_, stale_group) = env.market_state();
    assert_eq!(stale_group.assets[0].effective_price, RESCUE_PRICE);
    assert_eq!(stale_group.assets[0].slot_last, PRIME_SLOT);
    assert!(
        health_cert(&env.portfolio_state(user_account)).cert_oracle_epoch
            < stale_group.oracle_epoch
    );
    let insurance_before = stale_group.insurance;

    let market_before_omission = env.svm.get_account(&env.market).unwrap();
    let user_before_omission = env.svm.get_account(&user_account).unwrap();
    env.svm.expire_blockhash();
    let omitted = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: ATTACK_SLOT,
            observations: vec![],
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(user_account, false),
        ],
        &[],
    );
    let omitted_error = omitted.expect_err("missing rescue observation must reject");
    assert!(
        omitted_error.contains("Custom(22)"),
        "missing rescue observation returned the wrong error: {omitted_error}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_omission,
        "rejected stale-funding refresh must roll back market state"
    );
    assert_eq!(
        env.svm.get_account(&user_account).unwrap(),
        user_before_omission,
        "rejected stale-funding refresh must roll back the victim"
    );

    // Supplying the later-leg observation books the rescue funding and leaves
    // the same account healthy without charging a liquidation fee.
    env.svm.expire_blockhash();
    env.crank(
        observer,
        ProgInstruction::PermissionlessCrank {
            now_slot: ATTACK_SLOT,
            observations: crank_observations(0),
        },
    );
    assert!(env.market_state().1.assets[0].f_long_num > 0);
    let complete_observations = || {
        vec![
            CrankObservationHint {
                asset_index: 0,
                oracle_accounts: 0,
            },
            CrankObservationHint {
                asset_index: 1,
                oracle_accounts: 0,
            },
        ]
    };
    env.svm.expire_blockhash();
    env.crank(
        counterparty_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: ATTACK_SLOT,
            observations: complete_observations(),
        },
    );
    env.svm.expire_blockhash();
    env.crank(
        user_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: ATTACK_SLOT,
            observations: complete_observations(),
        },
    );
    let healthy = env.portfolio_state(user_account);
    assert_eq!(health_cert(&healthy).certified_liq_deficit, 0);
    assert_eq!(
        active_leg_for_asset(&healthy, 1).basis_pos_q.unsigned_abs(),
        adverse_position_before
    );
    assert_eq!(env.market_state().1.insurance, insurance_before);
}

#[test]
fn v16_attack_no_observation_refresh_cannot_skip_premium_funding() {
    const PRICE: u64 = 1_000_000;
    const DEPOSIT: u128 = 100_000_000;
    const OPEN_SLOT: u64 = 1;
    const PREMIUM_SLOT: u64 = 2;
    const STALE_SLOT: u64 = 3;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        max_portfolio_assets: 2,
        initial_price: PRICE,
        max_price_move_bps_per_slot: 1_000,
        max_accrual_dt_slots: 1,
        max_abs_funding_e9_per_slot: 1_000,
        min_funding_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(OPEN_SLOT);
    env.configure_ewma_mark_with_cu(OPEN_SLOT, PRICE, 1, 0);
    env.configure_auth_mark_for_asset_as_admin(1, OPEN_SLOT, PRICE);

    let owner_a = Keypair::new();
    let owner_b = Keypair::new();
    let asset1_owner = Keypair::new();
    let asset1_counter_owner = Keypair::new();
    let account_a = env.create_portfolio(&owner_a);
    let account_b = env.create_portfolio(&owner_b);
    let asset1_account = env.create_portfolio(&asset1_owner);
    let asset1_counter = env.create_portfolio(&asset1_counter_owner);
    env.deposit(&owner_a, account_a, DEPOSIT);
    env.deposit(&owner_b, account_b, DEPOSIT);
    env.deposit(&asset1_owner, asset1_account, DEPOSIT);
    env.deposit(&asset1_counter_owner, asset1_counter, DEPOSIT);
    env.trade_asset_with_cu(
        0,
        &owner_a,
        account_a,
        &owner_b,
        account_b,
        POS_SCALE as i128,
        PRICE,
        0,
    );
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        1,
        &owner_a,
        account_a,
        &owner_b,
        account_b,
        POS_SCALE as i128,
        PRICE,
        0,
    );
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        1,
        &asset1_owner,
        asset1_account,
        &asset1_counter_owner,
        asset1_counter,
        POS_SCALE as i128,
        PRICE,
        0,
    );

    env.svm.warp_to_slot(PREMIUM_SLOT);
    env.push_ewma_mark_with_cu(PREMIUM_SLOT, PRICE * 2);
    env.crank(
        account_a,
        ProgInstruction::PermissionlessCrank {
            now_slot: PREMIUM_SLOT,
            observations: crank_observations(0),
        },
    );
    let (_, premium_group) = env.market_state();
    assert_eq!(
        premium_group.assets[0].effective_price,
        PRICE + PRICE / 10,
        "first observed crank applies only the capped price move"
    );
    let premium_profile =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap();
    assert!(
        premium_profile.mark_ewma_e6 > premium_group.assets[0].effective_price,
        "EWMA premium remains after the first capped move"
    );
    assert_eq!(
        premium_group.funding_epoch, 0,
        "newly staged premium must not retroactively charge funding"
    );
    assert!(
        premium_group.assets[0].f_long_num == 0 && premium_group.assets[0].f_short_num == 0,
        "setup has not charged funding yet"
    );

    env.svm.warp_to_slot(STALE_SLOT);
    env.push_auth_mark_for_asset_as_admin(1, STALE_SLOT, PRICE + PRICE / 100);
    env.crank(
        asset1_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: STALE_SLOT,
            observations: crank_observations(1),
        },
    );
    let (_, before_no_observation) = env.market_state();
    assert!(
        health_cert(&env.portfolio_state(account_a)).cert_oracle_epoch
            < before_no_observation.oracle_epoch,
        "asset-1 progress makes account A stale while asset-0 premium funding is pending"
    );
    assert_eq!(
        before_no_observation.assets[0].slot_last, PREMIUM_SLOT,
        "asset-0 still has an unaccrued premium window"
    );
    let profile0_before_no_observation =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap();
    let next_asset0_price = oracle_v16::effective_price_from_target(
        before_no_observation.assets[0].effective_price,
        profile0_before_no_observation.mark_ewma_e6,
        before_no_observation.config.max_price_move_bps_per_slot,
        STALE_SLOT - before_no_observation.assets[0].slot_last,
        true,
    );
    assert_ne!(
        next_asset0_price, before_no_observation.assets[0].effective_price,
        "setup must leave a real selected-asset mark move for the missing-observation guard"
    );

    let market_before = env.svm.get_account(&env.market).unwrap();
    let account_before = env.svm.get_account(&account_a).unwrap();
    env.svm.expire_blockhash();
    let omitted_observation = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: STALE_SLOT,
            observations: vec![],
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(account_a, false),
        ],
        &[],
    );
    assert!(
        omitted_observation.is_err(),
        "no-observation refresh must not advance asset-0 with zero funding while premium funding is pending"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected no-observation refresh must not consume the premium funding window"
    );
    assert_eq!(
        env.svm.get_account(&account_a).unwrap(),
        account_before,
        "rejected no-observation refresh must not certify the target account"
    );

    env.svm.expire_blockhash();
    let observed = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: STALE_SLOT,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(account_a, false),
        ],
        &[],
    );
    assert!(
        observed.is_ok(),
        "supplying the selected premium observation keeps the public refresh route live: {observed:?}"
    );
    let (_, funded_group) = env.market_state();
    assert!(
        funded_group.funding_epoch > before_no_observation.funding_epoch,
        "observed retry accrues the premium funding that no-observation refresh would have skipped"
    );
    assert_ne!(
        funded_group.assets[0].f_long_num, before_no_observation.assets[0].f_long_num,
        "asset-0 funding ledger advances on the observed retry"
    );
    assert_eq!(
        funded_group.assets[0].slot_last, STALE_SLOT,
        "observed retry consumes the premium window at the authenticated slot"
    );
}

#[test]
fn v16_attack_no_observation_liquidation_cannot_skip_premium_funding() {
    const INITIAL_PRICE: u64 = 1_000_000;
    const TARGET_PRICE: u64 = 3_000_000;
    const OPEN_SLOT: u64 = 1;
    const PREMIUM_SLOT: u64 = 2;
    const LIQUIDATION_SLOT: u64 = 3;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: INITIAL_PRICE,
        max_price_move_bps_per_slot: 1_000,
        max_accrual_dt_slots: 1,
        max_abs_funding_e9_per_slot: 1_000,
        min_funding_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(OPEN_SLOT);
    env.configure_auth_mark_with_cu(OPEN_SLOT, INITIAL_PRICE);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 100_000_000);
    env.deposit(&short_owner, short_account, 30_000_000);
    env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        (10 * POS_SCALE) as i128,
        INITIAL_PRICE,
        0,
    );

    env.svm.warp_to_slot(PREMIUM_SLOT);
    env.push_auth_mark_with_cu(PREMIUM_SLOT, TARGET_PRICE);
    env.crank(
        short_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: PREMIUM_SLOT,
            observations: crank_observations(0),
        },
    );
    let before = env.market_state().1;
    let before_short = env.portfolio_state(short_account);
    let before_cert = health_cert(&before_short);
    assert_eq!(before.assets[0].effective_price, 1_100_000);
    assert_eq!(before.assets[0].slot_last, PREMIUM_SLOT);
    assert_eq!(before.funding_epoch, 0);
    assert_eq!(before.assets[0].f_long_num, 0);
    assert_eq!(before.assets[0].f_short_num, 0);
    assert!(
        before_cert.certified_equity > 0 && before_cert.certified_liq_deficit > 0,
        "setup must be solvent but liquidatable with a current cert: {before_cert:?}"
    );

    env.svm.warp_to_slot(LIQUIDATION_SLOT);
    let market_before_omission = env.svm.get_account(&env.market).unwrap();
    let short_before_omission = env.svm.get_account(&short_account).unwrap();
    env.svm.expire_blockhash();
    let omitted = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: LIQUIDATION_SLOT,
            observations: vec![],
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short_account, false),
        ],
        &[],
    );
    assert!(
        omitted.is_err(),
        "a no-observation liquidation must not consume a pending premium-funding interval"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_omission
    );
    assert_eq!(
        env.svm.get_account(&short_account).unwrap(),
        short_before_omission
    );

    env.svm.expire_blockhash();
    let observed_refresh = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: LIQUIDATION_SLOT,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short_account, false),
        ],
        &[],
    );
    assert!(
        observed_refresh.is_ok(),
        "the authenticated retry must accrue and refresh: {observed_refresh:?}"
    );
    let after_funding = env.market_state().1;
    let refreshed_short = env.portfolio_state(short_account);
    assert!(after_funding.funding_epoch > before.funding_epoch);
    assert_ne!(after_funding.assets[0].f_long_num, 0);
    assert_ne!(after_funding.assets[0].f_short_num, 0);
    assert_eq!(after_funding.assets[0].slot_last, LIQUIDATION_SLOT);
    assert!(
        health_cert(&refreshed_short).certified_liq_deficit > 0,
        "the observed first step must leave a real liquidation continuation"
    );

    let oi_before_liquidation = after_funding.assets[0].oi_eff_short_q;
    env.svm.expire_blockhash();
    let liquidation = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: LIQUIDATION_SLOT,
            observations: vec![],
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short_account, false),
        ],
        &[],
    );
    assert!(
        liquidation.is_ok(),
        "after funding is booked, the same-slot no-observation liquidation must make progress: {liquidation:?}"
    );
    let after_liquidation = env.market_state().1;
    assert!(
        after_liquidation.assets[0].oi_eff_short_q < oi_before_liquidation,
        "liquidation must reduce the short's open interest"
    );
    assert_eq!(
        after_liquidation.assets[0].f_long_num,
        after_funding.assets[0].f_long_num
    );
    assert_eq!(
        after_liquidation.assets[0].f_short_num,
        after_funding.assets[0].f_short_num
    );
    assert_eq!(after_liquidation.vault as u64, env.token_amount(env.vault));
    assert!(after_liquidation.vault >= after_liquidation.c_tot + after_liquidation.insurance);
}
