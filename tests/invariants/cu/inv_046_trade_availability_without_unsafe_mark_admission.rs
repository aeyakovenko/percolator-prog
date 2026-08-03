//! INV-046 - Trade availability without unsafe mark admission.
//!
//! Normative obligation: Unsafe raw prices cannot poison state or remove every bounded user exit route.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_attack_nocpi_high_notional_ewma_exit_not_dosed_by_extreme_reported_price`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_attack_nocpi_high_notional_ewma_exit_not_dosed_by_extreme_reported_price() {
    const MARK: u64 = 1_000_000;
    const CAP_BPS: u64 = 50;
    const DEPOSIT: u128 = 4_900_000_000_000;
    const OPEN_Q: i128 = 4_800_000_000_000;
    const CLOSE_Q: i128 = -(POS_SCALE as i128);

    for path in [
        NoCpiReportedPricePath::Single,
        NoCpiReportedPricePath::Batch,
    ] {
        let mut env = V16CuEnv::new_with_init_params(V16CuMarketParams {
            initial_price: MARK,
            h_max: 20,
            max_trading_fee_bps: 37,
            max_price_move_bps_per_slot: CAP_BPS,
            max_accrual_dt_slots: 20,
            min_funding_lifetime_slots: 20,
            ..V16CuMarketParams::default()
        });
        env.svm.warp_to_slot(1);
        env.configure_ewma_mark_with_cu(1, MARK, 1, 0);
        env.svm.warp_to_slot(5);
        let (owner_a, account_a, owner_b, account_b) =
            funded_no_cpi_reported_price_pair(&mut env, DEPOSIT);

        try_no_cpi_reported_price_trade_with_cu(
            &mut env, path, &owner_a, account_a, &owner_b, account_b, OPEN_Q, MARK, 0,
        )
        .unwrap_or_else(|err| panic!("{path:?}: high-notional setup open failed: {err}"));
        let (_, opened_group) = env.market_state();
        assert_eq!(
            opened_group.assets[0].oi_eff_long_q,
            OPEN_Q.unsigned_abs(),
            "{path:?}: setup creates high-notional long OI"
        );
        assert_eq!(
            opened_group.assets[0].oi_eff_short_q,
            OPEN_Q.unsigned_abs(),
            "{path:?}: setup creates high-notional short OI"
        );
        assert!(
            opened_group.vault < percolator::MAX_VAULT_TVL,
            "{path:?}: setup stays public-reachable under the vault cap"
        );

        env.svm.expire_blockhash();
        let exit = try_no_cpi_reported_price_trade_with_cu(
            &mut env,
            path,
            &owner_a,
            account_a,
            &owner_b,
            account_b,
            CLOSE_Q,
            percolator::MAX_ORACLE_PRICE,
            0,
        );
        assert!(
            exit.is_ok(),
            "{path:?}: high-notional EWMA exit must not be DoSed by valid extreme reported price: {exit:?}"
        );
        let (_, after) = env.market_state();
        assert_eq!(
            after.assets[0].oi_eff_long_q,
            opened_group.assets[0].oi_eff_long_q - POS_SCALE,
            "{path:?}: high-notional exit reduces long OI"
        );
        assert_eq!(
            after.assets[0].oi_eff_short_q,
            opened_group.assets[0].oi_eff_short_q - POS_SCALE,
            "{path:?}: high-notional exit reduces short OI"
        );
        assert_eq!(
            after.vault as u64,
            env.token_amount(env.vault),
            "{path:?}: high-notional EWMA exit keeps vault accounting tied to SPL custody"
        );
        assert!(
            after.vault >= after.c_tot + after.insurance,
            "{path:?}: high-notional EWMA exit preserves senior conservation"
        );
    }
}
