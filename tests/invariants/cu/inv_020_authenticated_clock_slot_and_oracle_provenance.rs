//! INV-020 - Authenticated clock, slot, and oracle provenance.
//!
//! Normative obligation: Time and oracle observations are authenticated, coherent, and cannot be caller-rewound.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): public
//! recovery-oracle, Pyth confidence/staleness/owner/key/scalar-domain,
//! Chainlink and Switchboard owner/key/staleness/malformed-provider checks,
//! composite arithmetic, crank time monotonicity, and same-publish-time replay tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_attack_recovery_oracle_push_cannot_extend_force_close_deadline() {
    const SHUTDOWN_SLOT: u64 = 2;
    const FORCE_CLOSE_SLOT: u64 = 7;

    let mut env = V16CuEnv::new();
    let oracle_authority = Keypair::new();
    let cranker = Keypair::new();
    env.configure_permissionless_resolve_with_cu(100, 5);
    env.activate_asset_with_authorities(
        1,
        1,
        100,
        env.admin.pubkey(),
        env.admin.pubkey(),
        env.admin.pubkey(),
        oracle_authority.pubkey(),
    );
    env.configure_auth_mark_for_asset_with_authority(1, &oracle_authority, 1, 100);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 1_000_000);
    env.deposit(&short_owner, short, 1_000_000);
    env.trade_asset_with_cu(
        1,
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        100,
        0,
    );

    env.svm.warp_to_slot(SHUTDOWN_SLOT);
    env.update_asset_lifecycle_as_admin_with_cu(
        processor::ASSET_ACTION_SHUTDOWN,
        1,
        SHUTDOWN_SLOT,
        0,
    );
    let shutdown_market = env.svm.get_account(&env.market).unwrap();
    let shutdown_profile = state::read_asset_oracle_profile(&shutdown_market.data, 1).unwrap();
    assert_eq!(shutdown_profile.last_good_oracle_slot, SHUTDOWN_SLOT);
    assert_eq!(
        env.market_state().1.assets[1].lifecycle,
        AssetLifecycleV16::Recovery
    );

    // Try one slot before the original deadline. If this push were accepted, it
    // would move the force-close deadline from slot 7 to slot 11.
    env.svm.warp_to_slot(FORCE_CLOSE_SLOT - 1);
    env.svm.expire_blockhash();
    let push = env.send(
        ProgInstruction::PushAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 1,
            now_slot: FORCE_CLOSE_SLOT - 1,
            mark_e6: 101,
        },
        vec![
            AccountMeta::new(oracle_authority.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&oracle_authority],
    );
    assert!(
        push.is_err(),
        "Recovery asset must reject oracle timestamp refresh"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        shutdown_market,
        "rejected push must preserve the original shutdown epoch"
    );

    env.svm.warp_to_slot(FORCE_CLOSE_SLOT);
    let force_close_cu = env.force_close_abandoned_asset_with_cu(
        &cranker,
        long,
        short,
        1,
        FORCE_CLOSE_SLOT,
        POS_SCALE,
    );
    assert_cu_within(
        "force close at immutable Recovery deadline",
        force_close_cu,
        TRADE_CU_LIMIT,
    );
    let after = env.market_state().1;
    assert_eq!(after.assets[1].oi_eff_long_q, 0);
    assert_eq!(after.assets[1].oi_eff_short_q, 0);
    assert!(!has_active_leg_for_asset(&env.portfolio_state(long), 1));
    assert!(!has_active_leg_for_asset(&env.portfolio_state(short), 1));
}

fn configure_single_pyth(
    env: &mut V16CuEnv,
    feed: [u8; 32],
    account: Pubkey,
    slot: u64,
    now_unix: i64,
    conf_filter_bps: u16,
) -> Result<u64, String> {
    env.try_configure_hybrid_asset_with_conf_filter_cu(
        0,
        1,
        0,
        [feed, [0u8; 32], [0u8; 32]],
        &[account],
        slot,
        now_unix,
        0,
        0,
        100,
        conf_filter_bps,
    )
}

#[test]
fn v16_program_oracle_wide_confidence_feed_rejected() {
    let price = 200_000i64;
    let configure = |conf: u64| -> (bool, bool) {
        let mut env = V16CuEnv::new();
        set_test_clock(&mut env, 1, 100);
        let feed = [0x55u8; 32];
        let account = env.set_pyth_price_with_conf(&feed, price, -6, conf, 100);
        let before = env.svm.get_account(&env.market).unwrap().data;
        let result = configure_single_pyth(&mut env, feed, account, 1, 100, 200);
        let after = env.svm.get_account(&env.market).unwrap().data;
        (result.is_ok(), before == after)
    };

    assert!(configure((price / 1000) as u64).0);
    let (wide_ok, wide_unchanged) = configure((price / 2) as u64);
    assert!(!wide_ok);
    assert!(wide_unchanged);
    assert!(!configure((price as u64 * 250) / 10_000).0);
}

#[test]
fn v16_program_oracle_composite_zero_and_overmax_reject_without_panic() {
    {
        let mut env = V16CuEnv::new();
        set_test_clock(&mut env, 1, 100);
        let feeds = [[0x71u8; 32], [0x72u8; 32], [0x73u8; 32]];
        let l0 = env.set_pyth_price_with_conf(&feeds[0], 1, -6, 0, 100);
        let l1 = env.set_pyth_price_with_conf(&feeds[1], 4_000_000_000, -6, 0, 100);
        let l2 = env.set_pyth_price_with_conf(&feeds[2], 4_000_000_000, -6, 0, 100);
        let before = env.svm.get_account(&env.market).unwrap().data;
        let result = env.try_configure_hybrid_asset_with_conf_filter_cu(
            0,
            3,
            ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3,
            feeds,
            &[l0, l1, l2],
            1,
            100,
            1,
            0,
            100,
            0,
        );
        let err = result.expect_err("inverting a zero-flooring composite must reject");
        assert!(err.contains("Custom(26)"));
        assert!(!err.contains("panic") && !err.contains("ProgramFailedToComplete"));
        assert_eq!(env.svm.get_account(&env.market).unwrap().data, before);
    }

    {
        let mut env = V16CuEnv::new();
        set_test_clock(&mut env, 1, 100);
        let feeds = [[0x81u8; 32], [0x82u8; 32], [0x83u8; 32]];
        let l0 = env.set_pyth_price_with_conf(&feeds[0], 4_000_000_000, -6, 0, 100);
        let zero_divisor = env.set_pyth_price_with_conf(&feeds[1], 0, -6, 0, 100);
        let l2 = env.set_pyth_price_with_conf(&feeds[2], 200_000_000, -6, 0, 100);
        let before = env.svm.get_account(&env.market).unwrap().data;
        let result = env.try_configure_hybrid_asset_with_conf_filter_cu(
            0,
            3,
            ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3,
            feeds,
            &[l0, zero_divisor, l2],
            1,
            100,
            0,
            0,
            100,
            0,
        );
        let err = result.expect_err("zero divide leg must reject");
        assert!(err.contains("Custom(26)"));
        assert!(!err.contains("panic") && !err.contains("ProgramFailedToComplete"));
        assert_eq!(env.svm.get_account(&env.market).unwrap().data, before);
    }

    {
        let mut env = V16CuEnv::new();
        set_test_clock(&mut env, 1, 100);
        let feeds = [[0x91u8; 32], [0x92u8; 32], [0x93u8; 32]];
        let l0 = env.set_pyth_price_with_conf(&feeds[0], 4_000_000_000, -6, 0, 100);
        let l1 = env.set_pyth_price_with_conf(&feeds[1], 1, -6, 0, 100);
        let l2 = env.set_pyth_price_with_conf(&feeds[2], 1, -6, 0, 100);
        let before = env.svm.get_account(&env.market).unwrap().data;
        let result = env.try_configure_hybrid_asset_with_conf_filter_cu(
            0,
            3,
            ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3,
            feeds,
            &[l0, l1, l2],
            1,
            100,
            0,
            0,
            100,
            0,
        );
        let err = result.expect_err("over-max composite price must reject");
        assert!(err.contains("Custom(26)"));
        assert!(!err.contains("panic") && !err.contains("ProgramFailedToComplete"));
        assert_eq!(env.svm.get_account(&env.market).unwrap().data, before);
    }
}

#[test]
fn v16_program_oracle_staleness_bound_exact() {
    let configure = |publish_time: i64| -> bool {
        let mut env = V16CuEnv::new();
        set_test_clock(&mut env, 1, 1000);
        let feed = [0x55u8; 32];
        let account = env.set_pyth_price_with_conf(&feed, 200_000, -6, 0, publish_time);
        configure_single_pyth(&mut env, feed, account, 1, 1000, 0).is_ok()
    };

    assert!(configure(950));
    assert!(configure(940));
    assert!(!configure(939));
    assert!(!configure(930));
}

#[test]
fn v16_program_oracle_feed_owner_and_id_binding_reject_spoofed_pyth() {
    let mut env = V16CuEnv::new();
    set_test_clock(&mut env, 1, 100);
    let expected_feed = [0x55u8; 32];
    let wrong_feed = [0x56u8; 32];
    let wrong_account = env.set_pyth_price_with_conf(&wrong_feed, 200_000, -6, 0, 100);
    let before = env.svm.get_account(&env.market).unwrap().data;
    let wrong_key = configure_single_pyth(&mut env, expected_feed, wrong_account, 1, 100, 0);
    let err = wrong_key.expect_err("wrong Pyth feed id must reject");
    assert!(err.contains("Custom(29)"));
    assert_eq!(env.svm.get_account(&env.market).unwrap().data, before);

    let forged_account = Pubkey::new_unique();
    env.svm
        .set_account(
            forged_account,
            Account {
                lamports: 1_000_000_000,
                data: make_pyth_data(&expected_feed, 200_000, -6, 0, 100),
                owner: Pubkey::new_unique(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    let fake_owner = configure_single_pyth(&mut env, expected_feed, forged_account, 1, 100, 0);
    let err = fake_owner.expect_err("attacker-owned Pyth-shaped data must reject");
    assert!(err.contains("IllegalOwner"));
    assert_eq!(env.svm.get_account(&env.market).unwrap().data, before);

    let real = env.set_pyth_price_with_conf(&expected_feed, 200_000, -6, 0, 100);
    env.svm.expire_blockhash();
    configure_single_pyth(&mut env, expected_feed, real, 1, 100, 0)
        .expect("matching Pyth feed id and owner configures");
}

#[test]
fn v16_program_crank_oracle_same_publish_time_price_change_rejects() {
    let mut env = V16CuEnv::new();
    set_test_clock(&mut env, 1, 100);
    let feed = [0x79u8; 32];
    let initial = env.set_pyth_price_with_conf(&feed, 200_000, -6, 0, 100);
    configure_single_pyth(&mut env, feed, initial, 1, 100, 0)
        .expect("configure matching one-leg hybrid oracle");

    let cranker_owner = Keypair::new();
    let cranker_portfolio = env.create_portfolio(&cranker_owner);
    set_test_clock(&mut env, 2, 100);
    let changed_same_publish = env.set_pyth_price_with_conf(&feed, 300_000, -6, 0, 100);
    let market_before = env.svm.get_account(&env.market).unwrap().data;
    let portfolio_before = env.svm.get_account(&cranker_portfolio).unwrap().data;

    env.svm.expire_blockhash();
    let replay = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations_with_accounts(0, 1),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(cranker_portfolio, false),
            AccountMeta::new_readonly(changed_same_publish, false),
        ],
        &[],
    );
    let err = replay.expect_err("same-publish-time price change must reject");
    assert!(err.contains("Custom(26)"));
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        market_before
    );
    assert_eq!(
        env.svm.get_account(&cranker_portfolio).unwrap().data,
        portfolio_before
    );

    set_test_clock(&mut env, 3, 101);
    let fresh = env.set_pyth_price_with_conf(&feed, 210_000, -6, 0, 101);
    env.svm.expire_blockhash();
    env.crank_with_oracle_tail(
        cranker_portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
        &[fresh],
    );
    let (cfg, group) = env.market_state();
    assert_eq!(cfg.last_good_oracle_slot, 3);
    assert_eq!(group.assets[0].raw_oracle_target_price, 210_000);
}

// (complements the composite trio #167/#168/#169).
#[test]
fn v16_program_oracle_unit_scale_transform_correct_and_floor0_rejected() {
    let configure = |scale: u32| -> (bool, u64, Option<String>) {
        let mut env = V16CuEnv::new();
        set_test_clock(&mut env, 1, 100);
        let feed = [0x55u8; 32];
        let acct = env.set_pyth_price_with_conf(&feed, 200_000, -6, 0, 100); // price_e6 = 200_000
        let r = env.try_configure_hybrid_asset_with_conf_filter_cu(
            0,
            1,
            0,
            [feed, [0u8; 32], [0u8; 32]],
            &[acct],
            1,
            100,
            0,
            scale,
            100,
            0,
        );
        let eff = if r.is_ok() {
            env.market_state().1.assets[0].effective_price
        } else {
            0
        };
        (r.is_ok(), eff, r.err())
    };

    // VALID scale: mark is the EXACT divided price (200_000 / 2 = 100_000).
    let (ok2, eff2, _) = configure(2);
    assert!(ok2, "a valid unit_scale configures");
    assert_eq!(
        eff2, 100_000,
        "unit_scale divides the mark exactly (200_000/2)"
    );
    // and unit_scale 4 -> 50_000.
    let (ok4, eff4, _) = configure(4);
    assert!(
        ok4 && eff4 == 50_000,
        "unit_scale=4 -> 200_000/4 = 50_000, got {}",
        eff4
    );

    // FLOOR-TO-0: a unit_scale > the price floors the mark to 0 -> rejected (no zero/garbage mark).
    let (ok_floor, _, floor_err) = configure(300_000);
    assert!(
        !ok_floor,
        "a unit_scale that floors the price to 0 must reject"
    );
    let e = floor_err.unwrap();
    assert!(
        e.contains("Custom(26)"),
        "floor-to-0 must be OracleInvalid (Custom 26), got: {}",
        e
    );
}

// only a positive price; negative or zero -> OracleInvalid. Completes the oracle price-domain coverage.
#[test]
fn v16_program_oracle_negative_or_zero_price_rejected() {
    let configure = |price: i64| -> (bool, Option<String>) {
        let mut env = V16CuEnv::new();
        set_test_clock(&mut env, 1, 100);
        let feed = [0x55u8; 32];
        let acct = env.set_pyth_price_with_conf(&feed, price, -6, 0, 100);
        let r = env.try_configure_hybrid_asset_with_conf_filter_cu(
            0,
            1,
            0,
            [feed, [0u8; 32], [0u8; 32]],
            &[acct],
            1,
            100,
            0,
            0,
            100,
            0,
        );
        (r.is_ok(), r.err())
    };
    // a positive price configures.
    let (pos_ok, _) = configure(200_000);
    assert!(pos_ok, "a positive Pyth price configures");
    // a NEGATIVE price is rejected (no sign-flipped mark).
    let (neg_ok, neg_err) = configure(-200_000);
    assert!(!neg_ok, "a negative Pyth price must reject");
    assert!(
        neg_err.unwrap().contains("Custom(26)"),
        "negative price must be OracleInvalid (Custom 26)"
    );
    // a ZERO price is rejected (no zero mark / downstream div-by-zero).
    let (zero_ok, zero_err) = configure(0);
    assert!(!zero_ok, "a zero Pyth price must reject");
    assert!(
        zero_err.unwrap().contains("Custom(26)"),
        "zero price must be OracleInvalid (Custom 26)"
    );
}

// through the public ConfigureHybridOracle path without partially installing a bad oracle profile.
#[test]
fn v16_program_chainlink_oracle_malformed_fields_reject_without_mutation() {
    let mut env = V16CuEnv::new();
    set_test_clock(&mut env, 1, 100);
    let valid = || make_chainlink_data(1, 8, 1, 1, 1, 100, 10_000);
    let install = |env: &mut V16CuEnv, data: Vec<u8>| -> Pubkey {
        let key = Pubkey::new_unique();
        env.svm
            .set_account(
                key,
                Account {
                    lamports: 1_000_000_000,
                    data,
                    owner: oracle_v16::CHAINLINK_STORE_PROGRAM_ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        key
    };

    let before = env.svm.get_account(&env.market).unwrap().data;
    let mut cases: Vec<(&str, Vec<u8>)> = Vec::new();
    let mut bad_disc = valid();
    bad_disc[0] ^= 0xff;
    cases.push(("bad discriminator", bad_disc));
    cases.push((
        "zero version",
        make_chainlink_data(0, 8, 1, 1, 1, 100, 10_000),
    ));
    cases.push((
        "zero latest round",
        make_chainlink_data(1, 8, 0, 1, 1, 100, 10_000),
    ));
    cases.push((
        "non-single live length",
        make_chainlink_data(1, 8, 1, 2, 1, 100, 10_000),
    ));
    cases.push(("zero slot", make_chainlink_data(1, 8, 1, 1, 0, 100, 10_000)));
    cases.push((
        "zero publish time",
        make_chainlink_data(1, 8, 1, 1, 1, 0, 10_000),
    ));
    cases.push((
        "decimals over bound",
        make_chainlink_data(1, 19, 1, 1, 1, 100, 10_000),
    ));
    cases.push((
        "negative answer",
        make_chainlink_data(1, 8, 1, 1, 1, 100, -10_000),
    ));

    for (label, data) in cases {
        let acct = install(&mut env, data);
        env.svm.expire_blockhash();
        let rejected = env.try_configure_hybrid_asset_with_conf_filter_cu(
            0,
            1,
            0,
            [acct.to_bytes(), [0u8; 32], [0u8; 32]],
            &[acct],
            1,
            100,
            0,
            0,
            100,
            0,
        );
        assert!(rejected.is_err(), "malformed Chainlink {label} must reject");
        assert_eq!(
            env.svm.get_account(&env.market).unwrap().data,
            before,
            "malformed Chainlink {label} must not mutate market state"
        );
    }

    let ok_acct = install(&mut env, valid());
    env.svm.expire_blockhash();
    let ok = env.try_configure_hybrid_asset_with_conf_filter_cu(
        0,
        1,
        0,
        [ok_acct.to_bytes(), [0u8; 32], [0u8; 32]],
        &[ok_acct],
        1,
        100,
        0,
        0,
        100,
        0,
    );
    assert!(ok.is_ok(), "valid Chainlink leg configures: {ok:?}");
    assert_eq!(
        env.market_state().1.assets[0].effective_price,
        100,
        "valid Chainlink answer/decimals seeds the expected mark"
    );
}

// transmissions account under the wrong key, or under an attacker owner, must reject atomically.
#[test]
fn v16_program_chainlink_owner_and_key_binding_reject_spoofed_feed() {
    let mut env = V16CuEnv::new();
    set_test_clock(&mut env, 1, 100);
    let install = |env: &mut V16CuEnv, owner: Pubkey| -> Pubkey {
        let key = Pubkey::new_unique();
        env.svm
            .set_account(
                key,
                Account {
                    lamports: 1_000_000_000,
                    data: make_chainlink_data(1, 8, 1, 1, 1, 100, 10_000),
                    owner,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        key
    };
    let configure = |env: &mut V16CuEnv, expected_feed: Pubkey, account: Pubkey| {
        env.try_configure_hybrid_asset_with_conf_filter_cu(
            0,
            1,
            0,
            [expected_feed.to_bytes(), [0u8; 32], [0u8; 32]],
            &[account],
            1,
            100,
            0,
            0,
            100,
            0,
        )
    };
    let before = env.svm.get_account(&env.market).unwrap().data;

    let wrong_key = install(&mut env, oracle_v16::CHAINLINK_STORE_PROGRAM_ID);
    let expected_key = Pubkey::new_unique();
    env.svm.expire_blockhash();
    let key_spoof = configure(&mut env, expected_key, wrong_key);
    assert!(
        key_spoof.is_err(),
        "a Chainlink transmissions account must match the configured account key"
    );
    let key_err = key_spoof.unwrap_err();
    assert!(
        key_err.contains("Custom(29)"),
        "wrong Chainlink account key must reject as InvalidOracleKey (Custom 29), got: {key_err}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before,
        "wrong-key Chainlink spoof must not mutate the market"
    );

    let fake_owner = install(&mut env, Pubkey::new_unique());
    env.svm.expire_blockhash();
    let owner_spoof = configure(&mut env, fake_owner, fake_owner);
    assert!(
        owner_spoof.is_err(),
        "attacker-owned Chainlink-shaped data must reject before parsing the price"
    );
    let owner_err = owner_spoof.unwrap_err();
    assert!(
        owner_err.contains("IllegalOwner"),
        "attacker-owned Chainlink feed must reject as IllegalOwner, got: {owner_err}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before,
        "attacker-owned Chainlink spoof must not mutate the market"
    );

    let valid = install(&mut env, oracle_v16::CHAINLINK_STORE_PROGRAM_ID);
    env.svm.expire_blockhash();
    configure(&mut env, valid, valid).expect("real Chainlink owner/key pair should configure");
    assert_eq!(
        env.market_state().1.assets[0].effective_price,
        100,
        "valid Chainlink owner/key pair seeds the expected mark"
    );
}

// an outdated mark. This covers the Chainlink-specific timestamp offset and OracleStale branch.
#[test]
fn v16_program_chainlink_stale_feed_rejected_without_mutation() {
    let mut env = V16CuEnv::new();
    set_test_clock(&mut env, 10, 1_000);
    let install = |env: &mut V16CuEnv, publish_time: u32| -> Pubkey {
        let key = Pubkey::new_unique();
        env.svm
            .set_account(
                key,
                Account {
                    lamports: 1_000_000_000,
                    data: make_chainlink_data(1, 8, 1, 1, 1, publish_time, 10_000),
                    owner: oracle_v16::CHAINLINK_STORE_PROGRAM_ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        key
    };
    let configure = |env: &mut V16CuEnv, feed: Pubkey| {
        env.try_configure_hybrid_asset_with_conf_filter_cu(
            0,
            1,
            0,
            [feed.to_bytes(), [0u8; 32], [0u8; 32]],
            &[feed],
            10,
            1_000,
            0,
            0,
            100,
            0,
        )
    };
    let stale = install(&mut env, 1);
    let before = env.svm.get_account(&env.market).unwrap().data;

    env.svm.expire_blockhash();
    let rejected = configure(&mut env, stale);
    assert!(
        rejected.is_err(),
        "a stale Chainlink feed must reject instead of seeding the oracle"
    );
    let err = rejected.unwrap_err();
    assert!(
        err.contains("Custom(27)"),
        "stale Chainlink feed must reject as OracleStale (Custom 27), got: {err}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before,
        "rejected stale Chainlink config must not mutate the market"
    );

    let fresh = install(&mut env, 1_000);
    env.svm.expire_blockhash();
    configure(&mut env, fresh).expect("fresh Chainlink feed should configure");
    assert_eq!(
        env.market_state().1.assets[0].effective_price,
        100,
        "fresh Chainlink answer/decimals seeds the expected mark"
    );
}

// ConfigureHybridOracle without installing a malformed oracle profile.
#[test]
fn v16_program_oracle_pyth_exponent_bounds_enforced() {
    let configure = |price: i64, expo: i32| -> (bool, bool) {
        let mut env = V16CuEnv::new();
        set_test_clock(&mut env, 1, 100);
        let feed = [0xa5u8; 32];
        let acct = env.set_pyth_price_with_conf(&feed, price, expo, 0, 100);
        let before = env.svm.get_account(&env.market).unwrap().data;
        let result = env.try_configure_hybrid_asset_with_conf_filter_cu(
            0,
            1,
            0,
            [feed, [0u8; 32], [0u8; 32]],
            &[acct],
            1,
            100,
            0,
            0,
            100,
            0,
        );
        let after = env.svm.get_account(&env.market).unwrap().data;
        (result.is_ok(), before == after)
    };

    let (ok, _) = configure(200_000, -6);
    assert!(ok, "normal Pyth exponent configures");

    for (label, price, expo) in [
        ("positive exponent over bound", 1, 19),
        ("negative exponent over bound", 1_000_000_000, -19),
        ("positive exponent scales over max", 1, 18),
        ("negative exponent floors to zero", 1, -18),
    ] {
        let (configured, unchanged) = configure(price, expo);
        assert!(
            !configured,
            "Pyth {label} must reject instead of installing a malformed mark"
        );
        assert!(
            unchanged,
            "Pyth {label} rejection must leave market state unchanged"
        );
    }
}

// (read_switchboard_price_e6: age > max_staleness_secs -> OracleStale), same as the Pyth path.
#[test]
fn v16_program_switchboard_stale_feed_rejected() {
    let mut env = V16CuEnv::new();
    env.svm.warp_to_slot(10);
    let mut clock = env.svm.get_sysvar::<Clock>();
    clock.unix_timestamp = 100_000;
    env.svm.set_sysvar(&clock);
    // publish_time = 1, authenticated now = 100_000 -> age ~100_000 >> max_staleness_secs -> stale.
    let sb = env.set_switchboard_price(100 * 1_000_000_000_000, 1, 1);
    let r = env.try_configure_hybrid_with_cu(
        1,
        0,
        [sb.to_bytes(), [0u8; 32], [0u8; 32]],
        &[sb],
        10,
        100_000,
        0,
        0,
        3,
    );
    assert!(
        r.is_err(),
        "a stale Switchboard feed must be rejected (OracleStale), not seed the oracle"
    );
}

// A wide-variance feed must not install an uncertain mark, and the rejection must roll back the market.
#[test]
fn v16_program_switchboard_wide_std_dev_rejected_without_mutation() {
    let mut env = V16CuEnv::new();
    set_test_clock(&mut env, 10, 1_000);
    let value: i128 = 100 * 1_000_000_000_000;
    let wide_std_dev: i128 = 2 * 1_000_000_000_000; // 2% of value, over the 1% filter below.
    let sb = env.set_switchboard_price(value, wide_std_dev, 1_000);
    let before = env.svm.get_account(&env.market).unwrap().data;

    let rejected = env.try_configure_hybrid_asset_with_conf_filter_cu(
        0,
        1,
        0,
        [sb.to_bytes(), [0u8; 32], [0u8; 32]],
        &[sb],
        10,
        1_000,
        0,
        0,
        3,
        100,
    );
    assert!(
        rejected.is_err(),
        "a Switchboard feed over the configured std_dev confidence bound must reject"
    );
    let err = rejected.unwrap_err();
    assert!(
        err.contains("Custom(28)"),
        "wide Switchboard std_dev must reject as OracleConfTooWide (Custom 28), got: {err}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before,
        "rejected wide-std-dev Switchboard config must not mutate the market"
    );

    let tight = env.set_switchboard_price(value, 1, 1_000);
    env.try_configure_hybrid_asset_with_conf_filter_cu(
        0,
        1,
        0,
        [tight.to_bytes(), [0u8; 32], [0u8; 32]],
        &[tight],
        10,
        1_000,
        0,
        0,
        3,
        100,
    )
    .expect("tight Switchboard std_dev should configure under the same confidence filter");
    assert_eq!(
        env.market_state().1.assets[0].effective_price,
        100,
        "accepted Switchboard value/1e12 seeds the expected mark"
    );
}

// the sample count satisfies the feed's declared minimum.
#[test]
fn v16_program_switchboard_low_sample_quorum_rejected_without_mutation() {
    let mut env = V16CuEnv::new();
    set_test_clock(&mut env, 10, 1_000);
    let value: i128 = 100 * 1_000_000_000_000;
    let install = |env: &mut V16CuEnv, num_samples: u8, min_sample_size: u8| -> Pubkey {
        let key = Pubkey::new_unique();
        env.svm
            .set_account(
                key,
                Account {
                    lamports: 1_000_000_000,
                    data: make_switchboard_data(
                        &[0xABu8; 32],
                        value,
                        1,
                        1_000,
                        num_samples,
                        min_sample_size,
                        1,
                    ),
                    owner: oracle_v16::SWITCHBOARD_ON_DEMAND_MAINNET_PROGRAM_ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        key
    };
    let configure = |env: &mut V16CuEnv, feed: Pubkey| {
        env.try_configure_hybrid_asset_with_conf_filter_cu(
            0,
            1,
            0,
            [feed.to_bytes(), [0u8; 32], [0u8; 32]],
            &[feed],
            10,
            1_000,
            0,
            0,
            3,
            100,
        )
    };
    let before = env.svm.get_account(&env.market).unwrap().data;

    for (label, num_samples, min_sample_size) in [("zero minimum", 3, 0), ("below minimum", 1, 2)] {
        let bad = install(&mut env, num_samples, min_sample_size);
        env.svm.expire_blockhash();
        let rejected = configure(&mut env, bad);
        assert!(
            rejected.is_err(),
            "Switchboard {label} sample quorum must reject"
        );
        let err = rejected.unwrap_err();
        assert!(
            err.contains("Custom(26)"),
            "Switchboard {label} sample quorum must reject as OracleInvalid (Custom 26), got: {err}"
        );
        assert_eq!(
            env.svm.get_account(&env.market).unwrap().data,
            before,
            "rejected Switchboard {label} sample quorum must not mutate the market"
        );
    }

    let valid = install(&mut env, 2, 2);
    env.svm.expire_blockhash();
    configure(&mut env, valid).expect("Switchboard sample quorum should configure once satisfied");
    assert_eq!(
        env.market_state().1.assets[0].effective_price,
        100,
        "accepted Switchboard quorum value/1e12 seeds the expected mark"
    );
}

// looking account under the wrong key, or under an attacker owner, must not seed the oracle profile.
#[test]
fn v16_program_switchboard_owner_and_key_binding_reject_spoofed_feed() {
    let mut env = V16CuEnv::new();
    set_test_clock(&mut env, 10, 1_000);
    let value: i128 = 100 * 1_000_000_000_000;
    let install = |env: &mut V16CuEnv, owner: Pubkey| -> Pubkey {
        let key = Pubkey::new_unique();
        env.svm
            .set_account(
                key,
                Account {
                    lamports: 1_000_000_000,
                    data: make_switchboard_data(&[0xABu8; 32], value, 1, 1_000, 3, 1, 1),
                    owner,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        key
    };
    let configure = |env: &mut V16CuEnv, expected_feed: Pubkey, account: Pubkey| {
        env.try_configure_hybrid_asset_with_conf_filter_cu(
            0,
            1,
            0,
            [expected_feed.to_bytes(), [0u8; 32], [0u8; 32]],
            &[account],
            10,
            1_000,
            0,
            0,
            3,
            100,
        )
    };
    let before = env.svm.get_account(&env.market).unwrap().data;

    let wrong_key = install(
        &mut env,
        oracle_v16::SWITCHBOARD_ON_DEMAND_MAINNET_PROGRAM_ID,
    );
    let expected_key = Pubkey::new_unique();
    env.svm.expire_blockhash();
    let key_spoof = configure(&mut env, expected_key, wrong_key);
    assert!(
        key_spoof.is_err(),
        "a Switchboard PullFeed account must match the configured account key"
    );
    let key_err = key_spoof.unwrap_err();
    assert!(
        key_err.contains("Custom(29)"),
        "wrong Switchboard account key must reject as InvalidOracleKey (Custom 29), got: {key_err}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before,
        "wrong-key Switchboard spoof must not mutate the market"
    );

    let fake_owner = install(&mut env, Pubkey::new_unique());
    env.svm.expire_blockhash();
    let owner_spoof = configure(&mut env, fake_owner, fake_owner);
    assert!(
        owner_spoof.is_err(),
        "attacker-owned Switchboard-shaped data must reject before parsing the price"
    );
    let owner_err = owner_spoof.unwrap_err();
    assert!(
        owner_err.contains("IllegalOwner"),
        "attacker-owned Switchboard feed must reject as IllegalOwner, got: {owner_err}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before,
        "attacker-owned Switchboard spoof must not mutate the market"
    );

    let valid = install(
        &mut env,
        oracle_v16::SWITCHBOARD_ON_DEMAND_MAINNET_PROGRAM_ID,
    );
    env.svm.expire_blockhash();
    configure(&mut env, valid, valid).expect("real Switchboard owner/key pair should configure");
    assert_eq!(
        env.market_state().1.assets[0].effective_price,
        100,
        "valid Switchboard owner/key pair seeds the expected mark"
    );
}

// advertise a divide leg that has no corresponding account.
#[test]
fn v16_program_hybrid_oracle_rejects_duplicate_or_malformed_leg_config() {
    let mut env = V16CuEnv::new();
    set_test_clock(&mut env, 1, 100);
    let feed0 = [0xa0u8; 32];
    let feed1 = [0xa1u8; 32];
    let feed2 = [0xa2u8; 32];
    let oracle0 = env.set_pyth_price_with_conf(&feed0, 200_000, -6, 0, 100);
    let oracle1 = env.set_pyth_price_with_conf(&feed1, 300_000, -6, 0, 100);
    let oracle2 = env.set_pyth_price_with_conf(&feed2, 400_000, -6, 0, 100);
    let configure =
        |env: &mut V16CuEnv, count: u8, flags: u8, feeds: [[u8; 32]; 3], accounts: &[Pubkey]| {
            env.try_configure_hybrid_asset_with_conf_filter_cu(
                0, count, flags, feeds, accounts, 1, 100, 0, 0, 100, 0,
            )
        };
    let before = env.svm.get_account(&env.market).unwrap().data;

    for (label, count, flags, feeds, accounts) in [
        (
            "count-1 stray second feed",
            1,
            0,
            [feed0, feed1, [0u8; 32]],
            vec![oracle0],
        ),
        (
            "count-2 duplicate first feed",
            2,
            0,
            [feed0, feed0, [0u8; 32]],
            vec![oracle0, oracle1],
        ),
        (
            "count-2 divide third flag",
            2,
            ORACLE_LEG_FLAG_DIVIDE_LEG3,
            [feed0, feed1, [0u8; 32]],
            vec![oracle0, oracle1],
        ),
        (
            "count-3 duplicate third feed",
            3,
            0,
            [feed0, feed1, feed1],
            vec![oracle0, oracle1, oracle2],
        ),
    ] {
        env.svm.expire_blockhash();
        let rejected = configure(&mut env, count, flags, feeds, &accounts);
        assert!(rejected.is_err(), "hybrid oracle {label} must reject");
        assert_eq!(
            env.svm.get_account(&env.market).unwrap().data,
            before,
            "rejected hybrid oracle {label} must not mutate the market"
        );
    }

    env.svm.expire_blockhash();
    configure(&mut env, 1, 0, [feed0, [0u8; 32], [0u8; 32]], &[oracle0])
        .expect("well-formed one-leg hybrid oracle should configure");
    assert_eq!(
        env.market_state().1.assets[0].effective_price,
        200_000,
        "valid one-leg control seeds the expected mark"
    );
}

// entrypoints. The legitimate update path remains PermissionlessCrank with the oracle tail.
#[test]
fn v16_program_pushed_mark_cannot_override_external_oracle_asset() {
    let mut env = V16CuEnv::new();
    set_test_clock(&mut env, 1, 100);
    let feed = [0xb6u8; 32];
    let initial = env.set_pyth_price_with_conf(&feed, 100, 0, 0, 100);
    env.try_configure_hybrid_asset_with_conf_filter_cu(
        0,
        1,
        0,
        [feed, [0u8; 32], [0u8; 32]],
        &[initial],
        1,
        100,
        0,
        0,
        100,
        0,
    )
    .expect("configure a one-leg external oracle");
    assert_eq!(env.market_state().1.assets[0].effective_price, 100_000_000);

    let admin = env.admin.insecure_clone();
    let hybrid_before = env.svm.get_account(&env.market).unwrap();
    set_test_clock(&mut env, 2, 101);
    env.svm.expire_blockhash();
    let auth_push = env.send(
        ProgInstruction::PushAuthMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 2,
            mark_e6: 9_999_999,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        auth_push.is_err(),
        "PushAuthMark must not override a Hybrid/external oracle asset"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        hybrid_before,
        "rejected PushAuthMark leaves the Hybrid oracle profile unchanged"
    );

    env.svm.expire_blockhash();
    let ewma_push = env.send(
        ProgInstruction::PushEwmaMark {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 2,
            mark_e6: 9_999_999,
        },
        vec![
            AccountMeta::new(admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&admin],
    );
    assert!(
        ewma_push.is_err(),
        "PushEwmaMark must not override a Hybrid/external oracle asset"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        hybrid_before,
        "rejected PushEwmaMark leaves the Hybrid oracle profile unchanged"
    );

    let cranker = Keypair::new();
    let cranker_portfolio = env.create_portfolio(&cranker);
    let updated = env.set_pyth_price_with_conf(&feed, 110, 0, 0, 101);
    env.svm.expire_blockhash();
    env.crank_with_oracle_tail(
        cranker_portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
        &[updated],
    );
    assert_eq!(
        env.market_state().1.assets[0].raw_oracle_target_price,
        110_000_000,
        "external oracle tail remains the valid update path"
    );
}

// rejects on NotEnoughAccountKeys before any mutation; the correctly-sized config configures.
#[test]
fn v16_program_oracle_legcount_account_mismatch_rejects_clean() {
    let mut env = V16CuEnv::new();
    set_test_clock(&mut env, 1, 100);
    let feeds = [[0xb1u8; 32], [0xb2u8; 32], [0xb3u8; 32]];
    let a0 = env.set_pyth_price_with_conf(&feeds[0], 4_000_000_000, -6, 0, 100);
    let a1 = env.set_pyth_price_with_conf(&feeds[1], 150_000_000, -6, 0, 100);
    let a2 = env.set_pyth_price_with_conf(&feeds[2], 200_000_000, -6, 0, 100);
    let flags = ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3;
    let before = env.svm.get_account(&env.market).unwrap().data;

    // declare a 3-leg composite but supply ONLY 1 oracle account -> reject, market unmutated.
    let r_bad = env.try_configure_hybrid_asset_with_conf_filter_cu(
        0,
        3,
        flags,
        feeds,
        &[a0],
        1,
        100,
        0,
        0,
        100,
        0,
    );
    assert!(
        r_bad.is_err(),
        "a 3-leg config with 1 oracle account must reject"
    );
    let after_bad = env.svm.get_account(&env.market).unwrap().data;
    assert_eq!(
        after_bad, before,
        "rejected malformed config must not mutate/partially-configure the market"
    );

    // DISCRIMINATING CONTROL: the correctly-sized 3-account config configures.
    let r_ok = env.try_configure_hybrid_asset_with_conf_filter_cu(
        0,
        3,
        flags,
        feeds,
        &[a0, a1, a2],
        1,
        100,
        0,
        0,
        100,
        0,
    );
    assert!(
        r_ok.is_ok(),
        "the correctly-sized 3-leg/3-account config configures: {:?}",
        r_ok
    );
}

// and IGNORE the caller's value — accrual reflects only real elapsed slots.
#[test]
fn v16_program_crank_future_now_slot_does_not_overaccrue() {
    const INITIAL_PRICE: u64 = 1_000_000;
    const DEPOSIT: u128 = 10_000_000;
    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: INITIAL_PRICE,
        max_price_move_bps_per_slot: 1_000,
        max_accrual_dt_slots: 1,
        max_abs_funding_e9_per_slot: 1_000,
        min_funding_lifetime_slots: 1,
        ..V16CuMarketParams::default()
    });
    env.svm.warp_to_slot(0);
    env.configure_ewma_mark_with_cu(0, INITIAL_PRICE, 1, 0);
    let lo_owner = Keypair::new();
    let lo = env.create_portfolio(&lo_owner);
    let sh_owner = Keypair::new();
    let sh = env.create_portfolio(&sh_owner);
    env.deposit(&lo_owner, lo, DEPOSIT);
    env.deposit(&sh_owner, sh, DEPOSIT);
    env.trade_with_cu(
        &lo_owner,
        lo,
        &sh_owner,
        sh,
        POS_SCALE as i128,
        INITIAL_PRICE,
        0,
    );
    env.svm.warp_to_slot(1);
    env.push_ewma_mark_with_cu(1, INITIAL_PRICE * 2);

    // REAL clock is slot 2. Cranker lies with now_slot = 1_000_000 (a ~half-million-slot jump).
    env.svm.warp_to_slot(2);
    const LIE: u64 = 1_000_000;
    for acct in [lo, sh, lo] {
        env.svm.expire_blockhash();
        let _ = env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: LIE,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(acct, false),
            ],
            &[],
        );
    }
    let a = state::read_portfolio(&env.svm.get_account(&lo).unwrap().data).unwrap();
    let b = state::read_portfolio(&env.svm.get_account(&sh).unwrap().data).unwrap();
    let (_, g) = env.market_state();
    // the market advanced to the REAL clock slot (2), NOT the caller's lie.
    assert_eq!(
        g.slot_last, 2,
        "accrual used the authenticated clock slot, not the caller's now_slot"
    );
    assert!(
        g.assets[0].slot_last < LIE,
        "asset slot_last is the real clock, not the spoofed future"
    );
    // price moved at most the per-slot clamp over REAL elapsed slots (not 1M slots of movement).
    assert!(
        g.assets[0].effective_price <= INITIAL_PRICE * 2,
        "price bounded by real elapsed time + circuit breaker"
    );
    // Value conserved: after both accounts are current, junior PnL is residual-backed and the
    // still-converging mark blocks favorable conversion rather than making it withdrawable.
    assert_eq!(g.vault, 2 * DEPOSIT, "no tokens created/destroyed");
    assert!(
        g.vault >= g.c_tot + g.insurance,
        "senior conservation under slot-spoof attempt"
    );
    let residual = g.vault - g.c_tot - g.insurance;
    let paper_pnl = a.pnl.get().max(0) as u128 + b.pnl.get().max(0) as u128;
    assert!(
        paper_pnl > 0,
        "authenticated elapsed time produces bounded PnL"
    );
    assert!(
        paper_pnl <= residual,
        "canonical paper PnL is residual-backed"
    );
    let total_equity =
        (a.capital.get() as i128 + a.pnl.get()) + (b.capital.get() as i128 + b.pnl.get());
    assert!(
        total_equity + g.insurance as i128 <= g.vault as i128,
        "canonical refreshed equity is vault-bounded"
    );
    let market_before_conversion = env.svm.get_account(&env.market).unwrap();
    let portfolio_before_conversion = env.svm.get_account(&lo).unwrap();
    env.svm.expire_blockhash();
    let conversion = env.send(
        env.convert_released_pnl_ix(lo, u128::MAX),
        vec![
            AccountMeta::new(lo_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(lo, false),
        ],
        &[&lo_owner],
    );
    let conversion_error = conversion.expect_err("pending mark must block favorable conversion");
    assert!(
        conversion_error.contains("Custom(21)"),
        "pending-mark conversion returned the wrong error: {conversion_error}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_conversion,
        "rejected conversion rolls back market state"
    );
    assert_eq!(
        env.svm.get_account(&lo).unwrap(),
        portfolio_before_conversion,
        "rejected conversion rolls back portfolio state"
    );
}

// security.md sweep — maintenance-fee slot spoofing (#30/#19 DoS): SyncMaintenanceFee's now_slot is
// caller-supplied. A caller passes a far-future now_slot to over-charge maintenance fees and drain a
// victim. The handler must authenticate against the real Clock (charge only real elapsed slots).
#[test]
fn v16_attack_sync_maintenance_fee_future_slot_no_overcharge() {
    let mut env = V16CuEnv::new_with_market_params_price_move_and_maintenance_fee(
        1, 10_000, 10_000, 10_000, 58,
    );
    let owner = Keypair::new();
    let p = env.create_portfolio(&owner);
    env.deposit(&owner, p, 1_000_000);
    env.update_maintenance_fee_policy_with_cu(0);
    // real clock = slot 10; attacker lies with now_slot = 1_000_000.
    env.svm.warp_to_slot(10);
    let _ = env.try_sync_maintenance_fee_with_cu(p, None, 1_000_000);
    let a = env.portfolio_state(p);
    // fee reflects ~10 real slots (10*58 = 580), NOT 1_000_000 slots (which would drain everything).
    assert!(
        a.capital.get() >= 1_000_000 - 10_000,
        "fee bounded by real elapsed slots, not the lie (capital {})",
        a.capital.get()
    );
    assert!(
        a.capital.get() < 1_000_000,
        "some fee was charged for the real elapsed time (non-vacuous)"
    );
    assert_eq!(
        a.last_fee_slot.get(),
        10,
        "fee settled to the authenticated clock slot, not the spoofed future"
    );
    let (_, g) = env.market_state();
    assert_eq!(
        g.vault, 1_000_000,
        "fee is internal (capital->insurance): vault unchanged"
    );
    assert_eq!(
        g.vault,
        g.c_tot + g.insurance,
        "exact conservation under slot-spoof attempt"
    );
    // a follow-up sync at the same real slot is a no-op (no further drain).
    let cap_before = env.portfolio_state(p).capital.get();
    env.svm.expire_blockhash();
    let _ = env.try_sync_maintenance_fee_with_cu(p, None, 1_000_000);
    assert_eq!(
        env.portfolio_state(p).capital.get(),
        cap_before,
        "same-real-slot re-sync is a no-op despite future now_slot"
    );
}

// security.md sweep -- permissionless resolve slot spoof (#30 DoS): ResolveStalePermissionless is
// public. A cranker must not be able to pass a far-future caller now_slot and resolve a still-fresh
// market. The handler must authenticate against Clock; once the real Clock reaches the stale window,
// the same instruction may resolve even if caller now_slot is stale/low.
#[test]
fn v16_attack_permissionless_resolve_uses_authenticated_clock_slot() {
    let mut env = V16CuEnv::new();
    env.configure_permissionless_resolve_with_cu(5, 5);
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000_000);

    env.svm.warp_to_slot(4);
    let market_before = env.svm.get_account(&env.market).unwrap();
    env.svm.expire_blockhash();
    let spoof = env.send(
        ProgInstruction::ResolveStalePermissionless { now_slot: u64::MAX },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(
        spoof.is_err(),
        "far-future caller now_slot must not resolve before the real Clock reaches stale_slots"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected spoofed resolve leaves market bytes unchanged"
    );

    let (dest, _) = env.withdraw_with_cu(&owner, portfolio, 100_000);
    assert_eq!(
        env.token_amount(dest),
        100_000,
        "market remains Live and user funds remain withdrawable after rejected spoof"
    );

    env.svm.warp_to_slot(5);
    env.svm.expire_blockhash();
    let real = env.send(
        ProgInstruction::ResolveStalePermissionless { now_slot: 0 },
        vec![AccountMeta::new(env.market, false)],
        &[],
    );
    assert!(
        real.is_ok(),
        "once the real Clock reaches stale_slots, resolve succeeds even with a low caller now_slot: {real:?}"
    );
    let (_, group) = env.market_state();
    assert_eq!(group.mode, percolator::MarketModeV16::Resolved);
    assert_eq!(
        group.resolved_slot, 5,
        "resolved_slot is the authenticated Clock slot, not the caller's now_slot"
    );
}

// security.md sweep — oracle unit_scale transform: correct scaling + floor-to-0 rejected (#37/#39):
// apply_transform divides the price by unit_scale (`price /= unit_scale`, src/v16_program.rs:4064) and
// the post-check rejects a 0 result. Attacker goal: a unit_scale larger than the price floors the mark
// to 0 (slipping a garbage/zero mark) OR the scaling yields a wrong price. Protection: the mark is the
// exact divided price, and a scale that floors it to 0 -> OracleInvalid. Last degenerate-price path
// security.md sweep — Pyth feed negative/zero price rejected (#37/#39): a Pyth price is an i64 and can
// be negative or zero. Attacker goal: a malicious/garbage feed reporting a negative or zero price injects
// an invalid mark (settlement at a nonsensical price / sign flip). Protection: the oracle parse accepts
// security.md sweep — Chainlink oracle parser hardening (#37/#44): a Chainlink-owned transmissions
// account is accepted as a hybrid oracle leg, so malformed header/transmission fields must reject
// Chainlink spoofing gate: Chainlink feeds are account-key-bound like Switchboard. A valid-looking
// Chainlink staleness gate: a well-formed transmissions account with an old timestamp must not seed
// Hybrid oracle leg-shape gate: duplicate feed identities, stray feeds, and impossible divide flags
// must reject before installing an oracle profile. Otherwise a config could double-count one feed or
// security.md sweep - Hybrid oracle scalar bounds (#37/#39/#44): feed-shape tests cover duplicate
// and missing legs; this covers the scalar knobs that can otherwise install a malformed after-hours
// fallback profile. Rejected scalar configs must leave the market byte-identical.
#[test]
fn v16_attack_hybrid_oracle_scalar_bounds_reject_atomically() {
    let mut env = V16CuEnv::new();
    set_test_clock(&mut env, 1, 100);
    let admin = env.admin.insecure_clone();
    let feed = [0xba; 32];
    let oracle = env.set_pyth_price_with_conf(&feed, 200_000, -6, 0, 100);
    let feeds = [feed, [0u8; 32], [0u8; 32]];
    let accounts = vec![
        AccountMeta::new(admin.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new_readonly(oracle, false),
    ];

    let reject_unchanged = |env: &mut V16CuEnv,
                            oracle_leg_count: u8,
                            max_staleness_secs: u64,
                            hybrid_soft_stale_slots: u64,
                            mark_ewma_halflife_slots: u64,
                            invert: u8,
                            conf_filter_bps: u16,
                            label: &str| {
        let before = env.svm.get_account(&env.market).unwrap();
        env.svm.expire_blockhash();
        let rejected = send_tx(
            &mut env.svm,
            env.program_id,
            &env.payer,
            ProgInstruction::ConfigureHybridOracle {
                market_id: 0,
                observation_sequence: u64::MAX,
                asset_index: 0,
                now_slot: 1,
                now_unix_ts: 100,
                oracle_leg_count,
                oracle_leg_flags: 0,
                max_staleness_secs,
                hybrid_soft_stale_slots,
                mark_ewma_halflife_slots,
                mark_min_fee: 0,
                invert,
                unit_scale: 0,
                conf_filter_bps,
                oracle_leg_feeds: feeds,
            },
            accounts.clone(),
            &[&admin],
        );
        assert!(rejected.is_err(), "{label} must reject");
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            before,
            "{label} must leave the market byte-identical"
        );
    };

    reject_unchanged(&mut env, 0, 60, 3, 10, 0, 500, "zero leg count");
    reject_unchanged(&mut env, 4, 60, 3, 10, 0, 500, "leg count over cap");
    reject_unchanged(&mut env, 1, 0, 3, 10, 0, 500, "zero max staleness");
    reject_unchanged(&mut env, 1, 60, 0, 10, 0, 500, "zero soft-stale slots");
    reject_unchanged(&mut env, 1, 60, 3, 0, 0, 500, "zero fallback EWMA halflife");
    reject_unchanged(&mut env, 1, 60, 3, 10, 2, 500, "bad invert flag");
    reject_unchanged(
        &mut env,
        1,
        60,
        3,
        10,
        0,
        10_001,
        "confidence filter over 100%",
    );

    env.svm.expire_blockhash();
    let ok = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureHybridOracle {
            market_id: 0,
            observation_sequence: u64::MAX,
            asset_index: 0,
            now_slot: 1,
            now_unix_ts: 100,
            oracle_leg_count: 1,
            oracle_leg_flags: 0,
            max_staleness_secs: 60,
            hybrid_soft_stale_slots: 3,
            mark_ewma_halflife_slots: 10,
            mark_min_fee: 0,
            invert: 0,
            unit_scale: 0,
            conf_filter_bps: 500,
            oracle_leg_feeds: feeds,
        },
        accounts,
        &[&admin],
    );
    assert!(
        ok.is_ok(),
        "valid Hybrid oracle config remains live after scalar-bound probes: {ok:?}"
    );
    let (cfg, group) = env.market_state();
    assert_eq!(
        cfg.oracle_mode,
        percolator_prog::constants::ORACLE_MODE_HYBRID_AFTER_HOURS
    );
    assert_eq!(cfg.mark_ewma_halflife_slots, 10);
    assert_eq!(group.assets[0].effective_price, 200_000);
}

// security.md sweep — Pyth exponent bounds (#37/#39): exponent scaling is part of the trusted mark.
// Out-of-range exponents, over-max scaled prices, and floor-to-zero scaled prices must reject through
// security.md sweep — oracle mode isolation (#6/#37): once an asset is configured as a Hybrid/external
// oracle, even the correct oracle_authority must not bypass the external feed by calling pushed-mark
#[test]
fn v16_attack_crank_oracle_feed_id_mismatch_rejects_without_mutation() {
    let mut env = V16CuEnv::new();
    set_test_clock(&mut env, 1, 100);
    let expected_feed = [0x77u8; 32];
    let wrong_feed = [0x78u8; 32];
    let initial_acct = env.set_pyth_price_with_conf(&expected_feed, 200_000, -6, 0, 100);
    env.try_configure_hybrid_asset_with_conf_filter_cu(
        0,
        1,
        0,
        [expected_feed, [0u8; 32], [0u8; 32]],
        &[initial_acct],
        1,
        100,
        0,
        0,
        10,
        0,
    )
    .expect("configure matching one-leg hybrid oracle");

    let cranker_owner = Keypair::new();
    let cranker_portfolio = env.create_portfolio(&cranker_owner);
    set_test_clock(&mut env, 2, 101);
    let wrong_acct = env.set_pyth_price_with_conf(&wrong_feed, 500_000, -6, 0, 101);
    let market_before = env.svm.get_account(&env.market).unwrap().data;
    let portfolio_before = env.svm.get_account(&cranker_portfolio).unwrap().data;

    env.svm.expire_blockhash();
    let bad = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations_with_accounts(0, 1),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(cranker_portfolio, false),
            AccountMeta::new_readonly(wrong_acct, false),
        ],
        &[],
    );
    assert!(
        bad.is_err(),
        "permissionless crank must reject a PriceUpdate for the wrong configured feed"
    );
    let err = bad.err().unwrap();
    assert!(
        err.contains("Custom(29)"),
        "wrong feed id should reject as InvalidOracleKey (Custom 29), got: {err}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        market_before,
        "wrong-feed crank must not partially update the oracle profile or market state"
    );
    assert_eq!(
        env.svm.get_account(&cranker_portfolio).unwrap().data,
        portfolio_before,
        "wrong-feed crank must not mutate the cranker portfolio"
    );

    let correct_acct = env.set_pyth_price_with_conf(&expected_feed, 210_000, -6, 0, 101);
    env.svm.expire_blockhash();
    env.crank_with_oracle_tail(
        cranker_portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
        &[correct_acct],
    );
    let (cfg, group) = env.market_state();
    assert_eq!(cfg.last_good_oracle_slot, 2);
    assert_eq!(group.assets[0].raw_oracle_target_price, 210_000);
}

#[test]
fn v16_attack_crank_oracle_regressed_publish_time_rejects_even_when_fresh() {
    let mut env = V16CuEnv::new();
    set_test_clock(&mut env, 1, 100);
    let feed = [0x7au8; 32];
    let initial = env.set_pyth_price_with_conf(&feed, 200_000, -6, 0, 100);
    env.try_configure_hybrid_asset_with_conf_filter_cu(
        0,
        1,
        0,
        [feed, [0u8; 32], [0u8; 32]],
        &[initial],
        1,
        100,
        0,
        0,
        10,
        0,
    )
    .expect("configure matching one-leg hybrid oracle");

    let cranker_owner = Keypair::new();
    let cranker_portfolio = env.create_portfolio(&cranker_owner);
    set_test_clock(&mut env, 2, 100);
    let regressed_but_fresh = env.set_pyth_price_with_conf(&feed, 210_000, -6, 0, 99);
    let market_before = env.svm.get_account(&env.market).unwrap().data;
    let portfolio_before = env.svm.get_account(&cranker_portfolio).unwrap().data;

    env.svm.expire_blockhash();
    let replay = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations_with_accounts(0, 1),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(cranker_portfolio, false),
            AccountMeta::new_readonly(regressed_but_fresh, false),
        ],
        &[],
    );
    assert!(
        replay.is_err(),
        "a regressed oracle publish_time must reject even when still inside max staleness"
    );
    let err = replay.unwrap_err();
    assert!(
        err.contains("Custom(27)"),
        "regressed publish_time must reject as OracleStale (Custom 27), got: {err}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        market_before,
        "rejected regressed-publish replay must not mutate market state"
    );
    assert_eq!(
        env.svm.get_account(&cranker_portfolio).unwrap().data,
        portfolio_before,
        "rejected regressed-publish replay must not mutate the cranker portfolio"
    );

    set_test_clock(&mut env, 3, 101);
    let fresh = env.set_pyth_price_with_conf(&feed, 210_000, -6, 0, 101);
    env.svm.expire_blockhash();
    env.crank_with_oracle_tail(
        cranker_portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
        &[fresh],
    );
    let (cfg, group) = env.market_state();
    assert_eq!(cfg.last_good_oracle_slot, 3);
    assert_eq!(group.assets[0].raw_oracle_target_price, 210_000);
}

// [from pr125]
// LoF sweep — Pyth price updates must be FULLY verified (SOL-024 oracle integrity). read_pyth_price_e6
// rejects unless data[OFF_VERIFICATION_LEVEL] == PYTH_VERIFICATION_FULL_TAG (1). The Pyth Solana Receiver
// can write a PriceUpdateV2 at a PARTIAL verification level (fewer guardian signatures verified); trusting
// such a partially-verified price as the settlement mark would accept oracle data the receiver itself did
// not fully validate — a manipulation/mispricing vector (LoF). Drives the gate: a FULL price configures,
// while partial (0) and any other non-FULL level reject as OracleInvalid. No existing test varies the
// Pyth verification level (make_pyth_data hardcodes FULL).
#[test]
fn v16_attack_pyth_partial_verification_rejected() {
    let feed = [0x55u8; 32];
    let configure = |verification_level: u8| -> (bool, Option<String>) {
        let mut env = V16CuEnv::new();
        set_test_clock(&mut env, 1, 100);
        let mut data = make_pyth_data(&feed, 200_000, -6, 0, 100); // well-formed, fresh, in-bounds price
        data[40] = verification_level; // OFF_VERIFICATION_LEVEL
        let acct = Pubkey::new_unique();
        env.svm
            .set_account(
                acct,
                Account {
                    lamports: 1_000_000_000,
                    data,
                    owner: oracle_v16::PYTH_RECEIVER_PROGRAM_ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let r = env.try_configure_hybrid_asset_with_conf_filter_cu(
            0,
            1,
            0,
            [feed, [0u8; 32], [0u8; 32]],
            &[acct],
            1,
            100,
            0,
            0,
            100,
            0,
        );
        (r.is_ok(), r.err())
    };

    // FULL verification (1) -> configures (proves the price is otherwise valid, isolating the gate).
    let (full_ok, full_err) = configure(1);
    assert!(
        full_ok,
        "a FULLY-verified Pyth price configures: {full_err:?}"
    );

    // PARTIAL verification (0) -> reject: the price was not fully verified by the Pyth receiver.
    let (partial_ok, partial_err) = configure(0);
    assert!(!partial_ok, "a partially-verified Pyth price must reject");
    assert!(
        partial_err.unwrap().contains("Custom(26)"),
        "partial-verification Pyth price must be OracleInvalid (Custom 26)"
    );

    // Any other non-FULL tag (2) -> also reject (only the exact FULL tag is accepted).
    let (other_ok, other_err) = configure(2);
    assert!(!other_ok, "a non-FULL verification tag must reject");
    assert!(
        other_err.unwrap().contains("Custom(26)"),
        "non-FULL verification tag must be OracleInvalid (Custom 26)"
    );
}

// [from pr125]
// LoF sweep — Pyth account header validation: discriminator + minimum length (SOL-019 type confusion).
// read_pyth_price_e6 rejects an account whose first 8 bytes are not the PriceUpdateV2 anchor discriminator
// (OracleInvalid) and one shorter than PRICE_UPDATE_V2_MIN_LEN (InvalidAccountData), BEFORE deserializing
// the price. Combined with the owner check, this stops a Pyth-RECEIVER-owned account that is NOT a
// PriceUpdateV2 (a different Pyth account type, or a crafted blob) from being parsed as a price feed --
// type confusion that could inject an arbitrary mark (LoF). No existing test corrupts the Pyth
// discriminator or truncates the account (make_pyth_data always writes the correct 134-byte header).
#[test]
fn v16_attack_pyth_bad_discriminator_or_short_account_rejected() {
    let feed = [0x55u8; 32];
    let configure = |mutate: &dyn Fn(&mut Vec<u8>)| -> (bool, Option<String>) {
        let mut env = V16CuEnv::new();
        set_test_clock(&mut env, 1, 100);
        let mut data = make_pyth_data(&feed, 200_000, -6, 0, 100);
        mutate(&mut data);
        let acct = Pubkey::new_unique();
        env.svm
            .set_account(
                acct,
                Account {
                    lamports: 1_000_000_000,
                    data,
                    owner: oracle_v16::PYTH_RECEIVER_PROGRAM_ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        let r = env.try_configure_hybrid_asset_with_conf_filter_cu(
            0,
            1,
            0,
            [feed, [0u8; 32], [0u8; 32]],
            &[acct],
            1,
            100,
            0,
            0,
            100,
            0,
        );
        (r.is_ok(), r.err())
    };

    // Control: an untouched (valid header) Pyth account configures.
    let (ok, ok_err) = configure(&|_d| {});
    assert!(ok, "a well-formed Pyth account configures: {ok_err:?}");

    // Corrupted anchor discriminator -> rejected before the price is parsed (type confusion blocked).
    let (disc_ok, disc_err) = configure(&|d| d[0] ^= 0xff);
    assert!(
        !disc_ok,
        "a Pyth account with a wrong discriminator must reject"
    );
    assert!(
        disc_err.unwrap().contains("Custom(26)"),
        "bad Pyth discriminator must be OracleInvalid (Custom 26)"
    );

    // Truncated account (below PRICE_UPDATE_V2_MIN_LEN) -> rejected on the length gate, no out-of-bounds.
    let (short_ok, short_err) = configure(&|d| d.truncate(100));
    assert!(!short_ok, "a too-short Pyth account must reject");
    let short_msg = short_err.unwrap_or_default();
    assert!(
        short_msg.contains("InvalidAccountData") || short_msg.contains("Custom"),
        "a too-short Pyth account must reject cleanly (no panic / OOB read): {short_msg}"
    );
}

#[test]
fn v16_bpf_permissionless_append_activation_uses_authenticated_slot() {
    let mut env = V16CuEnv::new();
    let attacker = Keypair::new();
    env.update_market_init_fee_policy_with_cu(1);
    env.svm.warp_to_slot(100);

    let (_fee_source, _cu) = env.activate_permissionless_asset_with_fee(
        &attacker,
        1,
        u64::MAX,
        100,
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        1,
    );

    let (_, group) = env.market_state();
    assert_eq!(
        group.current_slot, 100,
        "permissionless append activation must authenticate now_slot against Clock"
    );
    assert_eq!(group.assets[1].slot_last, 100);

    let cranker = Keypair::new();
    let cranker_portfolio = env.create_portfolio(&cranker);
    env.crank(
        cranker_portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 100,
            observations: crank_observations(0),
        },
    );
}

#[test]
fn v16_bpf_permissionless_reuse_activation_uses_authenticated_slot() {
    let mut env = V16CuEnv::new();
    let attacker = Keypair::new();
    env.update_market_init_fee_policy_with_cu(1);

    env.svm.warp_to_slot(1);
    env.activate_permissionless_asset_with_fee(
        &attacker,
        1,
        1,
        100,
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        1,
    );

    env.svm.warp_to_slot(3);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_RETIRE,
        1,
        3,
        0,
    );
    let (_, retired_group) = env.market_state();
    assert_eq!(
        retired_group.assets[1].lifecycle,
        AssetLifecycleV16::Retired
    );

    env.svm.warp_to_slot(4);
    env.activate_permissionless_asset_with_fee(
        &attacker,
        1,
        u64::MAX,
        250,
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        attacker.pubkey(),
        1,
    );

    let (_, group) = env.market_state();
    assert_eq!(
        group.current_slot, 4,
        "permissionless reuse activation must authenticate now_slot against Clock"
    );
    assert_eq!(group.assets[1].slot_last, 4);

    let cranker = Keypair::new();
    let cranker_portfolio = env.create_portfolio(&cranker);
    env.crank(
        cranker_portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 4,
            observations: crank_observations(0),
        },
    );
}

#[test]
fn v16_bpf_privileged_retire_uses_authenticated_slot() {
    let mut env = V16CuEnv::new();
    env.activate_asset(1, 1, 100);
    env.svm.warp_to_slot(3);

    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_RETIRE,
        1,
        u64::MAX,
        0,
    );

    let (_, group) = env.market_state();
    assert_eq!(
        group.current_slot, 3,
        "privileged retire must authenticate now_slot against Clock"
    );
    assert_eq!(group.assets[1].retired_slot, 3);

    let cranker = Keypair::new();
    let cranker_portfolio = env.create_portfolio(&cranker);
    env.crank(
        cranker_portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 3,
            observations: crank_observations(0),
        },
    );
}

#[test]
fn v16_bpf_privileged_reactivate_uses_authenticated_slot() {
    let mut env = V16CuEnv::new();
    env.activate_asset(1, 1, 100);
    env.svm.warp_to_slot(3);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_RETIRE,
        1,
        3,
        0,
    );

    env.svm.warp_to_slot(4);
    env.update_asset_lifecycle_as_admin_with_cu(
        percolator_prog::processor::ASSET_ACTION_ACTIVATE,
        1,
        u64::MAX,
        250,
    );

    let (_, group) = env.market_state();
    assert_eq!(
        group.current_slot, 4,
        "privileged reactivation must authenticate now_slot against Clock"
    );
    assert_eq!(group.assets[1].slot_last, 4);

    let cranker = Keypair::new();
    let cranker_portfolio = env.create_portfolio(&cranker);
    env.crank(
        cranker_portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 4,
            observations: crank_observations(0),
        },
    );
}

#[test]
fn v16_bpf_permissionless_crank_uses_authenticated_clock_slot_not_caller_slot() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    let portfolio = env.create_portfolio(&owner);
    env.deposit(&owner, portfolio, 1_000_000);

    let real_slot = 10;
    let spoofed_slot = 1_000_000;
    env.svm.warp_to_slot(real_slot);
    env.crank(
        portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: spoofed_slot,
            observations: crank_observations(0),
        },
    );

    let clock = env.svm.get_sysvar::<Clock>();
    assert_eq!(clock.slot, real_slot);
    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, group) = state::read_market(&market_data).unwrap();
    assert_eq!(
        group.current_slot, clock.slot,
        "permissionless crank must authenticate engine time from SVM Clock, not the instruction body"
    );
    assert_ne!(
        group.current_slot, spoofed_slot,
        "caller-supplied crank now_slot must not be able to move engine time into the future"
    );
}

#[test]
fn v16_bpf_configure_hybrid_oracle_uses_authenticated_clock_slot_not_caller_slot() {
    let mut env = V16CuEnv::new();
    let real_slot = 10;
    let spoofed_slot = 1_000_000;
    env.svm.warp_to_slot(real_slot);
    let mut clock = env.svm.get_sysvar::<Clock>();
    clock.unix_timestamp = 1_000;
    env.svm.set_sysvar(&clock);
    let clock = env.svm.get_sysvar::<Clock>();

    let feeds = [[0x91u8; 32], [0x92u8; 32], [0x93u8; 32]];
    let leg0 = env.set_pyth_price(&feeds[0], 4_000_000_000, -6, clock.unix_timestamp);
    let leg1 = env.set_pyth_price(&feeds[1], 150_000_000, -6, clock.unix_timestamp);
    let leg2 = env.set_pyth_price(&feeds[2], 200_000_000, -6, clock.unix_timestamp);
    env.configure_three_leg_hybrid_with_cu(
        feeds,
        leg0,
        leg1,
        leg2,
        spoofed_slot,
        clock.unix_timestamp,
    );

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (cfg, group) = state::read_market(&market_data).unwrap();
    assert_eq!(
        group.current_slot, real_slot,
        "hybrid configuration must authenticate engine time from SVM Clock, not the instruction body"
    );
    assert_eq!(group.slot_last, real_slot);
    assert_eq!(cfg.last_good_oracle_slot, real_slot);
    assert_eq!(cfg.mark_ewma_last_slot, real_slot);
    assert_ne!(
        group.current_slot, spoofed_slot,
        "caller-supplied configure now_slot must not future-clock the market"
    );
}

#[test]
fn v16_bpf_configure_hybrid_oracle_uses_authenticated_unix_time_not_caller_time() {
    let mut env = V16CuEnv::new();
    env.svm.warp_to_slot(10);
    let mut clock = env.svm.get_sysvar::<Clock>();
    clock.unix_timestamp = 1_000;
    env.svm.set_sysvar(&clock);

    let feeds = [[0xa1u8; 32], [0xa2u8; 32], [0xa3u8; 32]];
    let stale_publish_time = 1;
    let leg0 = env.set_pyth_price(&feeds[0], 4_000_000_000, -6, stale_publish_time);
    let leg1 = env.set_pyth_price(&feeds[1], 150_000_000, -6, stale_publish_time);
    let leg2 = env.set_pyth_price(&feeds[2], 200_000_000, -6, stale_publish_time);
    let before = env.svm.get_account(&env.market).unwrap().data;

    let spoofed_fresh_unix = stale_publish_time;
    let result =
        env.try_configure_three_leg_hybrid(feeds, leg0, leg1, leg2, 10, spoofed_fresh_unix);

    assert!(
        result.is_err(),
        "hybrid configuration must not accept stale oracle accounts by trusting caller now_unix_ts"
    );
    let after = env.svm.get_account(&env.market).unwrap().data;
    assert_eq!(
        after, before,
        "rejected stale-oracle configuration must not mutate the market"
    );
}

#[test]
fn v16_bpf_hybrid_fresh_oracle_trade_opens_and_closes() {
    for dt in [0, 1] {
        for oracle_leg_count in [1, 3] {
            for invert in [0, 1] {
                run_hybrid_fresh_oracle_trade_case(dt, oracle_leg_count, invert);
            }
        }
    }
}

#[test]
fn v16_bpf_hybrid_fresh_oracle_trade_production_risk_params_opens_and_closes() {
    for asset_index in [0, 1] {
        for direction_sign in [1, -1] {
            run_hybrid_fresh_oracle_production_risk_trade_case(
                asset_index,
                ProductionRiskTradeCase::baseline(),
                direction_sign,
            );
        }
    }
}

#[test]
fn v16_bpf_hybrid_fresh_oracle_trade_devnet_difference_axes() {
    for case in [
        ProductionRiskTradeCase::fixed_deposit(),
        ProductionRiskTradeCase::same_owner(),
        ProductionRiskTradeCase::sub_one_mark(),
        ProductionRiskTradeCase::real_conf_filter(),
    ] {
        for asset_index in [0, 1] {
            for direction_sign in [1, -1] {
                run_hybrid_fresh_oracle_production_risk_trade_case(
                    asset_index,
                    case,
                    direction_sign,
                );
            }
        }
    }
}

#[test]
fn v16_bpf_hybrid_mark_uses_ewma_after_hours_then_oracle_when_fresh() {
    let mut env = V16CuEnv::new();
    env.svm.warp_to_slot(1);
    let mut clock = env.svm.get_sysvar::<Clock>();
    clock.unix_timestamp = 100;
    env.svm.set_sysvar(&clock);

    let feeds = [[0xb1u8; 32], [0xb2u8; 32], [0xb3u8; 32]];
    let leg0 = env.set_pyth_price(&feeds[0], 4_000_000_000, -6, 100);
    let leg1 = env.set_pyth_price(&feeds[1], 150_000_000, -6, 100);
    let leg2 = env.set_pyth_price(&feeds[2], 200_000_000, -6, 100);
    let configure_cu = env.configure_three_leg_hybrid_with_cu(feeds, leg0, leg1, leg2, 1, 100);
    assert_cu_within("ConfigureHybridOracle", configure_cu, CUSTODY_CU_LIMIT);

    let keeper = Keypair::new();
    let keeper_portfolio = env.create_portfolio(&keeper);
    env.svm.warp_to_slot(2);
    let mut clock = env.svm.get_sysvar::<Clock>();
    clock.unix_timestamp = 101;
    env.svm.set_sysvar(&clock);
    let fresh_leg0 = env.set_pyth_price(&feeds[0], 4_200_000_000, -6, 101);
    let fresh_leg1 = env.set_pyth_price(&feeds[1], 150_000_000, -6, 101);
    let fresh_leg2 = env.set_pyth_price(&feeds[2], 200_000_000, -6, 101);
    let fresh_crank_cu = env.crank_with_oracle_tail(
        keeper_portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
        &[fresh_leg0, fresh_leg1, fresh_leg2],
    );
    assert_cu_within("HybridMark fresh crank", fresh_crank_cu, CRANK_CU_LIMIT);
    let (fresh_cfg, fresh_group) = env.market_state();
    assert_eq!(fresh_group.assets[0].effective_price, 140_000);
    assert_eq!(fresh_cfg.mark_ewma_e6, 140_000);
    assert_eq!(fresh_cfg.last_good_oracle_slot, 2);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long_account = env.create_portfolio(&long_owner);
    let short_account = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long_account, 10_000_000);
    env.deposit(&short_owner, short_account, 10_000_000);

    env.svm.warp_to_slot(10);
    let before_after_hours = env.market_state();
    let size_q = POS_SCALE;
    let after_hours_exec_price = before_after_hours.1.assets[0].effective_price * 150 / 100;
    let open_cu = env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        size_q as i128,
        after_hours_exec_price,
        0,
    );
    assert_cu_within("HybridMark after-hours open", open_cu, TRADE_CU_LIMIT);
    let (after_hours_cfg, after_hours_group) = env.market_state();
    assert!(
        after_hours_cfg.mark_ewma_e6 > before_after_hours.0.mark_ewma_e6,
        "after-hours hybrid trade must advance the fallback EWMA mark"
    );
    assert_eq!(
        after_hours_group.assets[0].effective_price, before_after_hours.1.assets[0].effective_price,
        "after-hours execution must not rewrite the last accepted oracle index"
    );
    assert!(
        after_hours_group.insurance > 0,
        "after-hours hybrid trade must charge a dynamic mark-movement fee"
    );

    let close_cu = env.trade_with_cu(
        &long_owner,
        long_account,
        &short_owner,
        short_account,
        -(size_q as i128),
        after_hours_exec_price,
        0,
    );
    assert_cu_within("HybridMark after-hours close", close_cu, TRADE_CU_LIMIT);
    let (_, flat_group) = env.market_state();
    assert_eq!(flat_group.assets[0].oi_eff_long_q, 0);
    assert_eq!(flat_group.assets[0].oi_eff_short_q, 0);

    env.svm.warp_to_slot(11);
    let mut clock = env.svm.get_sysvar::<Clock>();
    clock.unix_timestamp = 102;
    env.svm.set_sysvar(&clock);
    let normal_leg0 = env.set_pyth_price(&feeds[0], 4_500_000_000, -6, 102);
    let normal_leg1 = env.set_pyth_price(&feeds[1], 150_000_000, -6, 102);
    let normal_leg2 = env.set_pyth_price(&feeds[2], 200_000_000, -6, 102);
    let normal_crank_cu = env.crank_with_oracle_tail(
        keeper_portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 11,
            observations: crank_observations(0),
        },
        &[normal_leg0, normal_leg1, normal_leg2],
    );
    assert_cu_within(
        "HybridMark normal-hours crank",
        normal_crank_cu,
        CRANK_CU_LIMIT,
    );
    let (normal_cfg, normal_group) = env.market_state();
    assert_eq!(normal_cfg.last_good_oracle_slot, 11);
    assert_eq!(normal_cfg.mark_ewma_last_slot, 11);
    assert_eq!(normal_cfg.mark_ewma_e6, 150_000);
    assert_eq!(normal_group.assets[0].effective_price, 150_000);
    assert_eq!(normal_group.assets[0].raw_oracle_target_price, 150_000);
}

#[test]
fn v16_bpf_configure_and_push_ewma_mark_are_bounded_and_clock_authenticated() {
    let mut env = V16CuEnv::new();
    let configure_real_slot = 8;
    let push_real_slot = 9;
    let spoofed_slot = 1_000_000;
    env.svm.warp_to_slot(configure_real_slot);
    let configure_cu = env.configure_ewma_mark_with_cu(spoofed_slot, 100, 1, 0);
    env.svm.warp_to_slot(push_real_slot);
    let push_cu = env.push_ewma_mark_with_cu(spoofed_slot, 120);
    println!("v16 EwmaMark configure CU: {configure_cu}, push CU: {push_cu}");
    assert!(
        configure_cu <= CUSTODY_CU_LIMIT,
        "EwmaMark configure CU {} exceeded limit {}",
        configure_cu,
        CUSTODY_CU_LIMIT
    );
    assert!(
        push_cu <= CUSTODY_CU_LIMIT,
        "EwmaMark push CU {} exceeded limit {}",
        push_cu,
        CUSTODY_CU_LIMIT
    );

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (cfg, group) = state::read_market(&market_data).unwrap();
    assert_eq!(
        cfg.oracle_mode,
        percolator_prog::constants::ORACLE_MODE_EWMA_MARK
    );
    assert_eq!(group.current_slot, configure_real_slot);
    assert_eq!(group.slot_last, configure_real_slot);
    assert_eq!(cfg.mark_ewma_last_slot, push_real_slot);
    assert_eq!(
        cfg.mark_ewma_e6, 110,
        "authority mark push should update the EWMA using authenticated slot time"
    );
    assert_ne!(
        cfg.mark_ewma_last_slot, spoofed_slot,
        "caller-supplied PushEwmaMark now_slot must not authenticate mark liveness"
    );
}

#[test]
fn v16_bpf_configure_and_push_auth_mark_are_bounded_and_clock_authenticated() {
    let mut env = V16CuEnv::new();
    let configure_real_slot = 8;
    let push_real_slot = 9;
    let spoofed_slot = 1_000_000;
    env.svm.warp_to_slot(configure_real_slot);
    let configure_cu = env.configure_auth_mark_with_cu(spoofed_slot, 100);
    env.svm.warp_to_slot(push_real_slot);
    let push_cu = env.push_auth_mark_with_cu(spoofed_slot, 120);
    println!("v16 AuthMark configure CU: {configure_cu}, push CU: {push_cu}");
    assert!(
        configure_cu <= CUSTODY_CU_LIMIT,
        "AuthMark configure CU {} exceeded limit {}",
        configure_cu,
        CUSTODY_CU_LIMIT
    );
    assert!(
        push_cu <= CUSTODY_CU_LIMIT,
        "AuthMark push CU {} exceeded limit {}",
        push_cu,
        CUSTODY_CU_LIMIT
    );

    let market_data = env.svm.get_account(&env.market).unwrap().data;
    let (cfg, group) = state::read_market(&market_data).unwrap();
    assert_eq!(
        cfg.oracle_mode,
        percolator_prog::constants::ORACLE_MODE_AUTH_MARK
    );
    assert_eq!(group.current_slot, configure_real_slot);
    assert_eq!(group.slot_last, configure_real_slot);
    assert_eq!(cfg.mark_ewma_last_slot, push_real_slot);
    assert_eq!(
        cfg.mark_ewma_e6, 120,
        "authority mark push should store the AuthMark value directly"
    );
    assert_eq!(cfg.oracle_target_price_e6, 120);
    assert_eq!(cfg.mark_ewma_halflife_slots, 0);
    assert_ne!(
        cfg.mark_ewma_last_slot, spoofed_slot,
        "caller-supplied PushAuthMark now_slot must not authenticate mark liveness"
    );
}

// Switchboard oracle source coverage: the entire read_switchboard_price_e6 path (owner check, key
// binding, discriminator, sample-size, staleness, std-dev conf filter, /1e12 scale) had ZERO
// integration tests (only the Pyth path was covered). This crafts a valid Switchboard On-Demand feed,
// configures a single Switchboard leg, and asserts the read works end-to-end: the feed value/1e12 seeds
// the asset-0 oracle target. A wrong offset/scale/owner would make ConfigureHybridOracle reject.
#[test]
fn v16_bpf_switchboard_oracle_feed_read_and_applied() {
    let mut env = V16CuEnv::new();
    env.svm.warp_to_slot(10);
    let mut clock = env.svm.get_sysvar::<Clock>();
    clock.unix_timestamp = 1_000;
    env.svm.set_sysvar(&clock);
    let now_unix = 1_000i64;

    // out = value / SWITCHBOARD_RESULT_SCALE (1e12). Pick out == initial_price (100) to avoid any jump.
    let out: i128 = 100;
    let value: i128 = out * 1_000_000_000_000;
    let sb = env.set_switchboard_price(value, 1, now_unix);

    env.try_configure_hybrid_with_cu(
        1,
        0,
        [sb.to_bytes(), [0u8; 32], [0u8; 32]],
        &[sb],
        10,
        now_unix,
        0,
        0,
        3,
    )
    .expect("configure 1-leg switchboard oracle (read must succeed)");

    let (cfg, _g) = env.market_state();
    assert_eq!(
        cfg.oracle_target_price_e6, out as u64,
        "switchboard feed value/1e12 must seed the oracle target end-to-end"
    );
}

// Switchboard staleness gate: a feed whose last_update_timestamp is far in the past must be rejected
// Switchboard confidence gate: std_dev is parsed from a different vendor layout than Pyth `conf`.
// Switchboard sample quorum gate: a PullFeed update with too few samples is a manipulable oracle input.
// The public configure path must reject low-quorum feeds atomically, while accepting the same value once
// Switchboard spoofing gate: unlike Pyth, the configured feed is the PullFeed account key. A valid-
// A bilateral trade's signed fee must consent to the current mutable base-fee policy. Rejecting a
// lower value prevents fee evasion without letting a post-sign policy update increase either
// Oracle invert correctness: apply_transform computes price = 1e12 / raw when invert=1 (inverse pairs,
// e.g. a BASE/QUOTE feed flipped to QUOTE/BASE). The zero-edge is covered (32090) but the happy-path
// reciprocal VALUE was untested -- a wrong constant/formula would mis-price every inverse market.
#[test]
fn v16_bpf_oracle_invert_produces_correct_reciprocal() {
    let mut env = V16CuEnv::new();
    env.svm.warp_to_slot(10);
    let mut clock = env.svm.get_sysvar::<Clock>();
    clock.unix_timestamp = 1_000;
    env.svm.set_sysvar(&clock);
    let now_unix = 1_000i64;

    // raw leg price = 2_000_000 (e6, "$2"); invert -> 1e12 / 2_000_000 = 500_000 ("$0.50" = 1/2).
    let feed = [0x77u8; 32];
    let leg = env.set_pyth_price(&feed, 2_000_000, -6, now_unix);
    env.try_configure_hybrid_with_cu(
        1,
        0,
        [feed, [0u8; 32], [0u8; 32]],
        &[leg],
        10,
        now_unix,
        1, // invert
        0,
        3,
    )
    .expect("configure inverted 1-leg oracle");

    let (cfg, _g) = env.market_state();
    assert_eq!(
        cfg.oracle_target_price_e6, 500_000,
        "invert must produce 1e12/raw = 1e12/2_000_000 = 500_000, got {}",
        cfg.oracle_target_price_e6
    );
}

// Oracle composite cross-rate VALUE: DIVIDE-leg flags compute leg0/(leg1*leg2) via sequential
// compose (acc*1e6/leg). Existing 3-leg tests only assert mark > 0 (run_hybrid_fresh_oracle_trade_case)
// or clock/slot -- not the composed VALUE. A wrong compose formula/scaling would mis-price every
// cross-rate market with a wrong-but-nonzero mark. Clean exact case: 6 / (2 * 3) = 1.00.
#[test]
fn v16_bpf_oracle_composite_divide_legs_produce_correct_cross_rate() {
    let mut env = V16CuEnv::new();
    set_test_clock(&mut env, 10, 1_000);
    let feeds = [[0xd1u8; 32], [0xd2u8; 32], [0xd3u8; 32]];
    let leg0 = env.set_pyth_price(&feeds[0], 6_000_000, -6, 1_000); // $6
    let leg1 = env.set_pyth_price(&feeds[1], 2_000_000, -6, 1_000); // $2 (divisor)
    let leg2 = env.set_pyth_price(&feeds[2], 3_000_000, -6, 1_000); // $3 (divisor)
    env.try_configure_hybrid_with_cu(
        3,
        ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3,
        feeds,
        &[leg0, leg1, leg2],
        10,
        1_000,
        0,
        0,
        3,
    )
    .expect("configure 3-leg divide composite");
    let (cfg, _g) = env.market_state();
    // leg0 / (leg1 * leg2) in e6 = 6 / (2*3) = 1.00 = 1_000_000.
    assert_eq!(
        cfg.oracle_target_price_e6, 1_000_000,
        "composite cross-rate must be leg0/(leg1*leg2) = 6/(2*3) = 1.00, got {}",
        cfg.oracle_target_price_e6
    );
}
