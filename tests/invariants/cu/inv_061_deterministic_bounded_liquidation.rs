//! INV-061 - Deterministic, bounded liquidation.
//!
//! Normative obligation: Liquidation is deterministic, risk reducing, OI coherent, and bounded at maximum shape.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_program_reset_carry_liquidation_matrix_discovers_crank_lock`, `v16_attack_liquidation_reward_share_without_tail_still_progresses`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_program_reset_carry_liquidation_matrix_discovers_crank_lock() {
    let mut params = V16CuMarketParams::default();
    params.initial_price = 1;
    params.max_price_move_bps_per_slot = 10_000;
    let mut env = V16CuEnv::new_with_init_params(params);
    env.configure_auth_mark_with_cu(0, 1);

    let l1o = Keypair::new();
    let l2o = Keypair::new();
    let l3o = Keypair::new();
    let l4o = Keypair::new();
    let l5o = Keypair::new();
    let s1o = Keypair::new();
    let s2o = Keypair::new();
    let s3o = Keypair::new();
    let l1 = env.create_portfolio(&l1o);
    let l2 = env.create_portfolio(&l2o);
    let l3 = env.create_portfolio(&l3o);
    let l4 = env.create_portfolio(&l4o);
    let l5 = env.create_portfolio(&l5o);
    let s1 = env.create_portfolio(&s1o);
    let s2 = env.create_portfolio(&s2o);
    let s3 = env.create_portfolio(&s3o);

    for (owner, portfolio, deposit) in [
        (&l1o, l1, 1_000),
        (&l2o, l2, 1_000),
        (&l3o, l3, 1_000),
        (&l4o, l4, 1_000),
        (&l5o, l5, 1_000),
        (&s1o, s1, 2),
        (&s2o, s2, 5),
        (&s3o, s3, 1_000),
    ] {
        env.deposit(owner, portfolio, deposit);
    }
    for (long_owner, long, short_owner, short, quantity) in [
        (&l1o, l1, &s1o, s1, 1_897_305),
        (&l2o, l2, &s1o, s1, 102_695),
        (&l2o, l2, &s2o, s2, 666_301),
        (&l3o, l3, &s2o, s2, 65_831),
        (&l4o, l4, &s2o, s2, 430_043),
        (&l4o, l4, &s3o, s3, 1_130_061),
        (&l5o, l5, &s3o, s3, 767_244),
    ] {
        env.trade_asset_with_cu(0, long_owner, long, short_owner, short, quantity, 1, 0);
    }

    for (slot, mark) in [(1u64, 2u64), (2, 3), (3, 4), (4, 5), (5, 6)] {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_with_cu(slot, mark);
        env.crank(
            s3,
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
        );
    }
    for _ in 0..4 {
        if !has_active_leg_for_asset(&env.portfolio_state(s1), 0) {
            break;
        }
        env.crank(
            s1,
            ProgInstruction::PermissionlessCrank {
                now_slot: 5,
                observations: crank_observations(0),
            },
        );
    }
    let first = env.market_state().1;
    assert!(!has_active_leg_for_asset(&env.portfolio_state(s1), 0));
    assert_eq!(first.mode, MarketModeV16::Live);
    assert_eq!(first.assets[0].social_loss_remainder_long_num, 322_760);
    assert_ne!(first.assets[0].b_long_num, 0);

    for _ in 0..8 {
        let leg = active_leg_for_asset(&env.portfolio_state(l1), 0);
        if !leg.b_stale && leg.b_snap == env.market_state().1.assets[0].b_long_num {
            break;
        }
        env.crank(
            l1,
            ProgInstruction::PermissionlessCrank {
                now_slot: 5,
                observations: crank_observations(0),
            },
        );
    }
    assert_eq!(
        active_leg_for_asset(&env.portfolio_state(l1), 0).b_rem,
        percolator::SOCIAL_LOSS_DEN - 121_035
    );
    env.rebalance_reduce_with_cu(&l1o, l1, 0, 1_897_305);
    assert!(!has_active_leg_for_asset(&env.portfolio_state(l1), 0));
    let carry_state = env.market_state().1;
    assert_eq!(carry_state.assets[0].oi_eff_long_q, 1_162_175);
    assert_eq!(carry_state.assets[0].oi_eff_short_q, 1_162_175);
    assert_eq!(
        carry_state.assets[0].social_loss_dust_long_num,
        percolator::SOCIAL_LOSS_DEN - 121_035
    );

    env.svm.warp_to_slot(6);
    env.push_auth_mark_with_cu(6, 7);
    env.crank(
        s2,
        ProgInstruction::PermissionlessCrank {
            now_slot: 6,
            observations: crank_observations(0),
        },
    );
    assert!(has_active_leg_for_asset(&env.portfolio_state(s2), 0));
    assert!(health_cert(&env.portfolio_state(s2)).certified_liq_deficit != 0);

    let fixed_market = env.svm.get_account(&env.market).unwrap();
    let fixed_loser = env.svm.get_account(&s2).unwrap();
    let fixed_vault = env.svm.get_account(&env.vault).unwrap();
    for slot in 6..=8u64 {
        env.svm.warp_to_slot(slot);
        env.svm.expire_blockhash();
        let liquidation = env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(s2, false),
            ],
            &[],
        );
        assert!(liquidation.is_err(), "reset carry unexpectedly progressed");
        assert_eq!(env.svm.get_account(&env.market).unwrap(), fixed_market);
        assert_eq!(env.svm.get_account(&s2).unwrap(), fixed_loser);
        assert_eq!(env.svm.get_account(&env.vault).unwrap(), fixed_vault);
    }
    assert_eq!(env.market_state().1.mode, MarketModeV16::Live);
    assert!(has_active_leg_for_asset(&env.portfolio_state(s2), 0));
    assert_eq!(env.market_state().1.assets[0].oi_eff_long_q, 1_162_175);
    assert_eq!(env.market_state().1.assets[0].oi_eff_short_q, 1_162_175);
}

#[test]
fn v16_attack_liquidation_reward_share_without_tail_still_progresses() {
    const LIQ_SLOT: u64 = 30;

    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.update_liquidation_fee_policy_with_cu(5_000);
    env.configure_auth_mark_with_cu(0, 1_000_000);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let keeper = Keypair::new();
    env.ensure_signer_account(keeper.pubkey());
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 100_000_000);
    env.deposit(&short_owner, short, 100_000);
    env.trade_asset_with_cu(
        0,
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        1_000_000,
        0,
    );

    for slot in 1..=LIQ_SLOT {
        env.svm.warp_to_slot(slot);
        env.push_auth_mark_with_cu(slot, 2_000_000);
        env.svm.expire_blockhash();
        let _ = env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: slot,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(short, false),
            ],
            &[],
        );
    }
    assert!(
        health_cert(&env.portfolio_state(short)).certified_liq_deficit != 0,
        "setup must make the target liquidatable before the no-tail reward-share crank"
    );

    let (_, before) = env.market_state();
    env.svm.expire_blockhash();
    let accepted = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: LIQ_SLOT,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(keeper.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short, false),
        ],
        &[&keeper],
    );
    assert!(
        accepted.is_ok(),
        "reward-enabled liquidation must remain live when the keeper omits the optional reward tail: {accepted:?}"
    );

    let (_, after) = env.market_state();
    assert!(
        after.insurance > before.insurance,
        "without a reward tail, the liquidation fee is retained by insurance"
    );
    assert_eq!(
        after.vault, before.vault,
        "liquidation reward sharing is an internal fee split, not a vault mint"
    );
    assert_eq!(
        after.vault as u64,
        env.token_amount(env.vault),
        "vault accounting remains tied to SPL custody"
    );
    assert!(
        after.vault >= after.c_tot + after.insurance,
        "senior conservation"
    );
}
