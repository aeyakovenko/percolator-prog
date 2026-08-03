//! INV-047 - Equivalent-route semantics.
//!
//! Normative obligation: Economically equivalent public routes have equivalent normalized state deltas.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_audit_empty_target_oracle_crank_matches_exposed_target_settlement`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_audit_empty_target_oracle_crank_matches_exposed_target_settlement() {
    fn run(commit_through_empty: bool) -> (u128, i128, u128, i128, [u64; 2], u128, u128) {
        const OPEN_PRICE: u64 = 1_000_000;
        const SETTLE_PRICE: u64 = 1_100_000;

        let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
        env.configure_auth_mark_with_cu(0, OPEN_PRICE);

        let long_owner = Keypair::new();
        let long = env.create_portfolio(&long_owner);
        let short_owner = Keypair::new();
        let short = env.create_portfolio(&short_owner);
        let empty_owner = Keypair::new();
        let empty = env.create_portfolio(&empty_owner);
        env.deposit(&long_owner, long, 2_000_000);
        env.deposit(&short_owner, short, 2_000_000);
        env.trade_asset_with_cu(
            0,
            &long_owner,
            long,
            &short_owner,
            short,
            POS_SCALE as i128,
            OPEN_PRICE,
            0,
        );

        env.svm.warp_to_slot(1);
        env.push_auth_mark_with_cu(1, SETTLE_PRICE);
        let first_target = if commit_through_empty { empty } else { long };
        env.svm.expire_blockhash();
        let first = env.send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 1,
                observations: crank_observations(0),
            },
            vec![
                AccountMeta::new(env.payer.pubkey(), true),
                AccountMeta::new(env.market, false),
                AccountMeta::new(first_target, false),
            ],
            &[],
        );
        assert!(
            first.is_ok(),
            "authenticated mark must commit through either valid target: {first:?}"
        );
        assert_eq!(
            env.market_state().1.assets[0].effective_price,
            SETTLE_PRICE,
            "the observation advances the exposed market even when the target portfolio is empty"
        );

        for target in [long, short] {
            env.svm.expire_blockhash();
            let refresh = env.send(
                ProgInstruction::PermissionlessCrank {
                    now_slot: 1,
                    observations: vec![],
                },
                vec![
                    AccountMeta::new(env.payer.pubkey(), true),
                    AccountMeta::new(env.market, false),
                    AccountMeta::new(target, false),
                ],
                &[],
            );
            assert!(
                refresh.is_ok(),
                "both exposed accounts retain a bounded no-observation refresh: {refresh:?}"
            );
        }

        let long_after = env.portfolio_state(long);
        let short_after = env.portfolio_state(short);
        assert_eq!(long_after.pnl.get(), 100_000);
        assert_eq!(long_after.capital.get(), 2_000_000);
        assert_eq!(short_after.pnl.get(), 0);
        assert_eq!(
            short_after.capital.get(),
            1_900_000,
            "the adverse PnL is crystallized from short principal during refresh"
        );

        env.trade_asset_with_cu(
            0,
            &long_owner,
            long,
            &short_owner,
            short,
            -(POS_SCALE as i128),
            SETTLE_PRICE,
            0,
        );
        env.resolve();
        let long_dest = env.close_resolved(&long_owner, long);
        let short_dest = env.close_resolved(&short_owner, short);
        let payouts = [env.token_amount(long_dest), env.token_amount(short_dest)];
        let (_, group) = env.market_state();
        (
            long_after.capital.get(),
            long_after.pnl.get(),
            short_after.capital.get(),
            short_after.pnl.get(),
            payouts,
            group.vault,
            group.c_tot,
        )
    }

    let exposed_target = run(false);
    let empty_target = run(true);
    assert_eq!(
        empty_target, exposed_target,
        "wrapper pre-accrual through an empty target must not change settlement or terminal value"
    );
    assert_eq!(empty_target.4, [2_100_000, 1_900_000]);
    assert_eq!(empty_target.5, 0);
    assert_eq!(empty_target.6, 0);
}
