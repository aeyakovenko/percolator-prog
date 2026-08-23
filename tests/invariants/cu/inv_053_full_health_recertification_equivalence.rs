//! INV-053 - Full-health recertification equivalence.
//!
//! Normative obligation: Fast or incremental certification is never more favorable than full recomputation.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_attack_pending_later_rounded_rescue_funding_requires_observation`, `v16_attack_no_observation_refresh_cannot_skip_premium_funding`, `v16_attack_no_observation_liquidation_cannot_skip_premium_funding`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: the first test certifies the minimized PR220/PR366 trace on the fixed
//! program and checks exact rollback plus the fully observed healthy control. A maximum-shape
//! matrix proves every one of the fourteen active slots is mandatory when it has pending accrual,
//! then executes the complete refresh below the CU ceiling. The stateful matrix supplies bounded
//! coverage over all four trade routes and both active-leg orders.

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

#[test]
fn v16_bpf_trade_refreshes_stale_related_portfolio_leg_on_demand() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(4, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(1, 0, 100);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 1_000_000_000);
    env.deposit(&short_owner, short_account, 1_000_000_000);

    env.trade_asset_with_cu(
        1,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        (10 * POS_SCALE) as i128,
        100,
        0,
    );

    let crank_long_owner = Keypair::new();
    let crank_short_owner = Keypair::new();
    let crank_long_account = env.create_portfolio(&crank_long_owner);
    let crank_short_account = env.create_portfolio(&crank_short_owner);
    env.deposit(&crank_long_owner, crank_long_account, 1_000_000_000);
    env.deposit(&crank_short_owner, crank_short_account, 1_000_000_000);
    env.trade_asset_with_cu(
        1,
        &crank_long_owner,
        crank_long_account,
        &crank_short_owner,
        crank_short_account,
        POS_SCALE as i128,
        100,
        0,
    );

    let (_, group_before_push) = env.market_state();
    let long_before_push = env.portfolio_state(long_account);
    let short_before_push = env.portfolio_state(short_account);
    assert_eq!(
        health_cert(&long_before_push).cert_oracle_epoch,
        group_before_push.oracle_epoch
    );
    assert_eq!(
        health_cert(&short_before_push).cert_oracle_epoch,
        group_before_push.oracle_epoch
    );

    env.svm.warp_to_slot(1);
    env.push_auth_mark_for_asset_as_admin(1, 1, 105);
    env.crank(
        crank_long_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(1),
        },
    );

    let (_, group_after_push) = env.market_state();
    let long_stale = env.portfolio_state(long_account);
    let short_stale = env.portfolio_state(short_account);
    assert_eq!(group_after_push.assets[1].effective_price, 105);
    assert!(
        health_cert(&long_stale).cert_oracle_epoch < group_after_push.oracle_epoch,
        "asset[1] mark push made the participating long portfolio cert stale"
    );
    assert!(
        health_cert(&short_stale).cert_oracle_epoch < group_after_push.oracle_epoch,
        "asset[1] mark push made the participating short portfolio cert stale"
    );
    assert_ne!(
        active_leg_for_asset(&long_stale, 1).k_snap,
        group_after_push.assets[1].k_long,
        "long asset[1] leg snapshot is stale before the asset[0] trade"
    );
    assert_ne!(
        active_leg_for_asset(&short_stale, 1).k_snap,
        group_after_push.assets[1].k_short,
        "short asset[1] leg snapshot is stale before the asset[0] trade"
    );

    let trade_cu = env.trade_asset_with_cu(
        0,
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        POS_SCALE as i128,
        100,
        0,
    );
    println!("v16 TradeNoCpi refreshes stale related leg on-demand CU: {trade_cu}");
    assert_cu_within(
        "TradeNoCpi on-demand refresh of stale related leg",
        trade_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );

    let (_, group_after_trade) = env.market_state();
    let long_after = env.portfolio_state(long_account);
    let short_after = env.portfolio_state(short_account);
    assert_eq!(
        health_cert(&long_after).cert_oracle_epoch,
        group_after_trade.oracle_epoch,
        "trade refreshed and re-certified the long account"
    );
    assert_eq!(
        health_cert(&short_after).cert_oracle_epoch,
        group_after_trade.oracle_epoch,
        "trade refreshed and re-certified the short account"
    );
    assert_eq!(
        active_leg_for_asset(&long_after, 1).k_snap,
        group_after_trade.assets[1].k_long,
        "trade settled the stale related long leg in-place"
    );
    assert_eq!(
        active_leg_for_asset(&short_after, 1).k_snap,
        group_after_trade.assets[1].k_short,
        "trade settled the stale related short leg in-place"
    );
    assert!(has_active_leg_for_asset(&long_after, 0));
    assert!(has_active_leg_for_asset(&short_after, 0));
    assert!(has_active_leg_for_asset(&long_after, 1));
    assert!(has_active_leg_for_asset(&short_after, 1));
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&long_after)),
        2
    );
    assert_eq!(
        percolator::active_bitmap_count_ones(active_bitmap(&short_after)),
        2
    );
}

#[test]
fn v16_bpf_tradecpi_refreshes_stale_traded_portfolio_leg_on_demand() {
    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
    env.configure_auth_mark_for_asset_as_admin(1, 0, 100);
    let matcher_program = Pubkey::new_unique();
    let matcher_bytes = std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
    env.svm.add_program(matcher_program, &matcher_bytes);

    let taker_owner = Keypair::new();
    let lp_owner = Keypair::new();
    let taker_account = env.create_portfolio(&taker_owner);
    let lp_account = env.create_portfolio(&lp_owner);
    env.deposit(&taker_owner, taker_account, 1_000_000_000);
    env.deposit(&lp_owner, lp_account, 1_000_000_000);
    let (ctx, delegate, _) = env.init_auth_matcher_context(matcher_program, &lp_owner, lp_account);

    let initial_size = (10 * POS_SCALE) as i128;
    env.trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker_account,
        &lp_owner,
        lp_account,
        matcher_program,
        ctx,
        delegate,
        1,
        initial_size,
        100,
    );

    let crank_long_owner = Keypair::new();
    let crank_short_owner = Keypair::new();
    let crank_long_account = env.create_portfolio(&crank_long_owner);
    let crank_short_account = env.create_portfolio(&crank_short_owner);
    env.deposit(&crank_long_owner, crank_long_account, 1_000_000_000);
    env.deposit(&crank_short_owner, crank_short_account, 1_000_000_000);
    env.trade_asset_with_cu(
        1,
        &crank_long_owner,
        crank_long_account,
        &crank_short_owner,
        crank_short_account,
        POS_SCALE as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(1);
    env.push_auth_mark_for_asset_as_admin(1, 1, 105);
    env.crank(
        crank_long_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: 1,
            observations: crank_observations(1),
        },
    );

    let (_, group_after_mark) = env.market_state();
    let taker_stale = env.portfolio_state(taker_account);
    let lp_stale = env.portfolio_state(lp_account);
    assert_eq!(group_after_mark.assets[1].effective_price, 105);
    assert_eq!(
        group_after_mark.assets[1].slot_last, 1,
        "test setup must make the traded market asset fresh"
    );
    assert!(
        health_cert(&taker_stale).cert_oracle_epoch < group_after_mark.oracle_epoch,
        "mark crank made the taker's traded asset leg stale"
    );
    assert!(
        health_cert(&lp_stale).cert_oracle_epoch < group_after_mark.oracle_epoch,
        "mark crank made the LP's traded asset leg stale"
    );
    assert_ne!(
        active_leg_for_asset(&taker_stale, 1).k_snap,
        group_after_mark.assets[1].k_long,
        "taker traded leg snapshot is stale before TradeCpi"
    );
    assert_ne!(
        active_leg_for_asset(&lp_stale, 1).k_snap,
        group_after_mark.assets[1].k_short,
        "LP traded leg snapshot is stale before TradeCpi"
    );

    let market_before_rejection = env.svm.get_account(&env.market).unwrap();
    let taker_before_rejection = env.svm.get_account(&taker_account).unwrap();
    let lp_before_rejection = env.svm.get_account(&lp_account).unwrap();
    let matcher_before_rejection = env.svm.get_account(&ctx).unwrap();
    let stale_error = env
        .try_trade_cpi_with_cu_on_asset(
            &taker_owner,
            taker_account,
            &lp_owner,
            lp_account,
            matcher_program,
            ctx,
            delegate,
            1,
            POS_SCALE as i128,
            100,
        )
        .expect_err("risk increase cannot consume an unsettled K/F cohort");
    assert!(
        stale_error.contains("Custom(21)") || stale_error.contains("custom program error: 0x15"),
        "stale-cohort CPI trade failed for the wrong reason: {stale_error}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_rejection
    );
    assert_eq!(
        env.svm.get_account(&taker_account).unwrap(),
        taker_before_rejection
    );
    assert_eq!(
        env.svm.get_account(&lp_account).unwrap(),
        lp_before_rejection
    );
    assert_eq!(env.svm.get_account(&ctx).unwrap(), matcher_before_rejection);

    for account in [taker_account, lp_account, crank_short_account] {
        env.crank(
            account,
            ProgInstruction::PermissionlessCrank {
                now_slot: 1,
                observations: vec![],
            },
        );
    }

    let trade_cu = env.trade_cpi_with_cu_on_asset(
        &taker_owner,
        taker_account,
        &lp_owner,
        lp_account,
        matcher_program,
        ctx,
        delegate,
        1,
        POS_SCALE as i128,
        100,
    );
    println!("v16 TradeCpi after explicit stale-cohort refresh CU: {trade_cu}");
    assert_cu_within(
        "TradeCpi after stale-cohort refresh",
        trade_cu,
        MULTI_ASSET_OPEN_TRADE_CU_LIMIT,
    );

    let (_, group_after_trade) = env.market_state();
    let taker_after = env.portfolio_state(taker_account);
    let lp_after = env.portfolio_state(lp_account);
    assert_eq!(
        health_cert(&taker_after).cert_oracle_epoch,
        group_after_trade.oracle_epoch,
        "TradeCpi refreshed and re-certified the taker"
    );
    assert_eq!(
        health_cert(&lp_after).cert_oracle_epoch,
        group_after_trade.oracle_epoch,
        "TradeCpi refreshed and re-certified the LP"
    );
    assert_eq!(
        active_leg_for_asset(&taker_after, 1).k_snap,
        group_after_trade.assets[1].k_long,
        "TradeCpi settled the stale traded taker leg"
    );
    assert_eq!(
        active_leg_for_asset(&lp_after, 1).k_snap,
        group_after_trade.assets[1].k_short,
        "TradeCpi settled the stale traded LP leg"
    );
    assert_eq!(
        active_leg_for_asset(&taker_after, 1).basis_pos_q,
        initial_size + POS_SCALE as i128,
        "TradeCpi increased the stale traded taker leg"
    );
    assert_eq!(
        active_leg_for_asset(&lp_after, 1).basis_pos_q,
        -(initial_size + POS_SCALE as i128),
        "TradeCpi increased the opposite stale traded LP leg"
    );
}

#[test]
fn v16_bpf_auth_mark_target_effective_lag_counts_toward_liquidation_health() {
    const INITIAL_MARK: u64 = 100_000_000;
    const TARGET_MARK: u64 = 90_000_000;
    const EXPECTED_EFFECTIVE_AFTER_ONE_SLOT: u64 = 99_760_000;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 24);
    env.svm.warp_to_slot(1);
    env.configure_auth_mark_with_cu(1, INITIAL_MARK);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_portfolio = env.create_portfolio(&long_owner);
    let short_portfolio = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_portfolio, 100_000_000);
    env.deposit(&short_owner, short_portfolio, 200_000_000);
    env.trade_with_cu(
        &long_owner,
        long_portfolio,
        &short_owner,
        short_portfolio,
        POS_SCALE as i128,
        INITIAL_MARK,
        0,
    );

    env.svm.warp_to_slot(2);
    env.push_auth_mark_with_cu(2, TARGET_MARK);
    env.crank(
        long_portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
    );

    let (_, lagged_group) = env.market_state();
    assert_eq!(
        lagged_group.assets[0].raw_oracle_target_price, TARGET_MARK,
        "AuthMark stores the un-clamped target for health certification"
    );
    assert_eq!(
        lagged_group.assets[0].effective_price, EXPECTED_EFFECTIVE_AFTER_ONE_SLOT,
        "effective price should be clamp-lagged by one 24 bps slot"
    );

    let lagged_long = env.portfolio_state(long_portfolio);
    assert!(
        health_cert(&lagged_long).valid,
        "refresh must write a health certificate"
    );
    assert!(
        health_cert(&lagged_long).certified_maintenance_req > INITIAL_MARK as u128,
        "maintenance must include the adverse target/effective lag penalty"
    );
    assert!(
        health_cert(&lagged_long).certified_liq_deficit > 0,
        "lagged adverse AuthMark target must make the under-margined long liquidatable"
    );

    env.crank_steps(
        long_portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
        2,
    );
    let liquidated_long = env.portfolio_state(long_portfolio);
    let remaining_q = if has_active_leg_for_asset(&liquidated_long, 0) {
        active_leg_for_asset(&liquidated_long, 0)
            .basis_pos_q
            .unsigned_abs()
    } else {
        0
    };
    assert!(
        remaining_q < POS_SCALE,
        "positive lag-deficit certification must allow risk-reducing liquidation"
    );
    assert_eq!(
        health_cert(&liquidated_long).certified_liq_deficit,
        0,
        "engine-selected lag liquidation restores health"
    );
}

#[test]
fn v16_program_max_shape_refresh_rejects_each_single_omitted_pending_leg() {
    const ASSET_COUNT: u16 = 14;
    const INITIAL_MARK: u64 = 100;
    const MOVED_MARK: u64 = 95;
    const REFRESH_SLOT: u64 = 2;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(ASSET_COUNT, 1_000, 1_000, 500);
    env.svm.warp_to_slot(1);
    for asset_index in 0..ASSET_COUNT {
        env.configure_auth_mark_for_asset_as_admin(asset_index, 1, INITIAL_MARK);
    }

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 10_000_000);
    env.deposit(&short_owner, short, 10_000_000);
    let legs = (0..ASSET_COUNT)
        .map(|asset_index| BatchTradeLeg {
            asset_index,
            market_id: first_generation_market_id(asset_index),
            size_q: POS_SCALE as i128,
            exec_price: INITIAL_MARK,
            fee_bps: 0,
        })
        .collect();
    env.send(
        env.batch_trade_no_cpi_ix(long, short, legs),
        vec![
            AccountMeta::new(long_owner.pubkey(), true),
            AccountMeta::new(short_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(long, false),
            AccountMeta::new(short, false),
        ],
        &[&long_owner, &short_owner],
    )
    .expect("open maximum-shape public portfolio");

    env.svm.warp_to_slot(REFRESH_SLOT);
    for asset_index in 0..ASSET_COUNT {
        env.push_auth_mark_for_asset_as_admin(asset_index, REFRESH_SLOT, MOVED_MARK);
    }
    let market_before = env.svm.get_account(&env.market).unwrap();
    let long_before = env.svm.get_account(&long).unwrap();
    let short_before = env.svm.get_account(&short).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    for omitted in 0..ASSET_COUNT {
        let observations = (0..ASSET_COUNT)
            .filter(|asset_index| *asset_index != omitted)
            .map(|asset_index| CrankObservationHint {
                asset_index,
                oracle_accounts: 0,
            })
            .collect();
        env.svm.expire_blockhash();
        let error = env
            .send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: REFRESH_SLOT,
                    observations,
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(long, false),
                ],
                &[],
            )
            .expect_err("omitting any pending active leg must reject full-account refresh");
        assert!(
            error.contains("Custom(22)") || error.contains("custom program error: 0x16"),
            "omitting asset {omitted} reached the wrong guard: {error}"
        );
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_before,
            "omitting asset {omitted} mutated market state"
        );
        assert_eq!(
            env.svm.get_account(&long).unwrap(),
            long_before,
            "omitting asset {omitted} mutated the refreshed portfolio"
        );
        assert_eq!(env.svm.get_account(&short).unwrap(), short_before);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
    }

    let asset_indices = (0..ASSET_COUNT).collect::<Vec<_>>();
    env.svm.expire_blockhash();
    let refresh_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: REFRESH_SLOT,
                observations: crank_observations_for_assets(&asset_indices),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(long, false),
            ],
            &[],
        )
        .expect("complete maximum-shape observation set must retain refresh liveness");
    println!("INV-053 complete 14-leg AuthMark refresh CU: {refresh_cu}");
    assert_cu_within("maximum-shape complete refresh", refresh_cu, 900_000);

    let after = env.market_state().1;
    let long_after = env.portfolio_state(long);
    for asset_index in 0..ASSET_COUNT as usize {
        assert_eq!(after.assets[asset_index].effective_price, MOVED_MARK);
        assert_eq!(
            active_leg_for_asset(&long_after, asset_index)
                .basis_pos_q
                .unsigned_abs(),
            POS_SCALE
        );
    }
    assert_eq!(
        health_cert(&long_after).cert_oracle_epoch,
        after.oracle_epoch
    );
    assert_eq!(env.svm.get_account(&short).unwrap(), short_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);
}
