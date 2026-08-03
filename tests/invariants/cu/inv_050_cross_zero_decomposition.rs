//! INV-050 - Cross-zero decomposition.
//!
//! Normative obligation: A cross-zero operation reduces only real exposure and subjects the new open to normal gates.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_attack_batch_nocpi_exit_only_rejects_cross_zero_flip`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_attack_batch_nocpi_exit_only_rejects_cross_zero_flip() {
    for lifecycle_case in ["DrainOnly", "Recovery"] {
        let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 5_000, 10_000, 1_000);
        if lifecycle_case == "Recovery" {
            env.configure_permissionless_resolve_with_cu(100, 50);
        }
        let long_owner = Keypair::new();
        let short_owner = Keypair::new();
        let long_account = env.create_portfolio(&long_owner);
        let short_account = env.create_portfolio(&short_owner);
        env.deposit(&long_owner, long_account, 1_000_000);
        env.deposit(&short_owner, short_account, 1_000_000);
        env.trade_asset_with_cu(
            0,
            &long_owner,
            long_account,
            &short_owner,
            short_account,
            POS_SCALE as i128,
            100,
            0,
        );

        match lifecycle_case {
            "DrainOnly" => {
                env.update_asset_lifecycle_as_admin_with_cu(
                    percolator_prog::processor::ASSET_ACTION_DRAIN_ONLY,
                    0,
                    0,
                    0,
                );
                assert_eq!(
                    env.market_state().1.assets[0].lifecycle,
                    AssetLifecycleV16::DrainOnly
                );
            }
            "Recovery" => {
                env.svm.warp_to_slot(10);
                env.update_asset_lifecycle_as_admin_with_cu(
                    percolator_prog::processor::ASSET_ACTION_SHUTDOWN,
                    0,
                    10,
                    0,
                );
                assert_eq!(
                    env.market_state().1.assets[0].lifecycle,
                    AssetLifecycleV16::Recovery
                );
            }
            _ => unreachable!(),
        }

        let market_before = env.svm.get_account(&env.market).unwrap();
        let long_before = env.svm.get_account(&long_account).unwrap();
        let short_before = env.svm.get_account(&short_account).unwrap();
        env.svm.expire_blockhash();
        let flip = env.send(
            ProgInstruction::BatchTradeNoCpi {
                legs: vec![BatchTradeLeg {
                    asset_index: 0,
                    size_q: -(2 * POS_SCALE as i128),
                    exec_price: 100,
                    fee_bps: 0,
                }],
            },
            vec![
                AccountMeta::new(long_owner.pubkey(), true),
                AccountMeta::new(short_owner.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(long_account, false),
                AccountMeta::new(short_account, false),
            ],
            &[&long_owner, &short_owner],
        );
        assert!(
            flip.is_err(),
            "{lifecycle_case} BatchTradeNoCpi must reject a cross-zero flip"
        );
        assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
        assert_eq!(env.svm.get_account(&long_account).unwrap(), long_before);
        assert_eq!(env.svm.get_account(&short_account).unwrap(), short_before);

        env.svm.expire_blockhash();
        let close_cu = env
            .send(
                ProgInstruction::BatchTradeNoCpi {
                    legs: vec![BatchTradeLeg {
                        asset_index: 0,
                        size_q: -(POS_SCALE as i128),
                        exec_price: 100,
                        fee_bps: 0,
                    }],
                },
                vec![
                    AccountMeta::new(long_owner.pubkey(), true),
                    AccountMeta::new(short_owner.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(long_account, false),
                    AccountMeta::new(short_account, false),
                ],
                &[&long_owner, &short_owner],
            )
            .expect("exact BatchTradeNoCpi lifecycle close must remain live");
        assert_cu_within(
            &format!("{lifecycle_case} BatchTradeNoCpi exact close"),
            close_cu,
            TRADE_CU_LIMIT,
        );
        let (_, group_after) = env.market_state();
        assert_eq!(group_after.assets[0].oi_eff_long_q, 0);
        assert_eq!(group_after.assets[0].oi_eff_short_q, 0);
        assert!(
            !has_active_leg_for_asset(&env.portfolio_state(long_account), 0),
            "{lifecycle_case} exact close leaves the long account flat"
        );
        assert!(
            !has_active_leg_for_asset(&env.portfolio_state(short_account), 0),
            "{lifecycle_case} exact close leaves the short account flat"
        );
        assert_eq!(group_after.vault as u64, env.token_amount(env.vault));
        assert!(group_after.vault >= group_after.c_tot + group_after.insurance);
    }
}
