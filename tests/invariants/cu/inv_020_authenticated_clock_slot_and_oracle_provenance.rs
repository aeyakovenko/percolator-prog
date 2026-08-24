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
//! The all-provider matrices additionally cross legal composite transforms, coherent rewind,
//! exact freshness boundaries, a real adverse-price liquidation with a subsequent owner trade
//! exit, composite shutdown/forced-exit/restart with old-provenance rejection and fresh trading,
//! every single provider through DrainOnly, Recovery, and Resolved value-bearing routes, and six
//! multi-provider formulas through those lifecycles with denominator, expiry, and malformed-tail
//! controls.
//! An independent typed parser model covers 726 boundary words, 15,552 structural/semantic
//! combinations, and 12,288 seeded valid layouts. An independent overflow-free confidence oracle
//! compares all 65,536 basis-point settings across wide carry and overflow operands.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter. The bounded reference does
//! not mathematically exhaust every provider byte string, every provider-order/transform/lifecycle
//! product, or the solver-bound relational wide-product theorem.

use super::*;
use rand::{Rng, SeedableRng};
use rand_xorshift::XorShiftRng;

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

// Issue #405: PullFeed.last_update_timestamp dates the account write, not CurrentResult.value.
// A successful Switchboard response can advance that account-wide field while retaining an older
// selected submission. This public ConfigureHybridOracle route must age the timestamp selected by
// CurrentResult.submission_idx and reject before mutating the market.
#[test]
fn v16_program_switchboard_fresh_account_timestamp_cannot_revive_stale_selected_result() {
    let mut env = V16CuEnv::new();
    set_test_clock(&mut env, 222, 189);
    let value: i128 = 100 * 1_000_000_000_000;
    let feed = Pubkey::new_unique();
    let mut data = make_switchboard_data(&[0xAB; 32], value, 1, 100, 3, 1, 1);
    data[2216..2224].copy_from_slice(&189i64.to_le_bytes());
    data[2361] = 7;
    data[2952..2960].copy_from_slice(&189i64.to_le_bytes());
    data[3008..3016].copy_from_slice(&100i64.to_le_bytes());
    assert_eq!(
        i64::from_le_bytes(data[3008..3016].try_into().unwrap()),
        100,
        "the selected submission remains 89 seconds old"
    );
    env.svm
        .set_account(
            feed,
            Account {
                lamports: 1_000_000_000,
                data,
                owner: oracle_v16::SWITCHBOARD_ON_DEMAND_MAINNET_PROGRAM_ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    let before = env.svm.get_account(&env.market).unwrap().data;

    let rejected = env.try_configure_hybrid_asset_with_conf_filter_cu(
        0,
        1,
        0,
        [feed.to_bytes(), [0; 32], [0; 32]],
        &[feed],
        222,
        189,
        0,
        0,
        3,
        100,
    );
    let err = rejected.expect_err(
        "a fresh account write timestamp must not revive a stale selected Switchboard result",
    );
    assert!(
        err.contains("Custom(27)"),
        "stale selected result must reject as OracleStale, got: {err}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before,
        "selected-result freshness rejection must roll back the complete market"
    );

    let malformed_feed = Pubkey::new_unique();
    let mut malformed = make_switchboard_data(&[0xAB; 32], value, 1, 189, 3, 1, 222);
    malformed[2361] = 32;
    env.svm
        .set_account(
            malformed_feed,
            Account {
                lamports: 1_000_000_000,
                data: malformed,
                owner: oracle_v16::SWITCHBOARD_ON_DEMAND_MAINNET_PROGRAM_ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    let malformed_rejection = env
        .try_configure_hybrid_asset_with_conf_filter_cu(
            0,
            1,
            0,
            [malformed_feed.to_bytes(), [0; 32], [0; 32]],
            &[malformed_feed],
            222,
            189,
            0,
            0,
            3,
            100,
        )
        .expect_err("an out-of-range selected submission index must reject");
    assert!(
        malformed_rejection.contains("Custom(26)"),
        "invalid selected index must reject as OracleInvalid, got: {malformed_rejection}"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap().data, before);

    let fresh_feed = Pubkey::new_unique();
    let mut fresh_data = make_switchboard_data(&[0xAB; 32], value, 1, 189, 3, 1, 222);
    fresh_data[2361] = 31;
    fresh_data[3200..3208].copy_from_slice(&189i64.to_le_bytes());
    env.svm
        .set_account(
            fresh_feed,
            Account {
                lamports: 1_000_000_000,
                data: fresh_data,
                owner: oracle_v16::SWITCHBOARD_ON_DEMAND_MAINNET_PROGRAM_ID,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
    env.svm.expire_blockhash();
    env.try_configure_hybrid_asset_with_conf_filter_cu(
        0,
        1,
        0,
        [fresh_feed.to_bytes(), [0; 32], [0; 32]],
        &[fresh_feed],
        222,
        189,
        0,
        0,
        3,
        100,
    )
    .expect("a genuinely current selected Switchboard result remains usable");
}

#[test]
fn v16_program_switchboard_selected_timestamp_staleness_boundary_is_inclusive() {
    let configure = |selected_publish_time: i64| {
        let mut env = V16CuEnv::new();
        set_test_clock(&mut env, 222, 189);
        let feed = Pubkey::new_unique();
        let mut data = make_switchboard_data(
            &[0xAB; 32],
            100 * 1_000_000_000_000,
            1,
            selected_publish_time,
            3,
            1,
            222,
        );
        data[2216..2224].copy_from_slice(&189i64.to_le_bytes());
        env.svm
            .set_account(
                feed,
                Account {
                    lamports: 1_000_000_000,
                    data,
                    owner: oracle_v16::SWITCHBOARD_ON_DEMAND_MAINNET_PROGRAM_ID,
                    executable: false,
                    rent_epoch: 0,
                },
            )
            .unwrap();
        env.try_configure_hybrid_asset_with_conf_filter_cu(
            0,
            1,
            0,
            [feed.to_bytes(), [0; 32], [0; 32]],
            &[feed],
            222,
            189,
            0,
            0,
            3,
            100,
        )
    };

    configure(129).expect("age exactly equal to max_staleness_secs must remain valid");
    let stale = configure(128).expect_err("max_staleness_secs + 1 must reject");
    assert!(
        stale.contains("Custom(27)"),
        "one second outside the selected-result freshness bound must be OracleStale: {stale}"
    );
}

#[test]
fn v16_program_switchboard_stale_selected_result_cannot_refresh_crank_liveness() {
    let mut env = V16CuEnv::new();
    set_test_clock(&mut env, 1, 100);
    let feed = env.set_switchboard_price(100 * 1_000_000_000_000, 1, 100);
    env.try_configure_hybrid_asset_with_conf_filter_cu(
        0,
        1,
        0,
        [feed.to_bytes(), [0; 32], [0; 32]],
        &[feed],
        1,
        100,
        0,
        0,
        3,
        100,
    )
    .expect("configure a genuinely current selected result");
    assert_eq!(env.market_state().0.last_good_oracle_slot, 1);

    // Model the production-observed Switchboard transition: only the account write timestamp
    // advances. The selected value, result slot, submission index, and selected timestamp remain
    // unchanged. A permissionless crank may use Hybrid fallback, but must not certify this as a new
    // good oracle observation or postpone stale-oracle recovery.
    let mut account = env.svm.get_account(&feed).unwrap();
    account.data[2216..2224].copy_from_slice(&189i64.to_le_bytes());
    env.svm.set_account(feed, account).unwrap();
    let keeper = Keypair::new();
    let portfolio = env.create_portfolio(&keeper);
    set_test_clock(&mut env, 222, 189);
    let cu = env.crank_with_oracle_tail(
        portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 222,
            observations: crank_observations(0),
        },
        &[feed],
    );
    assert_cu_within(
        "Switchboard stale selected-result crank",
        cu,
        CRANK_CU_LIMIT,
    );
    let (cfg, _) = env.market_state();
    assert_eq!(
        cfg.last_good_oracle_slot, 1,
        "account timestamp churn must not refresh selected-result liveness"
    );
    assert_eq!(cfg.oracle_target_publish_time, 100);
    assert_eq!(cfg.oracle_leg_publish_times[0], 100);
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

    let extreme = env.set_switchboard_price(i128::MAX, i128::MAX, 1_000);
    let before_extreme = env.svm.get_account(&env.market).unwrap().data;
    let extreme_rejected = env.try_configure_hybrid_asset_with_conf_filter_cu(
        0,
        1,
        0,
        [extreme.to_bytes(), [0u8; 32], [0u8; 32]],
        &[extreme],
        10,
        1_000,
        0,
        0,
        3,
        100,
    );
    let err = extreme_rejected.expect_err("full-width confidence products must reject cleanly");
    assert!(
        err.contains("Custom(28)"),
        "full-width Switchboard confidence must reject as OracleConfTooWide, got: {err}"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap().data,
        before_extreme,
        "full-width confidence rejection must roll back the market"
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
        let _ = env.send_crank_if_actionable(
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
    env.sync_maintenance_fee_with_cu(p, None, 1_000_000);
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
    env.sync_maintenance_fee_with_cu(p, None, 1_000_000);
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EpochMatrixProvider {
    Pyth,
    Switchboard,
    Chainlink,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct EpochMatrixCase {
    providers: [EpochMatrixProvider; 3],
    count: u8,
    flags: u8,
    invert: u8,
    unit_scale: u32,
}

#[derive(Clone, Copy, Debug)]
struct EpochMatrixLeg {
    provider: EpochMatrixProvider,
    account: Pubkey,
    feed: [u8; 32],
    price_e6: u64,
}

fn parse_oracle_through_account_info(
    owner: Pubkey,
    key: Pubkey,
    data: &[u8],
    expected_feed: &[u8; 32],
    now_unix_ts: i64,
    max_staleness_secs: u64,
    conf_bps: u16,
) -> Result<(u64, i64), solana_program::program_error::ProgramError> {
    let mut lamports = 1u64;
    let mut owned_data = data.to_vec();
    let account = solana_program::account_info::AccountInfo::new(
        &key,
        false,
        false,
        &mut lamports,
        &mut owned_data,
        &owner,
        false,
        0,
    );
    oracle_v16::read_oracle_price_e6(
        &account,
        expected_feed,
        now_unix_ts,
        max_staleness_secs,
        conf_bps,
    )
}

#[test]
fn host_oracle_accountinfo_delegation_matches_pure_parser_on_single_byte_corpus() {
    const PRICE_E6: u64 = 1_500_000;
    const PUBLISH_TIME: i64 = 100;
    const NOW: i64 = 120;
    const MAX_STALENESS: u64 = 60;
    const CONF_BPS: u16 = 100;

    let pyth_key = Pubkey::new_unique();
    let pyth_feed = [0x41u8; 32];
    let switchboard_key = Pubkey::new_unique();
    let chainlink_key = Pubkey::new_unique();
    let fixtures = [
        (
            oracle_v16::PYTH_RECEIVER_PROGRAM_ID,
            pyth_key,
            pyth_feed,
            make_pyth_data(&pyth_feed, PRICE_E6 as i64, -6, 1, PUBLISH_TIME),
        ),
        (
            oracle_v16::SWITCHBOARD_ON_DEMAND_MAINNET_PROGRAM_ID,
            switchboard_key,
            switchboard_key.to_bytes(),
            make_switchboard_data(
                &[0x42u8; 32],
                i128::from(PRICE_E6) * 1_000_000_000_000,
                1,
                PUBLISH_TIME,
                3,
                1,
                1,
            ),
        ),
        (
            oracle_v16::CHAINLINK_STORE_PROGRAM_ID,
            chainlink_key,
            chainlink_key.to_bytes(),
            make_chainlink_data(1, 6, 1, 1, 1, PUBLISH_TIME as u32, i128::from(PRICE_E6)),
        ),
    ];

    let mut compared_words = 0usize;
    for (fixture_index, (owner, key, expected_feed, data)) in fixtures.iter().enumerate() {
        let pure = oracle_v16::read_oracle_price_e6_from_bytes(
            owner,
            key,
            data,
            expected_feed,
            NOW,
            MAX_STALENESS,
            CONF_BPS,
        );
        let account = parse_oracle_through_account_info(
            *owner,
            *key,
            data,
            expected_feed,
            NOW,
            MAX_STALENESS,
            CONF_BPS,
        );
        assert_eq!(pure, Ok((PRICE_E6, PUBLISH_TIME)));
        assert_eq!(account, pure, "valid fixture {fixture_index}");
        compared_words += 1;

        for prefix_len in 0..data.len() {
            let prefix = &data[..prefix_len];
            let pure = oracle_v16::read_oracle_price_e6_from_bytes(
                owner,
                key,
                prefix,
                expected_feed,
                NOW,
                MAX_STALENESS,
                CONF_BPS,
            );
            let account = parse_oracle_through_account_info(
                *owner,
                *key,
                prefix,
                expected_feed,
                NOW,
                MAX_STALENESS,
                CONF_BPS,
            );
            assert_eq!(
                account, pure,
                "prefix fixture {fixture_index} length {prefix_len}"
            );
            compared_words += 1;
        }

        for byte_index in 0..data.len() {
            let mut mutated = data.clone();
            mutated[byte_index] ^= 1u8 << (byte_index % 8);
            let pure = oracle_v16::read_oracle_price_e6_from_bytes(
                owner,
                key,
                &mutated,
                expected_feed,
                NOW,
                MAX_STALENESS,
                CONF_BPS,
            );
            let account = parse_oracle_through_account_info(
                *owner,
                *key,
                &mutated,
                expected_feed,
                NOW,
                MAX_STALENESS,
                CONF_BPS,
            );
            assert_eq!(
                account, pure,
                "single-byte fixture {fixture_index} offset {byte_index}"
            );
            compared_words += 1;
        }
    }
    assert_eq!(compared_words, 2 * (134 + 3_208 + 248) + fixtures.len());
    println!("oracle AccountInfo/pure parser equivalence: {compared_words} words");
}

type OracleParseResult = Result<(u64, i64), solana_program::program_error::ProgramError>;
const REFERENCE_SWITCHBOARD_SCALE: u128 = 1_000_000_000_000;

fn reference_publish_time_is_fresh(
    publish_time: i64,
    now_unix_ts: i64,
    max_staleness_secs: u64,
) -> bool {
    let age = i128::from(now_unix_ts) - i128::from(publish_time);
    age >= 0 && age as u128 <= u128::from(max_staleness_secs)
}

fn reference_confidence_is_too_wide(uncertainty: u128, value: u128, conf_bps: u16) -> bool {
    if conf_bps == 0 {
        return false;
    }
    // uncertainty * 10_000 > value * conf_bps, evaluated without a wide product.
    let conf_bps = u128::from(conf_bps);
    let quotient = value / 10_000;
    let remainder = value % 10_000;
    let Some(base) = quotient.checked_mul(conf_bps) else {
        return false;
    };
    let tail = remainder * conf_bps / 10_000;
    let Some(floor_threshold) = base.checked_add(tail) else {
        return false;
    };
    uncertainty > floor_threshold
}

#[test]
fn host_oracle_confidence_comparison_exhausts_bps_for_wide_boundary_pairs() {
    let middle = 1u128 << 64;
    let scale = REFERENCE_SWITCHBOARD_SCALE;
    let cases = [
        (0, 0),
        (0, 1),
        (1, 0),
        (1, 1),
        (9_999, 10_000),
        (10_000, 9_999),
        (middle - 1, middle - 1),
        (middle - 1, middle),
        (middle, middle - 1),
        (middle, middle),
        (middle + 1, middle),
        (middle, middle + 1),
        (scale - 1, scale),
        (scale, scale - 1),
        (u128::from(percolator::MAX_ORACLE_PRICE) * scale, scale),
        (scale, u128::from(percolator::MAX_ORACLE_PRICE) * scale),
        (u128::MAX - 1, u128::MAX),
        (u128::MAX, u128::MAX - 1),
        (u128::MAX, 1),
        (1, u128::MAX),
    ];
    let mut compared = 0usize;
    for conf_bps in 0..=u16::MAX {
        for (uncertainty, value) in cases {
            assert_eq!(
                oracle_v16::oracle_confidence_is_too_wide(uncertainty, value, conf_bps),
                reference_confidence_is_too_wide(uncertainty, value, conf_bps),
                "confidence mismatch for uncertainty={uncertainty}, value={value}, bps={conf_bps}"
            );
            compared += 1;
        }
    }
    assert_eq!(compared, 20 * (usize::from(u16::MAX) + 1));
}

fn reference_scale_decimal_to_e6(mantissa: i128, scale: u32) -> OracleParseResult {
    if mantissa <= 0 || scale > 18 {
        return Err(PercolatorError::OracleInvalid.into());
    }
    let mantissa = mantissa as u128;
    let out = if scale >= 6 {
        mantissa / 10u128.pow(scale - 6)
    } else {
        mantissa
            .checked_mul(10u128.pow(6 - scale))
            .ok_or(PercolatorError::EngineArithmeticOverflow)?
    };
    if out == 0 || out > u128::from(percolator::MAX_ORACLE_PRICE) {
        return Err(PercolatorError::OracleInvalid.into());
    }
    Ok((out as u64, 0))
}

#[derive(Clone, Copy)]
struct ReferencePythObservation {
    feed_id: [u8; 32],
    price: i64,
    exponent: i32,
    confidence: u64,
    publish_time: i64,
}

fn reference_pyth_observation(
    observation: ReferencePythObservation,
    expected_feed_id: [u8; 32],
    now_unix_ts: i64,
    max_staleness_secs: u64,
    conf_bps: u16,
) -> OracleParseResult {
    if observation.feed_id != expected_feed_id {
        return Err(PercolatorError::InvalidOracleKey.into());
    }
    if observation.price <= 0 || !(-18..=18).contains(&observation.exponent) {
        return Err(PercolatorError::OracleInvalid.into());
    }
    if !reference_publish_time_is_fresh(observation.publish_time, now_unix_ts, max_staleness_secs) {
        return Err(PercolatorError::OracleStale.into());
    }
    let price = observation.price as u128;
    if reference_confidence_is_too_wide(u128::from(observation.confidence), price, conf_bps) {
        return Err(PercolatorError::OracleConfTooWide.into());
    }
    let scale = observation.exponent + 6;
    let out = if scale >= 0 {
        price
            .checked_mul(10u128.pow(scale as u32))
            .ok_or(PercolatorError::EngineArithmeticOverflow)?
    } else {
        price / 10u128.pow((-scale) as u32)
    };
    if out == 0 || out > u128::from(percolator::MAX_ORACLE_PRICE) {
        return Err(PercolatorError::OracleInvalid.into());
    }
    Ok((out as u64, observation.publish_time))
}

#[derive(Clone, Copy)]
struct ReferenceSwitchboardObservation {
    feed_hash: [u8; 32],
    value: i128,
    std_dev: i128,
    account_update_time: i64,
    publish_time: i64,
    num_samples: u8,
    min_sample_size: u8,
    submission_idx: u8,
    result_slot: u64,
}

fn make_reference_switchboard_data(observation: ReferenceSwitchboardObservation) -> Vec<u8> {
    let mut data = make_switchboard_data(
        &observation.feed_hash,
        observation.value,
        observation.std_dev,
        observation.publish_time,
        observation.num_samples,
        observation.min_sample_size,
        observation.result_slot,
    );
    data[2216..2224].copy_from_slice(&observation.account_update_time.to_le_bytes());
    data[2361] = observation.submission_idx;
    if usize::from(observation.submission_idx) < 32 {
        let offset = 2952 + usize::from(observation.submission_idx) * 8;
        data[offset..offset + 8].copy_from_slice(&observation.publish_time.to_le_bytes());
    }
    data
}

fn reference_switchboard_observation(
    observation: ReferenceSwitchboardObservation,
    now_unix_ts: i64,
    max_staleness_secs: u64,
    conf_bps: u16,
) -> OracleParseResult {
    if observation.feed_hash == [0; 32]
        || observation.min_sample_size == 0
        || observation.num_samples < observation.min_sample_size
        || observation.submission_idx >= 32
        || observation.result_slot == 0
        || observation.account_update_time <= 0
        || observation.value <= 0
        || observation.std_dev < 0
    {
        return Err(PercolatorError::OracleInvalid.into());
    }
    if observation.publish_time <= 0
        || !reference_publish_time_is_fresh(
            observation.publish_time,
            now_unix_ts,
            max_staleness_secs,
        )
    {
        return Err(PercolatorError::OracleStale.into());
    }
    let value = observation.value as u128;
    if reference_confidence_is_too_wide(observation.std_dev as u128, value, conf_bps) {
        return Err(PercolatorError::OracleConfTooWide.into());
    }
    let out = value / REFERENCE_SWITCHBOARD_SCALE;
    if out == 0 || out > u128::from(percolator::MAX_ORACLE_PRICE) {
        return Err(PercolatorError::OracleInvalid.into());
    }
    Ok((out as u64, observation.publish_time))
}

#[derive(Clone, Copy)]
struct ReferenceChainlinkObservation {
    version: u8,
    decimals: u8,
    latest_round_id: u32,
    live_length: u32,
    result_slot: u64,
    publish_time: u32,
    answer: i128,
}

fn reference_chainlink_observation(
    observation: ReferenceChainlinkObservation,
    now_unix_ts: i64,
    max_staleness_secs: u64,
) -> OracleParseResult {
    let publish_time = i64::from(observation.publish_time);
    if observation.version == 0
        || observation.latest_round_id == 0
        || observation.live_length != 1
        || observation.result_slot == 0
        || publish_time <= 0
    {
        return Err(PercolatorError::OracleInvalid.into());
    }
    if !reference_publish_time_is_fresh(publish_time, now_unix_ts, max_staleness_secs) {
        return Err(PercolatorError::OracleStale.into());
    }
    reference_scale_decimal_to_e6(observation.answer, u32::from(observation.decimals))
        .map(|(price, _)| (price, publish_time))
}

fn assert_pure_oracle_parser_matches_reference(
    label: &str,
    owner: Pubkey,
    account_key: Pubkey,
    data: &[u8],
    expected_feed_id: &[u8; 32],
    now_unix_ts: i64,
    max_staleness_secs: u64,
    conf_bps: u16,
    expected: OracleParseResult,
) {
    let parsed = std::panic::catch_unwind(|| {
        oracle_v16::read_oracle_price_e6_from_bytes(
            &owner,
            &account_key,
            data,
            expected_feed_id,
            now_unix_ts,
            max_staleness_secs,
            conf_bps,
        )
    })
    .unwrap_or_else(|_| panic!("{label}: production parser panicked"));
    assert_eq!(parsed, expected, "{label}");
}

fn take_axis<T: Copy>(word: &mut usize, axis: &[T]) -> T {
    let value = axis[*word % axis.len()];
    *word /= axis.len();
    value
}

#[test]
fn host_oracle_valid_layout_boundaries_match_independent_typed_reference() {
    const NOW: i64 = 100;
    const MAX_STALENESS: u64 = 60;
    const PRICE: u64 = 1_500_000;
    let feed_id = [0x61; 32];
    let pyth_key = Pubkey::new_unique();
    let base_pyth = ReferencePythObservation {
        feed_id,
        price: PRICE as i64,
        exponent: -6,
        confidence: 1,
        publish_time: NOW,
    };
    let mut compared = 0usize;

    for price in [i64::MIN, -1, 0, 1, 999_999, 1_000_000, i64::MAX] {
        for exponent in [-19, -18, -7, -6, -5, 0, 12, 18, 19] {
            let observation = ReferencePythObservation {
                price,
                exponent,
                ..base_pyth
            };
            let data = make_pyth_data(
                &observation.feed_id,
                observation.price,
                observation.exponent,
                observation.confidence,
                observation.publish_time,
            );
            assert_pure_oracle_parser_matches_reference(
                "Pyth price/exponent boundary",
                oracle_v16::PYTH_RECEIVER_PROGRAM_ID,
                pyth_key,
                &data,
                &feed_id,
                NOW,
                MAX_STALENESS,
                100,
                reference_pyth_observation(observation, feed_id, NOW, MAX_STALENESS, 100),
            );
            compared += 1;
        }
    }
    for confidence in [0, 1, 14_999, 15_000, 15_001, u64::MAX] {
        for conf_bps in [0, 1, 99, 100, 10_000, u16::MAX] {
            let observation = ReferencePythObservation {
                confidence,
                ..base_pyth
            };
            let data = make_pyth_data(
                &observation.feed_id,
                observation.price,
                observation.exponent,
                observation.confidence,
                observation.publish_time,
            );
            assert_pure_oracle_parser_matches_reference(
                "Pyth confidence boundary",
                oracle_v16::PYTH_RECEIVER_PROGRAM_ID,
                pyth_key,
                &data,
                &feed_id,
                NOW,
                MAX_STALENESS,
                conf_bps,
                reference_pyth_observation(observation, feed_id, NOW, MAX_STALENESS, conf_bps),
            );
            compared += 1;
        }
    }
    for publish_time in [i64::MIN, -1, 0, 39, 40, 100, 101, i64::MAX] {
        for max_staleness_secs in [0, 60, 61, u64::MAX] {
            let observation = ReferencePythObservation {
                publish_time,
                ..base_pyth
            };
            let data = make_pyth_data(
                &observation.feed_id,
                observation.price,
                observation.exponent,
                observation.confidence,
                observation.publish_time,
            );
            assert_pure_oracle_parser_matches_reference(
                "Pyth timestamp boundary",
                oracle_v16::PYTH_RECEIVER_PROGRAM_ID,
                pyth_key,
                &data,
                &feed_id,
                NOW,
                max_staleness_secs,
                100,
                reference_pyth_observation(observation, feed_id, NOW, max_staleness_secs, 100),
            );
            compared += 1;
        }
    }
    let wrong_feed = [0x62; 32];
    let wrong_pyth = ReferencePythObservation {
        feed_id: wrong_feed,
        ..base_pyth
    };
    assert_pure_oracle_parser_matches_reference(
        "Pyth feed identity boundary",
        oracle_v16::PYTH_RECEIVER_PROGRAM_ID,
        pyth_key,
        &make_pyth_data(
            &wrong_pyth.feed_id,
            wrong_pyth.price,
            wrong_pyth.exponent,
            wrong_pyth.confidence,
            wrong_pyth.publish_time,
        ),
        &feed_id,
        NOW,
        MAX_STALENESS,
        100,
        reference_pyth_observation(wrong_pyth, feed_id, NOW, MAX_STALENESS, 100),
    );
    compared += 1;

    let switchboard_key = Pubkey::new_unique();
    let switchboard_feed = switchboard_key.to_bytes();
    let base_switchboard = ReferenceSwitchboardObservation {
        feed_hash: [0x71; 32],
        value: i128::from(PRICE) * REFERENCE_SWITCHBOARD_SCALE as i128,
        std_dev: 1,
        account_update_time: NOW,
        publish_time: NOW,
        num_samples: 3,
        min_sample_size: 1,
        submission_idx: 0,
        result_slot: 1,
    };
    for value in [
        i128::MIN,
        -1,
        0,
        1,
        REFERENCE_SWITCHBOARD_SCALE as i128 - 1,
        REFERENCE_SWITCHBOARD_SCALE as i128,
        i128::from(percolator::MAX_ORACLE_PRICE) * REFERENCE_SWITCHBOARD_SCALE as i128,
        (i128::from(percolator::MAX_ORACLE_PRICE) + 1) * REFERENCE_SWITCHBOARD_SCALE as i128,
        i128::MAX,
    ] {
        for std_dev in [i128::MIN, -1, 0, 1, 14_999, 15_000, 15_001, i128::MAX] {
            for conf_bps in [0, 1, 99, 100, 10_000, u16::MAX] {
                let observation = ReferenceSwitchboardObservation {
                    value,
                    std_dev,
                    ..base_switchboard
                };
                let data = make_reference_switchboard_data(observation);
                assert_pure_oracle_parser_matches_reference(
                    "Switchboard value/confidence boundary",
                    oracle_v16::SWITCHBOARD_ON_DEMAND_MAINNET_PROGRAM_ID,
                    switchboard_key,
                    &data,
                    &switchboard_feed,
                    NOW,
                    MAX_STALENESS,
                    conf_bps,
                    reference_switchboard_observation(observation, NOW, MAX_STALENESS, conf_bps),
                );
                compared += 1;
            }
        }
    }
    for publish_time in [i64::MIN, -1, 0, 39, 40, 100, 101, i64::MAX] {
        for max_staleness_secs in [0, 60, 61, u64::MAX] {
            let observation = ReferenceSwitchboardObservation {
                publish_time,
                ..base_switchboard
            };
            let data = make_reference_switchboard_data(observation);
            assert_pure_oracle_parser_matches_reference(
                "Switchboard timestamp boundary",
                oracle_v16::SWITCHBOARD_ON_DEMAND_MAINNET_PROGRAM_ID,
                switchboard_key,
                &data,
                &switchboard_feed,
                NOW,
                max_staleness_secs,
                100,
                reference_switchboard_observation(observation, NOW, max_staleness_secs, 100),
            );
            compared += 1;
        }
    }
    for (
        feed_hash,
        account_update_time,
        num_samples,
        min_sample_size,
        submission_idx,
        result_slot,
    ) in [
        ([0; 32], NOW, 3, 1, 0, 1),
        ([0x71; 32], 0, 3, 1, 0, 1),
        ([0x71; 32], NOW, 3, 0, 0, 1),
        ([0x71; 32], NOW, 0, 1, 0, 1),
        ([0x71; 32], NOW, 3, 1, 31, 1),
        ([0x71; 32], NOW, 3, 1, 32, 1),
        ([0x71; 32], NOW, 3, 1, u8::MAX, 1),
        ([0x71; 32], NOW, 3, 1, 0, 0),
        ([0x71; 32], NOW, 3, 1, 0, u64::MAX),
    ] {
        let observation = ReferenceSwitchboardObservation {
            feed_hash,
            account_update_time,
            num_samples,
            min_sample_size,
            submission_idx,
            result_slot,
            ..base_switchboard
        };
        let data = make_reference_switchboard_data(observation);
        assert_pure_oracle_parser_matches_reference(
            "Switchboard structural boundary",
            oracle_v16::SWITCHBOARD_ON_DEMAND_MAINNET_PROGRAM_ID,
            switchboard_key,
            &data,
            &switchboard_feed,
            NOW,
            MAX_STALENESS,
            100,
            reference_switchboard_observation(observation, NOW, MAX_STALENESS, 100),
        );
        compared += 1;
    }
    let switchboard_data = make_reference_switchboard_data(base_switchboard);
    assert_pure_oracle_parser_matches_reference(
        "Switchboard devnet owner boundary",
        oracle_v16::SWITCHBOARD_ON_DEMAND_DEVNET_PROGRAM_ID,
        switchboard_key,
        &switchboard_data,
        &switchboard_feed,
        NOW,
        MAX_STALENESS,
        100,
        reference_switchboard_observation(base_switchboard, NOW, MAX_STALENESS, 100),
    );
    compared += 1;

    let chainlink_key = Pubkey::new_unique();
    let chainlink_feed = chainlink_key.to_bytes();
    let base_chainlink = ReferenceChainlinkObservation {
        version: 1,
        decimals: 6,
        latest_round_id: 1,
        live_length: 1,
        result_slot: 1,
        publish_time: NOW as u32,
        answer: i128::from(PRICE),
    };
    for answer in [
        i128::MIN,
        -1,
        0,
        1,
        999_999,
        1_000_000,
        i128::from(percolator::MAX_ORACLE_PRICE),
        i128::from(percolator::MAX_ORACLE_PRICE) + 1,
        i128::MAX,
    ] {
        for decimals in [0, 1, 5, 6, 7, 12, 17, 18, 19, u8::MAX] {
            let observation = ReferenceChainlinkObservation {
                answer,
                decimals,
                ..base_chainlink
            };
            let data = make_chainlink_data(
                observation.version,
                observation.decimals,
                observation.latest_round_id,
                observation.live_length,
                observation.result_slot,
                observation.publish_time,
                observation.answer,
            );
            assert_pure_oracle_parser_matches_reference(
                "Chainlink answer/decimal boundary",
                oracle_v16::CHAINLINK_STORE_PROGRAM_ID,
                chainlink_key,
                &data,
                &chainlink_feed,
                NOW,
                MAX_STALENESS,
                0,
                reference_chainlink_observation(observation, NOW, MAX_STALENESS),
            );
            compared += 1;
        }
    }
    for publish_time in [0, 39, 40, 100, 101, u32::MAX] {
        for max_staleness_secs in [0, 60, 61, u64::MAX] {
            let observation = ReferenceChainlinkObservation {
                publish_time,
                ..base_chainlink
            };
            let data = make_chainlink_data(
                observation.version,
                observation.decimals,
                observation.latest_round_id,
                observation.live_length,
                observation.result_slot,
                observation.publish_time,
                observation.answer,
            );
            assert_pure_oracle_parser_matches_reference(
                "Chainlink timestamp boundary",
                oracle_v16::CHAINLINK_STORE_PROGRAM_ID,
                chainlink_key,
                &data,
                &chainlink_feed,
                NOW,
                max_staleness_secs,
                0,
                reference_chainlink_observation(observation, NOW, max_staleness_secs),
            );
            compared += 1;
        }
    }
    for (version, latest_round_id, live_length, result_slot) in [
        (0, 1, 1, 1),
        (1, 0, 1, 1),
        (1, 1, 0, 1),
        (1, 1, 2, 1),
        (1, 1, 1, 0),
        (u8::MAX, u32::MAX, 1, u64::MAX),
    ] {
        let observation = ReferenceChainlinkObservation {
            version,
            latest_round_id,
            live_length,
            result_slot,
            ..base_chainlink
        };
        let data = make_chainlink_data(
            observation.version,
            observation.decimals,
            observation.latest_round_id,
            observation.live_length,
            observation.result_slot,
            observation.publish_time,
            observation.answer,
        );
        assert_pure_oracle_parser_matches_reference(
            "Chainlink structural boundary",
            oracle_v16::CHAINLINK_STORE_PROGRAM_ID,
            chainlink_key,
            &data,
            &chainlink_feed,
            NOW,
            MAX_STALENESS,
            0,
            reference_chainlink_observation(observation, NOW, MAX_STALENESS),
        );
        compared += 1;
    }

    assert_eq!(compared, 726);
    println!("independent valid-layout oracle reference: {compared} boundary words");
}

#[test]
fn host_oracle_structural_cartesian_matches_independent_error_precedence() {
    const NOW: i64 = 100;
    const MAX_STALENESS: u64 = 60;
    const PRICE: u64 = 1_500_000;
    let pyth_key = Pubkey::new_unique();
    let pyth_feed = [0x91; 32];
    let pyth_other_feed = [0x92; 32];
    let pyth_case_count: usize = 2 * 2 * 2 * 2 * 3 * 3 * 2 * 2;
    for case in 0..pyth_case_count {
        let mut word = case;
        let discriminator_valid = take_axis(&mut word, &[false, true]);
        let verification_full = take_axis(&mut word, &[false, true]);
        let feed_matches = take_axis(&mut word, &[false, true]);
        let price = take_axis(&mut word, &[0, PRICE as i64]);
        let exponent = take_axis(&mut word, &[-19, -6, 19]);
        let publish_time = take_axis(&mut word, &[0, 40, 101]);
        let confidence = take_axis(&mut word, &[0, 15_001]);
        let conf_bps = take_axis(&mut word, &[0, 100]);
        assert_eq!(word, 0);
        let observation = ReferencePythObservation {
            feed_id: pyth_feed,
            price,
            exponent,
            confidence,
            publish_time,
        };
        let mut data = make_pyth_data(
            &observation.feed_id,
            observation.price,
            observation.exponent,
            observation.confidence,
            observation.publish_time,
        );
        if !discriminator_valid {
            data[0] ^= u8::MAX;
        }
        if !verification_full {
            data[40] = 0;
        }
        let expected_feed = if feed_matches {
            pyth_feed
        } else {
            pyth_other_feed
        };
        let expected = if !discriminator_valid || !verification_full {
            Err(PercolatorError::OracleInvalid.into())
        } else {
            reference_pyth_observation(observation, expected_feed, NOW, MAX_STALENESS, conf_bps)
        };
        assert_pure_oracle_parser_matches_reference(
            "Pyth structural Cartesian",
            oracle_v16::PYTH_RECEIVER_PROGRAM_ID,
            pyth_key,
            &data,
            &expected_feed,
            NOW,
            MAX_STALENESS,
            conf_bps,
            expected,
        );
    }

    let switchboard_key = Pubkey::new_from_array([0xA1; 32]);
    let switchboard_other_feed = [0xA2; 32];
    let switchboard_case_count: usize = 2 * 2 * 2 * 2 * 3 * 2 * 2 * 3 * 2 * 3 * 2 * 2;
    for case in 0..switchboard_case_count {
        let mut word = case;
        let key_matches = take_axis(&mut word, &[false, true]);
        let discriminator_valid = take_axis(&mut word, &[false, true]);
        let feed_hash = take_axis(&mut word, &[[0; 32], [0xA3; 32]]);
        let account_update_time = take_axis(&mut word, &[0, NOW]);
        let (num_samples, min_sample_size) = take_axis(&mut word, &[(3, 1), (1, 0), (0, 1)]);
        let submission_idx = take_axis(&mut word, &[31, 32]);
        let result_slot = take_axis(&mut word, &[0, 1]);
        let publish_time = take_axis(&mut word, &[0, 40, 101]);
        let value = take_axis(
            &mut word,
            &[0, i128::from(PRICE) * REFERENCE_SWITCHBOARD_SCALE as i128],
        );
        let std_dev = take_axis(&mut word, &[-1, 0, REFERENCE_SWITCHBOARD_SCALE as i128]);
        let max_staleness_secs = take_axis(&mut word, &[0, MAX_STALENESS]);
        let conf_bps = take_axis(&mut word, &[0, 100]);
        assert_eq!(word, 0);
        let observation = ReferenceSwitchboardObservation {
            feed_hash,
            value,
            std_dev,
            account_update_time,
            publish_time,
            num_samples,
            min_sample_size,
            submission_idx,
            result_slot,
        };
        let mut data = make_reference_switchboard_data(observation);
        if !discriminator_valid {
            data[0] ^= u8::MAX;
        }
        let expected_feed = if key_matches {
            switchboard_key.to_bytes()
        } else {
            switchboard_other_feed
        };
        let expected = if !key_matches {
            Err(PercolatorError::InvalidOracleKey.into())
        } else if !discriminator_valid {
            Err(PercolatorError::OracleInvalid.into())
        } else {
            reference_switchboard_observation(observation, NOW, max_staleness_secs, conf_bps)
        };
        assert_pure_oracle_parser_matches_reference(
            "Switchboard structural Cartesian",
            oracle_v16::SWITCHBOARD_ON_DEMAND_MAINNET_PROGRAM_ID,
            switchboard_key,
            &data,
            &expected_feed,
            NOW,
            max_staleness_secs,
            conf_bps,
            expected,
        );
    }

    let chainlink_key = Pubkey::new_from_array([0xB1; 32]);
    let chainlink_other_feed = [0xB2; 32];
    let chainlink_case_count: usize = 2 * 2 * 2 * 2 * 3 * 2 * 3 * 2 * 2;
    for case in 0..chainlink_case_count {
        let mut word = case;
        let key_matches = take_axis(&mut word, &[false, true]);
        let discriminator_valid = take_axis(&mut word, &[false, true]);
        let version = take_axis(&mut word, &[0, 1]);
        let latest_round_id = take_axis(&mut word, &[0, 1]);
        let live_length = take_axis(&mut word, &[0, 1, 2]);
        let result_slot = take_axis(&mut word, &[0, 1]);
        let publish_time = take_axis(&mut word, &[0, 40, 101]);
        let answer = take_axis(&mut word, &[0, i128::from(PRICE)]);
        let decimals = take_axis(&mut word, &[6, 19]);
        assert_eq!(word, 0);
        let observation = ReferenceChainlinkObservation {
            version,
            decimals,
            latest_round_id,
            live_length,
            result_slot,
            publish_time,
            answer,
        };
        let mut data = make_chainlink_data(
            observation.version,
            observation.decimals,
            observation.latest_round_id,
            observation.live_length,
            observation.result_slot,
            observation.publish_time,
            observation.answer,
        );
        if !discriminator_valid {
            data[0] ^= u8::MAX;
        }
        let expected_feed = if key_matches {
            chainlink_key.to_bytes()
        } else {
            chainlink_other_feed
        };
        let expected = if !key_matches {
            Err(PercolatorError::InvalidOracleKey.into())
        } else if !discriminator_valid {
            Err(PercolatorError::OracleInvalid.into())
        } else {
            reference_chainlink_observation(observation, NOW, MAX_STALENESS)
        };
        assert_pure_oracle_parser_matches_reference(
            "Chainlink structural Cartesian",
            oracle_v16::CHAINLINK_STORE_PROGRAM_ID,
            chainlink_key,
            &data,
            &expected_feed,
            NOW,
            MAX_STALENESS,
            0,
            expected,
        );
    }

    assert_eq!(pyth_case_count, 576);
    assert_eq!(switchboard_case_count, 13_824);
    assert_eq!(chainlink_case_count, 1_152);
    println!(
        "independent structural oracle Cartesian: {} words",
        pyth_case_count + switchboard_case_count + chainlink_case_count
    );
}

#[test]
fn host_oracle_generated_valid_layouts_match_independent_typed_reference() {
    const CASES_PER_PROVIDER: usize = 4_096;
    let mut rng = XorShiftRng::from_seed(*b"INV020-PARSER-01");
    let pyth_key = Pubkey::new_from_array([0xC1; 32]);
    let switchboard_key = Pubkey::new_from_array([0xC2; 32]);
    let chainlink_key = Pubkey::new_from_array([0xC3; 32]);

    for case in 0..CASES_PER_PROVIDER {
        let now_unix_ts: i64 = rng.gen();
        let max_staleness_secs: u64 = rng.gen();
        let conf_bps: u16 = rng.gen();
        let mut feed_id: [u8; 32] = rng.gen();
        feed_id[0] |= 1;
        let pyth = ReferencePythObservation {
            feed_id,
            price: rng.gen(),
            exponent: if case % 4 == 0 {
                rng.gen()
            } else {
                rng.gen_range(-18..=18)
            },
            confidence: rng.gen(),
            publish_time: rng.gen(),
        };
        let pyth_data = make_pyth_data(
            &pyth.feed_id,
            pyth.price,
            pyth.exponent,
            pyth.confidence,
            pyth.publish_time,
        );
        let pyth_label = format!(
            "Pyth generated valid layout case={case} price={} exponent={} confidence={} publish_time={} now={} max_staleness={} conf_bps={conf_bps}",
            pyth.price,
            pyth.exponent,
            pyth.confidence,
            pyth.publish_time,
            now_unix_ts,
            max_staleness_secs,
        );
        assert_pure_oracle_parser_matches_reference(
            &pyth_label,
            oracle_v16::PYTH_RECEIVER_PROGRAM_ID,
            pyth_key,
            &pyth_data,
            &pyth.feed_id,
            now_unix_ts,
            max_staleness_secs,
            conf_bps,
            reference_pyth_observation(
                pyth,
                pyth.feed_id,
                now_unix_ts,
                max_staleness_secs,
                conf_bps,
            ),
        );

        let mut feed_hash: [u8; 32] = rng.gen();
        feed_hash[0] |= 1;
        let switchboard = ReferenceSwitchboardObservation {
            feed_hash,
            value: rng.gen(),
            std_dev: rng.gen(),
            account_update_time: rng.gen(),
            publish_time: rng.gen(),
            num_samples: 3,
            min_sample_size: 1,
            submission_idx: (rng.gen::<u8>() % 32),
            result_slot: rng.gen(),
        };
        let switchboard_data = make_reference_switchboard_data(switchboard);
        assert_pure_oracle_parser_matches_reference(
            "Switchboard generated valid layout",
            oracle_v16::SWITCHBOARD_ON_DEMAND_MAINNET_PROGRAM_ID,
            switchboard_key,
            &switchboard_data,
            &switchboard_key.to_bytes(),
            now_unix_ts,
            max_staleness_secs,
            conf_bps,
            reference_switchboard_observation(
                switchboard,
                now_unix_ts,
                max_staleness_secs,
                conf_bps,
            ),
        );

        let chainlink = ReferenceChainlinkObservation {
            version: rng.gen(),
            decimals: if case % 4 == 0 {
                rng.gen()
            } else {
                rng.gen_range(0..=18)
            },
            latest_round_id: rng.gen(),
            live_length: if case % 4 == 0 { rng.gen() } else { 1 },
            result_slot: rng.gen(),
            publish_time: rng.gen(),
            answer: rng.gen(),
        };
        let chainlink_data = make_chainlink_data(
            chainlink.version,
            chainlink.decimals,
            chainlink.latest_round_id,
            chainlink.live_length,
            chainlink.result_slot,
            chainlink.publish_time,
            chainlink.answer,
        );
        assert_pure_oracle_parser_matches_reference(
            "Chainlink generated valid layout",
            oracle_v16::CHAINLINK_STORE_PROGRAM_ID,
            chainlink_key,
            &chainlink_data,
            &chainlink_key.to_bytes(),
            now_unix_ts,
            max_staleness_secs,
            0,
            reference_chainlink_observation(chainlink, now_unix_ts, max_staleness_secs),
        );
    }

    println!(
        "independent generated valid-layout oracle reference: {} words",
        CASES_PER_PROVIDER * 3
    );
}

fn write_epoch_matrix_leg(
    env: &mut V16CuEnv,
    leg: EpochMatrixLeg,
    price_e6: u64,
    publish_time: i64,
    result_slot: u64,
) {
    let (owner, data) = match leg.provider {
        EpochMatrixProvider::Pyth => (
            oracle_v16::PYTH_RECEIVER_PROGRAM_ID,
            make_pyth_data(
                &leg.feed,
                i64::try_from(price_e6).expect("matrix Pyth price fits i64"),
                -6,
                1,
                publish_time,
            ),
        ),
        EpochMatrixProvider::Switchboard => (
            oracle_v16::SWITCHBOARD_ON_DEMAND_MAINNET_PROGRAM_ID,
            make_switchboard_data(
                &[0xabu8; 32],
                i128::from(price_e6) * 1_000_000_000_000,
                1,
                publish_time,
                3,
                1,
                result_slot.max(1),
            ),
        ),
        EpochMatrixProvider::Chainlink => (
            oracle_v16::CHAINLINK_STORE_PROGRAM_ID,
            make_chainlink_data(
                1,
                6,
                1,
                1,
                result_slot.max(1),
                u32::try_from(publish_time).expect("matrix Chainlink time fits u32"),
                i128::from(price_e6),
            ),
        ),
    };
    env.svm
        .set_account(
            leg.account,
            Account {
                lamports: 1_000_000_000,
                data,
                owner,
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();
}

fn new_epoch_matrix_leg(
    env: &mut V16CuEnv,
    provider: EpochMatrixProvider,
    case_index: usize,
    leg_index: usize,
    price_e6: u64,
    publish_time: i64,
    result_slot: u64,
) -> EpochMatrixLeg {
    let account = Pubkey::new_unique();
    let feed = match provider {
        EpochMatrixProvider::Pyth => {
            let mut feed = [0u8; 32];
            feed[..8].copy_from_slice(&(case_index as u64 + 1).to_le_bytes());
            feed[8] = leg_index as u8 + 1;
            feed[31] = 1;
            feed
        }
        EpochMatrixProvider::Switchboard | EpochMatrixProvider::Chainlink => account.to_bytes(),
    };
    let leg = EpochMatrixLeg {
        provider,
        account,
        feed,
        price_e6,
    };
    write_epoch_matrix_leg(env, leg, price_e6, publish_time, result_slot);
    leg
}

#[test]
fn host_oracle_signed_elapsed_time_does_not_saturate() {
    const NOW_UNIX_TS: i64 = 100;
    const PUBLISH_TIME: i64 = i64::MIN;
    const EXACT_AGE: u64 = i64::MAX as u64 + 101;
    let account_key = Pubkey::new_unique();
    let feed = [0xD1; 32];
    let data = make_pyth_data(&feed, 1_000_000, -6, 0, PUBLISH_TIME);

    assert_pure_oracle_parser_matches_reference(
        "Pyth true age above large u64 freshness bound",
        oracle_v16::PYTH_RECEIVER_PROGRAM_ID,
        account_key,
        &data,
        &feed,
        NOW_UNIX_TS,
        EXACT_AGE - 1,
        0,
        Err(PercolatorError::OracleStale.into()),
    );
    assert_pure_oracle_parser_matches_reference(
        "Pyth true age exactly at large u64 freshness bound",
        oracle_v16::PYTH_RECEIVER_PROGRAM_ID,
        account_key,
        &data,
        &feed,
        NOW_UNIX_TS,
        EXACT_AGE,
        0,
        Ok((1_000_000, PUBLISH_TIME)),
    );
}

fn composite_epoch_matrix_cases() -> Vec<EpochMatrixCase> {
    const PROVIDERS: [EpochMatrixProvider; 3] = [
        EpochMatrixProvider::Pyth,
        EpochMatrixProvider::Switchboard,
        EpochMatrixProvider::Chainlink,
    ];
    const SCALES: [u32; 2] = [0, 10];
    const THREE_LEG_FLAGS: [u8; 4] = [
        0,
        ORACLE_LEG_FLAG_DIVIDE_LEG2,
        ORACLE_LEG_FLAG_DIVIDE_LEG3,
        ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3,
    ];

    let mut cases = Vec::new();
    for provider in PROVIDERS {
        for invert in [0, 1] {
            for unit_scale in SCALES {
                cases.push(EpochMatrixCase {
                    providers: [
                        provider,
                        EpochMatrixProvider::Pyth,
                        EpochMatrixProvider::Pyth,
                    ],
                    count: 1,
                    flags: 0,
                    invert,
                    unit_scale,
                });
            }
        }
    }
    for first in PROVIDERS {
        for second in PROVIDERS {
            for flags in [0, ORACLE_LEG_FLAG_DIVIDE_LEG2] {
                for invert in [0, 1] {
                    for unit_scale in SCALES {
                        cases.push(EpochMatrixCase {
                            providers: [first, second, EpochMatrixProvider::Pyth],
                            count: 2,
                            flags,
                            invert,
                            unit_scale,
                        });
                    }
                }
            }
        }
    }
    let mut word_index = 0usize;
    for first in PROVIDERS {
        for second in PROVIDERS {
            for third in PROVIDERS {
                let case = EpochMatrixCase {
                    providers: [first, second, third],
                    count: 3,
                    flags: THREE_LEG_FLAGS[word_index % THREE_LEG_FLAGS.len()],
                    invert: ((word_index / THREE_LEG_FLAGS.len()) % 2) as u8,
                    unit_scale: SCALES[(word_index / (THREE_LEG_FLAGS.len() * 2)) % SCALES.len()],
                };
                cases.push(case);
                word_index += 1;
            }
        }
    }
    for flags in THREE_LEG_FLAGS {
        for invert in [0, 1] {
            for unit_scale in SCALES {
                let case = EpochMatrixCase {
                    providers: [
                        EpochMatrixProvider::Pyth,
                        EpochMatrixProvider::Switchboard,
                        EpochMatrixProvider::Chainlink,
                    ],
                    count: 3,
                    flags,
                    invert,
                    unit_scale,
                };
                if !cases.contains(&case) {
                    cases.push(case);
                }
            }
        }
    }
    cases
}

fn try_epoch_matrix_crank(
    env: &mut V16CuEnv,
    keeper_portfolio: Pubkey,
    slot: u64,
    oracle_accounts: &[Pubkey],
) -> Result<u64, String> {
    let mut accounts = vec![
        AccountMeta::new(env.payer.pubkey(), true),
        AccountMeta::new(env.market, false),
        AccountMeta::new(keeper_portfolio, false),
    ];
    accounts.extend(
        oracle_accounts
            .iter()
            .copied()
            .map(|key| AccountMeta::new_readonly(key, false)),
    );
    env.svm.expire_blockhash();
    env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: slot,
            observations: crank_observations_with_accounts(0, oracle_accounts.len() as u8),
        },
        accounts,
        &[],
    )
}

#[test]
fn v16_program_composite_epoch_coherence_crosses_all_providers_and_transforms() {
    const PRICES_E6: [u64; 3] = [6_000_000, 2_000_000, 3_000_000];

    let mut env = V16CuEnv::new();
    let keeper = Keypair::new();
    let keeper_portfolio = env.create_portfolio(&keeper);
    let cases = composite_epoch_matrix_cases();
    let mut skew_rejections = 0usize;
    let mut rewind_rejections = 0usize;
    let mut max_config_cu = 0u64;
    let mut max_crank_cu = 0u64;

    for (case_index, case) in cases.iter().copied().enumerate() {
        let initial_slot = 10 + case_index as u64 * 2;
        let update_slot = initial_slot + 1;
        let initial_time = 1_000 + case_index as i64 * 2;
        let update_time = initial_time + 1;
        set_test_clock(&mut env, initial_slot, initial_time);

        let legs: Vec<EpochMatrixLeg> = (0..case.count as usize)
            .map(|leg_index| {
                new_epoch_matrix_leg(
                    &mut env,
                    case.providers[leg_index],
                    case_index,
                    leg_index,
                    PRICES_E6[leg_index],
                    initial_time,
                    initial_slot,
                )
            })
            .collect();
        let mut feeds = [[0u8; 32]; 3];
        for (index, leg) in legs.iter().enumerate() {
            feeds[index] = leg.feed;
        }
        let oracle_accounts: Vec<Pubkey> = legs.iter().map(|leg| leg.account).collect();
        let config_cu = env
            .try_configure_hybrid_asset_with_conf_filter_cu(
                0,
                case.count,
                case.flags,
                feeds,
                &oracle_accounts,
                initial_slot,
                initial_time,
                case.invert,
                case.unit_scale,
                3,
                100,
            )
            .unwrap_or_else(|error| panic!("matrix config {case_index} {case:?}: {error}"));
        max_config_cu = max_config_cu.max(config_cu);
        let baseline = env.market_state().0;
        assert!(
            baseline.oracle_target_price_e6 > 0,
            "case {case_index} {case:?}"
        );
        assert_eq!(baseline.oracle_target_publish_time, initial_time);
        assert_eq!(baseline.oracle_leg_count, case.count);
        assert_eq!(baseline.oracle_leg_flags, case.flags);
        assert_eq!(baseline.invert, case.invert);
        assert_eq!(baseline.unit_scale, case.unit_scale);

        set_test_clock(&mut env, update_slot, update_time);
        if case.count > 1 {
            let skew_index = case_index % case.count as usize;
            write_epoch_matrix_leg(
                &mut env,
                legs[skew_index],
                legs[skew_index].price_e6 + 1_000_000,
                update_time,
                update_slot,
            );
            let market_before = env.svm.get_account(&env.market).unwrap();
            let keeper_before = env.svm.get_account(&keeper_portfolio).unwrap();
            let error =
                try_epoch_matrix_crank(&mut env, keeper_portfolio, update_slot, &oracle_accounts)
                    .expect_err(
                        "cross-epoch composite report must reject before soft-stale fallback",
                    );
            assert!(
                error.contains("Custom(27)"),
                "case {case_index} {case:?} returned the wrong skew error: {error}"
            );
            assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
            assert_eq!(
                env.svm.get_account(&keeper_portfolio).unwrap(),
                keeper_before
            );
            skew_rejections += 1;
        }

        for leg in &legs {
            write_epoch_matrix_leg(&mut env, *leg, leg.price_e6, update_time, update_slot);
        }
        let crank_cu =
            try_epoch_matrix_crank(&mut env, keeper_portfolio, update_slot, &oracle_accounts)
                .unwrap_or_else(|error| {
                    panic!("coherent matrix crank {case_index} {case:?}: {error}")
                });
        max_crank_cu = max_crank_cu.max(crank_cu);
        let after = env.market_state().0;
        assert_eq!(
            after.oracle_target_price_e6, baseline.oracle_target_price_e6,
            "same-price coherent retry changed composition in case {case_index} {case:?}"
        );
        assert_eq!(after.oracle_target_publish_time, update_time);
        assert_eq!(after.last_good_oracle_slot, update_slot);
        assert_eq!(
            &after.oracle_leg_prices_e6[..case.count as usize],
            &PRICES_E6[..case.count as usize]
        );
        assert!(after.oracle_leg_publish_times[..case.count as usize]
            .iter()
            .all(|publish_time| *publish_time == update_time));

        let rewind_slot = update_slot + 1;
        let rewind_now = update_time + 1;
        set_test_clock(&mut env, rewind_slot, rewind_now);
        for leg in &legs {
            write_epoch_matrix_leg(&mut env, *leg, leg.price_e6, initial_time, rewind_slot);
        }
        let market_before = env.svm.get_account(&env.market).unwrap();
        let keeper_before = env.svm.get_account(&keeper_portfolio).unwrap();
        let rewind_error =
            try_epoch_matrix_crank(&mut env, keeper_portfolio, rewind_slot, &oracle_accounts)
                .expect_err("coherent but regressed composite epoch must reject");
        assert!(
            rewind_error.contains("Custom(27)"),
            "case {case_index} {case:?} returned the wrong rewind error: {rewind_error}"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(
            env.svm.get_account(&keeper_portfolio).unwrap(),
            keeper_before
        );
        rewind_rejections += 1;

        for leg in &legs {
            write_epoch_matrix_leg(&mut env, *leg, leg.price_e6, rewind_now, rewind_slot);
        }
        let retry_cu =
            try_epoch_matrix_crank(&mut env, keeper_portfolio, rewind_slot, &oracle_accounts)
                .unwrap_or_else(|error| panic!("post-rewind retry {case_index} {case:?}: {error}"));
        max_crank_cu = max_crank_cu.max(retry_cu);
        let retry = env.market_state().0;
        assert_eq!(
            retry.oracle_target_price_e6,
            baseline.oracle_target_price_e6
        );
        assert_eq!(retry.oracle_target_publish_time, rewind_now);
        assert_eq!(retry.last_good_oracle_slot, rewind_slot);
        assert!(retry.oracle_leg_publish_times[..case.count as usize]
            .iter()
            .all(|publish_time| *publish_time == rewind_now));
    }

    assert_eq!(
        skew_rejections,
        cases.iter().filter(|case| case.count > 1).count()
    );
    assert_eq!(rewind_rejections, cases.len());
    assert_cu_within(
        "composite epoch matrix configuration",
        max_config_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_cu_within(
        "composite epoch matrix coherent crank",
        max_crank_cu,
        CRANK_CU_LIMIT,
    );
    println!(
        "composite epoch matrix: {} cases, {} skew rejections, {} rewind rejections, config max {} CU, crank max {} CU",
        cases.len(),
        skew_rejections,
        rewind_rejections,
        max_config_cu,
        max_crank_cu
    );
}

fn composite_epoch_provider_words() -> Vec<EpochMatrixCase> {
    const PROVIDERS: [EpochMatrixProvider; 3] = [
        EpochMatrixProvider::Pyth,
        EpochMatrixProvider::Switchboard,
        EpochMatrixProvider::Chainlink,
    ];
    const THREE_LEG_FLAGS: [u8; 4] = [
        0,
        ORACLE_LEG_FLAG_DIVIDE_LEG2,
        ORACLE_LEG_FLAG_DIVIDE_LEG3,
        ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3,
    ];
    let mut cases = Vec::new();
    for provider in PROVIDERS {
        cases.push(EpochMatrixCase {
            providers: [
                provider,
                EpochMatrixProvider::Pyth,
                EpochMatrixProvider::Pyth,
            ],
            count: 1,
            flags: 0,
            invert: (cases.len() % 2) as u8,
            unit_scale: if cases.len() % 3 == 0 { 10 } else { 0 },
        });
    }
    for first in PROVIDERS {
        for second in PROVIDERS {
            let index = cases.len();
            cases.push(EpochMatrixCase {
                providers: [first, second, EpochMatrixProvider::Pyth],
                count: 2,
                flags: if index % 2 == 0 {
                    0
                } else {
                    ORACLE_LEG_FLAG_DIVIDE_LEG2
                },
                invert: ((index / 2) % 2) as u8,
                unit_scale: if (index / 4) % 2 == 0 { 0 } else { 10 },
            });
        }
    }
    for first in PROVIDERS {
        for second in PROVIDERS {
            for third in PROVIDERS {
                let index = cases.len();
                cases.push(EpochMatrixCase {
                    providers: [first, second, third],
                    count: 3,
                    flags: THREE_LEG_FLAGS[index % THREE_LEG_FLAGS.len()],
                    invert: ((index / THREE_LEG_FLAGS.len()) % 2) as u8,
                    unit_scale: if (index / (THREE_LEG_FLAGS.len() * 2)) % 2 == 0 {
                        0
                    } else {
                        10
                    },
                });
            }
        }
    }
    cases
}

#[test]
fn v16_program_composite_freshness_boundaries_cross_all_provider_orders() {
    const PRICES_E6: [u64; 3] = [6_000_000, 2_000_000, 3_000_000];

    let mut env = V16CuEnv::new();
    let keeper = Keypair::new();
    let keeper_portfolio = env.create_portfolio(&keeper);
    let cases = composite_epoch_provider_words();
    let mut stale_config_rejections = 0usize;
    let mut stale_crank_rejections = 0usize;
    let mut max_config_cu = 0u64;
    let mut max_crank_cu = 0u64;

    for (case_index, case) in cases.iter().copied().enumerate() {
        let initial_slot = 1_000 + case_index as u64 * 3;
        let now = 10_000 + case_index as i64 * 500;
        set_test_clock(&mut env, initial_slot, now);
        let legs: Vec<EpochMatrixLeg> = (0..case.count as usize)
            .map(|leg_index| {
                new_epoch_matrix_leg(
                    &mut env,
                    case.providers[leg_index],
                    case_index + 10_000,
                    leg_index,
                    PRICES_E6[leg_index],
                    now - 61,
                    initial_slot,
                )
            })
            .collect();
        let mut feeds = [[0u8; 32]; 3];
        for (index, leg) in legs.iter().enumerate() {
            feeds[index] = leg.feed;
        }
        let oracle_accounts: Vec<Pubkey> = legs.iter().map(|leg| leg.account).collect();
        let market_before = env.svm.get_account(&env.market).unwrap();
        let stale_config = env
            .try_configure_hybrid_asset_with_conf_filter_cu(
                0,
                case.count,
                case.flags,
                feeds,
                &oracle_accounts,
                initial_slot,
                now,
                case.invert,
                case.unit_scale,
                3,
                100,
            )
            .expect_err("max_staleness_secs + 1 configuration must reject");
        assert!(
            stale_config.contains("Custom(27)"),
            "case {case_index} {case:?} returned the wrong stale-config error: {stale_config}"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        stale_config_rejections += 1;

        for leg in &legs {
            write_epoch_matrix_leg(&mut env, *leg, leg.price_e6, now - 60, initial_slot);
        }
        env.svm.expire_blockhash();
        let config_cu = env
            .try_configure_hybrid_asset_with_conf_filter_cu(
                0,
                case.count,
                case.flags,
                feeds,
                &oracle_accounts,
                initial_slot,
                now,
                case.invert,
                case.unit_scale,
                3,
                100,
            )
            .unwrap_or_else(|error| panic!("exact-expiry config {case_index} {case:?}: {error}"));
        max_config_cu = max_config_cu.max(config_cu);
        let baseline_target = env.market_state().0.oracle_target_price_e6;

        let crank_slot = initial_slot + 1;
        let crank_now = now + 100;
        set_test_clock(&mut env, crank_slot, crank_now);
        for leg in &legs {
            write_epoch_matrix_leg(&mut env, *leg, leg.price_e6, crank_now - 61, crank_slot);
        }
        let market_before = env.svm.get_account(&env.market).unwrap();
        let keeper_before = env.svm.get_account(&keeper_portfolio).unwrap();
        let stale_crank =
            try_epoch_matrix_crank(&mut env, keeper_portfolio, crank_slot, &oracle_accounts)
                .expect_err("max_staleness_secs + 1 crank must reject before fallback maturity");
        assert!(
            stale_crank.contains("Custom(27)"),
            "case {case_index} {case:?} returned the wrong stale-crank error: {stale_crank}"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(
            env.svm.get_account(&keeper_portfolio).unwrap(),
            keeper_before
        );
        stale_crank_rejections += 1;

        for leg in &legs {
            write_epoch_matrix_leg(&mut env, *leg, leg.price_e6, crank_now - 60, crank_slot);
        }
        let exact_cu =
            try_epoch_matrix_crank(&mut env, keeper_portfolio, crank_slot, &oracle_accounts)
                .unwrap_or_else(|error| {
                    panic!("exact-expiry crank {case_index} {case:?}: {error}")
                });
        max_crank_cu = max_crank_cu.max(exact_cu);
        let exact = env.market_state().0;
        assert_eq!(exact.oracle_target_price_e6, baseline_target);
        assert_eq!(exact.oracle_target_publish_time, crank_now - 60);
        assert_eq!(exact.last_good_oracle_slot, crank_slot);

        let fresh_slot = initial_slot + 2;
        let fresh_now = now + 200;
        set_test_clock(&mut env, fresh_slot, fresh_now);
        for leg in &legs {
            write_epoch_matrix_leg(&mut env, *leg, leg.price_e6, fresh_now - 59, fresh_slot);
        }
        let fresh_cu =
            try_epoch_matrix_crank(&mut env, keeper_portfolio, fresh_slot, &oracle_accounts)
                .unwrap_or_else(|error| panic!("pre-expiry crank {case_index} {case:?}: {error}"));
        max_crank_cu = max_crank_cu.max(fresh_cu);
        let fresh = env.market_state().0;
        assert_eq!(fresh.oracle_target_price_e6, baseline_target);
        assert_eq!(fresh.oracle_target_publish_time, fresh_now - 59);
        assert_eq!(fresh.last_good_oracle_slot, fresh_slot);
    }

    assert_eq!(stale_config_rejections, cases.len());
    assert_eq!(stale_crank_rejections, cases.len());
    assert_cu_within(
        "composite freshness boundary configuration",
        max_config_cu,
        CUSTODY_CU_LIMIT,
    );
    assert_cu_within(
        "composite freshness boundary crank",
        max_crank_cu,
        CRANK_CU_LIMIT,
    );
    println!(
        "composite freshness matrix: {} provider words, config max {} CU, crank max {} CU",
        cases.len(),
        max_config_cu,
        max_crank_cu
    );
}

#[test]
fn v16_program_composite_epochs_gate_real_liquidation_across_provider_roles() {
    const INITIAL_PRICES_E6: [u64; 3] = [6_000_000, 2_000_000, 3_000_000];
    const ADVERSE_PRICES_E6: [u64; 3] = [12_000_000, 2_000_000, 3_000_000];
    const INITIAL_PRICE_E6: u64 = 1_000_000;
    const TARGET_PRICE_E6: u64 = 2_000_000;
    const MAX_STEPS: u64 = 40;
    const PROVIDER_ORDERS: [[EpochMatrixProvider; 3]; 3] = [
        [
            EpochMatrixProvider::Pyth,
            EpochMatrixProvider::Switchboard,
            EpochMatrixProvider::Chainlink,
        ],
        [
            EpochMatrixProvider::Switchboard,
            EpochMatrixProvider::Chainlink,
            EpochMatrixProvider::Pyth,
        ],
        [
            EpochMatrixProvider::Chainlink,
            EpochMatrixProvider::Pyth,
            EpochMatrixProvider::Switchboard,
        ],
    ];

    let mut max_oracle_cu = 0u64;
    let mut max_liquidation_cu = 0u64;
    for (world, providers) in PROVIDER_ORDERS.into_iter().enumerate() {
        let mut env = V16CuEnv::new_with_init_params(production_risk_params());
        env.update_liquidation_fee_policy_with_cu(5_000);
        set_test_clock(&mut env, 1, 100);
        let legs: Vec<EpochMatrixLeg> = (0..3)
            .map(|leg_index| {
                new_epoch_matrix_leg(
                    &mut env,
                    providers[leg_index],
                    20_000 + world,
                    leg_index,
                    INITIAL_PRICES_E6[leg_index],
                    100,
                    1,
                )
            })
            .collect();
        let oracle_accounts: Vec<Pubkey> = legs.iter().map(|leg| leg.account).collect();
        let mut feeds = [[0u8; 32]; 3];
        for (index, leg) in legs.iter().enumerate() {
            feeds[index] = leg.feed;
        }
        env.try_configure_hybrid_asset_with_conf_filter_cu(
            0,
            3,
            ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3,
            feeds,
            &oracle_accounts,
            1,
            100,
            0,
            0,
            1_000,
            100,
        )
        .unwrap_or_else(|error| panic!("liquidation world {world} config: {error}"));
        assert_eq!(
            env.market_state().0.oracle_target_price_e6,
            INITIAL_PRICE_E6
        );

        let long_owner = Keypair::new();
        let short_owner = Keypair::new();
        let cranker_owner = Keypair::new();
        let long = env.create_portfolio(&long_owner);
        let short = env.create_portfolio(&short_owner);
        let cranker = env.create_portfolio(&cranker_owner);
        env.deposit(&long_owner, long, 100_000_000);
        env.deposit(&short_owner, short, 100_000);
        env.deposit(&cranker_owner, cranker, 1_000);
        env.trade_asset_with_cu(
            0,
            &long_owner,
            long,
            &short_owner,
            short,
            POS_SCALE as i128,
            INITIAL_PRICE_E6,
            0,
        );

        let vault_before = env.token_amount(env.vault);
        let mut reached_liquidation = false;
        for step in 1..=MAX_STEPS {
            let slot = step + 1;
            let publish_time = 100 + step as i64;
            set_test_clock(&mut env, slot, publish_time);

            let skew_index = step as usize % legs.len();
            write_epoch_matrix_leg(
                &mut env,
                legs[skew_index],
                ADVERSE_PRICES_E6[skew_index],
                publish_time,
                slot,
            );
            let market_before = env.svm.get_account(&env.market).unwrap();
            let cranker_before = env.svm.get_account(&cranker).unwrap();
            let skew_error = try_epoch_matrix_crank(&mut env, cranker, slot, &oracle_accounts)
                .expect_err("a selected-leg epoch skew must reject before liquidation");
            assert!(
                skew_error.contains("Custom(27)"),
                "liquidation world {world} step {step} returned wrong skew error: {skew_error}"
            );
            assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
            assert_eq!(env.svm.get_account(&cranker).unwrap(), cranker_before);

            for (index, leg) in legs.iter().enumerate() {
                write_epoch_matrix_leg(
                    &mut env,
                    *leg,
                    ADVERSE_PRICES_E6[index],
                    publish_time,
                    slot,
                );
            }
            let oracle_cu = try_epoch_matrix_crank(&mut env, cranker, slot, &oracle_accounts)
                .unwrap_or_else(|error| {
                    panic!("liquidation world {world} coherent step {step}: {error}")
                });
            max_oracle_cu = max_oracle_cu.max(oracle_cu);
            let _ = env.send_crank_if_actionable(
                ProgInstruction::PermissionlessCrank {
                    now_slot: slot,
                    observations: Vec::new(),
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(short, false),
                ],
                &[],
            );
            if health_cert(&env.portfolio_state(short)).certified_liq_deficit != 0 {
                reached_liquidation = true;
                break;
            }
        }
        assert!(
            reached_liquidation,
            "coherent adverse composite never made world {world} genuinely liquidatable"
        );
        assert_eq!(env.market_state().0.oracle_target_price_e6, TARGET_PRICE_E6);

        let cranker_capital_before = env.portfolio_state(cranker).capital.get();
        let (_, group_before) = env.market_state();
        let liquidation_cu = env
            .send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: env.svm.get_sysvar::<Clock>().slot,
                    observations: Vec::new(),
                },
                vec![
                    AccountMeta::new(cranker_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(short, false),
                    AccountMeta::new(cranker, false),
                ],
                &[&cranker_owner],
            )
            .unwrap_or_else(|error| panic!("liquidation world {world}: {error}"));
        max_liquidation_cu = max_liquidation_cu.max(liquidation_cu);
        let (_, group_after) = env.market_state();
        let cranker_reward = env
            .portfolio_state(cranker)
            .capital
            .get()
            .checked_sub(cranker_capital_before)
            .expect("liquidation reward cannot reduce cranker capital");
        assert!(cranker_reward > 0, "world {world} liquidation was vacuous");
        assert!(
            group_after.assets[0].oi_eff_short_q < group_before.assets[0].oi_eff_short_q,
            "world {world} liquidation must reduce short OI"
        );
        assert_eq!(
            health_cert(&env.portfolio_state(short)).certified_liq_deficit,
            0,
            "world {world} liquidation must restore current health"
        );
        assert_eq!(env.token_amount(env.vault), vault_before);
        assert_eq!(group_after.vault as u64, vault_before);

        let long_leg = active_leg_for_asset(&env.portfolio_state(long), 0);
        let short_leg = active_leg_for_asset(&env.portfolio_state(short), 0);
        let remaining_long = reference_current_epoch_effective_abs(&group_after, long_leg);
        let remaining_short = reference_current_epoch_effective_abs(&group_after, short_leg);
        assert_eq!(remaining_long, remaining_short);
        assert!(remaining_long > 0 && remaining_long <= i128::MAX as u128);
        env.trade_asset_with_cu(
            0,
            &long_owner,
            long,
            &short_owner,
            short,
            -(remaining_long as i128),
            group_after.assets[0].effective_price,
            0,
        );
        let (_, terminal_group) = env.market_state();
        assert_eq!(terminal_group.assets[0].oi_eff_long_q, 0);
        assert_eq!(terminal_group.assets[0].oi_eff_short_q, 0);
        assert!(!has_active_leg_for_asset(&env.portfolio_state(long), 0));
        assert!(!has_active_leg_for_asset(&env.portfolio_state(short), 0));
        assert_eq!(terminal_group.vault as u64, env.token_amount(env.vault));
    }
    assert_cu_within(
        "composite-provider liquidation oracle ingestion",
        max_oracle_cu,
        CRANK_CU_LIMIT,
    );
    assert_cu_within(
        "composite-provider selected liquidation",
        max_liquidation_cu,
        CRANK_CU_LIMIT,
    );
    println!(
        "composite liquidation lifecycle: {} provider-role worlds, oracle max {} CU, liquidation max {} CU",
        PROVIDER_ORDERS.len(),
        max_oracle_cu,
        max_liquidation_cu
    );
}

#[test]
fn v16_program_composite_profile_shutdown_restart_clears_old_provenance() {
    const INITIAL_PRICES_E6: [u64; 3] = [6_000_000, 2_000_000, 3_000_000];
    const PRICE_E6: u64 = 1_000_000;
    const SHUTDOWN_SLOT: u64 = 2;
    const FORCE_CLOSE_SLOT: u64 = 7;
    const RESTART_SLOT: u64 = 8;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        initial_price: PRICE_E6,
        ..V16CuMarketParams::default()
    });
    env.configure_permissionless_resolve_with_cu(100, 5);
    set_test_clock(&mut env, 1, 100);
    let providers = [
        EpochMatrixProvider::Pyth,
        EpochMatrixProvider::Switchboard,
        EpochMatrixProvider::Chainlink,
    ];
    let legs: Vec<EpochMatrixLeg> = (0..3)
        .map(|index| {
            new_epoch_matrix_leg(
                &mut env,
                providers[index],
                30_000,
                index,
                INITIAL_PRICES_E6[index],
                100,
                1,
            )
        })
        .collect();
    let oracle_accounts: Vec<Pubkey> = legs.iter().map(|leg| leg.account).collect();
    let mut feeds = [[0u8; 32]; 3];
    for (index, leg) in legs.iter().enumerate() {
        feeds[index] = leg.feed;
    }
    env.try_configure_hybrid_asset_with_conf_filter_cu(
        0,
        3,
        ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3,
        feeds,
        &oracle_accounts,
        1,
        100,
        0,
        0,
        100,
        100,
    )
    .expect("configure three-provider shutdown profile");

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let keeper_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    let keeper = env.create_portfolio(&keeper_owner);
    env.deposit(&long_owner, long, 2_000_000);
    env.deposit(&short_owner, short, 2_000_000);
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        PRICE_E6,
        0,
    );
    let old_market_id = env.asset_market_id(0);
    let vault_before = env.token_amount(env.vault);

    set_test_clock(&mut env, SHUTDOWN_SLOT, 101);
    write_epoch_matrix_leg(&mut env, legs[0], 12_000_000, 101, SHUTDOWN_SLOT);
    let market_before_skew = env.svm.get_account(&env.market).unwrap();
    let keeper_before_skew = env.svm.get_account(&keeper).unwrap();
    let skew_error = try_epoch_matrix_crank(&mut env, keeper, SHUTDOWN_SLOT, &oracle_accounts)
        .expect_err("pre-shutdown composite skew must reject");
    assert!(skew_error.contains("Custom(27)"), "{skew_error}");
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before_skew
    );
    assert_eq!(env.svm.get_account(&keeper).unwrap(), keeper_before_skew);

    env.update_asset_lifecycle_as_admin_with_cu(
        processor::ASSET_ACTION_SHUTDOWN,
        0,
        SHUTDOWN_SLOT,
        0,
    );
    let shutdown_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, shutdown_group) = state::read_market(&shutdown_data).unwrap();
    let shutdown_profile = state::read_asset_oracle_profile(&shutdown_data, 0).unwrap();
    assert_eq!(
        shutdown_group.assets[0].lifecycle,
        AssetLifecycleV16::Recovery
    );
    assert_eq!(shutdown_group.assets[0].effective_price, PRICE_E6);
    assert_eq!(shutdown_profile.oracle_target_price_e6, PRICE_E6);
    assert_eq!(shutdown_profile.oracle_target_publish_time, 0);
    assert_eq!(shutdown_profile.last_good_oracle_slot, SHUTDOWN_SLOT);

    let recovery_before_tail = env.svm.get_account(&env.market).unwrap();
    let keeper_before_tail = env.svm.get_account(&keeper).unwrap();
    let _recovery_tail = try_epoch_matrix_crank(&mut env, keeper, SHUTDOWN_SLOT, &oracle_accounts)
        .expect_err("Recovery cannot consume the old composite profile");
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        recovery_before_tail
    );
    assert_eq!(env.svm.get_account(&keeper).unwrap(), keeper_before_tail);

    let admin = Keypair::from_bytes(&env.admin.to_bytes()).expect("clone market admin");
    set_test_clock(&mut env, 3, 102);
    let before_early_restart = env.svm.get_account(&env.market).unwrap();
    assert!(
        env.try_restart_asset_oracle_with_authority(&admin, 0, 3, PRICE_E6)
            .is_err(),
        "restart with live positions must reject"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        before_early_restart
    );

    let cranker = Keypair::new();
    set_test_clock(&mut env, FORCE_CLOSE_SLOT, 106);
    env.force_close_abandoned_asset_with_cu(&cranker, long, short, 0, FORCE_CLOSE_SLOT, POS_SCALE);
    assert!(!has_active_leg_for_asset(&env.portfolio_state(long), 0));
    assert!(!has_active_leg_for_asset(&env.portfolio_state(short), 0));

    set_test_clock(&mut env, RESTART_SLOT, 107);
    env.try_restart_asset_oracle_with_authority(&admin, 0, RESTART_SLOT, PRICE_E6)
        .expect("restart empty composite asset");
    let restarted_data = env.svm.get_account(&env.market).unwrap().data;
    let (_, restarted_group) = state::read_market(&restarted_data).unwrap();
    let restarted_profile = state::read_asset_oracle_profile(&restarted_data, 0).unwrap();
    assert_eq!(
        restarted_group.assets[0].lifecycle,
        AssetLifecycleV16::Active
    );
    assert_ne!(restarted_group.assets[0].market_id, old_market_id);
    assert_eq!(restarted_group.assets[0].effective_price, PRICE_E6);
    assert_eq!(
        restarted_profile.oracle_mode,
        percolator_prog::constants::ORACLE_MODE_MANUAL
    );
    assert_eq!(restarted_profile.oracle_leg_count, 0);
    assert_eq!(restarted_profile.oracle_leg_flags, 0);
    assert_eq!(restarted_profile.oracle_leg_feeds, [[0u8; 32]; 3]);
    assert_eq!(restarted_profile.oracle_leg_prices_e6, [0u64; 3]);
    assert_eq!(restarted_profile.oracle_leg_publish_times, [0i64; 3]);
    assert_eq!(restarted_profile.oracle_target_price_e6, PRICE_E6);
    assert_eq!(restarted_profile.oracle_target_publish_time, 0);

    let before_old_tail = env.svm.get_account(&env.market).unwrap();
    let keeper_before_old_tail = env.svm.get_account(&keeper).unwrap();
    let _old_tail = try_epoch_matrix_crank(&mut env, keeper, RESTART_SLOT, &oracle_accounts)
        .expect_err("old composite tail cannot attach to the restarted manual generation");
    assert_eq!(env.svm.get_account(&env.market).unwrap(), before_old_tail);
    assert_eq!(
        env.svm.get_account(&keeper).unwrap(),
        keeper_before_old_tail
    );

    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        PRICE_E6,
        0,
    );
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        -(POS_SCALE as i128),
        PRICE_E6,
        0,
    );
    let (_, terminal_group) = env.market_state();
    assert_eq!(terminal_group.assets[0].oi_eff_long_q, 0);
    assert_eq!(terminal_group.assets[0].oi_eff_short_q, 0);
    assert_eq!(terminal_group.vault as u64, vault_before);
    assert_eq!(env.token_amount(env.vault), vault_before);
}

#[derive(Clone, Copy, Debug)]
enum ProviderLifecycleScenario {
    DrainOnly,
    Recovery,
    Resolved,
}

#[test]
fn v16_program_each_provider_composes_through_every_value_bearing_lifecycle() {
    const PRICE_E6: u64 = 1_000_000;
    const UPDATED_PRICE_E6: u64 = 1_010_000;
    const PROVIDERS: [EpochMatrixProvider; 3] = [
        EpochMatrixProvider::Pyth,
        EpochMatrixProvider::Switchboard,
        EpochMatrixProvider::Chainlink,
    ];
    const SCENARIOS: [ProviderLifecycleScenario; 3] = [
        ProviderLifecycleScenario::DrainOnly,
        ProviderLifecycleScenario::Recovery,
        ProviderLifecycleScenario::Resolved,
    ];

    let mut worlds = 0usize;
    let mut max_oracle_cu = 0u64;
    for (provider_index, provider) in PROVIDERS.into_iter().enumerate() {
        for (scenario_index, scenario) in SCENARIOS.into_iter().enumerate() {
            let world = provider_index * SCENARIOS.len() + scenario_index;
            let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
                initial_price: PRICE_E6,
                ..V16CuMarketParams::default()
            });
            env.configure_permissionless_resolve_with_cu(100, 5);
            set_test_clock(&mut env, 1, 100);
            let leg = new_epoch_matrix_leg(&mut env, provider, 40_000 + world, 0, PRICE_E6, 100, 1);
            env.try_configure_hybrid_asset_with_conf_filter_cu(
                0,
                1,
                0,
                [leg.feed, [0; 32], [0; 32]],
                &[leg.account],
                1,
                100,
                0,
                0,
                3,
                100,
            )
            .unwrap_or_else(|error| {
                panic!("provider lifecycle world {world} failed configuration: {error}")
            });

            let long_owner = Keypair::new();
            let short_owner = Keypair::new();
            let keeper_owner = Keypair::new();
            let long = env.create_portfolio(&long_owner);
            let short = env.create_portfolio(&short_owner);
            let keeper = env.create_portfolio(&keeper_owner);
            env.deposit(&long_owner, long, 2_000_000);
            env.deposit(&short_owner, short, 2_000_000);
            env.trade_asset_with_cu(
                0,
                &long_owner,
                long,
                &short_owner,
                short,
                POS_SCALE as i128,
                PRICE_E6,
                0,
            );
            let vault_before = env.token_amount(env.vault);

            set_test_clock(&mut env, 2, 101);
            write_epoch_matrix_leg(&mut env, leg, UPDATED_PRICE_E6, 101, 2);
            let oracle_cu = try_epoch_matrix_crank(&mut env, keeper, 2, &[leg.account])
                .unwrap_or_else(|error| {
                    panic!("provider lifecycle world {world} failed observation: {error}")
                });
            max_oracle_cu = max_oracle_cu.max(oracle_cu);
            let active_profile = state::read_asset_oracle_profile(
                &env.svm.get_account(&env.market).unwrap().data,
                0,
            )
            .unwrap();
            assert_eq!(active_profile.oracle_target_price_e6, UPDATED_PRICE_E6);
            assert_eq!(active_profile.oracle_target_publish_time, 101);

            match scenario {
                ProviderLifecycleScenario::DrainOnly => {
                    set_test_clock(&mut env, 3, 102);
                    env.update_asset_lifecycle_as_admin_with_cu(
                        processor::ASSET_ACTION_DRAIN_ONLY,
                        0,
                        0,
                        0,
                    );
                    write_epoch_matrix_leg(&mut env, leg, UPDATED_PRICE_E6, 102, 3);
                    let drain_cu = try_epoch_matrix_crank(&mut env, keeper, 3, &[leg.account])
                        .unwrap_or_else(|error| {
                            panic!("provider lifecycle world {world} failed DrainOnly accrual: {error}")
                        });
                    max_oracle_cu = max_oracle_cu.max(drain_cu);
                    assert_eq!(
                        env.market_state().1.assets[0].lifecycle,
                        AssetLifecycleV16::DrainOnly
                    );
                    let exit_price = env.market_state().1.assets[0].effective_price;
                    env.trade_asset_with_cu(
                        0,
                        &long_owner,
                        long,
                        &short_owner,
                        short,
                        -(POS_SCALE as i128),
                        exit_price,
                        0,
                    );
                    for (owner, portfolio) in [(&long_owner, long), (&short_owner, short)] {
                        let capital = env.portfolio_state(portfolio).capital.get();
                        env.withdraw(owner, portfolio, capital);
                    }
                    let (_, terminal_group) = env.market_state();
                    assert_eq!(terminal_group.vault as u64, env.token_amount(env.vault));
                }
                ProviderLifecycleScenario::Recovery => {
                    set_test_clock(&mut env, 3, 102);
                    env.update_asset_lifecycle_as_admin_with_cu(
                        processor::ASSET_ACTION_SHUTDOWN,
                        0,
                        3,
                        0,
                    );
                    let recovery_before = env.svm.get_account(&env.market).unwrap();
                    write_epoch_matrix_leg(&mut env, leg, UPDATED_PRICE_E6 + 1, 102, 3);
                    let keeper_before = env.svm.get_account(&keeper).unwrap();
                    let error = try_epoch_matrix_crank(&mut env, keeper, 3, &[leg.account])
                        .expect_err("Recovery must not consume a retired provider profile");
                    assert!(
                        error.contains("Custom(21)")
                            || error.contains("Custom(22)")
                            || error.contains("Custom(19)"),
                        "provider lifecycle world {world} returned unexpected Recovery tail error: {error}"
                    );
                    assert_eq!(env.svm.get_account(&env.market).unwrap(), recovery_before);
                    assert_eq!(env.svm.get_account(&keeper).unwrap(), keeper_before);

                    set_test_clock(&mut env, 8, 107);
                    for _ in 0..8 {
                        if !has_active_leg_for_asset(&env.portfolio_state(long), 0)
                            && !has_active_leg_for_asset(&env.portfolio_state(short), 0)
                        {
                            break;
                        }
                        for (owner, portfolio) in [(&long_owner, long), (&short_owner, short)] {
                            if !has_active_leg_for_asset(&env.portfolio_state(portfolio), 0) {
                                continue;
                            }
                            let market_before = env.svm.get_account(&env.market).unwrap();
                            let portfolio_before = env.svm.get_account(&portfolio).unwrap();
                            env.forfeit_recovery_leg_with_cu(
                                owner,
                                portfolio,
                                0,
                                percolator::MAX_VAULT_TVL,
                            );
                            assert!(
                                env.svm.get_account(&env.market).unwrap() != market_before
                                    || env.svm.get_account(&portfolio).unwrap() != portfolio_before,
                                "provider lifecycle world {world} accepted a no-op Recovery continuation"
                            );
                        }
                    }
                    assert!(!has_active_leg_for_asset(&env.portfolio_state(long), 0));
                    assert!(!has_active_leg_for_asset(&env.portfolio_state(short), 0));
                    let old_generation = env.asset_market_id(0);
                    let admin = Keypair::from_bytes(&env.admin.to_bytes()).expect("clone admin");
                    set_test_clock(&mut env, 9, 108);
                    env.try_restart_asset_oracle_with_authority(&admin, 0, 9, PRICE_E6)
                        .unwrap_or_else(|error| {
                            panic!("provider lifecycle world {world} failed restart: {error}")
                        });
                    assert_ne!(env.asset_market_id(0), old_generation);
                    env.trade_asset_with_cu(
                        0,
                        &long_owner,
                        long,
                        &short_owner,
                        short,
                        POS_SCALE as i128,
                        PRICE_E6,
                        0,
                    );
                    env.trade_asset_with_cu(
                        0,
                        &long_owner,
                        long,
                        &short_owner,
                        short,
                        -(POS_SCALE as i128),
                        PRICE_E6,
                        0,
                    );
                    assert_eq!(env.token_amount(env.vault), vault_before);
                }
                ProviderLifecycleScenario::Resolved => {
                    env.resolve();
                    let (resolved_cfg, resolved_group) = env.market_state();
                    let permissionless_slot = resolved_group
                        .resolved_slot
                        .checked_add(resolved_cfg.force_close_delay_slots)
                        .expect("provider lifecycle permissionless slot overflow");
                    set_test_clock(
                        &mut env,
                        permissionless_slot,
                        101 + resolved_cfg.force_close_delay_slots as i64,
                    );
                    let resolved_profile = state::read_asset_oracle_profile(
                        &env.svm.get_account(&env.market).unwrap().data,
                        0,
                    )
                    .unwrap();
                    assert_eq!(resolved_profile.oracle_target_price_e6, UPDATED_PRICE_E6);
                    assert_eq!(resolved_profile.oracle_target_publish_time, 101);
                    let payouts = drain_resolved_cohort(
                        &mut env,
                        &[
                            (&long_owner, long),
                            (&short_owner, short),
                            (&keeper_owner, keeper),
                        ],
                        "provider lifecycle resolved payout",
                    );
                    let remaining_vault = env.token_amount(env.vault);
                    assert_eq!(
                        payouts.iter().sum::<u128>() + u128::from(remaining_vault),
                        u128::from(vault_before)
                    );
                    assert_eq!(env.market_state().1.vault as u64, remaining_vault);
                }
            }
            worlds += 1;
        }
    }

    assert_eq!(worlds, PROVIDERS.len() * SCENARIOS.len());
    assert_cu_within(
        "single-provider lifecycle oracle ingestion",
        max_oracle_cu,
        CRANK_CU_LIMIT,
    );
    println!("single-provider lifecycle Cartesian: {worlds} worlds, oracle max {max_oracle_cu} CU");
}

#[derive(Clone, Copy, Debug)]
struct CompositeLifecycleCase {
    providers: [EpochMatrixProvider; 3],
    count: u8,
    flags: u8,
    initial_prices_e6: [u64; 3],
    updated_prices_e6: [u64; 3],
}

fn composite_lifecycle_cases() -> [CompositeLifecycleCase; 6] {
    [
        CompositeLifecycleCase {
            providers: [
                EpochMatrixProvider::Pyth,
                EpochMatrixProvider::Switchboard,
                EpochMatrixProvider::Pyth,
            ],
            count: 2,
            flags: 0,
            initial_prices_e6: [500_000, 2_000_000, 0],
            updated_prices_e6: [505_000, 2_000_000, 0],
        },
        CompositeLifecycleCase {
            providers: [
                EpochMatrixProvider::Chainlink,
                EpochMatrixProvider::Pyth,
                EpochMatrixProvider::Pyth,
            ],
            count: 2,
            flags: ORACLE_LEG_FLAG_DIVIDE_LEG2,
            initial_prices_e6: [2_000_000, 2_000_000, 0],
            updated_prices_e6: [2_020_000, 2_000_000, 0],
        },
        CompositeLifecycleCase {
            providers: [
                EpochMatrixProvider::Pyth,
                EpochMatrixProvider::Switchboard,
                EpochMatrixProvider::Chainlink,
            ],
            count: 3,
            flags: 0,
            initial_prices_e6: [250_000, 2_000_000, 2_000_000],
            updated_prices_e6: [252_500, 2_000_000, 2_000_000],
        },
        CompositeLifecycleCase {
            providers: [
                EpochMatrixProvider::Switchboard,
                EpochMatrixProvider::Chainlink,
                EpochMatrixProvider::Pyth,
            ],
            count: 3,
            flags: ORACLE_LEG_FLAG_DIVIDE_LEG2,
            initial_prices_e6: [1_000_000, 2_000_000, 2_000_000],
            updated_prices_e6: [1_010_000, 2_000_000, 2_000_000],
        },
        CompositeLifecycleCase {
            providers: [
                EpochMatrixProvider::Chainlink,
                EpochMatrixProvider::Pyth,
                EpochMatrixProvider::Switchboard,
            ],
            count: 3,
            flags: ORACLE_LEG_FLAG_DIVIDE_LEG3,
            initial_prices_e6: [1_000_000, 2_000_000, 2_000_000],
            updated_prices_e6: [1_010_000, 2_000_000, 2_000_000],
        },
        CompositeLifecycleCase {
            providers: [
                EpochMatrixProvider::Pyth,
                EpochMatrixProvider::Chainlink,
                EpochMatrixProvider::Switchboard,
            ],
            count: 3,
            flags: ORACLE_LEG_FLAG_DIVIDE_LEG2 | ORACLE_LEG_FLAG_DIVIDE_LEG3,
            initial_prices_e6: [4_000_000, 2_000_000, 2_000_000],
            updated_prices_e6: [4_040_000, 2_000_000, 2_000_000],
        },
    ]
}

#[test]
fn v16_program_composite_provider_roles_cross_lifecycles_and_freshness_boundaries() {
    const INITIAL_TARGET_E6: u64 = 1_000_000;
    const UPDATED_TARGET_E6: u64 = 1_010_000;
    const SCENARIOS: [ProviderLifecycleScenario; 3] = [
        ProviderLifecycleScenario::DrainOnly,
        ProviderLifecycleScenario::Recovery,
        ProviderLifecycleScenario::Resolved,
    ];

    let cases = composite_lifecycle_cases();
    let mut worlds = 0usize;
    let mut malformed_rejections = 0usize;
    let mut stale_rejections = 0usize;
    let mut max_oracle_cu = 0u64;
    for (case_index, case) in cases.into_iter().enumerate() {
        for (scenario_index, scenario) in SCENARIOS.into_iter().enumerate() {
            let world = case_index * SCENARIOS.len() + scenario_index;
            let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
                initial_price: INITIAL_TARGET_E6,
                ..V16CuMarketParams::default()
            });
            env.configure_permissionless_resolve_with_cu(100, 5);
            set_test_clock(&mut env, 10, 100);
            let legs: Vec<EpochMatrixLeg> = (0..case.count as usize)
                .map(|leg_index| {
                    new_epoch_matrix_leg(
                        &mut env,
                        case.providers[leg_index],
                        50_000 + world,
                        leg_index,
                        case.initial_prices_e6[leg_index],
                        40,
                        10,
                    )
                })
                .collect();
            let mut feeds = [[0u8; 32]; 3];
            for (index, leg) in legs.iter().enumerate() {
                feeds[index] = leg.feed;
            }
            let oracle_accounts: Vec<Pubkey> = legs.iter().map(|leg| leg.account).collect();
            env.try_configure_hybrid_asset_with_conf_filter_cu(
                0,
                case.count,
                case.flags,
                feeds,
                &oracle_accounts,
                10,
                100,
                0,
                0,
                3,
                100,
            )
            .unwrap_or_else(|error| {
                panic!("composite lifecycle world {world} failed configuration: {error}")
            });
            assert_eq!(
                env.market_state().0.oracle_target_price_e6,
                INITIAL_TARGET_E6
            );

            let long_owner = Keypair::new();
            let short_owner = Keypair::new();
            let keeper_owner = Keypair::new();
            let long = env.create_portfolio(&long_owner);
            let short = env.create_portfolio(&short_owner);
            let keeper = env.create_portfolio(&keeper_owner);
            env.deposit(&long_owner, long, 2_000_000);
            env.deposit(&short_owner, short, 2_000_000);
            env.trade_asset_with_cu(
                0,
                &long_owner,
                long,
                &short_owner,
                short,
                POS_SCALE as i128,
                INITIAL_TARGET_E6,
                0,
            );
            let vault_before = env.token_amount(env.vault);

            set_test_clock(&mut env, 11, 160);
            for (leg_index, leg) in legs.iter().enumerate() {
                write_epoch_matrix_leg(&mut env, *leg, case.updated_prices_e6[leg_index], 100, 11);
            }
            let malformed_index = world % legs.len();
            let valid_provider = env
                .svm
                .get_account(&legs[malformed_index].account)
                .expect("composite provider account exists");
            let mut malformed_provider = valid_provider.clone();
            malformed_provider.data[0] ^= u8::MAX;
            env.svm
                .set_account(legs[malformed_index].account, malformed_provider)
                .unwrap();
            let market_before_malformed = env.svm.get_account(&env.market).unwrap();
            let long_before_malformed = env.svm.get_account(&long).unwrap();
            let short_before_malformed = env.svm.get_account(&short).unwrap();
            let keeper_before_malformed = env.svm.get_account(&keeper).unwrap();
            let vault_before_malformed = env.svm.get_account(&env.vault).unwrap();
            let malformed = try_epoch_matrix_crank(&mut env, keeper, 11, &oracle_accounts)
                .expect_err("a malformed selected composite provider must reject");
            assert!(
                malformed.contains("Custom(26)"),
                "composite lifecycle world {world} returned the wrong malformed-provider error: {malformed}"
            );
            assert_eq!(
                env.svm.get_account(&env.market).unwrap(),
                market_before_malformed
            );
            assert_eq!(env.svm.get_account(&long).unwrap(), long_before_malformed);
            assert_eq!(env.svm.get_account(&short).unwrap(), short_before_malformed);
            assert_eq!(
                env.svm.get_account(&keeper).unwrap(),
                keeper_before_malformed
            );
            assert_eq!(
                env.svm.get_account(&env.vault).unwrap(),
                vault_before_malformed
            );
            env.svm
                .set_account(legs[malformed_index].account, valid_provider)
                .unwrap();
            malformed_rejections += 1;

            let exact_expiry_cu = try_epoch_matrix_crank(&mut env, keeper, 11, &oracle_accounts)
                .unwrap_or_else(|error| {
                    panic!("composite lifecycle world {world} rejected exact freshness: {error}")
                });
            max_oracle_cu = max_oracle_cu.max(exact_expiry_cu);
            let exact_profile = env.market_state().0;
            assert_eq!(exact_profile.oracle_target_price_e6, UPDATED_TARGET_E6);
            assert_eq!(exact_profile.oracle_target_publish_time, 100);

            set_test_clock(&mut env, 12, 161);
            let market_before_stale = env.svm.get_account(&env.market).unwrap();
            let keeper_before_stale = env.svm.get_account(&keeper).unwrap();
            let stale = try_epoch_matrix_crank(&mut env, keeper, 12, &oracle_accounts)
                .expect_err("a composite report one second beyond freshness must reject");
            assert!(
                stale.contains("Custom(27)"),
                "composite lifecycle world {world} returned the wrong stale error: {stale}"
            );
            assert_eq!(
                env.svm.get_account(&env.market).unwrap(),
                market_before_stale
            );
            assert_eq!(env.svm.get_account(&keeper).unwrap(), keeper_before_stale);
            stale_rejections += 1;

            for (leg_index, leg) in legs.iter().enumerate() {
                write_epoch_matrix_leg(&mut env, *leg, case.updated_prices_e6[leg_index], 101, 12);
            }
            let refreshed_cu = try_epoch_matrix_crank(&mut env, keeper, 12, &oracle_accounts)
                .unwrap_or_else(|error| {
                    panic!("composite lifecycle world {world} failed fresh retry: {error}")
                });
            max_oracle_cu = max_oracle_cu.max(refreshed_cu);
            let refreshed_profile = env.market_state().0;
            assert_eq!(refreshed_profile.oracle_target_price_e6, UPDATED_TARGET_E6);
            assert_eq!(refreshed_profile.oracle_target_publish_time, 101);

            match scenario {
                ProviderLifecycleScenario::DrainOnly => {
                    set_test_clock(&mut env, 13, 162);
                    env.update_asset_lifecycle_as_admin_with_cu(
                        processor::ASSET_ACTION_DRAIN_ONLY,
                        0,
                        0,
                        0,
                    );
                    for (leg_index, leg) in legs.iter().enumerate() {
                        write_epoch_matrix_leg(
                            &mut env,
                            *leg,
                            case.updated_prices_e6[leg_index],
                            102,
                            13,
                        );
                    }
                    let drain_cu = try_epoch_matrix_crank(&mut env, keeper, 13, &oracle_accounts)
                        .unwrap_or_else(|error| {
                            panic!("composite lifecycle world {world} failed DrainOnly accrual: {error}")
                        });
                    max_oracle_cu = max_oracle_cu.max(drain_cu);
                    let exit_price = env.market_state().1.assets[0].effective_price;
                    env.trade_asset_with_cu(
                        0,
                        &long_owner,
                        long,
                        &short_owner,
                        short,
                        -(POS_SCALE as i128),
                        exit_price,
                        0,
                    );
                    let mut withdrawn = 0u128;
                    for (owner, portfolio) in [(&long_owner, long), (&short_owner, short)] {
                        let capital = env.portfolio_state(portfolio).capital.get();
                        withdrawn += capital;
                        env.withdraw(owner, portfolio, capital);
                    }
                    let remaining_vault = env.token_amount(env.vault);
                    let (_, group) = env.market_state();
                    assert_eq!(
                        withdrawn + u128::from(remaining_vault),
                        u128::from(vault_before)
                    );
                    assert_eq!(group.vault as u64, remaining_vault);
                    assert_eq!(group.c_tot, 0);
                }
                ProviderLifecycleScenario::Recovery => {
                    set_test_clock(&mut env, 13, 162);
                    env.update_asset_lifecycle_as_admin_with_cu(
                        processor::ASSET_ACTION_SHUTDOWN,
                        0,
                        13,
                        0,
                    );
                    let recovery_before = env.svm.get_account(&env.market).unwrap();
                    let keeper_before = env.svm.get_account(&keeper).unwrap();
                    let error = try_epoch_matrix_crank(&mut env, keeper, 13, &oracle_accounts)
                        .expect_err("Recovery must not consume a retired composite profile");
                    assert!(
                        error.contains("Custom(21)")
                            || error.contains("Custom(22)")
                            || error.contains("Custom(19)"),
                        "composite lifecycle world {world} returned unexpected Recovery tail error: {error}"
                    );
                    assert_eq!(env.svm.get_account(&env.market).unwrap(), recovery_before);
                    assert_eq!(env.svm.get_account(&keeper).unwrap(), keeper_before);

                    set_test_clock(&mut env, 18, 167);
                    for _ in 0..8 {
                        if !has_active_leg_for_asset(&env.portfolio_state(long), 0)
                            && !has_active_leg_for_asset(&env.portfolio_state(short), 0)
                        {
                            break;
                        }
                        for (owner, portfolio) in [(&long_owner, long), (&short_owner, short)] {
                            if has_active_leg_for_asset(&env.portfolio_state(portfolio), 0) {
                                env.forfeit_recovery_leg_with_cu(
                                    owner,
                                    portfolio,
                                    0,
                                    percolator::MAX_VAULT_TVL,
                                );
                            }
                        }
                    }
                    assert!(!has_active_leg_for_asset(&env.portfolio_state(long), 0));
                    assert!(!has_active_leg_for_asset(&env.portfolio_state(short), 0));
                    let old_generation = env.asset_market_id(0);
                    let admin = Keypair::from_bytes(&env.admin.to_bytes()).expect("clone admin");
                    set_test_clock(&mut env, 19, 168);
                    env.try_restart_asset_oracle_with_authority(&admin, 0, 19, INITIAL_TARGET_E6)
                        .unwrap_or_else(|error| {
                            panic!("composite lifecycle world {world} failed restart: {error}")
                        });
                    assert_ne!(env.asset_market_id(0), old_generation);
                    env.trade_asset_with_cu(
                        0,
                        &long_owner,
                        long,
                        &short_owner,
                        short,
                        POS_SCALE as i128,
                        INITIAL_TARGET_E6,
                        0,
                    );
                    env.trade_asset_with_cu(
                        0,
                        &long_owner,
                        long,
                        &short_owner,
                        short,
                        -(POS_SCALE as i128),
                        INITIAL_TARGET_E6,
                        0,
                    );
                    let mut withdrawn = 0u128;
                    for (owner, portfolio) in [(&long_owner, long), (&short_owner, short)] {
                        let capital = env.portfolio_state(portfolio).capital.get();
                        withdrawn += capital;
                        env.withdraw(owner, portfolio, capital);
                    }
                    let remaining_vault = env.token_amount(env.vault);
                    let (_, group) = env.market_state();
                    assert_eq!(
                        withdrawn + u128::from(remaining_vault),
                        u128::from(vault_before)
                    );
                    assert_eq!(group.vault as u64, remaining_vault);
                    assert_eq!(group.c_tot, 0);
                }
                ProviderLifecycleScenario::Resolved => {
                    env.resolve();
                    let (resolved_cfg, resolved_group) = env.market_state();
                    let permissionless_slot = resolved_group
                        .resolved_slot
                        .checked_add(resolved_cfg.force_close_delay_slots)
                        .expect("composite lifecycle permissionless slot overflow");
                    set_test_clock(
                        &mut env,
                        permissionless_slot,
                        161 + resolved_cfg.force_close_delay_slots as i64,
                    );
                    let payouts = drain_resolved_cohort(
                        &mut env,
                        &[
                            (&long_owner, long),
                            (&short_owner, short),
                            (&keeper_owner, keeper),
                        ],
                        "composite lifecycle resolved payout",
                    );
                    let remaining_vault = env.token_amount(env.vault);
                    let (_, group) = env.market_state();
                    assert_eq!(
                        payouts.iter().sum::<u128>() + u128::from(remaining_vault),
                        u128::from(vault_before)
                    );
                    assert_eq!(group.vault as u64, remaining_vault);
                    assert_eq!(group.c_tot, 0);
                }
            }
            worlds += 1;
        }
    }

    assert_eq!(worlds, composite_lifecycle_cases().len() * SCENARIOS.len());
    assert_eq!(malformed_rejections, worlds);
    assert_eq!(stale_rejections, worlds);
    assert_cu_within(
        "composite provider-role lifecycle oracle ingestion",
        max_oracle_cu,
        CRANK_CU_LIMIT,
    );
    println!(
        "composite provider-role lifecycle matrix: {worlds} worlds, oracle max {max_oracle_cu} CU"
    );
}
