//! INV-041 - Deterministic allocation and caller-order independence.
//!
//! Normative obligation: Caller ordering cannot change allocation, loss attribution, or economic outcome.
//!
//! Evidence in this file (I/C plus invariant-specific M assertions): `v16_attack_force_close_dust_chunking_is_value_path_independent`, `v16_attack_multi_observation_crank_order_cannot_change_economics`. These tests exercise the deployed public
//! wrapper with real SBF/LiteSVM account construction and assert economic state, token,
//! rollback, liveness, or compute outcomes appropriate to the invariant.
//!
//! Guarantee boundary: a quarantined counterexample demonstrates public reachability; it does
//! not certify the invariant on an unfixed pin. Certification requires the fixed-pin assertion
//! plus every additional verification method required by the charter.

use super::*;

#[test]
fn v16_attack_force_close_dust_chunking_is_value_path_independent() {
    fn run(
        chunks: &[u128],
        with_backing: bool,
    ) -> (
        (u128, i128, i128, u128, i128, i128, u128, u128, u128),
        (u128, u128, u128, u128),
    ) {
        const OPEN_PRICE: u64 = 101;
        const CLOSE_PRICE: u64 = 137;
        const SIZE_Q: u128 = POS_SCALE + 17;
        const SHUT_SLOT: u64 = 3;
        const CLOSE_SLOT: u64 = 5;

        let mut env = V16CuEnv::new_with_market_params_and_price_move(1, 10_000, 10_000, 10_000);
        env.configure_permissionless_resolve_with_cu(100, 1);
        env.configure_auth_mark_for_asset_as_admin(0, 1, OPEN_PRICE);
        if with_backing {
            env.top_up_backing_bucket(1, 1_000, 10);
        }

        let long_owner = Keypair::new();
        let short_owner = Keypair::new();
        let long = env.create_portfolio(&long_owner);
        let short = env.create_portfolio(&short_owner);
        env.deposit(&long_owner, long, 1_000_000);
        env.deposit(&short_owner, short, 1_000_000);
        env.trade_asset_with_cu(
            0,
            &long_owner,
            long,
            &short_owner,
            short,
            SIZE_Q as i128,
            OPEN_PRICE,
            0,
        );

        env.svm.warp_to_slot(2);
        env.push_auth_mark_for_asset_as_admin(0, 2, CLOSE_PRICE);
        env.crank_steps_after_market_catchup(
            long,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(0),
            },
            1,
        );
        env.crank(
            short,
            ProgInstruction::PermissionlessCrank {
                now_slot: 2,
                observations: crank_observations(0),
            },
        );
        assert_ne!(
            env.portfolio_state(long).pnl.get(),
            0,
            "setup must realize nonzero mark-to-market value"
        );

        env.svm.warp_to_slot(SHUT_SLOT);
        env.update_asset_lifecycle_as_admin_with_cu(
            processor::ASSET_ACTION_SHUTDOWN,
            0,
            SHUT_SLOT,
            0,
        );
        env.svm.warp_to_slot(CLOSE_SLOT);
        let cranker = Keypair::new();
        for &chunk in chunks {
            if !has_active_leg_for_asset(&env.portfolio_state(long), 0) {
                break;
            }
            env.force_close_abandoned_asset_with_cu(&cranker, long, short, 0, CLOSE_SLOT, chunk);
        }
        if has_active_leg_for_asset(&env.portfolio_state(long), 0) {
            env.force_close_abandoned_asset_with_cu(
                &cranker,
                long,
                short,
                0,
                CLOSE_SLOT,
                u128::MAX,
            );
        }

        let long_state = env.portfolio_state(long);
        let short_state = env.portfolio_state(short);
        let group = env.market_state().1;
        assert!(!has_active_leg_for_asset(&long_state, 0));
        assert!(!has_active_leg_for_asset(&short_state, 0));
        let source = group.source_credit[1];
        if with_backing {
            assert!(
                source.positive_claim_bound_num != 0,
                "backed setup must create a real source-credit claim"
            );
        }
        (
            (
                long_state.capital.get(),
                long_state.pnl.get(),
                long_state.fee_credits.get(),
                short_state.capital.get(),
                short_state.pnl.get(),
                short_state.fee_credits.get(),
                group.insurance,
                group.assets[0].oi_eff_long_q,
                group.assets[0].oi_eff_short_q,
            ),
            (
                source.positive_claim_bound_num,
                source.fresh_reserved_backing_num,
                source.provider_receivable_num,
                group.source_backing_buckets[1].fresh_unliened_backing_num,
            ),
        )
    }

    let one_shot = run(&[u128::MAX], false);
    let dust_chunked = run(
        &[
            1,
            POS_SCALE / 7,
            3,
            POS_SCALE / 5,
            11,
            POS_SCALE / 3,
            u128::MAX,
        ],
        false,
    );
    assert_eq!(
        dust_chunked, one_shot,
        "permissionless close_q chunking must not change either user's value or market accounting"
    );

    let backed_one_shot = run(&[u128::MAX], true);
    let backed_dust_chunked = run(
        &[
            1,
            POS_SCALE / 7,
            3,
            POS_SCALE / 5,
            11,
            POS_SCALE / 3,
            u128::MAX,
        ],
        true,
    );
    assert_eq!(
        backed_dust_chunked, backed_one_shot,
        "chunking must not alter source-credit or provider-backing value allocation"
    );
}

#[test]
fn v16_attack_multi_observation_crank_order_cannot_change_economics() {
    const MARK: u64 = 1_000_000;
    const OPEN_SLOT: u64 = 1;
    const CRANK_SLOT: u64 = 2;

    let run = |order: [u16; 2]| {
        let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
        env.configure_auth_mark_for_asset_as_admin(0, OPEN_SLOT, MARK);
        env.configure_auth_mark_for_asset_as_admin(1, OPEN_SLOT, MARK);

        let owner_a = Keypair::new();
        let owner_b = Keypair::new();
        let account_a = env.create_portfolio(&owner_a);
        let account_b = env.create_portfolio(&owner_b);
        env.deposit(&owner_a, account_a, 100_000_000);
        env.deposit(&owner_b, account_b, 100_000_000);
        env.trade_asset_with_cu(
            0,
            &owner_a,
            account_a,
            &owner_b,
            account_b,
            POS_SCALE as i128,
            MARK,
            0,
        );
        env.svm.expire_blockhash();
        env.trade_asset_with_cu(
            1,
            &owner_a,
            account_a,
            &owner_b,
            account_b,
            POS_SCALE as i128,
            MARK,
            0,
        );

        set_test_clock(&mut env, CRANK_SLOT, 101);
        env.push_auth_mark_for_asset_as_admin(0, CRANK_SLOT, MARK + 10_000);
        env.push_auth_mark_for_asset_as_admin(1, CRANK_SLOT, MARK - 20_000);
        let mut observations = Vec::new();
        for asset_index in order {
            observations.extend(crank_observations(asset_index));
        }
        env.crank(
            account_a,
            ProgInstruction::PermissionlessCrank {
                now_slot: CRANK_SLOT,
                observations,
            },
        );

        let mut group = env.market_state().1;
        group.market_group_id = [0; 32];
        let mut a = env.portfolio_state(account_a);
        a.provenance_header = Default::default();
        a.owner = [0; 32];
        let mut b = env.portfolio_state(account_b);
        b.provenance_header = Default::default();
        b.owner = [0; 32];
        (group, a, b, env.token_amount(env.vault))
    };

    let forward = run([0, 1]);
    let reverse = run([1, 0]);
    assert_eq!(
        forward, reverse,
        "caller-chosen observation order must not change market or user economics"
    );
}
