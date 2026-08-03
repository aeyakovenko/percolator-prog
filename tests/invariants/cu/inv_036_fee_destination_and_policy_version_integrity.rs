//! INV-036 - Fee destination and policy-version integrity.
//!
//! Normative obligation: Charged fees reach only the authorized destination under the bound policy version.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_attack_mixed_direction_batch_fees_conserve_by_asset`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_attack_mixed_direction_batch_fees_conserve_by_asset() {
    #[derive(Clone, Copy, Debug)]
    enum Path {
        NoCpi,
        Cpi,
    }

    for path in [Path::NoCpi, Path::Cpi] {
        let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 1_000, 1_000, 500);
        env.configure_auth_mark_for_asset_as_admin(0, 1, 100);
        env.configure_auth_mark_for_asset_as_admin(1, 1, 100);
        let taker = Keypair::new();
        let lp = Keypair::new();
        let taker_account = env.create_portfolio(&taker);
        let lp_account = env.create_portfolio(&lp);
        env.deposit(&taker, taker_account, 10_000_000);
        env.deposit(&lp, lp_account, 10_000_000);

        let before = env.market_state().1;
        let asset0_budget_before =
            before.insurance_domain_budget[0] + before.insurance_domain_budget[1];
        let asset1_budget_before =
            before.insurance_domain_budget[2] + before.insurance_domain_budget[3];
        let sz = (10 * POS_SCALE) as i128;

        env.svm.expire_blockhash();
        match path {
            Path::NoCpi => {
                env.send(
                    ProgInstruction::BatchTradeNoCpi {
                        legs: vec![
                            BatchTradeLeg {
                                asset_index: 0,
                                size_q: sz,
                                exec_price: 100,
                                fee_bps: 100,
                            },
                            BatchTradeLeg {
                                asset_index: 1,
                                size_q: -sz,
                                exec_price: 100,
                                fee_bps: 100,
                            },
                        ],
                    },
                    vec![
                        AccountMeta::new(taker.pubkey(), true),
                        AccountMeta::new(lp.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(taker_account, false),
                        AccountMeta::new(lp_account, false),
                    ],
                    &[&taker, &lp],
                )
                .unwrap_or_else(|err| panic!("{path:?} mixed-fee batch failed: {err}"));
            }
            Path::Cpi => {
                let matcher_program = Pubkey::new_unique();
                let matcher_bytes =
                    std::fs::read(auth_matcher_program_path()).expect("read auth matcher BPF");
                env.svm.add_program(matcher_program, &matcher_bytes);
                let (ctx, delegate, _) =
                    env.init_auth_matcher_context(matcher_program, &lp, lp_account);
                env.send(
                    ProgInstruction::BatchTradeCpi {
                        legs: vec![
                            BatchTradeCpiLeg {
                                asset_index: 0,
                                size_q: sz,
                                fee_bps: 100,
                                limit_price: 0,
                            },
                            BatchTradeCpiLeg {
                                asset_index: 1,
                                size_q: -sz,
                                fee_bps: 100,
                                limit_price: 0,
                            },
                        ],
                    },
                    vec![
                        AccountMeta::new(taker.pubkey(), true),
                        AccountMeta::new(env.market, false),
                        AccountMeta::new(taker_account, false),
                        AccountMeta::new(lp_account, false),
                        AccountMeta::new_readonly(matcher_program, false),
                        AccountMeta::new(ctx, false),
                        AccountMeta::new_readonly(delegate, false),
                    ],
                    &[&taker],
                )
                .unwrap_or_else(|err| panic!("{path:?} mixed-fee batch failed: {err}"));
            }
        }

        let after = env.market_state().1;
        let asset0_budget_delta = after.insurance_domain_budget[0]
            + after.insurance_domain_budget[1]
            - asset0_budget_before;
        let asset1_budget_delta = after.insurance_domain_budget[2]
            + after.insurance_domain_budget[3]
            - asset1_budget_before;
        let insurance_delta = after.insurance - before.insurance;
        assert!(insurance_delta > 0, "{path:?} must charge a nonzero fee");
        assert_eq!(
            asset0_budget_delta + asset1_budget_delta,
            insurance_delta,
            "{path:?} must budget every mixed-leg fee atom"
        );
        assert_eq!(
            asset0_budget_delta, asset1_budget_delta,
            "{path:?} same-size/same-fee mixed legs should credit equal per-asset fee budgets"
        );
        assert_eq!(after.vault, before.vault, "{path:?} must not move custody");
        assert_eq!(
            after.vault,
            after.c_tot + after.insurance,
            "{path:?} preserves senior conservation after mixed-fee batch"
        );
        assert_domain_budget_remaining_total_consistent(&after, "mixed-fee batch budgets");

        let taker_after = env.portfolio_state(taker_account);
        let lp_after = env.portfolio_state(lp_account);
        assert_eq!(active_leg_for_asset(&taker_after, 0).basis_pos_q, sz);
        assert_eq!(active_leg_for_asset(&taker_after, 1).basis_pos_q, -sz);
        assert_eq!(active_leg_for_asset(&lp_after, 0).basis_pos_q, -sz);
        assert_eq!(active_leg_for_asset(&lp_after, 1).basis_pos_q, sz);
    }
}
