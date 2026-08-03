//! INV-080 - Error propagation and exact rollback.
//!
//! Normative obligation: Every engine error aborts the instruction and commits no persistent bytes, tokens, or lamports.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_attack_hybrid_soft_stale_partial_oracle_error_does_not_poison_retry`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_attack_hybrid_soft_stale_partial_oracle_error_does_not_poison_retry() {
    let mut env = V16CuEnv::new();
    set_test_clock(&mut env, 1, 100);

    let feeds = [[0xc1u8; 32], [0xc2u8; 32], [0xc3u8; 32]];
    let initial_leg0 = env.set_pyth_price(&feeds[0], 4_000_000_000, -6, 100);
    let initial_leg1 = env.set_pyth_price(&feeds[1], 150_000_000, -6, 100);
    let initial_leg2 = env.set_pyth_price(&feeds[2], 200_000_000, -6, 100);
    env.configure_three_leg_hybrid_with_cu(feeds, initial_leg0, initial_leg1, initial_leg2, 1, 100);

    let keeper = Keypair::new();
    let keeper_portfolio = env.create_portfolio(&keeper);
    set_test_clock(&mut env, 2, 101);
    let fresh_leg0 = env.set_pyth_price(&feeds[0], 4_200_000_000, -6, 101);
    let fresh_leg1 = env.set_pyth_price(&feeds[1], 150_000_000, -6, 101);
    let fresh_leg2 = env.set_pyth_price(&feeds[2], 200_000_000, -6, 101);
    env.crank_with_oracle_tail(
        keeper_portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 2,
            observations: crank_observations(0),
        },
        &[fresh_leg0, fresh_leg1, fresh_leg2],
    );
    let (fresh_cfg, fresh_group) = env.market_state();
    assert_eq!(fresh_cfg.last_good_oracle_slot, 2);
    assert_eq!(fresh_group.assets[0].effective_price, 140_000);

    set_test_clock(&mut env, 6, 170);
    let mixed_fresh_leg0 = env.set_pyth_price(&feeds[0], 4_350_000_000, -6, 170);
    env.svm.expire_blockhash();
    let fallback = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 6,
            observations: crank_observations_with_accounts(0, 3),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(keeper_portfolio, false),
            AccountMeta::new_readonly(mixed_fresh_leg0, false),
            AccountMeta::new_readonly(fresh_leg1, false),
            AccountMeta::new_readonly(fresh_leg2, false),
        ],
        &[],
    );
    let fallback_cu = fallback.expect("soft-stale mixed oracle tail falls back without DoS");
    assert_cu_within(
        "HybridMark soft-stale mixed-tail fallback",
        fallback_cu,
        CRANK_CU_LIMIT,
    );
    let (fallback_cfg, fallback_group) = env.market_state();
    assert_eq!(
        fallback_cfg.last_good_oracle_slot, 2,
        "mixed-tail fallback must not claim a fresh external oracle slot"
    );
    assert_eq!(
        fallback_group.assets[0].effective_price, 140_000,
        "fallback uses the committed EWMA mark, not a partially composed external price"
    );

    set_test_clock(&mut env, 7, 171);
    let retry_leg0 = env.set_pyth_price(&feeds[0], 4_500_000_000, -6, 171);
    let retry_leg1 = env.set_pyth_price(&feeds[1], 150_000_000, -6, 171);
    let retry_leg2 = env.set_pyth_price(&feeds[2], 200_000_000, -6, 171);
    let retry_cu = env.crank_with_oracle_tail(
        keeper_portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 7,
            observations: crank_observations(0),
        },
        &[retry_leg0, retry_leg1, retry_leg2],
    );
    assert_cu_within(
        "HybridMark post-mixed-tail fresh retry",
        retry_cu,
        CRANK_CU_LIMIT,
    );
    let (retry_cfg, retry_group) = env.market_state();
    assert_eq!(retry_cfg.last_good_oracle_slot, 7);
    assert_eq!(retry_cfg.mark_ewma_last_slot, 7);
    assert_eq!(retry_cfg.mark_ewma_e6, 150_000);
    assert_eq!(retry_group.assets[0].effective_price, 150_000);
    assert_eq!(retry_group.assets[0].raw_oracle_target_price, 150_000);
}
