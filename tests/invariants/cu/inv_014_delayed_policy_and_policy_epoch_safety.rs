//! INV-014 - Delayed policy and policy-epoch safety.
//!
//! Normative obligation: an authorized control that lands after a newer control
//! cannot overwrite it. Independent policy scopes must not block one another,
//! and forward sequence gaps remain valid so retained transactions can land out
//! of order without requiring every intermediate sequence to execute.
//!
//! Evidence in this file uses the public SBF instruction boundary. The first
//! test covers strict monotonicity, gap acceptance, exact rollback, and lane
//! independence. The second covers cross-mode oracle supersession: EWMA,
//! authenticated mark, and hybrid configuration all consume one observation
//! lane, so switching instruction variants cannot revive stale consent.
//!
//! Guarantee boundary: these tests cover supersession within one live market
//! incarnation. Market recreation and authority A -> B -> A require persistent
//! incarnation identifiers and are tracked by INV-001 and INV-005.

use super::*;

fn send_admin_control(env: &mut V16CuEnv, instruction: ProgInstruction) -> Result<u64, String> {
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        instruction,
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
        ],
        &[&env.admin],
    )
}

#[test]
fn v16_control_sequences_accept_gaps_reject_replays_and_keep_lanes_independent() {
    let mut env = V16CuEnv::new();

    send_admin_control(
        &mut env,
        ProgInstruction::UpdateTradeFeePolicy {
            trade_fee_base_bps: 123,
            policy_sequence: 17,
        },
    )
    .expect("a forward sequence gap must be accepted");
    assert_eq!(env.control_sequences(0).trade_fee, 17);
    assert_eq!(env.market_state().0.trade_fee_base_bps, 123);

    for stale_sequence in [0, 16, 17] {
        let market_before = env.svm.get_account(&env.market).unwrap();
        env.svm.expire_blockhash();
        let result = send_admin_control(
            &mut env,
            ProgInstruction::UpdateTradeFeePolicy {
                trade_fee_base_bps: 999,
                policy_sequence: stale_sequence,
            },
        );
        assert!(
            result.is_err(),
            "sequence {stale_sequence} must not overwrite committed sequence 17"
        );
        assert_eq!(
            env.svm.get_account(&env.market).unwrap(),
            market_before,
            "a rejected stale control must roll the complete market account back"
        );
    }

    env.svm.expire_blockhash();
    send_admin_control(
        &mut env,
        ProgInstruction::UpdateLiquidationFeePolicy {
            cranker_share_bps: 1_000,
            policy_sequence: 1,
        },
    )
    .expect("an unrelated policy lane starts at its own sequence");
    let sequences = env.control_sequences(0);
    assert_eq!(sequences.trade_fee, 17);
    assert_eq!(sequences.liquidation_fee, 1);
    let (cfg, _) = env.market_state();
    assert_eq!(cfg.trade_fee_base_bps, 123);
    assert_eq!(cfg.liquidation_cranker_fee_share_bps, 1_000);

    env.svm.expire_blockhash();
    send_admin_control(
        &mut env,
        ProgInstruction::UpdateTradeFeePolicy {
            trade_fee_base_bps: 321,
            policy_sequence: 1_000_000,
        },
    )
    .expect("large forward gaps must remain valid for out-of-order landing");
    assert_eq!(env.control_sequences(0).trade_fee, 1_000_000);
    assert_eq!(env.market_state().0.trade_fee_base_bps, 321);
}

#[test]
fn v16_oracle_modes_share_one_supersession_sequence() {
    let mut env = V16CuEnv::new();
    set_test_clock(&mut env, 1, 100);

    send_admin_control(
        &mut env,
        ProgInstruction::ConfigureEwmaMark {
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: 100,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            observation_sequence: 2,
        },
    )
    .expect("new EWMA control");

    let ewma_before = env.svm.get_account(&env.market).unwrap();
    env.svm.expire_blockhash();
    let stale_auth = send_admin_control(
        &mut env,
        ProgInstruction::ConfigureAuthMark {
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: 200,
            observation_sequence: 1,
        },
    );
    assert!(
        stale_auth.is_err(),
        "a stale cross-mode control must reject"
    );
    assert_eq!(env.svm.get_account(&env.market).unwrap(), ewma_before);

    env.svm.expire_blockhash();
    send_admin_control(
        &mut env,
        ProgInstruction::ConfigureAuthMark {
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: 200,
            observation_sequence: 3,
        },
    )
    .expect("newer authenticated-mark control");

    let feed = [7u8; 32];
    let clock = env.svm.get_sysvar::<Clock>();
    let pyth = env.set_pyth_price(&feed, 300, 0, clock.unix_timestamp);
    let mut feeds = [[0u8; 32]; percolator_prog::constants::ORACLE_LEG_CAP];
    feeds[0] = feed;

    let auth_before = env.svm.get_account(&env.market).unwrap();
    env.svm.expire_blockhash();
    let stale_hybrid = send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureHybridOracle {
            asset_index: 0,
            now_slot: 1,
            now_unix_ts: clock.unix_timestamp,
            oracle_leg_count: 1,
            oracle_leg_flags: 0,
            max_staleness_secs: 60,
            hybrid_soft_stale_slots: 3,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            invert: 0,
            unit_scale: 0,
            conf_filter_bps: 500,
            oracle_leg_feeds: feeds,
            observation_sequence: 2,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new_readonly(pyth, false),
        ],
        &[&env.admin],
    );
    assert!(stale_hybrid.is_err(), "stale hybrid control must reject");
    assert_eq!(env.svm.get_account(&env.market).unwrap(), auth_before);

    env.svm.expire_blockhash();
    send_tx(
        &mut env.svm,
        env.program_id,
        &env.payer,
        ProgInstruction::ConfigureHybridOracle {
            asset_index: 0,
            now_slot: 1,
            now_unix_ts: clock.unix_timestamp,
            oracle_leg_count: 1,
            oracle_leg_flags: 0,
            max_staleness_secs: 60,
            hybrid_soft_stale_slots: 3,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            invert: 0,
            unit_scale: 0,
            conf_filter_bps: 500,
            oracle_leg_feeds: feeds,
            observation_sequence: 4,
        },
        vec![
            AccountMeta::new(env.admin.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new_readonly(pyth, false),
        ],
        &[&env.admin],
    )
    .expect("newer hybrid control");

    let hybrid_before = env.svm.get_account(&env.market).unwrap();
    env.svm.expire_blockhash();
    let stale_ewma = send_admin_control(
        &mut env,
        ProgInstruction::ConfigureEwmaMark {
            asset_index: 0,
            now_slot: 1,
            initial_mark_e6: 400,
            mark_ewma_halflife_slots: 1,
            mark_min_fee: 0,
            observation_sequence: 3,
        },
    );
    assert!(stale_ewma.is_err(), "stale EWMA control must reject");
    assert_eq!(env.svm.get_account(&env.market).unwrap(), hybrid_before);

    let (cfg, _) = env.market_state();
    assert_eq!(
        cfg.oracle_mode,
        percolator_prog::constants::ORACLE_MODE_HYBRID_AFTER_HOURS
    );
    assert_eq!(env.control_sequences(0).oracle_observation, 4);
}
