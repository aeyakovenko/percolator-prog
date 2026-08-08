//! INV-072 - order-robust crankability.
//!
//! Permissionless crank hints are discovery inputs. Reordering valid hints must not
//! change normalized progress, malformed hints must not mutate state or prevent
//! a later honest caller from discovering the canonical progressing action, and
//! out-of-order budgeted cranks must refresh without stealing rewards or starving
//! pending selected marks.

use super::*;

#[derive(Debug, PartialEq, Eq)]
struct CrankProgressSnapshot {
    current_slot: u64,
    oracle_epoch: u64,
    asset0_slot: u64,
    asset1_slot: u64,
    vault: u128,
    c_tot: u128,
    insurance: u128,
    portfolio_stale_state: u8,
}

fn two_asset_crank_world(order: &[u16]) -> CrankProgressSnapshot {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    env.activate_asset(1, 1, 100);
    let portfolio = env.create_portfolio(&owner);
    env.svm.warp_to_slot(4);
    let observations = order
        .iter()
        .copied()
        .map(|asset_index| CrankObservationHint {
            asset_index,
            oracle_accounts: 0,
        })
        .collect();
    env.crank_steps_after_market_catchup(
        portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 4,
            observations,
        },
        1,
    );
    let (_, group) = env.market_state();
    let portfolio = env.portfolio_state(portfolio);
    CrankProgressSnapshot {
        current_slot: group.current_slot,
        oracle_epoch: group.oracle_epoch,
        asset0_slot: group.assets[0].slot_last,
        asset1_slot: group.assets[1].slot_last,
        vault: group.vault,
        c_tot: group.c_tot,
        insurance: group.insurance,
        portfolio_stale_state: portfolio.stale_state,
    }
}

#[test]
fn v16_program_permissionless_crank_valid_hint_order_is_normalized() {
    assert_eq!(
        two_asset_crank_world(&[0, 1]),
        two_asset_crank_world(&[1, 0]),
        "valid hint order must not affect normalized crank progress",
    );
}

#[test]
fn v16_program_permissionless_crank_bad_hints_do_not_block_later_canonical_progress() {
    let mut env = V16CuEnv::new();
    let owner = Keypair::new();
    env.activate_asset(1, 1, 100);
    let portfolio = env.create_portfolio(&owner);
    env.svm.warp_to_slot(4);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&portfolio).unwrap();
    env.svm.expire_blockhash();
    let rejected = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 4,
            observations: vec![
                CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                },
                CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                },
            ],
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(portfolio, false),
        ],
        &[],
    );
    assert!(rejected.is_err(), "duplicate hint must reject");
    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&portfolio).unwrap(), portfolio_before);

    env.crank_steps_after_market_catchup(
        portfolio,
        ProgInstruction::PermissionlessCrank {
            now_slot: 4,
            observations: vec![
                CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                },
                CrankObservationHint {
                    asset_index: 1,
                    oracle_accounts: 0,
                },
            ],
        },
        1,
    );
    let (_, group) = env.market_state();
    assert_eq!(group.assets[0].slot_last, 4);
    assert_eq!(group.assets[1].slot_last, 4);
}

#[test]
fn v16_program_bad_hints_cannot_block_public_expired_close_recovery() {
    let PublicActiveCloseFixture { mut env, loss, .. } = public_asset1_bankrupt_close_fixture();
    let ledger = close_progress(&env.portfolio_state(loss));
    env.svm.warp_to_slot(ledger.max_close_slot + 1);

    let market_before = env.svm.get_account(&env.market).unwrap();
    let portfolio_before = env.svm.get_account(&loss).unwrap();
    let vault_before = env.svm.get_account(&env.vault).unwrap();

    env.svm.expire_blockhash();
    let bad_hints = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: 0,
            observations: vec![
                CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                },
                CrankObservationHint {
                    asset_index: 0,
                    oracle_accounts: 0,
                },
            ],
        },
        vec![
            AccountMeta::new_readonly(env.payer.pubkey(), false),
            AccountMeta::new(env.market, false),
            AccountMeta::new(loss, false),
        ],
        &[],
    );
    if bad_hints.is_ok() {
        assert!(
            matches!(
                env.market_state().1.mode,
                MarketModeV16::Recovery | MarketModeV16::Resolved
            ),
            "accepted adversarial hints must still make terminal progress"
        );
        return;
    }

    assert_eq!(env.svm.get_account(&env.market).unwrap(), market_before);
    assert_eq!(env.svm.get_account(&loss).unwrap(), portfolio_before);
    assert_eq!(env.svm.get_account(&env.vault).unwrap(), vault_before);

    env.svm.expire_blockhash();
    let cu = env
        .send(
            ProgInstruction::PermissionlessCrank {
                now_slot: 0,
                observations: vec![],
            },
            vec![
                AccountMeta::new_readonly(env.payer.pubkey(), false),
                AccountMeta::new(env.market, false),
                AccountMeta::new(loss, false),
            ],
            &[],
        )
        .expect("canonical expired-close crank remains live after bad hints");
    assert_cu_within(
        "INV-072 bad hints then canonical expired-close crank",
        cu,
        CRANK_CU_LIMIT,
    );
    assert!(
        matches!(
            env.market_state().1.mode,
            MarketModeV16::Recovery | MarketModeV16::Resolved
        ),
        "canonical crank must enter a terminal progress mode"
    );
}

// but it must not liquidate or pay a cranker reward for work it did not perform.
#[test]
fn v16_program_budgeted_out_of_order_crank_refreshes_without_unearned_reward() {
    const MARK: u64 = 1_000_000;
    const OPEN_SLOT: u64 = 1;
    const OBS_SLOT: u64 = 2;

    let mut env = V16CuEnv::new_with_init_params(production_risk_params());
    env.update_liquidation_fee_policy_with_cu(5_000);
    env.activate_asset(1, OPEN_SLOT, MARK);

    set_test_clock(&mut env, OPEN_SLOT, 100);
    let feed0 = [0x37u8; 32];
    let initial0 = env.set_pyth_price_with_conf(&feed0, MARK as i64, -6, 0, 100);
    env.try_configure_hybrid_asset_with_conf_filter_cu(
        0,
        1,
        0,
        [feed0, [0u8; 32], [0u8; 32]],
        &[initial0],
        OPEN_SLOT,
        100,
        0,
        0,
        10,
        0,
    )
    .expect("configure unrelated asset-0 hybrid oracle");
    env.configure_auth_mark_for_asset_as_admin(1, OPEN_SLOT, MARK);

    let long_owner = Keypair::new();
    let short_owner = Keypair::new();
    let cranker_owner = Keypair::new();
    let long = env.create_portfolio(&long_owner);
    let short = env.create_portfolio(&short_owner);
    let cranker = env.create_portfolio(&cranker_owner);
    env.deposit(&long_owner, long, 100_000_000);
    env.deposit(&short_owner, short, 100_000_000);
    env.deposit(&cranker_owner, cranker, 1_000);
    env.trade_asset_with_cu(
        1,
        &long_owner,
        long,
        &short_owner,
        short,
        POS_SCALE as i128,
        MARK,
        0,
    );

    set_test_clock(&mut env, OBS_SLOT, 101);
    env.push_auth_mark_for_asset_as_admin(1, OBS_SLOT, MARK + 10_000);
    env.crank(
        long,
        ProgInstruction::PermissionlessCrank {
            now_slot: OBS_SLOT,
            observations: crank_observations(1),
        },
    );
    let (_, group_after_asset1) = env.market_state();
    assert!(
        health_cert(&env.portfolio_state(short)).cert_oracle_epoch
            < group_after_asset1.oracle_epoch,
        "setup must leave the target account stale on its active asset"
    );

    let fresh0 = env.set_pyth_price_with_conf(&feed0, (MARK + 10_000) as i64, -6, 0, 101);
    let (_, group_before) = env.market_state();
    let short_before = env.portfolio_state(short);
    let short_oi_before = group_before.assets[1].oi_eff_short_q;
    let cranker_before = env.svm.get_account(&cranker).unwrap();

    env.svm.expire_blockhash();
    let refreshed = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: OBS_SLOT,
            observations: crank_observations_with_accounts(0, 1),
        },
        vec![
            AccountMeta::new(cranker_owner.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(short, false),
            AccountMeta::new_readonly(fresh0, false),
            AccountMeta::new(cranker, false),
        ],
        &[&cranker_owner],
    );
    assert!(
        refreshed.is_ok(),
        "budgeted stale tx may still refresh from committed state: {refreshed:?}"
    );
    let (_, group_after) = env.market_state();
    let short_after = env.portfolio_state(short);
    assert!(
        health_cert(&short_after).cert_oracle_epoch >= group_after.oracle_epoch,
        "stale target account becomes current"
    );
    assert_eq!(
        group_after.assets[1].oi_eff_short_q, short_oi_before,
        "out-of-order refresh must not liquidate the target position"
    );
    assert_eq!(
        active_leg_for_asset(&short_after, 1).basis_pos_q,
        active_leg_for_asset(&short_before, 1).basis_pos_q,
        "target leg size is unchanged by refresh-only progress"
    );
    assert_eq!(
        env.svm.get_account(&cranker).unwrap(),
        cranker_before,
        "refresh-only progress must not credit or rewrite the reward account"
    );
}

// user's economics, aggregate engine state, or custody.

#[test]
fn v16_program_auto_crank_refresh_not_blocked_by_unneeded_first_asset_oracle() {
    const MARK: u64 = 1_000_000;
    const OPEN_SLOT: u64 = 1;
    const REFRESH_SLOT: u64 = 2;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    set_test_clock(&mut env, OPEN_SLOT, 100);

    let stale_feed0 = [0x52u8; 32];
    let initial0 = env.set_pyth_price_with_conf(&stale_feed0, MARK as i64, -6, 0, 100);
    env.try_configure_hybrid_asset_with_conf_filter_cu(
        0,
        1,
        0,
        [stale_feed0, [0u8; 32], [0u8; 32]],
        &[initial0],
        OPEN_SLOT,
        100,
        0,
        0,
        1_000,
        0,
    )
    .expect("configure asset-0 hybrid oracle");
    env.configure_auth_mark_for_asset_as_admin(1, OPEN_SLOT, MARK);

    let owner_a = Keypair::new();
    let owner_b = Keypair::new();
    let account_a = env.create_portfolio(&owner_a);
    let account_b = env.create_portfolio(&owner_b);
    env.deposit(&owner_a, account_a, 10_000_000);
    env.deposit(&owner_b, account_b, 10_000_000);
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
    let a_before_update = env.portfolio_state(account_a);
    assert!(
        health_cert(&a_before_update).valid,
        "setup starts from a current multi-asset account"
    );
    assert_eq!(
        active_leg_for_asset(&a_before_update, 0).asset_index,
        0,
        "asset 0 is active"
    );
    assert_eq!(
        active_leg_for_asset(&a_before_update, 1).asset_index,
        1,
        "asset 1 is active"
    );

    set_test_clock(&mut env, REFRESH_SLOT, 200);
    env.push_auth_mark_for_asset_as_admin(1, REFRESH_SLOT, MARK + 10_000);
    env.crank(
        account_b,
        ProgInstruction::PermissionlessCrank {
            now_slot: REFRESH_SLOT,
            observations: crank_observations(1),
        },
    );
    let (_, group_after_asset1) = env.market_state();
    assert!(
        health_cert(&env.portfolio_state(account_a)).cert_oracle_epoch
            < group_after_asset1.oracle_epoch,
        "asset-1 progress makes account A stale"
    );

    env.svm.expire_blockhash();
    let stale_asset0_attempt = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: REFRESH_SLOT,
            observations: crank_observations_with_accounts(0, 1),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(account_a, false),
            AccountMeta::new_readonly(initial0, false),
        ],
        &[],
    );
    assert!(
        stale_asset0_attempt.is_err(),
        "asset-0 oracle is stale; the refresh must not depend on it"
    );

    env.svm.expire_blockhash();
    let refresh = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: REFRESH_SLOT,
            observations: vec![],
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(account_a, false),
        ],
        &[],
    );
    assert!(
        refresh.is_ok(),
        "account refresh should use already-current market state, not require an unneeded first-asset oracle: {refresh:?}"
    );
    assert!(
        health_cert(&env.portfolio_state(account_a)).cert_oracle_epoch
            >= env.market_state().1.oracle_epoch,
        "refresh makes the stale account current"
    );
}

// repeatedly landing no-observation refreshes first.
#[test]
fn v16_program_pending_selected_mark_requires_observation() {
    const MARK: u64 = 1_000_000;
    const NEXT_MARK0: u64 = 1_100_000;
    const NEXT_MARK1: u64 = 1_010_000;
    const OPEN_SLOT: u64 = 1;
    const CRANK_SLOT: u64 = 2;

    let mut env = V16CuEnv::new_with_market_params_and_price_move(2, 10_000, 10_000, 10_000);
    env.configure_auth_mark_for_asset_as_admin(0, OPEN_SLOT, MARK);
    env.configure_auth_mark_for_asset_as_admin(1, OPEN_SLOT, MARK);

    let owner_a = Keypair::new();
    let owner_b = Keypair::new();
    let asset1_owner = Keypair::new();
    let asset1_counter_owner = Keypair::new();
    let account_a = env.create_portfolio(&owner_a);
    let account_b = env.create_portfolio(&owner_b);
    let asset1_account = env.create_portfolio(&asset1_owner);
    let asset1_counter = env.create_portfolio(&asset1_counter_owner);
    env.deposit(&owner_a, account_a, 100_000_000);
    env.deposit(&owner_b, account_b, 100_000_000);
    env.deposit(&asset1_owner, asset1_account, 100_000_000);
    env.deposit(&asset1_counter_owner, asset1_counter, 100_000_000);
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
    env.svm.expire_blockhash();
    env.trade_asset_with_cu(
        1,
        &asset1_owner,
        asset1_account,
        &asset1_counter_owner,
        asset1_counter,
        POS_SCALE as i128,
        MARK,
        0,
    );
    let account_before_mark = env.portfolio_state(account_a);
    assert_eq!(
        leg(&account_before_mark, 0).asset_index,
        0,
        "asset 0 must occupy the first active slot for this selected-asset probe"
    );
    assert_eq!(
        active_leg_for_asset(&account_before_mark, 0).asset_index,
        0,
        "asset 0 is the engine-selected first active leg"
    );
    assert_eq!(active_leg_for_asset(&account_before_mark, 1).asset_index, 1);

    env.svm.warp_to_slot(CRANK_SLOT);
    env.push_auth_mark_for_asset_as_admin(0, CRANK_SLOT, NEXT_MARK0);
    let profile0 =
        state::read_asset_oracle_profile(&env.svm.get_account(&env.market).unwrap().data, 0)
            .unwrap();
    let (_, pending_group) = env.market_state();
    assert_eq!(profile0.mark_ewma_e6, NEXT_MARK0);
    assert_eq!(profile0.mark_ewma_last_slot, CRANK_SLOT);
    assert_eq!(
        pending_group.assets[0].effective_price, MARK,
        "PushAuthMark only stages the selected asset mark"
    );
    assert!(
        pending_group.assets[0].slot_last < CRANK_SLOT,
        "selected asset has a pending slot to consume"
    );

    env.push_auth_mark_for_asset_as_admin(1, CRANK_SLOT, NEXT_MARK1);
    env.crank(
        asset1_account,
        ProgInstruction::PermissionlessCrank {
            now_slot: CRANK_SLOT,
            observations: crank_observations(1),
        },
    );
    let (_, stale_group) = env.market_state();
    assert_eq!(stale_group.assets[1].effective_price, NEXT_MARK1);
    assert_eq!(
        stale_group.assets[0].effective_price, MARK,
        "selected asset mark is still pending after unrelated asset progress"
    );
    assert!(
        health_cert(&env.portfolio_state(account_a)).cert_oracle_epoch < stale_group.oracle_epoch,
        "unrelated asset progress makes the target account stale"
    );
    let market_before = env.svm.get_account(&env.market).unwrap();
    let account_before = env.svm.get_account(&account_a).unwrap();
    env.svm.expire_blockhash();
    let missing_selected_observation = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: CRANK_SLOT,
            observations: vec![],
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(account_a, false),
        ],
        &[],
    );
    assert!(
        missing_selected_observation.is_err(),
        "selected asset has a pending mark, so no-observation refresh must not consume its slot"
    );
    assert_eq!(
        env.svm.get_account(&env.market).unwrap(),
        market_before,
        "rejected missing-observation refresh must not advance the selected asset at the old price"
    );
    assert_eq!(
        env.svm.get_account(&account_a).unwrap(),
        account_before,
        "rejected missing-observation refresh must not certify the target against a pending mark"
    );

    env.svm.expire_blockhash();
    let observed = env.send(
        ProgInstruction::PermissionlessCrank {
            now_slot: CRANK_SLOT,
            observations: crank_observations(0),
        },
        vec![
            AccountMeta::new(env.payer.pubkey(), true),
            AccountMeta::new(env.market, false),
            AccountMeta::new(account_a, false),
        ],
        &[],
    );
    assert!(
        observed.is_ok(),
        "supplying the selected asset observation must remain live: {observed:?}"
    );
    let (_, observed_group) = env.market_state();
    assert_eq!(
        observed_group.assets[0].effective_price, NEXT_MARK0,
        "selected asset observation applies the pending mark"
    );
}
