//! INV-059 - Fee-fragmentation bound.
//!
//! Normative obligation: Splitting an execution or liquidation cannot multiply minimum or episode fees.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_attack_min_liquidation_fee_falls_back_to_full_close_progress`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_attack_min_liquidation_fee_falls_back_to_full_close_progress() {
    const PRICE: u64 = 100;
    const MIN_FEE: u128 = 10;

    let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
        min_nonzero_mm_req: 100,
        min_nonzero_im_req: 200,
        liquidation_fee_bps: 0,
        liquidation_fee_cap: MIN_FEE,
        min_liquidation_abs: MIN_FEE,
        max_price_move_bps_per_slot: 500,
        ..V16CuMarketParams::default()
    });
    env.configure_auth_mark_with_cu(0, PRICE);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    env.deposit(&long_owner, long, 1_000_000);
    env.deposit(&short_owner, short, 10 * PRICE as u128);
    env.trade_with_cu(
        &long_owner,
        long,
        &short_owner,
        short,
        (10 * POS_SCALE) as i128,
        PRICE,
        0,
    );

    // Same-slot target-only lag makes the short liquidatable without first changing its marked PnL.
    // A separate public crank commits the authenticated target while dt=0 keeps effective_price at
    // PRICE, matching the production out-of-order keeper flow.
    let staging_owner = Keypair::new();
    let staging = env.create_portfolio(&staging_owner);
    env.push_auth_mark_with_cu(0, PRICE * 2);
    env.crank(
        staging,
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: crank_observations(0),
        },
    );
    let (_, before_group) = env.market_state();
    let before_short = env.portfolio_state(short);
    assert_eq!(before_group.assets[0].effective_price, PRICE);
    assert_eq!(before_group.assets[0].raw_oracle_target_price, PRICE * 2);
    assert_eq!(before_short.pnl.get(), 0);
    assert!(
        health_cert(&before_short).cert_oracle_epoch < before_group.oracle_epoch,
        "target-only lag makes the victim certificate stale"
    );
    let insurance_before = before_group.insurance;

    env.svm.expire_blockhash();
    let refresh_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 0,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(short, false),
            ],
            &[],
        )
        .expect("the first auto-crank refreshes the target-lagged account");
    assert_cu_within(
        "minimum-fee liquidation pre-refresh",
        refresh_cu,
        CRANK_CU_LIMIT,
    );
    assert!(
        has_active_leg_for_asset(&env.portfolio_state(short), 0),
        "the first selected step is a refresh, not the liquidation under test"
    );

    env.svm.expire_blockhash();
    let liquidation_cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 0,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(short, false),
            ],
            &[],
        )
        .expect("minimum-fee liquidation must fall back to a full-close progress step");
    assert_cu_within(
        "minimum-fee full-close liquidation fallback",
        liquidation_cu,
        CRANK_CU_LIMIT,
    );

    let short_after = env.portfolio_state(short);
    let (_, after_group) = env.market_state();
    assert!(
        !has_active_leg_for_asset(&short_after, 0),
        "the inadmissible partial chunk falls back to closing the selected leg"
    );
    assert_eq!(
        short_after.capital.get(),
        10 * PRICE as u128 - MIN_FEE,
        "the configured full-close minimum fee is charged exactly once"
    );
    assert_eq!(
        after_group.insurance - insurance_before,
        MIN_FEE,
        "the collected minimum fee remains conserved in insurance"
    );
    assert_eq!(after_group.assets[0].oi_eff_long_q, 0);
    assert_eq!(after_group.assets[0].oi_eff_short_q, 0);
    assert_eq!(after_group.vault as u64, env.token_amount(env.vault));
}
